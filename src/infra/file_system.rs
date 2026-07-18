//! File system write abstraction.

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

/// Validate that an output directory exists and is safe.
///
/// Performs I/O (canonicalize, is_dir check), so it belongs in the infra layer.
/// Delegates traversal detection to the domain layer's pure function.
pub fn validate_output_dir(
    path: &Path,
) -> Result<std::path::PathBuf, crate::domain::path_gen::PathError> {
    use crate::domain::path_gen::{PathError, validate_path_components};

    validate_path_components(path)?;

    let canonical = path
        .canonicalize()
        .map_err(|e| PathError::CanonicalizeFailed(e.to_string()))?;

    if !canonical.is_dir() {
        return Err(PathError::DirNotFound(canonical.display().to_string()));
    }

    Ok(canonical)
}

/// Atomically install a file with the given mode.
///
/// Creates the parent directory, writes the content to a temporary file in the
/// *same* directory, sets the mode, then atomically renames it onto `path`.
///
/// The same-directory temp file guarantees the rename stays within one filesystem
/// (cross-filesystem rename would fail) and closes the TOCTOU window a two-step
/// "write then chmod on the target" would open. An existing target — including a
/// symlink — is replaced by the rename itself, so no write ever follows a symlink.
///
/// This is the non-DI write primitive shared by the installers, distinct from
/// [`RealFileWriter`] (which writes clipboard images at `0o600`).
pub fn install_file(path: &Path, content: &[u8], mode: u32) -> Result<(), FsError> {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|e| FsError::IoError(format!("failed to create directory: {e}")))?;

    let file_name = path
        .file_name()
        .ok_or_else(|| FsError::IoError(format!("invalid install path: {}", path.display())))?;
    let temp_name = format!(
        ".{}.c2p-tmp.{}",
        file_name.to_string_lossy(),
        std::process::id()
    );
    let temp = parent.join(temp_name);

    let write_result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .open(&temp)
            .map_err(|e| FsError::IoError(e.to_string()))?;
        file.write_all(content)
            .map_err(|e| FsError::IoError(e.to_string()))?;
        // Re-assert the mode: OpenOptions::mode is masked by umask on creation.
        fs::set_permissions(&temp, fs::Permissions::from_mode(mode))
            .map_err(|e| FsError::IoError(e.to_string()))?;
        fs::rename(&temp, path).map_err(|e| FsError::IoError(e.to_string()))
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write_result
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

        let mut file = options
            .open(path)
            .map_err(|e| FsError::IoError(e.to_string()))?;

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
    fn install_file_creates_parent_dir_and_writes() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_install_file_mkdir");
        let _ = std::fs::remove_dir_all(&tmp);
        let target = tmp.join("nested/dir/file.txt");

        install_file(&target, b"hello", 0o644).expect("install_file should succeed");

        assert_eq!(std::fs::read(&target).unwrap(), b"hello");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_file_sets_requested_mode() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_install_file_mode");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let f644 = tmp.join("a644");
        install_file(&f644, b"x", 0o644).unwrap();
        assert_eq!(
            std::fs::metadata(&f644).unwrap().permissions().mode() & 0o777,
            0o644
        );

        let f755 = tmp.join("b755");
        install_file(&f755, b"x", 0o755).unwrap();
        assert_eq!(
            std::fs::metadata(&f755).unwrap().permissions().mode() & 0o777,
            0o755
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_file_replaces_existing_file_content_and_mode() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_install_file_replace");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let target = tmp.join("file");

        install_file(&target, b"v1", 0o644).unwrap();
        install_file(&target, b"v2-longer", 0o755).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"v2-longer");
        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o755
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_file_replaces_symlink_without_writing_through() {
        use std::os::unix::fs::symlink;

        let tmp = std::env::temp_dir().join("clipboard2path_test_install_file_symlink");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let link_target = tmp.join("real_target");
        std::fs::write(&link_target, b"original").unwrap();
        let link = tmp.join("link");
        symlink(&link_target, &link).unwrap();

        install_file(&link, b"new content", 0o644).unwrap();

        // The link path is now a regular file (the symlink was replaced, not followed).
        let meta = std::fs::symlink_metadata(&link).unwrap();
        assert!(
            meta.file_type().is_file(),
            "symlink should be replaced by a regular file"
        );
        assert_eq!(std::fs::read(&link).unwrap(), b"new content");
        // The original link target is untouched.
        assert_eq!(std::fs::read(&link_target).unwrap(), b"original");

        let _ = std::fs::remove_dir_all(&tmp);
    }

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
    fn validate_output_dir_accepts_existing_dir() {
        let result = validate_output_dir(Path::new("/tmp"));
        assert!(result.is_ok());
    }

    #[test]
    fn validate_output_dir_rejects_traversal() {
        use crate::domain::path_gen::PathError;
        let result = validate_output_dir(Path::new("/tmp/../etc"));
        assert_eq!(result, Err(PathError::TraversalDetected));
    }

    #[test]
    fn validate_output_dir_rejects_nonexistent() {
        use crate::domain::path_gen::PathError;
        let result = validate_output_dir(Path::new("/nonexistent_dir_12345"));
        assert!(matches!(result, Err(PathError::CanonicalizeFailed(_))));
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
