//! Minimal FITS reader: single-HDU 2D image, BITPIX 8 / 16 (+BZERO/BSCALE) / −32, big-endian.
//! A bad header card becomes None; it never fails the load.
use super::{sane_f64, ImageMeta, SourceFormat};
use crate::{CoreError, Frame, PixelData, Result};

pub fn read_fits_bytes(bytes: &[u8]) -> Result<(Frame, ImageMeta)> {
    // ── Header: 80-byte cards in 2880-byte blocks, terminated by END ──
    if bytes.len() < 2880 || &bytes[0..6] != b"SIMPLE" {
        return Err(CoreError::InvalidInput(
            "not a FITS file (no SIMPLE)".into(),
        ));
    }
    let mut cards = std::collections::HashMap::new();
    let mut header_end = None;
    let mut off = 0;
    'blocks: while off + 2880 <= bytes.len() {
        for i in (off..off + 2880).step_by(80) {
            let card = &bytes[i..i + 80];
            let key = String::from_utf8_lossy(&card[..8]).trim_end().to_string();
            if key == "END" {
                header_end = Some(off + 2880);
                break 'blocks;
            }
            if card.get(8) == Some(&b'=') {
                let val = String::from_utf8_lossy(&card[10..])
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                cards.insert(key, val);
            }
        }
        off += 2880;
    }
    let data_start =
        header_end.ok_or_else(|| CoreError::InvalidInput("FITS header has no END card".into()))?;

    let get_i = |k: &str| cards.get(k).and_then(|v| v.parse::<i64>().ok());
    let get_f = |k: &str| cards.get(k).and_then(|v| v.parse::<f64>().ok());

    let bitpix = get_i("BITPIX")
        .ok_or_else(|| CoreError::InvalidInput("FITS missing BITPIX".into()))?
        as i32;
    let naxis = get_i("NAXIS").unwrap_or(0);
    // NAXIS3=3 is real-world colour FITS (planar RGB): read it as luminance
    let planes = match (naxis, get_i("NAXIS3")) {
        (2, _) => 1usize,
        (3, Some(1)) => 1,
        (3, Some(3)) => 3,
        _ => {
            return Err(CoreError::InvalidInput(format!(
                "unsupported FITS NAXIS={naxis} (need 2D image or 3-plane RGB)"
            )))
        }
    };
    let width = get_i("NAXIS1")
        .filter(|&v| v > 0 && v < 1 << 20)
        .ok_or_else(|| CoreError::InvalidInput("bad NAXIS1".into()))? as u32;
    let height = get_i("NAXIS2")
        .filter(|&v| v > 0 && v < 1 << 20)
        .ok_or_else(|| CoreError::InvalidInput("bad NAXIS2".into()))? as u32;
    let bzero = get_f("BZERO").unwrap_or(0.0);
    let bscale = get_f("BSCALE").unwrap_or(1.0);

    let n = width as usize * height as usize;
    let total = n * planes;
    let bpp = match bitpix {
        8 => 1,
        16 => 2,
        -32 => 4,
        other => {
            return Err(CoreError::InvalidInput(format!(
                "unsupported BITPIX {other} (supported: 8, 16, -32)"
            )))
        }
    };
    let need = data_start + total * bpp;
    if bytes.len() < need {
        return Err(CoreError::InvalidInput(format!(
            "FITS data truncated: have {}, need {need}",
            bytes.len()
        )));
    }
    let data = &bytes[data_start..need];

    // ── Data: big-endian → Frame (physical = BZERO + BSCALE·raw) ──
    // Three planes (planar RGB): read each sample as f64, then combine into luminance
    let sample_at = |i: usize| -> f64 {
        let v = match bitpix {
            8 => data[i] as f64,
            16 => i16::from_be_bytes([data[2 * i], data[2 * i + 1]]) as f64,
            _ => f32::from_be_bytes([
                data[4 * i],
                data[4 * i + 1],
                data[4 * i + 2],
                data[4 * i + 3],
            ]) as f64,
        };
        bzero + bscale * v
    };
    if planes == 3 {
        let pixels = PixelData::LumaF32(
            (0..n)
                .map(|i| {
                    (0.2126 * sample_at(i)
                        + 0.7152 * sample_at(n + i)
                        + 0.0722 * sample_at(2 * n + i)) as f32
                })
                .collect(),
        );
        let meta = build_meta(&cards, width, height, bitpix);
        return Ok((
            Frame {
                width,
                height,
                row_stride_bytes: None,
                pixels,
            },
            meta,
        ));
    }
    let pixels = match bitpix {
        8 => {
            if bzero == 0.0 && bscale == 1.0 {
                PixelData::Luma8(data.to_vec())
            } else {
                PixelData::LumaF32(
                    data.iter()
                        .map(|&v| (bzero + bscale * v as f64) as f32)
                        .collect(),
                )
            }
        }
        16 => {
            let raw = data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| i16::from_be_bytes(*c));
            // The u16 camera convention (BZERO=32768, BSCALE=1) maps straight to Luma16;
            // any other scaling goes through f32
            if bzero == 32768.0 && bscale == 1.0 {
                PixelData::Luma16(raw.map(|v| (v as i32 + 32768) as u16).collect())
            } else {
                PixelData::LumaF32(raw.map(|v| (bzero + bscale * v as f64) as f32).collect())
            }
        }
        _ => PixelData::LumaF32(
            data.as_chunks::<4>()
                .0
                .iter()
                .map(|c| {
                    let v = f32::from_be_bytes(*c);
                    (bzero + bscale * v as f64) as f32
                })
                .collect(),
        ),
    };

    let meta = build_meta(&cards, width, height, bitpix);
    Ok((
        Frame {
            width,
            height,
            row_stride_bytes: None,
            pixels,
        },
        meta,
    ))
}

/// Builds metadata; every value is sanitized and bad values are dropped, not reported.
fn build_meta(
    cards: &std::collections::HashMap<String, String>,
    width: u32,
    height: u32,
    bitpix: i32,
) -> ImageMeta {
    let get_i = |k: &str| cards.get(k).and_then(|v| v.parse::<i64>().ok());
    let get_f = |k: &str| cards.get(k).and_then(|v| v.parse::<f64>().ok());
    let get_s = |k: &str| {
        cards
            .get(k)
            .map(|v| v.trim_matches(|c| c == '\'' || c == ' ').to_string())
            .filter(|s| !s.is_empty())
    };
    let exposure_s = sane_f64(
        get_f("EXPTIME").or_else(|| get_f("EXPOSURE")),
        1e-6,
        86_400.0,
    );
    ImageMeta {
        exposure_s,
        observation_unix_ms: super::header_time(
            get_s("DATE-AVG").as_deref(),
            get_s("DATE-OBS").as_deref(),
            exposure_s,
        ),
        focal_len_mm: sane_f64(get_f("FOCALLEN"), 1.0, 100_000.0),
        pixel_size_um: sane_f64(get_f("XPIXSZ").or_else(|| get_f("PIXSIZE1")), 0.5, 50.0),
        binning: get_i("XBINNING")
            .filter(|&b| (1..=16).contains(&b))
            .map(|b| b as u32),
        date_obs: get_s("DATE-OBS"),
        instrument: get_s("INSTRUME"),
        bayer_pattern: get_s("BAYERPAT"),
        pointing_deg: super::header_pointing(get_s),
        ..ImageMeta::bare(
            width,
            height,
            SourceFormat::Fits,
            bitpix.unsigned_abs() as u8,
        )
    }
}

#[cfg(test)]
pub(crate) fn synth_fits(
    bitpix: i32,
    w: u32,
    h: u32,
    extra_cards: &[&str],
    data: Vec<u8>,
) -> Vec<u8> {
    let mut cards: Vec<String> = vec![
        "SIMPLE  =                    T".into(),
        format!("BITPIX  = {bitpix:>20}"),
        "NAXIS   =                    2".into(),
        format!("NAXIS1  = {w:>20}"),
        format!("NAXIS2  = {h:>20}"),
    ];
    cards.extend(extra_cards.iter().map(|s| s.to_string()));
    cards.push("END".into());
    let mut out = Vec::new();
    for c in &cards {
        let mut b = c.as_bytes().to_vec();
        b.resize(80, b' ');
        out.extend(b);
    }
    while out.len() % 2880 != 0 {
        out.push(b' ');
    }
    out.extend(&data);
    while out.len() % 2880 != 0 {
        out.push(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitpix16_bzero_roundtrip() {
        let data = vec![0x80, 0x00, 0x00, 0x00]; // big-endian i16: -32768, 0
        let f = synth_fits(16, 2, 1, &["BZERO   =                32768"], data);
        let (frame, meta) = read_fits_bytes(&f).unwrap();
        assert_eq!(meta.bit_depth, 16);
        assert_eq!(frame.to_luma_f32().unwrap(), vec![0.0, 32768.0]);
    }

    #[test]
    fn header_absence_is_not_an_error() {
        let (_, meta) = read_fits_bytes(&synth_fits(8, 2, 1, &[], vec![1, 2])).unwrap();
        assert!(meta.fov_hint_deg().is_none());
        assert!(meta.solve_hints().is_empty());
    }

    #[test]
    fn garbage_header_values_are_dropped_not_fatal() {
        let (_, meta) = read_fits_bytes(&synth_fits(
            8,
            2,
            1,
            &[
                "FOCALLEN=               -456.0",
                "XPIXSZ  =                  nan",
                "EXPTIME = 'oops'",
            ],
            vec![1, 2],
        ))
        .unwrap();
        assert!(meta.focal_len_mm.is_none());
        assert!(meta.pixel_size_um.is_none());
        assert!(meta.exposure_s.is_none());
        assert!(meta.fov_hint_deg().is_none());
    }

    #[test]
    fn hint_is_bounded_and_binning_doubles_candidates() {
        let (_, meta) = read_fits_bytes(&synth_fits(
            8,
            6248,
            2,
            &[
                "FOCALLEN=                456.0",
                "XPIXSZ  =                 3.76",
                "XBINNING=                    2",
            ],
            vec![0; 6248 * 2],
        ))
        .unwrap();
        let hints = meta.solve_hints();
        assert_eq!(hints.len(), 2, "hint + hint×binning");
        assert!(
            (hints[0].fov_deg - 2.95).abs() < 0.05,
            "{}",
            hints[0].fov_deg
        );
        assert!(
            (hints[1].fov_deg - 5.89).abs() < 0.1,
            "{}",
            hints[1].fov_deg
        );
        for h in &hints {
            assert!(
                (h.max_error_deg / h.fov_deg - 0.15).abs() < 0.01,
                "±15% tolerance"
            );
        }
        let (_, meta) = read_fits_bytes(&synth_fits(
            8,
            4,
            1,
            &[
                "FOCALLEN=                0.001",
                "XPIXSZ  =                 3.76",
            ],
            vec![0; 4],
        ))
        .unwrap();
        assert!(
            meta.fov_hint_deg().is_none(),
            "out-of-range hint must be dropped"
        );
    }

    #[test]
    fn observation_time_prefers_the_midpoint_then_start_plus_half_exposure() {
        let time = |cards: &[&str]| {
            read_fits_bytes(&synth_fits(8, 2, 1, cards, vec![1, 2]))
                .unwrap()
                .1
                .observation_unix_ms
        };
        // 2026-03-18T16:03:09.543Z, 600 s → midpoint 16:08:09.543
        let start = "DATE-OBS= '2026-03-18T16:03:09.5431999' / UTC";
        assert_eq!(
            time(&[start, "EXPTIME =                600.0"]),
            Some(1_773_849_789_543 + 300_000)
        );
        assert_eq!(time(&[start]), Some(1_773_849_789_543));
        assert_eq!(
            time(&[
                start,
                "EXPTIME =                600.0",
                "DATE-AVG= '2026-03-18T16:08:11.0769318'"
            ]),
            Some(1_773_850_091_076)
        );
        // A date alone or the old DD/MM/YY form gives no time (a wrong time is worse than none)
        assert_eq!(time(&["DATE-OBS= '2026-03-18'"]), None);
        assert_eq!(time(&["DATE-OBS= '18/03/26'"]), None);
        assert_eq!(time(&[]), None);
    }

    #[test]
    fn pointing_from_header_keywords() {
        let at = |cards: &[&str]| {
            read_fits_bytes(&synth_fits(8, 2, 1, cards, vec![1, 2]))
                .unwrap()
                .1
                .pointing_deg
        };
        let near = |p: Option<[f64; 2]>, ra: f64, dec: f64| {
            let p = p.unwrap_or_else(|| panic!("no pointing, want {ra} {dec}"));
            assert!(
                (p[0] - ra).abs() < 1e-4 && (p[1] - dec).abs() < 1e-4,
                "{p:?} vs {ra} {dec}"
            );
        };
        // Degrees, as acquisition software writes the mount position
        near(
            at(&[
                "RA      =     83.8220833333333",
                "DEC     =    -5.39111111111111",
            ]),
            83.82208,
            -5.39111,
        );
        // Sexagesimal target coordinates: RA in hours
        near(
            at(&["OBJCTRA = '05 35 17.300'", "OBJCTDEC= '-05 23 28.00'"]),
            83.82208,
            -5.39111,
        );
        // A negative declination keeps its sign on a zero degree field
        near(
            at(&["OBJCTRA = '00:30:00'", "OBJCTDEC= '-00:30:00'"]),
            7.5,
            -0.5,
        );
        // A plate solution's reference point, only on celestial axes
        near(
            at(&[
                "CTYPE1  = 'RA---TAN'",
                "CRVAL1  =               -10.0",
                "CRVAL2  =                45.0",
            ]),
            350.0,
            45.0,
        );
        assert!(at(&["CTYPE1  = 'GLON-TAN'", "CRVAL1  = 10.0", "CRVAL2  = 45.0"]).is_none());
        // RA/DEC come first; nonsense is dropped
        near(
            at(&[
                "RA      = 10.0",
                "DEC     = 20.0",
                "OBJCTRA = '05 35 17'",
                "OBJCTDEC= '-05 23 28'",
            ]),
            10.0,
            20.0,
        );
        assert!(at(&["RA      = 10.0", "DEC     = 95.0"]).is_none());
        assert!(at(&["OBJCTRA = '25 00 00'", "OBJCTDEC= '10 00 00'"]).is_none());
        assert!(at(&["OBJCTRA = '05 61 00'", "OBJCTDEC= '10 00 00'"]).is_none());
        assert!(at(&[]).is_none());
    }

    #[test]
    fn bitpix8_and_neg32_and_truncation() {
        let (frame, _) = read_fits_bytes(&synth_fits(8, 2, 1, &[], vec![7, 250])).unwrap();
        assert_eq!(frame.to_luma_f32().unwrap(), vec![7.0, 250.0]);
        let mut d = Vec::new();
        d.extend(1.5f32.to_be_bytes());
        d.extend(2.5f32.to_be_bytes());
        let (frame, _) = read_fits_bytes(&synth_fits(-32, 2, 1, &[], d)).unwrap();
        assert_eq!(frame.to_luma_f32().unwrap(), vec![1.5, 2.5]);
        let mut f = synth_fits(16, 100, 100, &[], vec![0; 10]);
        f.truncate(2880 + 10);
        assert!(read_fits_bytes(&f).is_err());
    }
}
