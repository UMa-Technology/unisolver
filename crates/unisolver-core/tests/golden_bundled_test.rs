//! Golden regression on the **bundled** wide-field database: synthetic frames rendered from
//! the real Gaia catalog (the same catalog the database is built from) must solve to the
//! pointing they were rendered at. Needs nothing outside the repository, so it runs on any
//! clean clone.
mod common;

use common::{bundled_w_db, repo_root};
use std::sync::OnceLock;
use unisolver_core::*;
use unisolver_synth::{look_at, radec_err_arcmin, render, RenderParams};

/// Reads a GDR3 catalog (b"GDR3", version u32, count u64, then 36-byte records:
/// source_id i64, ra f64, dec f64, mag f32, pmra f32, pmdec f32).
fn gaia_catalog() -> &'static [tetra3::Star] {
    static CAT: OnceLock<Vec<tetra3::Star>> = OnceLock::new();
    CAT.get_or_init(|| {
        let bytes =
            std::fs::read(repo_root().join("third_party/tetra3/data/gaia_merged.bin")).unwrap();
        assert_eq!(&bytes[0..4], b"GDR3");
        let count = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
        (0..count)
            .map(|i| {
                let r = &bytes[16 + i * 36..16 + (i + 1) * 36];
                tetra3::Star {
                    id: i64::from_le_bytes(r[0..8].try_into().unwrap()),
                    ra_rad: (f64::from_le_bytes(r[8..16].try_into().unwrap()) as f32).to_radians(),
                    dec_rad: (f64::from_le_bytes(r[16..24].try_into().unwrap()) as f32)
                        .to_radians(),
                    mag: f32::from_le_bytes(r[24..28].try_into().unwrap()),
                }
            })
            .collect()
    })
}

/// Renders a frame at (ra, dec) and solves it through the default photo path.
fn solve_at(fov: f32, ra: f64, dec: f64, w: u32, h: u32) -> SolvedGeometry {
    let q = look_at(ra, dec, 0.0);
    let img = render(gaia_catalog(), &q, fov, w, h, &RenderParams::default(), 30);
    let frame = Frame {
        width: w,
        height: h,
        row_stride_bytes: None,
        pixels: PixelData::LumaF32(img),
    };
    let solver = Solver::from_file(bundled_w_db().to_str().unwrap()).unwrap();
    let (out, attempts) = solver
        .solve_with_fov_presets(&frame, &SolveOptions::new(70.0), &aspect_ladder(w, h))
        .unwrap();
    assert!(
        matches!(out.status, SolveStatus::Ok),
        "fov {fov} at ({ra}, {dec}): {:?} after {attempts:?}",
        out.status
    );
    out.solution.unwrap()
}

#[test]
fn bundled_database_solves_real_sky_fields() {
    // (fov, ra, dec, width, height, pointing tolerance in arcmin)
    let cases = [
        // Very wide fields: the final refinement fixes rotation, not scale, so the error
        // grows with the field (≈ 0.1–0.2° at 60–75°)
        (73.3_f32, 250.07, -19.19, 1920, 1080, 15.0), // Scorpius, landscape phone frame
        (60.0, 10.0, 80.0, 1920, 1080, 15.0),         // near the north celestial pole
        (46.0, 83.8, -5.4, 1080, 1920, 3.0),          // Orion, portrait phone frame
        (20.0, 200.0, -75.0, 1600, 1200, 3.0),        // far south, 4:3
    ];
    for (fov, ra, dec, w, h, tol_arcmin) in cases {
        let g = solve_at(fov, ra, dec, w, h);
        let q = look_at(ra, dec, 0.0);
        let err = radec_err_arcmin(g.ra_deg, g.dec_deg, &q);
        assert!(
            err < tol_arcmin,
            "fov {fov} at ({ra}, {dec}): solved ({:.3}, {:.3}), {err:.1}′ off",
            g.ra_deg,
            g.dec_deg
        );
        assert!(
            (g.fov_deg - fov).abs() < 1.5,
            "fov {fov}: solved {:.2}",
            g.fov_deg
        );
    }
}
