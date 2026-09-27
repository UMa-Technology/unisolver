//! Constellation pack (`UCON` file): the 88 IAU constellations' line figures and their
//! boundaries, in J2000 right ascension and declination (degrees).
//!
//! A separate file from the DSO catalog because it has its own source and release cycle;
//! localized constellation names live in the names pack (keys `CON <abbr>`), so this file
//! carries only the IAU abbreviation and Latin name.
use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};

const MAGIC: &[u8; 4] = b"UCON";
const VERSION: u8 = 1;

/// One constellation's figure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstellationFigure {
    /// IAU abbreviation (`Ori`, `UMa`)
    pub abbr: String,
    /// IAU (Latin) name (`Orion`, `Ursa Major`)
    pub name: String,
    /// Line figure: polylines through its stars, each vertex `[ra, dec]` in degrees. Joined
    /// by great-circle arcs.
    pub lines: Vec<Vec<[f32; 2]>>,
    /// Label anchor `[ra, dec]` (the mean direction of the figure's stars)
    pub label: [f32; 2],
}

/// A stretch of boundary between two constellations, already densified (a stretch along a
/// B1875 parallel is a small circle, so its points are close enough to join by great-circle
/// arcs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundaryEdge {
    /// Indices into [`ConstellationPack::constellations`] of the two sides
    pub between: [u8; 2],
    pub points: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConstellationPack {
    pub constellations: Vec<ConstellationFigure>,
    pub boundaries: Vec<BoundaryEdge>,
}

impl ConstellationPack {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut buf = MAGIC.to_vec();
        buf.push(VERSION);
        buf.extend(
            postcard::to_allocvec(self).map_err(|e| CoreError::InvalidInput(e.to_string()))?,
        );
        Ok(buf)
    }

    pub fn write(&self, path: &str) -> Result<()> {
        std::fs::write(path, self.to_bytes()?)?;
        Ok(())
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 5 || &bytes[0..4] != MAGIC {
            return Err(CoreError::InvalidInput("not a UCON file".into()));
        }
        if bytes[4] != VERSION {
            return Err(CoreError::InvalidInput(format!(
                "unsupported UCON version {}",
                bytes[4]
            )));
        }
        let pack: Self = postcard::from_bytes(&bytes[5..])
            .map_err(|e| CoreError::InvalidInput(e.to_string()))?;
        let n = pack.constellations.len();
        if let Some(b) = pack
            .boundaries
            .iter()
            .find(|b| b.between.iter().any(|&i| i as usize >= n))
        {
            return Err(CoreError::InvalidInput(format!(
                "boundary between {:?} names a constellation outside the {n} in the pack",
                b.between
            )));
        }
        Ok(pack)
    }

    pub fn open(path: &str) -> Result<Self> {
        Self::from_bytes(&std::fs::read(path)?)
            .map_err(|e| CoreError::InvalidInput(format!("{path}: {e}")))
    }
}

/// A constellation in the frame: its figure and label, projected to pixels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstellationAnnotation {
    /// IAU abbreviation (`Ori`)
    pub abbr: String,
    /// Name in the requested language (names pack key `CON <abbr>`), else the IAU name
    pub name: String,
    /// Label position: the figure's anchor when it is in the frame, else the mean of its
    /// in-frame vertices; None when no vertex is in the frame
    pub label: Option<[f64; 2]>,
    /// Figure polylines in pixels (top-left origin). They may run past the frame edge; a
    /// polyline breaks where the figure leaves the camera's view.
    pub lines: Vec<Vec<[f64; 2]>>,
}

/// A stretch of IAU boundary in the frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryAnnotation {
    /// IAU abbreviations of the constellations on either side
    pub between: [String; 2],
    pub points: Vec<[f64; 2]>,
}

fn unit(ra_deg: f64, dec_deg: f64) -> [f64; 3] {
    let (ra, dec) = (ra_deg.to_radians(), dec_deg.to_radians());
    [dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin()]
}

fn radec(v: [f64; 3]) -> (f64, f64) {
    (
        v[1].atan2(v[0]).to_degrees().rem_euclid(360.0),
        v[2].clamp(-1.0, 1.0).asin().to_degrees(),
    )
}

fn angle(a: [f64; 3], b: [f64; 3]) -> f64 {
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    (cross[0].hypot(cross[1]).hypot(cross[2])).atan2(dot)
}

/// The smallest-ish spherical cap around a polyline (centre = mean direction, radius = the
/// farthest vertex). Caps under 90° are convex, so the great-circle arcs between the vertices
/// stay inside too: a polyline whose cap misses the view is skipped without touching its points.
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

/// A loaded pack with the caps of every figure line and boundary, computed once.
pub(crate) struct Loaded {
    pub pack: ConstellationPack,
    figure_caps: Vec<Vec<Cap>>,
    boundary_caps: Vec<Cap>,
}

impl Loaded {
    pub(crate) fn new(pack: ConstellationPack) -> Self {
        let figure_caps = pack
            .constellations
            .iter()
            .map(|c| c.lines.iter().map(|l| Cap::of(l)).collect())
            .collect();
        let boundary_caps = pack.boundaries.iter().map(|b| Cap::of(&b.points)).collect();
        Self {
            pack,
            figure_caps,
            boundary_caps,
        }
    }

    /// Figure polylines of constellation `i` in the view
    pub(crate) fn figure(&self, view: &View, i: usize) -> Vec<Vec<[f64; 2]>> {
        self.pack.constellations[i]
            .lines
            .iter()
            .zip(&self.figure_caps[i])
            .filter(|(_, cap)| view.meets(cap))
            .flat_map(|(l, _)| view.project(l))
            .collect()
    }

    /// Boundary polylines in the view, with the index of the edge each came from
    pub(crate) fn boundaries(&self, view: &View) -> Vec<(usize, Vec<[f64; 2]>)> {
        self.pack
            .boundaries
            .iter()
            .zip(&self.boundary_caps)
            .enumerate()
            .filter(|(_, (_, cap))| view.meets(cap))
            .flat_map(|(i, (b, _))| view.project(&b.points).into_iter().map(move |p| (i, p)))
            .collect()
    }
}

/// What the camera sees, for clipping sky polylines before projection: the WCS distortion
/// model is only meaningful near the frame, and far outside it can fold points back in.
pub(crate) struct View<'a> {
    wcs: &'a crate::Wcs,
    centre: [f64; 3],
    /// Points farther than this from the centre (radians) are not projected
    limit: f64,
    /// Great-circle sampling step (radians)
    step: f64,
}

impl<'a> View<'a> {
    pub(crate) fn new(wcs: &'a crate::Wcs) -> Self {
        let (w, h) = (wcs.width as f64, wcs.height as f64);
        let (cra, cdec) = wcs.pixel_to_world((w - 1.0) / 2.0, (h - 1.0) / 2.0);
        let centre = unit(cra, cdec);
        let corner = [
            (0.0, 0.0),
            (w - 1.0, 0.0),
            (0.0, h - 1.0),
            (w - 1.0, h - 1.0),
        ]
        .iter()
        .map(|&(x, y)| {
            let (ra, dec) = wcs.pixel_to_world(x, y);
            angle(centre, unit(ra, dec))
        })
        .fold(0.0, f64::max);
        let fov = (w * wcs.scale_arcsec_per_px() / 3600.0).to_radians();
        let step = (fov / 40.0).clamp(0.02f64.to_radians(), 1.0f64.to_radians());
        Self {
            wcs,
            centre,
            limit: corner * 1.2 + step,
            step,
        }
    }

    /// Whether a cap reaches into the view
    pub(crate) fn meets(&self, cap: &Cap) -> bool {
        angle(self.centre, cap.centre) <= self.limit + cap.radius
    }

    fn in_frame(&self, p: [f64; 2]) -> bool {
        p[0] >= 0.0 && p[1] >= 0.0 && p[0] < self.wcs.width as f64 && p[1] < self.wcs.height as f64
    }

    /// A sky polyline (vertices joined by great-circle arcs) as pixel polylines: arcs are
    /// sampled at the view's step, runs break where the sky leaves the view, and each run is
    /// simplified to half a pixel. Runs without a vertex in the frame are dropped.
    pub(crate) fn project(&self, vertices: &[[f32; 2]]) -> Vec<Vec<[f64; 2]>> {
        let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
        let mut run: Vec<[f64; 2]> = Vec::new();
        let flush = |run: &mut Vec<[f64; 2]>, runs: &mut Vec<Vec<[f64; 2]>>| {
            if run.len() >= 2 && run.iter().any(|&p| self.in_frame(p)) {
                runs.push(crate::annotate::simplify(run, 0.5));
            }
            run.clear();
        };
        let push = |v: [f64; 3], run: &mut Vec<[f64; 2]>, runs: &mut Vec<Vec<[f64; 2]>>| {
            let p = (angle(self.centre, v) <= self.limit)
                .then(|| {
                    let (ra, dec) = radec(v);
                    self.wcs.world_to_pixel(ra, dec)
                })
                .flatten();
            match p {
                Some((x, y)) => run.push([x, y]),
                None => flush(run, runs),
            }
        };
        let units: Vec<[f64; 3]> = vertices
            .iter()
            .map(|v| unit(v[0] as f64, v[1] as f64))
            .collect();
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

    /// Pixel position of a sky point when it falls in the frame.
    pub(crate) fn in_frame_point(&self, v: [f32; 2]) -> Option<[f64; 2]> {
        if angle(self.centre, unit(v[0] as f64, v[1] as f64)) > self.limit {
            return None;
        }
        let (x, y) = self.wcs.world_to_pixel(v[0] as f64, v[1] as f64)?;
        self.in_frame([x, y]).then_some([x, y])
    }

    pub(crate) fn contains(&self, p: [f64; 2]) -> bool {
        self.in_frame(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample() -> ConstellationPack {
        ConstellationPack {
            constellations: vec![
                ConstellationFigure {
                    abbr: "Ori".into(),
                    name: "Orion".into(),
                    lines: vec![vec![[88.79, 7.41], [81.28, 6.35], [78.63, -8.20]]],
                    label: [83.0, 1.0],
                },
                ConstellationFigure {
                    abbr: "Tau".into(),
                    name: "Taurus".into(),
                    lines: vec![vec![[68.98, 16.51], [84.41, 21.14]]],
                    label: [70.0, 18.0],
                },
            ],
            boundaries: vec![BoundaryEdge {
                between: [0, 1],
                points: vec![[86.0, 22.0], [86.0, 10.0]],
            }],
        }
    }

    #[test]
    fn round_trips_and_rejects_bad_files() {
        let p = sample();
        assert_eq!(
            ConstellationPack::from_bytes(&p.to_bytes().unwrap()).unwrap(),
            p
        );
        assert!(ConstellationPack::from_bytes(b"UNAM\x01").is_err());
        let mut bad = p.to_bytes().unwrap();
        bad[4] = 9;
        assert!(ConstellationPack::from_bytes(&bad).is_err());
        let mut dangling = sample();
        dangling.boundaries[0].between = [0, 7];
        assert!(ConstellationPack::from_bytes(&dangling.to_bytes().unwrap()).is_err());
    }
}
