//! Writes a tiny narrow-field package for the plugin's integration test: a synthetic 3° sky
//! (not catalog data), its blind index and star tiles. Usage: `mini_package <out dir>`. The test
//! fixtures are the two files compressed with `zstd -19`, and `narrow_mini.json` records the size
//! and sha256 of both forms of each.
use unisolver_starmatch::{blind, catalog};

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

fn main() {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("output directory"));
    std::fs::create_dir_all(&out).unwrap();
    let mut rng = Lcg(7);
    let depth = 1.0 - 3f64.to_radians().cos();
    let n = (TAU * depth * (180.0 / PI).powi(2) * 100.0) as usize;
    let (ra0, tilt) = (80f64.to_radians(), FRAC_PI_2 - 30f64.to_radians());
    let stars: Vec<catalog::CatalogStar> = (0..n)
        .map(|_| {
            let z = 1.0 - rng.next() * depth;
            let phi = rng.next() * TAU;
            let r = (1.0 - z * z).sqrt();
            let (x, y) = (r * phi.cos(), r * phi.sin());
            let (x, z) = (
                x * tilt.cos() + z * tilt.sin(),
                -x * tilt.sin() + z * tilt.cos(),
            );
            catalog::CatalogStar {
                ra: (y.atan2(x) + ra0).rem_euclid(TAU).to_degrees(),
                dec: z.clamp(-1.0, 1.0).asin().to_degrees(),
                mag: 4.0 + rng.next() as f32 * 8.0,
            }
        })
        .collect();
    let mut tiles = catalog::TileSetBuilder::new(16, 2016.0, "synthetic test sky");
    for s in &stars {
        tiles.add(s.ra, s.dec, s.mag);
    }
    tiles.write_to(&out.join("narrow_mini.stars")).unwrap();
    let params = blind::BlindParams {
        index_mag_limit: 12.0,
        max_pattern_deg: 3.0,
        ..Default::default()
    };
    blind::BlindIndex::build(&catalog::MemoryCatalog::new(stars), &params)
        .write_to(&out.join("narrow_mini.idx"))
        .unwrap();
    println!("wrote {n} stars");
}
