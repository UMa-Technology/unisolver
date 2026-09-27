//! Small robust-statistics helpers shared by the WCS refinement and the
//! distortion fitters.

/// Scale factor turning a median absolute deviation into a Gaussian σ estimate.
pub(crate) const MAD_SCALE: f64 = 1.4826;

/// Median and MAD-derived σ estimate (`MAD_SCALE · MAD`) of `vals`.
///
/// Both order statistics use the sorted midpoint `v[len / 2]`, selected via
/// partial ordering rather than a full sort. NaNs are ordered by
/// [`f64::total_cmp`] (after every finite value) so a stray non-finite input
/// cannot violate the comparator's total-order contract. `vals` is reordered
/// in place; an empty slice yields `(0.0, 0.0)`.
pub(crate) fn median_mad_sigma(vals: &mut [f64]) -> (f64, f64) {
    if vals.is_empty() {
        return (0.0, 0.0);
    }
    let mid = vals.len() / 2;
    vals.select_nth_unstable_by(mid, f64::total_cmp);
    let median = vals[mid];
    for v in vals.iter_mut() {
        *v = (*v - median).abs();
    }
    vals.select_nth_unstable_by(mid, f64::total_cmp);
    (median, MAD_SCALE * vals[mid])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_mad_basic() {
        let mut v = vec![1.0, 2.0, 3.0, 4.0, 100.0];
        let (m, s) = median_mad_sigma(&mut v);
        assert_eq!(m, 3.0);
        assert!((s - MAD_SCALE).abs() < 1e-12); // |devs| = 2,1,0,1,97 → MAD 1
        assert_eq!(median_mad_sigma(&mut []), (0.0, 0.0));
    }

    #[test]
    fn median_mad_tolerates_nan() {
        let mut v = vec![1.0, f64::NAN, 2.0, 3.0];
        let (m, _) = median_mad_sigma(&mut v);
        assert!(m.is_finite());
    }
}
