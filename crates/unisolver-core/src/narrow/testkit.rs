//! Synthetic sky and centroids for the narrow engine's tests (unit and integration). Not part
//! of the API.
use crate::outcome::CentroidOut;
use seiza::catalog::{CatalogStar, MemoryCatalog, StarCatalog};

/// Deterministic generator (same recurrence as seiza's tests)
pub struct Lcg(pub u64);

impl Lcg {
    #[allow(clippy::should_implement_trait)] // mirrors seiza's test helper
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
    truth: &seiza::Wcs,
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
