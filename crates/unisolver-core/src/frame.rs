use crate::{CoreError, Result};

#[derive(Debug, Clone)]
pub enum PixelData {
    Luma8(Vec<u8>),
    Luma16(Vec<u16>),
    LumaF32(Vec<f32>),
    Rgba8(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Bytes per source row (Android YUV_420_888 Y-plane rowStride, iOS bytesPerRow).
    /// None means tightly packed.
    pub row_stride_bytes: Option<u32>,
    pub pixels: PixelData,
}

impl Frame {
    pub fn to_luma_f32(&self) -> Result<Vec<f32>> {
        self.to_luma_f32_rows(0, self.height as usize)
    }

    /// Rows `y0..y1` as luminance, validating the whole buffer as [`Self::to_luma_f32`] does:
    /// large frames are extracted band by band without a full-frame f32 copy.
    pub fn to_luma_f32_rows(&self, y0: usize, y1: usize) -> Result<Vec<f32>> {
        let mut out = Vec::new();
        self.to_luma_f32_rows_into(y0, y1, &mut out)?;
        Ok(out)
    }

    /// As [`Self::to_luma_f32_rows`], into `out` (cleared first; its capacity is reused).
    pub fn to_luma_f32_rows_into(&self, y0: usize, y1: usize, out: &mut Vec<f32>) -> Result<()> {
        let (w, h) = (self.width as usize, self.height as usize);
        if y0 >= y1 || y1 > h {
            return Err(CoreError::InvalidInput(format!(
                "rows {y0}..{y1} outside a frame of height {h}"
            )));
        }
        if w == 0 || h == 0 {
            return Err(CoreError::InvalidInput("frame has zero dimension".into()));
        }
        let bpp = match &self.pixels {
            PixelData::Luma8(_) => 1,
            PixelData::Luma16(_) => 2,
            PixelData::LumaF32(_) => 4,
            PixelData::Rgba8(_) => 4,
        };
        let row_bytes = w * bpp;
        let stride = self
            .row_stride_bytes
            .map(|s| s as usize)
            .unwrap_or(row_bytes);
        if stride < row_bytes {
            return Err(CoreError::InvalidInput(format!(
                "row_stride {stride} < row bytes {row_bytes}"
            )));
        }
        let need = stride * (h - 1) + row_bytes;
        out.clear();
        out.reserve(w * (y1 - y0));
        match &self.pixels {
            PixelData::Luma8(b) => {
                if b.len() < need {
                    return Err(CoreError::InvalidInput(format!(
                        "buffer {} < needed {need}",
                        b.len()
                    )));
                }
                for y in y0..y1 {
                    let row = &b[y * stride..y * stride + row_bytes];
                    out.extend(row.iter().map(|&v| v as f32));
                }
            }
            PixelData::Luma16(b) => {
                if !stride.is_multiple_of(2) {
                    return Err(CoreError::InvalidInput("Luma16 stride must be even".into()));
                }
                let se = stride / 2;
                if b.len() * 2 < need {
                    return Err(CoreError::InvalidInput(format!(
                        "buffer {} u16 < needed {} bytes",
                        b.len(),
                        need
                    )));
                }
                for y in y0..y1 {
                    out.extend(b[y * se..y * se + w].iter().map(|&v| v as f32));
                }
            }
            PixelData::LumaF32(b) => {
                if !stride.is_multiple_of(4) {
                    return Err(CoreError::InvalidInput(
                        "LumaF32 stride must be multiple of 4".into(),
                    ));
                }
                let se = stride / 4;
                if b.len() * 4 < need {
                    return Err(CoreError::InvalidInput("f32 buffer too small".into()));
                }
                for y in y0..y1 {
                    out.extend_from_slice(&b[y * se..y * se + w]);
                }
            }
            PixelData::Rgba8(b) => {
                if b.len() < need {
                    return Err(CoreError::InvalidInput(format!(
                        "rgba buffer {} < needed {need}",
                        b.len()
                    )));
                }
                for y in y0..y1 {
                    let row = &b[y * stride..y * stride + row_bytes];
                    out.extend(row.as_chunks::<4>().0.iter().map(|p| {
                        0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32
                    }));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stride_is_honored_for_luma8() {
        // 2x2 image, 4-byte stride (2 padding bytes per row)
        let buf = vec![10u8, 20, 99, 99, 30, 40, 99, 99];
        let f = Frame {
            width: 2,
            height: 2,
            row_stride_bytes: Some(4),
            pixels: PixelData::Luma8(buf),
        };
        assert_eq!(f.to_luma_f32().unwrap(), vec![10.0, 20.0, 30.0, 40.0]);
    }
    #[test]
    fn row_ranges_match_the_full_conversion() {
        let f = Frame {
            width: 3,
            height: 4,
            row_stride_bytes: Some(8),
            pixels: PixelData::Luma16((0..16u16).collect()),
        };
        let full = f.to_luma_f32().unwrap();
        assert_eq!(full.len(), 12);
        assert_eq!(f.to_luma_f32_rows(1, 3).unwrap(), full[3..9]);
        assert!(f.to_luma_f32_rows(2, 2).is_err());
        assert!(f.to_luma_f32_rows(3, 5).is_err());
    }

    #[test]
    fn rgba_luma_and_len_checks() {
        let f = Frame {
            width: 1,
            height: 1,
            row_stride_bytes: None,
            pixels: PixelData::Rgba8(vec![255, 0, 0, 255]),
        };
        let l = f.to_luma_f32().unwrap();
        assert!((l[0] - 0.2126 * 255.0).abs() < 0.01);
        let bad = Frame {
            width: 3,
            height: 2,
            row_stride_bytes: None,
            pixels: PixelData::Luma8(vec![0; 5]),
        };
        assert!(bad.to_luma_f32().is_err());
        // A stride shorter than the row must be rejected
        let bad2 = Frame {
            width: 4,
            height: 1,
            row_stride_bytes: Some(2),
            pixels: PixelData::Luma8(vec![0; 4]),
        };
        assert!(bad2.to_luma_f32().is_err());
    }
}
