use std::io::Cursor;

use image::{codecs::jpeg::JpegEncoder, imageops::FilterType, ImageReader, Limits};

use crate::cover::error::CoverArtError;

const MAX_DIMENSION: u32 = 512;
const JPEG_QUALITY: u8 = 85;

pub fn normalize_upload_bytes(bytes: &[u8]) -> Result<Vec<u8>, CoverArtError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| CoverArtError::other(format!("image format detection failed: {e}")))?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);

    let decoded = reader
        .decode()
        .map_err(|e| CoverArtError::other(format!("image decode failed: {e}")))?;
    let normalized = if decoded.width() > MAX_DIMENSION || decoded.height() > MAX_DIMENSION {
        decoded.resize(MAX_DIMENSION, MAX_DIMENSION, FilterType::Lanczos3)
    } else {
        decoded
    }
    .into_rgb8();
    let mut output = Vec::with_capacity(64 * 1024);
    JpegEncoder::new_with_quality(&mut output, JPEG_QUALITY)
        .encode_image(&normalized)
        .map_err(|e| CoverArtError::other(format!("image re-encode failed: {e}")))?;

    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{codecs::jpeg::JpegEncoder, DynamicImage, Rgb, RgbImage};

    use super::normalize_upload_bytes;

    #[test]
    fn repairs_jpeg_without_end_marker() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(32, 32, Rgb([24, 48, 72])));
        let mut truncated = Vec::new();
        JpegEncoder::new_with_quality(&mut truncated, 90)
            .encode_image(&image)
            .unwrap();
        assert_eq!(truncated.split_off(truncated.len() - 2), [0xff, 0xd9]);

        let normalized = normalize_upload_bytes(&truncated).unwrap();
        let decoded = image::load_from_memory(&normalized).unwrap();

        assert_eq!((decoded.width(), decoded.height()), (32, 32));
        assert_eq!(&normalized[normalized.len() - 2..], &[0xff, 0xd9]);
    }

    #[test]
    fn bounds_large_images() {
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(1024, 768, Rgb([24, 48, 72])));
        let mut source = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut source), image::ImageFormat::Png)
            .unwrap();

        let normalized = normalize_upload_bytes(&source).unwrap();
        let decoded = image::load_from_memory(&normalized).unwrap();

        assert!(decoded.width() <= 512);
        assert!(decoded.height() <= 512);
    }
}
