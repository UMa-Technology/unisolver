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
use crate::solver::{build_solve_config_with, db_range_ladder, ExtractCache};
use crate::{CoreError, FovPreset, Frame, Result, Solver};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

/// Public information about a registered tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierInfo {
    /// Tier name = file name without extension (e.g. `unisolver_10_80`), same as the manifest `name`
    pub name: String,
    pub path: String,
    pub min_fov_deg: f32,
    pub max_fov_deg: f32,
    pub num_stars: u64,
    pub num_patterns: u32,
    pub star_max_magnitude: f32,
}

/// One cross-tier attempt (a `FovAttempt` plus the database it used).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolAttempt {
    /// Database used for this attempt
    pub db: String,
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

struct Tier {
    info: TierInfo,
    solver: Solver,
}

/// A pool of tier databases. Registration is explicit (`register` / `open_dir`); the pool
/// never downloads anything, which is the integration layer's job (Flutter `DbManager`).
pub struct SolverPool {
    rayon: Arc<rayon::ThreadPool>,
    tiers: Vec<Tier>,
}

impl std::fmt::Debug for SolverPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolverPool")
            .field(
                "tiers",
                &self
                    .tiers
                    .iter()
                    .map(|t| {
                        format!(
                            "{} [{:.1}–{:.1}°]",
                            t.info.name, t.info.min_fov_deg, t.info.max_fov_deg
                        )
                    })
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
        })
    }

    /// Registers every `*.db` in `dir`, ordered wide to narrow. A file that fails to open
    /// is skipped and listed in `skipped`, so one bad file does not sink the pool; but if
    /// **none** opens it is an error rather than an empty pool that never solves.
    pub fn open_dir(dir: &str) -> Result<(Self, Vec<(String, String)>)> {
        let mut pool = Self::new()?;
        let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| CoreError::InvalidInput(format!("read_dir {dir}: {e}")))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("db")))
            .collect();
        files.sort();
        let mut skipped = Vec::new();
        for f in &files {
            let path = f.to_string_lossy().to_string();
            if let Err(e) = pool.register(&path) {
                skipped.push((path, e.to_string()));
            }
        }
        if pool.tiers.is_empty() {
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
        let key = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.to_string());
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

    pub fn tiers(&self) -> Vec<TierInfo> {
        self.tiers.iter().map(|t| t.info.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.tiers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiers.is_empty()
    }

    /// The underlying `Solver` by name (annotation uses the tier that solved: narrow tiers are denser).
    pub fn solver(&self, name: &str) -> Option<&Solver> {
        self.tiers
            .iter()
            .find(|t| t.info.name == name)
            .map(|t| &t.solver)
    }

    /// Solves a frame without naming a database. `hints` is the single-database ladder
    /// (header hints + aspect ladder, from `presets_with_hints` / `aspect_ladder`); the
    /// pool dispatches it by tier range and adds a range sweep for tiers it never reached.
    ///
    /// A calibrated camera or tracking (with attitude_hint) **does not sweep**: the FOV is
    /// known, so only tiers covering it are tried.
    pub fn solve_auto(
        &self,
        frame: &Frame,
        base: &crate::SolveOptions,
        hints: &[FovPreset],
    ) -> Result<PoolOutcome> {
        if self.tiers.is_empty() {
            return Err(CoreError::InvalidInput(
                "solver pool is empty: register a database first".into(),
            ));
        }
        let t_total = Instant::now();
        let (w, h) = (frame.width, frame.height);
        let spans: Vec<(f32, f32)> = self
            .tiers
            .iter()
            .map(|t| (t.info.min_fov_deg, t.info.max_fov_deg))
            .collect();

        let steps: Vec<(usize, Option<FovPreset>)> = match known_fov(base, w) {
            Some(fov) => {
                let mut cands = covering(&spans, fov);
                if cands.is_empty() {
                    // Out of every range: let the nearest tier try once rather than
                    // fail outright (the tolerance is conservative; edge frames often solve).
                    cands = vec![nearest(&spans, fov)];
                }
                cands.into_iter().map(|i| (i, None)).collect()
            }
            None => plan(&spans, hints)
                .into_iter()
                .map(|(i, p)| (i, Some(p)))
                .collect(),
        };

        // Unknown FOV: the staged search over the plan (see `search`). Known FOV: each
        // covering tier once, with every centroid and the full timeout.
        let passes: Vec<Pass> = if steps.iter().all(|(_, p)| p.is_some()) {
            search::schedule(steps.len(), base.timeout_ms, base.thorough)
        } else {
            (0..steps.len())
                .map(|rung| Pass {
                    rung,
                    pattern_stars: u32::MAX,
                    timeout_ms: base.timeout_ms,
                })
                .collect()
        };

        let luma = frame.to_luma_f32()?;
        let mut cache = ExtractCache::default();
        let mut attempts: Vec<PoolAttempt> = Vec::with_capacity(passes.len());
        let mut last: Option<SolveOutcome> = None;
        let mut solved_by: Option<String> = None;

        search::run(&passes, |pass, first| {
            let (ti, preset) = steps[pass.rung];
            let tier = &self.tiers[ti];
            let mut o = base.clone();
            if let Some(p) = preset {
                o.fov_estimate_deg = p.fov_deg;
                o.fov_max_error_deg = Some(p.max_error_deg);
            }
            o.timeout_ms = pass.timeout_ms;
            let cfg = build_solve_config_with(&o, w, h, pass.pattern_stars)?;
            let ext = cache.get(&luma, w, h, &o.extraction.resolve(), &self.rayon)?;
            let (mut out, _) = tier.solver.solve_extracted(&ext, &cfg, w, h, t_total)?;

            // Profile retry, as in the single-database ladder: once per step, only on TooFew
            // (NoMatch more likely means a wrong FOV); tracking and Custom never retry.
            if first
                && matches!(out.status, SolveStatus::TooFew)
                && o.retry_alternate_profile
                && o.attitude_hint.is_none()
            {
                if let Some(alt) = o.extraction.alternate() {
                    let ext2 = cache.get(&luma, w, h, &alt.resolve(), &self.rayon)?;
                    let (out2, _) = tier.solver.solve_extracted(&ext2, &cfg, w, h, t_total)?;
                    if matches!(out2.status, SolveStatus::Ok) {
                        out = out2;
                    }
                    out.extraction_retried = true;
                    out.timing.total_ms = t_total.elapsed().as_secs_f32() * 1000.0;
                }
            }

            attempts.push(PoolAttempt {
                db: tier.info.name.clone(),
                fov_deg: o.fov_estimate_deg,
                status: out.status,
                solve_ms: out.timing.solve_ms,
            });
            let ok = matches!(out.status, SolveStatus::Ok);
            if ok {
                solved_by = Some(tier.info.name.clone());
            }
            last = Some(out);
            Ok(ok)
        })?;
        Ok(PoolOutcome {
            outcome: last.ok_or_else(|| {
                CoreError::InvalidInput("no tier covers this frame (empty routing plan)".into())
            })?,
            attempts,
            db: solved_by,
            extract_count: cache.len(),
        })
    }

    /// Fully automatic file entry: load any of the five formats → header hints + aspect ladder → route.
    #[cfg(feature = "imageio")]
    pub fn solve_image_file_auto(
        &self,
        path: &str,
        base: &crate::SolveOptions,
    ) -> Result<PoolOutcome> {
        let (frame, meta) = crate::imageio::load_image(path)?;
        let hints = crate::presets_with_hints(&meta, frame.width, frame.height);
        self.solve_auto(&frame, base, &hints)
    }
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
fn covers(span: (f32, f32), fov: f32) -> bool {
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
///    every tier covering it, closest-centred first;
/// 2. Fill-in: tiers never reached in step 1 sweep their own range (`db_range_ladder`),
///    **to themselves only**; otherwise each narrow tier would re-sweep the wide ones;
/// 3. No (database, FOV) pair twice.
fn plan(spans: &[(f32, f32)], hints: &[FovPreset]) -> Vec<(usize, FovPreset)> {
    let mut steps: Vec<(usize, FovPreset)> = Vec::new();
    let dup = |steps: &[(usize, FovPreset)], ti: usize, fov: f32| {
        steps
            .iter()
            .any(|(t, q)| *t == ti && (q.fov_deg - fov).abs() < 0.01 * fov.max(1.0))
    };
    for hp in hints {
        for i in covering(spans, hp.fov_deg) {
            if !dup(&steps, i, hp.fov_deg) {
                steps.push((i, *hp));
            }
        }
    }
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by(|&a, &b| spans[b].0.total_cmp(&spans[a].0));
    for i in order {
        if steps.iter().any(|(t, _)| *t == i) {
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
