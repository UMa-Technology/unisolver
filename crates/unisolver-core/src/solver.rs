use crate::outcome::{geometry_from_solution, CentroidOut, SolveOutcome, SolveStatus, Timing};
use crate::{camera::CameraParams, coords, CoreError, Frame, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tetra3::{
    extract_centroids_fast, extract_centroids_from_raw, CentroidExtractionConfig,
    FastCentroidConfig, SolveConfig, SolverDatabase,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtractionOptions {
    Ccl {
        sigma_threshold: f32,
        max_centroids: usize,
    },
    Fast {
        sigma_threshold: f32,
        max_centroids: usize,
    },
}

/// Extraction profile: makes the assumption about the input material explicit.
/// The best σ threshold depends on it: compressed phone JPEGs need a high threshold to
/// suppress noise, clean sensors a low one to keep faint stars; one default cannot serve both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExtractionProfile {
    /// Adaptive: σ10 first, then σ5 on TooFew/NoMatch (reusing the profile retry).
    /// A density-based σ5 probe was tried and rejected: on 47 phone frames it cut the
    /// success rate from 44/47 to 34/47 (noise-limited frames and dense Milky Way fields
    /// have similar σ5 blob density). With format metadata, prefer
    /// `imageio::suggested_profile`.
    Auto,
    /// Phone and consumer compressed images (8-bit JPEG, camera previews). σ=10 was best
    /// on 47 phone frames (σ5: 27/47, σ10: 42/47); it suppresses JPEG noise.
    PhoneJpeg,
    /// Clean sensors (16-bit RAW/FITS, cooled astro cameras, synthetic data). σ=5, the
    /// classic detection threshold: narrow fields cannot afford to lose faint stars.
    CleanSensor,
    /// Fully custom (method and parameters from the caller; never retried with another profile).
    Custom(ExtractionOptions),
}

impl ExtractionProfile {
    /// Exact extraction parameters of this profile (blind solve of a single photo)
    pub fn resolve(&self) -> ExtractionOptions {
        match self {
            // Auto: σ10 first (same as PhoneJpeg); the fallback is alternate()
            Self::Auto => ExtractionOptions::Ccl {
                sigma_threshold: 10.0,
                max_centroids: 100,
            },
            Self::PhoneJpeg => ExtractionOptions::Ccl {
                sigma_threshold: 10.0,
                max_centroids: 100,
            },
            Self::CleanSensor => ExtractionOptions::Ccl {
                sigma_threshold: 5.0,
                max_centroids: 100,
            },
            Self::Custom(o) => o.clone(),
        }
    }

    /// The other profile for a retry: PhoneJpeg ↔ CleanSensor; Custom has none.
    pub fn alternate(&self) -> Option<ExtractionProfile> {
        match self {
            Self::Auto | Self::PhoneJpeg => Some(Self::CleanSensor),
            Self::CleanSensor => Some(Self::PhoneJpeg),
            Self::Custom(_) => None,
        }
    }
}

/// Result of solve_inner: (public outcome, raw solver output on success)
pub(crate) type SolveInner = (
    SolveOutcome,
    Option<(tetra3::Solution, Vec<tetra3::Centroid>)>,
);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FovPreset {
    pub fov_deg: f32,
    pub max_error_deg: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FovAttempt {
    pub fov_deg: f32,
    pub status: SolveStatus,
    pub solve_ms: f32,
}

/// Approximate pointing for the narrow-field engine: a mount's position, or a header's RA/Dec.
/// A hint, never truth: when the search around it fails, the engine solves blind.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointingHint {
    /// ICRS right ascension, degrees
    pub ra_deg: f64,
    /// ICRS declination, degrees
    pub dec_deg: f64,
    /// Search radius, degrees; None searches one FOV around it
    #[serde(default)]
    pub radius_deg: Option<f64>,
}

impl PointingHint {
    pub(crate) fn validate(&self) -> Result<()> {
        let ok = self.ra_deg.is_finite()
            && (-90.0..=90.0).contains(&self.dec_deg)
            && self
                .radius_deg
                .is_none_or(|r| r.is_finite() && r > 0.0 && r <= 180.0);
        if ok {
            Ok(())
        } else {
            Err(CoreError::InvalidInput(format!(
                "pointing_hint invalid: {self:?}"
            )))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveOptions {
    pub fov_estimate_deg: f32,
    pub fov_max_error_deg: Option<f32>,
    pub camera: Option<CameraParams>,
    /// [w,x,y,z]; Some(_) switches to tracking mode
    pub attitude_hint: Option<[f32; 4]>,
    pub hint_uncertainty_deg: f32,
    pub strict_hint: bool,
    pub extraction: ExtractionProfile,
    /// On a failed blind solve, re-extract with the other profile (PhoneJpeg ↔
    /// CleanSensor) and solve once more. Triggers: a single blind solve on TooFew or
    /// NoMatch; inside a FOV ladder on TooFew only (NoMatch more likely means a wrong
    /// FOV). Never for tracking (with a hint) or Custom. Worst case ≈ 2 × timeout_ms.
    pub retry_alternate_profile: bool,
    /// Ladders only: after the staged search fails, search every rung again with every
    /// centroid and the full timeout (the exhaustive search). Off by default, so frames
    /// without stars fail in about two seconds; turn it on when waiting beats missing.
    #[serde(default)]
    pub thorough: bool,
    pub match_threshold: f64,
    pub timeout_ms: Option<u64>,
    /// Observation time (Unix ms), reported back on the outcome for the solar-system layer
    /// and the horizontal grid. It does not change the solution. Camera frames: system clock;
    /// photos: EXIF.
    pub observation_unix_ms: Option<i64>,
    /// Advanced: observer ICRS velocity in km/s, to correct stellar aberration. The solution
    /// then gives the camera's physical pointing, up to 20″ from the J2000 catalog frame, and
    /// catalog positions projected through its WCS (annotation, grids, transforms) miss the
    /// stars by as much. Leave it unset for annotation and mount sync.
    pub observer_velocity_km_s: Option<[f64; 3]>,
    /// Ladders only: a 35 mm-equivalent focal length from metadata the caller read itself
    /// (EXIF through the platform, for HEIC and other formats the engine does not decode).
    /// Its FOV goes first as a hint (±15%) and the ladder still follows; ignored by single
    /// solves and when the FOV is known (camera or tracking).
    #[serde(default)]
    pub focal_length_35mm: Option<f32>,
    /// Wide frames only (≥ 20°, ≥ 30 matched stars): after the pinhole solve, fit the focal
    /// length and one radial distortion term to the matched stars and keep the result when it
    /// fits them better, so annotations follow the lens. Never when `camera` is given.
    /// Default on.
    #[serde(default = "default_true")]
    pub fit_lens: bool,
    /// Lost-in-space solves of wide fields (20° or more) only: re-measure the scale (focal
    /// length) from the brightest stars and keep it when they land closer to catalog stars. The
    /// upstream refinement keeps the scale its 4-star pattern measured, 1–3% off on wide
    /// frames. Never with `camera` or a tracking hint. Default on.
    #[serde(default = "default_true")]
    pub refine_scale: bool,
    /// Pools with the narrow-field engine only: approximate pointing (a mount's position). File
    /// entries fill it from the header's RA/Dec when it is None. The tetra3 tiers never use it,
    /// and it changes no routing.
    #[serde(default)]
    pub pointing_hint: Option<PointingHint>,
    /// Pools with the narrow-field engine only: when the FOV is unknown, blind-solve once over
    /// the engine's range after every tetra3 tier failed. Off by default, so frames of unknown
    /// FOV (phones, frames without stars) fail as fast as without the engine.
    #[serde(default)]
    pub narrow_blind: bool,
    /// Pools with the narrow-field engine only: the most time (ms) it gets after the tetra3
    /// tiers failed, also capped by `timeout_ms`; 0 never runs it after them. Default 2000.
    #[serde(default = "default_narrow_fallback_ms")]
    pub narrow_fallback_ms: u64,
}

/// The refinements a solve runs after it succeeds
#[derive(Clone, Copy)]
pub(crate) struct Refine {
    pub scale: bool,
    pub lens: bool,
}

impl Refine {
    pub(crate) fn from_opts(opts: &SolveOptions) -> Self {
        Self {
            scale: opts.refine_scale && opts.camera.is_none() && opts.attitude_hint.is_none(),
            lens: opts.fit_lens && opts.camera.is_none(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_narrow_fallback_ms() -> u64 {
    2000
}

impl SolveOptions {
    pub fn new(fov_estimate_deg: f32) -> Self {
        Self {
            fov_estimate_deg,
            fov_max_error_deg: None,
            camera: None,
            attitude_hint: None,
            hint_uncertainty_deg: 3.0, // hand shake between consecutive long exposures
            strict_hint: false,
            // Phones are the default input; choose CleanSensor for astro cameras and RAW
            extraction: ExtractionProfile::PhoneJpeg,
            retry_alternate_profile: true,
            thorough: false,
            fit_lens: true,
            refine_scale: true,
            match_threshold: 1e-5,
            timeout_ms: Some(5000),
            observation_unix_ms: None,
            observer_velocity_km_s: None,
            focal_length_35mm: None,
            pointing_hint: None,
            narrow_blind: false,
            narrow_fallback_ms: default_narrow_fallback_ms(),
        }
    }
}

pub struct DbProperties {
    pub min_fov_deg: f32,
    pub max_fov_deg: f32,
    pub num_stars: usize,
    pub num_patterns: u32,
    pub star_max_magnitude: f32,
}

pub struct Solver {
    db: Arc<SolverDatabase>,
    /// Rayon pool for extraction. `Arc` so one pool serves every tier in a `SolverPool`;
    /// otherwise N tiers would start N×4 threads, which mobile devices cannot afford.
    pool: Arc<rayon::ThreadPool>,
}

pub(crate) fn build_pool() -> Result<Arc<rayon::ThreadPool>> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(4);
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map(Arc::new)
        .map_err(|e| CoreError::InvalidInput(e.to_string()))
}

/// Translates `SolveOptions` into upstream's `SolveConfig` (FOV validity, camera model).
/// Single source of truth: the single-database path and `SolverPool` must build the same
/// config, or switching entry points would change the solve semantics.
pub(crate) fn build_solve_config(opts: &SolveOptions, w: u32, h: u32) -> Result<SolveConfig> {
    build_solve_config_with(opts, w, h, u32::MAX)
}

/// As [`build_solve_config`], with a cap on the centroids that form patterns (the staged
/// ladder search varies it per pass, see `search`).
pub(crate) fn build_solve_config_with(
    opts: &SolveOptions,
    w: u32,
    h: u32,
    pattern_stars: u32,
) -> Result<SolveConfig> {
    if !(opts.fov_estimate_deg.is_finite()
        && opts.fov_estimate_deg > 0.1
        && opts.fov_estimate_deg < 179.0)
    {
        return Err(CoreError::InvalidInput(format!(
            "fov_estimate_deg invalid: {}",
            opts.fov_estimate_deg
        )));
    }
    let camera = match &opts.camera {
        Some(c) => {
            c.validate(w, h)?;
            c.clone()
        }
        None => CameraParams::from_horizontal_fov(opts.fov_estimate_deg as f64, w, h)?,
    };
    Ok(SolveConfig {
        fov_max_error_rad: opts.fov_max_error_deg.map(|d| d.to_radians()),
        match_threshold: opts.match_threshold,
        solve_timeout_ms: opts.timeout_ms,
        attitude_hint: opts
            .attitude_hint
            .map(|a| numeris::Quaternion::new(a[0], a[1], a[2], a[3])),
        hint_uncertainty_rad: opts.hint_uncertainty_deg.to_radians(),
        strict_hint: opts.strict_hint,
        // Only on request: the time alone would move the WCS off the catalog frame
        observer_velocity_km_s: opts.observer_velocity_km_s,
        // Upstream 0.13 builds patterns from the brightest 24 centroids by default (tuned
        // for clean tracker frames). On phone frames the brightest often include hot
        // pixels, light-pollution blobs and trailed stars, pushing the true pattern out of
        // the top 24 (the 47-frame regression dropped 44 → 42 at 24). Single solves use
        // every centroid (max_centroids already bounds them); ladders stage the cap, see
        // `search`.
        pattern_checking_stars: pattern_stars,
        ..SolveConfig::with_camera_model(camera.to_tetra3(w, h)?)
    })
}

/// The solve config of one staged-search pass: its pattern-star cap and, for the probes, its
/// pattern budget (see `search`)
pub(crate) fn pass_config(
    opts: &SolveOptions,
    w: u32,
    h: u32,
    pass: &crate::search::Pass,
) -> Result<SolveConfig> {
    let mut cfg = build_solve_config_with(opts, w, h, pass.pattern_stars)?;
    if pass.max_patterns.is_some() {
        cfg.max_patterns_checked = pass.max_patterns;
    }
    Ok(cfg)
}

impl Solver {
    pub fn from_file(path: &str) -> Result<Self> {
        Self::from_file_with_pool(path, build_pool()?)
    }

    pub(crate) fn from_file_with_pool(path: &str, pool: Arc<rayon::ThreadPool>) -> Result<Self> {
        let db = SolverDatabase::load_from_file(path)?;
        Ok(Self {
            db: Arc::new(db),
            pool,
        })
    }

    pub(crate) fn from_db(db: Arc<SolverDatabase>) -> Result<Self> {
        Ok(Self {
            db,
            pool: build_pool()?,
        })
    }

    pub(crate) fn db(&self) -> &Arc<SolverDatabase> {
        &self.db
    }

    pub fn properties(&self) -> DbProperties {
        let p = &self.db.props;
        DbProperties {
            min_fov_deg: p.min_fov_rad.to_degrees(),
            max_fov_deg: p.max_fov_rad.to_degrees(),
            num_stars: self.db.star_catalog.len(),
            num_patterns: p.num_patterns,
            star_max_magnitude: p.star_max_magnitude,
        }
    }

    pub fn solve(&self, frame: &Frame, opts: &SolveOptions) -> Result<SolveOutcome> {
        self.solve_inner(frame, opts).map(|(out, _)| out)
    }

    /// On success the second item carries the raw (Solution, centre-origin centroids), used internally by calibration.
    pub(crate) fn solve_inner(&self, frame: &Frame, opts: &SolveOptions) -> Result<SolveInner> {
        // Single blind solve: NoMatch may also retry (the FOV is trusted, so extraction is the likelier culprit)
        self.solve_inner_with(frame, opts, true)
    }

    /// retry_on_nomatch=false is for FOV ladders: there NoMatch more likely means a wrong
    /// FOV, and only TooFew (σ clearly too high, and fast to fail) is worth a retry.
    pub(crate) fn solve_inner_with(
        &self,
        frame: &Frame,
        opts: &SolveOptions,
        retry_on_nomatch: bool,
    ) -> Result<SolveInner> {
        let t_total = Instant::now();
        let (w, h) = (frame.width, frame.height);
        let cfg = build_solve_config(opts, w, h)?;

        let refine = Refine::from_opts(opts);
        let (mut out, mut raw) =
            self.extract_and_solve(frame, &opts.extraction.resolve(), &cfg, t_total, refine)?;

        // Profile retry: the fallback when the material guess was wrong. Triggers: see the field docs.
        let trigger = matches!(out.status, SolveStatus::TooFew)
            || (retry_on_nomatch && matches!(out.status, SolveStatus::NoMatch));
        if trigger && opts.retry_alternate_profile && opts.attitude_hint.is_none() {
            if let Some(alt) = opts.extraction.alternate() {
                let (out2, raw2) =
                    self.extract_and_solve(frame, &alt.resolve(), &cfg, t_total, refine)?;
                if matches!(out2.status, SolveStatus::Ok) {
                    let mut out2 = out2;
                    out2.extraction_retried = true;
                    out2.observation_unix_ms = opts.observation_unix_ms;
                    return Ok((out2, raw2));
                }
                // The retry failed too: keep the first result (its status reflects the
                // original profile) and only record that a retry happened
                out.extraction_retried = true;
                out.timing.total_ms = t_total.elapsed().as_secs_f32() * 1000.0;
                let _ = raw2;
            }
        }
        let _ = &mut raw;
        out.observation_unix_ms = opts.observation_unix_ms;
        Ok((out, raw))
    }

    /// One extraction plus solve. extract_ms/solve_ms belong to this attempt; total_ms counts from t_total.
    fn extract_and_solve(
        &self,
        frame: &Frame,
        extraction: &ExtractionOptions,
        cfg: &SolveConfig,
        t_total: Instant,
        refine: Refine,
    ) -> Result<SolveInner> {
        let ext = extract_frame(frame, extraction, &self.pool)?;
        self.solve_extracted(&ext, cfg, frame.width, frame.height, t_total, refine)
    }

    /// Solves once from already-extracted centroids. **Extraction does not depend on the
    /// database**, so `SolverPool` extracts once and reuses it across tiers (a cross-tier
    /// ladder can have a dozen rungs; extracting a 26 Mpx frame takes seconds). A solve is then
    /// refined as `refine` asks: its scale (`scale`), then a lens fitted to its stars (`lens`).
    pub(crate) fn solve_extracted(
        &self,
        ext: &Extracted,
        cfg: &SolveConfig,
        w: u32,
        h: u32,
        t_total: Instant,
        refine: Refine,
    ) -> Result<SolveInner> {
        let t_solve = Instant::now();
        let result = self
            .db
            .solve_from_centroids(&ext.centroids, cfg)
            .map(|sol| {
                let g = geometry_from_solution(&sol, w, h, &ext.topleft);
                let mut best = (sol, g);
                if refine.scale {
                    if let Some(r) =
                        crate::scale::refine(&self.db, ext, cfg, w, h, &best.0, &best.1)
                    {
                        best = r;
                    }
                }
                if refine.lens {
                    if let Some(r) = crate::lens::refine(&self.db, ext, cfg, w, h, &best.0, &best.1)
                    {
                        best = r;
                    }
                }
                best
            });
        let solve_ms = t_solve.elapsed().as_secs_f32() * 1000.0;
        let timing = Timing {
            extract_ms: ext.extract_ms,
            solve_ms,
            total_ms: t_total.elapsed().as_secs_f32() * 1000.0,
        };

        match result {
            Ok((sol, geometry)) => Ok((
                SolveOutcome {
                    status: SolveStatus::Ok,
                    solution: Some(geometry),
                    centroids: ext.topleft.clone(),
                    timing,
                    extraction_retried: false,
                    median_elongation: ext.median_elongation,
                    observation_unix_ms: None,
                    observer: None,
                },
                Some((sol, ext.centroids.clone())),
            )),
            Err(f) => {
                let status = match f.status {
                    tetra3::SolveStatus::NoMatch => SolveStatus::NoMatch,
                    tetra3::SolveStatus::Timeout => SolveStatus::Timeout,
                    tetra3::SolveStatus::TooFew => SolveStatus::TooFew,
                    tetra3::SolveStatus::InvalidConfig => {
                        return Err(CoreError::InvalidInput(
                            "upstream rejected SolveConfig (InvalidConfig) — core validation gap"
                                .into(),
                        ));
                    }
                };
                Ok((
                    SolveOutcome {
                        status,
                        solution: None,
                        centroids: ext.topleft.clone(),
                        timing,
                        extraction_retried: false,
                        median_elongation: ext.median_elongation,
                        observation_unix_ms: None,
                        observer: None,
                    },
                    None,
                ))
            }
        }
    }
}

/// One extraction: upstream centroids (centre origin, for the solver), public centroids
/// (top-left origin), shape diagnostics and timing. Depends only on (luma, profile).
pub(crate) struct Extracted {
    pub centroids: Vec<tetra3::Centroid>,
    pub topleft: Vec<CentroidOut>,
    pub median_elongation: Option<f32>,
    pub extract_ms: f32,
}

/// Extractions of one frame, one per set of resolved extraction options: extraction does
/// not depend on the database or the FOV, so ladders and pools reuse it across attempts.
#[derive(Default)]
pub(crate) struct ExtractCache {
    entries: Vec<(ExtractionOptions, Arc<Extracted>)>,
}

impl ExtractCache {
    pub(crate) fn get(
        &mut self,
        frame: &Frame,
        opts: &ExtractionOptions,
        rayon_pool: &rayon::ThreadPool,
    ) -> Result<Arc<Extracted>> {
        if let Some((_, e)) = self.entries.iter().find(|(k, _)| k == opts) {
            return Ok(e.clone());
        }
        let e = Arc::new(extract_frame(frame, opts, rayon_pool)?);
        self.entries.push((opts.clone(), e.clone()));
        Ok(e)
    }

    /// The extraction with the most centroids so far (extracting with `primary` when there is
    /// none yet), and whether it came from other options than `primary`
    #[cfg(feature = "narrow")]
    pub(crate) fn richest(
        &mut self,
        frame: &Frame,
        primary: &ExtractionOptions,
        rayon_pool: &rayon::ThreadPool,
    ) -> Result<(Arc<Extracted>, bool)> {
        match self.entries.iter().max_by_key(|(_, e)| e.topleft.len()) {
            Some((opts, e)) => Ok((e.clone(), opts != primary)),
            None => Ok((self.get(frame, primary, rayon_pool)?, false)),
        }
    }

    /// Extractions performed so far
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Extracts a frame; rayon parallelism stays inside the given pool (≤ 4 threads). The
/// luminance is converted here, per extraction, rather than held for a whole ladder; frames
/// above [`crate::bands::BANDED_ABOVE_PX`] take the banded path and never exist as
/// full-frame f32.
pub(crate) fn extract_frame(
    frame: &Frame,
    extraction: &ExtractionOptions,
    rayon_pool: &rayon::ThreadPool,
) -> Result<Extracted> {
    let (w, h) = (frame.width, frame.height);
    let t_ex = Instant::now();
    let centroids = rayon_pool.install(|| -> Result<Vec<tetra3::Centroid>> {
        Ok(match extraction {
            ExtractionOptions::Ccl {
                sigma_threshold,
                max_centroids,
            } => {
                let cfg = CentroidExtractionConfig {
                    sigma_threshold: *sigma_threshold,
                    max_centroids: Some(*max_centroids),
                    ..Default::default()
                };
                if (w as usize) * (h as usize) > crate::bands::BANDED_ABOVE_PX {
                    crate::bands::extract(frame, &cfg)?
                } else {
                    extract_centroids_from_raw(&frame.to_luma_f32()?, w, h, &cfg)?.centroids
                }
            }
            ExtractionOptions::Fast {
                sigma_threshold,
                max_centroids,
            } => {
                extract_centroids_fast(
                    &frame.to_luma_f32()?,
                    w,
                    h,
                    &FastCentroidConfig {
                        sigma_threshold: *sigma_threshold,
                        max_centroids: Some(*max_centroids),
                        ..Default::default()
                    },
                )?
                .centroids
            }
        })
    })?;
    let extract_ms = t_ex.elapsed().as_secs_f32() * 1000.0;

    let topleft: Vec<CentroidOut> = centroids
        .iter()
        .map(|c| {
            let (x, y) = coords::center_to_topleft(c.x as f64, c.y as f64, w, h);
            CentroidOut {
                x,
                y,
                mass: c.mass,
                elongation: c.cov.as_ref().map(crate::outcome::elongation_of),
            }
        })
        .collect();
    let median_elongation = crate::outcome::median_elongation(&topleft);
    Ok(Extracted {
        centroids,
        topleft,
        median_elongation,
        extract_ms,
    })
}

impl Solver {
    /// Photos with an unknown FOV: blind-solve rung by rung, stopping at the first Ok.
    /// With a calibrated camera call solve() instead, so base.camera must be None.
    pub fn solve_with_fov_presets(
        &self,
        frame: &Frame,
        base: &SolveOptions,
        presets: &[FovPreset],
    ) -> Result<(SolveOutcome, Vec<FovAttempt>)> {
        if presets.is_empty() {
            return Err(CoreError::InvalidInput("presets must not be empty".into()));
        }
        if base.camera.is_some() {
            return Err(CoreError::InvalidInput(
                "solve_with_fov_presets is for unknown-FOV frames; a calibrated camera should call solve()".into(),
            ));
        }
        // Rungs outside the database range always fail (pattern scales are not in it)
        // and each burns a full timeout on large databases (70/55/42° against a
        // 2.5–12° database: three rungs × 4 s wasted). Clamp to the database range
        // (with tolerance: edge frames can solve from neighbouring scales); if nothing
        // survives, the prior ladder does not match the database at all, so sweep the
        // database's own range instead.
        let props = self.properties();
        let (lo, hi) = (props.min_fov_deg * 0.8, props.max_fov_deg * 1.25);
        let presets = with_focal_hint(base, frame.width, frame.height, presets);
        let mut presets: Vec<FovPreset> = presets
            .iter()
            .copied()
            .filter(|p| (lo..=hi).contains(&p.fov_deg))
            .collect();
        let informed = first_rung_informed(&presets, frame.width, frame.height);
        if presets.is_empty() {
            presets = db_range_ladder(props.min_fov_deg, props.max_fov_deg);
        }
        // Staged search over the rungs (see `search`), extracting once
        let t_total = Instant::now();
        let (w, h) = (frame.width, frame.height);
        let mut cache = ExtractCache::default();
        let mut attempts = Vec::new();
        let mut last: Option<SolveOutcome> = None;
        let passes =
            crate::search::schedule(presets.len(), base.timeout_ms, base.thorough, informed);
        crate::search::run(&passes, |pass, first| {
            let p = presets[pass.rung];
            let mut o = base.clone();
            o.fov_estimate_deg = p.fov_deg;
            o.fov_max_error_deg = Some(p.max_error_deg);
            o.timeout_ms = pass.timeout_ms;
            let cfg = pass_config(&o, w, h, pass)?;
            let ext = cache.get(frame, &o.extraction.resolve(), &self.pool)?;
            let refine = Refine::from_opts(&o);
            let (mut out, _) = self.solve_extracted(&ext, &cfg, w, h, t_total, refine)?;
            // Profile retry, once per rung and only on TooFew: inside a ladder NoMatch more
            // likely means a wrong FOV
            if first
                && matches!(out.status, SolveStatus::TooFew)
                && o.retry_alternate_profile
                && o.attitude_hint.is_none()
            {
                if let Some(alt) = o.extraction.alternate() {
                    let ext2 = cache.get(frame, &alt.resolve(), &self.pool)?;
                    let (out2, _) = self.solve_extracted(&ext2, &cfg, w, h, t_total, refine)?;
                    if matches!(out2.status, SolveStatus::Ok) {
                        out = out2;
                    }
                    out.extraction_retried = true;
                    out.timing.total_ms = t_total.elapsed().as_secs_f32() * 1000.0;
                }
            }
            attempts.push(FovAttempt {
                fov_deg: p.fov_deg,
                status: out.status,
                solve_ms: out.timing.solve_ms,
            });
            let ok = matches!(out.status, SolveStatus::Ok);
            last = Some(out);
            Ok(ok)
        })?;
        let mut out = last.expect("presets non-empty");
        out.observation_unix_ms = base.observation_unix_ms;
        Ok((out, attempts))
    }
}

/// Fallback ladder over the database range: from max × 0.75 down to min. A 0.75 step
/// with ±20% tolerance leaves no gaps (rung f covers [0.8f, 1.2f]; 0.75f reaches 0.9f).
pub(crate) fn db_range_ladder(min_fov_deg: f32, max_fov_deg: f32) -> Vec<FovPreset> {
    let mut v = Vec::new();
    let mut f = max_fov_deg;
    while f > min_fov_deg {
        v.push(FovPreset {
            fov_deg: f,
            max_error_deg: (f * 0.2).max(0.5),
        });
        f *= 0.75;
    }
    v.push(FovPreset {
        fov_deg: min_fov_deg,
        max_error_deg: (min_fov_deg * 0.2).max(0.5),
    });
    v
}

/// Aspect-aware default FOV ladder (rungs validated on 47 phone frames; shared by the
/// CLI and Flutter). Landscape tries main-camera horizontal spans first, portrait the short side.
pub fn aspect_ladder(width: u32, height: u32) -> Vec<FovPreset> {
    let landscape = width >= height;
    let mut v = if landscape {
        vec![
            FovPreset {
                fov_deg: 70.0,
                max_error_deg: 9.0,
            },
            FovPreset {
                fov_deg: 55.0,
                max_error_deg: 8.0,
            },
            FovPreset {
                fov_deg: 42.0,
                max_error_deg: 7.0,
            },
        ]
    } else {
        vec![
            FovPreset {
                fov_deg: 46.0,
                max_error_deg: 7.0,
            },
            FovPreset {
                fov_deg: 60.0,
                max_error_deg: 8.0,
            },
            FovPreset {
                fov_deg: 33.0,
                max_error_deg: 6.0,
            },
        ]
    };
    v.extend([
        FovPreset {
            fov_deg: 20.0,
            max_error_deg: 5.0,
        },
        FovPreset {
            fov_deg: 13.0,
            max_error_deg: 3.5,
        },
    ]);
    v
}

/// A rung around a FOV read from metadata: ±15%, because headers are hints, never truth.
pub(crate) fn hint_preset(fov_deg: f32) -> FovPreset {
    FovPreset {
        fov_deg,
        max_error_deg: fov_deg * 0.15,
    }
}

/// Hint rung for a 35 mm-equivalent focal length (EXIF FocalLengthIn35mmFilm) on a frame
/// of this size: the CIPA diagonal convention gives the diagonal FOV, the aspect ratio the
/// horizontal one. None for an implausible focal length or a FOV outside (0.2°, 120°).
pub fn focal_35mm_hint(mm: f32, width: u32, height: u32) -> Option<FovPreset> {
    let cam = CameraParams::from_equivalent_focal_35mm(mm as f64, width, height).ok()?;
    let fov = cam.horizontal_fov_deg(width);
    (0.2..=120.0)
        .contains(&fov)
        .then(|| hint_preset(fov as f32))
}

/// Whether `presets` starts with an informed guess: a first rung that is no rung of the
/// built-in ladder for this frame shape came from a header, a focal-length hint or the
/// caller, and the staged search probes it longer (`search::HINTED_PROBE_PATTERNS`).
pub(crate) fn first_rung_informed(presets: &[FovPreset], width: u32, height: u32) -> bool {
    let ladder = aspect_ladder(width, height);
    presets
        .first()
        .is_some_and(|p| !ladder.iter().any(|q| same_rung(p, q)))
}

pub(crate) fn same_rung(a: &FovPreset, b: &FovPreset) -> bool {
    a.fov_deg == b.fov_deg && a.max_error_deg == b.max_error_deg
}

/// Hint rungs first, then `ladder` without the rungs within 1° of a hint. Hints are only
/// hints: when they fail, the rest of the ladder still runs.
pub fn ladder_after_hints(hints: &[FovPreset], ladder: &[FovPreset]) -> Vec<FovPreset> {
    let mut out = hints.to_vec();
    for p in ladder {
        if !out.iter().any(|q| (q.fov_deg - p.fov_deg).abs() < 1.0) {
            out.push(*p);
        }
    }
    out
}

/// `presets` with the caller's 35 mm focal-length hint (`SolveOptions::focal_length_35mm`)
/// in front, when there is one.
pub(crate) fn with_focal_hint(
    base: &SolveOptions,
    width: u32,
    height: u32,
    presets: &[FovPreset],
) -> Vec<FovPreset> {
    match base
        .focal_length_35mm
        .and_then(|mm| focal_35mm_hint(mm, width, height))
    {
        Some(h) => ladder_after_hints(&[h], presets),
        None => presets.to_vec(),
    }
}

/// Header hint rungs (possibly none) placed before the aspect ladder; rungs within 1°
/// are deduplicated. Headers are hints: after they fail, the ladder still runs.
#[cfg(feature = "imageio")]
pub fn presets_with_hints(
    meta: &crate::imageio::ImageMeta,
    width: u32,
    height: u32,
) -> Vec<FovPreset> {
    ladder_after_hints(&meta.solve_hints(), &aspect_ladder(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The built-in ladder is an uninformed guess; a hint in front of it, or a caller's own
    /// FOV, is informed. A hint 0.02° off a ladder rung replaces that rung and still counts.
    #[test]
    fn informed_first_rungs() {
        let (w, h) = (720, 1280);
        let ladder = aspect_ladder(w, h);
        assert!(!first_rung_informed(&ladder, w, h));
        assert!(!first_rung_informed(&ladder[1..], w, h));
        assert!(!first_rung_informed(&[], w, h));
        let hinted = ladder_after_hints(&[hint_preset(45.98)], &ladder);
        assert!(first_rung_informed(&hinted, w, h));
        let own = [FovPreset {
            fov_deg: 70.0,
            max_error_deg: 9.0,
        }];
        assert!(first_rung_informed(&own, w, h));
        // A portrait frame's ladder starts at 46°: 70° ±9° is a rung only of the landscape one
        assert!(!first_rung_informed(&own, h, w));
    }

    /// The probe reaches upstream as a pattern budget (the same search on every machine);
    /// the other passes keep upstream's backstop.
    #[test]
    fn probes_carry_their_pattern_budget() {
        let o = SolveOptions::new(40.0);
        let passes = crate::search::schedule(3, Some(5000), false, false);
        let cfg = |i: usize| pass_config(&o, 800, 600, &passes[i]).unwrap();
        assert_eq!(
            cfg(3).max_patterns_checked,
            Some(crate::search::DEEP_PROBE_PATTERNS)
        );
        assert_eq!(
            cfg(0).max_patterns_checked,
            Some(SolveConfig::DEFAULT_MAX_PATTERNS_CHECKED)
        );
    }

    /// The narrow-field options are off unless asked for, and pointing hints are checked
    #[test]
    fn narrow_options_default_off_and_pointing_hints_are_checked() {
        let o = SolveOptions::new(2.0);
        assert!(o.pointing_hint.is_none() && !o.narrow_blind);
        assert_eq!(o.narrow_fallback_ms, 2000);
        let ok = PointingHint {
            ra_deg: 83.8,
            dec_deg: -5.4,
            radius_deg: None,
        };
        assert!(ok.validate().is_ok());
        assert!(PointingHint {
            radius_deg: Some(2.0),
            ..ok
        }
        .validate()
        .is_ok());
        for bad in [
            PointingHint {
                dec_deg: 91.0,
                ..ok
            },
            PointingHint {
                ra_deg: f64::NAN,
                ..ok
            },
            PointingHint {
                radius_deg: Some(0.0),
                ..ok
            },
            PointingHint {
                radius_deg: Some(f64::INFINITY),
                ..ok
            },
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
    }
}
