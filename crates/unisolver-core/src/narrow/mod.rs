//! Narrow-field engine: a star-centred blind pattern index and star tiles
//! (unisolver-starmatch), fed with unisolver's own centroids and checked with unisolver's own
//! matcher. Desktop builds only (`narrow` feature).
pub(crate) mod geometry;
pub(crate) mod route;
#[doc(hidden)]
pub mod testkit;
pub(crate) mod verify;

use crate::error::{CoreError, Result};
use crate::outcome::{CentroidOut, SolveStatus, SolvedGeometry};
use std::sync::Arc;
use std::time::{Duration, Instant};
use unisolver_starmatch::blind::{BlindIndex, BlindParams};
use unisolver_starmatch::catalog::{StarCatalog, TileCatalog};

pub use route::NARROW_MAX_FOV_DEG;

/// Matches a blind solution must keep after our own check (unisolver-starmatch's own blind floor)
pub const BLIND_MIN_MATCHES: usize = 12;
/// Matches a hinted solution must keep after our own check
pub const HINTED_MIN_MATCHES: usize = 8;
/// Highest accepted chance-match probability (tetra3's `match_threshold`)
pub const MAX_MISMATCH_PROB: f64 = 1e-5;

/// Index schema 1: disc radius (degrees) and magnitude cap of each tier
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
    /// Opens a blind index (`UNIBLIX1`) and its star tiles (`UNISTAR1`). Both are memory
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
        let mut stars: Vec<unisolver_starmatch::DetectedStar> = centroids
            .iter()
            .map(|c| {
                let m = c.mass.unwrap_or(0.0) as f64;
                unisolver_starmatch::DetectedStar {
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
        // unisolver-starmatch's solution, kept when our own check finds `floor` matches or more
        let accept = |sol: unisolver_starmatch::solve::Solution, floor: usize| {
            self.geometry(&sol, centroids, width, height, floor)
                .ok_or(SolveStatus::NoMatch)
        };

        let blind = |lo_fov: f64, hi_fov: f64| {
            let (a, b) = (scale_of(lo_fov, width), scale_of(hi_fov, width));
            let params = BlindParams {
                min_scale_arcsec_px: a.min(b),
                max_scale_arcsec_px: a.max(b),
                index_mag_limit: self.index.index_mag_limit(),
                max_pattern_deg: self.index.max_pattern_deg(),
                ..Default::default()
            };
            match unisolver_starmatch::blind::solve_blind_until(
                &stars,
                catalog,
                &self.index,
                &params,
                dims,
                req.deadline,
            ) {
                Ok(sol) => accept(sol, BLIND_MIN_MATCHES),
                Err(unisolver_starmatch::Error::Timeout) => Err(SolveStatus::Timeout),
                Err(_) => Err(SolveStatus::NoMatch),
            }
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
                let hint = unisolver_starmatch::solve::SolveHint {
                    center: (ra_deg, dec_deg),
                    radius_deg,
                    scale_arcsec_px: scale_of(fov_deg, width),
                    scale_tolerance: fov_tolerance,
                    sip_order: 0,
                };
                // A field near the hint is found in tens of milliseconds; one further away
                // makes the search scan its whole radius, and one outside it can yield a wrong
                // fit. So the hint gets part of the time, its result is checked, and anything
                // short of a checked solution leaves the rest to the blind search.
                let phase = hinted_phase_end(Instant::now(), req.deadline);
                let hinted = unisolver_starmatch::solve::solve_until(
                    &stars,
                    catalog,
                    &hint,
                    dims,
                    Some(phase),
                )
                .map_err(|_| SolveStatus::NoMatch)
                .and_then(|sol| accept(sol, HINTED_MIN_MATCHES));
                match hinted {
                    Ok(g) => Ok(g),
                    Err(_) if req.deadline.is_some_and(|d| Instant::now() >= d) => {
                        Err(SolveStatus::Timeout)
                    }
                    Err(_) => blind(
                        fov_deg * (1.0 - fov_tolerance),
                        fov_deg * (1.0 + fov_tolerance),
                    ),
                }
            }
        });
        match solved {
            Ok(g) => done(SolveStatus::Ok, Some(g)),
            Err(status) => done(status, None),
        }
    }

    /// The geometry of a unisolver-starmatch solution after our own check: None unless it keeps
    /// `floor` matches at a low enough chance probability
    fn geometry(
        &self,
        sol: &unisolver_starmatch::solve::Solution,
        centroids: &[CentroidOut],
        width: u32,
        height: u32,
        floor: usize,
    ) -> Option<SolvedGeometry> {
        let wcs = geometry::wcs_from_linear(&sol.wcs, width, height);
        let checked = verify::verify(&wcs, centroids, &*self.catalog);
        if checked.matched.len() < floor || checked.prob > MAX_MISMATCH_PROB {
            return None;
        }
        let (ra_deg, dec_deg) =
            wcs.pixel_to_world((width as f64 - 1.0) / 2.0, (height as f64 - 1.0) / 2.0);
        let fov_deg =
            (2.0 * (width as f64 / (2.0 * wcs.camera.focal_length_px)).atan()).to_degrees() as f32;
        Some(SolvedGeometry {
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
        })
    }
}

/// Reads `paths` once from start to end so the page cache holds them: the engine's files are
/// memory mapped, and a first solve on cold files would wait on the disk mid-search (seconds past
/// its deadline inside a pattern loop). Returns the bytes read.
pub(crate) fn prefetch(paths: &[std::path::PathBuf]) -> std::io::Result<u64> {
    use std::io::Read;
    let mut buf = vec![0u8; 8 << 20];
    let mut total = 0u64;
    for p in paths {
        let mut f = std::fs::File::open(p)?;
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            total += n as u64;
        }
    }
    Ok(total)
}

/// Most time the hinted search gets before the blind search takes over
const HINTED_PHASE_MAX: Duration = Duration::from_secs(1);

/// End of the hinted search: half of what is left before `deadline`, at most
/// [`HINTED_PHASE_MAX`], so a hint far off still leaves the blind search its time
fn hinted_phase_end(now: Instant, deadline: Option<Instant>) -> Instant {
    let share = deadline.map_or(HINTED_PHASE_MAX, |d| {
        (d.saturating_duration_since(now) / 2).min(HINTED_PHASE_MAX)
    });
    now + share
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefetching_reads_every_byte_of_every_file() {
        let dir = std::env::temp_dir().join(format!("unisolver_prefetch_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a.idx"), dir.join("b.stars"));
        std::fs::write(&a, vec![1u8; 3 << 20]).unwrap();
        std::fs::write(&b, vec![2u8; 9 << 20]).unwrap();
        assert_eq!(prefetch(&[a.clone(), b]).unwrap(), 12 << 20);
        assert!(prefetch(&[a, dir.join("missing")]).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_hint_gets_half_the_time_left_and_at_most_a_second() {
        let now = Instant::now();
        let ms = |d: Option<u64>| {
            hinted_phase_end(now, d.map(|ms| now + Duration::from_millis(ms)))
                .duration_since(now)
                .as_millis()
        };
        assert_eq!(ms(None), 1000);
        assert_eq!(ms(Some(6000)), 1000);
        assert_eq!(ms(Some(1200)), 600);
        assert_eq!(ms(Some(0)), 0);
    }
}
