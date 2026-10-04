//! Unified astronomical image input (feature = "imageio").
//! Three rules:
//!   1. Never assume headers exist: all metadata is optional; missing is not an error.
//!   2. Never assume headers are right: a header FOV is only a hint (±15% tolerance,
//!      with the full ladder as fallback).
//!   3. FITS / XISF / PNG / JPEG / TIFF, dispatched on magic bytes. HEIC/HEIF is not
//!      decoded (only LGPL decoders exist, with HEVC patents): the error points to the
//!      platform decoder and the frame entries.
use crate::{CoreError, Frame, Result};

mod exif;
mod fits;
mod preview;
mod raster;
mod time;
mod xisf;
pub use fits::read_fits_bytes;
pub use preview::{load_preview, preview, Preview};
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
    /// As written: FITS/XISF `DATE-OBS`; for EXIF, DateTimeOriginal in ISO form (with the
    /// zone when EXIF gives one, local time otherwise)
    pub date_obs: Option<String>,
    pub instrument: Option<String>,
    pub bayer_pattern: Option<String>,
    /// 35 mm-equivalent focal length (EXIF FocalLengthIn35mmFilm): the FOV hint phones give
    pub focal_35mm_mm: Option<f64>,
    /// Observation time, Unix ms UTC, mid-exposure when the exposure is known. Only when the
    /// header pins the zone: FITS/XISF `DATE-AVG`, or `DATE-OBS` (UTC by definition) plus half
    /// the exposure; EXIF DateTimeOriginal with OffsetTimeOriginal, or with a GPS time that
    /// fixes the zone. File entries report it in the outcome, for the solar-system layer and
    /// the horizontal grid.
    pub observation_unix_ms: Option<i64>,
    /// Where the photo was taken, from EXIF GPS. Never used by the solve; file entries report
    /// it in the outcome for the annotator (the moon's parallax, satellites)
    pub observer: Option<crate::Observer>,
    /// Where the telescope pointed, ICRS degrees `[RA, Dec]`, from the header: XISF
    /// `Observation:Center:RA`/`Dec`, then FITS keywords `RA`/`DEC` (degrees, or sexagesimal
    /// text), `OBJCTRA`/`OBJCTDEC` (sexagesimal), a celestial `CRVAL1`/`CRVAL2`. A hint for the
    /// narrow-field engine only, see [`Self::apply_pointing`].
    pub pointing_deg: Option<[f64; 2]>,
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
            focal_35mm_mm: None,
            observation_unix_ms: None,
            observer: None,
            pointing_deg: None,
        }
    }

    /// Fills in what EXIF gives (rasters only; FITS/XISF use their own keywords).
    fn apply_exif(&mut self, e: &exif::Exif) {
        self.focal_len_mm = e.focal_mm;
        self.focal_35mm_mm = e.focal_35mm;
        // Pitch per **stored** pixel: the sensor width the camera recorded, spread over the
        // width actually stored (smaller when the photo was resized after capture)
        self.pixel_size_um = e.pitch_um.map(|p| {
            p * e
                .recorded_width
                .map_or(1.0, |rw| rw as f64 / self.width as f64)
        });
        self.exposure_s = e.exposure_s;
        self.date_obs = e.date_obs();
        self.observation_unix_ms = e.observation_unix_ms();
        self.observer = e.observer;
        self.instrument = match (&e.make, &e.model) {
            (Some(make), Some(model)) if !model.starts_with(make.as_str()) => {
                Some(format!("{make} {model}"))
            }
            (_, Some(model)) => Some(model.clone()),
            (make, None) => make.clone(),
        };
    }

    /// The header's observation time fills in when the caller gave none (what the caller
    /// passes always wins). File entries call this.
    pub fn apply_time(&self, opts: &mut crate::SolveOptions) {
        if opts.observation_unix_ms.is_none() {
            opts.observation_unix_ms = self.observation_unix_ms;
        }
    }

    /// Puts the header's observation place on a file entry's outcome (its time is already
    /// there, through [`Self::apply_time`]).
    pub fn apply_place(&self, out: &mut crate::SolveOutcome) {
        out.observer = out.observer.or(self.observer);
    }

    /// The header's pointing fills in when the caller gave none, searched with the default
    /// radius (pool file entries call this; what the caller passes always wins).
    pub fn apply_pointing(&self, opts: &mut crate::SolveOptions) {
        if opts.pointing_hint.is_none() {
            opts.pointing_hint = self
                .pointing_deg
                .map(|[ra_deg, dec_deg]| crate::PointingHint {
                    ra_deg,
                    dec_deg,
                    radius_deg: None,
                });
        }
    }

    /// Horizontal FOV **hint** from the header (not ground truth). Needs both focal
    /// length and pixel size, sanitized by the parser, and a result in (0.2°, 120°);
    /// otherwise None. Use it as a hint, see [`Self::solve_hints`].
    ///
    /// The pixel size is per **stored** pixel: the parsers bound the physical pitch
    /// (0.5–50 µm), but a photo resized after capture has proportionally larger pixels, so
    /// only a loose bound applies here.
    pub fn fov_hint_deg(&self) -> Option<f64> {
        let (f, p) = (self.focal_len_mm?, self.pixel_size_um?);
        if !(f.is_finite() && f > 0.0 && p.is_finite() && (0.1..=5000.0).contains(&p)) {
            return None;
        }
        let fov = 2.0
            * (self.width as f64 * p / 1000.0 / 2.0 / f)
                .atan()
                .to_degrees();
        (0.2..=120.0).contains(&fov).then_some(fov)
    }

    /// Horizontal FOV **hint** from the 35 mm-equivalent focal length (EXIF), through the
    /// CIPA diagonal convention; None without one or outside (0.2°, 120°).
    pub fn fov_hint_35mm_deg(&self) -> Option<f64> {
        crate::solver::focal_35mm_hint(self.focal_35mm_mm? as f32, self.width, self.height)
            .map(|p| p.fov_deg as f64)
    }

    /// Hint rungs (0–3), each with ±15% tolerance, rungs within 1° merged: the 35 mm
    /// equivalent (phones write it and it already accounts for crops), the focal length
    /// with the pixel size and, when binning > 1, that hint × binning, because XPIXSZ may
    /// mean either the physical or the binned pixel.
    pub fn solve_hints(&self) -> Vec<crate::FovPreset> {
        let mut fovs: Vec<f64> = Vec::new();
        fovs.extend(self.fov_hint_35mm_deg());
        if let Some(h) = self.fov_hint_deg() {
            fovs.push(h);
            if let Some(b) = self.binning.filter(|&b| b > 1 && b <= 4) {
                fovs.push(h * b as f64);
            }
        }
        let mut out: Vec<crate::FovPreset> = Vec::new();
        for h in fovs.into_iter().filter(|h| (0.2..=120.0).contains(h)) {
            if !out.iter().any(|q| (q.fov_deg as f64 - h).abs() < 1.0) {
                out.push(crate::solver::hint_preset(h as f32));
            }
        }
        out
    }
}

/// Header value sanitizing shared by the parsers: non-finite or out-of-range means absent.
pub(crate) fn sane_f64(v: Option<f64>, min: f64, max: f64) -> Option<f64> {
    v.filter(|x| x.is_finite() && (min..=max).contains(x))
}

/// FITS/XISF observation time: `DATE-AVG` (the midpoint) when present, else `DATE-OBS` (the
/// start) plus half the exposure. Both are UTC by the FITS standard.
pub(crate) fn header_time(
    date_avg: Option<&str>,
    date_obs: Option<&str>,
    exposure_s: Option<f64>,
) -> Option<i64> {
    date_avg
        .and_then(time::civil_ms)
        .or_else(|| time::mid_exposure(date_obs.and_then(time::civil_ms), exposure_s))
}

/// Pointing from FITS keywords (`get` returns a value with its quotes trimmed): `RA`/`DEC`
/// first, then `OBJCTRA`/`OBJCTDEC`, then `CRVAL1`/`CRVAL2` when `CTYPE1` is celestial.
pub(crate) fn header_pointing(get: impl Fn(&str) -> Option<String>) -> Option<[f64; 2]> {
    let pair = |ra: &str, dec: &str| Some([parse_ra(&get(ra)?)?, parse_dec(&get(dec)?)?]);
    pair("RA", "DEC")
        .or_else(|| pair("OBJCTRA", "OBJCTDEC"))
        .or_else(|| {
            get("CTYPE1").filter(|t| t.starts_with("RA"))?;
            let ra = get("CRVAL1")?.trim().parse::<f64>().ok()?;
            let dec = parse_dec(&get("CRVAL2")?)?;
            ra.is_finite().then_some([ra.rem_euclid(360.0), dec])
        })
}

/// A header angle: a plain number, or two or three sexagesimal fields separated by spaces or
/// colons ("-05 23 28", "05:35:17.3"). Returns the value and whether it had fields.
fn header_angle(v: &str) -> Option<(f64, bool)> {
    let v = v.trim();
    if let Ok(x) = v.parse::<f64>() {
        return x.is_finite().then_some((x, false));
    }
    let parts: Vec<&str> = v.split([' ', ':']).filter(|p| !p.is_empty()).collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let negative = parts[0].starts_with('-');
    let d = parts[0]
        .trim_start_matches(['+', '-'])
        .parse::<f64>()
        .ok()?;
    let m = parts[1].parse::<f64>().ok()?;
    let s = match parts.get(2) {
        Some(p) => p.parse::<f64>().ok()?,
        None => 0.0,
    };
    if !(d.is_finite() && (0.0..60.0).contains(&m) && (0.0..60.0).contains(&s)) {
        return None;
    }
    let x = d + m / 60.0 + s / 3600.0;
    Some((if negative { -x } else { x }, true))
}

/// Right ascension as written: a number is degrees, sexagesimal fields are hours
fn parse_ra(v: &str) -> Option<f64> {
    let (x, fields) = header_angle(v)?;
    let deg = if fields { x * 15.0 } else { x };
    (0.0..=360.0)
        .contains(&deg)
        .then_some(deg.rem_euclid(360.0))
}

/// Declination in degrees, as a number or sexagesimal fields
fn parse_dec(v: &str) -> Option<f64> {
    header_angle(v)
        .map(|(x, _)| x)
        .filter(|d| (-90.0..=90.0).contains(d))
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
    if &bytes[4..8] == b"ftyp"
        && matches!(
            &bytes[8..12],
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"mif1" | b"msf1" | b"avif"
        )
    {
        return Err(CoreError::InvalidInput(
            "HEIC/HEIF is not decoded by the engine: decode it with the platform (ImageIO on \
             Apple, ImageDecoder on Android) and pass the pixels to a frame entry, with the EXIF \
             35 mm focal length and capture time in the options"
                .into(),
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_pointing_fills_in_only_when_the_caller_gave_none() {
        let mut meta = ImageMeta::bare(10, 10, SourceFormat::Fits, 16);
        meta.pointing_deg = Some([10.0, 20.0]);
        let mut o = crate::SolveOptions::new(1.0);
        meta.apply_pointing(&mut o);
        assert_eq!(
            o.pointing_hint,
            Some(crate::PointingHint {
                ra_deg: 10.0,
                dec_deg: 20.0,
                radius_deg: None
            })
        );
        let mine = crate::PointingHint {
            ra_deg: 1.0,
            dec_deg: 2.0,
            radius_deg: Some(0.5),
        };
        o.pointing_hint = Some(mine);
        meta.apply_pointing(&mut o);
        assert_eq!(o.pointing_hint, Some(mine));
    }
}
