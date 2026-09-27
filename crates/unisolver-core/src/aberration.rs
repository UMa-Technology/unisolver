use crate::solver::SolveOptions;

/// Unix milliseconds to days since J2000.0 (2000-01-01T12:00:00Z = Unix 946728000 s).
pub fn days_since_j2000(unix_ms: i64) -> f64 {
    unix_ms as f64 / 86_400_000.0 - 10_957.5
}

/// An explicit override wins; otherwise Earth's orbital velocity is computed from
/// the observation time, which is accurate enough for ground-based cameras.
pub(crate) fn observer_velocity(opts: &SolveOptions) -> Option<[f64; 3]> {
    opts.observer_velocity_km_s.or_else(|| {
        opts.observation_unix_ms
            .map(|ms| tetra3::earth_barycentric_velocity(days_since_j2000(ms)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn j2000_epoch_maps_to_zero_days() {
        // J2000.0 = 2000-01-01T12:00:00Z = Unix 946_728_000 s
        assert!((days_since_j2000(946_728_000_000)).abs() < 1e-6);
        // One day later
        assert!((days_since_j2000(946_728_000_000 + 86_400_000) - 1.0).abs() < 1e-6);
    }
    #[test]
    fn earth_velocity_magnitude_sane() {
        let v = tetra3::earth_barycentric_velocity(days_since_j2000(1_756_600_000_000));
        let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!((28.0..32.0).contains(&mag), "|v|={mag}");
    }
}
