//! Batch transforms through a solve's WCS, for overlays the annotation layers do not draw
//! (framing marks, crosshairs, tap-to-identify). Batched because every call crosses the
//! Dart ⇄ Rust boundary: thousands of single-point calls per frame would be slow.
use crate::api::types::WcsDto;
use anyhow::Result;
use flutter_rust_bridge::frb;
use unisolver_core as core;

fn batch(
    wcs: WcsDto,
    points: Vec<f64>,
    map: impl Fn(&core::Wcs, &[[f64; 2]]) -> Vec<Option<[f64; 2]>>,
) -> Result<Vec<f64>> {
    anyhow::ensure!(
        points.len().is_multiple_of(2),
        "points must be interleaved pairs"
    );
    let w: core::Wcs = wcs.try_into()?;
    let pts: Vec<[f64; 2]> = points.as_chunks::<2>().0.to_vec();
    Ok(map(&w, &pts)
        .into_iter()
        .flat_map(|p| p.unwrap_or([f64::NAN, f64::NAN]))
        .collect())
}

/// Sky → pixel: interleaved `[ra0, dec0, ra1, dec1, …]` (degrees) to interleaved top-left
/// pixels. A point the lens model cannot place (behind the camera, or beyond 1.2× the
/// frame's corner distance, where the distortion polynomial folds points back in) comes back
/// as NaN, NaN.
#[frb(sync)]
pub fn wcs_sky_to_pixels(wcs: WcsDto, radec: Vec<f64>) -> Result<Vec<f64>> {
    batch(wcs, radec, |w, p| w.sky_to_pixels(p))
}

/// Pixel → sky: interleaved top-left pixels to interleaved `[ra, dec]` (degrees); NaN, NaN for
/// pixels more than a quarter frame outside the image.
#[frb(sync)]
pub fn wcs_pixels_to_sky(wcs: WcsDto, pixels: Vec<f64>) -> Result<Vec<f64>> {
    batch(wcs, pixels, |w, p| w.pixels_to_sky(p))
}
