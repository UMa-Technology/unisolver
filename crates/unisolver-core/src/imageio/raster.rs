//! Regular rasters (PNG/JPEG/TIFF, including 16-bit), decoded by the image crate.
//! Rasters carry no astronomical header; their EXIF, when present, gives the focal lengths,
//! pixel pitch, exposure and capture time (see `exif`).
use super::{exif, ImageMeta, SourceFormat};
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
    let mut meta = ImageMeta::bare(w, h, source, depth);
    let block = match source {
        SourceFormat::Jpeg => exif::jpeg_tiff(bytes),
        SourceFormat::Png => exif::png_tiff(bytes),
        _ => Some(bytes),
    };
    if let Some(e) = block.and_then(exif::parse) {
        meta.apply_exif(&e);
    }
    Ok((
        Frame {
            width: w,
            height: h,
            row_stride_bytes: None,
            pixels,
        },
        meta,
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

    fn gray_jpeg(w: u32, h: u32) -> Vec<u8> {
        let mut jb: Vec<u8> = Vec::new();
        JpegEncoder::new_with_quality(&mut jb, 90)
            .write_image(
                &vec![40u8; (w * h) as usize],
                w,
                h,
                image::ExtendedColorType::L8,
            )
            .unwrap();
        jb
    }

    #[test]
    fn jpeg_exif_gives_hints_and_time() {
        use unisolver_synth::exif::{jpeg_with_exif, ExifFields};
        let exif = ExifFields {
            make: Some("Apple".into()),
            model: Some("iPhone 15 Pro".into()),
            focal_mm: Some(6.86),
            focal_35mm: Some(24),
            exposure: Some((1, 1)),
            date_time_original: Some("2026:03:19 08:03:09".into()),
            offset_time_original: Some("+08:00".into()),
            ..Default::default()
        };
        let (_, meta) = load_image_bytes(&jpeg_with_exif(&gray_jpeg(64, 48), &exif)).unwrap();
        assert_eq!(meta.focal_35mm_mm, Some(24.0));
        assert_eq!(meta.instrument.as_deref(), Some("Apple iPhone 15 Pro"));
        assert_eq!(meta.date_obs.as_deref(), Some("2026-03-19T08:03:09+08:00"));
        assert_eq!(meta.observation_unix_ms, Some(1_773_878_589_000 + 500));
        // 24 mm on 4:3: diagonal 2·atan(21.633/24) = 84.1°, horizontal 71.6° (main camera, landscape)
        let hints = meta.solve_hints();
        assert_eq!(hints.len(), 1, "no pixel pitch, so only the 35 mm hint");
        assert!(
            (hints[0].fov_deg - 71.6).abs() < 0.1,
            "{}",
            hints[0].fov_deg
        );
        // Resized after capture: FocalPlaneXResolution describes the recorded width, so the
        // pitch per stored pixel scales up and the FOV stays the sensor's
        let dslr = ExifFields {
            make: Some("Canon".into()),
            model: Some("Canon EOS 90D".into()),
            focal_mm: Some(50.0),
            // 6960 px over 22.3 mm, in pixels per cm; the JPEG below is 64 px wide
            focal_plane_x_res: Some((6960 * 1000, 2230)),
            focal_plane_unit: Some(3),
            pixel_x_dimension: Some(6960),
            ..Default::default()
        };
        let (_, meta) = load_image_bytes(&jpeg_with_exif(&gray_jpeg(64, 43), &dslr)).unwrap();
        assert_eq!(meta.instrument.as_deref(), Some("Canon EOS 90D"));
        let fov = meta.fov_hint_deg().unwrap();
        let expected = 2.0 * (22.3f64 / 2.0 / 50.0).atan().to_degrees();
        assert!((fov - expected).abs() < 0.05, "{fov} vs {expected}");
        assert_eq!(meta.observation_unix_ms, None, "no capture time");
    }

    #[test]
    fn heic_points_to_the_platform_decoder() {
        let mut heic = vec![0, 0, 0, 24];
        heic.extend(b"ftypheic");
        heic.extend([0u8; 16]);
        let err = load_image_bytes(&heic).unwrap_err().to_string();
        assert!(err.contains("HEIC") && err.contains("frame entry"), "{err}");
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
