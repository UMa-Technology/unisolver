//! Narrow-field engine on a synthetic sky: blind and hinted solves, the outcome fields,
//! refusals, deadlines and opening real index / star tile files.
use std::time::{Duration, Instant};
use unisolver_core::narrow::testkit::{centroids_for, synthetic_sky, write_package, Lcg};
use unisolver_core::narrow::{NarrowEngine, NarrowMode, NarrowRequest};
use unisolver_core::{CentroidOut, SolveStatus};

const W: u32 = 4000;
const H: u32 = 3000;
const CENTER: (f64, f64) = (212.4, -35.7);
/// 6″/px over 4000 px
const FOV: f64 = 6.0 * 4000.0 / 3600.0;

fn index_params() -> seiza::blind::BlindParams {
    seiza::blind::BlindParams {
        min_scale_arcsec_px: 1.0,
        max_scale_arcsec_px: 15.0,
        ..Default::default()
    }
}

fn engine() -> NarrowEngine {
    let sky = synthetic_sky(7);
    let index = seiza::blind::BlindIndex::build(&sky, &index_params());
    NarrowEngine::from_parts("synthetic", index, Box::new(sky)).unwrap()
}

fn scene() -> Vec<CentroidOut> {
    let truth = seiza::Wcs::from_center_scale_rotation(CENTER, (1999.5, 1499.5), 6.0, 74.0, false);
    centroids_for(&truth, &synthetic_sky(7), W, H, 11)
}

fn off_arcmin(ra: f64, dec: f64) -> f64 {
    let (r1, d1, r2, d2) = (
        ra.to_radians(),
        dec.to_radians(),
        CENTER.0.to_radians(),
        CENTER.1.to_radians(),
    );
    let h = ((d2 - d1) / 2.0).sin().powi(2) + d1.cos() * d2.cos() * ((r2 - r1) / 2.0).sin().powi(2);
    (2.0 * h.sqrt().asin()).to_degrees() * 60.0
}

fn blind_request() -> NarrowRequest {
    NarrowRequest {
        mode: NarrowMode::Blind {
            min_fov_deg: FOV * 0.8,
            max_fov_deg: FOV * 1.25,
        },
        deadline: None,
    }
}

#[test]
fn blind_solves_and_reports_like_tetra3() {
    let e = engine();
    let out = e.solve(&scene(), W, H, &blind_request());
    assert!(matches!(out.status, SolveStatus::Ok), "{:?}", out.status);
    let sol = out.solution.unwrap();
    let off = off_arcmin(sol.ra_deg, sol.dec_deg);
    assert!(off < 0.6, "centre off by {off}′");
    assert!(
        (sol.fov_deg as f64 - FOV).abs() / FOV < 0.01,
        "fov {}",
        sol.fov_deg
    );
    assert!(sol.num_matches as usize >= unisolver_core::narrow::BLIND_MIN_MATCHES);
    assert_eq!(sol.num_matches as usize, sol.matched.len());
    assert!(sol.prob <= unisolver_core::narrow::MAX_MISMATCH_PROB);
    // WCS round trip (top-left origin)
    let (ra, dec) = sol.wcs.pixel_to_world(1000.0, 700.0);
    let (x, y) = sol.wcs.world_to_pixel(ra, dec).unwrap();
    assert!((x - 1000.0).abs() < 0.01 && (y - 700.0).abs() < 0.01);
}

#[test]
fn hinted_solves_and_falls_back_to_blind() {
    let e = engine();
    let hinted = |ra: f64, dec: f64| NarrowRequest {
        mode: NarrowMode::Hinted {
            ra_deg: ra,
            dec_deg: dec,
            radius_deg: 2.0,
            fov_deg: FOV,
            fov_tolerance: 0.1,
        },
        deadline: None,
    };
    let near = e.solve(&scene(), W, H, &hinted(CENTER.0 + 0.5, CENTER.1 - 0.3));
    assert!(matches!(near.status, SolveStatus::Ok), "{:?}", near.status);
    // A hint 40° away cannot find the field, the blind fallback does
    let far = e.solve(&scene(), W, H, &hinted(CENTER.0 - 40.0, CENTER.1 + 20.0));
    assert!(matches!(far.status, SolveStatus::Ok), "{:?}", far.status);
    let sol = far.solution.unwrap();
    assert!(off_arcmin(sol.ra_deg, sol.dec_deg) < 0.6);
}

/// The hinted search scans its whole radius when the field is not near the hint: a hint 40° off
/// with a 60° radius gets part of the time, then the blind search solves within the deadline
#[test]
fn a_far_hint_with_a_wide_radius_leaves_time_for_the_blind_search() {
    let e = engine();
    let req = NarrowRequest {
        mode: NarrowMode::Hinted {
            ra_deg: CENTER.0 - 40.0,
            dec_deg: CENTER.1 + 20.0,
            radius_deg: 60.0,
            fov_deg: FOV,
            fov_tolerance: 0.1,
        },
        deadline: Some(Instant::now() + Duration::from_secs(6)),
    };
    let out = e.solve(&scene(), W, H, &req);
    assert!(
        matches!(out.status, SolveStatus::Ok),
        "{:?} after {} ms",
        out.status,
        out.solve_ms
    );
    let sol = out.solution.unwrap();
    assert!(off_arcmin(sol.ra_deg, sol.dec_deg) < 0.6);
    assert!(
        sol.num_matches as usize >= unisolver_core::narrow::BLIND_MIN_MATCHES,
        "a blind solution keeps the blind floor"
    );
}

#[test]
fn noise_does_not_solve_and_a_deadline_stops_the_search() {
    let e = engine();
    let mut rng = Lcg(5);
    let noise: Vec<CentroidOut> = (0..40)
        .map(|i| CentroidOut {
            x: rng.next() * W as f64,
            y: rng.next() * H as f64,
            mass: Some(1000.0 - i as f32),
            elongation: None,
        })
        .collect();
    let blind = || NarrowMode::Blind {
        min_fov_deg: 1.0,
        max_fov_deg: 10.0,
    };
    let out = e.solve(
        &noise,
        W,
        H,
        &NarrowRequest {
            mode: blind(),
            deadline: None,
        },
    );
    assert!(
        matches!(out.status, SolveStatus::NoMatch | SolveStatus::Timeout),
        "{:?}",
        out.status
    );
    assert!(out.solution.is_none());

    let started = Instant::now();
    let out = e.solve(
        &noise,
        W,
        H,
        &NarrowRequest {
            mode: blind(),
            deadline: Some(Instant::now() + Duration::from_millis(50)),
        },
    );
    assert!(
        matches!(out.status, SolveStatus::Timeout | SolveStatus::NoMatch),
        "{:?}",
        out.status
    );
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn too_few_centroids_are_refused_up_front() {
    let e = engine();
    let few: Vec<CentroidOut> = scene().into_iter().take(7).collect();
    let out = e.solve(&few, W, H, &blind_request());
    assert!(
        matches!(out.status, SolveStatus::TooFew),
        "{:?}",
        out.status
    );
}

#[test]
fn opens_an_index_and_star_tiles_from_files() {
    let dir = std::env::temp_dir().join(format!("unisolver-narrow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (idx, stars) = write_package(&synthetic_sky(7), (1.0, 15.0), &dir, "synthetic");
    let e = NarrowEngine::open(idx.to_str().unwrap(), stars.to_str().unwrap()).unwrap();
    assert_eq!(e.info().name, "synthetic");
    assert_eq!(e.info().num_stars, 129_000);
    assert!(
        (e.info().min_fov_deg - 0.6).abs() < 1e-6,
        "{}",
        e.info().min_fov_deg
    );
    let out = e.solve(&scene(), W, H, &blind_request());
    assert!(matches!(out.status, SolveStatus::Ok), "{:?}", out.status);
    std::fs::remove_dir_all(&dir).ok();
}
