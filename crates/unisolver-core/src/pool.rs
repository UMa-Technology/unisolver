//! Multi-tier routing: register several star databases and route each frame, by FOV,
//! to a tier that can solve it, stepping across tiers on failure. Callers no longer
//! need to know which tier to use.
//!
//! Three constraints:
//! 1. **The solve path is untouched**: routing only decides which database and which
//!    FOV to try; each attempt runs the same `build_solve_config` + `solve_extracted`
//!    as the single-database path.
//! 2. **Extract once**: extraction depends only on (luma, extraction profile), not on
//!    the database, while a cross-tier ladder can have a dozen rungs. On a 26 Mpx
//!    frame one extraction takes seconds.
//! 3. **One hot database at a time**: fill-in rungs are grouped by database (wide
//!    tiers first), not interleaved by FOV, to avoid paging between large mmaps.
use crate::outcome::{SolveOutcome, SolveStatus};
use crate::search::{self, Pass};
use crate::solver::{
    db_range_ladder, pass_config, range_rungs_reaching, with_focal_hint, ExtractCache,
};
use crate::{CoreError, FovPreset, Frame, Result, Solver};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

#[cfg(feature = "narrow")]
use crate::narrow::route::{NarrowStep, When};

/// What answers for a registered tier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TierKind {
    /// A tetra3 pattern database (`*.db`)
    #[default]
    Tetra3,
    /// The narrow-field engine: a blind index and its star tiles (desktop builds)
    Narrow,
}

/// Public information about a registered tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierInfo {
    /// Tier name = file name without extension (e.g. `unisolver_10_80`; for the narrow-field
    /// engine, its index file), same as the manifest `name`
    pub name: String,
    pub path: String,
    pub min_fov_deg: f32,
    pub max_fov_deg: f32,
    pub num_stars: u64,
    pub num_patterns: u32,
    pub star_max_magnitude: f32,
    /// tetra3 database or narrow-field engine
    #[serde(default)]
    pub kind: TierKind,
}

/// One cross-tier attempt (a `FovAttempt` plus the database it used).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolAttempt {
    /// Database used for this attempt
    pub db: String,
    /// Engine of the database used
    #[serde(default)]
    pub kind: TierKind,
    pub fov_deg: f32,
    pub status: SolveStatus,
    pub solve_ms: f32,
}

/// Result of a cross-tier solve.
pub struct PoolOutcome {
    pub outcome: SolveOutcome,
    /// Every attempt; on failure, the complete record of what was tried
    pub attempts: Vec<PoolAttempt>,
    /// Name of the database that solved it; None on failure
    pub db: Option<String>,
    /// Extractions actually performed (a dozen-rung ladder still extracts once or
    /// twice; the second only when retrying with the other profile)
    pub extract_count: usize,
}

/// What a solve has tried so far
#[derive(Default)]
struct Tally {
    attempts: Vec<PoolAttempt>,
    last: Option<SolveOutcome>,
    /// The tier that solved it
    db: Option<String>,
}

impl Tally {
    /// Records one attempt; true when it solved
    fn record(&mut self, info: &TierInfo, fov_deg: f32, out: SolveOutcome) -> bool {
        self.attempts.push(PoolAttempt {
            db: info.name.clone(),
            kind: info.kind,
            fov_deg,
            status: out.status,
            solve_ms: out.timing.solve_ms,
        });
        let ok = matches!(out.status, SolveStatus::Ok);
        if ok {
            self.db = Some(info.name.clone());
        }
        self.last = Some(out);
        ok
    }
}

struct Tier {
    info: TierInfo,
    solver: Solver,
}

#[cfg(feature = "narrow")]
struct NarrowTier {
    info: TierInfo,
    engine: crate::narrow::NarrowEngine,
}

/// A pool of tier databases. Registration is explicit (`register` / `open_dir`); the pool
/// never downloads anything, which is the integration layer's job (Flutter `DbManager`).
pub struct SolverPool {
    rayon: Arc<rayon::ThreadPool>,
    tiers: Vec<Tier>,
    /// The narrow-field engine: one package at most
    #[cfg(feature = "narrow")]
    narrow: Option<NarrowTier>,
}

impl std::fmt::Debug for SolverPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolverPool")
            .field(
                "tiers",
                &self
                    .tiers()
                    .iter()
                    .map(|t| format!("{} [{:.1}–{:.1}°]", t.name, t.min_fov_deg, t.max_fov_deg))
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl SolverPool {
    pub fn new() -> Result<Self> {
        Ok(Self {
            rayon: crate::solver::build_pool()?,
            tiers: Vec::new(),
            #[cfg(feature = "narrow")]
            narrow: None,
        })
    }

    /// Registers every `*.db` in `dir`, ordered wide to narrow, and the narrow-field package
    /// when there is one: its blind index and star tiles are recognized by their headers, never
    /// by name (one of each pairs up; otherwise files sharing a stem). A file that fails to open
    /// is skipped and listed in `skipped`, so one bad file does not sink the pool; but if
    /// **nothing** opens it is an error rather than an empty pool that never solves.
    pub fn open_dir(dir: &str) -> Result<(Self, Vec<(String, String)>)> {
        let mut pool = Self::new()?;
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| CoreError::InvalidInput(format!("read_dir {dir}: {e}")))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        files.sort();
        let mut skipped = Vec::new();
        let (mut indexes, mut tiles) = (Vec::new(), Vec::new());
        for f in files {
            // Downloads and decompressions in progress (or left by a crash) are not databases
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.ends_with(".tmp") || name.ends_with(".part") {
                continue;
            }
            if f.extension().is_some_and(|x| x.eq_ignore_ascii_case("db")) {
                let path = f.to_string_lossy().to_string();
                if let Err(e) = pool.register(&path) {
                    skipped.push((path, e.to_string()));
                }
                continue;
            }
            match magic(&f) {
                Some(m) if &m == NARROW_INDEX_MAGIC => indexes.push(f),
                Some(m) if &m == NARROW_TILE_MAGIC => tiles.push(f),
                _ => {}
            }
        }
        for (index, stars) in pair_narrow(indexes, tiles, &mut skipped) {
            let (index, stars) = (index.to_string_lossy(), stars.to_string_lossy());
            if let Err(e) = pool.register_narrow(&index, &stars) {
                skipped.push((index.to_string(), e.to_string()));
            }
        }
        if pool.is_empty() {
            let why = if skipped.is_empty() {
                format!("no *.db files in {dir}")
            } else {
                skipped
                    .iter()
                    .map(|(p, e)| format!("{p}: {e}"))
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            return Err(CoreError::InvalidInput(format!(
                "solver pool has no usable database ({why})"
            )));
        }
        Ok((pool, skipped))
    }

    /// Registers one tier. Registering the same path again is idempotent (returns the
    /// existing entry): install-then-register flows easily report a file twice.
    pub fn register(&mut self, path: &str) -> Result<TierInfo> {
        let key = canonical(path);
        if let Some(t) = self.tiers.iter().find(|t| t.info.path == key) {
            return Ok(t.info.clone());
        }
        let solver = Solver::from_file_with_pool(path, self.rayon.clone())?;
        let p = solver.properties();
        let name = std::path::Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| key.clone());
        let info = TierInfo {
            name,
            path: key,
            min_fov_deg: p.min_fov_deg,
            max_fov_deg: p.max_fov_deg,
            num_stars: p.num_stars as u64,
            num_patterns: p.num_patterns,
            star_max_magnitude: p.star_max_magnitude,
            kind: TierKind::Tetra3,
        };
        self.tiers.push(Tier {
            info: info.clone(),
            solver,
        });
        // Wide to narrow: both the plan's fill-in order and "wide tiers first" paging rely on it
        self.tiers
            .sort_by(|a, b| b.info.min_fov_deg.total_cmp(&a.info.min_fov_deg));
        Ok(info)
    }

    /// Registers the narrow-field engine: a blind index (`UNIBLIX1`) and its star tiles
    /// (`UNISTAR1`). Both are memory mapped and only their headers are read, so it is as quick as
    /// registering a tetra3 tier; a background thread then reads both files once, so the first
    /// solve does not wait on the disk (set `UNISOLVER_NO_PREFETCH=1` to skip it). One package
    /// per pool: registering the same
    /// index again returns it, another one is an error (open a new pool to replace it). Builds
    /// without the narrow-field engine (mobile) return an error.
    pub fn register_narrow(&mut self, index_path: &str, stars_path: &str) -> Result<TierInfo> {
        #[cfg(feature = "narrow")]
        {
            let key = canonical(index_path);
            if let Some(n) = &self.narrow {
                return if n.info.path == key {
                    Ok(n.info.clone())
                } else {
                    Err(CoreError::InvalidInput(format!(
                        "a narrow-field package is already registered ({}); open a new pool to replace it",
                        n.info.name
                    )))
                };
            }
            let engine = crate::narrow::NarrowEngine::open_with_pool(
                index_path,
                stars_path,
                self.rayon.clone(),
            )?;
            let i = engine.info();
            let info = TierInfo {
                name: i.name.clone(),
                path: key,
                min_fov_deg: i.min_fov_deg,
                max_fov_deg: crate::narrow::NARROW_MAX_FOV_DEG.min(i.max_fov_deg),
                num_stars: i.num_stars,
                num_patterns: i.num_patterns.min(u32::MAX as u64) as u32,
                star_max_magnitude: i.index_mag_limit,
                kind: TierKind::Narrow,
            };
            self.narrow = Some(NarrowTier {
                info: info.clone(),
                engine,
            });
            if std::env::var_os("UNISOLVER_NO_PREFETCH").is_none() {
                let files = vec![
                    std::path::PathBuf::from(index_path),
                    std::path::PathBuf::from(stars_path),
                ];
                // Registration stays as quick as before; a failed read only means a slower first solve
                let _ = std::thread::Builder::new()
                    .name("unisolver-prefetch".into())
                    .spawn(move || {
                        let _ = crate::narrow::prefetch(&files);
                    });
            }
            Ok(info)
        }
        #[cfg(not(feature = "narrow"))]
        {
            let _ = (index_path, stars_path);
            Err(CoreError::InvalidInput(
                "this build has no narrow-field engine (desktop builds only)".into(),
            ))
        }
    }

    /// Registered tiers: the tetra3 tiers wide to narrow, then the narrow-field engine
    pub fn tiers(&self) -> Vec<TierInfo> {
        #[allow(unused_mut)]
        let mut v: Vec<TierInfo> = self.tiers.iter().map(|t| t.info.clone()).collect();
        #[cfg(feature = "narrow")]
        v.extend(self.narrow.as_ref().map(|n| n.info.clone()));
        v
    }

    pub fn len(&self) -> usize {
        #[cfg(feature = "narrow")]
        let narrow = usize::from(self.narrow.is_some());
        #[cfg(not(feature = "narrow"))]
        let narrow = 0;
        self.tiers.len() + narrow
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The underlying `Solver` by name (annotation uses the tier that solved: narrow tiers are denser).
    pub fn solver(&self, name: &str) -> Option<&Solver> {
        self.tiers
            .iter()
            .find(|t| t.info.name == name)
            .map(|t| &t.solver)
    }

    /// The tetra3 tier to annotate a solve with: the tier named `db` (pass the one that solved,
    /// since narrow tiers have denser catalogs); for the narrow-field engine, which has no
    /// tetra3 catalog, the narrowest tetra3 tier; None picks the widest. None when there is no
    /// such tier.
    pub fn annotation_solver(&self, db: Option<&str>) -> Option<&Solver> {
        let Some(name) = db else {
            return self.tiers.first().map(|t| &t.solver);
        };
        if let Some(s) = self.solver(name) {
            return Some(s);
        }
        #[cfg(feature = "narrow")]
        if self.narrow.as_ref().is_some_and(|n| n.info.name == name) {
            return self.tiers.last().map(|t| &t.solver);
        }
        None
    }

    /// Solves a frame without naming a database. `hints` is the single-database ladder
    /// (header hints + aspect ladder, from `presets_with_hints` / `aspect_ladder`); the
    /// pool dispatches it by tier range and adds a range sweep for tiers it never reached.
    ///
    /// A calibrated camera or tracking (with attitude_hint) **does not sweep**: the FOV is
    /// known, so only tiers covering it are tried.
    ///
    /// With the narrow-field engine registered, it is one step before or after the tetra3
    /// plan, never inside it: see `narrow::route` for when it runs. Without it, or for frames it
    /// does not take, the tetra3 plan runs exactly as before.
    pub fn solve_auto(
        &self,
        frame: &Frame,
        base: &crate::SolveOptions,
        hints: &[FovPreset],
    ) -> Result<PoolOutcome> {
        if self.is_empty() {
            return Err(CoreError::InvalidInput(
                "solver pool is empty: register a database first".into(),
            ));
        }
        if let Some(p) = &base.pointing_hint {
            p.validate()?;
        }
        let t_total = Instant::now();
        let (w, h) = (frame.width, frame.height);
        let spans: Vec<(f32, f32)> = self
            .tiers
            .iter()
            .map(|t| (t.info.min_fov_deg, t.info.max_fov_deg))
            .collect();

        let ladder = with_focal_hint(base, w, h, hints);
        let steps: Vec<(usize, Option<FovPreset>)> = match known_fov(base, w) {
            Some(fov) => {
                let mut cands = covering(&spans, fov);
                if cands.is_empty() && !spans.is_empty() {
                    // Out of every range: let the nearest tier try once rather than
                    // fail outright (the tolerance is conservative; edge frames often solve).
                    cands = vec![nearest(&spans, fov)];
                }
                cands.into_iter().map(|i| (i, None)).collect()
            }
            None => plan(&spans, &ladder)
                .into_iter()
                .map(|(i, p)| (i, Some(p)))
                .collect(),
        };

        // Unknown FOV: the staged search over the plan (see `search`). Known FOV: each
        // covering tier once, with every centroid and the full timeout.
        let passes: Vec<Pass> = if steps.iter().all(|(_, p)| p.is_some()) {
            // The informed first rung counts only when the plan kept it first, itself or as
            // the sweep rungs that took its place (see `plan`)
            let informed = crate::solver::first_rung_informed(&ladder, w, h)
                && steps
                    .first()
                    .and_then(|(_, p)| *p)
                    .zip(ladder.first())
                    .is_some_and(|(a, b)| {
                        crate::solver::same_rung(&a, b)
                            || (covering(&spans, b.fov_deg).is_empty()
                                && (a.fov_deg - b.fov_deg).abs() <= a.max_error_deg)
                    });
            search::schedule(steps.len(), base.timeout_ms, base.thorough, informed)
        } else {
            (0..steps.len())
                .map(|rung| Pass {
                    rung,
                    pattern_stars: u32::MAX,
                    timeout_ms: base.timeout_ms,
                    max_patterns: None,
                })
                .collect()
        };

        let mut cache = ExtractCache::default();
        let mut tally = Tally::default();
        #[cfg(feature = "narrow")]
        let narrow = self.narrow.as_ref().and_then(|n| {
            crate::narrow::route::route(base, &ladder, w, h, &spans, n.engine.info())
                .map(|step| (n, step))
        });
        #[cfg(feature = "narrow")]
        if let Some((n, step)) = narrow
            .as_ref()
            .filter(|(_, s)| s.when == When::BeforeTetra3)
        {
            if let Some(out) = self.solve_narrow(n, step, frame, base, &mut cache, t_total)? {
                tally.record(&n.info, step.fov_deg, out);
            }
        }

        if tally.db.is_none() {
            search::run(&passes, |pass, first| {
                let (ti, preset) = steps[pass.rung];
                let tier = &self.tiers[ti];
                let mut o = base.clone();
                if let Some(p) = preset {
                    o.fov_estimate_deg = p.fov_deg;
                    o.fov_max_error_deg = Some(p.max_error_deg);
                }
                o.timeout_ms = pass.timeout_ms;
                let cfg = pass_config(&o, w, h, pass)?;
                let ext = cache.get(frame, &o.extraction.resolve(), &self.rayon)?;
                let refine = crate::solver::Refine::from_opts(&o);
                let (mut out, _) =
                    tier.solver
                        .solve_extracted(ext.pick(&o, w), &cfg, w, h, t_total, refine)?;

                // Profile retry, as in the single-database ladder: once per step, only on
                // TooFew (NoMatch more likely means a wrong FOV); tracking and Custom never retry.
                if first
                    && matches!(out.status, SolveStatus::TooFew)
                    && o.retry_alternate_profile
                    && o.attitude_hint.is_none()
                {
                    if let Some(alt) = o.extraction.alternate() {
                        let ext2 = cache.get(frame, &alt.resolve(), &self.rayon)?;
                        let (out2, _) = tier.solver.solve_extracted(
                            ext2.pick(&o, w),
                            &cfg,
                            w,
                            h,
                            t_total,
                            refine,
                        )?;
                        if matches!(out2.status, SolveStatus::Ok) {
                            out = out2;
                        }
                        out.extraction_retried = true;
                        out.timing.total_ms = t_total.elapsed().as_secs_f32() * 1000.0;
                    }
                }
                Ok(tally.record(&tier.info, o.fov_estimate_deg, out))
            })?;
        }

        #[cfg(feature = "narrow")]
        if tally.db.is_none() {
            if let Some((n, step)) = narrow.as_ref().filter(|(_, s)| s.when == When::AfterTetra3) {
                if let Some(out) = self.solve_narrow(n, step, frame, base, &mut cache, t_total)? {
                    tally.record(&n.info, step.fov_deg, out);
                }
            }
        }

        let mut outcome = tally.last.ok_or_else(|| {
            CoreError::InvalidInput("no tier covers this frame (empty routing plan)".into())
        })?;
        outcome.observation_unix_ms = base.observation_unix_ms;
        Ok(PoolOutcome {
            outcome,
            attempts: tally.attempts,
            db: tally.db,
            extract_count: cache.len(),
        })
    }

    /// One run of the narrow-field engine on the frame's richest extraction so far (extracting
    /// once when there is none); None when that has too few centroids for the engine, which
    /// then never runs.
    #[cfg(feature = "narrow")]
    fn solve_narrow(
        &self,
        n: &NarrowTier,
        step: &NarrowStep,
        frame: &Frame,
        base: &crate::SolveOptions,
        cache: &mut ExtractCache,
        t_total: Instant,
    ) -> Result<Option<SolveOutcome>> {
        use crate::narrow::{NarrowMode, NarrowRequest, BLIND_MIN_MATCHES, HINTED_MIN_MATCHES};
        let (extraction, retried) =
            cache.richest(frame, &base.extraction.resolve(), &self.rayon)?;
        // The narrow-field engine takes the brightest, as before
        let ext = &extraction.brightest;
        let floor = match step.mode {
            NarrowMode::Blind { .. } => BLIND_MIN_MATCHES,
            NarrowMode::Hinted { .. } => HINTED_MIN_MATCHES,
        };
        if ext.topleft.len() < floor {
            return Ok(None);
        }
        let req = NarrowRequest {
            mode: step.mode.clone(),
            deadline: step
                .budget_ms
                .map(|ms| Instant::now() + std::time::Duration::from_millis(ms)),
        };
        let r = n
            .engine
            .solve(&ext.topleft, frame.width, frame.height, &req);
        Ok(Some(SolveOutcome {
            status: r.status,
            solution: r.solution,
            centroids: ext.topleft.clone(),
            timing: crate::outcome::Timing {
                extract_ms: ext.extract_ms,
                solve_ms: r.solve_ms,
                total_ms: t_total.elapsed().as_secs_f32() * 1000.0,
            },
            extraction_retried: retried,
            median_elongation: ext.median_elongation,
            observation_unix_ms: None,
            observer: None,
        }))
    }

    /// Fully automatic file entry: load any of the five formats → header hints + aspect ladder
    /// → route. The header's observation time fills in when `base` has none, and so does its
    /// pointing (FITS/XISF RA/Dec, a hint for the narrow-field engine); the outcome carries the
    /// time and the header's place (EXIF GPS) for the annotator.
    #[cfg(feature = "imageio")]
    pub fn solve_image_file_auto(
        &self,
        path: &str,
        base: &crate::SolveOptions,
    ) -> Result<PoolOutcome> {
        let (frame, meta) = crate::imageio::load_image(path)?;
        let hints = crate::presets_with_hints(&meta, frame.width, frame.height);
        let mut base = base.clone();
        meta.apply_time(&mut base);
        meta.apply_pointing(&mut base);
        let mut r = self.solve_auto(&frame, &base, &hints)?;
        meta.apply_place(&mut r.outcome);
        Ok(r)
    }
}

/// A path as registered: canonical when it resolves, so the same file registers once
fn canonical(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string())
}

/// Headers of the narrow-field package's files (the engine's `blind::INDEX_MAGIC` and
/// `catalog::TILE_MAGIC`), spelled out so builds without the engine still recognize them
const NARROW_INDEX_MAGIC: &[u8; 8] = b"UNIBLIX1";
const NARROW_TILE_MAGIC: &[u8; 8] = b"UNISTAR1";

/// The first 8 bytes of a file (narrow-field files are recognized by their headers)
fn magic(path: &std::path::Path) -> Option<[u8; 8]> {
    use std::io::Read;
    let mut m = [0u8; 8];
    std::fs::File::open(path).ok()?.read_exact(&mut m).ok()?;
    Some(m)
}

/// Pairs blind indexes with star-tile files: one of each pair up, otherwise files sharing a
/// stem; the rest go into `skipped`
fn pair_narrow(
    indexes: Vec<std::path::PathBuf>,
    mut tiles: Vec<std::path::PathBuf>,
    skipped: &mut Vec<(String, String)>,
) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    if indexes.len() == 1 && tiles.len() == 1 {
        return vec![(indexes[0].clone(), tiles.remove(0))];
    }
    let mut pairs = Vec::new();
    for index in indexes {
        match tiles
            .iter()
            .position(|t| t.file_stem() == index.file_stem())
        {
            Some(i) => pairs.push((index, tiles.remove(i))),
            None => skipped.push((
                index.to_string_lossy().to_string(),
                "narrow-field index without a matching star-tile file".into(),
            )),
        }
    }
    for t in tiles {
        skipped.push((
            t.to_string_lossy().to_string(),
            "star-tile file without a matching narrow-field index".into(),
        ));
    }
    pairs
}

/// The two sources of a known FOV: a calibrated camera and tracking (previous attitude and FOV).
fn known_fov(base: &crate::SolveOptions, width: u32) -> Option<f32> {
    if let Some(c) = &base.camera {
        return Some(c.horizontal_fov_deg(width) as f32);
    }
    base.attitude_hint.map(|_| base.fov_estimate_deg)
}

/// Does the tier range (with tolerance) cover this FOV? Same tolerance as the clamp in
/// `solve_with_fov_presets` ([0.8×min, 1.25×max]): adjacent tiers meet without a gap.
pub(crate) fn covers(span: (f32, f32), fov: f32) -> bool {
    fov >= span.0 * 0.8 && fov <= span.1 * 1.25
}

/// Distance of a FOV from the tier's geometric centre: 10° is in range for both a 10–80°
/// and a 5–10° tier, and the 5–10° tier is closer, so it goes first. Log scale, as rungs are geometric.
fn fitness(span: (f32, f32), fov: f32) -> f32 {
    (fov / (span.0 * span.1).sqrt()).ln().abs()
}

fn covering(spans: &[(f32, f32)], fov: f32) -> Vec<usize> {
    let mut v: Vec<usize> = (0..spans.len())
        .filter(|&i| covers(spans[i], fov))
        .collect();
    v.sort_by(|&a, &b| fitness(spans[a], fov).total_cmp(&fitness(spans[b], fov)));
    v
}

fn nearest(spans: &[(f32, f32)], fov: f32) -> usize {
    (0..spans.len())
        .min_by(|&a, &b| fitness(spans[a], fov).total_cmp(&fitness(spans[b], fov)))
        .unwrap_or(0)
}

/// Routing plan `(database index, rung)` in the order tried:
/// 1. The caller's ladder (header hints first, aspect ladder after), each rung sent to
///    every tier covering it, closest-centred first. A rung no tier covers takes, in its
///    place, the rungs of a tier's own range sweep that reach it (`range_rungs_reaching`):
///    an upright 0.72° frame goes first to the 1–2.5° tier's 1° ± 0.5° rung, instead of
///    after the wide tier's ladder and every sweep;
/// 2. Fill-in: tiers no rung was routed to in step 1 sweep their own range
///    (`db_range_ladder`), **to themselves only**; otherwise each narrow tier would re-sweep
///    the wide ones. A tier that only lent rungs in step 1 still sweeps the rest, so those
///    rungs change the order of the attempts, not which ones run;
/// 3. No (database, FOV) pair twice.
fn plan(spans: &[(f32, f32)], hints: &[FovPreset]) -> Vec<(usize, FovPreset)> {
    let mut steps: Vec<(usize, FovPreset)> = Vec::new();
    let dup = |steps: &[(usize, FovPreset)], ti: usize, fov: f32| {
        steps
            .iter()
            .any(|(t, q)| *t == ti && (q.fov_deg - fov).abs() < 0.01 * fov.max(1.0))
    };
    let mut routed = vec![false; spans.len()];
    for hp in hints {
        let cands = covering(spans, hp.fov_deg);
        if cands.is_empty() {
            for (i, s) in spans.iter().enumerate() {
                for r in range_rungs_reaching(s.0, s.1, hp.fov_deg) {
                    if !dup(&steps, i, r.fov_deg) {
                        steps.push((i, r));
                    }
                }
            }
        }
        for i in cands {
            routed[i] = true;
            if !dup(&steps, i, hp.fov_deg) {
                steps.push((i, *hp));
            }
        }
    }
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by(|&a, &b| spans[b].0.total_cmp(&spans[a].0));
    for i in order {
        if routed[i] {
            continue;
        }
        for p in db_range_ladder(spans[i].0, spans[i].1) {
            if !dup(&steps, i, p.fov_deg) {
                steps.push((i, p));
            }
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "narrow")]
    #[test]
    fn narrow_headers_match_the_engine() {
        assert_eq!(
            super::NARROW_INDEX_MAGIC,
            unisolver_starmatch::blind::INDEX_MAGIC
        );
        assert_eq!(
            super::NARROW_TILE_MAGIC,
            unisolver_starmatch::catalog::TILE_MAGIC
        );
    }

    use super::*;

    /// Three tiers (10–80°, 5–10°, 2.5–5°) with a portrait phone ladder: the wide tier takes the ladder, narrow tiers fill in their own range.
    #[test]
    fn plan_routes_the_ladder_and_fills_in_unvisited_tiers() {
        let spans = [(10.0, 80.0), (5.0, 10.0), (2.5, 5.0)];
        let hints = crate::aspect_ladder(1080, 1920);
        let steps = plan(&spans, &hints);
        // Ladder rungs (46/60/33/20/13) all go to the wide tier; 13° is outside the 5–10° tier's tolerance (max 12.5)
        for (i, p) in steps.iter().take(hints.len()) {
            assert_eq!(*i, 0, "ladder step {p:?} should route to the wide tier");
        }
        // Both narrow tiers are filled in, wider before narrower
        let mid = steps
            .iter()
            .position(|(i, _)| *i == 1)
            .expect("5–10° tier visited");
        let narrow = steps
            .iter()
            .position(|(i, _)| *i == 2)
            .expect("2.5–5° tier visited");
        assert!(
            mid < narrow,
            "wide tiers must be swept first (mmap locality)"
        );
        // Fill-in rungs fall inside each tier's range
        for (i, p) in &steps {
            assert!(
                covers(spans[*i], p.fov_deg),
                "step {p:?} outside tier {i} span"
            );
        }
    }

    /// A header hint inside a narrow tier routes there directly, without sweeping the wide tier first.
    #[test]
    fn header_hint_routes_straight_to_the_matching_tier() {
        let spans = [(10.0, 80.0), (5.0, 10.0), (2.5, 5.0)];
        let mut hints = vec![FovPreset {
            fov_deg: 2.9,
            max_error_deg: 0.44,
        }];
        hints.extend(crate::aspect_ladder(6248, 4176));
        let steps = plan(&spans, &hints);
        assert_eq!(steps[0].0, 2, "2.9° hint must hit the 2.5–5° tier first");
        assert!((steps[0].1.fov_deg - 2.9).abs() < 1e-6);
    }

    /// A header hint narrower than every tier (an upright 1080×1920 frame at 250 mm with 2.9 µm
    /// pixels, 0.72° across) goes first to the 1–2.5° tier's sweep rungs that reach it, closest
    /// first. The attempts are the ones the plan without the hint makes, only earlier.
    #[test]
    fn hint_below_every_tier_moves_the_reaching_sweep_rungs_first() {
        let spans = [(10.0, 80.0), (5.0, 10.0), (2.5, 5.0), (1.0, 2.5)];
        let ladder = crate::aspect_ladder(1080, 1920);
        let hinted =
            crate::solver::ladder_after_hints(&[crate::solver::hint_preset(0.718)], &ladder);
        let steps = plan(&spans, &hinted);
        assert_eq!((steps[0].0, steps[0].1.fov_deg), (3, 1.0));
        assert_eq!((steps[1].0, steps[1].1.fov_deg), (3, 1.0546875));
        let attempts = |s: &[(usize, FovPreset)]| {
            let mut v: Vec<(usize, u32)> = s
                .iter()
                .map(|(i, p)| (*i, (p.fov_deg * 1e4).round() as u32))
                .collect();
            v.sort();
            v
        };
        assert_eq!(attempts(&steps), attempts(&plan(&spans, &ladder)));
    }

    /// Where ranges overlap (10° is within both tiers' tolerance) both are tried, the closer-centred first.
    #[test]
    fn overlapping_span_tries_the_better_centred_tier_first() {
        let spans = [(10.0, 80.0), (5.0, 10.0)];
        let hints = vec![FovPreset {
            fov_deg: 10.0,
            max_error_deg: 2.0,
        }];
        let steps = plan(&spans, &hints);
        assert_eq!(steps[0].0, 1, "the 5–10° tier is better centred on 10°");
        assert_eq!(steps[1].0, 0, "the wide tier still gets a try");
    }

    /// With a single tier the plan equals the single-database ladder (clamped, plus a range sweep).
    #[test]
    fn single_tier_plan_matches_the_single_db_ladder() {
        let spans = [(10.0, 80.0)];
        let hints = crate::aspect_ladder(4000, 3000);
        let steps = plan(&spans, &hints);
        assert!(steps.iter().all(|(i, _)| *i == 0));
        // 13° is within tolerance (10×0.8=8), so all 5 rungs stay
        assert_eq!(steps.len(), hints.len());
    }
}
