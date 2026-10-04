//! unisolver's own check of a narrow-engine solution: project the catalog through the WCS,
//! match the brightest centroids one to one, and turn the match count into the probability
//! that it arose by chance (binomial tail, as tetra3's acceptance test). Gives the outcome the
//! match list and residual statistics of a tetra3 solve, and a second gate against chance
//! solutions.
use crate::outcome::{CentroidOut, MatchOut, Wcs};
use seiza::catalog::StarCatalog;

/// Brightest centroids taken into the check
pub(crate) const CHECK_CENTROIDS: usize = 60;
/// Match radius, pixels
pub(crate) const MATCH_RADIUS_PX: f64 = 3.0;

#[allow(dead_code)] // read by the engine
pub(crate) struct Verified {
    pub matched: Vec<MatchOut>,
    pub rmse_arcsec: f32,
    pub p90_arcsec: f32,
    pub max_err_arcsec: f32,
    /// Probability of at least this many chance matches
    pub prob: f64,
}

#[allow(dead_code)] // used by the engine
pub(crate) fn verify(wcs: &Wcs, centroids: &[CentroidOut], catalog: &dyn StarCatalog) -> Verified {
    let mut order: Vec<usize> = (0..centroids.len()).collect();
    order.sort_by(|&a, &b| {
        centroids[b]
            .mass
            .unwrap_or(0.0)
            .total_cmp(&centroids[a].mass.unwrap_or(0.0))
    });
    order.truncate(CHECK_CENTROIDS);

    let (w, h) = (wcs.width as f64, wcs.height as f64);
    let scale = wcs.scale_arcsec_per_px();
    let (ra0, dec0) = wcs.pixel_to_world((w - 1.0) / 2.0, (h - 1.0) / 2.0);
    let radius_deg = 0.5 * w.hypot(h) * scale / 3600.0 * 1.05;
    // The brightest catalog stars in the frame, twice as many as the centroids checked
    let stars: Vec<(f64, f64)> = catalog
        .cone_search(ra0, dec0, radius_deg, order.len() * 4)
        .iter()
        .filter_map(|s| wcs.world_to_pixel(s.ra, s.dec))
        .filter(|&(x, y)| x >= 0.0 && y >= 0.0 && x < w && y < h)
        .take(order.len() * 2)
        .collect();

    // One-to-one, nearest pairs first
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for &ci in &order {
        let c = &centroids[ci];
        for (si, &(x, y)) in stars.iter().enumerate() {
            let d = (c.x - x).hypot(c.y - y);
            if d <= MATCH_RADIUS_PX {
                pairs.push((d, ci, si));
            }
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut used_c = vec![false; centroids.len()];
    let mut used_s = vec![false; stars.len()];
    let mut matched = Vec::new();
    let mut errs: Vec<f64> = Vec::new();
    for (d, ci, si) in pairs {
        if used_c[ci] || used_s[si] {
            continue;
        }
        used_c[ci] = true;
        used_s[si] = true;
        matched.push(MatchOut {
            centroid_index: ci,
            catalog_id: 0,
            x: centroids[ci].x,
            y: centroids[ci].y,
        });
        errs.push(d * scale);
    }
    matched.sort_by_key(|m| m.centroid_index);
    errs.sort_by(|a, b| a.total_cmp(b));

    // Chance that one centroid lands within the radius of some in-frame star
    let p = (stars.len() as f64 * std::f64::consts::PI * MATCH_RADIUS_PX * MATCH_RADIUS_PX
        / (w * h))
        .min(1.0);
    let n = errs.len();
    Verified {
        prob: binomial_tail(order.len(), n, p),
        rmse_arcsec: if n == 0 {
            0.0
        } else {
            (errs.iter().map(|e| e * e).sum::<f64>() / n as f64).sqrt() as f32
        },
        p90_arcsec: if n == 0 {
            0.0
        } else {
            errs[((n - 1) as f64 * 0.9) as usize] as f32
        },
        max_err_arcsec: errs.last().copied().unwrap_or(0.0) as f32,
        matched,
    }
}

/// P(X ≥ k) for X ~ Binomial(n, p), summed in log space
pub(crate) fn binomial_tail(n: usize, k: usize, p: f64) -> f64 {
    if k == 0 {
        return 1.0;
    }
    if k > n || p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return 1.0;
    }
    let (ln_p, ln_q) = (p.ln(), (1.0 - p).ln());
    let mut ln_choose = 0.0f64;
    let mut total = 0.0f64;
    for i in 0..=n {
        if i > 0 {
            ln_choose += ((n - i + 1) as f64).ln() - (i as f64).ln();
        }
        if i >= k {
            total += (ln_choose + i as f64 * ln_p + (n - i) as f64 * ln_q).exp();
        }
    }
    total.min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::narrow::geometry::wcs_from_seiza;
    use crate::narrow::testkit::{centroids_for, synthetic_sky};

    #[test]
    fn binomial_tail_values() {
        assert_eq!(binomial_tail(10, 0, 0.3), 1.0);
        assert!((binomial_tail(10, 10, 0.5) - 1.0 / 1024.0).abs() < 1e-15);
        assert!((binomial_tail(5, 3, 0.5) - 0.5).abs() < 1e-12);
        assert_eq!(binomial_tail(5, 3, 0.0), 0.0);
    }

    #[test]
    fn the_true_wcs_verifies_and_a_wrong_one_does_not() {
        let sky = synthetic_sky(7);
        let (w, h) = (4000u32, 3000u32);
        let truth = seiza::Wcs::from_center_scale_rotation(
            (212.4, -35.7),
            (1999.5, 1499.5),
            6.0,
            74.0,
            false,
        );
        let cents = centroids_for(&truth, &sky, w, h, 11);
        assert!(cents.len() > 25, "scene too sparse: {}", cents.len());

        let good = verify(&wcs_from_seiza(&truth, w, h), &cents, &sky);
        assert!(good.matched.len() >= 20, "matched {}", good.matched.len());
        assert!(good.prob < 1e-10, "prob {}", good.prob);
        assert!(good.rmse_arcsec < 3.0, "rmse {}", good.rmse_arcsec);
        assert!(good.matched.iter().all(|m| m.catalog_id == 0));

        let off = seiza::Wcs::from_center_scale_rotation(
            (213.4, -35.2),
            (1999.5, 1499.5),
            6.0,
            74.0,
            false,
        );
        let bad = verify(&wcs_from_seiza(&off, w, h), &cents, &sky);
        assert!(
            bad.prob > 1e-5,
            "a wrong field must not verify (prob {})",
            bad.prob
        );
    }
}
