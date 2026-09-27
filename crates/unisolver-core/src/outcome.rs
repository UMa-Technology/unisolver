use crate::{camera::CameraParams, coords, quat::quat_to_wxyz};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolveStatus {
    Ok,
    NoMatch,
    Timeout,
    TooFew,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CentroidOut {
    pub x: f64,
    pub y: f64,
    pub mass: Option<f32>,
    /// Axis ratio √(λmax/λmin) from the extraction's second-moment covariance.
    /// 1.0 is a round star; trailing or tracking error raises it. None when the
    /// extraction path gives no covariance.
    pub elongation: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchOut {
    pub centroid_index: usize,
    pub catalog_id: i64,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Timing {
    pub extract_ms: f32,
    pub solve_ms: f32,
    pub total_ms: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wcs {
    pub width: u32,
    pub height: u32,
    pub cd: [[f64; 2]; 2],
    pub crval_deg: [f64; 2],
    pub theta_rad: f64,
    pub camera: CameraParams,
}

impl Wcs {
    /// Rebuilds a tetra3::Solution used only for coordinate transforms: its
    /// pixel_to_world/world_to_pixel read only camera_model, theta_rad and crval_rad
    /// (vendored solver/mod.rs:776-809).
    fn geom(&self) -> tetra3::Solution {
        tetra3::Solution {
            qicrs2cam: numeris::Quaternion::new(1.0, 0.0, 0.0, 0.0), // unused by the transforms
            fov_rad: 0.0,
            num_matches: 0,
            rmse_rad: 0.0,
            p90e_rad: 0.0,
            max_err_rad: 0.0,
            prob: 0.0,
            // Diagnostics added upstream in 0.13; the transforms do not read them
            attitude_cov_rad2: [[0.0; 3]; 3],
            observer_velocity_km_s: None,
            solve_time_ms: 0.0,
            parity_flip: self.camera.parity_flip,
            matched_catalog_ids: Vec::new(),
            matched_centroid_indices: Vec::new(),
            cd_matrix: self.cd,
            crval_rad: [
                self.crval_deg[0].to_radians(),
                self.crval_deg[1].to_radians(),
            ],
            camera_model: self
                .camera
                .to_tetra3(self.width, self.height)
                .expect("wcs camera was validated at construction"),
            theta_rad: self.theta_rad,
        }
    }

    /// Top-left-origin pixel → (ra, dec) in degrees
    pub fn pixel_to_world(&self, x: f64, y: f64) -> (f64, f64) {
        let (cx, cy) = coords::topleft_to_center(x, y, self.width, self.height);
        self.geom().pixel_to_world(cx, cy)
    }

    /// (ra, dec) in degrees → top-left-origin pixel (None behind the camera)
    pub fn world_to_pixel(&self, ra_deg: f64, dec_deg: f64) -> Option<(f64, f64)> {
        let (cx, cy) = self.geom().world_to_pixel(ra_deg, dec_deg)?;
        Some(coords::center_to_topleft(cx, cy, self.width, self.height))
    }

    pub fn scale_arcsec_per_px(&self) -> f64 {
        (1.0 / self.camera.focal_length_px).to_degrees() * 3600.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolvedGeometry {
    /// Attitude quaternion [w,x,y,z] from the SVD refinement. Upstream's final 3-DOF
    /// WCS refinement does not write back to it, so the two can differ by a few
    /// arcminutes: ra/dec_deg and every pixel↔sky transform follow the WCS.
    pub quat_icrs2cam_wxyz: [f32; 4],
    /// Boresight (image centre through the WCS), degrees
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub roll_deg: f64,
    pub fov_deg: f32,
    pub num_matches: u32,
    pub rmse_arcsec: f32,
    pub p90_arcsec: f32,
    pub max_err_arcsec: f32,
    pub prob: f64,
    pub wcs: Wcs,
    pub matched: Vec<MatchOut>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveOutcome {
    pub status: SolveStatus,
    pub solution: Option<SolvedGeometry>,
    pub centroids: Vec<CentroidOut>,
    pub timing: Timing,
    /// The extraction was retried with the other profile: on Ok the solution came
    /// from it; on failure both were tried. See SolveOptions::retry_alternate_profile.
    pub extraction_retried: bool,
    /// Median axis ratio of the brightest quarter of centroids (see
    /// [`median_elongation`]): a star-shape diagnostic that separates "stars trailed
    /// into streaks" from "no stars at all".
    ///
    /// **It is relative; there is no cross-device threshold.** It depends on pixel
    /// scale, PSF size, exposure and tracking. Round stars on a 2.0″/px telescope
    /// camera measure 1.3–1.4 and its 3 s untracked frames 1.8–1.9, while phone
    /// frames span 1.13–2.51 and all solve, so a fixed "≥ 1.8 means trailed" rule
    /// would misreport a quarter of solvable phone frames.
    ///
    /// Compare within one camera: baseline on known-good frames, then look at how
    /// far a failing frame deviates. For finer analysis use the per-centroid
    /// [`CentroidOut::elongation`] (for example the p75 of bright stars). The
    /// extraction covariance underestimates long streaks. None with no centroids.
    pub median_elongation: Option<f32>,
    /// Observation time the solve used (Unix ms, UTC): the caller's, or for file entries the
    /// header's when it pins the zone (FITS `DATE-OBS`, EXIF with an offset). Hand it to the
    /// annotator for the solar-system layer; None when neither gave one.
    #[serde(default)]
    pub observation_unix_ms: Option<i64>,
    /// Where the observation was made, for file entries whose header says so (EXIF GPS).
    /// Not used by the solve; hand it to the annotator with the time (the moon's parallax
    /// reaches 1° without it). None for frames and files without a position.
    #[serde(default)]
    pub observer: Option<crate::Observer>,
}

/// Axis ratio √(λmax/λmin) from the extraction's second-moment covariance.
pub(crate) fn elongation_of(cov: &tetra3::Matrix2) -> f32 {
    let (cxx, cyy, cxy) = (cov[(0, 0)] as f64, cov[(1, 1)] as f64, cov[(0, 1)] as f64);
    let trace = cxx + cyy;
    let disc = (trace * trace - 4.0 * (cxx * cyy - cxy * cxy))
        .max(0.0)
        .sqrt();
    let lmax = (trace + disc) / 2.0;
    let lmin = ((trace - disc) / 2.0).max(1e-12);
    (lmax / lmin).sqrt() as f32
}

/// Trailing diagnostic: median elongation of the **brightest quarter** of centroids.
///
/// Not the overall median: the faint end is full of noise and hot-pixel blobs that
/// are naturally round and flatten the median. Measured on one telescope camera,
/// trailed vs round frames were 1.8 vs 1.5 overall (barely separable) and 1.9 vs
/// 1.4 over the brightest quarter. Falls back to the overall median without mass
/// information; None when empty or without covariance.
pub(crate) fn median_elongation(centroids: &[CentroidOut]) -> Option<f32> {
    let mut v: Vec<(f32, f32)> = centroids
        .iter()
        .filter_map(|c| c.elongation.map(|e| (c.mass.unwrap_or(0.0), e)))
        .collect();
    if v.is_empty() {
        return None;
    }
    // Sort by mass, descending; take the top 25% (at least 4, or all if fewer)
    v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let take = (v.len() / 4).max(4).min(v.len());
    let mut e: Vec<f32> = v[..take].iter().map(|t| t.1).collect();
    e.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(e[e.len() / 2])
}

pub(crate) fn geometry_from_solution(
    sol: &tetra3::Solution,
    width: u32,
    height: u32,
    centroids_topleft: &[CentroidOut],
) -> SolvedGeometry {
    let wcs = Wcs {
        width,
        height,
        cd: sol.cd_matrix,
        crval_deg: [
            sol.crval_rad[0].to_degrees().rem_euclid(360.0),
            sol.crval_rad[1].to_degrees(),
        ],
        theta_rad: sol.theta_rad,
        camera: CameraParams::from_tetra3(&sol.camera_model),
    };
    // Boresight = sky position of the image centre, from the WCS (not the SVD quaternion)
    let (ra_deg, dec_deg) =
        wcs.pixel_to_world((width as f64 - 1.0) / 2.0, (height as f64 - 1.0) / 2.0);
    let matched = sol
        .matched_centroid_indices
        .iter()
        .zip(&sol.matched_catalog_ids)
        .map(|(&ci, &id)| MatchOut {
            centroid_index: ci,
            catalog_id: id,
            x: centroids_topleft[ci].x,
            y: centroids_topleft[ci].y,
        })
        .collect();
    SolvedGeometry {
        quat_icrs2cam_wxyz: quat_to_wxyz(&sol.qicrs2cam),
        ra_deg,
        dec_deg,
        roll_deg: sol.theta_rad.to_degrees(),
        fov_deg: sol.fov_rad.to_degrees(),
        num_matches: sol.num_matches,
        rmse_arcsec: sol.rmse_rad.to_degrees() * 3600.0,
        p90_arcsec: sol.p90e_rad.to_degrees() * 3600.0,
        max_err_arcsec: sol.max_err_rad.to_degrees() * 3600.0,
        prob: sol.prob,
        wcs,
        matched,
    }
}
