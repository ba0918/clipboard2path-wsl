use std::path::Path;

use crate::domain::{clipboard_change, debounce};
use crate::infra::clipboard::{ClipboardReader, ClipboardWriter};
use crate::infra::file_system::FileWriter;
use crate::service::converter::{ConvertService, TimestampProvider};

/// Result of a single poll iteration.
#[derive(Debug, PartialEq)]
pub enum PollResult {
    /// Clipboard changed and conversion succeeded.
    Converted(std::path::PathBuf),
    /// Clipboard changed but no BMP image present — skipped.
    NoBmpImage,
    /// Clipboard unchanged — no action taken.
    NoChange,
    /// Within debounce window — skipped.
    Debounced,
    /// Conversion failed.
    ConvertError(String),
    /// Clipboard read error.
    ClipboardError(String),
}

/// Execute a single poll iteration.
///
/// This is the core logic extracted as a testable function.
/// The daemon loop calls this repeatedly.
pub fn poll_once<C, W, F, T>(
    service: &ConvertService<C, W, F, T>,
    previous_types: &[String],
    last_write_ms: Option<u64>,
    current_ms: u64,
    debounce_ms: u64,
    base_dir: &Path,
) -> (PollResult, Vec<String>)
where
    C: ClipboardReader,
    W: ClipboardWriter,
    F: FileWriter,
    T: TimestampProvider,
{
    // 1. Check debounce
    if !debounce::should_process_event(last_write_ms, current_ms, debounce_ms) {
        return (PollResult::Debounced, previous_types.to_vec());
    }

    // 2. List current types
    let current_types = match service.reader().list_types() {
        Ok(types) => types,
        Err(e) => {
            return (
                PollResult::ClipboardError(e.to_string()),
                previous_types.to_vec(),
            );
        }
    };

    // 3. Check for change
    if !clipboard_change::has_clipboard_changed(previous_types, &current_types) {
        return (PollResult::NoChange, current_types);
    }

    // 4. Check for BMP
    if !clipboard_change::has_bmp_image(&current_types) {
        return (PollResult::NoBmpImage, current_types);
    }

    // 5. Convert
    match service.convert_once(base_dir) {
        Ok(path) => (PollResult::Converted(path), current_types),
        Err(e) => (PollResult::ConvertError(e.to_string()), current_types),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::clipboard::{ClipboardError as CErr, ClipboardReader, ClipboardWriter};
    use crate::infra::file_system::{FileWriter, FsError};
    use crate::service::converter::ConvertService;
    use std::cell::RefCell;
    use std::io::Cursor;
    use std::path::Path;

    // --- Mocks ---

    struct MockReader {
        types: Vec<String>,
        bmp_data: Vec<u8>,
    }

    impl ClipboardReader for MockReader {
        fn read_image_bmp(&self) -> Result<Vec<u8>, CErr> {
            Ok(self.bmp_data.clone())
        }

        fn list_types(&self) -> Result<Vec<String>, CErr> {
            Ok(self.types.clone())
        }
    }

    struct MockWriter {
        written: RefCell<Option<String>>,
    }

    impl MockWriter {
        fn new() -> Self {
            Self {
                written: RefCell::new(None),
            }
        }
    }

    impl ClipboardWriter for MockWriter {
        fn write_text(&self, text: &str) -> Result<(), CErr> {
            *self.written.borrow_mut() = Some(text.to_string());
            Ok(())
        }
    }

    struct MockFileWriter;

    impl FileWriter for MockFileWriter {
        fn write_bytes(&self, _path: &Path, _data: &[u8]) -> Result<(), FsError> {
            Ok(())
        }
    }

    struct FixedTimestamp(String);
    impl TimestampProvider for FixedTimestamp {
        fn now(&self) -> String {
            self.0.clone()
        }
    }

    fn make_1x1_bmp() -> Vec<u8> {
        use image::{ImageBuffer, Rgb};
        let img = ImageBuffer::from_pixel(1, 1, Rgb([255u8, 0, 0]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Bmp)
            .unwrap();
        buf
    }

    // --- Tests ---

    #[test]
    fn poll_once_converts_when_bmp_present_and_changed() {
        let bmp = make_1x1_bmp();
        let service = ConvertService::new(
            MockReader {
                types: vec!["image/bmp".to_string()],
                bmp_data: bmp,
            },
            MockWriter::new(),
            MockFileWriter,
            FixedTimestamp("1".into()),
        );

        let (result, _) = poll_once(
            &service,
            &[],  // previous: empty (change detected)
            None, // no last write
            1000, // current ms
            500,  // debounce
            Path::new("/tmp"),
        );

        assert!(matches!(result, PollResult::Converted(_)));
    }

    #[test]
    fn poll_once_skips_when_no_change() {
        let service = ConvertService::new(
            MockReader {
                types: vec!["image/bmp".to_string()],
                bmp_data: vec![],
            },
            MockWriter::new(),
            MockFileWriter,
            FixedTimestamp("1".into()),
        );

        let prev = vec!["image/bmp".to_string()];
        let (result, _) = poll_once(
            &service,
            &prev, // same as current
            None,
            1000,
            500,
            Path::new("/tmp"),
        );

        assert_eq!(result, PollResult::NoChange);
    }

    #[test]
    fn poll_once_skips_when_debounced() {
        let service = ConvertService::new(
            MockReader {
                types: vec!["image/bmp".to_string()],
                bmp_data: vec![],
            },
            MockWriter::new(),
            MockFileWriter,
            FixedTimestamp("1".into()),
        );

        let (result, _) = poll_once(
            &service,
            &[],
            Some(900), // wrote at 900ms
            1000,      // current 1000ms — only 100ms elapsed
            500,       // debounce 500ms
            Path::new("/tmp"),
        );

        assert_eq!(result, PollResult::Debounced);
    }

    #[test]
    fn poll_once_skips_when_no_bmp() {
        let service = ConvertService::new(
            MockReader {
                types: vec!["text/plain".to_string()],
                bmp_data: vec![],
            },
            MockWriter::new(),
            MockFileWriter,
            FixedTimestamp("1".into()),
        );

        let (result, _) = poll_once(
            &service,
            &[], // empty previous -> change detected
            None,
            1000,
            500,
            Path::new("/tmp"),
        );

        assert_eq!(result, PollResult::NoBmpImage);
    }
}
