//! Sky geometry shared by the polyline layers (constellations, grids) and the batch
//! transforms: unit vectors, the camera's view for clipping, precession and refraction.
use crate::Wcs;
use serde::{Deserialize, Serialize};

/// What the app shows of the image right now: annotation layers that draw lines and labels
/// fit them to it (grid spacing, sampling, simplification, label positions), so they read
/// the same at any zoom. Everything stays in image pixels; the app maps them to the screen.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    /// Visible part of the image, in image pixels (top-left origin)
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Screen pixels per image pixel at the current zoom
    pub scale: f64,
}

pub(crate) fn unit(ra_deg: f64, dec_deg: f64) -> [f64; 3] {
    let (ra, dec) = (ra_deg.to_radians(), dec_deg.to_radians());
    [dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()]
}

pub(crate) fn radec(v: [f64; 3]) -> (f64, f64) {
    (
        v[1].atan2(v[0]).to_degrees().rem_euclid(360.0),
        v[2].clamp(-1.0, 1.0).asin().to_degrees(),
    )
}

/// Angle between two unit vectors (radians), accurate at every separation
pub(crate) fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    (cross[0].hypot(cross[1]).hypot(cross[2])).atan2(dot)
}

pub(crate) fn rotate(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

pub(crate) fn transpose(m: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    [0, 1, 2].map(|i| [m[0][i], m[1][i], m[2][i]])
}

/// Precession from J2000 to the mean equator and equinox of date (IAU 2006, Capitaine et al.
/// 2003), `t` in Julian centuries from J2000. About 0.36° by 2026: the horizontal grid needs it.
pub(crate) fn precession_j2000_to(t: f64) -> [[f64; 3]; 3] {
    let arcsec = |a: f64| (a / 3600.0).to_radians();
    let poly = |c: [f64; 6]| c.iter().rev().fold(0.0, |acc, &k| acc * t + k);
    let zeta = arcsec(poly([
        2.650545,
        2306.083227,
        0.2988499,
        0.01801828,
        -0.000005971,
        -0.0000003173,
    ]));
    let z = arcsec(poly([
        -2.650545,
        2306.077181,
        1.0927348,
        0.01826837,
        -0.000028596,
        -0.0000002904,
    ]));
    let theta = arcsec(poly([
        0.0,
        2004.191903,
        -0.4294934,
        -0.04182264,
        -0.000007089,
        -0.0000001274,
    ]));
    let (cz, sz, czz, szz, ct, st) = (
        zeta.cos(),
        zeta.sin(),
        z.cos(),
        z.sin(),
        theta.cos(),
        theta.sin(),
    );
    [
        [
            cz * ct * czz - sz * szz,
            -sz * ct * czz - cz * szz,
            -st * czz,
        ],
        [
            cz * ct * szz + sz * czz,
            -sz * ct * szz + cz * czz,
            -st * szz,
        ],
        [cz * st, -sz * st, ct],
    ]
}

/// Refraction (degrees) at an **apparent** altitude (Bennett 1982): 0.57° at the horizon,
/// 0.1° at 10°, under 1′ above 45°. Standard atmosphere; clamped below −1°.
pub(crate) fn refraction_at_apparent(alt_deg: f64) -> f64 {
    let h = alt_deg.max(-1.0);
    1.0 / (h + 7.31 / (h + 4.4)).to_radians().tan() / 60.0
}

/// Refraction (degrees) at a **true** altitude (Sæmundsson 1986), the inverse of the above to
/// within a few arcseconds.
pub(crate) fn refraction_at_true(alt_deg: f64) -> f64 {
    let h = alt_deg.max(-1.0);
    1.02 / (h + 10.3 / (h + 5.11)).to_radians().tan() / 60.0
}

/// A spherical cap around a polyline (centre = mean direction, radius = the farthest vertex).
/// Caps under 90° are convex, so the great-circle arcs between the vertices stay inside too: a
/// polyline whose cap misses the view is skipped without touching its points.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Cap {
    centre: [f64; 3],
    radius: f64,
}

impl Cap {
    pub(crate) fn of(vertices: &[[f32; 2]]) -> Self {
        let units: Vec<[f64; 3]> = vertices
            .iter()
            .map(|v| unit(v[0] as f64, v[1] as f64))
            .collect();
        let mut sum = [0.0; 3];
        for u in &units {
            (0..3).for_each(|k| sum[k] += u[k]);
        }
        let norm = sum[0].hypot(sum[1]).hypot(sum[2]);
        if norm < 1e-9 {
            // Degenerate (vertices spread round the sky): never skip
            return Self {
                centre: [0.0, 0.0, 1.0],
                radius: std::f64::consts::PI,
            };
        }
        let centre = [sum[0] / norm, sum[1] / norm, sum[2] / norm];
        let radius = units.iter().map(|&u| angle(centre, u)).fold(0.0, f64::max);
        Self { centre, radius }
    }
}

/// What the camera shows, for clipping sky polylines before projection: the WCS distortion
/// model is only meaningful near the frame, and far outside it can fold points back in. With
/// a [`Viewport`] the region is the visible part of the image, and sampling and
/// simplification follow its zoom.
pub(crate) struct View<'a> {
    wcs: &'a Wcs,
    /// Visible region in image pixels: x0, y0, x1, y1
    region: [f64; 4],
    /// Screen pixels per image pixel
    scale: f64,
    centre: [f64; 3],
    /// Points farther than this from the centre (radians) are not projected
    limit: f64,
    /// Great-circle sampling step (radians)
    step: f64,
}

impl<'a> View<'a> {
    /// The view of the whole image, or of the visible part; None when the viewport misses
    /// the image.
    pub(crate) fn new(wcs: &'a Wcs, viewport: Option<&Viewport>) -> Option<Self> {
        let (w, h) = (wcs.width as f64, wcs.height as f64);
        let (region, scale) = match viewport {
            None => ([0.0, 0.0, w, h], 1.0),
            Some(v) => {
                let r = [
                    v.x.max(0.0),
                    v.y.max(0.0),
                    (v.x + v.width).min(w),
                    (v.y + v.height).min(h),
                ];
                let scale = if v.scale.is_finite() && v.scale > 0.0 {
                    v.scale
                } else {
                    1.0
                };
                ((r[2] > r[0] && r[3] > r[1]).then_some(r)?, scale)
            }
        };
        let (cra, cdec) =
            wcs.pixel_to_world((region[0] + region[2]) / 2.0, (region[1] + region[3]) / 2.0);
        let centre = unit(cra, cdec);
        let corner = [
            (region[0], region[1]),
            (region[2], region[1]),
            (region[0], region[3]),
            (region[2], region[3]),
        ]
        .iter()
        .map(|&(x, y)| {
            let (ra, dec) = wcs.pixel_to_world(x, y);
            angle(centre, unit(ra, dec))
        })
        .fold(0.0, f64::max);
        let fov = ((region[2] - region[0]) * wcs.scale_arcsec_per_px() / 3600.0).to_radians();
        let step = (fov / 40.0).clamp(0.002f64.to_radians(), 1.0f64.to_radians());
        Some(Self {
            wcs,
            region,
            scale,
            centre,
            limit: corner * 1.2 + step,
            step,
        })
    }

    pub(crate) fn wcs(&self) -> &Wcs {
        self.wcs
    }

    pub(crate) fn centre(&self) -> [f64; 3] {
        self.centre
    }

    pub(crate) fn limit(&self) -> f64 {
        self.limit
    }

    pub(crate) fn step(&self) -> f64 {
        self.step
    }

    pub(crate) fn scale(&self) -> f64 {
        self.scale
    }

    pub(crate) fn region(&self) -> [f64; 4] {
        self.region
    }

    /// Simplification tolerance: half a screen pixel, in image pixels
    pub(crate) fn tolerance(&self) -> f64 {
        0.5 / self.scale
    }

    /// Whether a cap reaches into the view
    pub(crate) fn meets(&self, cap: &Cap) -> bool {
        angle(self.centre, cap.centre) <= self.limit + cap.radius
    }

    pub(crate) fn contains(&self, p: [f64; 2]) -> bool {
        let r = self.region;
        p[0] >= r[0] && p[1] >= r[1] && p[0] < r[2] && p[1] < r[3]
    }

    /// Pixel position of a direction, None beyond the view limit or behind the camera
    pub(crate) fn pixel(&self, v: [f64; 3]) -> Option<[f64; 2]> {
        if angle(self.centre, v) > self.limit {
            return None;
        }
        let (ra, dec) = radec(v);
        self.wcs.world_to_pixel(ra, dec).map(|(x, y)| [x, y])
    }

    /// A sky polyline (vertices joined by great-circle arcs) as pixel polylines: arcs are
    /// sampled at the view's step, runs break where the sky leaves the view, and each run is
    /// simplified to half a screen pixel. Runs without a vertex in the visible region are dropped.
    pub(crate) fn project(&self, vertices: &[[f32; 2]]) -> Vec<Vec<[f64; 2]>> {
        let units: Vec<[f64; 3]> = vertices
            .iter()
            .map(|v| unit(v[0] as f64, v[1] as f64))
            .collect();
        self.project_units(&units)
    }

    /// As [`Self::project`], from unit vectors
    pub(crate) fn project_units(&self, units: &[[f64; 3]]) -> Vec<Vec<[f64; 2]>> {
        let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
        let mut run: Vec<[f64; 2]> = Vec::new();
        let flush = |run: &mut Vec<[f64; 2]>, runs: &mut Vec<Vec<[f64; 2]>>| {
            if run.len() >= 2 && run.iter().any(|&p| self.contains(p)) {
                runs.push(crate::annotate::simplify(run, self.tolerance()));
            }
            run.clear();
        };
        let push = |v: [f64; 3], run: &mut Vec<[f64; 2]>, runs: &mut Vec<Vec<[f64; 2]>>| match self
            .pixel(v)
        {
            Some(p) => run.push(p),
            None => flush(run, runs),
        };
        for (i, &b) in units.iter().enumerate() {
            if i == 0 {
                push(b, &mut run, &mut runs);
                continue;
            }
            let a = units[i - 1];
            let theta = angle(a, b);
            // An arc wholly outside the view: nothing to sample
            if angle(self.centre, a).min(angle(self.centre, b)) > self.limit + theta {
                flush(&mut run, &mut runs);
                continue;
            }
            let n = ((theta / self.step).ceil() as usize).max(1);
            let s = theta.sin();
            for k in 1..=n {
                let t = k as f64 / n as f64;
                let v = if s < 1e-12 {
                    b
                } else {
                    let (wa, wb) = (((1.0 - t) * theta).sin() / s, (t * theta).sin() / s);
                    [
                        wa * a[0] + wb * b[0],
                        wa * a[1] + wb * b[1],
                        wa * a[2] + wb * b[2],
                    ]
                };
                push(v, &mut run, &mut runs);
            }
        }
        flush(&mut run, &mut runs);
        runs
    }

    /// Pixel position of a sky point when it falls in the visible region.
    pub(crate) fn visible_point(&self, v: [f32; 2]) -> Option<[f64; 2]> {
        self.pixel(unit(v[0] as f64, v[1] as f64))
            .filter(|&p| self.contains(p))
    }
}

impl Wcs {
    /// Batch sky → pixel for drawing your own overlays: `[ra, dec]` in degrees to top-left
    /// pixels. None behind the camera or farther from the frame than the lens model reaches
    /// (1.2× the corner distance), where the distortion polynomial folds points back in.
    pub fn sky_to_pixels(&self, radec: &[[f64; 2]]) -> Vec<Option<[f64; 2]>> {
        let view = View::new(self, None).expect("the whole image is always a view");
        radec
            .iter()
            .map(|&[ra, dec]| view.pixel(unit(ra, dec)))
            .collect()
    }

    /// Batch pixel → sky: top-left pixels to `[ra, dec]` in degrees. None for pixels more than
    /// a quarter of the frame outside the image, where the lens model no longer applies.
    pub fn pixels_to_sky(&self, pixels: &[[f64; 2]]) -> Vec<Option<[f64; 2]>> {
        let (w, h) = (self.width as f64, self.height as f64);
        pixels
            .iter()
            .map(|&[x, y]| {
                let inside = x.is_finite()
                    && y.is_finite()
                    && (-0.25 * w..=1.25 * w).contains(&x)
                    && (-0.25 * h..=1.25 * h).contains(&y);
                inside.then(|| {
                    let (ra, dec) = self.pixel_to_world(x, y);
                    [ra, dec]
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refraction_forms_agree_and_match_the_textbook_values() {
        assert!((refraction_at_apparent(0.0) * 60.0 - 34.5).abs() < 0.3);
        assert!((refraction_at_apparent(10.0) * 60.0 - 5.3).abs() < 0.2);
        assert!(refraction_at_apparent(60.0) * 3600.0 < 40.0);
        // true = apparent − R(apparent); R(true) takes it back
        for app in [0.5, 2.0, 5.0, 15.0, 45.0] {
            let tru = app - refraction_at_apparent(app);
            assert!(
                (tru + refraction_at_true(tru) - app).abs() * 3600.0 < 5.0,
                "{app}"
            );
        }
    }

    #[test]
    fn precession_to_2026_moves_the_equinox_by_about_a_third_of_a_degree() {
        let p = precession_j2000_to(0.265);
        let (ra, dec) = radec(rotate(&p, unit(0.0, 0.0)));
        assert!(
            (ra - 0.34).abs() < 0.01 && (dec - 0.148).abs() < 0.01,
            "{ra} {dec}"
        );
        let back = rotate(&transpose(&p), rotate(&p, unit(123.0, 45.0)));
        assert!(angle(back, unit(123.0, 45.0)) < 1e-12);
    }
}
