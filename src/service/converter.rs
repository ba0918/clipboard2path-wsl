use std::fmt;
use std::path::{Path, PathBuf};

use crate::domain::image_convert::{self, ConvertError};
use crate::domain::path_gen::{self, PathError};
use crate::infra::clipboard::{ClipboardError, ClipboardReader, ClipboardWriter};
use crate::infra::file_system::{FileWriter, FsError};

/// Unified application error type.
#[derive(Debug)]
pub enum AppError {
    Clipboard(ClipboardError),
    Convert(ConvertError),
    Path(PathError),
    Fs(FsError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Clipboard(e) => write!(f, "clipboard error: {e}"),
            AppError::Convert(e) => write!(f, "conversion error: {e}"),
            AppError::Path(e) => write!(f, "path error: {e}"),
            AppError::Fs(e) => write!(f, "file system error: {e}"),
        }
    }
}

impl From<ClipboardError> for AppError {
    fn from(e: ClipboardError) -> Self {
        AppError::Clipboard(e)
    }
}

impl From<ConvertError> for AppError {
    fn from(e: ConvertError) -> Self {
        AppError::Convert(e)
    }
}

impl From<PathError> for AppError {
    fn from(e: PathError) -> Self {
        AppError::Path(e)
    }
}

impl From<FsError> for AppError {
    fn from(e: FsError) -> Self {
        AppError::Fs(e)
    }
}

/// Timestamp provider trait for dependency injection.
pub trait TimestampProvider {
    fn now(&self) -> String;
}

/// Real timestamp provider using system time.
pub struct SystemTimestamp;

impl TimestampProvider for SystemTimestamp {
    fn now(&self) -> String {
        use std::time::SystemTime;
        let duration = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        format!("{}", duration.as_secs())
    }
}

/// Orchestrates the clipboard-to-file conversion workflow.
///
/// Contains zero business logic — delegates to domain functions and infra traits.
pub struct ConvertService<C, W, F, T>
where
    C: ClipboardReader,
    W: ClipboardWriter,
    F: FileWriter,
    T: TimestampProvider,
{
    clipboard_reader: C,
    clipboard_writer: W,
    file_writer: F,
    timestamp: T,
}

impl<C, W, F, T> ConvertService<C, W, F, T>
where
    C: ClipboardReader,
    W: ClipboardWriter,
    F: FileWriter,
    T: TimestampProvider,
{
    pub fn new(clipboard_reader: C, clipboard_writer: W, file_writer: F, timestamp: T) -> Self {
        Self {
            clipboard_reader,
            clipboard_writer,
            file_writer,
            timestamp,
        }
    }

    /// Execute a single conversion: read BMP from clipboard, convert to PNG,
    /// save to file, write path back to clipboard.
    pub fn convert_once(&self, base_dir: &Path) -> Result<PathBuf, AppError> {
        // 1. Read BMP from clipboard
        let bmp_data = self.clipboard_reader.read_image_bmp()?;

        // 2. Convert BMP -> PNG (domain logic)
        let png_data = image_convert::convert_bmp_to_png(&bmp_data)?;

        // 3. Generate save path (domain logic)
        let timestamp = self.timestamp.now();
        let save_path = path_gen::generate_save_path(base_dir, &timestamp)?;

        // 4. Write PNG to file (infra)
        self.file_writer.write_bytes(&save_path, &png_data)?;

        // 5. Write path to clipboard (infra)
        let path_str = save_path.to_string_lossy();
        self.clipboard_writer.write_text(&path_str)?;

        Ok(save_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::Cursor;
    use std::path::PathBuf;

    // --- Mock implementations ---

    struct MockClipboardReader {
        data: Vec<u8>,
    }

    impl ClipboardReader for MockClipboardReader {
        fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError> {
            Ok(self.data.clone())
        }
    }

    struct FailingClipboardReader;

    impl ClipboardReader for FailingClipboardReader {
        fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError> {
            Err(ClipboardError::CommandFailed {
                tool: "wl-paste".to_string(),
                stderr: "no image in clipboard".to_string(),
            })
        }
    }

    struct MockClipboardWriter {
        written: RefCell<Option<String>>,
    }

    impl MockClipboardWriter {
        fn new() -> Self {
            Self {
                written: RefCell::new(None),
            }
        }
    }

    impl ClipboardWriter for MockClipboardWriter {
        fn write_text(&self, text: &str) -> Result<(), ClipboardError> {
            *self.written.borrow_mut() = Some(text.to_string());
            Ok(())
        }
    }

    struct MockFileWriter {
        written: RefCell<Vec<(PathBuf, Vec<u8>)>>,
    }

    impl MockFileWriter {
        fn new() -> Self {
            Self {
                written: RefCell::new(Vec::new()),
            }
        }
    }

    impl FileWriter for MockFileWriter {
        fn write_bytes(&self, path: &Path, data: &[u8]) -> Result<(), FsError> {
            self.written
                .borrow_mut()
                .push((path.to_path_buf(), data.to_vec()));
            Ok(())
        }
    }

    struct FixedTimestamp(String);

    impl TimestampProvider for FixedTimestamp {
        fn now(&self) -> String {
            self.0.clone()
        }
    }

    // --- Helper ---

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
    fn convert_once_full_flow() {
        let bmp = make_1x1_bmp();
        let reader = MockClipboardReader { data: bmp };
        let writer = MockClipboardWriter::new();
        let file_writer = MockFileWriter::new();
        let timestamp = FixedTimestamp("20260406-120000".to_string());

        let service = ConvertService::new(reader, writer, file_writer, timestamp);
        let result = service.convert_once(Path::new("/tmp"));

        assert!(result.is_ok());
        let path = result.unwrap();
        assert_eq!(path, PathBuf::from("/tmp/clipboard-20260406-120000.png"));
    }

    #[test]
    fn convert_once_writes_png_to_file() {
        let bmp = make_1x1_bmp();
        let reader = MockClipboardReader { data: bmp };
        let writer = MockClipboardWriter::new();
        let file_writer = MockFileWriter::new();
        let timestamp = FixedTimestamp("12345".to_string());

        let service = ConvertService::new(reader, writer, file_writer, timestamp);
        service.convert_once(Path::new("/tmp")).unwrap();

        let svc_ref = &service;
        let writes = svc_ref.file_writer.written.borrow();
        assert_eq!(writes.len(), 1);
        // Verify it wrote PNG data (magic bytes)
        assert_eq!(&writes[0].1[..4], &[0x89, b'P', b'N', b'G']);
    }

    #[test]
    fn convert_once_writes_path_to_clipboard() {
        let bmp = make_1x1_bmp();
        let reader = MockClipboardReader { data: bmp };
        let writer = MockClipboardWriter::new();
        let file_writer = MockFileWriter::new();
        let timestamp = FixedTimestamp("99999".to_string());

        let service = ConvertService::new(reader, writer, file_writer, timestamp);
        service.convert_once(Path::new("/tmp")).unwrap();

        let clipboard_text = service.clipboard_writer.written.borrow().clone();
        assert_eq!(clipboard_text, Some("/tmp/clipboard-99999.png".to_string()));
    }

    #[test]
    fn convert_once_clipboard_error_propagates() {
        let reader = FailingClipboardReader;
        let writer = MockClipboardWriter::new();
        let file_writer = MockFileWriter::new();
        let timestamp = FixedTimestamp("0".to_string());

        let service = ConvertService::new(reader, writer, file_writer, timestamp);
        let result = service.convert_once(Path::new("/tmp"));

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Clipboard(_)));
    }

    #[test]
    fn convert_once_invalid_bmp_error_propagates() {
        let reader = MockClipboardReader {
            data: vec![0xFF, 0xFE],
        };
        let writer = MockClipboardWriter::new();
        let file_writer = MockFileWriter::new();
        let timestamp = FixedTimestamp("0".to_string());

        let service = ConvertService::new(reader, writer, file_writer, timestamp);
        let result = service.convert_once(Path::new("/tmp"));

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::Convert(_)));
    }
}
