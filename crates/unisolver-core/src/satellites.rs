//! Satellite positions (feature = "satellites"): TLE + SGP4 propagation → topocentric RA/Dec.
//!
//! Accuracy, stated plainly: TEME is treated as J2000 (under 0.4°, fine for
//! annotation), GMST follows Meeus 12.4 and parallax assumes a spherical Earth.
//! Geometry is tested for magnitude; comparison against observed passes is pending.
use crate::{CoreError, Result};

#[derive(Debug, Clone)]
pub struct SatellitePos {
    pub name: String,
    pub ra_deg: f64,
    pub dec_deg: f64,
    /// Topocentric range (km) and whether it is above the horizon (below it, do not draw)
    pub range_km: f64,
    pub above_horizon: bool,
}

use crate::ephemeris::observer_eci;
/// Shares the observer model with the moon's parallax correction (`ephemeris`) so there
/// is one GMST, not two drifting apart.
pub use crate::ephemeris::Observer;

/// Propagates TLE text (two- or three-line sets, any number) to `unix_ms` and returns
/// topocentric positions.
pub fn satellite_positions(
    tle_text: &str,
    unix_ms: i64,
    observer: &Observer,
) -> Result<Vec<SatellitePos>> {
    let mut out = Vec::new();
    let lines: Vec<&str> = tle_text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let mut i = 0;
    while i < lines.len() {
        let (name, l1, l2) = if lines[i].starts_with('1') && i + 1 < lines.len() {
            ("SAT".to_string(), lines[i], lines[i + 1])
        } else if i + 2 < lines.len() {
            (lines[i].to_string(), lines[i + 1], lines[i + 2])
        } else {
            break;
        };
        let advance = if lines[i].starts_with('1') { 2 } else { 3 };
        i += advance;
        // The next line must be a "2 " line; otherwise this is not a TLE (an error, not zero satellites)
        if !l1.starts_with('1') || !l2.starts_with('2') {
            return Err(CoreError::InvalidInput(format!(
                "not a TLE record near line {}: expected '1 ...' / '2 ...'",
                i.saturating_sub(advance) + 1
            )));
        }

        let elements = sgp4::Elements::from_tle(Some(name.clone()), l1.as_bytes(), l2.as_bytes())
            .map_err(|e| CoreError::InvalidInput(format!("TLE parse: {e}")))?;
        let constants = sgp4::Constants::from_elements(&elements)
            .map_err(|e| CoreError::InvalidInput(format!("SGP4 init: {e}")))?;
        // TLE epoch → minutes since epoch
        let epoch_unix_ms = {
            let dt = elements.datetime;
            // sgp4 2.x exposes a chrono-free datetime; convert via years since J2000
            let jd = sgp4::julian_years_since_j2000(&dt) * 365.25 * 86_400_000.0;
            (jd as i64) + 946_728_000_000
        };
        let minutes = (unix_ms - epoch_unix_ms) as f64 / 60_000.0;
        let pred = constants
            .propagate(sgp4::MinutesSinceEpoch(minutes))
            .map_err(|e| CoreError::InvalidInput(format!("SGP4 propagate: {e}")))?;
        let sat = pred.position; // TEME, km

        let obs = observer_eci(observer, unix_ms);
        let g = [sat[0] - obs[0], sat[1] - obs[1], sat[2] - obs[2]];
        let range = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt();
        let ra = g[1].atan2(g[0]).to_degrees().rem_euclid(360.0);
        let dec = (g[2] / range).asin().to_degrees();
        // Above the horizon when the topocentric and observer vectors are < 90° apart
        let obs_norm = (obs[0] * obs[0] + obs[1] * obs[1] + obs[2] * obs[2]).sqrt();
        let cos_zenith = (g[0] * obs[0] + g[1] * obs[1] + g[2] * obs[2]) / (range * obs_norm);
        out.push(SatellitePos {
            name,
            ra_deg: ra,
            dec_deg: dec,
            range_km: range,
            above_horizon: cos_zenith > 0.0,
        });
    }
    // Text that yields no satellite is an error: an empty list would read as "no passes
    // today" when the TLE is actually wrong.
    if out.is_empty() && !lines.is_empty() {
        return Err(CoreError::InvalidInput(format!(
            "no TLE record found in {} non-blank lines",
            lines.len()
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Historical ISS TLE (around 2024-01-01), fixed for geometry-magnitude tests
    const ISS: &str = "ISS (ZARYA)
1 25544U 98067A   24001.50000000  .00016717  00000-0  30777-3 0  9991
2 25544  51.6400 208.9163 0006317  69.9862 290.2117 15.49560538429085";

    /// Garbage is an error, not "zero satellites" (which would read as "no passes today")
    #[test]
    fn garbage_text_is_an_error_not_an_empty_list() {
        let obs = Observer {
            lat_deg: 0.0,
            lon_deg: 0.0,
            alt_m: 0.0,
        };
        for bad in [
            "not a TLE at all\nreally not",
            "NAME\n1 25544U 98067A   24001.5 .0001 0 9991\nnope not line two",
            "<html>404</html>",
        ] {
            assert!(
                satellite_positions(bad, 0, &obs).is_err(),
                "should reject: {bad}"
            );
        }
        // Empty text means "none given", not an error
        assert!(satellite_positions("", 0, &obs).unwrap().is_empty());
        assert!(satellite_positions("   \n\n", 0, &obs).unwrap().is_empty());
    }

    #[test]
    fn iss_geometry_is_sane() {
        // TLE epoch = 2024-001.5 = 2024-01-01T12:00Z
        let t = 1_704_110_400_000i64;
        let obs = Observer {
            lat_deg: 31.2,
            lon_deg: 121.5,
            alt_m: 10.0,
        }; // Shanghai
        let sats = satellite_positions(ISS, t, &obs).unwrap();
        assert_eq!(sats.len(), 1);
        let s = &sats[0];
        // LEO range bounds: ≲2400 km above the horizon, up to ~13,200 km below (antipodal)
        assert!(
            s.range_km > 350.0 && s.range_km < 13_500.0,
            "range {}",
            s.range_km
        );
        if s.above_horizon {
            assert!(
                s.range_km < 2_500.0,
                "above-horizon LEO must be near: {}",
                s.range_km
            );
        }
        assert!((-90.0..=90.0).contains(&s.dec_deg));
        // 45 minutes later it has moved far (half an LEO orbit ≈ 46 min)
        let later = satellite_positions(ISS, t + 45 * 60 * 1000, &obs).unwrap();
        let d =
            ((s.ra_deg - later[0].ra_deg).abs()).min(360.0 - (s.ra_deg - later[0].ra_deg).abs());
        assert!(d > 10.0, "moved {d}°");
    }
}
