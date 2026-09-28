//! Constellation pack (`UCON` file): the 88 IAU constellations' line figures and their
//! boundaries, in J2000 right ascension and declination (degrees), and a zone table for
//! looking up which constellation a position is in.
//!
//! A separate file from the DSO catalog because it has its own source and release cycle;
//! localized constellation names live in the names pack (keys `CON <abbr>`), so this file
//! carries only the IAU abbreviation and Latin name.
use crate::sky::{Cap, View};
use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};

const MAGIC: &[u8; 4] = b"UCON";
const VERSION: u8 = 2;

/// Julian centuries from J2000 to B1875.0, the epoch the IAU boundaries were drawn in
const B1875: f64 = (2_405_889.258_55 - 2_451_545.0) / 36_525.0;
/// Declination of the south pole in arcminutes, where the zone table starts
const SOUTH_POLE_ARCMIN: i16 = -90 * 60;
/// 24h in seconds of time
const FULL_CIRCLE_SECONDS: u32 = 86_400;

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

/// One declination band of the zone table. The IAU boundaries run along meridians and
/// parallels of B1875, so between two neighbouring boundary parallels the sky splits into
/// ranges of right ascension, each wholly inside one constellation. Units are the boundaries'
/// own, which keeps every edge exact: arcminutes of declination, seconds of time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneBand {
    /// Southern edge (B1875 arcminutes); the band reaches the next band's edge, the last
    /// one the north pole
    pub dec_min: i16,
    /// `(start, constellation)`: B1875 right ascension in seconds of time, ascending from 0,
    /// each range reaching the next one's start and the last one 24h; the constellation is an
    /// index into [`ConstellationPack::constellations`]
    pub ranges: Vec<(u32, u8)>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ConstellationPack {
    pub constellations: Vec<ConstellationFigure>,
    pub boundaries: Vec<BoundaryEdge>,
    /// Bands from the south pole up (empty: no lookup, [`Self::index_at`] gives None)
    pub zones: Vec<ZoneBand>,
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
        pack.check_zones()?;
        Ok(pack)
    }

    /// The zone table covers the sphere once: bands ascend from the south pole, each band's
    /// ranges ascend from 0h, below 24h, naming constellations in the pack.
    fn check_zones(&self) -> Result<()> {
        let bad = |what: String| Err(CoreError::InvalidInput(format!("zone table: {what}")));
        let Some(first) = self.zones.first() else {
            return Ok(());
        };
        if first.dec_min != SOUTH_POLE_ARCMIN {
            return bad(format!("starts at {}′, not the south pole", first.dec_min));
        }
        if let Some(w) = self.zones.windows(2).find(|w| w[0].dec_min >= w[1].dec_min) {
            return bad(format!(
                "bands at {}′ and {}′ out of order",
                w[0].dec_min, w[1].dec_min
            ));
        }
        for z in &self.zones {
            let in_order = z.ranges.windows(2).all(|w| w[0].0 < w[1].0);
            let (start, end) = match (z.ranges.first(), z.ranges.last()) {
                (Some(f), Some(l)) => (f.0, l.0),
                _ => return bad(format!("band at {}′ is empty", z.dec_min)),
            };
            if start != 0 || end >= FULL_CIRCLE_SECONDS || !in_order {
                return bad(format!(
                    "band at {}′ does not run 0h–24h in order",
                    z.dec_min
                ));
            }
            if let Some(r) = z
                .ranges
                .iter()
                .find(|r| r.1 as usize >= self.constellations.len())
            {
                return bad(format!(
                    "band at {}′ names constellation {} of {}",
                    z.dec_min,
                    r.1,
                    self.constellations.len()
                ));
            }
        }
        Ok(())
    }

    /// Index into [`Self::constellations`] of the constellation containing J2000 `(ra, dec)`
    /// (degrees). None when the pack has no zone table or the position is not finite.
    pub fn index_at(&self, ra_deg: f64, dec_deg: f64) -> Option<usize> {
        let p = crate::sky::precession_j2000_to(B1875);
        let (ra, dec) =
            crate::sky::radec(crate::sky::rotate(&p, crate::sky::unit(ra_deg, dec_deg)));
        self.index_at_b1875(ra, dec)
    }

    /// As [`Self::index_at`], for a position already in B1875 (the boundaries' own frame).
    pub fn index_at_b1875(&self, ra_deg: f64, dec_deg: f64) -> Option<usize> {
        let dec = dec_deg * 60.0;
        let band = self
            .zones
            .partition_point(|z| f64::from(z.dec_min) <= dec)
            .checked_sub(1)?;
        let ranges = &self.zones[band].ranges;
        let ra = ra_deg.rem_euclid(360.0) * 240.0;
        let i = ranges
            .partition_point(|r| f64::from(r.0) <= ra)
            .checked_sub(1)?;
        Some(ranges[i].1 as usize)
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

/// The constellation a position is in ([`crate::Annotator::constellation_at`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConstellationName {
    /// IAU abbreviation (`Ori`)
    pub abbr: String,
    /// Name in the requested language (names pack key `CON <abbr>`), else the IAU name
    pub name: String,
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
            // Orion south of +10° (B1875), Taurus north of it between 4h and 6h, Orion again
            // elsewhere
            zones: vec![
                ZoneBand {
                    dec_min: -5400,
                    ranges: vec![(0, 0)],
                },
                ZoneBand {
                    dec_min: 600,
                    ranges: vec![(0, 0), (4 * 3600, 1), (6 * 3600, 0)],
                },
            ],
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

    #[test]
    fn rejects_zone_tables_that_do_not_cover_the_sky() {
        let broken: [fn(&mut ConstellationPack); 5] = [
            |p| p.zones[0].dec_min = -5399,
            |p| p.zones[1].dec_min = -5400,
            |p| p.zones[1].ranges[0].0 = 60,
            |p| p.zones[1].ranges.swap(1, 2),
            |p| p.zones[1].ranges[1].1 = 2,
        ];
        for (i, f) in broken.iter().enumerate() {
            let mut p = sample();
            f(&mut p);
            let err = ConstellationPack::from_bytes(&p.to_bytes().unwrap()).unwrap_err();
            assert!(err.to_string().contains("zone table"), "case {i}: {err}");
        }
        let mut none = sample();
        none.zones.clear();
        assert_eq!(
            ConstellationPack::from_bytes(&none.to_bytes().unwrap())
                .unwrap()
                .index_at(83.0, 0.0),
            None
        );
    }

    #[test]
    fn looks_up_zones_in_b1875_and_precesses_j2000_positions() {
        let p = sample();
        assert_eq!(p.index_at_b1875(75.0, 20.0), Some(1));
        assert_eq!(p.index_at_b1875(75.0, 9.9), Some(0));
        assert_eq!(p.index_at_b1875(95.0, 20.0), Some(0));
        assert_eq!(p.index_at_b1875(359.99, 89.9), Some(0));
        assert_eq!(p.index_at_b1875(-285.0, 20.0), Some(1), "RA wraps");
        assert_eq!(p.index_at_b1875(f64::NAN, 0.0), None);
        // From B1875 to J2000, RA near 6h at +20° grows by about 1.85°: J2000 91° was B1875
        // 5h 56m, still inside Taurus's range
        assert_eq!(p.index_at_b1875(91.0, 20.0), Some(0));
        assert_eq!(p.index_at(91.0, 20.0), Some(1));
        assert_eq!(p.index_at(92.5, 20.0), Some(0));
    }
}
