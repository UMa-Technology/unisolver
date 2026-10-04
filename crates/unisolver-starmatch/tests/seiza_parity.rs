//! Parity with the vendored seiza this crate was ported from: the same index and tile bytes,
//! the same blind and hinted solutions. Solves run on one thread, so batch verification picks
//! the same hypothesis on both sides. Removed together with third_party/seiza; the index hash
//! it prints is pinned by tests/golden.rs.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use unisolver_starmatch as ours;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// (ra, dec, mag) of a 3° cap around (80°, 30°): 400 stars per square degree, mag 4–14
fn sky(seed: u64) -> Vec<(f64, f64, f32)> {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let mut rng = Lcg(seed);
    let depth = 1.0 - 3f64.to_radians().cos();
    let n = (TAU * depth * (180.0 / PI).powi(2) * 400.0) as usize;
    let (ra0, tilt) = (80f64.to_radians(), FRAC_PI_2 - 30f64.to_radians());
    (0..n)
        .map(|_| {
            let z = 1.0 - rng.next() * depth;
            let phi = rng.next() * TAU;
            let r = (1.0 - z * z).sqrt();
            let (x, y) = (r * phi.cos(), r * phi.sin());
            let (x, z) = (
                x * tilt.cos() + z * tilt.sin(),
                -x * tilt.sin() + z * tilt.cos(),
            );
            let ra = (y.atan2(x) + ra0).rem_euclid(TAU).to_degrees();
            let mag = 4.0 + rng.next() as f32 * 10.0;
            (ra, z.clamp(-1.0, 1.0).asin().to_degrees(), mag)
        })
        .collect()
}

fn catalogs(
    stars: &[(f64, f64, f32)],
) -> (ours::catalog::MemoryCatalog, seiza::catalog::MemoryCatalog) {
    let a = stars
        .iter()
        .map(|&(ra, dec, mag)| ours::catalog::CatalogStar { ra, dec, mag });
    let b = stars
        .iter()
        .map(|&(ra, dec, mag)| seiza::catalog::CatalogStar { ra, dec, mag });
    (
        ours::catalog::MemoryCatalog::new(a.collect()),
        seiza::catalog::MemoryCatalog::new(b.collect()),
    )
}

fn params_ours() -> ours::blind::BlindParams {
    ours::blind::BlindParams {
        index_mag_limit: 12.7,
        max_pattern_deg: 3.0,
        ..Default::default()
    }
}

fn params_seiza() -> seiza::blind::BlindParams {
    seiza::blind::BlindParams {
        index_mag_limit: 12.7,
        max_pattern_deg: 3.0,
        ..Default::default()
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("starmatch-parity-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn sha(path: &Path) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}

fn one_thread<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(f)
}

#[test]
fn index_bytes_match() {
    let (a, b) = catalogs(&sky(7));
    let (pa, pb) = (scratch("ours.idx"), scratch("seiza.idx"));
    ours::blind::BlindIndex::build(&a, &params_ours())
        .write_to(&pa)
        .unwrap();
    seiza::blind::BlindIndex::build(&b, &params_seiza())
        .write_to(&pb)
        .unwrap();
    let (ha, hb) = (sha(&pa), sha(&pb));
    println!(
        "index sha256 {ha} ({} bytes)",
        std::fs::metadata(&pa).unwrap().len()
    );
    assert_eq!(ha, hb);
}

#[test]
fn tile_bytes_match() {
    let stars = sky(11);
    let mut a = ours::catalog::TileSetBuilder::new(16, 2016.0, "parity");
    let mut b = seiza::catalog::TileSetBuilder::new(16, 2016.0, "parity");
    for &(ra, dec, mag) in &stars {
        a.add(ra, dec, mag);
        b.add(ra, dec, mag);
    }
    let (pa, pb) = (scratch("ours.bin"), scratch("seiza.bin"));
    a.write_to(&pa).unwrap();
    b.write_to(&pb).unwrap();
    assert_eq!(sha(&pa), sha(&pb));
}

/// Detections of `stars` through `truth`: 10 % missed, ±0.3 px jitter, flux from magnitude,
/// brightest first, as (x, y, flux)
fn detections(
    truth: &ours::Wcs,
    stars: &[(f64, f64, f32)],
    dims: (u32, u32),
    seed: u64,
) -> Vec<(f64, f64, f64)> {
    let mut rng = Lcg(seed);
    let mut out: Vec<(f64, f64, f64)> = stars
        .iter()
        .filter_map(|&(ra, dec, mag)| {
            let (x, y) = truth.world_to_pixel(ra, dec)?;
            let inside = x >= 0.0 && y >= 0.0 && x < dims.0 as f64 && y < dims.1 as f64;
            (inside && rng.next() >= 0.1).then(|| {
                let jx = (rng.next() - 0.5) * 0.6;
                let jy = (rng.next() - 0.5) * 0.6;
                (x + jx, y + jy, 10f64.powf(-0.4 * mag as f64) * 1e6)
            })
        })
        .collect();
    out.sort_by(|a, b| b.2.total_cmp(&a.2));
    out
}

fn centre(w: (f64, f64)) -> (u64, u64) {
    (w.0.to_bits(), w.1.to_bits())
}

#[test]
fn blind_and_hinted_solutions_match() {
    let stars = sky(7);
    let (a, b) = catalogs(&stars);
    let ia = ours::blind::BlindIndex::build(&a, &params_ours());
    let ib = seiza::blind::BlindIndex::build(&b, &params_seiza());
    let dims = (3000u32, 2000u32);
    let mid = ((dims.0 as f64 - 1.0) / 2.0, (dims.1 as f64 - 1.0) / 2.0);
    let mut rng = Lcg(23);
    let mut solved = 0;
    for k in 0..12u64 {
        let fov = 0.8 + rng.next() * 1.7;
        let (ra, dec) = (
            80.0 + (rng.next() - 0.5) * 2.0,
            30.0 + (rng.next() - 0.5) * 2.0,
        );
        let roll = rng.next() * 360.0;
        let scale = fov * 3600.0 / dims.0 as f64;
        let truth = ours::Wcs::from_center_scale_rotation((ra, dec), mid, scale, roll, false);
        let det = detections(&truth, &stars, dims, 100 + k);
        let da: Vec<ours::DetectedStar> = det
            .iter()
            .map(|&(x, y, f)| ours::DetectedStar {
                x,
                y,
                flux: f,
                peak: f as f32,
                area: 9,
            })
            .collect();
        let db: Vec<seiza::DetectedStar> = det
            .iter()
            .map(|&(x, y, f)| seiza::DetectedStar {
                x,
                y,
                flux: f,
                peak: f as f32,
                area: 9,
            })
            .collect();

        // Blind, the FOV known to ±10 %
        let pa = ours::blind::BlindParams {
            min_scale_arcsec_px: scale * 0.9,
            max_scale_arcsec_px: scale * 1.1,
            ..params_ours()
        };
        let pb = seiza::blind::BlindParams {
            min_scale_arcsec_px: scale * 0.9,
            max_scale_arcsec_px: scale * 1.1,
            ..params_seiza()
        };
        let x = one_thread(|| ours::blind::solve_blind_until(&da, &a, &ia, &pa, dims, None));
        let y = one_thread(|| seiza::blind::solve_blind_until(&db, &b, &ib, &pb, dims, None));
        match (&x, &y) {
            (Ok(s), Ok(t)) => {
                solved += 1;
                assert_eq!(s.matched_stars, t.matched_stars, "blind frame {k}");
                assert_eq!(
                    s.rms_arcsec.to_bits(),
                    t.rms_arcsec.to_bits(),
                    "blind frame {k}"
                );
                assert_eq!(
                    centre(s.wcs.pixel_to_world(mid.0, mid.1)),
                    centre(t.wcs.pixel_to_world(mid.0, mid.1)),
                    "blind frame {k}"
                );
            }
            (Err(e), Err(f)) => assert_eq!(e.to_string(), f.to_string(), "blind frame {k}"),
            _ => panic!(
                "blind frame {k}: ours ok={}, seiza ok={}",
                x.is_ok(),
                y.is_ok()
            ),
        }

        // Hinted, the pointing 0.3 FOV off, searching one FOV
        let ha = ours::solve::SolveHint {
            center: (ra + fov * 0.3, dec),
            radius_deg: fov,
            scale_arcsec_px: scale,
            scale_tolerance: 0.1,
            sip_order: 0,
        };
        let hb = seiza::solve::SolveHint {
            center: (ra + fov * 0.3, dec),
            radius_deg: fov,
            scale_arcsec_px: scale,
            scale_tolerance: 0.1,
            sip_order: 0,
        };
        let x = one_thread(|| ours::solve::solve_until(&da, &a, &ha, dims, None));
        let y = one_thread(|| seiza::solve::solve_until(&db, &b, &hb, dims, None));
        match (&x, &y) {
            (Ok(s), Ok(t)) => {
                assert_eq!(s.matched_stars, t.matched_stars, "hinted frame {k}");
                assert_eq!(
                    s.rms_arcsec.to_bits(),
                    t.rms_arcsec.to_bits(),
                    "hinted frame {k}"
                );
                assert_eq!(
                    centre(s.wcs.pixel_to_world(mid.0, mid.1)),
                    centre(t.wcs.pixel_to_world(mid.0, mid.1)),
                    "hinted frame {k}"
                );
            }
            (Err(e), Err(f)) => assert_eq!(e.to_string(), f.to_string(), "hinted frame {k}"),
            _ => panic!(
                "hinted frame {k}: ours ok={}, seiza ok={}",
                x.is_ok(),
                y.is_ok()
            ),
        }
    }
    assert!(
        solved >= 10,
        "only {solved}/12 blind frames solved: the check needs solvable frames"
    );
}
