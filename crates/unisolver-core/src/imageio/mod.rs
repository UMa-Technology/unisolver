//! Unified astronomical image input (feature = "imageio").
//! Three rules:
//!   1. Never assume headers exist: all metadata is optional; missing is not an error.
//!   2. Never assume headers are right: a header FOV is only a hint (±15% tolerance,
//!      with the full ladder as fallback).
//!   3. FITS / XISF / PNG / JPEG / TIFF, dispatched on magic bytes.
use crate::{CoreError, Frame, Result};

mod fits;
mod raster;
mod xisf;
pub use fits::read_fits_bytes;
pub use xisf::read_xisf_bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Fits,
    Xisf,
    Png,
    Jpeg,
    Tiff,
}

#[derive(Debug, Clone)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    pub source: SourceFormat,
    pub bit_depth: u8,
    pub exposure_s: Option<f64>,
    pub focal_len_mm: Option<f64>,
    pub pixel_size_um: Option<f64>,
    pub binning: Option<u32>,
    pub date_obs: Option<String>,
    pub instrument: Option<String>,
    pub bayer_pattern: Option<String>,
}

impl ImageMeta {
    pub(crate) fn bare(width: u32, height: u32, source: SourceFormat, bit_depth: u8) -> Self {
        Self {
            width,
            height,
            source,
            bit_depth,
            exposure_s: None,
            focal_len_mm: None,
            pixel_size_um: None,
            binning: None,
            date_obs: None,
            instrument: None,
            bayer_pattern: None,
        }
    }

    /// Horizontal FOV **hint** from the header (not ground truth). Needs both focal
    /// length and pixel size, sanitized by the parser, and a result in (0.2°, 120°);
    /// otherwise None. Use it as a hint, see [`Self::solve_hints`].
    pub fn fov_hint_deg(&self) -> Option<f64> {
        let (f, p) = (self.focal_len_mm?, self.pixel_size_um?);
        if !(f.is_finite() && f > 0.0 && p.is_finite() && (0.5..=50.0).contains(&p)) {
            return None;
        }
        let fov = 2.0
            * (self.width as f64 * p / 1000.0 / 2.0 / f)
                .atan()
                .to_degrees();
        (0.2..=120.0).contains(&fov).then_some(fov)
    }

    /// Hint rungs (0–2), each with ±15% tolerance: the FOV hint and, when binning > 1,
    /// hint × binning, because XPIXSZ may mean either the physical or the binned pixel.
    pub fn solve_hints(&self) -> Vec<crate::FovPreset> {
        let mut out = Vec::new();
        if let Some(h) = self.fov_hint_deg() {
            out.push(crate::FovPreset {
                fov_deg: h as f32,
                max_error_deg: (h * 0.15) as f32,
            });
            if let Some(b) = self.binning.filter(|&b| b > 1 && b <= 4) {
                let hb = h * b as f64;
                if (0.2..=120.0).contains(&hb) {
                    out.push(crate::FovPreset {
                        fov_deg: hb as f32,
                        max_error_deg: (hb * 0.15) as f32,
                    });
                }
            }
        }
        out
    }
}

/// Header value sanitizing shared by the parsers: non-finite or out-of-range means absent.
pub(crate) fn sane_f64(v: Option<f64>, min: f64, max: f64) -> Option<f64> {
    v.filter(|x| x.is_finite() && (min..=max).contains(x))
}

pub fn load_image(path: &str) -> Result<(Frame, ImageMeta)> {
    load_image_bytes(&std::fs::read(path)?)
}

pub fn load_image_bytes(bytes: &[u8]) -> Result<(Frame, ImageMeta)> {
    if bytes.len() < 12 {
        return Err(CoreError::InvalidInput("image too small to sniff".into()));
    }
    if &bytes[0..6] == b"SIMPLE" {
        return fits::read_fits_bytes(bytes);
    }
    if &bytes[0..8] == b"XISF0100" {
        return xisf::read_xisf_bytes(bytes);
    }
    if bytes[0..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        return raster::read_raster_bytes(bytes, SourceFormat::Png);
    }
    if bytes[0..2] == [0xFF, 0xD8] {
        return raster::read_raster_bytes(bytes, SourceFormat::Jpeg);
    }
    if &bytes[0..4] == b"II*\0" || &bytes[0..4] == b"MM\0*" {
        return raster::read_raster_bytes(bytes, SourceFormat::Tiff);
    }
    Err(CoreError::InvalidInput(
        "unrecognized image format (supported: FITS, XISF, PNG, JPEG, TIFF)".into(),
    ))
}

/// Suggested extraction profile from the input format (better informed than a guess
/// inside solve): astronomical formats (FITS/XISF) → CleanSensor (σ5 keeps faint stars);
/// consumer rasters → PhoneJpeg (σ10 suppresses JPEG noise). Both fall back on failure.
pub fn suggested_profile(meta: &ImageMeta) -> crate::ExtractionProfile {
    match meta.source {
        SourceFormat::Fits | SourceFormat::Xisf => crate::ExtractionProfile::CleanSensor,
        SourceFormat::Png | SourceFormat::Jpeg | SourceFormat::Tiff => {
            crate::ExtractionProfile::PhoneJpeg
        }
    }
}
