//! Shared test mocks and helpers for service-layer tests.

use crate::infra::clipboard::{ClipboardError, ClipboardReader};
use crate::infra::file_system::{FileWriter, FsError};
use crate::infra::path_notifier::{NotifyError, PathNotifier};
use crate::service::converter::TimestampProvider;
use std::io::Cursor;
use std::path::Path;

pub struct MockClipboardReader {
    pub types: Vec<String>,
    pub bmp_data: Vec<u8>,
}

impl ClipboardReader for MockClipboardReader {
    fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError> {
        Ok(self.bmp_data.clone())
    }

    fn list_types(&self) -> Result<Vec<String>, ClipboardError> {
        Ok(self.types.clone())
    }
}

pub struct MockFileWriter;

impl FileWriter for MockFileWriter {
    fn write_bytes(&self, _path: &Path, _data: &[u8]) -> Result<(), FsError> {
        Ok(())
    }
}

pub struct MockPathNotifier;

impl PathNotifier for MockPathNotifier {
    fn notify(&self, _path: &Path) -> Result<(), NotifyError> {
        Ok(())
    }

    fn clear(&self) -> Result<(), NotifyError> {
        Ok(())
    }
}

pub struct FixedTimestamp(pub String);

impl TimestampProvider for FixedTimestamp {
    fn now(&self) -> String {
        self.0.clone()
    }
}

/// Create a minimal 1x1 BMP image for testing.
pub fn make_1x1_bmp() -> Vec<u8> {
    use image::{ImageBuffer, Rgb};
    let img = ImageBuffer::from_pixel(1, 1, Rgb([255u8, 0, 0]));
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Bmp)
        .unwrap();
    buf
}
