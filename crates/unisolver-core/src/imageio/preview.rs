//! Display previews: an image of any supported format, box-averaged down to a size a screen
//! can use and auto-stretched to 8 bits. Astronomical frames (FITS, XISF, 16-bit TIFF) hold
//! linear data whose sky sits in the bottom percent of the range, so shown as-is they are
//! black; the stretch lifts the background to a quarter of full scale.
use crate::{Frame, Result};

/// An 8-bit greyscale preview. Pixel (x, y) of the preview covers source pixels
/// `[x·f, (x+1)·f) × [y·f, (y+1)·f)` with `f = source_width / width` rounded up, so drawn over
/// the source's size it lines up with the source's pixel coordinates.
#[derive(Debug, Clone)]
pub struct Preview {
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    /// Row-major, `width × height`
    pub luma: Vec<u8>,
}

impl Preview {
    /// The same pixels as RGBA (opaque grey), ready for image APIs that take 32-bit pixels.
    pub fn rgba(&self) -> Vec<u8> {
        self.luma.iter().flat_map(|&v| [v, v, v, 255]).collect()
    }
}

/// Background level the stretch aims for (PixInsight's AutoSTF default)
const TARGET_BACKGROUND: f64 = 0.25;
/// Shadows clip: this many (normalized) MADs below the median
const SHADOWS_CLIP_MADS: f64 = 2.8;
/// Samples for the statistics; more buys nothing visible
const STAT_SAMPLES: usize = 200_000;

/// Midtones transfer function: 0 → 0, 1 → 1, `m` → 0.5
fn mtf(x: f64, m: f64) -> f64 {
    if x <= 0.0 {
        0.0
    } else if x >= 1.0 {
        1.0
    } else {
        (m - 1.0) * x / ((2.0 * m - 1.0) * x - m)
    }
}

fn median(v: &mut [f64]) -> f64 {
    let mid = v.len() / 2;
    *v.select_nth_unstable_by(mid, f64::total_cmp).1
}

/// Auto-stretch to 8 bits: normalize to the data range, clip the shadows a few MADs below the
/// median, then choose the midtones so the median lands on [`TARGET_BACKGROUND`]. A flat image
/// (no spread) is shown linearly; non-finite pixels are black.
fn stretch(values: &[f32]) -> Vec<u8> {
    let step = (values.len() / STAT_SAMPLES).max(1);
    let mut sample: Vec<f64> = values
        .iter()
        .step_by(step)
        .filter(|v| v.is_finite())
        .map(|&v| v as f64)
        .collect();
    let (lo, hi) = values
        .iter()
        .filter(|v| v.is_finite())
        .fold((f64::MAX, f64::MIN), |(a, b), &v| {
            (a.min(v as f64), b.max(v as f64))
        });
    if sample.is_empty() || hi <= lo {
        return vec![0; values.len()];
    }
    let span = hi - lo;
    for s in &mut sample {
        *s = (*s - lo) / span;
    }
    let med = median(&mut sample);
    let mut dev: Vec<f64> = sample.iter().map(|s| (s - med).abs()).collect();
    let mad = median(&mut dev) * 1.4826;
    let (c0, m) = if mad < 1e-9 {
        (0.0, 0.5) // no spread to work with: linear
    } else {
        let c0 = (med - SHADOWS_CLIP_MADS * mad).clamp(0.0, 1.0);
        let x0 = ((med - c0) / (1.0 - c0)).clamp(1e-6, 1.0 - 1e-6);
        (c0, mtf(x0, TARGET_BACKGROUND))
    };
    values
        .iter()
        .map(|&v| {
            if !v.is_finite() {
                return 0;
            }
            let x = ((v as f64 - lo) / span - c0) / (1.0 - c0);
            (mtf(x, m) * 255.0).round() as u8
        })
        .collect()
}

/// A preview of `frame` no larger than `max_side` on either side (integer box averaging, which
/// also evens out a colour camera's Bayer mosaic), auto-stretched. The source is read band by
/// band, never as a whole f32 copy.
pub fn preview(frame: &Frame, max_side: u32) -> Result<Preview> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let f = w.max(h).div_ceil(max_side.max(1) as usize).max(1);
    let (pw, ph) = (w.div_ceil(f), h.div_ceil(f));
    let mut small = vec![f32::NAN; pw * ph];
    let mut band = Vec::new();
    for py in 0..ph {
        let (y0, y1) = (py * f, ((py + 1) * f).min(h));
        frame.to_luma_f32_rows_into(y0, y1, &mut band)?;
        for px in 0..pw {
            let (x0, x1) = (px * f, ((px + 1) * f).min(w));
            let (mut sum, mut n) = (0.0f64, 0u32);
            for row in band.chunks_exact(w) {
                for &v in &row[x0..x1] {
                    if v.is_finite() {
                        sum += v as f64;
                        n += 1;
                    }
                }
            }
            if n > 0 {
                small[py * pw + px] = (sum / n as f64) as f32;
            }
        }
    }
    Ok(Preview {
        width: pw as u32,
        height: ph as u32,
        source_width: frame.width,
        source_height: frame.height,
        luma: stretch(&small),
    })
}

/// [`preview`] of an image file (any of the five formats).
pub fn load_preview(path: &str, max_side: u32) -> Result<Preview> {
    preview(&super::load_image(path)?.0, max_side)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PixelData;

    /// Linear sky: a dim background with noise, a few bright stars and a hot column, in a
    /// 16-bit range, as a cooled camera records it.
    fn sky(w: u32, h: u32) -> Frame {
        let mut state = 7u64;
        let mut noise = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % 41) as f32 - 20.0
        };
        let mut px: Vec<u16> = (0..w * h).map(|_| (1000.0 + noise()) as u16).collect();
        for &(sx, sy) in &[(20u32, 30u32), (70, 12), (90, 60)] {
            px[(sy * w + sx) as usize] = 60_000;
        }
        for y in 0..h {
            px[(y * w + 5) as usize] = 65_535;
        }
        Frame {
            width: w,
            height: h,
            row_stride_bytes: None,
            pixels: PixelData::Luma16(px),
        }
    }

    #[test]
    fn linear_sky_becomes_visible() {
        let p = preview(&sky(120, 80), 4096).unwrap();
        assert_eq!((p.width, p.height), (120, 80));
        let mut v = p.luma.clone();
        v.sort_unstable();
        let med = v[v.len() / 2];
        // The background lands near a quarter of full scale instead of 1000/65535 ≈ 0.4
        assert!((58..=70).contains(&med), "median {med}");
        assert_eq!(p.luma[(30 * 120 + 20) as usize], 255, "stars saturate");
        assert!(v[v.len() / 50] < med, "noise below the median stays darker");
    }

    #[test]
    fn downsampling_rounds_up_and_keeps_the_source_size() {
        let p = preview(&sky(1000, 333), 100).unwrap();
        // f = ceil(1000 / 100) = 10 → 100 × 34
        assert_eq!((p.width, p.height), (100, 34));
        assert_eq!((p.source_width, p.source_height), (1000, 333));
        assert_eq!(p.luma.len(), 100 * 34);
        assert_eq!(p.rgba().len(), 100 * 34 * 4);
        assert_eq!(&p.rgba()[..4], &[p.luma[0], p.luma[0], p.luma[0], 255]);
    }

    #[test]
    fn flat_and_empty_images_do_not_break() {
        let flat = Frame {
            width: 4,
            height: 4,
            row_stride_bytes: None,
            pixels: PixelData::LumaF32(vec![3.0; 16]),
        };
        assert!(preview(&flat, 16).unwrap().luma.iter().all(|&v| v == 0));
        let nan = Frame {
            width: 2,
            height: 2,
            row_stride_bytes: None,
            pixels: PixelData::LumaF32(vec![f32::NAN, 1.0, 2.0, 4.0]),
        };
        let p = preview(&nan, 16).unwrap();
        assert_eq!(p.luma[0], 0);
        assert_eq!(p.luma[3], 255);
    }
}
