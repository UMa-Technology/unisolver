//! Staged search over a FOV ladder: cheap passes first, expensive ones on a short budget.
//!
//! Upstream (0.13) builds patterns from the brightest 24 centroids by default. On phone
//! frames the brightest often include hot pixels and light-pollution blobs, so some real
//! frames need more; but searching every rung with every centroid is what makes a frame
//! without stars expensive, as each rung enumerates patterns until its timeout. The
//! schedule spends effort where the prior puts the FOV:
//!
//! 1. the first (likeliest) rung with the brightest 28 centroids;
//! 2. the next two rungs with the brightest 24;
//! 3. the first rung again with every centroid, for 100 ms (400 ms when that rung is an
//!    informed guess: a header or focal-length hint, or the caller's own FOV);
//! 4. the remaining rungs with the brightest 24;
//! 5. with `thorough`, every rung with every centroid and the full timeout (the old search).
//!
//! The numbers come from replaying per-rung timings of 47 real phone frames and stacked
//! frames: every frame that solved before still solves, frames without stars fail in under
//! two seconds instead of 10–16, and `thorough` keeps the exhaustive search for callers who
//! would rather wait than miss. A borderline frame needs 65–120 ms in the probe depending on
//! the FOV estimate, so a hint 0.02° off the ladder's rung tipped it out of 100 ms: an
//! informed rung is usually right, and a frame without stars that carries one fails 0.3 s
//! later instead.

/// Pattern stars for the first rung.
pub(crate) const FIRST_RUNG_PATTERN_STARS: u32 = 28;
/// Pattern stars for the quick sweep of the other rungs.
pub(crate) const SWEEP_PATTERN_STARS: u32 = 24;
/// Rungs that get the quick sweep before the deep probe.
pub(crate) const SWEEP_RUNGS: usize = 3;
/// Rungs that get a deep (all-centroid) probe: the likeliest only. Probing the next two as
/// well rescued no frame in the measurements and added 200 ms to every failure.
pub(crate) const DEEP_PROBE_RUNGS: usize = 1;
/// Budget of each deep probe.
pub(crate) const DEEP_PROBE_MS: u64 = 100;
/// Budget of the deep probe when the first rung is an informed guess
pub(crate) const HINTED_PROBE_MS: u64 = 400;

/// One attempt of the schedule: which rung, how many pattern stars, how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pass {
    pub rung: usize,
    pub pattern_stars: u32,
    pub timeout_ms: Option<u64>,
}

/// The staged schedule over `rungs` ladder rungs (see the module docs); `informed` when the
/// first rung is an informed guess rather than the built-in ladder's.
pub(crate) fn schedule(
    rungs: usize,
    timeout_ms: Option<u64>,
    thorough: bool,
    informed: bool,
) -> Vec<Pass> {
    let pass = |rung, pattern_stars, timeout_ms| Pass {
        rung,
        pattern_stars,
        timeout_ms,
    };
    if rungs == 0 {
        return Vec::new();
    }
    let sweep = SWEEP_RUNGS.min(rungs);
    let budget = if informed {
        HINTED_PROBE_MS
    } else {
        DEEP_PROBE_MS
    };
    let probe = Some(timeout_ms.map_or(budget, |t| t.min(budget)));
    let mut v = vec![pass(0, FIRST_RUNG_PATTERN_STARS, timeout_ms)];
    v.extend((1..sweep).map(|r| pass(r, SWEEP_PATTERN_STARS, timeout_ms)));
    v.extend((0..DEEP_PROBE_RUNGS.min(rungs)).map(|r| pass(r, u32::MAX, probe)));
    v.extend((sweep..rungs).map(|r| pass(r, SWEEP_PATTERN_STARS, timeout_ms)));
    if thorough {
        v.extend((0..rungs).map(|r| pass(r, u32::MAX, timeout_ms)));
    }
    v
}

/// Runs `passes` until one solves. `attempt(pass, first_visit)` performs one attempt,
/// records it and says whether it solved; `first_visit` is true the first time a rung is
/// tried (profile retries happen only then).
pub(crate) fn run<F>(passes: &[Pass], mut attempt: F) -> crate::Result<bool>
where
    F: FnMut(&Pass, bool) -> crate::Result<bool>,
{
    let mut visited = Vec::new();
    for pass in passes {
        if visited.len() <= pass.rung {
            visited.resize(pass.rung + 1, false);
        }
        let first = !visited[pass.rung];
        visited[pass.rung] = true;
        if attempt(pass, first)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(rung: usize, pattern_stars: u32, timeout_ms: Option<u64>) -> Pass {
        Pass {
            rung,
            pattern_stars,
            timeout_ms,
        }
    }

    #[test]
    fn five_rungs() {
        let t = Some(4000);
        assert_eq!(
            schedule(5, t, false, false),
            [
                p(0, 28, t),
                p(1, 24, t),
                p(2, 24, t),
                p(0, u32::MAX, Some(100)),
                p(3, 24, t),
                p(4, 24, t),
            ]
        );
    }

    #[test]
    fn thorough_appends_the_exhaustive_search() {
        let t = Some(4000);
        let s = schedule(5, t, true, false);
        assert_eq!(s.len(), 6 + 5);
        assert_eq!(&s[..6], schedule(5, t, false, false).as_slice());
        for (i, pass) in s[6..].iter().enumerate() {
            assert_eq!(*pass, p(i, u32::MAX, t));
        }
    }

    #[test]
    fn run_stops_at_the_first_solve_and_flags_first_visits() {
        let passes = schedule(5, Some(4000), false, false);
        let mut seen = Vec::new();
        let solved = run(&passes, |p, first| {
            seen.push((p.rung, first));
            Ok(p.rung == 3)
        })
        .unwrap();
        assert!(solved);
        assert_eq!(
            seen,
            [(0, true), (1, true), (2, true), (0, false), (3, true)]
        );
        assert!(!run(&passes, |_, _| Ok(false)).unwrap());
    }

    #[test]
    fn short_ladders_and_budgets() {
        assert_eq!(
            schedule(1, Some(4000), false, false),
            [p(0, 28, Some(4000)), p(0, u32::MAX, Some(100))]
        );
        // A caller budget below the probe budget caps the probe; no budget means 100 ms
        assert_eq!(schedule(1, Some(50), false, false)[1].timeout_ms, Some(50));
        assert_eq!(schedule(1, None, false, false)[1].timeout_ms, Some(100));
        assert!(schedule(0, Some(4000), true, false).is_empty());
    }

    #[test]
    fn an_informed_first_rung_is_probed_longer() {
        let t = Some(4000);
        let s = schedule(5, t, false, true);
        assert_eq!(s[3], p(0, u32::MAX, Some(400)));
        // Everything else is the uninformed schedule
        let plain = schedule(5, t, false, false);
        assert_eq!(s.len(), plain.len());
        assert!(s
            .iter()
            .zip(&plain)
            .enumerate()
            .all(|(i, (a, b))| i == 3 || a == b));
        // The caller's budget still caps it
        assert_eq!(schedule(1, Some(250), false, true)[1].timeout_ms, Some(250));
        assert_eq!(schedule(1, None, false, true)[1].timeout_ms, Some(400));
    }
}
