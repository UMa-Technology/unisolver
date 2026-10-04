//! The bytes the index builder writes, pinned: a change that alters them must also change
//! `INDEX_BUILDER` (packages built before are then reported for a rebuild) and this hash.
use sha2::{Digest, Sha256};
use unisolver_starmatch::{INDEX_BUILDER, blind, catalog};

/// `INDEX_BUILDER` and the sha256 of the index built below (839 888 bytes)
const GOLDEN: (&str, &str) = (
    "seiza-0.19.2",
    "a3639fe8117213aaf3772f6b3494cfc6044f8add53f961a6a5115edf8828eceb",
);

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

/// A 3° cap around (80°, 30°): 400 stars per square degree, mag 4–14
fn sky(seed: u64) -> Vec<catalog::CatalogStar> {
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
            catalog::CatalogStar {
                ra,
                dec: z.clamp(-1.0, 1.0).asin().to_degrees(),
                mag,
            }
        })
        .collect()
}

#[test]
fn the_index_builder_writes_the_pinned_bytes() {
    let cat = catalog::MemoryCatalog::new(sky(7));
    let params = blind::BlindParams {
        index_mag_limit: 12.7,
        max_pattern_deg: 3.0,
        ..Default::default()
    };
    let path = std::env::temp_dir().join(format!("starmatch-golden-{}.idx", std::process::id()));
    blind::BlindIndex::build(&cat, &params)
        .write_to(&path)
        .unwrap();
    let sha = format!("{:x}", Sha256::digest(std::fs::read(&path).unwrap()));
    std::fs::remove_file(&path).ok();
    assert_eq!(
        (INDEX_BUILDER, sha.as_str()),
        GOLDEN,
        "the index bytes changed: change INDEX_BUILDER and the hash together"
    );
}
