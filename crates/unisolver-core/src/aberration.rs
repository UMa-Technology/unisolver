/// Unix milliseconds to days since J2000.0 (2000-01-01T12:00:00Z = Unix 946728000 s).
pub fn days_since_j2000(unix_ms: i64) -> f64 {
    unix_ms as f64 / 86_400_000.0 - 10_957.5
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
}
