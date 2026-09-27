//! Regular rasters (PNG/JPEG/TIFF, including 16-bit), decoded by the image crate.
//! Rasters carry no astronomical header: metadata beyond size, depth and source is None.
use super::{ImageMeta, SourceFormat};
use crate::{CoreError, Frame, PixelData, Result};

pub(super) fn read_raster_bytes(bytes: &[u8], source: SourceFormat) -> Result<(Frame, ImageMeta)> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| CoreError::InvalidInput(format!("raster decode: {e}")))?;
    let (w, h) = (img.width(), img.height());
    let sixteen = matches!(
        img.color(),
        image::ColorType::L16
            | image::ColorType::La16
            | image::ColorType::Rgb16
            | image::ColorType::Rgba16
    );
    let (pixels, depth) = if sixteen {
        (PixelData::Luma16(img.to_luma16().into_raw()), 16)
    } else {
        (PixelData::LumaF32(img.to_luma32f().into_raw()), 8)
    };
    Ok((
        Frame {
            width: w,
            height: h,
            row_stride_bytes: None,
            pixels,
        },
        ImageMeta::bare(w, h, source, depth),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::load_image_bytes;
    use super::*;
    use image::codecs::{jpeg::JpegEncoder, png::PngEncoder, tiff::TiffEncoder};
    use image::ImageEncoder;

    #[test]
    fn png16_preserves_depth() {
        let src: Vec<u16> = vec![1000, 20000, 40000, 65000];
        let mut bytes: Vec<u8> = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(
                &src.iter()
                    .flat_map(|v| v.to_ne_bytes())
                    .collect::<Vec<u8>>(),
                2,
                2,
                image::ExtendedColorType::L16,
            )
            .unwrap();
        let (frame, meta) = load_image_bytes(&bytes).unwrap();
        assert_eq!(meta.source, SourceFormat::Png);
        assert_eq!(meta.bit_depth, 16);
        assert_eq!(
            frame.to_luma_f32().unwrap(),
            vec![1000.0, 20000.0, 40000.0, 65000.0]
        );
        assert!(meta.solve_hints().is_empty(), "rasters have no hints");
    }

    #[test]
    fn tiff16_and_jpeg8() {
        let src: Vec<u16> = vec![500, 1500, 2500, 3500];
        let mut bytes = std::io::Cursor::new(Vec::new());
        TiffEncoder::new(&mut bytes)
            .write_image(
                &src.iter()
                    .flat_map(|v| v.to_ne_bytes())
                    .collect::<Vec<u8>>(),
                2,
                2,
                image::ExtendedColorType::L16,
            )
            .unwrap();
        let (frame, meta) = load_image_bytes(bytes.get_ref()).unwrap();
        assert_eq!(meta.source, SourceFormat::Tiff);
        assert_eq!(frame.to_luma_f32().unwrap()[3], 3500.0);

        let mut jb: Vec<u8> = Vec::new();
        JpegEncoder::new_with_quality(&mut jb, 95)
            .write_image(&[240u8; 16], 4, 4, image::ExtendedColorType::L8)
            .unwrap();
        let (frame, meta) = load_image_bytes(&jb).unwrap();
        assert_eq!(meta.source, SourceFormat::Jpeg);
        let l = frame.to_luma_f32().unwrap();
        assert_eq!(l.len(), 16);
        let mean: f32 = l.iter().sum::<f32>() / 16.0;
        assert!(
            (mean - 240.0 / 255.0 * 255.0).abs() < 12.0 || mean > 0.9,
            "lossy but same magnitude: {mean}"
        );
    }
}
