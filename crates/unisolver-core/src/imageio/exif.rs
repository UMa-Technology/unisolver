//! EXIF from JPEG (APP1), PNG (eXIf) and TIFF: only the tags that give a FOV hint or the
//! capture time. EXIF is untrusted input like any header: every read is bounds-checked,
//! only the IFD0 → Exif / GPS pointers are followed (so no loops), and anything malformed
//! is simply absent.
use super::time;

/// The EXIF fields the engine uses, already sanitized.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Exif {
    pub make: Option<String>,
    pub model: Option<String>,
    pub focal_mm: Option<f64>,
    pub focal_35mm: Option<f64>,
    /// Pixel pitch on the sensor (µm) for the width the camera recorded, from
    /// FocalPlaneXResolution / FocalPlaneResolutionUnit
    pub pitch_um: Option<f64>,
    /// PixelXDimension: the width the camera recorded (a later resize shrinks the stored width)
    pub recorded_width: Option<u32>,
    pub exposure_s: Option<f64>,
    /// DateTimeOriginal as written (local time, `YYYY:MM:DD HH:MM:SS`)
    pub date_time_original: Option<String>,
    pub subsec: Option<String>,
    /// OffsetTimeOriginal, falling back to OffsetTime
    pub offset: Option<String>,
    pub gps_date: Option<String>,
    pub gps_time_s: Option<f64>,
    /// Where the photo was taken (GPSLatitude/Longitude/Altitude with their refs)
    pub observer: Option<crate::Observer>,
}

impl Exif {
    /// `YYYY-MM-DDTHH:MM:SS[.sss][±HH:MM]`: DateTimeOriginal in ISO form, with the zone
    /// when EXIF gives one (it is local time otherwise).
    pub fn date_obs(&self) -> Option<String> {
        let dt = self.date_time_original.as_deref()?;
        time::civil_ms(dt)?;
        let mut s = format!("{}-{}-{}T{}", &dt[0..4], &dt[5..7], &dt[8..10], &dt[11..19]);
        if let Some(ms) = self.subsec.as_deref().and_then(time::subsec_ms) {
            s.push_str(&format!(".{ms:03}"));
        }
        if let Some(off) = self
            .offset
            .as_deref()
            .filter(|o| time::offset_ms(o).is_some())
        {
            s.push_str(off.trim());
        }
        Some(s)
    }

    /// Capture time in UTC (Unix ms, mid-exposure when the exposure is known), or None when
    /// the zone is unknown. DateTimeOriginal is local time: OffsetTimeOriginal pins the
    /// zone; failing that, a GPS time (UTC) does, but only when local − GPS lands within
    /// two minutes of a quarter-hour zone. A stale GPS fix fails that test rather than
    /// shifting the time by hours (the moon moves half a degree an hour).
    pub fn observation_unix_ms(&self) -> Option<i64> {
        let local = time::civil_ms(self.date_time_original.as_deref()?)?
            + self
                .subsec
                .as_deref()
                .and_then(time::subsec_ms)
                .unwrap_or(0);
        let utc = match self.offset.as_deref().and_then(time::offset_ms) {
            Some(off) => local - off,
            None => {
                let date = time::civil_ms(&format!("{} 00:00:00", self.gps_date.as_deref()?))?;
                let gps = date + (self.gps_time_s? * 1000.0).round() as i64;
                let quarter = 15 * 60_000;
                let zone = ((local - gps) as f64 / quarter as f64).round() as i64 * quarter;
                if zone.abs() > 14 * 3_600_000 || (local - gps - zone).abs() > 2 * 60_000 {
                    return None;
                }
                local - zone
            }
        };
        time::mid_exposure(Some(utc), self.exposure_s)
    }
}

/// The TIFF block inside a JPEG's `Exif\0\0` APP1 segment. Walks the markers up to the
/// scan: cameras put APP1 first, but editors may insert JFIF or ICC segments before it.
pub(crate) fn jpeg_tiff(bytes: &[u8]) -> Option<&[u8]> {
    let mut i = 2;
    loop {
        if *bytes.get(i)? != 0xFF {
            return None;
        }
        while *bytes.get(i + 1)? == 0xFF {
            i += 1; // fill bytes
        }
        let marker = *bytes.get(i + 1)?;
        match marker {
            0xD9 | 0xDA => return None, // end of image / start of scan: no EXIF before the data
            0x01 | 0xD0..=0xD7 => {
                i += 2;
                continue;
            }
            _ => {}
        }
        let len = u16::from_be_bytes([*bytes.get(i + 2)?, *bytes.get(i + 3)?]) as usize;
        let seg = bytes.get(i + 4..i + 2 + len.max(2))?;
        if marker == 0xE1 && seg.starts_with(b"Exif\0\0") {
            return Some(&seg[6..]);
        }
        i += 2 + len;
    }
}

/// The TIFF block in a PNG `eXIf` chunk.
pub(crate) fn png_tiff(bytes: &[u8]) -> Option<&[u8]> {
    let mut i = 8;
    loop {
        let len = u32::from_be_bytes(bytes.get(i..i + 4)?.try_into().ok()?) as usize;
        let kind = bytes.get(i + 4..i + 8)?;
        let data = bytes.get(i + 8..(i + 8).checked_add(len)?)?;
        match kind {
            b"eXIf" => return Some(data),
            b"IEND" => return None,
            _ => i = i.checked_add(12)?.checked_add(len)?,
        }
    }
}

/// One IFD entry: tag, type, count, and where its value bytes start.
struct Entry {
    tag: u16,
    kind: u16,
    count: u32,
    at: usize,
}

struct Tiff<'a> {
    b: &'a [u8],
    be: bool,
}

impl<'a> Tiff<'a> {
    fn u16(&self, at: usize) -> Option<u16> {
        let v: [u8; 2] = self.b.get(at..at + 2)?.try_into().ok()?;
        Some(if self.be {
            u16::from_be_bytes(v)
        } else {
            u16::from_le_bytes(v)
        })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let v: [u8; 4] = self.b.get(at..at + 4)?.try_into().ok()?;
        Some(if self.be {
            u32::from_be_bytes(v)
        } else {
            u32::from_le_bytes(v)
        })
    }

    /// Entries of the IFD at `at`. Values of 4 bytes or fewer sit in the entry itself,
    /// larger ones at the offset it holds; an entry whose value does not fit in the block,
    /// or of an unknown type, is skipped.
    fn ifd(&self, at: usize) -> Vec<Entry> {
        let mut out = Vec::new();
        let Some(n) = self.u16(at) else {
            return out;
        };
        // A real IFD has tens of entries; a huge count is garbage (and bounds the loop)
        for k in 0..n.min(512) as usize {
            let e = at + 2 + 12 * k;
            let (Some(tag), Some(kind), Some(count)) =
                (self.u16(e), self.u16(e + 2), self.u32(e + 4))
            else {
                break;
            };
            let unit = match kind {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 => 4,
                5 | 10 | 12 => 8,
                _ => continue,
            };
            let Some(size) = (count as usize).checked_mul(unit).filter(|&s| s > 0) else {
                continue;
            };
            let value_at = if size <= 4 {
                e + 8
            } else {
                match self.u32(e + 8) {
                    Some(v) => v as usize,
                    None => break,
                }
            };
            if value_at
                .checked_add(size)
                .is_some_and(|end| end <= self.b.len())
            {
                out.push(Entry {
                    tag,
                    kind,
                    count,
                    at: value_at,
                });
            }
        }
        out
    }

    fn ascii(&self, e: &Entry) -> Option<String> {
        if e.kind != 2 {
            return None;
        }
        let raw = &self.b[e.at..e.at + e.count as usize];
        let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
        let s = std::str::from_utf8(&raw[..end]).ok()?.trim();
        (!s.is_empty()).then(|| s.to_string())
    }

    fn byte(&self, e: &Entry) -> Option<u8> {
        matches!(e.kind, 1 | 7).then(|| self.b[e.at])
    }

    fn uint(&self, e: &Entry) -> Option<u32> {
        match e.kind {
            3 => self.u16(e.at).map(u32::from),
            4 => self.u32(e.at),
            _ => None,
        }
    }

    fn rational(&self, e: &Entry, k: usize) -> Option<f64> {
        if k >= e.count as usize {
            return None;
        }
        let at = e.at + 8 * k;
        let (n, d) = match e.kind {
            5 => (self.u32(at)? as f64, self.u32(at + 4)? as f64),
            10 => (self.u32(at)? as i32 as f64, self.u32(at + 4)? as i32 as f64),
            _ => return None,
        };
        (d != 0.0).then_some(n / d).filter(|v| v.is_finite())
    }
}

/// Parses a TIFF block (the start of a TIFF file, or the payload of APP1 / eXIf).
pub(crate) fn parse(block: &[u8]) -> Option<Exif> {
    let be = match block.get(0..4)? {
        b"II*\0" => false,
        b"MM\0*" => true,
        _ => return None,
    };
    let t = Tiff { b: block, be };
    let ifd0 = t.ifd(t.u32(4)? as usize);
    let find = |ifd: &[Entry], tag: u16| ifd.iter().position(|e| e.tag == tag);
    let pointer = |tag: u16| {
        find(&ifd0, tag)
            .and_then(|i| t.uint(&ifd0[i]))
            .map(|at| t.ifd(at as usize))
            .unwrap_or_default()
    };
    let (exif, gps) = (pointer(0x8769), pointer(0x8825));
    let s = |ifd: &[Entry], tag| find(ifd, tag).and_then(|i| t.ascii(&ifd[i]));
    let u = |ifd: &[Entry], tag| find(ifd, tag).and_then(|i| t.uint(&ifd[i]));
    let r = |ifd: &[Entry], tag, k| find(ifd, tag).and_then(|i| t.rational(&ifd[i], k));

    // FocalPlaneXResolution is pixels per unit of the recorded width; unit 2 (the default,
    // and 1 = "none" in practice) inch, 3 cm, 4 mm, 5 µm
    let pitch_um = r(&exif, 0xA20E, 0).and_then(|res| {
        let unit_um = match u(&exif, 0xA210).unwrap_or(2) {
            1 | 2 => 25_400.0,
            3 => 10_000.0,
            4 => 1_000.0,
            5 => 1.0,
            _ => return None,
        };
        Some(unit_um / res)
    });
    let gps_time_s = (|| {
        let (h, m, sec) = (
            r(&gps, 0x0007, 0)?,
            r(&gps, 0x0007, 1)?,
            r(&gps, 0x0007, 2)?,
        );
        (h < 24.0 && m < 60.0 && sec < 61.0 && h >= 0.0 && m >= 0.0 && sec >= 0.0)
            .then_some(h * 3600.0 + m * 60.0 + sec)
    })();
    // Degrees, minutes, seconds; the refs give the hemisphere. Altitude is optional (sea
    // level otherwise: it barely moves the moon's parallax)
    let dms = |tag| Some(r(&gps, tag, 0)? + r(&gps, tag, 1)? / 60.0 + r(&gps, tag, 2)? / 3600.0);
    let observer = (|| {
        let lat = match s(&gps, 0x0001)?.as_str() {
            "N" => dms(0x0002)?,
            "S" => -dms(0x0002)?,
            _ => return None,
        };
        let lon = match s(&gps, 0x0003)?.as_str() {
            "E" => dms(0x0004)?,
            "W" => -dms(0x0004)?,
            _ => return None,
        };
        let below = find(&gps, 0x0005).and_then(|i| t.byte(&gps[i])) == Some(1);
        let alt = r(&gps, 0x0006, 0).map_or(0.0, |a| if below { -a } else { a });
        let o = crate::Observer {
            lat_deg: lat,
            lon_deg: lon,
            alt_m: alt,
        };
        // (0, 0) is what a camera without a fix writes
        (o.validate().is_ok() && (lat, lon) != (0.0, 0.0)).then_some(o)
    })();
    Some(Exif {
        make: s(&ifd0, 0x010F),
        model: s(&ifd0, 0x0110),
        focal_mm: super::sane_f64(r(&exif, 0x920A, 0), 0.5, 100_000.0),
        // 0 means unknown in EXIF
        focal_35mm: super::sane_f64(u(&exif, 0xA405).map(f64::from), 1.0, 2000.0),
        pitch_um: super::sane_f64(pitch_um, 0.3, 50.0),
        recorded_width: u(&exif, 0xA002).filter(|&w| w > 0),
        exposure_s: super::sane_f64(r(&exif, 0x829A, 0), 1e-6, 86_400.0),
        date_time_original: s(&exif, 0x9003),
        subsec: s(&exif, 0x9291),
        offset: s(&exif, 0x9011).or_else(|| s(&exif, 0x9010)),
        gps_date: s(&gps, 0x001D),
        gps_time_s,
        observer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use unisolver_synth::exif::{jpeg_with_exif, ExifFields};

    fn phone() -> ExifFields {
        ExifFields {
            make: Some("Apple".into()),
            model: Some("iPhone 15 Pro".into()),
            focal_mm: Some(6.86),
            focal_35mm: Some(24),
            exposure: Some((3, 1)),
            date_time_original: Some("2026:03:19 08:03:09".into()),
            subsec_original: Some("250".into()),
            offset_time_original: Some("+08:00".into()),
            ..Default::default()
        }
    }

    #[test]
    fn reads_both_byte_orders() {
        for big_endian in [false, true] {
            let e = parse(
                &ExifFields {
                    big_endian,
                    ..phone()
                }
                .tiff(),
            )
            .expect("parses");
            assert_eq!(e.make.as_deref(), Some("Apple"));
            assert_eq!(e.model.as_deref(), Some("iPhone 15 Pro"));
            assert_eq!(e.focal_35mm, Some(24.0));
            assert!((e.focal_mm.unwrap() - 6.86).abs() < 1e-9);
            assert_eq!(e.exposure_s, Some(3.0));
            assert_eq!(
                e.date_obs().as_deref(),
                Some("2026-03-19T08:03:09.250+08:00")
            );
            // 2026-03-19T00:03:09.250Z plus half of 3 s
            assert_eq!(e.observation_unix_ms(), Some(1_773_878_589_250 + 1500));
        }
    }

    #[test]
    fn finds_app1_after_other_segments() {
        let jfif = [0xFF, 0xE0, 0x00, 0x04, 0xAB, 0xCD];
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend(jfif);
        jpeg.extend(phone().app1());
        jpeg.extend([0xFF, 0xDA, 0x00, 0x02, 0xFF, 0xD9]);
        let e = parse(jpeg_tiff(&jpeg).expect("finds APP1")).unwrap();
        assert_eq!(e.focal_35mm, Some(24.0));
        // Splicing right after SOI, as the test helper does, works too
        let spliced = jpeg_with_exif(&[0xFF, 0xD8, 0xFF, 0xD9], &phone());
        assert!(jpeg_tiff(&spliced).is_some());
        // No APP1 before the scan: none
        assert!(jpeg_tiff(&[0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02]).is_none());
    }

    #[test]
    fn png_exif_chunk() {
        let tiff = phone().tiff();
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        for (kind, data) in [
            (&b"IHDR"[..], &[0u8; 13][..]),
            (b"eXIf", &tiff),
            (b"IEND", &[]),
        ] {
            png.extend((data.len() as u32).to_be_bytes());
            png.extend(kind);
            png.extend(data);
            png.extend([0u8; 4]);
        }
        assert_eq!(png_tiff(&png), Some(&tiff[..]));
    }

    #[test]
    fn zone_from_gps_only_when_it_lands_on_a_zone() {
        let no_offset = ExifFields {
            offset_time_original: None,
            exposure: None,
            subsec_original: None,
            ..phone()
        };
        // Local 08:03:09, GPS 00:02:40 UTC: +8 h within half a minute → trusted
        let near = ExifFields {
            gps_date: Some("2026:03:19".into()),
            gps_time: Some([(0, 1), (2, 1), (40, 1)]),
            ..no_offset.clone()
        };
        let e = parse(&near.tiff()).unwrap();
        assert_eq!(e.observation_unix_ms(), Some(1_773_878_589_000));
        // A fix 40 minutes stale (23:23 the day before) does not land on a zone → no time
        let stale = ExifFields {
            gps_date: Some("2026:03:18".into()),
            gps_time: Some([(23, 1), (23, 1), (0, 1)]),
            ..no_offset.clone()
        };
        assert_eq!(parse(&stale.tiff()).unwrap().observation_unix_ms(), None);
        // Neither offset nor GPS: local time only, so no UTC, but the string is kept
        let e = parse(&no_offset.tiff()).unwrap();
        assert_eq!(e.observation_unix_ms(), None);
        assert_eq!(e.date_obs().as_deref(), Some("2026-03-19T08:03:09"));
    }

    #[test]
    fn gps_position_gives_the_observer() {
        let at = |lat_ref: &str, lon_ref: &str, below: bool| {
            parse(
                &ExifFields {
                    gps_position: Some((
                        lat_ref.into(),
                        [(31, 1), (12, 1), (3600, 100)],
                        lon_ref.into(),
                        [(121, 1), (30, 1), (0, 1)],
                        below,
                        (125, 10),
                    )),
                    ..Default::default()
                }
                .tiff(),
            )
            .unwrap()
            .observer
        };
        let o = at("N", "E", false).unwrap();
        assert!((o.lat_deg - (31.0 + 12.0 / 60.0 + 36.0 / 3600.0)).abs() < 1e-9);
        assert!((o.lon_deg - 121.5).abs() < 1e-9);
        assert!((o.alt_m - 12.5).abs() < 1e-9);
        let o = at("S", "W", true).unwrap();
        assert!(o.lat_deg < 0.0 && o.lon_deg < 0.0 && o.alt_m < 0.0);
        assert_eq!(at("X", "E", false), None, "unknown hemisphere");
    }

    #[test]
    fn focal_plane_resolution_gives_the_pitch() {
        // 5184 px over a 22.3 mm sensor (APS-C): FocalPlaneXResolution in pixels per cm
        let e = parse(
            &ExifFields {
                focal_plane_x_res: Some((5184 * 1000, 2230)),
                focal_plane_unit: Some(3),
                pixel_x_dimension: Some(5184),
                ..Default::default()
            }
            .tiff(),
        )
        .unwrap();
        assert!((e.pitch_um.unwrap() - 22_300.0 / 5184.0).abs() < 1e-6);
        assert_eq!(e.recorded_width, Some(5184));
    }

    #[test]
    fn unknown_values_are_absent() {
        let e = parse(
            &ExifFields {
                focal_35mm: Some(0),
                focal_mm: Some(0.0),
                date_time_original: Some("    :  :     :  :  ".into()),
                offset_time_original: Some("   :  ".into()),
                ..Default::default()
            }
            .tiff(),
        )
        .unwrap();
        assert_eq!(e.focal_35mm, None);
        assert_eq!(e.focal_mm, None);
        assert_eq!(e.date_obs(), None);
        assert_eq!(e.observation_unix_ms(), None);
    }

    /// Truncated and corrupted blocks never panic (EXIF is untrusted input).
    #[test]
    fn garbage_never_panics() {
        let full = ExifFields {
            gps_date: Some("2026:03:19".into()),
            gps_time: Some([(0, 1), (2, 1), (40, 1)]),
            ..phone()
        };
        for big_endian in [false, true] {
            let tiff = ExifFields {
                big_endian,
                ..full.clone()
            }
            .tiff();
            for n in 0..tiff.len() {
                let _ = parse(&tiff[..n]).map(|e| e.observation_unix_ms());
            }
            let mut state = 0x2545_F491_4F6C_DD1Du64;
            for _ in 0..2000 {
                let mut t = tiff.clone();
                for _ in 0..4 {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    let i = (state as usize) % t.len();
                    t[i] = (state >> 32) as u8;
                }
                let _ = parse(&t).map(|e| (e.observation_unix_ms(), e.date_obs()));
                let mut jpeg = vec![0xFF, 0xD8];
                jpeg.extend(&t[..t.len().min(200)]);
                let _ = jpeg_tiff(&jpeg);
                let _ = png_tiff(&jpeg);
            }
        }
    }
}
