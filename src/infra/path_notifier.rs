//! Path notification via file + symlink (no clipboard write).

use std::fmt;
use std::path::{Path, PathBuf};

/// Error type for path notification.
#[derive(Debug)]
pub enum NotifyError {
    /// I/O error during notification.
    IoError(String),
}

impl fmt::Display for NotifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotifyError::IoError(msg) => write!(f, "notify error: {msg}"),
        }
    }
}

/// Notify the saved file path via a side channel (not clipboard).
pub trait PathNotifier {
    fn notify(&self, path: &Path) -> Result<(), NotifyError>;
    /// Clear the latest-path notification (clipboard no longer has an image).
    fn clear(&self) -> Result<(), NotifyError>;
}

/// Writes the path to `latest-path` file and updates `latest.png` symlink.
///
/// Both operations are atomic (write to temp file, then rename).
pub struct FilePathNotifier {
    /// The directory where `latest-path` and `latest.png` live.
    runtime_dir: PathBuf,
}

impl FilePathNotifier {
    pub fn new(runtime_dir: PathBuf) -> Self {
        Self { runtime_dir }
    }
}

impl PathNotifier for FilePathNotifier {
    fn notify(&self, path: &Path) -> Result<(), NotifyError> {
        let latest_path_file = self.runtime_dir.join("latest-path");
        let latest_symlink = self.runtime_dir.join("latest.png");

        // Atomic write to latest-path: write to temp file, then rename
        let tmp_path = self.runtime_dir.join(".latest-path.tmp");
        atomic_write_file(
            &tmp_path,
            &latest_path_file,
            path.to_string_lossy().as_bytes(),
        )?;

        // Atomic symlink update: create temp symlink, then rename
        let tmp_link = self.runtime_dir.join(".latest.png.tmp");
        atomic_symlink(&tmp_link, &latest_symlink, path)?;

        Ok(())
    }

    fn clear(&self) -> Result<(), NotifyError> {
        let latest_path_file = self.runtime_dir.join("latest-path");
        let latest_symlink = self.runtime_dir.join("latest.png");

        if latest_path_file.exists() {
            std::fs::remove_file(&latest_path_file)
                .map_err(|e| NotifyError::IoError(format!("failed to remove latest-path: {e}")))?;
        }
        if latest_symlink.exists() || latest_symlink.symlink_metadata().is_ok() {
            let _ = std::fs::remove_file(&latest_symlink);
        }

        Ok(())
    }
}

/// Atomically write data to a file via temp-file + rename.
fn atomic_write_file(tmp: &Path, target: &Path, data: &[u8]) -> Result<(), NotifyError> {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    // Remove stale temp file if it exists
    let _ = fs::remove_file(tmp);

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(tmp)
        .map_err(|e| NotifyError::IoError(format!("failed to create temp file: {e}")))?;

    file.write_all(data)
        .map_err(|e| NotifyError::IoError(format!("failed to write temp file: {e}")))?;

    fs::rename(tmp, target)
        .map_err(|e| NotifyError::IoError(format!("failed to rename temp file: {e}")))
}

/// Atomically update a symlink via temp-symlink + rename.
fn atomic_symlink(tmp: &Path, target: &Path, link_to: &Path) -> Result<(), NotifyError> {
    use std::fs;
    use std::os::unix::fs as unix_fs;

    // Remove stale temp symlink if it exists
    let _ = fs::remove_file(tmp);

    unix_fs::symlink(link_to, tmp)
        .map_err(|e| NotifyError::IoError(format!("failed to create temp symlink: {e}")))?;

    fs::rename(tmp, target)
        .map_err(|e| NotifyError::IoError(format!("failed to rename temp symlink: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_path_notifier_writes_latest_path_and_symlink() {
        let tmp_dir = std::env::temp_dir().join("clipboard2path_test_notifier");
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let notifier = FilePathNotifier::new(tmp_dir.clone());
        let png_path = tmp_dir.join("clipboard-12345.png");

        // Create a dummy file so symlink target exists
        std::fs::write(&png_path, b"fake png").unwrap();

        notifier.notify(&png_path).expect("notify should succeed");

        // Check latest-path file
        let content = std::fs::read_to_string(tmp_dir.join("latest-path")).unwrap();
        assert_eq!(content, png_path.to_string_lossy().as_ref());

        // Check latest.png symlink
        let link_target = std::fs::read_link(tmp_dir.join("latest.png")).unwrap();
        assert_eq!(link_target, png_path);

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn file_path_notifier_updates_atomically() {
        let tmp_dir = std::env::temp_dir().join("clipboard2path_test_notifier_atomic");
        let _ = std::fs::remove_dir_all(&tmp_dir);
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let notifier = FilePathNotifier::new(tmp_dir.clone());

        let path1 = tmp_dir.join("clipboard-1.png");
        let path2 = tmp_dir.join("clipboard-2.png");
        std::fs::write(&path1, b"png1").unwrap();
        std::fs::write(&path2, b"png2").unwrap();

        notifier.notify(&path1).unwrap();
        notifier.notify(&path2).unwrap();

        let content = std::fs::read_to_string(tmp_dir.join("latest-path")).unwrap();
        assert_eq!(content, path2.to_string_lossy().as_ref());

        let link_target = std::fs::read_link(tmp_dir.join("latest.png")).unwrap();
        assert_eq!(link_target, path2);

        let _ = std::fs::remove_dir_all(&tmp_dir);
    }

    /// Mock for testing service layer without file I/O.
    pub struct MockPathNotifier {
        pub notified: std::cell::RefCell<Option<PathBuf>>,
    }

    impl MockPathNotifier {
        pub fn new() -> Self {
            Self {
                notified: std::cell::RefCell::new(None),
            }
        }
    }

    impl PathNotifier for MockPathNotifier {
        fn notify(&self, path: &Path) -> Result<(), NotifyError> {
            *self.notified.borrow_mut() = Some(path.to_path_buf());
            Ok(())
        }

        fn clear(&self) -> Result<(), NotifyError> {
            *self.notified.borrow_mut() = None;
            Ok(())
        }
    }

    #[test]
    fn mock_notifier_records_path() {
        let mock = MockPathNotifier::new();
        mock.notify(Path::new("/tmp/test.png")).unwrap();
        assert_eq!(
            *mock.notified.borrow(),
            Some(PathBuf::from("/tmp/test.png"))
        );
    }
}
