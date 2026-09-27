//! Coordinate grids over a solved frame: equatorial (J2000 right ascension and declination)
//! and horizontal (apparent altitude and azimuth, with the horizon). Lines are pixel
//! polylines through the lens model; each line carries one label anchor on the edge of the
//! visible region. Spacing follows the display scale (about `spacing_px` screen pixels
//! between lines), so a zoomed view gets a finer grid.
use crate::sky::{self, View};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridSystem {
    /// J2000 right ascension and declination
    Equatorial,
    /// Apparent altitude and azimuth (azimuth from north through east), refraction included
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridKind {
    /// Line of constant right ascension
    Ra,
    /// Line of constant declination
    Dec,
    /// Line of constant altitude
    Alt,
    /// Line of constant azimuth
    Az,
    /// Altitude 0°
    Horizon,
}

/// Which edge of the visible region a label sits on, so the app can nudge the text inward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridEdge {
    Left,
    Right,
    Top,
    Bottom,
    /// The line never leaves the region (a small circle round a pole): mid-line
    Inside,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GridLabel {
    pub x: f64,
    pub y: f64,
    /// Direction of the line there, counter-clockwise from +x (image y points down), folded
    /// into (−90°, 90°] so text drawn along it is never upside down
    pub angle_deg: f64,
    pub edge: GridEdge,
}

/// One grid line (all its visible stretches) with its value and label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridLineAnnotation {
    pub system: GridSystem,
    pub kind: GridKind,
    /// Right ascension or azimuth in [0, 360), declination or altitude in degrees
    pub value_deg: f64,
    /// The value as charts print it: `16h30m`, `−20°30′`, `180°`
    pub text: String,
    /// Compass point for azimuths on a multiple of 45° (`N`, `NE`, … `NW`)
    pub cardinal: Option<String>,
    /// Pixel polylines (top-left origin); they may run past the region edge
    pub lines: Vec<Vec<[f64; 2]>>,
    /// Where to write `text`; None when the line only grazes the region
    pub label: Option<GridLabel>,
}

/// Grid steps in degrees, fine to coarse (degrees, then arcminutes, then arcseconds)
const DMS_STEPS: [f64; 18] = [
    1.0 / 3600.0,
    2.0 / 3600.0,
    5.0 / 3600.0,
    10.0 / 3600.0,
    20.0 / 3600.0,
    30.0 / 3600.0,
    1.0 / 60.0,
    2.0 / 60.0,
    5.0 / 60.0,
    10.0 / 60.0,
    20.0 / 60.0,
    30.0 / 60.0,
    1.0,
    2.0,
    5.0,
    10.0,
    20.0,
    30.0,
];
/// Right-ascension steps in seconds of time, fine to coarse
const RA_STEPS_S: [f64; 15] = [
    1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1200.0, 1800.0, 3600.0, 7200.0,
    10800.0,
];
/// Azimuth steps: as declination, with 15° and 45° for the compass
const AZ_STEPS: [f64; 19] = [
    1.0 / 3600.0,
    2.0 / 3600.0,
    5.0 / 3600.0,
    10.0 / 3600.0,
    20.0 / 3600.0,
    30.0 / 3600.0,
    1.0 / 60.0,
    2.0 / 60.0,
    5.0 / 60.0,
    10.0 / 60.0,
    20.0 / 60.0,
    30.0 / 60.0,
    1.0,
    2.0,
    5.0,
    10.0,
    15.0,
    30.0,
    45.0,
];
/// Screen pixels between grid lines when the app does not say
pub(crate) const DEFAULT_SPACING_PX: f64 = 150.0;

/// The finest step at least `target` degrees apart (the coarsest when none is)
fn pick(steps: impl IntoIterator<Item = f64>, target: f64) -> f64 {
    let mut last = 0.0;
    for s in steps {
        last = s;
        if s >= target {
            return s;
        }
    }
    last
}

/// Degrees of sky per screen pixel at the view's scale
fn deg_per_screen_px(view: &View) -> f64 {
    view.wcs().scale_arcsec_per_px() / 3600.0 / view.scale()
}

fn fmt_ra(deg: f64, step: f64) -> String {
    let total = (deg.rem_euclid(360.0) * 240.0).round() as i64 % 86_400;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if step >= 15.0 || (m == 0 && s == 0) {
        format!("{h}h")
    } else if step >= 0.25 || s == 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{h}h{m:02}m{s:02}s")
    }
}

/// `+20°`, `−20°30′`, `0°`; unsigned for altitude and azimuth
fn fmt_dms(deg: f64, step: f64, signed: bool) -> String {
    let total = (deg.abs() * 3600.0).round() as i64;
    let (d, m, s) = (total / 3600, total / 60 % 60, total % 60);
    let sign = match (signed, total == 0, deg < 0.0) {
        (false, _, _) | (_, true, _) => "",
        (true, false, true) => "−",
        (true, false, false) => "+",
    };
    if step >= 1.0 || (m == 0 && s == 0) {
        format!("{sign}{d}°")
    } else if step >= 1.0 / 60.0 || s == 0 {
        format!("{sign}{d}°{m:02}′")
    } else {
        format!("{sign}{d}°{m:02}′{s:02}″")
    }
}

fn cardinal(az: f64) -> Option<String> {
    let k = az / 45.0;
    ((k - k.round()).abs() < 1e-9)
        .then(|| ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][(k.round() as usize) % 8].to_string())
}

/// Evenly spaced values `lo..=hi` with `n` intervals
fn span(lo: f64, hi: f64, max_step: f64) -> impl Iterator<Item = f64> {
    let n = (((hi - lo) / max_step).ceil() as usize).clamp(1, 20_000);
    (0..=n).map(move |i| lo + (hi - lo) * i as f64 / n as f64)
}

/// Longitude half-width (degrees) of the view's bounding cap at latitude `lat_c`, or None when
/// the cap holds a pole (every longitude is in view)
fn lon_half_width(lat_c: f64, limit_deg: f64) -> Option<f64> {
    if limit_deg >= 89.9 || 90.0 - lat_c.abs() <= limit_deg {
        return None;
    }
    Some(
        (limit_deg.to_radians().sin() / lat_c.to_radians().cos())
            .clamp(-1.0, 1.0)
            .asin()
            .to_degrees(),
    )
}

/// Where a segment crosses the region border (Liang–Barsky): the entry and exit points, each
/// with its edge. A long simplified segment can cross the region with both ends outside.
fn crossings(view: &View, a: [f64; 2], b: [f64; 2]) -> Vec<([f64; 2], GridEdge)> {
    let [x0, y0, x1, y1] = view.region();
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let (mut e0, mut e1) = (None, None);
    for (p, q, edge_in, edge_out) in [
        (-dx, a[0] - x0, GridEdge::Left, GridEdge::Left),
        (dx, x1 - a[0], GridEdge::Right, GridEdge::Right),
        (-dy, a[1] - y0, GridEdge::Top, GridEdge::Top),
        (dy, y1 - a[1], GridEdge::Bottom, GridEdge::Bottom),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return Vec::new(); // parallel to this edge and outside it
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            if t > t1 {
                return Vec::new();
            }
            if t > t0 {
                (t0, e0) = (t, Some(edge_in));
            }
        } else {
            if t < t0 {
                return Vec::new();
            }
            if t < t1 {
                (t1, e1) = (t, Some(edge_out));
            }
        }
    }
    let at = |t: f64| [a[0] + dx * t, a[1] + dy * t];
    [(t0, e0), (t1, e1)]
        .into_iter()
        .filter_map(|(t, e)| e.map(|e| (at(t), e)))
        .collect()
}

/// Image angle of the segment a → b, folded into (−90°, 90°]
fn text_angle(a: [f64; 2], b: [f64; 2]) -> f64 {
    let deg = (-(b[1] - a[1])).atan2(b[0] - a[0]).to_degrees();
    if deg > 90.0 {
        deg - 180.0
    } else if deg <= -90.0 {
        deg + 180.0
    } else {
        deg
    }
}

/// One label per line: where it crosses the region border, on the edge the kind prefers
/// (meridians label along the bottom, parallels along the left), else mid-line
fn label(view: &View, kind: GridKind, lines: &[Vec<[f64; 2]>]) -> Option<GridLabel> {
    let order: [GridEdge; 4] = match kind {
        GridKind::Ra | GridKind::Az => [
            GridEdge::Bottom,
            GridEdge::Top,
            GridEdge::Left,
            GridEdge::Right,
        ],
        _ => [
            GridEdge::Left,
            GridEdge::Right,
            GridEdge::Bottom,
            GridEdge::Top,
        ],
    };
    let mut found: Vec<GridLabel> = Vec::new();
    for l in lines {
        for w in l.windows(2) {
            for (p, edge) in crossings(view, w[0], w[1]) {
                found.push(GridLabel {
                    x: p[0],
                    y: p[1],
                    angle_deg: text_angle(w[0], w[1]),
                    edge,
                });
            }
        }
    }
    for e in order {
        if let Some(l) = found.iter().find(|l| l.edge == e) {
            return Some(*l);
        }
    }
    // Entirely inside: mid-line of the longest stretch
    let l = lines.iter().max_by_key(|l| l.len())?;
    if !l.iter().all(|&p| view.contains(p)) {
        return None;
    }
    let i = l.len() / 2;
    Some(GridLabel {
        x: l[i][0],
        y: l[i][1],
        angle_deg: text_angle(l[i.saturating_sub(1)], l[(i + 1).min(l.len() - 1)]),
        edge: GridEdge::Inside,
    })
}

#[allow(clippy::too_many_arguments)]
fn line(
    view: &View,
    system: GridSystem,
    kind: GridKind,
    value_deg: f64,
    text: String,
    cardinal: Option<String>,
    units: Vec<[f64; 3]>,
    out: &mut Vec<GridLineAnnotation>,
) {
    let lines = view.project_units(&units);
    if lines.is_empty() {
        return;
    }
    let label = label(view, kind, &lines);
    out.push(GridLineAnnotation {
        system,
        kind,
        value_deg,
        text,
        cardinal,
        lines,
        label,
    });
}

/// Equatorial grid (J2000) over the view.
pub(crate) fn equatorial(view: &View, spacing_px: f64) -> Vec<GridLineAnnotation> {
    let (cra, cdec) = sky::radec(view.centre());
    let limit = view.limit().to_degrees();
    let step = view.step().to_degrees();
    let target = spacing_px * deg_per_screen_px(view);
    let dec_step = pick(DMS_STEPS, target);
    let ra_step = pick(
        RA_STEPS_S.iter().map(|s| s / 240.0),
        target / cdec.to_radians().cos().max(0.05),
    );
    let (dmin, dmax) = ((cdec - limit).max(-90.0), (cdec + limit).min(90.0));
    let (ra0, ra1) = match lon_half_width(cdec, limit) {
        None => (0.0, 360.0),
        Some(hw) => (cra - hw, cra + hw),
    };
    let mut out = Vec::new();
    let mut k = (dmin / dec_step).ceil() as i64;
    while k as f64 * dec_step <= dmax {
        let dec = k as f64 * dec_step;
        if dec.abs() < 90.0 - 1e-9 {
            let d_ra = step / dec.to_radians().cos().max(1e-3);
            let units = span(ra0, ra1, d_ra).map(|ra| sky::unit(ra, dec)).collect();
            line(
                view,
                GridSystem::Equatorial,
                GridKind::Dec,
                dec,
                fmt_dms(dec, dec_step, true),
                None,
                units,
                &mut out,
            );
        }
        k += 1;
    }
    let full = ra1 - ra0 >= 360.0 - 1e-9;
    let (k0, k1) = if full {
        (0, (360.0 / ra_step).round() as i64 - 1)
    } else {
        (
            (ra0 / ra_step).ceil() as i64,
            (ra1 / ra_step).floor() as i64,
        )
    };
    for k in k0..=k1 {
        let ra = (k as f64 * ra_step).rem_euclid(360.0);
        let units = span(dmin, dmax, step)
            .map(|dec| sky::unit(ra, dec))
            .collect();
        line(
            view,
            GridSystem::Equatorial,
            GridKind::Ra,
            ra,
            fmt_ra(ra, ra_step),
            None,
            units,
            &mut out,
        );
    }
    out
}

/// Observer-frame conversions for one instant and place: J2000 ⇄ apparent (alt, az)
struct Horizon {
    precession: [[f64; 3]; 3],
    lst_deg: f64,
    lat: f64,
}

impl Horizon {
    fn new(unix_ms: i64, observer: &crate::Observer) -> Self {
        let t = crate::days_since_j2000(unix_ms) / 36_525.0;
        Self {
            precession: sky::precession_j2000_to(t),
            lst_deg: crate::ephemeris::gmst_deg(unix_ms) + observer.lon_deg,
            lat: observer.lat_deg.to_radians(),
        }
    }

    /// J2000 direction → apparent (altitude, azimuth) in degrees
    fn alt_az(&self, v: [f64; 3]) -> (f64, f64) {
        let (ra, dec) = sky::radec(sky::rotate(&self.precession, v));
        let (h, d, phi) = ((self.lst_deg - ra).to_radians(), dec.to_radians(), self.lat);
        let alt = (phi.sin() * d.sin() + phi.cos() * d.cos() * h.cos())
            .clamp(-1.0, 1.0)
            .asin()
            .to_degrees();
        let az = (-h.sin() * d.cos())
            .atan2(phi.cos() * d.sin() - phi.sin() * d.cos() * h.cos())
            .to_degrees()
            .rem_euclid(360.0);
        (alt + sky::refraction_at_true(alt), az)
    }

    /// Apparent (altitude, azimuth) in degrees → J2000 direction
    fn direction(&self, alt_app: f64, az: f64) -> [f64; 3] {
        let alt = (alt_app - sky::refraction_at_apparent(alt_app)).to_radians();
        let (a, phi) = (az.to_radians(), self.lat);
        let dec = (phi.sin() * alt.sin() + phi.cos() * alt.cos() * a.cos())
            .clamp(-1.0, 1.0)
            .asin();
        let h =
            (-alt.cos() * a.sin()).atan2(alt.sin() * phi.cos() - alt.cos() * phi.sin() * a.cos());
        let ra = self.lst_deg - h.to_degrees();
        sky::rotate(
            &sky::transpose(&self.precession),
            sky::unit(ra, dec.to_degrees()),
        )
    }
}

/// Horizontal grid (apparent altitude above the horizon, and azimuth) over the view.
pub(crate) fn horizontal(
    view: &View,
    spacing_px: f64,
    unix_ms: i64,
    observer: &crate::Observer,
) -> Vec<GridLineAnnotation> {
    let hz = Horizon::new(unix_ms, observer);
    let (alt_c, az_c) = hz.alt_az(view.centre());
    let limit = view.limit().to_degrees();
    let step = view.step().to_degrees();
    let target = spacing_px * deg_per_screen_px(view);
    let alt_step = pick(DMS_STEPS, target);
    let az_step = pick(AZ_STEPS, target / alt_c.to_radians().cos().max(0.05));
    let (amin, amax) = ((alt_c - limit).max(0.0), (alt_c + limit).min(90.0));
    if amax <= amin {
        return Vec::new(); // looking at the ground
    }
    let (az0, az1) = match lon_half_width(alt_c, limit) {
        None => (0.0, 360.0),
        Some(hw) => (az_c - hw, az_c + hw),
    };
    let mut out = Vec::new();
    let mut k = (amin / alt_step).ceil() as i64;
    while k as f64 * alt_step <= amax {
        let alt = k as f64 * alt_step;
        if alt < 90.0 - 1e-9 {
            let d_az = step / alt.to_radians().cos().max(1e-3);
            let units = span(az0, az1, d_az)
                .map(|az| hz.direction(alt, az))
                .collect();
            let kind = if k == 0 {
                GridKind::Horizon
            } else {
                GridKind::Alt
            };
            line(
                view,
                GridSystem::Horizontal,
                kind,
                alt,
                fmt_dms(alt, alt_step, false),
                None,
                units,
                &mut out,
            );
        }
        k += 1;
    }
    let full = az1 - az0 >= 360.0 - 1e-9;
    let (k0, k1) = if full {
        (0, (360.0 / az_step).round() as i64 - 1)
    } else {
        (
            (az0 / az_step).ceil() as i64,
            (az1 / az_step).floor() as i64,
        )
    };
    for k in k0..=k1 {
        let az = (k as f64 * az_step).rem_euclid(360.0);
        let units = span(amin, amax, step)
            .map(|alt| hz.direction(alt, az))
            .collect();
        line(
            view,
            GridSystem::Horizontal,
            GridKind::Az,
            az,
            fmt_dms(az, az_step, false),
            cardinal(az),
            units,
            &mut out,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_print_as_charts_print_them() {
        assert_eq!(fmt_ra(240.0, 15.0), "16h");
        assert_eq!(fmt_ra(247.5, 7.5), "16h30m");
        assert_eq!(fmt_ra(247.5 + 20.0 / 240.0, 5.0 / 240.0), "16h30m20s");
        assert_eq!(fmt_ra(359.99999, 15.0), "0h");
        assert_eq!(fmt_dms(-20.0, 10.0, true), "−20°");
        assert_eq!(fmt_dms(-20.5, 0.5, true), "−20°30′");
        assert_eq!(
            fmt_dms(20.0 + 10.0 / 3600.0, 10.0 / 3600.0, true),
            "+20°00′10″"
        );
        assert_eq!(fmt_dms(0.0, 10.0, true), "0°");
        assert_eq!(fmt_dms(180.0, 15.0, false), "180°");
        assert_eq!(cardinal(180.0).as_deref(), Some("S"));
        assert_eq!(cardinal(315.0).as_deref(), Some("NW"));
        assert_eq!(cardinal(150.0), None);
    }

    #[test]
    fn steps_are_the_finest_at_least_the_target() {
        assert_eq!(pick(DMS_STEPS, 7.0), 10.0);
        assert_eq!(pick(DMS_STEPS, 0.3), 20.0 / 60.0);
        assert_eq!(pick(DMS_STEPS, 100.0), 30.0);
        assert_eq!(pick(RA_STEPS_S.iter().map(|s| s / 240.0), 10.0), 15.0);
    }

    /// Against astropy (ICRS → AltAz with refraction at 1010 hPa, 10 °C), Beijing, the
    /// Scorpius frame's instant. Nutation and aberration are left out (about 20″ each), so 1′.
    #[test]
    fn alt_az_matches_astropy() {
        let obs = crate::Observer {
            lat_deg: 39.9,
            lon_deg: 116.4,
            alt_m: 0.0,
        };
        let hz = Horizon::new(1_781_018_520_519, &obs);
        for (ra, dec, alt, az) in [
            (247.35192, -26.43200, 23.59228, 177.21509),
            (279.23473, 38.78369, 63.43597, 81.12151),
            (37.95456, 89.26411, 39.32503, 0.2615),
        ] {
            let (a, z) = hz.alt_az(sky::unit(ra, dec));
            // Compare as directions: azimuth near the pole is not a fair measure
            let sep = sky::angle(sky::unit(z, a), sky::unit(az, alt)).to_degrees() * 60.0;
            assert!(
                sep < 1.0,
                "({ra}, {dec}) → alt {a} az {z}, astropy {alt} {az}: {sep:.2}′"
            );
        }
    }

    #[test]
    fn horizon_round_trips() {
        let obs = crate::Observer {
            lat_deg: 39.9,
            lon_deg: 116.4,
            alt_m: 0.0,
        };
        let hz = Horizon::new(1_781_018_520_519, &obs);
        for (alt, az) in [(0.5, 10.0), (20.0, 180.0), (45.0, 270.0), (80.0, 33.0)] {
            let (a2, z2) = hz.alt_az(hz.direction(alt, az));
            assert!(
                (a2 - alt).abs() * 3600.0 < 5.0 && (z2 - az).abs() * 3600.0 < 5.0,
                "{alt},{az} → {a2},{z2}"
            );
        }
    }
}
