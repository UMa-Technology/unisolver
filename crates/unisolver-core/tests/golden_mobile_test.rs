//! Golden regression on real phone photos with the bundled database. Needs the private
//! corpus testdata/private/mobile/; without it (a clean clone) it prints a skipped line
//! and passes. The public counterpart is golden_bundled_test.
mod common;

use common::{bundled_w_db, repo_root};
use unisolver_core::*;

#[test]
fn golden_set1_frame1_solves_to_scorpius() {
    let root = repo_root();
    let img = root.join("testdata/private/mobile/set1/frame_1_1781018520519.JPG");
    if !img.exists() {
        eprintln!("skipped: golden needs the private corpus of real photos");
        return;
    }
    let solver = Solver::from_file(bundled_w_db().to_str().unwrap()).unwrap();
    let dynimg = image::open(&img).unwrap().to_luma32f();
    let (w, h) = (dynimg.width(), dynimg.height());
    let frame = Frame {
        width: w,
        height: h,
        row_stride_bytes: None,
        pixels: PixelData::LumaF32(dynimg.into_raw()),
    };
    let mut o = SolveOptions::new(70.0);
    o.fov_max_error_deg = Some(9.0);
    o.timeout_ms = Some(10_000);
    let out = solver.solve(&frame, &o).unwrap();
    assert!(matches!(out.status, SolveStatus::Ok), "{:?}", out.status);
    let g = out.solution.unwrap();
    // Hand-checked truth: east of Antares in Scorpius, phone main camera, landscape
    assert!(
        (g.ra_deg - 250.07).abs() < 0.3 && (g.dec_deg + 19.19).abs() < 0.3,
        "ra/dec=({}, {})",
        g.ra_deg,
        g.dec_deg
    );
    assert!((g.fov_deg - 73.3).abs() < 1.0, "fov={}", g.fov_deg);
    assert!(g.num_matches >= 50, "matches={}", g.num_matches);
}
