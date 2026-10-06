//! A lit foreground on a wide frame: 143 blobs in the bottom-left corner, each brighter than
//! any star, as leaves or windows under streetlights are against a light-polluted sky. The
//! brightest 100 are then all foreground; spreading the picks over the frame
//! (`SolveOptions::spread_wide`, on by default) leaves the stars enough to solve. Narrow fields
//! keep the brightest.
use unisolver_core::*;
use unisolver_synth as synth;

const W: u32 = 1024;
const H: u32 = 768;
const RA: f64 = 150.0;
const DEC: f64 = 20.0;

fn in_foreground(c: &CentroidOut) -> bool {
    c.x < 300.0 && c.y >= 420.0
}

/// A 30° field (inside the 15–40° test tier) under an 11 × 13 grid of blobs 26 px apart, far
/// enough to stay separate blobs. The test sky's brightest star (magnitude 0.5) has a flux of
/// 200 000; the blobs 300 000–570 000.
fn frame() -> Frame {
    let q = synth::look_at(RA, DEC, 10.0);
    let mut img = synth::render(
        synth::test_db().star_catalog.stars(),
        &q,
        30.0,
        W,
        H,
        &synth::RenderParams::default(),
        3,
    );
    let (s2, r) = (2.0f32 * 1.5 * 1.5, 8i64);
    for gy in 0..13u32 {
        for gx in 0..11u32 {
            let px = 12.0 + gx as f32 * 26.0 + ((gx * 7 + gy * 3) % 4) as f32;
            let py = 432.0 + gy as f32 * 26.0 + ((gx * 5 + gy * 11) % 4) as f32;
            let flux = 300_000.0 * (1.0 + ((gx * 13 + gy * 17) % 10) as f32 / 10.0);
            let norm = flux / (std::f32::consts::PI * s2);
            let (ix, iy) = (px.round() as i64, py.round() as i64);
            for yy in (iy - r).max(0)..=(iy + r).min(H as i64 - 1) {
                for xx in (ix - r).max(0)..=(ix + r).min(W as i64 - 1) {
                    let (dx, dy) = (xx as f32 - px, yy as f32 - py);
                    img[yy as usize * W as usize + xx as usize] +=
                        norm * (-(dx * dx + dy * dy) / s2).exp();
                }
            }
        }
    }
    Frame {
        width: W,
        height: H,
        row_stride_bytes: None,
        pixels: PixelData::LumaF32(img),
    }
}

fn solver() -> Solver {
    Solver::from_file(&synth::test_db_file("unisolver_core_test.db")).unwrap()
}

#[test]
fn the_brightest_100_are_all_foreground() {
    let mut o = SolveOptions::new(30.0);
    o.spread_wide = false;
    let out = solver().solve(&frame(), &o).unwrap();
    assert!(!matches!(out.status, SolveStatus::Ok), "{:?}", out.status);
    assert_eq!(out.centroids.len(), 100);
    assert!(out.centroids.iter().all(in_foreground));
}

#[test]
fn spread_centroids_solve_past_the_foreground() {
    let out = solver().solve(&frame(), &SolveOptions::new(30.0)).unwrap();
    assert!(matches!(out.status, SolveStatus::Ok), "{:?}", out.status);
    let s = out.solution.unwrap();
    assert!(
        (s.ra_deg - RA).abs() * DEC.to_radians().cos() < 0.05 && (s.dec_deg - DEC).abs() < 0.05,
        "ra/dec=({}, {})",
        s.ra_deg,
        s.dec_deg
    );
    // The outcome carries the list the solve took: the foreground holds a share, not every slot
    let fg = out.centroids.iter().filter(|c| in_foreground(c)).count();
    assert!(fg < 40, "{fg} of {} in the foreground", out.centroids.len());
    // and every matched star is in the open sky
    assert!(s
        .matched
        .iter()
        .all(|m| !in_foreground(&out.centroids[m.centroid_index])));
}

#[test]
fn narrow_fields_keep_the_brightest() {
    // Below 10° the spread is off: the same frame at 9° (outside the test tier, so it does not
    // solve) reports the brightest 100, all of them foreground
    let mut o = SolveOptions::new(9.0);
    o.retry_alternate_profile = false;
    let out = solver().solve(&frame(), &o).unwrap();
    assert_eq!(out.centroids.len(), 100);
    assert!(out.centroids.iter().all(in_foreground));
}
