//! Monolithic XISF 1.0 reader: Gray (1 ch) / RGB (3 ch), planar storage,
//! UInt8/16/32 and Float32/64 (32/64-bit reduced to f32), zlib / lz4 / lz4hc / zstd
//! (including +sh byte shuffling). Anything else is a clear Err. FITSKeyword
//! metadata passes through sanitized (bad values dropped).
use super::{sane_f64, ImageMeta, SourceFormat};
use crate::{CoreError, Frame, PixelData, Result};
use quick_xml::events::Event;

const SIGNATURE: &[u8; 8] = b"XISF0100";

#[derive(Clone, Copy, PartialEq)]
enum SampleFormat {
    U8,
    U16,
    U32,
    F32,
    F64,
}

impl SampleFormat {
    fn parse(v: &str) -> Result<Self> {
        Ok(match v {
            "UInt8" | "Byte" => Self::U8,
            "UInt16" | "UShort" => Self::U16,
            "UInt32" | "UInt" => Self::U32,
            "Float32" | "Float" => Self::F32,
            "Float64" | "Double" => Self::F64,
            other => {
                return Err(CoreError::InvalidInput(format!(
                    "XISF sample format {other:?} unsupported"
                )))
            }
        })
    }
    fn bytes(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 => 2,
            Self::U32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }
}

struct Compression {
    codec: &'static str,
    uncompressed: usize,
    shuffle_item: Option<usize>,
}

fn err(msg: impl Into<String>) -> CoreError {
    CoreError::InvalidInput(msg.into())
}

pub fn read_xisf_bytes(bytes: &[u8]) -> Result<(Frame, ImageMeta)> {
    if bytes.len() < 16 || &bytes[0..8] != SIGNATURE {
        return Err(err("not an XISF 1.0 file"));
    }
    let header_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if bytes[12..16] != [0; 4] {
        return Err(err("XISF reserved preamble field is not zero"));
    }
    if header_len == 0 || 16 + header_len > bytes.len() {
        return Err(err("XISF XML header length out of range"));
    }
    let xml = &bytes[16..16 + header_len];

    // ── XML: attributes of the first <Image> and its <FITSKeyword> children ──
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut attrs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut fits_kw: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut in_image = false;
    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| err(format!("XISF XML: {e}")))?
        {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = e.local_name().as_ref().to_vec();
                if name == b"Image" && attrs.is_empty() {
                    for a in e.attributes().flatten() {
                        attrs.insert(
                            String::from_utf8_lossy(a.key.local_name().as_ref()).into_owned(),
                            String::from_utf8_lossy(&a.value).into_owned(),
                        );
                    }
                    in_image = true;
                } else if in_image && name == b"FITSKeyword" {
                    let mut kname = None;
                    let mut kval = None;
                    for a in e.attributes().flatten() {
                        match a.key.local_name().as_ref() {
                            b"name" => kname = Some(String::from_utf8_lossy(&a.value).into_owned()),
                            b"value" => kval = Some(String::from_utf8_lossy(&a.value).into_owned()),
                            _ => {}
                        }
                    }
                    if let (Some(k), Some(v)) = (kname, kval) {
                        fits_kw.insert(k, v);
                    }
                }
            }
            Event::End(ref e) if e.local_name().as_ref() == b"Image" => break,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if attrs.is_empty() {
        return Err(err("XISF has no <Image> element"));
    }

    // ── Geometry and sample format ──
    let geometry = attrs
        .get("geometry")
        .ok_or_else(|| err("XISF missing geometry"))?;
    let dims: Vec<usize> = geometry
        .split(':')
        .map(|p| p.parse::<usize>().map_err(|_| err("bad geometry")))
        .collect::<Result<_>>()?;
    if dims.len() != 3 {
        return Err(err(format!(
            "XISF geometry {geometry:?}: only 2D images supported"
        )));
    }
    let (w, h, channels) = (dims[0], dims[1], dims[2]);
    if !(w > 0 && h > 0 && w < 1 << 20 && h < 1 << 20) {
        return Err(err("XISF geometry out of range"));
    }
    let color_space = attrs
        .get("colorSpace")
        .map(String::as_str)
        .unwrap_or("Gray");
    if !matches!((channels, color_space), (1, "Gray") | (3, "RGB")) {
        return Err(err(format!(
            "XISF {channels}-channel {color_space} unsupported (Gray/1 or RGB/3)"
        )));
    }
    let storage = attrs
        .get("pixelStorage")
        .map(String::as_str)
        .unwrap_or("Planar");
    if storage != "Planar" {
        return Err(err(format!(
            "XISF pixelStorage {storage:?} unsupported (Planar only, matching seiza)"
        )));
    }
    let big_endian = match attrs.get("byteOrder").map(String::as_str) {
        None | Some("little") => false,
        Some("big") => true,
        Some(other) => return Err(err(format!("XISF byteOrder {other:?} invalid"))),
    };
    let sample = SampleFormat::parse(
        attrs
            .get("sampleFormat")
            .ok_or_else(|| err("XISF missing sampleFormat"))?,
    )?;

    // location="attachment:OFFSET:SIZE"
    let loc = attrs
        .get("location")
        .ok_or_else(|| err("XISF missing location"))?;
    let parts: Vec<&str> = loc.split(':').collect();
    if parts.len() != 3 || parts[0] != "attachment" {
        return Err(err(format!(
            "XISF location {loc:?} unsupported (attachment only)"
        )));
    }
    let off: usize = parts[1].parse().map_err(|_| err("bad attachment offset"))?;
    let size: usize = parts[2].parse().map_err(|_| err("bad attachment size"))?;
    if off < 16 + header_len || off.checked_add(size).is_none_or(|e| e > bytes.len()) {
        return Err(err("XISF attachment outside file"));
    }
    let raw = &bytes[off..off + size];

    // ── Decompression ──
    let count = w * h * channels;
    let expected = count * sample.bytes();
    let compression = match attrs.get("compression") {
        None => None,
        Some(c) => {
            let parts: Vec<&str> = c.split(':').collect();
            if !matches!(parts.len(), 2 | 3) {
                return Err(err(format!("XISF compression {c:?} malformed")));
            }
            let (codec_name, shuffled) = parts[0]
                .strip_suffix("+sh")
                .map_or((parts[0], false), |n| (n, true));
            let codec = match codec_name {
                "zlib" => "zlib",
                "lz4" | "lz4hc" => "lz4",
                "zstd" => "zstd",
                other => return Err(err(format!("XISF codec {other:?} unsupported"))),
            };
            let uncompressed: usize = parts[1].parse().map_err(|_| err("bad uncompressed size"))?;
            if uncompressed != expected {
                return Err(err("XISF uncompressed size mismatches geometry"));
            }
            let shuffle_item = if shuffled {
                Some(
                    parts
                        .get(2)
                        .ok_or_else(|| err("missing shuffle item size"))?
                        .parse::<usize>()
                        .map_err(|_| err("bad shuffle item size"))?,
                )
            } else {
                None
            };
            if let Some(it) = shuffle_item {
                if it != sample.bytes() {
                    return Err(err("shuffle item size mismatches sample format"));
                }
            }
            Some(Compression {
                codec,
                uncompressed,
                shuffle_item,
            })
        }
    };
    let data: Vec<u8> = match &compression {
        None => {
            if raw.len() != expected {
                return Err(err("XISF attachment size mismatches geometry"));
            }
            raw.to_vec()
        }
        Some(c) => {
            let out = match c.codec {
                "zlib" => {
                    use std::io::Read;
                    let mut d = flate2::read::ZlibDecoder::new(raw);
                    let mut v = Vec::with_capacity(c.uncompressed);
                    d.read_to_end(&mut v)
                        .map_err(|e| err(format!("zlib: {e}")))?;
                    v
                }
                "lz4" => lz4_flex::block::decompress(raw, c.uncompressed)
                    .map_err(|e| err(format!("lz4: {e}")))?,
                _ => {
                    use std::io::Read;
                    let mut d = ruzstd::decoding::StreamingDecoder::new(std::io::Cursor::new(raw))
                        .map_err(|e| err(format!("zstd: {e}")))?;
                    let mut v = Vec::with_capacity(c.uncompressed);
                    d.read_to_end(&mut v)
                        .map_err(|e| err(format!("zstd: {e}")))?;
                    v
                }
            };
            if out.len() != c.uncompressed {
                return Err(err("XISF decompressed size mismatch"));
            }
            out
        }
    };

    // ── Samples (un-shuffling lane-major bytes[lane*count + sample]) ──
    let item = sample.bytes();
    let shuffled = compression.as_ref().and_then(|c| c.shuffle_item).is_some();
    let read_sample = |i: usize| -> f64 {
        let mut b = [0u8; 8];
        for (lane, slot) in b.iter_mut().enumerate().take(item) {
            let idx = if shuffled {
                let stored_lane = if big_endian { item - lane - 1 } else { lane };
                stored_lane * count + i
            } else {
                i * item + lane
            };
            *slot = data[idx];
        }
        // Unshuffled data uses the declared byte order; the shuffle path is already little-endian
        match (sample, big_endian && !shuffled) {
            (SampleFormat::U8, _) => b[0] as f64,
            (SampleFormat::U16, false) => u16::from_le_bytes([b[0], b[1]]) as f64,
            (SampleFormat::U16, true) => u16::from_be_bytes([b[0], b[1]]) as f64,
            (SampleFormat::U32, false) => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            (SampleFormat::U32, true) => u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64,
            (SampleFormat::F32, false) => f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64,
            (SampleFormat::F32, true) => f32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f64,
            (SampleFormat::F64, false) => f64::from_le_bytes(b),
            (SampleFormat::F64, true) => f64::from_be_bytes(b),
        }
    };

    let n = w * h;
    let pixels = if channels == 1 {
        match (sample, shuffled, big_endian) {
            // Fast path: unshuffled native 8/16-bit grayscale
            (SampleFormat::U8, false, _) => PixelData::Luma8(data.clone()),
            (SampleFormat::U16, false, false) => PixelData::Luma16(
                data.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c))
                    .collect(),
            ),
            _ => PixelData::LumaF32((0..n).map(|i| read_sample(i) as f32).collect()),
        }
    } else {
        // Planar RGB → luminance
        PixelData::LumaF32(
            (0..n)
                .map(|i| {
                    let r = read_sample(i);
                    let g = read_sample(n + i);
                    let b = read_sample(2 * n + i);
                    (0.2126 * r + 0.7152 * g + 0.0722 * b) as f32
                })
                .collect(),
        )
    };

    let kwf = |k: &str| fits_kw.get(k).and_then(|v| v.trim().parse::<f64>().ok());
    let kws = |k: &str| {
        fits_kw
            .get(k)
            .map(|v| v.trim_matches(|c| c == '\'' || c == ' ').to_string())
            .filter(|s| !s.is_empty())
    };
    let meta = ImageMeta {
        exposure_s: sane_f64(kwf("EXPTIME").or_else(|| kwf("EXPOSURE")), 1e-6, 86_400.0),
        focal_len_mm: sane_f64(kwf("FOCALLEN"), 1.0, 100_000.0),
        pixel_size_um: sane_f64(kwf("XPIXSZ").or_else(|| kwf("PIXSIZE1")), 0.5, 50.0),
        binning: kwf("XBINNING")
            .filter(|b| (1.0..=16.0).contains(b))
            .map(|b| b as u32),
        date_obs: kws("DATE-OBS"),
        instrument: kws("INSTRUME"),
        bayer_pattern: kws("BAYERPAT"),
        ..ImageMeta::bare(w as u32, h as u32, SourceFormat::Xisf, (item * 8) as u8)
    };
    Ok((
        Frame {
            width: w as u32,
            height: h as u32,
            row_stride_bytes: None,
            pixels,
        },
        meta,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_xisf(
        sample_format: &str,
        compression: Option<&str>,
        w: usize,
        h: usize,
        channels: usize,
        extra: &str,
        payload: &[u8],
    ) -> Vec<u8> {
        let color = if channels == 3 { "RGB" } else { "Gray" };
        let attachment_at = 4096usize;
        let xml = format!(
            r#"<?xml version="1.0"?><xisf version="1.0"><Image geometry="{w}:{h}:{channels}" sampleFormat="{sample_format}" colorSpace="{color}" location="attachment:{attachment_at}:{}"{}>{extra}</Image></xisf>"#,
            payload.len(),
            compression
                .map(|c| format!(r#" compression="{c}""#))
                .unwrap_or_default(),
        );
        let mut out = Vec::new();
        out.extend(SIGNATURE);
        out.extend((xml.len() as u32).to_le_bytes());
        out.extend([0u8; 4]);
        out.extend(xml.as_bytes());
        out.resize(attachment_at, 0);
        out.extend(payload);
        out
    }

    #[test]
    fn u16_gray_uncompressed() {
        let payload: Vec<u8> = [100u16, 200, 300, 400]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let f = synth_xisf("UInt16", None, 2, 2, 1, "", &payload);
        let (frame, meta) = read_xisf_bytes(&f).unwrap();
        assert_eq!(meta.source, SourceFormat::Xisf);
        assert_eq!(
            frame.to_luma_f32().unwrap(),
            vec![100.0, 200.0, 300.0, 400.0]
        );
    }

    #[test]
    fn u16_gray_zlib_and_zstd_and_lz4() {
        let raw: Vec<u8> = (0..64u16).flat_map(|v| (v * 100).to_le_bytes()).collect();
        for (codec, packed) in [
            ("zlib", {
                use std::io::Write;
                let mut e =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(&raw).unwrap();
                e.finish().unwrap()
            }),
            ("lz4", lz4_flex::block::compress(&raw)),
            ("zstd", {
                // ruzstd only decodes and core has no zstd dev-dependency, so build
                // the smallest valid frame by hand: a single RAW block.
                let mut v = vec![0x28, 0xB5, 0x2F, 0xFD]; // magic
                                                          // Frame header descriptor: single_segment=1 (bit 5) → 1-byte FCS, no window byte
                v.push(0x20);
                let len = raw.len();
                assert!(len < 256, "test frame FCS 1-byte limit");
                v.push(len as u8); // Frame Content Size
                                   // block header: last=1, type=0 (raw), size=len (3-byte little-endian bitfield)
                let bh = 1u32 | ((len as u32) << 3);
                v.extend(&bh.to_le_bytes()[0..3]);
                v.extend(&raw);
                v
            }),
        ] {
            let f = synth_xisf(
                "UInt16",
                Some(&format!("{codec}:{}", raw.len())),
                8,
                8,
                1,
                "",
                &packed,
            );
            let (frame, _) = read_xisf_bytes(&f).unwrap_or_else(|e| panic!("{codec}: {e}"));
            let l = frame.to_luma_f32().unwrap();
            assert_eq!(l[1], 100.0, "{codec}");
            assert_eq!(l[63], 6300.0, "{codec}");
        }
    }

    #[test]
    fn u16_gray_zlib_shuffled() {
        // Lane-major shuffle by hand: all of lane 0 first, then lane 1
        let values: Vec<u16> = (0..16).map(|v| v * 1000).collect();
        let count = values.len();
        let mut shuffled = vec![0u8; count * 2];
        for (i, v) in values.iter().enumerate() {
            let b = v.to_le_bytes();
            shuffled[i] = b[0];
            shuffled[count + i] = b[1];
        }
        use std::io::Write;
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&shuffled).unwrap();
        let packed = e.finish().unwrap();
        let f = synth_xisf(
            "UInt16",
            Some(&format!("zlib+sh:{}:2", count * 2)),
            4,
            4,
            1,
            "",
            &packed,
        );
        let (frame, _) = read_xisf_bytes(&f).unwrap();
        assert_eq!(frame.to_luma_f32().unwrap()[15], 15000.0);
    }

    #[test]
    fn f32_rgb_planar_luma_mix() {
        let n = 4usize;
        let mut payload = Vec::new();
        for plane_val in [1.0f32, 2.0, 4.0] {
            for _ in 0..n {
                payload.extend(plane_val.to_le_bytes());
            }
        }
        let f = synth_xisf("Float32", None, 2, 2, 3, "", &payload);
        let (frame, _) = read_xisf_bytes(&f).unwrap();
        let expect = 0.2126 * 1.0 + 0.7152 * 2.0 + 0.0722 * 4.0;
        for v in frame.to_luma_f32().unwrap() {
            assert!((v - expect as f32).abs() < 1e-5);
        }
    }

    #[test]
    fn fits_keywords_flow_into_meta_with_sanitize() {
        let payload = vec![0u8; 4];
        let extra = r#"<FITSKeyword name="FOCALLEN" value="456.0"/><FITSKeyword name="XPIXSZ" value="3.76"/><FITSKeyword name="EXPTIME" value="-5"/>"#;
        let f = synth_xisf("UInt8", None, 2, 2, 1, extra, &payload);
        let (_, meta) = read_xisf_bytes(&f).unwrap();
        assert_eq!(meta.focal_len_mm, Some(456.0));
        assert!(
            meta.exposure_s.is_none(),
            "negative exposure must be dropped"
        );
    }

    #[test]
    fn unsupported_surfaces_err_clearly() {
        let f = synth_xisf("UInt16", None, 2, 2, 2, "", &[0u8; 16]);
        let e = read_xisf_bytes(&f).unwrap_err().to_string();
        assert!(e.contains("unsupported"), "{e}");
        assert!(read_xisf_bytes(b"XISF0100\x00\x00\x00\x00\x00\x00\x00\x00").is_err());
    }
}
