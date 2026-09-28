//! Display previews for images Flutter cannot show (FITS, XISF, 16-bit TIFF): read by the
//! engine, box-averaged down and auto-stretched to 8 bits.
use anyhow::Result;
use unisolver_core as core;

/// An auto-stretched greyscale preview as RGBA pixels (feed `rgba` to
/// `ui.decodeImageFromPixels` with `PixelFormat.rgba8888`). Drawn over the source's size
/// (`sourceWidth` × `sourceHeight`), it lines up with the source's pixel coordinates, so
/// annotations land where they belong.
pub struct ImagePreviewDto {
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    /// `width × height × 4` bytes
    pub rgba: Vec<u8>,
}

/// Preview of an image file (FITS / XISF / PNG / JPEG / TIFF) no larger than `max_side` on
/// either side. Astronomical frames hold linear data that shows black as-is; the stretch
/// (median and MAD based, as PixInsight's AutoSTF) lifts the sky to a quarter of full scale.
/// About 40 ms for a 26 Mpx frame.
pub fn image_preview(path: String, max_side: u32) -> Result<ImagePreviewDto> {
    let p = core::imageio::load_preview(&path, max_side)?;
    Ok(ImagePreviewDto {
        width: p.width,
        height: p.height,
        source_width: p.source_width,
        source_height: p.source_height,
        rgba: p.rgba(),
    })
}
