//! Narrow-field engine: seiza's blind pattern index and star tiles, fed with unisolver's own
//! centroids and checked with unisolver's own matcher. Desktop builds only (`narrow` feature).
pub(crate) mod geometry;
#[allow(dead_code)] // the pool takes it up in the next commit
pub(crate) mod route;
#[doc(hidden)]
pub mod testkit;
pub(crate) mod verify;

use crate::error::{CoreError, Result};
use crate::outcome::{CentroidOut, SolveStatus, SolvedGeometry};
use seiza::blind::{BlindIndex, BlindParams};
use seiza::catalog::{StarCatalog, TileCatalog};
use std::sync::Arc;
use std::time::Instant;

pub use route::NARROW_MAX_FOV_DEG;

/// Matches a blind solution must keep after our own check (seiza's own blind floor)
pub const BLIND_MIN_MATCHES: usize = 12;
/// Matches a hinted solution must keep after our own check
pub const HINTED_MIN_MATCHES: usize = 8;
/// Highest accepted chance-match probability (tetra3's `match_threshold`)
pub const MAX_MISMATCH_PROB: f64 = 1e-5;

/// seiza's index schema 1: disc radius (degrees) and magnitude cap of each tier
const SCHEMA1_TIERS: [(f64, f32); 8] = [
    (6.0, 6.1),
    (3.0, 7.6),
    (1.5, 9.2),
    (0.75, 10.7),
    (0.4, 11.8),
    (0.2, 12.7),
    (0.1, 14.2),
    (0.06, 16.0),
];

/// Smallest horizontal FOV an index serves: a frame must hold a whole disc of its narrowest
/// fully populated tier, i.e. a short side of two radii; three radii of width covers a 3:2
/// frame.
pub(crate) fn min_fov_for(index_mag_limit: f32) -> f32 {
    let r = SCHEMA1_TIERS
        .iter()
        .filter(|(_, cap)| *cap <= index_mag_limit + 1e-3)
        .map(|(r, _)| *r)
        .fold(f64::INFINITY, f64::min);
    (3.0 * r) as f32
}

pub struct NarrowInfo {
    /// File stem of the index (or the name given to `from_parts`)
    pub name: String,
    pub min_fov_deg: f32,
    pub max_fov_deg: f32,
    pub num_patterns: u64,
    pub num_stars: u64,
    pub index_mag_limit: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NarrowMode {
    /// No position: horizontal FOV range, degrees
    Blind { min_fov_deg: f64, max_fov_deg: f64 },
    /// Approximate pointing; on failure the engine falls back to a blind search over
    /// `fov_deg·(1 ± fov_tolerance)`
    Hinted {
        ra_deg: f64,
        dec_deg: f64,
        radius_deg: f64,
        fov_deg: f64,
        fov_tolerance: f64,
    },
}

pub struct NarrowRequest {
    pub mode: NarrowMode,
    /// Give up after this instant (`SolveStatus::Timeout`)
    pub deadline: Option<Instant>,
}

pub struct NarrowOutcome {
    pub status: SolveStatus,
    pub solution: Option<SolvedGeometry>,
    pub solve_ms: f32,
}

pub struct NarrowEngine {
    index: BlindIndex,
    catalog: Box<dyn StarCatalog + Send + Sync>,
    info: NarrowInfo,
    pool: Arc<rayon::ThreadPool>,
}

/// Pixel scale (″/px) at the centre of a pinhole frame of horizontal FOV `fov_deg` over
/// `width` px
fn scale_of(fov_deg: f64, width: u32) -> f64 {
    let f_px = (width as f64 / 2.0) / (fov_deg.to_radians() / 2.0).tan();
    (1.0 / f_px).to_degrees() * 3600.0
}

impl NarrowEngine {
    /// Opens a blind index (`SEIZABI1`) and its star tiles (`SEIZAST1/2`). Both are memory
    /// mapped; nothing is read beyond their headers.
    pub fn open(index_path: &str, stars_path: &str) -> Result<NarrowEngine> {
        Self::open_with_pool(index_path, stars_path, crate::solver::build_pool()?)
    }

    pub(crate) fn open_with_pool(
        index_path: &str,
        stars_path: &str,
        pool: Arc<rayon::ThreadPool>,
    ) -> Result<NarrowEngine> {
        let index = BlindIndex::open(std::path::Path::new(index_path))
            .map_err(|e| CoreError::InvalidInput(format!("{index_path}: {e}")))?;
        let catalog = TileCatalog::open(std::path::Path::new(stars_path))?;
        let name = std::path::Path::new(index_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| index_path.to_string());
        Self::assemble(&name, index, Box::new(catalog), pool)
    }

    #[doc(hidden)]
    pub fn from_parts(
        name: &str,
        index: BlindIndex,
        catalog: Box<dyn StarCatalog + Send + Sync>,
    ) -> Result<NarrowEngine> {
        Self::assemble(name, index, catalog, crate::solver::build_pool()?)
    }

    fn assemble(
        name: &str,
        index: BlindIndex,
        catalog: Box<dyn StarCatalog + Send + Sync>,
        pool: Arc<rayon::ThreadPool>,
    ) -> Result<NarrowEngine> {
        let info = NarrowInfo {
            name: name.to_string(),
            min_fov_deg: min_fov_for(index.index_mag_limit()),
            max_fov_deg: (2.0 * index.max_pattern_deg()) as f32,
            num_patterns: index.pattern_count() as u64,
            num_stars: catalog.star_count(),
            index_mag_limit: index.index_mag_limit(),
        };
        Ok(NarrowEngine {
            index,
            catalog,
            info,
            pool,
        })
    }

    pub fn info(&self) -> &NarrowInfo {
        &self.info
    }

    /// Solves `centroids` (top-left pixel coordinates, any order) of a `width`×`height` frame.
    pub fn solve(
        &self,
        centroids: &[CentroidOut],
        width: u32,
        height: u32,
        req: &NarrowRequest,
    ) -> NarrowOutcome {
        let t0 = Instant::now();
        let done = |status, solution| NarrowOutcome {
            status,
            solution,
            solve_ms: t0.elapsed().as_secs_f32() * 1000.0,
        };
        let floor = match req.mode {
            NarrowMode::Blind { .. } => BLIND_MIN_MATCHES,
            NarrowMode::Hinted { .. } => HINTED_MIN_MATCHES,
        };
        if centroids.len() < floor {
            return done(SolveStatus::TooFew, None);
        }
        let mut stars: Vec<seiza::DetectedStar> = centroids
            .iter()
            .map(|c| {
                let m = c.mass.unwrap_or(0.0) as f64;
                seiza::DetectedStar {
                    x: c.x,
                    y: c.y,
                    flux: m,
                    peak: m as f32,
                    area: 9,
                }
            })
            .collect();
        stars.sort_by(|a, b| b.flux.total_cmp(&a.flux));
        let dims = (width, height);
        let catalog: &(dyn StarCatalog + Sync) = &*self.catalog;

        let blind = |lo_fov: f64, hi_fov: f64| {
            let (a, b) = (scale_of(lo_fov, width), scale_of(hi_fov, width));
            let params = BlindParams {
                min_scale_arcsec_px: a.min(b),
                max_scale_arcsec_px: a.max(b),
                index_mag_limit: self.index.index_mag_limit(),
                max_pattern_deg: self.index.max_pattern_deg(),
                ..Default::default()
            };
            seiza::blind::solve_blind_until(
                &stars,
                catalog,
                &self.index,
                &params,
                dims,
                req.deadline,
            )
        };

        let solved = self.pool.install(|| match req.mode {
            NarrowMode::Blind {
                min_fov_deg,
                max_fov_deg,
            } => blind(min_fov_deg, max_fov_deg),
            NarrowMode::Hinted {
                ra_deg,
                dec_deg,
                radius_deg,
                fov_deg,
                fov_tolerance,
            } => {
                let hint = seiza::solve::SolveHint {
                    center: (ra_deg, dec_deg),
                    radius_deg,
                    scale_arcsec_px: scale_of(fov_deg, width),
                    scale_tolerance: fov_tolerance,
                    sip_order: 0,
                };
                match seiza::solve::solve_until(&stars, catalog, &hint, dims, req.deadline) {
                    Err(seiza::Error::Solve(_)) => blind(
                        fov_deg * (1.0 - fov_tolerance),
                        fov_deg * (1.0 + fov_tolerance),
                    ),
                    other => other,
                }
            }
        });

        let sol = match solved {
            Ok(s) => s,
            Err(seiza::Error::Timeout) => return done(SolveStatus::Timeout, None),
            Err(_) => return done(SolveStatus::NoMatch, None),
        };
        let wcs = geometry::wcs_from_seiza(&sol.wcs, width, height);
        let checked = verify::verify(&wcs, centroids, catalog);
        if checked.matched.len() < floor || checked.prob > MAX_MISMATCH_PROB {
            return done(SolveStatus::NoMatch, None);
        }
        let (ra_deg, dec_deg) =
            wcs.pixel_to_world((width as f64 - 1.0) / 2.0, (height as f64 - 1.0) / 2.0);
        let fov_deg =
            (2.0 * (width as f64 / (2.0 * wcs.camera.focal_length_px)).atan()).to_degrees() as f32;
        let geometry = SolvedGeometry {
            quat_icrs2cam_wxyz: geometry::attitude_wxyz(&wcs),
            ra_deg,
            dec_deg,
            roll_deg: wcs.theta_rad.to_degrees(),
            fov_deg,
            num_matches: checked.matched.len() as u32,
            rmse_arcsec: checked.rmse_arcsec,
            p90_arcsec: checked.p90_arcsec,
            max_err_arcsec: checked.max_err_arcsec,
            prob: checked.prob,
            wcs,
            matched: checked.matched,
            lens_fitted: false,
            scale_refined: false,
        };
        done(SolveStatus::Ok, Some(geometry))
    }
}
