//! Constellation pack (`UCON` file): the 88 IAU constellations' line figures and their
//! boundaries, in J2000 right ascension and declination (degrees).
//!
//! A separate file from the DSO catalog because it has its own source and release cycle;
//! localized constellation names live in the names pack (keys `CON <abbr>`), so this file
//! carries only the IAU abbreviation and Latin name.
use crate::sky::{Cap, View};
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
