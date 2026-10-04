//! When the pool hands a frame to the narrow-field engine. The tetra3 tiers keep their routing
//! plan exactly as it was; the engine is one extra step before or after that plan, and only
//! for frames whose FOV is known to be narrow (or on request). Phone frames, frames without
//! stars and frames of unknown FOV fail exactly as fast as without the engine.
//!
//! - Known FOV no tetra3 tier covers (narrower than every tier): the engine goes first, on
//!   half the timeout, then the tetra3 plan as before (headers are hints, not truth).
//! - Known FOV a tetra3 tier covers, up to [`NARROW_MAX_FOV_DEG`]: after every tetra3 attempt
//!   failed, on `narrow_fallback_ms` at most.
//! - Unknown FOV: only with `narrow_blind`, after the tetra3 plan, over the engine's range.
//! - Never for tracking (an attitude hint), wider FOVs, or FOVs below the index's floor.
//!
//! A FOV is known from a calibrated camera or an informed first rung (a header or focal-length
//! hint, or the caller's own FOV); the tetra3 plan itself treats the latter as a hint only.
use super::{NarrowInfo, NarrowMode};
use crate::{FovPreset, SolveOptions};

/// Widest FOV handed to the narrow-field engine, degrees: a 2.5° tetra3 tier with the pool's
/// range tolerance (×1.25). Wider frames belong to the tetra3 tiers alone.
pub const NARROW_MAX_FOV_DEG: f32 = 3.125;
/// Relative FOV tolerance of a calibrated camera (the engine still searches a scale range)
const CAMERA_FOV_TOLERANCE: f64 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum When {
    /// No tetra3 tier covers the FOV: the engine first, then the tetra3 plan
    BeforeTetra3,
    /// After every tetra3 attempt failed
    AfterTetra3,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NarrowStep {
    pub when: When,
    pub mode: NarrowMode,
    /// Time the engine gets, ms; None for no limit
    pub budget_ms: Option<u64>,
    /// FOV recorded for the attempt, degrees
    pub fov_deg: f32,
}

/// A FOV the narrow step trusts, with its relative tolerance
#[derive(Debug, Clone, Copy)]
struct KnownFov {
    fov_deg: f32,
    tolerance: f64,
}

fn known_fov(
    base: &SolveOptions,
    ladder: &[FovPreset],
    width: u32,
    height: u32,
) -> Option<KnownFov> {
    if let Some(c) = &base.camera {
        return Some(KnownFov {
            fov_deg: c.horizontal_fov_deg(width) as f32,
            tolerance: CAMERA_FOV_TOLERANCE,
        });
    }
    let first = ladder.first()?;
    crate::solver::first_rung_informed(ladder, width, height).then(|| KnownFov {
        fov_deg: first.fov_deg,
        tolerance: (first.max_error_deg / first.fov_deg) as f64,
    })
}

/// The narrow step of one solve, or None when the engine must not run (see the module docs).
/// `ladder` is the tetra3 ladder with its hints first, `spans` the tetra3 tiers' FOV ranges.
pub(crate) fn route(
    base: &SolveOptions,
    ladder: &[FovPreset],
    width: u32,
    height: u32,
    spans: &[(f32, f32)],
    info: &NarrowInfo,
) -> Option<NarrowStep> {
    if base.attitude_hint.is_some() {
        return None;
    }
    let ceiling = NARROW_MAX_FOV_DEG.min(info.max_fov_deg);
    let (when, mode, fov_deg) = match known_fov(base, ladder, width, height) {
        Some(k) => {
            if k.fov_deg > ceiling || k.fov_deg < info.min_fov_deg * 0.8 {
                return None;
            }
            let covered = spans.iter().any(|&s| crate::pool::covers(s, k.fov_deg));
            let when = if covered {
                When::AfterTetra3
            } else {
                When::BeforeTetra3
            };
            (when, mode_for(base, k), k.fov_deg)
        }
        None if base.narrow_blind => (
            When::AfterTetra3,
            NarrowMode::Blind {
                min_fov_deg: info.min_fov_deg as f64,
                max_fov_deg: ceiling as f64,
            },
            (info.min_fov_deg * ceiling).sqrt(),
        ),
        None => return None,
    };
    let budget_ms = match when {
        When::BeforeTetra3 => base.timeout_ms.map(|t| t / 2),
        When::AfterTetra3 if base.narrow_fallback_ms == 0 => return None,
        When::AfterTetra3 => Some(
            base.timeout_ms
                .map_or(base.narrow_fallback_ms, |t| t.min(base.narrow_fallback_ms)),
        ),
    };
    Some(NarrowStep {
        when,
        mode,
        budget_ms,
        fov_deg,
    })
}

/// Hinted around the pointing hint when there is one, blind over the FOV's tolerance otherwise.
/// The default search radius is one FOV: the hinted search finds a field within about 0.75 FOV
/// of the hint in tens of milliseconds, and scans its radius for one further away (about 0.5 s
/// for a radius of one FOV, 3 s for three, on real sub-degree and 3° frames), so a wider default
/// would only delay the blind search that takes over.
fn mode_for(base: &SolveOptions, k: KnownFov) -> NarrowMode {
    let fov = k.fov_deg as f64;
    match base.pointing_hint {
        Some(p) => NarrowMode::Hinted {
            ra_deg: p.ra_deg,
            dec_deg: p.dec_deg,
            radius_deg: p.radius_deg.unwrap_or(fov),
            fov_deg: fov,
            fov_tolerance: k.tolerance,
        },
        None => NarrowMode::Blind {
            min_fov_deg: fov * (1.0 - k.tolerance),
            max_fov_deg: fov * (1.0 + k.tolerance),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solver::{aspect_ladder, hint_preset, ladder_after_hints, with_focal_hint};

    /// Typical tetra3 tiers: 10–80°, 5–10°, 2.5–5°, 1–2.5°
    const SPANS: [(f32, f32); 4] = [(10.0, 80.0), (5.0, 10.0), (2.5, 5.0), (1.0, 2.5)];
    const W: u32 = 4000;
    const H: u32 = 3000;

    fn info() -> NarrowInfo {
        NarrowInfo {
            name: "pkg".into(),
            min_fov_deg: 0.18,
            max_fov_deg: 12.0,
            num_patterns: 0,
            num_stars: 0,
            index_mag_limit: 16.0,
        }
    }

    /// A header FOV hint in front of the aspect ladder (±15%)
    fn header(fov: f32) -> Vec<FovPreset> {
        ladder_after_hints(&[hint_preset(fov)], &aspect_ladder(W, H))
    }

    fn base() -> SolveOptions {
        SolveOptions::new(70.0) // timeout 5000 ms
    }

    #[test]
    fn unknown_fov_runs_only_on_request_after_tetra3() {
        let ladder = aspect_ladder(W, H);
        assert_eq!(route(&base(), &ladder, W, H, &SPANS, &info()), None);
        let mut o = base();
        o.narrow_blind = true;
        let s = route(&o, &ladder, W, H, &SPANS, &info()).unwrap();
        assert_eq!(s.when, When::AfterTetra3);
        assert_eq!(
            s.mode,
            NarrowMode::Blind {
                min_fov_deg: 0.18f32 as f64,
                max_fov_deg: 3.125
            }
        );
        assert_eq!(s.budget_ms, Some(2000));
    }

    #[test]
    fn phone_frames_and_tracking_never_run() {
        let mut o = base();
        o.focal_length_35mm = Some(26.0);
        o.narrow_blind = true;
        let ladder = with_focal_hint(&o, W, H, &aspect_ladder(W, H));
        assert_eq!(route(&o, &ladder, W, H, &SPANS, &info()), None);

        let mut o = base();
        o.camera = Some(crate::CameraParams::from_horizontal_fov(0.5, W, H).unwrap());
        o.attitude_hint = Some([1.0, 0.0, 0.0, 0.0]);
        o.narrow_blind = true;
        assert_eq!(route(&o, &header(0.5), W, H, &SPANS, &info()), None);
    }

    #[test]
    fn an_uncovered_narrow_fov_goes_first_on_half_the_timeout() {
        let s = route(&base(), &header(0.5), W, H, &SPANS, &info()).unwrap();
        assert_eq!(s.when, When::BeforeTetra3);
        assert_eq!(s.budget_ms, Some(2500));
        assert_eq!(s.fov_deg, 0.5);
        let NarrowMode::Blind {
            min_fov_deg,
            max_fov_deg,
        } = s.mode
        else {
            panic!("{:?}", s.mode)
        };
        assert!((min_fov_deg - 0.425).abs() < 1e-6 && (max_fov_deg - 0.575).abs() < 1e-6);
        let mut o = base();
        o.timeout_ms = None;
        assert_eq!(
            route(&o, &header(0.5), W, H, &SPANS, &info())
                .unwrap()
                .budget_ms,
            None
        );
        // Without tetra3 tiers every narrow FOV goes first
        let s = route(&base(), &header(1.5), W, H, &[], &info()).unwrap();
        assert_eq!(s.when, When::BeforeTetra3);
    }

    #[test]
    fn a_covered_fov_falls_back_after_tetra3_within_the_cap() {
        let s = route(&base(), &header(1.5), W, H, &SPANS, &info()).unwrap();
        assert_eq!(s.when, When::AfterTetra3);
        assert_eq!(s.budget_ms, Some(2000));
        let mut o = base();
        o.timeout_ms = Some(1200);
        assert_eq!(
            route(&o, &header(1.5), W, H, &SPANS, &info())
                .unwrap()
                .budget_ms,
            Some(1200)
        );
        o.timeout_ms = None;
        assert_eq!(
            route(&o, &header(1.5), W, H, &SPANS, &info())
                .unwrap()
                .budget_ms,
            Some(2000)
        );
        o.narrow_fallback_ms = 0;
        assert_eq!(route(&o, &header(1.5), W, H, &SPANS, &info()), None);
        // The ceiling: 3.0° still falls back, 4.0° never runs
        assert_eq!(
            route(&base(), &header(3.0), W, H, &SPANS, &info())
                .unwrap()
                .when,
            When::AfterTetra3
        );
        assert_eq!(route(&base(), &header(4.0), W, H, &SPANS, &info()), None);
    }

    #[test]
    fn below_the_index_floor_is_left_to_tetra3() {
        assert_eq!(route(&base(), &header(0.1), W, H, &SPANS, &info()), None);
    }

    #[test]
    fn a_calibrated_camera_counts_as_a_known_fov() {
        let mut o = base();
        o.camera = Some(crate::CameraParams::from_horizontal_fov(0.7, W, H).unwrap());
        let s = route(&o, &aspect_ladder(W, H), W, H, &SPANS, &info()).unwrap();
        assert_eq!(s.when, When::BeforeTetra3);
        let NarrowMode::Blind {
            min_fov_deg,
            max_fov_deg,
        } = s.mode
        else {
            panic!("{:?}", s.mode)
        };
        assert!((min_fov_deg - 0.665).abs() < 1e-3 && (max_fov_deg - 0.735).abs() < 1e-3);
    }

    #[test]
    fn a_pointing_hint_switches_to_the_hinted_search() {
        let mut o = base();
        o.pointing_hint = Some(crate::PointingHint {
            ra_deg: 83.8,
            dec_deg: -5.4,
            radius_deg: None,
        });
        let s = route(&o, &header(1.5), W, H, &SPANS, &info()).unwrap();
        assert_eq!(s.when, When::AfterTetra3, "a hint changes no routing");
        let NarrowMode::Hinted {
            ra_deg,
            radius_deg,
            fov_deg,
            fov_tolerance,
            ..
        } = s.mode
        else {
            panic!("{:?}", s.mode)
        };
        assert_eq!(ra_deg, 83.8);
        assert!((radius_deg - 1.5).abs() < 1e-6, "one FOV");
        assert!((fov_deg - 1.5).abs() < 1e-6 && (fov_tolerance - 0.15).abs() < 1e-6);
        let NarrowMode::Hinted { radius_deg, .. } =
            route(&o, &header(0.2), W, H, &SPANS, &info()).unwrap().mode
        else {
            panic!()
        };
        assert!((radius_deg - 0.2).abs() < 1e-6, "one FOV");
        o.pointing_hint.as_mut().unwrap().radius_deg = Some(0.7);
        let NarrowMode::Hinted { radius_deg, .. } =
            route(&o, &header(1.5), W, H, &SPANS, &info()).unwrap().mode
        else {
            panic!()
        };
        assert_eq!(radius_deg, 0.7);
    }
}
