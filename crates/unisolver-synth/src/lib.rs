//! Synthetic star fields for hermetic testing: random sky, attitudes,
//! ideal centroids, and rendered images.
use numeris::{Matrix3, Quaternion, Vector3};
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use rand_distr::{Distribution, Normal};
use std::sync::{Mutex, OnceLock};
use tetra3::{Centroid, GenerateDatabaseConfig, SolverDatabase, Star};

pub mod exif;

pub fn random_sky(n: usize, seed: u64, mag_min: f32, mag_max: f32) -> Vec<Star> {
    let mut rng = StdRng::seed_from_u64(seed);
    (0..n)
        .map(|i| {
            let z: f32 = rng.random_range(-1.0..1.0);
            let ra: f32 = rng.random_range(0.0..std::f32::consts::TAU);
            Star {
                id: i as i64 + 1,
                ra_rad: ra,
                dec_rad: z.asin(),
                mag: rng.random_range(mag_min..mag_max),
            }
        })
        .collect()
}

/// camera_vec = q * icrs_vec; +Z (boresight) points at (ra, dec); roll about the boresight (degrees, counter-clockwise).
pub fn look_at(ra_deg: f64, dec_deg: f64, roll_deg: f64) -> Quaternion<f32> {
    let (ra, dec) = (ra_deg.to_radians() as f32, dec_deg.to_radians() as f32);
    let bore = Vector3::from_array([dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()]);
    // Celestial north is up; near the poles use the x axis to avoid degeneracy
    let up = if dec_deg.abs() > 89.0 {
        Vector3::from_array([1.0, 0.0, 0.0])
    } else {
        Vector3::from_array([0.0, 0.0, 1.0])
    };
    let cam_z = bore.normalize();
    let cam_x = up.cross(&cam_z).normalize();
    let cam_y = cam_z.cross(&cam_x);
    let rot = Matrix3::new([
        [cam_x[0], cam_x[1], cam_x[2]],
        [cam_y[0], cam_y[1], cam_y[2]],
        [cam_z[0], cam_z[1], cam_z[2]],
    ]);
    let base = Quaternion::from_rotation_matrix(&rot);
    let half = (roll_deg.to_radians() as f32) / 2.0;
    let roll = Quaternion::new(half.cos(), 0.0, 0.0, half.sin());
    roll * base
}

/// Centre-origin centroids (tetra3 convention). mag_limit=None takes all; noise is a per-axis Gaussian σ (pixels).
#[allow(clippy::too_many_arguments)] // a test helper: the arguments are the scene description
pub fn ideal_centroids(
    stars: &[Star],
    quat: &Quaternion<f32>,
    fov_deg: f32,
    width: u32,
    height: u32,
    mag_limit: Option<f32>,
    noise_sigma_px: f32,
    seed: u64,
) -> Vec<Centroid> {
    let fov = fov_deg.to_radians();
    let f = (width as f32 / 2.0) / (fov / 2.0).tan(); // focal length in pixels, as CameraModel::from_fov
    let (hw, hh) = (width as f32 / 2.0, height as f32 / 2.0);
    let mut rng = StdRng::seed_from_u64(seed);
    let noise = Normal::new(0.0f32, noise_sigma_px.max(1e-6)).unwrap();
    let mut out: Vec<(f32, Centroid)> = stars
        .iter()
        .filter(|s| mag_limit.is_none_or(|m| s.mag <= m))
        .filter_map(|s| {
            let v = *quat * s.uvec();
            if v[2] < 0.2 {
                return None;
            }
            let x = v[0] / v[2] * f + noise.sample(&mut rng);
            let y = v[1] / v[2] * f + noise.sample(&mut rng);
            (x.abs() < hw && y.abs() < hh).then(|| {
                (
                    s.mag,
                    Centroid {
                        x,
                        y,
                        mass: Some(10.0f32.powf(0.4 * (8.0 - s.mag))),
                        cov: None,
                    },
                )
            })
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0)); // brightest first
    out.into_iter().map(|(_, c)| c).collect()
}

/// Angle (arcmin) between two directions as atan2(|a×b|, a·b): exact for tiny angles and
/// independent of the vectors' lengths. acos of the dot product is neither — axes rotated by an
/// f32 quaternion are off unit length by ~1e-7, which acos reads as ~1′.
fn angle_arcmin(a: [f64; 3], b: [f64; 3]) -> f32 {
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let sin = cross.iter().map(|c| c * c).sum::<f64>().sqrt();
    let cos: f64 = (0..3).map(|i| a[i] * b[i]).sum();
    (sin.atan2(cos).to_degrees() * 60.0) as f32
}

fn boresight(q: &Quaternion<f32>) -> [f64; 3] {
    let b = q.inverse() * Vector3::from_array([0.0f32, 0.0, 1.0]);
    [b[0] as f64, b[1] as f64, b[2] as f64]
}

pub fn boresight_err_arcmin(a: &Quaternion<f32>, b: &Quaternion<f32>) -> f32 {
    angle_arcmin(boresight(a), boresight(b))
}

/// Angular distance (arcmin) between (ra_deg, dec_deg) and the quaternion's boresight
pub fn radec_err_arcmin(ra_deg: f64, dec_deg: f64, q: &Quaternion<f32>) -> f32 {
    let (ra, dec) = (ra_deg.to_radians(), dec_deg.to_radians());
    let t = [dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()];
    angle_arcmin(boresight(q), t)
}

#[derive(Debug, Clone)]
pub struct RenderParams {
    pub psf_sigma_px: f32,
    pub background: f32,
    pub noise_sigma: f32,
    pub mag_limit: f32,
    /// Total flux (ADU) of a magnitude-8 star; flux = flux_mag8 · 10^(0.4·(8−mag))
    pub flux_mag8: f32,
}
impl Default for RenderParams {
    fn default() -> Self {
        Self {
            psf_sigma_px: 1.5,
            background: 100.0,
            noise_sigma: 2.0,
            mag_limit: 7.0,
            flux_mag8: 200.0,
        }
    }
}

pub fn render(
    stars: &[Star],
    quat: &Quaternion<f32>,
    fov_deg: f32,
    width: u32,
    height: u32,
    params: &RenderParams,
    seed: u64,
) -> Vec<f32> {
    let (w, h) = (width as usize, height as usize);
    let mut img = vec![params.background; w * h];
    // Ideal position (no noise) → top-left-origin raster coordinates
    let cents = ideal_centroids(
        stars,
        quat,
        fov_deg,
        width,
        height,
        Some(params.mag_limit),
        0.0,
        0,
    );
    let (cx0, cy0) = ((width as f32 - 1.0) / 2.0, (height as f32 - 1.0) / 2.0);
    let s2 = 2.0 * params.psf_sigma_px * params.psf_sigma_px;
    let norm = 1.0 / (std::f32::consts::PI * s2); // 2D Gaussian normalization, sum ≈ flux
    let r = (4.0 * params.psf_sigma_px).ceil() as i64;
    for c in &cents {
        let flux = params.flux_mag8 * c.mass.unwrap_or(1.0);
        let (px, py) = (c.x + cx0, c.y + cy0);
        let (ix, iy) = (px.round() as i64, py.round() as i64);
        for yy in (iy - r).max(0)..=(iy + r).min(h as i64 - 1) {
            for xx in (ix - r).max(0)..=(ix + r).min(w as i64 - 1) {
                let (dx, dy) = (xx as f32 - px, yy as f32 - py);
                img[yy as usize * w + xx as usize] +=
                    flux * norm * (-(dx * dx + dy * dy) / s2).exp();
            }
        }
    }
    let mut rng = StdRng::seed_from_u64(seed);
    let n = Normal::new(0.0f32, params.noise_sigma.max(1e-6)).unwrap();
    for p in &mut img {
        *p += n.sample(&mut rng);
    }
    img
}

/// A small 15–40° multi-scale database from 6000 random stars, built once per process (~seconds).
pub fn test_db() -> &'static SolverDatabase {
    static DB: OnceLock<SolverDatabase> = OnceLock::new();
    DB.get_or_init(|| {
        let stars = random_sky(6000, 42, 0.5, 8.0);
        let cfg = GenerateDatabaseConfig {
            min_fov_deg: Some(15.0),
            max_fov_deg: 40.0,
            lattice_field_oversampling: 20,
            patterns_per_lattice_field: 20,
            epoch_proper_motion_year: None,
            ..Default::default()
        };
        SolverDatabase::generate_from_star_list(stars, &cfg, 2026.0).expect("gen test db")
    })
}

/// `test_db()` saved as a UNISOLV2 file named `name` in the temp dir; returns its path. Tests run
/// in parallel, so the file is written under a lock and moved into place by rename: no caller
/// ever opens a half-written file.
pub fn test_db_file(name: &str) -> String {
    static WRITE: Mutex<()> = Mutex::new(());
    let path = std::env::temp_dir().join(name);
    let _guard = WRITE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !path.exists() {
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        test_db()
            .save_to_file_v2(tmp.to_str().expect("UTF-8 temp dir"))
            .expect("write the test database");
        std::fs::rename(&tmp, &path).expect("move the test database into place");
    }
    path.to_string_lossy().into_owned()
}

/// Narrow test database (8–15°), the second tier for multi-tier routing. **Its catalog is
/// independent of `test_db`** (a different random sky), so the wrong tier really fails and
/// routing assertions mean something. 30000 stars ≈ 0.73/deg², about 50 in a 10° field.
pub fn narrow_test_db() -> &'static SolverDatabase {
    static DB: OnceLock<SolverDatabase> = OnceLock::new();
    DB.get_or_init(|| {
        let stars = random_sky(30000, 4242, 0.5, 9.0);
        let cfg = GenerateDatabaseConfig {
            min_fov_deg: Some(8.0),
            max_fov_deg: 15.0,
            lattice_field_oversampling: 10,
            patterns_per_lattice_field: 15,
            epoch_proper_motion_year: None,
            ..Default::default()
        };
        SolverDatabase::generate_from_star_list(stars, &cfg, 2026.0).expect("gen narrow test db")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use numeris::Vector3;

    #[test]
    fn boresight_errors_resolve_sub_arcsecond_offsets() {
        let q = look_at(52.5, -17.0, 153.0);
        let half = (1.0f32 / 3600.0).to_radians() / 2.0;
        let tilt = Quaternion::new(half.cos(), half.sin(), 0.0, 0.0); // 1″ about the camera x axis
        let one = boresight_err_arcmin(&(tilt * q), &q) * 60.0;
        assert!((one - 1.0).abs() < 0.1, "a 1″ tilt reads {one}″");
        assert!(boresight_err_arcmin(&q, &q) * 60.0 < 0.05);
        let b = q.inverse() * Vector3::from_array([0.0f32, 0.0, 1.0]);
        let ra = (b[1] as f64).atan2(b[0] as f64).to_degrees();
        let dec = (b[2] as f64 / (b.norm() as f64)).asin().to_degrees();
        let own = radec_err_arcmin(ra, dec, &q) * 60.0;
        assert!(own < 0.05, "own boresight reads {own}″");
    }

    #[test]
    fn look_at_points_boresight_at_target() {
        let q = look_at(80.0, 10.0, 30.0);
        // ICRS direction of the camera +Z (boresight) = q⁻¹ * [0,0,1]
        let b = q.inverse() * Vector3::from_array([0.0f32, 0.0, 1.0]);
        let ra = b[1].atan2(b[0]).to_degrees().rem_euclid(360.0);
        let dec = b[2].asin().to_degrees();
        assert!((ra - 80.0).abs() < 0.01, "ra={ra}");
        assert!((dec - 10.0).abs() < 0.01, "dec={dec}");
    }

    #[test]
    fn synth_field_solves_against_test_db() {
        let db = test_db();
        let q = look_at(80.0, 10.0, 30.0);
        let cents = ideal_centroids(db.star_catalog.stars(), &q, 20.0, 1024, 768, None, 0.1, 7);
        assert!(cents.len() >= 10, "only {} centroids", cents.len());
        let cfg = tetra3::SolveConfig {
            fov_max_error_rad: Some(3.0f32.to_radians()),
            solve_timeout_ms: Some(30_000),
            ..tetra3::SolveConfig::new(20.0f32.to_radians(), 1024, 768)
        };
        let sol = db.solve_from_centroids(&cents, &cfg).expect("solve failed");
        assert!(boresight_err_arcmin(&sol.qicrs2cam, &q) < 2.0);
    }

    #[test]
    fn rendered_field_extracts_and_solves() {
        let db = test_db();
        let q = look_at(200.0, -30.0, 0.0);
        let img = render(
            db.star_catalog.stars(),
            &q,
            20.0,
            1024,
            768,
            &RenderParams::default(),
            3,
        );
        let ext = tetra3::extract_centroids_from_raw(
            &img,
            1024,
            768,
            &tetra3::CentroidExtractionConfig {
                max_centroids: Some(80),
                ..Default::default()
            },
        )
        .expect("extract");
        assert!(
            ext.centroids.len() >= 10,
            "extracted {}",
            ext.centroids.len()
        );
        let cfg = tetra3::SolveConfig {
            fov_max_error_rad: Some(3.0f32.to_radians()),
            solve_timeout_ms: Some(30_000),
            ..tetra3::SolveConfig::new(20.0f32.to_radians(), 1024, 768)
        };
        let sol = db
            .solve_from_centroids(&ext.centroids, &cfg)
            .expect("solve");
        // The WCS is authoritative: crval must hit the target boresight (< 0.5′)
        let dra = (sol.crval_rad[0].to_degrees().rem_euclid(360.0) - 200.0).abs();
        let ddec = (sol.crval_rad[1].to_degrees() - (-30.0)).abs();
        assert!(
            dra * (-30.0f64).to_radians().cos() * 60.0 < 0.5 && ddec * 60.0 < 0.5,
            "crval off: dra={dra} ddec={ddec}"
        );
        // Known upstream behaviour: qicrs2cam is the SVD-stage attitude, not refined with the WCS (≈3.1′ here)
        let err = boresight_err_arcmin(&sol.qicrs2cam, &q);
        assert!(err < 6.0, "quat boresight err={err}");
    }
}
