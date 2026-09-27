//! Satellite positions: TLE text comes from the caller (e.g. CelesTrak); **the engine never goes online**.
//!
//! The annotation layer can overlay satellites directly (`AnnotateOptionsDto.satelliteTle`),
//! projected to pixels and without those below the horizon. This function returns raw
//! topocentric positions, for pass lists and visibility checks that do not need a frame.
use crate::api::types::{ObserverDto, SatellitePosDto};
use anyhow::Result;
use unisolver_core as core;

pub fn satellite_positions(
    tle_text: String,
    unix_ms: i64,
    observer: ObserverDto,
) -> Result<Vec<SatellitePosDto>> {
    let obs: core::Observer = observer.into();
    obs.validate()?;
    Ok(
        core::satellites::satellite_positions(&tle_text, unix_ms, &obs)?
            .into_iter()
            .map(|s| SatellitePosDto {
                name: s.name,
                ra_deg: s.ra_deg,
                dec_deg: s.dec_deg,
                range_km: s.range_km,
                above_horizon: s.above_horizon,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Historical ISS TLE (epoch 2024-001.5 = 2024-01-01T12:00Z), fixed for geometry-magnitude tests
    const ISS: &str = "ISS (ZARYA)
1 25544U 98067A   24001.50000000  .00016717  00000-0  30777-3 0  9991
2 25544  51.6400 208.9163 0006317  69.9862 290.2117 15.49560538429085";

    #[test]
    fn iss_positions_cross_the_dart_boundary() {
        let obs = ObserverDto {
            lat_deg: 31.2,
            lon_deg: 121.5,
            alt_m: 10.0,
        };
        let sats = satellite_positions(ISS.to_string(), 1_704_110_400_000, obs).unwrap();
        assert_eq!(sats.len(), 1);
        assert_eq!(sats[0].name, "ISS (ZARYA)");
        // LEO topocentric range: ≲ 2500 km above the horizon, up to ~13,200 km antipodal
        assert!(sats[0].range_km > 350.0 && sats[0].range_km < 13_500.0);
        assert!((-90.0..=90.0).contains(&sats[0].dec_deg));
    }

    #[test]
    fn bad_observer_and_bad_tle_are_errors_not_panics() {
        let bad_obs = ObserverDto {
            lat_deg: 120.0,
            lon_deg: 0.0,
            alt_m: 0.0,
        };
        assert!(satellite_positions(ISS.to_string(), 0, bad_obs).is_err());
        let obs = ObserverDto {
            lat_deg: 0.0,
            lon_deg: 0.0,
            alt_m: 0.0,
        };
        assert!(satellite_positions("not a TLE at all\nreally not".into(), 0, obs).is_err());
    }
}
