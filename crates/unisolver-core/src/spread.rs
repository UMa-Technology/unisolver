//! Spread selection: which of a frame's detections a wide-field solve takes.
//!
//! Extraction ranks detections by brightness, and a solve used to take the brightest 100. On
//! a wide phone frame a foreground lit by streetlights or windows (leaves, branches, buildings,
//! a telescope) can outshine every star of a light-polluted sky and take all of them: 5 of 12
//! phone photos failed so, with 0–24 stars among their brightest 100. Here the frame is cut
//! into square cells, 8 along its long side, and each cell gives its brightest in turn, so a
//! cluster keeps to its own cells and the open sky gets its share.
//!
//! Measured on the full ladder and pool against the brightest 100: those photos went from 7 to
//! 11 solved; the 47 phone frames of the regression, 24 stacked frames and 6 FITS/XISF frames
//! solved as before, within 0.07° of where they did and no slower. 8 cells was the steadiest
//! grid: 6 lost a regression frame, 12 and 16 solved the photos up to 70% slower. Dropping
//! dense cells instead lost regression frames as soon as its threshold moved, and texture
//! (local noise) does not tell a foreground from a sky that a phone's night mode has smoothed.
//!
//! Only blind solves of wide fields (10° or more, the widest tier's range) use it: narrow
//! fields have no foreground but do have star clusters, and keep the brightest, as does
//! tracking.
use crate::solver::SolveOptions;
use tetra3::Centroid;

/// Cells along the frame's long side (the cells are square)
pub(crate) const GRID: u32 = 8;
/// Blind solves at this horizontal FOV or wider take the spread
pub(crate) const MIN_FOV_DEG: f32 = 10.0;

/// Whether a solve with `opts` on a frame `width` pixels wide takes the spread: switched on
/// (`spread_wide`), blind (no tracking hint), and a FOV of at least [`MIN_FOV_DEG`] (the
/// camera's when one is given).
pub(crate) fn applies(opts: &SolveOptions, width: u32) -> bool {
    let fov = match &opts.camera {
        Some(c) => c.horizontal_fov_deg(width) as f32,
        None => opts.fov_estimate_deg,
    };
    opts.spread_wide && opts.attitude_hint.is_none() && fov >= MIN_FOV_DEG
}

/// Up to `max` of `all` (centre-origin, brightest first, as extraction returns them), cell by
/// cell: round r takes every cell's r-th brightest, brightest first within the round.
pub(crate) fn select(all: &[Centroid], width: u32, height: u32, max: usize) -> Vec<Centroid> {
    let cell = width.max(height) as f64 / GRID as f64;
    let cols = ((width as f64 / cell).ceil() as usize).max(1);
    let rows = ((height as f64 / cell).ceil() as usize).max(1);
    let mut cells: Vec<Vec<&Centroid>> = vec![Vec::new(); cols * rows];
    for c in all {
        let (x, y) = crate::coords::center_to_topleft(c.x as f64, c.y as f64, width, height);
        let col = ((x / cell).floor().max(0.0) as usize).min(cols - 1);
        let row = ((y / cell).floor().max(0.0) as usize).min(rows - 1);
        cells[row * cols + col].push(c);
    }
    let mut out: Vec<Centroid> = Vec::with_capacity(max.min(all.len()));
    for round in 0usize.. {
        if out.len() >= max {
            break;
        }
        let mut picked: Vec<&Centroid> =
            cells.iter().filter_map(|v| v.get(round).copied()).collect();
        if picked.is_empty() {
            break;
        }
        picked.sort_by(|a, b| mass(b).total_cmp(&mass(a)));
        let room = max - out.len();
        out.extend(picked.into_iter().take(room).cloned());
    }
    out
}

fn mass(c: &Centroid) -> f32 {
    c.mass.unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A centroid at top-left pixel (x, y) of a w × h frame
    fn at(x: f64, y: f64, mass: f32, w: u32, h: u32) -> Centroid {
        Centroid {
            x: (x - (w as f64 - 1.0) / 2.0) as f32,
            y: (y - (h as f64 - 1.0) / 2.0) as f32,
            mass: Some(mass),
            cov: None,
        }
    }

    fn brightest_first(mut v: Vec<Centroid>) -> Vec<Centroid> {
        v.sort_by(|a, b| mass(b).total_cmp(&mass(a)));
        v
    }

    #[test]
    fn a_bright_cluster_keeps_to_its_own_cell() {
        // 800 × 600 in 100 px cells (8 × 6): 60 bright blobs in the top-left cell and one faint
        // star in each of 20 other cells
        let (w, h) = (800, 600);
        let mut all: Vec<Centroid> = (0..60)
            .map(|i| {
                let (x, y) = (10.0 + (i % 8) as f64 * 10.0, 10.0 + (i / 8) as f64 * 10.0);
                at(x, y, 1000.0 - i as f32, w, h)
            })
            .collect();
        all.extend((0..20).map(|i| {
            let (x, y) = (
                150.0 + (i % 6) as f64 * 100.0,
                150.0 + (i / 6) as f64 * 100.0,
            );
            at(x, y, 10.0 + i as f32, w, h)
        }));
        let all = brightest_first(all);
        // The brightest 30 overall would be the cluster alone
        assert!(all[..30].iter().all(|c| mass(c) > 900.0));
        let picked = select(&all, w, h, 30);
        assert_eq!(picked.len(), 30);
        // Round 0: the cluster's brightest, then the 20 stars, brightest first; then the
        // cluster again, one per round
        assert_eq!(picked[0].mass, Some(1000.0));
        assert!(picked[1..21].iter().all(|c| mass(c) < 100.0));
        assert!(picked[1..21].windows(2).all(|p| mass(&p[0]) >= mass(&p[1])));
        assert!(picked[21..].iter().all(|c| mass(c) > 900.0));
    }

    #[test]
    fn cells_are_an_eighth_of_the_long_side() {
        // 4032 × 2268: 504 px cells. B shares A's cell and C sits in the next, so C comes
        // before B although B is brighter
        let (w, h) = (4032, 2268);
        let all = vec![
            at(10.0, 10.0, 3.0, w, h),
            at(500.0, 10.0, 2.0, w, h),
            at(520.0, 10.0, 1.0, w, h),
        ];
        let picked: Vec<f32> = select(&all, w, h, 2).iter().map(mass).collect();
        assert_eq!(picked, vec![3.0, 1.0]);
        // Upright: the same along y
        let (w, h) = (2268, 4032);
        let all = vec![
            at(10.0, 10.0, 3.0, w, h),
            at(10.0, 500.0, 2.0, w, h),
            at(10.0, 520.0, 1.0, w, h),
        ];
        let picked: Vec<f32> = select(&all, w, h, 2).iter().map(mass).collect();
        assert_eq!(picked, vec![3.0, 1.0]);
    }

    #[test]
    fn everything_comes_back_when_there_is_room() {
        let (w, h) = (800, 600);
        let all = brightest_first(
            (0..50)
                .map(|i| at((i * 15) as f64, (i * 11) as f64, i as f32, w, h))
                .collect(),
        );
        let picked = select(&all, w, h, 100);
        let (mut a, mut b): (Vec<f32>, Vec<f32>) = (
            all.iter().map(mass).collect(),
            picked.iter().map(mass).collect(),
        );
        a.sort_by(f32::total_cmp);
        b.sort_by(f32::total_cmp);
        assert_eq!(a, b);
    }

    #[test]
    fn only_blind_wide_solves_spread() {
        let w = 4032;
        let mut o = SolveOptions::new(10.0);
        assert!(o.spread_wide, "on by default");
        assert!(applies(&o, w));
        o.fov_estimate_deg = 9.9;
        assert!(!applies(&o, w));
        // A calibrated camera's FOV decides, not the estimate
        o.camera = Some(crate::CameraParams::from_horizontal_fov(70.0, w, 2268).unwrap());
        assert!(applies(&o, w));
        // Tracking keeps the brightest
        o.attitude_hint = Some([1.0, 0.0, 0.0, 0.0]);
        assert!(!applies(&o, w));
        // The switch turns it off
        let mut o = SolveOptions::new(70.0);
        o.spread_wide = false;
        assert!(!applies(&o, w));
    }
}
