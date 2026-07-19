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

/// Reader whose type listing always fails (e.g. wl-paste unavailable).
pub struct FailingListClipboardReader;

impl ClipboardReader for FailingListClipboardReader {
    fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError> {
        Err(ClipboardError::CommandFailed {
            tool: "mock".to_string(),
            stderr: "read failed".to_string(),
        })
    }

    fn list_types(&self) -> Result<Vec<String>, ClipboardError> {
        Err(ClipboardError::CommandFailed {
            tool: "mock".to_string(),
            stderr: "list failed".to_string(),
        })
    }
}

/// Reader backed by shared mutable state, so a test can change the
/// "clipboard content" while a watch loop is running (via callbacks).
pub struct SharedClipboardReader {
    pub types: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    pub bmp_data: Vec<u8>,
}

impl ClipboardReader for SharedClipboardReader {
    fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError> {
        Ok(self.bmp_data.clone())
    }

    fn list_types(&self) -> Result<Vec<String>, ClipboardError> {
        Ok(self.types.borrow().clone())
    }
}

/// Notifier that records notify/clear calls for assertions.
pub struct RecordingPathNotifier {
    pub events: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}

impl PathNotifier for RecordingPathNotifier {
    fn notify(&self, path: &Path) -> Result<(), NotifyError> {
        self.events
            .borrow_mut()
            .push(format!("notify:{}", path.display()));
        Ok(())
    }

    fn clear(&self) -> Result<(), NotifyError> {
        self.events.borrow_mut().push("clear".to_string());
        Ok(())
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
