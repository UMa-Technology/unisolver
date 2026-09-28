//! Single-frame lens fit. A phone's wide lens bends the edges of the frame by several pixels,
//! which a pinhole solve leaves as residual (and the annotations inherit). After a pinhole
//! solve with enough stars, fit the focal length and the radial term k1 about the image centre
//! to every matched star by linear least squares, and k2 as well when it predicts left-out stars
//! better; re-solve in tracking mode with that lens, and keep the result only when it fits the
//! same stars better. A second round refits on the stars the refined solve matched, which reach
//! further out: at the edges, where the lens bends most, a pinhole solve misses stars once its
//! match radius tightens.
//!
//! The centre stays fixed and other terms are left out on purpose. On 45 real phone frames a
//! free centre with k1–k3 and tangential terms (8 parameters) fitted the middle closely and
//! diverged at the edges, up to 50 px; k1 alone improved 28 of them by more than 10% and made
//! none worse. k2 predicts left-out stars 11% better than k1 alone, but past the last matched
//! star its r⁵ term bends fast: on frames whose few stars reached only 60% of the way to the
//! corners it put them 45–85 px off and folded the lens back. So k2 is used only when the fit
//! pins the corners down, and any fitted lens must stay monotonic out to them. Distortion that
//! is not radial about the centre needs a calibration over many frames (`CalibrationSession`).
use crate::camera::{CameraParams, DistortionParams};
use crate::outcome::{geometry_from_solution, SolvedGeometry, Wcs};
use crate::solver::Extracted;
use std::collections::HashMap;
use tetra3::{Solution, SolveConfig, SolverDatabase};

/// Fewer matched stars do not pin down a distortion term
pub(crate) const MIN_MATCHES: u32 = 30;
/// Narrower fields bend by less than a pixel (and measure their scale well: the scale
/// refinement stops here too)
pub(crate) const MIN_FOV_DEG: f32 = 20.0;
/// The fit is kept when the mean residual drops by at least this fraction
const MIN_GAIN: f64 = 0.05;
/// k2 joins k1 only when it lowers the leave-one-out residual by at least this fraction...
const K2_GAIN: f64 = 0.05;
/// ...and the fit pins the corners down: the standard error of its displacement there (pixels)
/// stays below this. Past the last matched star the r⁵ term is extrapolated, and it bends fast.
const K2_CORNER_SE_PX: f64 = 3.0;
/// Out to the corners the fitted radial map keeps at least this slope (a lens does not fold
/// its image back)
const MIN_SLOPE: f64 = 0.8;
/// Fit rounds: the second fits the stars the first lens let the solve match. At the edges,
/// where the lens bends most, a pinhole solve misses stars once its match radius is tight.
const ROUNDS: usize = 2;

/// A catalog star in or near the frame: id, position (degrees) and magnitude
pub(crate) struct FieldStar {
    pub id: i64,
    pub ra: f64,
    pub dec: f64,
    pub mag: f32,
}

/// Catalog stars within 1.2× the frame's circumscribed circle, brightest first
pub(crate) fn field_stars(db: &SolverDatabase, g: &SolvedGeometry) -> Vec<FieldStar> {
    let w = &g.wcs;
    let radius_deg =
        (w.width as f64).hypot(w.height as f64) / 2.0 * w.scale_arcsec_per_px() / 3600.0 * 1.2;
    let stars = db.star_catalog.stars();
    let mut out: Vec<FieldStar> = db
        .star_catalog
        .query_indices(
            g.ra_deg.to_radians() as f32,
            g.dec_deg.to_radians() as f32,
            radius_deg.to_radians() as f32,
        )
        .into_iter()
        .map(|i| {
            let s = &stars[i];
            FieldStar {
                id: s.id,
                ra: (s.ra_rad as f64).to_degrees(),
                dec: (s.dec_rad as f64).to_degrees(),
                mag: s.mag,
            }
        })
        .collect();
    out.sort_by(|a, b| a.mag.total_cmp(&b.mag));
    out
}

/// Re-solves the extracted frame in tracking mode from `sol`'s attitude with `camera` (tracking
/// keeps the camera's focal length), under the same limits as `cfg`.
pub(crate) fn track(
    db: &SolverDatabase,
    ext: &Extracted,
    cfg: &SolveConfig,
    w: u32,
    h: u32,
    sol: &Solution,
    camera: &CameraParams,
) -> Option<(Solution, SolvedGeometry)> {
    let tracking = SolveConfig {
        attitude_hint: Some(sol.qicrs2cam),
        hint_uncertainty_rad: 1f32.to_radians(),
        strict_hint: false,
        match_threshold: cfg.match_threshold,
        solve_timeout_ms: cfg.solve_timeout_ms,
        observer_velocity_km_s: cfg.observer_velocity_km_s,
        pattern_checking_stars: cfg.pattern_checking_stars,
        ..SolveConfig::with_camera_model(camera.to_tetra3(w, h).ok()?)
    };
    let refined = db.solve_from_centroids(&ext.centroids, &tracking).ok()?;
    let g = geometry_from_solution(&refined, w, h, &ext.topleft);
    Some((refined, g))
}

/// A matched star: its catalog position (degrees) and where it was detected (top-left pixels)
struct Pair {
    ra: f64,
    dec: f64,
    x: f64,
    y: f64,
}

fn pairs(db: &SolverDatabase, g: &SolvedGeometry) -> Vec<Pair> {
    let by_id: HashMap<i64, (f64, f64)> = field_stars(db, g)
        .into_iter()
        .map(|s| (s.id, (s.ra, s.dec)))
        .collect();
    g.matched
        .iter()
        .filter_map(|m| {
            by_id.get(&m.catalog_id).map(|&(ra, dec)| Pair {
                ra,
                dec,
                x: m.x,
                y: m.y,
            })
        })
        .collect()
}

/// Mean distance (pixels) between where the pairs were detected and where `wcs` puts their
/// stars. None without pairs, or when the lens model cannot place a star.
fn residual(pairs: &[Pair], wcs: &Wcs) -> Option<f64> {
    let mut total = 0.0;
    for p in pairs {
        let (px, py) = wcs.world_to_pixel(p.ra, p.dec)?;
        total += (p.x - px).hypot(p.y - py);
    }
    (!pairs.is_empty()).then(|| total / pairs.len() as f64)
}

/// A radial lens fitted to the pairs, with its mean leave-one-out residual and the standard
/// error of its displacement at the corners (pixels)
struct RadialFit {
    camera: CameraParams,
    k1: f64,
    k2: f64,
    loo: f64,
    corner_se: f64,
}

/// Focal length and `terms` radial coefficients (1: k1; 2: k1 and k2) about the principal
/// point, by least squares on every pair: with `u` a star's pinhole position in the tangent
/// plane (pixel offset over focal length) and `d` its detected offset from the centre,
/// `d = a·u + b·|u|²·u + c·|u|⁴·u`, so the focal length is `a`, `k1 = b / a³` and `k2 = c / a⁵`
/// (the lens model's `r · (1 + k1·r² + k2·r⁴)` with r in pixels). The leave-one-out residual
/// comes from the hat matrix, each star's x and y rows left out together; the corner's standard
/// error from the residual variance and the parameters' covariance. None for a lens that is not
/// a pinhole, or a degenerate star layout.
fn fit_radial(pairs: &[Pair], wcs: &Wcs, terms: usize, corner: f64) -> Option<RadialFit> {
    let cam = &wcs.camera;
    if !matches!(cam.distortion, DistortionParams::None) || pairs.is_empty() {
        return None;
    }
    let (cx, cy) = cam.principal_point;
    let f0 = cam.focal_length_px;
    let n = terms + 1;
    // Per pair: the basis for its x and y rows, and its detected offset
    let mut rows = Vec::with_capacity(pairs.len());
    for p in pairs {
        let (px, py) = wcs.world_to_pixel(p.ra, p.dec)?;
        let (ux, uy) = ((px - cx) / f0, (py - cy) / f0);
        let r2 = ux * ux + uy * uy;
        let powers = [1.0, r2, r2 * r2];
        let bx: Vec<f64> = powers[..n].iter().map(|s| ux * s).collect();
        let by: Vec<f64> = powers[..n].iter().map(|s| uy * s).collect();
        rows.push((bx, by, p.x - cx, p.y - cy));
    }
    let mut normal = vec![vec![0.0; n]; n];
    let mut rhs = vec![0.0; n];
    for (bx, by, dx, dy) in &rows {
        for (i, (row, r)) in normal.iter_mut().zip(rhs.iter_mut()).enumerate() {
            *r += bx[i] * dx + by[i] * dy;
            for (j, v) in row.iter_mut().enumerate() {
                *v += bx[i] * bx[j] + by[i] * by[j];
            }
        }
    }
    let m = invert(&normal)?;
    let beta: Vec<f64> = m.iter().map(|row| dot(row, &rhs)).collect();
    // `b1ᵀ M b2`
    let form =
        |b1: &[f64], b2: &[f64]| -> f64 { m.iter().zip(b1).map(|(row, x)| x * dot(row, b2)).sum() };
    let (mut loo, mut rss) = (0.0, 0.0);
    for (bx, by, dx, dy) in &rows {
        let (ex, ey) = (dx - dot(bx, &beta), dy - dot(by, &beta));
        rss += ex * ex + ey * ey;
        // Left out, the pair's residual is (I − H)⁻¹ e, H its 2×2 block of the hat matrix
        let (a11, a12, a22) = (1.0 - form(bx, bx), -form(bx, by), 1.0 - form(by, by));
        let det = a11 * a22 - a12 * a12;
        if det.abs() < 1e-12 {
            return None;
        }
        loo += ((a22 * ex - a12 * ey) / det).hypot((a11 * ey - a12 * ex) / det);
    }
    // The radial displacement at the corners is `bᵀβ` with b = (u, u³, u⁵) at u = corner / f
    let dof = (2 * rows.len()).saturating_sub(n).max(1) as f64;
    let uc = corner / f0;
    let bc: Vec<f64> = (0..n).map(|k| uc.powi(2 * k as i32 + 1)).collect();
    let corner_se = (rss / dof * form(&bc, &bc)).sqrt();
    let a = beta[0];
    let k1 = beta[1] / a.powi(3);
    let k2 = beta.get(2).map_or(0.0, |c| c / a.powi(5));
    (a.is_finite() && a > 0.0 && k1.is_finite() && k2.is_finite()).then(|| RadialFit {
        camera: CameraParams {
            focal_length_px: a,
            principal_point: (cx, cy),
            parity_flip: cam.parity_flip,
            distortion: DistortionParams::Radial {
                k1,
                k2,
                k3: 0.0,
                p1: 0.0,
                p2: 0.0,
                center: None,
            },
        },
        k1,
        k2,
        loo: loo / rows.len() as f64,
        corner_se,
    })
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Inverse of a small symmetric positive-definite matrix (Gauss–Jordan); None when a pivot
/// all but vanishes against its diagonal entry (a degenerate star layout)
fn invert(a: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    let mut m = a.to_vec();
    let mut inv: Vec<Vec<f64>> = (0..n)
        .map(|i| (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect())
        .collect();
    for c in 0..n {
        let d = m[c][c];
        if d.is_nan() || d <= 1e-9 * a[c][c] {
            return None;
        }
        m[c].iter_mut().for_each(|v| *v /= d);
        inv[c].iter_mut().for_each(|v| *v /= d);
        let (mc, ic) = (m[c].clone(), inv[c].clone());
        for r in (0..n).filter(|&r| r != c) {
            let f = m[r][c];
            m[r].iter_mut().zip(&mc).for_each(|(v, p)| *v -= f * p);
            inv[r].iter_mut().zip(&ic).for_each(|(v, p)| *v -= f * p);
        }
    }
    Some(inv)
}

/// Whether the radial map `r · (1 + k1·r² + k2·r⁴)` keeps a slope of at least [`MIN_SLOPE`]
/// from the centre out to radius `reach` (pixels)
fn monotonic(k1: f64, k2: f64, reach: f64) -> bool {
    (0..=64).all(|i| {
        let r2 = (reach * i as f64 / 64.0).powi(2);
        1.0 + 3.0 * k1 * r2 + 5.0 * k2 * r2 * r2 >= MIN_SLOPE
    })
}

/// k1 alone, or k1 and k2 when they predict left-out stars better and pin the corners down;
/// None when the chosen lens is not monotonic out to the corners
fn fit_lens(pairs: &[Pair], wcs: &Wcs) -> Option<CameraParams> {
    let (cx, cy) = wcs.camera.principal_point;
    let corner = [
        (0.0, 0.0),
        (wcs.width as f64, 0.0),
        (0.0, wcs.height as f64),
        (wcs.width as f64, wcs.height as f64),
    ]
    .iter()
    .map(|&(x, y)| (x - cx).hypot(y - cy))
    .fold(0.0, f64::max);
    let k1 = fit_radial(pairs, wcs, 1, corner)?;
    let chosen = match fit_radial(pairs, wcs, 2, corner) {
        Some(k12)
            if k12.loo <= k1.loo * (1.0 - K2_GAIN)
                && k12.corner_se <= K2_CORNER_SE_PX
                && monotonic(k12.k1, k12.k2, corner) =>
        {
            k12
        }
        _ => k1,
    };
    monotonic(chosen.k1, chosen.k2, corner).then_some(chosen.camera)
}

/// The pinhole solve `(sol, g)` of a wide frame, refined with a lens fitted to its own stars:
/// Some(refined) when the fitted lens tracks the frame and fits its matched stars better (mean
/// residual down by [`MIN_GAIN`]); None keeps the pinhole solve. A second round refits on the
/// refined solve's matches, kept only when it does better again.
pub(crate) fn refine(
    db: &SolverDatabase,
    ext: &Extracted,
    cfg: &SolveConfig,
    w: u32,
    h: u32,
    sol: &Solution,
    g: &SolvedGeometry,
) -> Option<(Solution, SolvedGeometry)> {
    if g.num_matches < MIN_MATCHES || g.fov_deg < MIN_FOV_DEG {
        return None;
    }
    let mut best: Option<(Solution, SolvedGeometry)> = None;
    for _ in 0..ROUNDS {
        let (from_sol, from_g) = best.as_ref().map_or((sol, g), |(s, g)| (s, g));
        let pairs = pairs(db, from_g);
        // The lens is fitted afresh against the same attitude, focal length and centre
        let pinhole = Wcs {
            camera: CameraParams {
                distortion: DistortionParams::None,
                ..from_g.wcs.camera.clone()
            },
            ..from_g.wcs.clone()
        };
        let Some(before) = residual(&pairs, &from_g.wcs) else {
            break;
        };
        let Some(camera) = fit_lens(&pairs, &pinhole) else {
            break;
        };
        let Some((refined, mut g2)) = track(db, ext, cfg, w, h, from_sol, &camera) else {
            break;
        };
        match residual(&pairs, &g2.wcs) {
            Some(after) if after <= before * (1.0 - MIN_GAIN) => {
                g2.scale_refined = g.scale_refined;
                g2.lens_fitted = true;
                best = Some((refined, g2));
            }
            _ => break,
        }
    }
    best
}
