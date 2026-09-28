//! Single-frame lens fit. A phone's wide lens bends the edges of the frame by several pixels,
//! which a pinhole solve leaves as residual (and the annotations inherit). After a pinhole
//! solve with enough stars, fit the focal length and one radial term k1 about the image centre
//! to every matched star by linear least squares, re-solve in tracking mode with that lens,
//! and keep the result only when it fits the same stars better.
//!
//! The centre stays fixed and higher terms are left out on purpose. On 45 real phone frames a
//! free centre with k1–k3 and tangential terms (8 parameters) fitted the middle closely and
//! diverged at the edges, up to 50 px; k1 alone improved 28 of them by more than 10% and made
//! none worse. Distortion that is not radial about the centre needs a calibration over many
//! frames (`CalibrationSession`).
use crate::camera::{CameraParams, DistortionParams};
use crate::outcome::{geometry_from_solution, SolvedGeometry, Wcs};
use crate::solver::Extracted;
use std::collections::HashMap;
use tetra3::{Solution, SolveConfig, SolverDatabase};

/// Fewer matched stars do not pin down a distortion term
pub(crate) const MIN_MATCHES: u32 = 30;
/// Narrower fields bend by less than a pixel
pub(crate) const MIN_FOV_DEG: f32 = 20.0;
/// The fit is kept when the mean residual drops by at least this fraction
const MIN_GAIN: f64 = 0.05;

/// A matched star: its catalog position (degrees) and where it was detected (top-left pixels)
struct Pair {
    ra: f64,
    dec: f64,
    x: f64,
    y: f64,
}

fn pairs(db: &SolverDatabase, g: &SolvedGeometry) -> Vec<Pair> {
    let w = &g.wcs;
    let radius_deg =
        (w.width as f64).hypot(w.height as f64) / 2.0 * w.scale_arcsec_per_px() / 3600.0 * 1.2;
    let stars = db.star_catalog.stars();
    let by_id: HashMap<i64, (f64, f64)> = db
        .star_catalog
        .query_indices(
            g.ra_deg.to_radians() as f32,
            g.dec_deg.to_radians() as f32,
            radius_deg.to_radians() as f32,
        )
        .into_iter()
        .map(|i| {
            let s = &stars[i];
            (
                s.id,
                (
                    (s.ra_rad as f64).to_degrees(),
                    (s.dec_rad as f64).to_degrees(),
                ),
            )
        })
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
/// stars, over all of them and over those beyond half the half-diagonal (None when there are
/// none). None when the lens model cannot place a star.
fn residuals(pairs: &[Pair], wcs: &Wcs) -> Option<(f64, Option<f64>)> {
    let (cx, cy) = (wcs.width as f64 / 2.0, wcs.height as f64 / 2.0);
    let half = cx.hypot(cy);
    let (mut all, mut edge) = (Vec::new(), Vec::new());
    for p in pairs {
        let (px, py) = wcs.world_to_pixel(p.ra, p.dec)?;
        let d = (p.x - px).hypot(p.y - py);
        all.push(d);
        if (p.x - cx).hypot(p.y - cy) > half / 2.0 {
            edge.push(d);
        }
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    (!all.is_empty()).then(|| (mean(&all), (!edge.is_empty()).then(|| mean(&edge))))
}

/// Focal length and k1 about the principal point, by least squares on every pair: with `u` a
/// star's pinhole position in the tangent plane (pixel offset over focal length) and `d` its
/// detected offset from the centre, `d = a·u + b·|u|²·u`, so the focal length is `a` and
/// `k1 = b / a³` (the lens model's `r · (1 + k1·r²)` with r in pixels). None for a lens that
/// is not a pinhole, or a degenerate star layout.
fn fit_k1(pairs: &[Pair], wcs: &Wcs) -> Option<CameraParams> {
    let cam = &wcs.camera;
    if !matches!(cam.distortion, DistortionParams::None) {
        return None;
    }
    let (cx, cy) = cam.principal_point;
    let f0 = cam.focal_length_px;
    let (mut s11, mut s12, mut s22, mut t1, mut t2) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for p in pairs {
        let (px, py) = wcs.world_to_pixel(p.ra, p.dec)?;
        let (ux, uy) = ((px - cx) / f0, (py - cy) / f0);
        let r2 = ux * ux + uy * uy;
        let ud = ux * (p.x - cx) + uy * (p.y - cy);
        s11 += r2;
        s12 += r2 * r2;
        s22 += r2 * r2 * r2;
        t1 += ud;
        t2 += r2 * ud;
    }
    let det = s11 * s22 - s12 * s12;
    if det <= 1e-9 * s11 * s22 {
        return None;
    }
    let a = (t1 * s22 - t2 * s12) / det;
    let b = (s11 * t2 - s12 * t1) / det;
    (a.is_finite() && a > 0.0 && b.is_finite()).then(|| CameraParams {
        focal_length_px: a,
        principal_point: (cx, cy),
        parity_flip: cam.parity_flip,
        distortion: DistortionParams::Radial {
            k1: b / (a * a * a),
            k2: 0.0,
            k3: 0.0,
            p1: 0.0,
            p2: 0.0,
            center: None,
        },
    })
}

/// The pinhole solve `(sol, g)` of a wide frame, refined with a lens fitted to its own stars:
/// Some(refined) when the fitted lens tracks the frame and fits its matched stars better (mean
/// residual down by [`MIN_GAIN`], the edge not worse); None keeps the pinhole solve.
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
    let pairs = pairs(db, g);
    let (before, before_edge) = residuals(&pairs, &g.wcs)?;
    let camera = fit_k1(&pairs, &g.wcs)?;
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
    let mut g2 = geometry_from_solution(&refined, w, h, &ext.topleft);
    let (after, after_edge) = residuals(&pairs, &g2.wcs)?;
    let edge_ok = match (before_edge, after_edge) {
        (Some(b), Some(a)) => a <= b,
        _ => true,
    };
    (after <= before * (1.0 - MIN_GAIN) && edge_ok).then(|| {
        g2.lens_fitted = true;
        (refined, g2)
    })
}
