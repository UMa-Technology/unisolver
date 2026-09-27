//! Test EXIF: writes the tags the engine reads as a TIFF block, and splices it into a JPEG
//! as an APP1 segment, so tests can feed photos "with EXIF" without real camera files.
//! Either byte order; only the IFD0 → Exif / GPS layout cameras use.

/// The tags the engine reads. `None` leaves a tag out.
#[derive(Debug, Clone, Default)]
pub struct ExifFields {
    pub big_endian: bool,
    pub make: Option<String>,
    pub model: Option<String>,
    /// FocalLength (0x920A), written as a rational over 100
    pub focal_mm: Option<f64>,
    /// FocalLengthIn35mmFilm (0xA405)
    pub focal_35mm: Option<u16>,
    /// FocalPlaneXResolution (0xA20E) as numerator / denominator
    pub focal_plane_x_res: Option<(u32, u32)>,
    /// FocalPlaneResolutionUnit (0xA210): 2 inch, 3 cm, 4 mm, 5 µm
    pub focal_plane_unit: Option<u16>,
    /// PixelXDimension (0xA002)
    pub pixel_x_dimension: Option<u32>,
    /// ExposureTime (0x829A) as numerator / denominator
    pub exposure: Option<(u32, u32)>,
    /// DateTimeOriginal (0x9003), `YYYY:MM:DD HH:MM:SS`
    pub date_time_original: Option<String>,
    /// SubSecTimeOriginal (0x9291)
    pub subsec_original: Option<String>,
    /// OffsetTimeOriginal (0x9011), `±HH:MM`
    pub offset_time_original: Option<String>,
    /// GPSDateStamp (0x001D), `YYYY:MM:DD`
    pub gps_date: Option<String>,
    /// GPSTimeStamp (0x0007): hours, minutes, seconds as rationals
    pub gps_time: Option<[(u32, u32); 3]>,
    /// GPS position: latitude ref (`N`/`S`), latitude d/m/s, longitude ref (`E`/`W`),
    /// longitude d/m/s, below sea level, altitude (m) as a rational
    #[allow(clippy::type_complexity)]
    pub gps_position: Option<(
        String,
        [(u32, u32); 3],
        String,
        [(u32, u32); 3],
        bool,
        (u32, u32),
    )>,
}

const ASCII: u16 = 2;
const SHORT: u16 = 3;
const LONG: u16 = 4;
const RATIONAL: u16 = 5;

struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    value: Vec<u8>,
}

impl ExifFields {
    fn u16b(&self, v: u16) -> [u8; 2] {
        if self.big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    }

    fn u32b(&self, v: u32) -> [u8; 4] {
        if self.big_endian {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    }

    fn ascii(&self, tag: u16, s: &str) -> Entry {
        let mut value = s.as_bytes().to_vec();
        value.push(0);
        Entry {
            tag,
            kind: ASCII,
            count: value.len() as u32,
            value,
        }
    }

    fn short(&self, tag: u16, v: u16) -> Entry {
        Entry {
            tag,
            kind: SHORT,
            count: 1,
            value: self.u16b(v).to_vec(),
        }
    }

    fn long(&self, tag: u16, v: u32) -> Entry {
        Entry {
            tag,
            kind: LONG,
            count: 1,
            value: self.u32b(v).to_vec(),
        }
    }

    fn rationals(&self, tag: u16, vs: &[(u32, u32)]) -> Entry {
        let mut value = Vec::new();
        for &(n, d) in vs {
            value.extend(self.u32b(n));
            value.extend(self.u32b(d));
        }
        Entry {
            tag,
            kind: RATIONAL,
            count: vs.len() as u32,
            value,
        }
    }

    fn exif_entries(&self) -> Vec<Entry> {
        let mut v = Vec::new();
        if let Some(e) = self.exposure {
            v.push(self.rationals(0x829A, &[e]));
        }
        if let Some(s) = &self.date_time_original {
            v.push(self.ascii(0x9003, s));
        }
        if let Some(s) = &self.offset_time_original {
            v.push(self.ascii(0x9011, s));
        }
        if let Some(f) = self.focal_mm {
            v.push(self.rationals(0x920A, &[((f * 100.0).round() as u32, 100)]));
        }
        if let Some(s) = &self.subsec_original {
            v.push(self.ascii(0x9291, s));
        }
        if let Some(w) = self.pixel_x_dimension {
            v.push(self.long(0xA002, w));
        }
        if let Some(r) = self.focal_plane_x_res {
            v.push(self.rationals(0xA20E, &[r]));
        }
        if let Some(u) = self.focal_plane_unit {
            v.push(self.short(0xA210, u));
        }
        if let Some(f) = self.focal_35mm {
            v.push(self.short(0xA405, f));
        }
        v
    }

    fn gps_entries(&self) -> Vec<Entry> {
        let mut v = Vec::new();
        if let Some((lat_ref, lat, lon_ref, lon, below, alt)) = &self.gps_position {
            v.push(self.ascii(0x0001, lat_ref));
            v.push(self.rationals(0x0002, lat));
            v.push(self.ascii(0x0003, lon_ref));
            v.push(self.rationals(0x0004, lon));
            v.push(Entry {
                tag: 0x0005,
                kind: 1,
                count: 1,
                value: vec![*below as u8],
            });
            v.push(self.rationals(0x0006, &[*alt]));
        }
        if let Some(t) = self.gps_time {
            v.push(self.rationals(0x0007, &t));
        }
        if let Some(s) = &self.gps_date {
            v.push(self.ascii(0x001D, s));
        }
        v
    }

    /// Bytes of an IFD at `at`: entries, next-IFD offset 0, then out-of-line values.
    fn ifd(&self, entries: &[Entry], at: u32) -> Vec<u8> {
        let mut data_at = at + 2 + 12 * entries.len() as u32 + 4;
        let mut head = self.u16b(entries.len() as u16).to_vec();
        let mut data = Vec::new();
        for e in entries {
            head.extend(self.u16b(e.tag));
            head.extend(self.u16b(e.kind));
            head.extend(self.u32b(e.count));
            if e.value.len() <= 4 {
                let mut inline = e.value.clone();
                inline.resize(4, 0);
                head.extend(inline);
            } else {
                head.extend(self.u32b(data_at));
                data.extend(&e.value);
                if data.len() % 2 == 1 {
                    data.push(0);
                }
                data_at = at + 2 + 12 * entries.len() as u32 + 4 + data.len() as u32;
            }
        }
        head.extend(self.u32b(0));
        head.extend(data);
        head
    }

    /// The TIFF block (what follows `Exif\0\0` in APP1, or a PNG eXIf chunk).
    pub fn tiff(&self) -> Vec<u8> {
        let exif = self.exif_entries();
        let gps = self.gps_entries();
        let mut ifd0: Vec<Entry> = Vec::new();
        if let Some(s) = &self.make {
            ifd0.push(self.ascii(0x010F, s));
        }
        if let Some(s) = &self.model {
            ifd0.push(self.ascii(0x0110, s));
        }
        // Pointer values depend on the layout, and the sizes do not depend on the pointer
        // values: size with placeholders, then write for real
        let with_pointers = |exif_at: u32, gps_at: u32| {
            let mut v: Vec<Entry> = ifd0
                .iter()
                .map(|e| Entry {
                    tag: e.tag,
                    kind: e.kind,
                    count: e.count,
                    value: e.value.clone(),
                })
                .collect();
            if !exif.is_empty() {
                v.push(self.long(0x8769, exif_at));
            }
            if !gps.is_empty() {
                v.push(self.long(0x8825, gps_at));
            }
            v
        };
        let ifd0_len = self.ifd(&with_pointers(0, 0), 8).len() as u32;
        let exif_at = 8 + ifd0_len;
        let exif_bytes = self.ifd(&exif, exif_at);
        let gps_at = exif_at
            + if exif.is_empty() {
                0
            } else {
                exif_bytes.len() as u32
            };

        let mut out = if self.big_endian {
            b"MM\0*".to_vec()
        } else {
            b"II*\0".to_vec()
        };
        out.extend(self.u32b(8));
        out.extend(self.ifd(&with_pointers(exif_at, gps_at), 8));
        if !exif.is_empty() {
            out.extend(exif_bytes);
        }
        if !gps.is_empty() {
            out.extend(self.ifd(&gps, gps_at));
        }
        out
    }

    /// An APP1 segment: marker, length, `Exif\0\0`, TIFF block.
    pub fn app1(&self) -> Vec<u8> {
        let tiff = self.tiff();
        let len = (2 + 6 + tiff.len()) as u16;
        let mut out = vec![0xFF, 0xE1];
        out.extend(len.to_be_bytes());
        out.extend(b"Exif\0\0");
        out.extend(tiff);
        out
    }
}

/// `jpeg` with the EXIF spliced in right after SOI (where cameras put it).
pub fn jpeg_with_exif(jpeg: &[u8], exif: &ExifFields) -> Vec<u8> {
    assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "not a JPEG");
    let mut out = jpeg[..2].to_vec();
    out.extend(exif.app1());
    out.extend(&jpeg[2..]);
    out
}
