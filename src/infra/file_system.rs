use std::fmt;
use std::path::Path;

/// Error type for file system operations.
#[derive(Debug)]
pub enum FsError {
    /// I/O error during file write.
    IoError(String),
}

impl fmt::Display for FsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FsError::IoError(msg) => write!(f, "I/O error: {msg}"),
        }
    }
}

/// Write bytes to a file.
pub trait FileWriter {
    fn write_bytes(&self, path: &Path, data: &[u8]) -> Result<(), FsError>;
}

/// Real file system implementation.
///
/// Saves files with `0o600` permissions (owner read/write only).
pub struct RealFileWriter;

impl FileWriter for RealFileWriter {
    fn write_bytes(&self, path: &Path, data: &[u8]) -> Result<(), FsError> {
        use std::fs;
        use std::os::unix::fs::OpenOptionsExt;

        // Write with restrictive permissions: owner read/write only (0o600)
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);

        let mut file = options.open(path).map_err(|e| FsError::IoError(e.to_string()))?;

        use std::io::Write;
        file.write_all(data)
            .map_err(|e| FsError::IoError(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn writes_file_with_correct_permissions() {
        let dir = std::env::temp_dir();
        let path = dir.join("clipboard2path_test_write.tmp");

        let writer = RealFileWriter;
        writer
            .write_bytes(&path, b"test data")
            .expect("write should succeed");

        // Verify content
        let content = std::fs::read(&path).expect("should read file");
        assert_eq!(content, b"test data");

        // Verify permissions (0o600)
        let metadata = std::fs::metadata(&path).expect("should get metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "file should have 0o600 permissions");

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_to_nonexistent_dir_fails() {
        let path = Path::new("/nonexistent_dir_12345/test.tmp");
        let writer = RealFileWriter;
        let result = writer.write_bytes(path, b"data");
        assert!(result.is_err());
    }

    // Mock implementation for use in service layer tests
    pub struct MockFileWriter {
        pub written: std::cell::RefCell<Vec<(std::path::PathBuf, Vec<u8>)>>,
    }

    impl MockFileWriter {
        pub fn new() -> Self {
            Self {
                written: std::cell::RefCell::new(Vec::new()),
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

    #[test]
    fn mock_writer_records_writes() {
        let writer = MockFileWriter::new();
        writer
            .write_bytes(Path::new("/tmp/test.png"), b"png data")
            .unwrap();
        let writes = writer.written.borrow();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, Path::new("/tmp/test.png"));
        assert_eq!(writes[0].1, b"png data");
    }
}
