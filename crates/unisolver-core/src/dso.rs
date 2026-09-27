use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DsoKind {
    Galaxy,
    OpenCluster,
    GlobularCluster,
    Nebula,
    PlanetaryNebula,
    HiiRegion,
    SupernovaRemnant,
    DarkNebula,
    ClusterWithNebula,
    Association,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DsoRecord {
    pub designation: String,
    pub common_name_en: Option<String>,
    pub common_name_zh: Option<String>,
    pub kind: DsoKind,
    pub ra_deg: f64,
    pub dec_deg: f64,
    pub mag: Option<f32>,
    pub major_arcmin: Option<f32>,
    pub minor_arcmin: Option<f32>,
    /// Position angle in degrees, north through east (FITS convention)
    pub pa_deg: Option<f32>,
    /// Hand-drawn outlines at up to three brightness levels (OpenNGC); empty for most objects
    pub outlines: Vec<OutlineLevel>,
}

/// One brightness level of an object's outline. OpenNGC draws up to three: level 1 traces
/// the faint outer edge, level 3 only the bright core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutlineLevel {
    pub level: u8,
    pub contours: Vec<OutlineRing>,
}

/// One contour as (ra, dec) degrees; a closed contour's last vertex joins its first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutlineRing {
    pub closed: bool,
    pub vertices: Vec<(f32, f32)>,
}

/// The record layout of version-1 files (before outlines), still readable.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DsoRecordV1 {
    designation: String,
    common_name_en: Option<String>,
    common_name_zh: Option<String>,
    kind: DsoKind,
    ra_deg: f64,
    dec_deg: f64,
    mag: Option<f32>,
    major_arcmin: Option<f32>,
    minor_arcmin: Option<f32>,
    pa_deg: Option<f32>,
}

impl From<DsoRecordV1> for DsoRecord {
    fn from(r: DsoRecordV1) -> Self {
        Self {
            designation: r.designation,
            common_name_en: r.common_name_en,
            common_name_zh: r.common_name_zh,
            kind: r.kind,
            ra_deg: r.ra_deg,
            dec_deg: r.dec_deg,
            mag: r.mag,
            major_arcmin: r.major_arcmin,
            minor_arcmin: r.minor_arcmin,
            pa_deg: r.pa_deg,
            outlines: Vec::new(),
        }
    }
}

const MAGIC: &[u8; 4] = b"UDSO";
/// Version 2 added outlines; version 1 files still open.
const VERSION: u8 = 2;

pub fn write_catalog(path: &str, records: &[DsoRecord]) -> Result<()> {
    let mut buf = Vec::with_capacity(records.len() * 64 + 5);
    buf.extend_from_slice(MAGIC);
    buf.push(VERSION);
    buf.extend(
        postcard::to_allocvec(&records.to_vec())
            .map_err(|e| CoreError::InvalidInput(e.to_string()))?,
    );
    std::fs::write(path, buf)?;
    Ok(())
}

pub struct DsoCatalog {
    records: Vec<DsoRecord>,
    /// Per record: how far its outline reaches from its position (degrees; 0 without one)
    outline_radius_deg: Vec<f64>,
}

impl DsoCatalog {
    pub fn open(path: &str) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        if bytes.len() < 5 || &bytes[0..4] != MAGIC {
            return Err(CoreError::InvalidInput(format!("{path}: not a UDSO file")));
        }
        let bad = |e: postcard::Error| CoreError::InvalidInput(format!("{path}: {e}"));
        let records: Vec<DsoRecord> = match bytes[4] {
            1 => postcard::from_bytes::<Vec<DsoRecordV1>>(&bytes[5..])
                .map_err(bad)?
                .into_iter()
                .map(DsoRecord::from)
                .collect(),
            2 => postcard::from_bytes(&bytes[5..]).map_err(bad)?,
            v => {
                return Err(CoreError::InvalidInput(format!(
                    "{path}: unsupported UDSO version {v}"
                )))
            }
        };
        for r in &records {
            if !(r.ra_deg.is_finite()
                && r.dec_deg.is_finite()
                && (-90.0..=90.0).contains(&r.dec_deg))
            {
                return Err(CoreError::InvalidInput(format!(
                    "{path}: bad coords for {}",
                    r.designation
                )));
            }
        }
        let outline_radius_deg = records.iter().map(outline_radius).collect();
        Ok(Self {
            records,
            outline_radius_deg,
        })
    }
    pub fn records(&self) -> &[DsoRecord] {
        &self.records
    }

    /// How far record `i`'s outline reaches from its position, in degrees (0 without one).
    pub fn outline_radius_deg(&self, i: usize) -> f64 {
        self.outline_radius_deg[i]
    }
}

/// Largest angular distance from the record's position to any outline vertex.
fn outline_radius(r: &DsoRecord) -> f64 {
    let (ra0, dec0) = (r.ra_deg.to_radians(), r.dec_deg.to_radians());
    r.outlines
        .iter()
        .flat_map(|l| &l.contours)
        .flat_map(|c| &c.vertices)
        .map(|&(ra, dec)| {
            let (ra, dec) = ((ra as f64).to_radians(), (dec as f64).to_radians());
            let cos = dec0.sin() * dec.sin() + dec0.cos() * dec.cos() * (ra - ra0).cos();
            cos.clamp(-1.0, 1.0).acos().to_degrees()
        })
        .fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_file() {
        let recs = vec![DsoRecord {
            designation: "M42".into(),
            common_name_en: Some("Orion Nebula".into()),
            common_name_zh: Some("猎户座大星云".into()),
            kind: DsoKind::Nebula,
            ra_deg: 83.822,
            dec_deg: -5.391,
            mag: Some(4.0),
            major_arcmin: Some(85.0),
            minor_arcmin: Some(60.0),
            pa_deg: None,
            outlines: Vec::new(),
        }];
        let p = std::env::temp_dir().join("udso_test.bin");
        write_catalog(p.to_str().unwrap(), &recs).unwrap();
        let cat = DsoCatalog::open(p.to_str().unwrap()).unwrap();
        assert_eq!(cat.records().len(), 1);
        assert_eq!(cat.records()[0].designation, "M42");
        // Reject a bad magic
        std::fs::write(&p, b"XXXX....").unwrap();
        assert!(DsoCatalog::open(p.to_str().unwrap()).is_err());
    }

    #[test]
    fn outlines_roundtrip_and_v1_files_still_open() {
        let mut rec = DsoRecord {
            designation: "NGC1976".into(),
            common_name_en: Some("Orion Nebula".into()),
            common_name_zh: None,
            kind: DsoKind::Nebula,
            ra_deg: 83.822,
            dec_deg: -5.391,
            mag: Some(4.0),
            major_arcmin: Some(85.0),
            minor_arcmin: Some(60.0),
            pa_deg: None,
            outlines: Vec::new(),
        };
        rec.outlines = vec![OutlineLevel {
            level: 1,
            contours: vec![OutlineRing {
                closed: true,
                vertices: vec![(83.0, -5.0), (84.5, -5.0), (84.0, -6.0)],
            }],
        }];
        let p = std::env::temp_dir().join("udso_v2_test.bin");
        write_catalog(p.to_str().unwrap(), std::slice::from_ref(&rec)).unwrap();
        assert_eq!(std::fs::read(&p).unwrap()[4], 2, "writes version 2");
        let cat = DsoCatalog::open(p.to_str().unwrap()).unwrap();
        assert_eq!(cat.records()[0].outlines, rec.outlines);
        // The outline reaches about 0.95° from the record's position
        assert!((cat.outline_radius_deg(0) - 0.95).abs() < 0.05);

        // A version-1 file (no outline section) still opens, with no outlines
        let v1 = DsoRecordV1 {
            designation: rec.designation.clone(),
            common_name_en: rec.common_name_en.clone(),
            common_name_zh: None,
            kind: rec.kind,
            ra_deg: rec.ra_deg,
            dec_deg: rec.dec_deg,
            mag: rec.mag,
            major_arcmin: rec.major_arcmin,
            minor_arcmin: rec.minor_arcmin,
            pa_deg: None,
        };
        let mut buf = b"UDSO\x01".to_vec();
        buf.extend(postcard::to_allocvec(&vec![v1]).unwrap());
        std::fs::write(&p, buf).unwrap();
        let cat = DsoCatalog::open(p.to_str().unwrap()).unwrap();
        assert_eq!(cat.records()[0].designation, "NGC1976");
        assert!(cat.records()[0].outlines.is_empty());
        assert_eq!(cat.outline_radius_deg(0), 0.0);
    }
}
