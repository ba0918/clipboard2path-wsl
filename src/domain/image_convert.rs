//! BMP to PNG image conversion (pure functions).

use std::fmt;
use std::io::Cursor;

/// Error type for image conversion.
#[derive(Debug, PartialEq)]
pub enum ConvertError {
    /// Input data is empty.
    EmptyInput,
    /// Input is not valid BMP data.
    InvalidBmp(String),
    /// PNG encoding failed.
    PngEncodeFailed(String),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvertError::EmptyInput => write!(f, "input data is empty"),
            ConvertError::InvalidBmp(msg) => write!(f, "invalid BMP: {msg}"),
            ConvertError::PngEncodeFailed(msg) => write!(f, "PNG encode failed: {msg}"),
        }
    }
}

/// Convert BMP bytes to PNG bytes.
///
/// Pure function: takes raw BMP data, returns PNG-encoded bytes.
pub fn convert_bmp_to_png(bmp_bytes: &[u8]) -> Result<Vec<u8>, ConvertError> {
    if bmp_bytes.is_empty() {
        return Err(ConvertError::EmptyInput);
    }

    let img = image::load_from_memory_with_format(bmp_bytes, image::ImageFormat::Bmp)
        .map_err(|e| ConvertError::InvalidBmp(e.to_string()))?;

    let mut png_buf = Vec::with_capacity(bmp_bytes.len());
    img.write_to(&mut Cursor::new(&mut png_buf), image::ImageFormat::Png)
        .map_err(|e| ConvertError::PngEncodeFailed(e.to_string()))?;

    Ok(png_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a minimal valid BMP (1x1 pixel, 24-bit).
    fn make_1x1_bmp() -> Vec<u8> {
        // Use the image crate to generate a valid BMP in-memory.
        use image::{ImageBuffer, Rgb};

        let img = ImageBuffer::from_pixel(1, 1, Rgb([255u8, 0, 0]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Bmp)
            .expect("failed to create test BMP");
        buf
    }

    #[test]
    fn converts_valid_bmp_to_png() {
        let bmp = make_1x1_bmp();
        let result = convert_bmp_to_png(&bmp);
        assert!(result.is_ok());
        let png_bytes = result.unwrap();
        // PNG magic bytes
        assert_eq!(&png_bytes[..4], &[0x89, b'P', b'N', b'G']);
    }

    #[test]
    fn rejects_empty_input() {
        let result = convert_bmp_to_png(&[]);
        assert_eq!(result, Err(ConvertError::EmptyInput));
    }

    #[test]
    fn rejects_invalid_bmp() {
        let result = convert_bmp_to_png(&[0xFF, 0xFE, 0x00, 0x01]);
        assert!(matches!(result, Err(ConvertError::InvalidBmp(_))));
    }

    #[test]
    fn rejects_random_data() {
        let garbage: Vec<u8> = (0..100).map(|i| (i * 37 % 256) as u8).collect();
        let result = convert_bmp_to_png(&garbage);
        assert!(result.is_err());
    }
}
