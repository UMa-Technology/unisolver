//! Synthetic sky and centroids for the narrow engine's tests (unit and integration). Not part
//! of the API.
use crate::outcome::CentroidOut;
use unisolver_starmatch::catalog::{CatalogStar, MemoryCatalog, StarCatalog};

/// Deterministic generator (same recurrence as unisolver-starmatch's tests)
pub struct Lcg(pub u64);

impl Lcg {
    #[allow(clippy::should_implement_trait)] // mirrors unisolver-starmatch's test helper
    pub fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// A whole sky of 9 000 stars brighter than mag 6.5 and 120 000 down to mag 12, uniform on the
/// sphere (about 3 per square degree): enough for 4–8° fields at a few arcseconds per pixel.
pub fn synthetic_sky(seed: u64) -> MemoryCatalog {
    let mut rng = Lcg(seed);
    let mut stars = Vec::with_capacity(129_000);
    for (n, lo, span) in [(9_000, 2.0f32, 4.5f32), (120_000, 6.5, 5.5)] {
        for _ in 0..n {
            stars.push(CatalogStar {
                ra: rng.next() * 360.0,
                dec: (2.0 * rng.next() - 1.0).asin().to_degrees(),
                mag: lo + rng.next() as f32 * span,
            });
        }
    }
    MemoryCatalog::new(stars)
}

/// Centroids of `sky` seen through `truth` on a `width`×`height` frame: 15 % missed, ±0.3 px
/// jitter, mass from magnitude, brightest first (as an extraction delivers them).
pub fn centroids_for(
    truth: &unisolver_starmatch::Wcs,
    sky: &MemoryCatalog,
    width: u32,
    height: u32,
    seed: u64,
) -> Vec<CentroidOut> {
    let mut rng = Lcg(seed);
    let mut out: Vec<CentroidOut> = sky
        .all_brighter_than(30.0)
        .into_iter()
        .filter_map(|s| {
            let (x, y) = truth.world_to_pixel(s.ra, s.dec)?;
            let inside = x >= 0.0 && y >= 0.0 && x < width as f64 && y < height as f64;
            (inside && rng.next() >= 0.15).then(|| CentroidOut {
                x: x + (rng.next() - 0.5) * 0.6,
                y: y + (rng.next() - 0.5) * 0.6,
                mass: Some(10f32.powf(-0.4 * s.mag) * 1e6),
                elongation: None,
            })
        })
        .collect();
    out.sort_by(|a, b| b.mass.unwrap_or(0.0).total_cmp(&a.mass.unwrap_or(0.0)));
    out
}

/// Stars of a sky cap: `radius_deg` around `center` (RA, Dec in degrees), `per_sq_deg` per
/// square degree, magnitudes 4–12.5 uniform. Dense enough for 1–3° frames, small enough to
/// index in a moment.
pub fn patch_sky(center: (f64, f64), radius_deg: f64, per_sq_deg: f64, seed: u64) -> MemoryCatalog {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let mut rng = Lcg(seed);
    let depth = 1.0 - radius_deg.to_radians().cos();
    let n = (TAU * depth * (180.0 / PI).powi(2) * per_sq_deg) as usize;
    let (ra0, tilt) = (center.0.to_radians(), FRAC_PI_2 - center.1.to_radians());
    let stars = (0..n)
        .map(|_| {
            // Uniform in a cap around +Z, tilted onto the centre's declination, turned to its RA
            let z = 1.0 - rng.next() * depth;
            let phi = rng.next() * TAU;
            let r = (1.0 - z * z).sqrt();
            let (x, y) = (r * phi.cos(), r * phi.sin());
            let (x1, z1) = (
                x * tilt.cos() + z * tilt.sin(),
                -x * tilt.sin() + z * tilt.cos(),
            );
            let (x2, y2) = (
                x1 * ra0.cos() - y * ra0.sin(),
                x1 * ra0.sin() + y * ra0.cos(),
            );
            CatalogStar {
                ra: y2.atan2(x2).to_degrees().rem_euclid(360.0),
                dec: z1.clamp(-1.0, 1.0).asin().to_degrees(),
                mag: 4.0 + rng.next() as f32 * 8.5,
            }
        })
        .collect();
    MemoryCatalog::new(stars)
}

/// Writes `sky` as a narrow-field package in `dir`: a blind index for pixel scales
/// `scale_arcsec_px` (″/px, min and max) as `<stem>.idx` and its star tiles as `<stem>.stars`.
/// Each file is written aside and renamed into place, so parallel tests never read half a file.
pub fn write_package(
    sky: &MemoryCatalog,
    scale_arcsec_px: (f64, f64),
    dir: &std::path::Path,
    stem: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let params = unisolver_starmatch::blind::BlindParams {
        min_scale_arcsec_px: scale_arcsec_px.0,
        max_scale_arcsec_px: scale_arcsec_px.1,
        ..Default::default()
    };
    let (idx, stars) = (
        dir.join(format!("{stem}.idx")),
        dir.join(format!("{stem}.stars")),
    );
    let aside = |p: &std::path::Path| {
        let mut name = p.as_os_str().to_owned();
        name.push(format!(".tmp{}", std::process::id()));
        std::path::PathBuf::from(name)
    };
    unisolver_starmatch::blind::BlindIndex::build(sky, &params)
        .write_to(&aside(&idx))
        .expect("write index");
    let mut tiles =
        unisolver_starmatch::catalog::TileSetBuilder::new(16, 2026.0, "synthetic test sky");
    for s in sky.all_brighter_than(30.0) {
        tiles.add(s.ra, s.dec, s.mag);
    }
    tiles.write_to(&aside(&stars)).expect("write star tiles");
    std::fs::rename(aside(&idx), &idx).expect("rename index");
    std::fs::rename(aside(&stars), &stars).expect("rename star tiles");
    (idx, stars)
}
