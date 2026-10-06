//! Scale refinement after a lost-in-space solve of a wide field. The upstream refinement keeps
//! the pixel scale its 4-star pattern match measured (on purpose: a wrong focal-length guess
//! must not bias it). On wide frames that scale is 1–3% off, which puts the stars at the edges
//! 5–7 px from their catalog positions while the solve still verifies: a synthetic 73.3°
//! Scorpius frame solved at 74.36°, 6.8 px off on average, and phone frames of one camera
//! solved anywhere between 72.7° and 75.6°.
//!
//! Find the scale that brings the brightest detections onto catalog stars (a chamfer distance,
//! which needs no matches: at a wrong scale the matches themselves are wrong at the edges; each
//! scale is tried with the shift that lines the stars up best, since a wrong-scale attitude
//! absorbs part of the error as one), re-solve in tracking mode at that scale (tracking keeps
//! the given focal length), then polish the focal length by least squares on the new matches.
//! Keep the result only when the brightest detections land at least 10% closer to catalog stars.
use crate::camera::{CameraParams, DistortionParams};
use crate::lens::{field_stars, track, FieldStar, MIN_FOV_DEG};
use crate::outcome::{SolvedGeometry, Wcs};
use crate::solver::Extracted;
use std::collections::HashMap;
use tetra3::{Solution, SolveConfig, SolverDatabase};

/// Brightest detections the scale is judged by
const DETECTIONS: usize = 40;
/// Brightest catalog stars they are compared with
const STARS: usize = 200;
/// A detection farther than this from every star counts as this far (noise, hot pixels)
const CAP_PX: f64 = 8.0;
/// Scales searched around the solved one (±6%), first in coarse steps, then in fine steps
/// around the best coarse one
const SEARCH: f64 = 0.06;
const COARSE: f64 = 0.002;
const FINE: f64 = 0.0005;
/// The result is kept when the chamfer distance drops by at least this fraction
const MIN_GAIN: f64 = 0.1;
/// Least-squares polish rounds after the search
const POLISH: usize = 3;

/// The brightest detections (top-left pixels)
fn detections(ext: &Extracted) -> Vec<[f64; 2]> {
    let mut d: Vec<_> = ext.topleft.iter().collect();
    d.sort_by(|a, b| b.mass.unwrap_or(0.0).total_cmp(&a.mass.unwrap_or(0.0)));
    d.iter().take(DETECTIONS).map(|c| [c.x, c.y]).collect()
}

/// The brightest stars through `wcs` that any searched scale could bring into the frame
fn projected(stars: &[FieldStar], wcs: &Wcs) -> Vec<[f64; 2]> {
    let (w, h) = (wcs.width as f64, wcs.height as f64);
    let m = SEARCH + 0.01;
    stars
        .iter()
        .filter_map(|s| wcs.world_to_pixel(s.ra, s.dec))
        .filter(|&(x, y)| x >= -m * w && x <= (1.0 + m) * w && y >= -m * h && y <= (1.0 + m) * h)
        .take(STARS)
        .map(|(x, y)| [x, y])
        .collect()
}

/// Squared distance and offset from `d` to its nearest star
fn nearest(stars: &[[f64; 2]], d: &[f64; 2]) -> (f64, [f64; 2]) {
    stars.iter().fold((f64::INFINITY, [0.0; 2]), |best, p| {
        let o = [p[0] - d[0], p[1] - d[1]];
        let d2 = o[0] * o[0] + o[1] * o[1];
        if d2 < best.0 {
            (d2, o)
        } else {
            best
        }
    })
}

/// Mean distance (pixels, capped) from each detection to its nearest star
fn chamfer(stars: &[[f64; 2]], dets: &[[f64; 2]]) -> f64 {
    let total: f64 = dets
        .iter()
        .map(|d| nearest(stars, d).0.sqrt().min(CAP_PX))
        .sum();
    total / dets.len() as f64
}

/// The chamfer distance with the stars scaled by `s` about `c`, then shifted by the median
/// offset from the detections to their nearest stars (twice, re-pairing after the first
/// shift). A wrong-scale solve's attitude absorbs part of the scale error as a shift, so the
/// right scale about the principal point lines the stars up only together with a shift.
fn scaled_chamfer(stars: &[[f64; 2]], dets: &[[f64; 2]], c: (f64, f64), s: f64) -> f64 {
    let mut scaled: Vec<[f64; 2]> = stars
        .iter()
        .map(|p| [c.0 + s * (p[0] - c.0), c.1 + s * (p[1] - c.1)])
        .collect();
    for _ in 0..2 {
        let (mut dx, mut dy): (Vec<f64>, Vec<f64>) = dets
            .iter()
            .map(|d| {
                let o = nearest(&scaled, d).1;
                (o[0], o[1])
            })
            .unzip();
        let t = [median(&mut dx), median(&mut dy)];
        for p in &mut scaled {
            p[0] -= t[0];
            p[1] -= t[1];
        }
    }
    chamfer(&scaled, dets)
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// Focal length that best fits the matched stars through a pinhole about the principal point:
/// with `u` a star's pinhole position (pixel offset over focal length) and `d` its detected
/// offset, `d ≈ f·u`, so `f = Σ u·d / Σ |u|²`
fn lsq_focal(stars: &[FieldStar], g: &SolvedGeometry) -> Option<f64> {
    let by_id: HashMap<i64, (f64, f64)> = stars.iter().map(|s| (s.id, (s.ra, s.dec))).collect();
    let cam = &g.wcs.camera;
    let (cx, cy) = cam.principal_point;
    let f0 = cam.focal_length_px;
    let (mut num, mut den) = (0.0, 0.0);
    for m in &g.matched {
        let Some(&(ra, dec)) = by_id.get(&m.catalog_id) else {
            continue;
        };
        let Some((px, py)) = g.wcs.world_to_pixel(ra, dec) else {
            continue;
        };
        let (ux, uy) = ((px - cx) / f0, (py - cy) / f0);
        num += ux * (m.x - cx) + uy * (m.y - cy);
        den += ux * ux + uy * uy;
    }
    let f = num / den;
    (den > 0.0 && f.is_finite() && f > 0.0).then_some(f)
}

fn pinhole(cam: &CameraParams, focal_length_px: f64) -> CameraParams {
    CameraParams {
        focal_length_px,
        distortion: DistortionParams::None,
        ..cam.clone()
    }
}

/// The lost-in-space solve `(sol, g)` at the scale its brightest detections fit best: Some
/// when that brings them at least 10% closer to catalog stars, None keeps the solve as it is.
/// Wide fields only, as for the lens fit: narrow ones measure their scale well, and on 16
/// telescope and live-camera frames (2–8°) the refinement moved nothing by more than a
/// pixel, in either direction.
pub(crate) fn refine(
    db: &SolverDatabase,
    ext: &Extracted,
    cfg: &SolveConfig,
    w: u32,
    h: u32,
    sol: &Solution,
    g: &SolvedGeometry,
) -> Option<(Solution, SolvedGeometry)> {
    if g.fov_deg < MIN_FOV_DEG || !matches!(g.wcs.camera.distortion, DistortionParams::None) {
        return None;
    }
    let dets = detections(ext);
    let stars = field_stars(db, g);
    let p0 = projected(&stars, &g.wcs);
    if dets.len() < 10 || p0.len() < 10 {
        return None;
    }
    let judge = |g: &SolvedGeometry| chamfer(&projected(&stars, &g.wcs), &dets);
    let c0 = chamfer(&p0, &dets);
    let pp = g.wcs.camera.principal_point;
    let search = |from: f64, step: f64, n: i32| {
        (0..=n)
            .map(|k| from + k as f64 * step)
            .map(|s| (s, scaled_chamfer(&p0, &dets, pp, s)))
            .fold((1.0, f64::INFINITY), |a, b| if b.1 < a.1 { b } else { a })
    };
    let (coarse, _) = search(1.0 - SEARCH, COARSE, (2.0 * SEARCH / COARSE).round() as i32);
    let (best_s, best_c) = search(coarse - COARSE, FINE, (2.0 * COARSE / FINE).round() as i32);

    let mut best: Option<(Solution, SolvedGeometry)> = None;
    let mut cur_c = c0;
    if best_c < (1.0 - MIN_GAIN) * c0 {
        let cam = pinhole(&g.wcs.camera, g.wcs.camera.focal_length_px * best_s);
        if let Some(r) = track(db, ext, cfg, w, h, sol, &cam) {
            let c = judge(&r.1);
            if c < cur_c {
                cur_c = c;
                best = Some(r);
            }
        }
    }
    for _ in 0..POLISH {
        let (from_sol, from_g) = match &best {
            Some((s, g)) => (s, g),
            None => (sol, g),
        };
        let Some(f) = lsq_focal(&stars, from_g) else {
            break;
        };
        let cam = pinhole(&from_g.wcs.camera, f);
        let Some(r) = track(db, ext, cfg, w, h, from_sol, &cam) else {
            break;
        };
        let c = judge(&r.1);
        if c < cur_c - 0.01 {
            cur_c = c;
            best = Some(r);
        } else {
            break;
        }
    }
    let (s2, mut g2) = best?;
    (cur_c <= (1.0 - MIN_GAIN) * c0).then(|| {
        g2.scale_refined = true;
        (s2, g2)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outcome::geometry_from_solution;
    use crate::solver::{build_pool, build_solve_config, extract_frame, SolveOptions};
    use crate::{Frame, PixelData};
    use unisolver_synth as synth;

    /// A synthetic 1024×768 frame of `fov` degrees, extracted and solved lost-in-space, and the
    /// same solve re-tracked 3% off in scale (as the upstream refinement leaves some wide frames)
    fn solved(
        fov: f32,
    ) -> (
        Extracted,
        SolveConfig,
        Solution,
        SolvedGeometry,
        (Solution, SolvedGeometry),
    ) {
        let db = synth::test_db();
        let (w, h) = (1024, 768);
        let img = synth::render(
            db.star_catalog.stars(),
            &synth::look_at(210.0, -20.0, 25.0),
            fov,
            w,
            h,
            &Default::default(),
            9,
        );
        let frame = Frame {
            width: w,
            height: h,
            row_stride_bytes: None,
            pixels: PixelData::LumaF32(img),
        };
        let opts = SolveOptions::new(fov);
        let ext = extract_frame(&frame, &opts.extraction.resolve(), &build_pool().unwrap())
            .unwrap()
            .brightest;
        let cfg = build_solve_config(&opts, w, h).unwrap();
        let sol = db.solve_from_centroids(&ext.centroids, &cfg).unwrap();
        let g = geometry_from_solution(&sol, w, h, &ext.topleft);
        let wrong = pinhole(&g.wcs.camera, g.wcs.camera.focal_length_px * 1.03);
        let off = track(db, &ext, &cfg, w, h, &sol, &wrong).expect("tracks");
        assert!(
            (off.1.fov_deg / fov - 1.0).abs() > 0.02,
            "start {}",
            off.1.fov_deg
        );
        (ext, cfg, sol, g, off)
    }

    /// A wide solve 3% off in scale comes back to the rendered scale; a right one stays right.
    #[test]
    fn a_wrong_scale_comes_back() {
        let db = synth::test_db();
        let fov = 35.0;
        let (ext, cfg, sol, g, (sol3, g3)) = solved(fov);
        let close = |g: &SolvedGeometry| (g.fov_deg / fov - 1.0).abs() < 0.002;

        let (_, g2) = refine(db, &ext, &cfg, 1024, 768, &sol3, &g3).expect("refined");
        assert!(g2.scale_refined);
        assert!(close(&g2), "refined to {} from {}", g2.fov_deg, g3.fov_deg);

        match refine(db, &ext, &cfg, 1024, 768, &sol, &g) {
            Some((_, g4)) => assert!(close(&g4), "{} from {}", g4.fov_deg, g.fov_deg),
            None => assert!(close(&g), "kept {}", g.fov_deg),
        }
    }

    /// Narrow fields keep the scale they solved at, even a wrong one: they measure it well, and
    /// the refinement is for wide fields only.
    #[test]
    fn narrow_fields_keep_their_scale() {
        let (ext, cfg, _, _, (sol3, g3)) = solved(16.0);
        assert!(refine(synth::test_db(), &ext, &cfg, 1024, 768, &sol3, &g3).is_none());
    }
}
