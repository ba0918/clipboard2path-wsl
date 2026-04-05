//! Daemon lifecycle management (directory setup/teardown).

use std::fmt;
use std::path::Path;

/// Error type for lifecycle operations.
#[derive(Debug)]
pub enum LifecycleError {
    /// I/O error during setup or teardown.
    IoError(String),
}

impl fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LifecycleError::IoError(msg) => write!(f, "lifecycle error: {msg}"),
        }
    }
}

/// Daemon lifecycle trait for directory management.
pub trait DaemonLifecycle {
    /// Create the runtime directory with secure permissions.
    fn setup(&self, dir: &Path) -> Result<(), LifecycleError>;
    /// Remove all files in the directory and the directory itself.
    fn teardown(&self, dir: &Path) -> Result<(), LifecycleError>;
}

/// Real filesystem implementation.
pub struct FsDaemonLifecycle;

impl DaemonLifecycle for FsDaemonLifecycle {
    fn setup(&self, dir: &Path) -> Result<(), LifecycleError> {
        use std::fs;
        use std::os::unix::fs::DirBuilderExt;

        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|e| LifecycleError::IoError(format!("failed to create {}: {e}", dir.display())))
    }

    fn teardown(&self, dir: &Path) -> Result<(), LifecycleError> {
        use std::fs;

        if !dir.exists() {
            return Ok(());
        }

        // Remove all files in the directory
        let entries =
            fs::read_dir(dir).map_err(|e| LifecycleError::IoError(e.to_string()))?;

        for entry in entries {
            let entry = entry.map_err(|e| LifecycleError::IoError(e.to_string()))?;
            let path = entry.path();
            if path.is_file() || path.is_symlink() {
                fs::remove_file(&path)
                    .map_err(|e| LifecycleError::IoError(e.to_string()))?;
            }
        }

        // Remove the directory itself
        fs::remove_dir(dir)
            .map_err(|e| LifecycleError::IoError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn setup_creates_directory_with_correct_permissions() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_lifecycle_setup");
        let _ = std::fs::remove_dir_all(&tmp);

        let lifecycle = FsDaemonLifecycle;
        lifecycle.setup(&tmp).expect("setup should succeed");

        assert!(tmp.is_dir());
        let mode = std::fs::metadata(&tmp).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn teardown_removes_files_and_directory() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_lifecycle_teardown");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("test.txt"), b"data").unwrap();
        std::fs::write(tmp.join("test2.txt"), b"data2").unwrap();

        let lifecycle = FsDaemonLifecycle;
        lifecycle.teardown(&tmp).expect("teardown should succeed");

        assert!(!tmp.exists());
    }

    #[test]
    fn teardown_nonexistent_dir_is_ok() {
        let lifecycle = FsDaemonLifecycle;
        let result = lifecycle.teardown(Path::new("/nonexistent_dir_99999"));
        assert!(result.is_ok());
    }
}
