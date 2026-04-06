//! wl-paste wrapper installation/removal.

use std::fmt;
use std::path::Path;

use crate::domain::wl_paste_wrapper;

/// Error type for wrapper installation.
#[derive(Debug)]
pub enum WrapperError {
    /// I/O error.
    IoError(String),
    /// Non-managed file already exists at the install path.
    ExistingNonManaged(String),
}

impl fmt::Display for WrapperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WrapperError::IoError(msg) => write!(f, "wrapper error: {msg}"),
            WrapperError::ExistingNonManaged(path) => {
                write!(
                    f,
                    "non-managed file exists at {path} (use --force to overwrite)"
                )
            }
        }
    }
}

/// Trait for wrapper installation (enables testing with mock filesystem).
pub trait WrapperInstaller {
    fn install(&self, force: bool) -> Result<String, WrapperError>;
    fn uninstall(&self) -> Result<(), WrapperError>;
    fn is_installed(&self) -> bool;
}

/// Real filesystem-based wrapper installer.
pub struct FsWrapperInstaller {
    home_dir: String,
}

impl FsWrapperInstaller {
    pub fn new(home_dir: String) -> Self {
        Self { home_dir }
    }

    /// Check if a file contains the managed marker.
    fn is_managed(path: &Path) -> bool {
        std::fs::read_to_string(path)
            .map(|content| content.contains(wl_paste_wrapper::WRAPPER_MARKER))
            .unwrap_or(false)
    }
}

impl WrapperInstaller for FsWrapperInstaller {
    fn install(&self, force: bool) -> Result<String, WrapperError> {
        let install_path = wl_paste_wrapper::wrapper_install_path(&self.home_dir);
        let path = Path::new(&install_path);

        // Check for existing non-managed file
        if path.exists() && !Self::is_managed(path) && !force {
            return Err(WrapperError::ExistingNonManaged(install_path));
        }

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| WrapperError::IoError(format!("failed to create directory: {e}")))?;
        }

        // Generate and write the wrapper script
        let script = wl_paste_wrapper::generate_wrapper(wl_paste_wrapper::DEFAULT_REAL_WL_PASTE);
        std::fs::write(path, &script)
            .map_err(|e| WrapperError::IoError(format!("failed to write {install_path}: {e}")))?;

        // Set executable permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(path, perms)
                .map_err(|e| WrapperError::IoError(format!("failed to set permissions: {e}")))?;
        }

        Ok(install_path)
    }

    fn uninstall(&self) -> Result<(), WrapperError> {
        let install_path = wl_paste_wrapper::wrapper_install_path(&self.home_dir);
        let path = Path::new(&install_path);

        if !path.exists() {
            return Ok(()); // Already absent, nothing to do
        }

        if !Self::is_managed(path) {
            return Ok(()); // Not our file, leave it alone
        }

        std::fs::remove_file(path)
            .map_err(|e| WrapperError::IoError(format!("failed to remove {install_path}: {e}")))?;

        Ok(())
    }

    fn is_installed(&self) -> bool {
        let install_path = wl_paste_wrapper::wrapper_install_path(&self.home_dir);
        let path = Path::new(&install_path);
        path.exists() && Self::is_managed(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn setup_temp_dir(name: &str) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!("clipboard2path_test_wrapper_{name}"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    fn cleanup(tmp: &std::path::PathBuf) {
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn install_creates_directory_when_missing() {
        let tmp = setup_temp_dir("create_dir");
        let home = tmp.to_string_lossy().to_string();
        let installer = FsWrapperInstaller::new(home);
        let result = installer.install(false);
        assert!(result.is_ok());

        let expected_dir = tmp.join(".local/bin");
        assert!(expected_dir.exists());
        cleanup(&tmp);
    }

    #[test]
    fn install_writes_script_to_correct_path() {
        let tmp = setup_temp_dir("correct_path");
        let home = tmp.to_string_lossy().to_string();
        let installer = FsWrapperInstaller::new(home);
        let path = installer.install(false).unwrap();

        assert_eq!(
            path,
            format!("{}/.local/bin/wl-paste", tmp.to_string_lossy())
        );
        assert!(Path::new(&path).exists());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains(wl_paste_wrapper::WRAPPER_MARKER));
        cleanup(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn install_sets_executable_permissions() {
        let tmp = setup_temp_dir("perms");
        let home = tmp.to_string_lossy().to_string();
        let installer = FsWrapperInstaller::new(home);
        let path = installer.install(false).unwrap();

        let metadata = std::fs::metadata(&path).unwrap();
        let mode = metadata.permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        cleanup(&tmp);
    }

    #[test]
    fn install_overwrites_managed_file_idempotent() {
        let tmp = setup_temp_dir("idempotent");
        let home = tmp.to_string_lossy().to_string();
        let installer = FsWrapperInstaller::new(home);

        installer.install(false).unwrap();
        // Second install should succeed (managed file)
        let result = installer.install(false);
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    #[test]
    fn install_rejects_non_managed_file_without_force() {
        let tmp = setup_temp_dir("reject_non_managed");
        let home = tmp.to_string_lossy().to_string();

        // Create a non-managed file at the install path
        let install_dir = tmp.join(".local/bin");
        std::fs::create_dir_all(&install_dir).unwrap();
        let install_path = install_dir.join("wl-paste");
        std::fs::write(&install_path, "#!/bin/bash\necho custom\n").unwrap();

        let installer = FsWrapperInstaller::new(home);
        let result = installer.install(false);
        assert!(matches!(result, Err(WrapperError::ExistingNonManaged(_))));
        cleanup(&tmp);
    }

    #[test]
    fn install_force_overwrites_non_managed_file() {
        let tmp = setup_temp_dir("force_overwrite");
        let home = tmp.to_string_lossy().to_string();

        // Create a non-managed file
        let install_dir = tmp.join(".local/bin");
        std::fs::create_dir_all(&install_dir).unwrap();
        let install_path = install_dir.join("wl-paste");
        std::fs::write(&install_path, "#!/bin/bash\necho custom\n").unwrap();

        let installer = FsWrapperInstaller::new(home);
        let result = installer.install(true);
        assert!(result.is_ok());

        // Verify it now contains our marker
        let content = std::fs::read_to_string(&install_path).unwrap();
        assert!(content.contains(wl_paste_wrapper::WRAPPER_MARKER));
        cleanup(&tmp);
    }

    #[test]
    fn uninstall_removes_managed_file() {
        let tmp = setup_temp_dir("uninstall_managed");
        let home = tmp.to_string_lossy().to_string();
        let installer = FsWrapperInstaller::new(home);

        installer.install(false).unwrap();
        let result = installer.uninstall();
        assert!(result.is_ok());

        let install_path = wl_paste_wrapper::wrapper_install_path(&tmp.to_string_lossy());
        assert!(!Path::new(&install_path).exists());
        cleanup(&tmp);
    }

    #[test]
    fn uninstall_does_not_remove_non_managed_file() {
        let tmp = setup_temp_dir("uninstall_non_managed");
        let home = tmp.to_string_lossy().to_string();

        // Create a non-managed file
        let install_dir = tmp.join(".local/bin");
        std::fs::create_dir_all(&install_dir).unwrap();
        let install_path = install_dir.join("wl-paste");
        std::fs::write(&install_path, "#!/bin/bash\necho custom\n").unwrap();

        let installer = FsWrapperInstaller::new(home);
        let result = installer.uninstall();
        assert!(result.is_ok());

        // File should still exist
        assert!(install_path.exists());
        cleanup(&tmp);
    }

    #[test]
    fn uninstall_no_error_when_file_missing() {
        let tmp = setup_temp_dir("uninstall_missing");
        let home = tmp.to_string_lossy().to_string();
        let installer = FsWrapperInstaller::new(home);

        let result = installer.uninstall();
        assert!(result.is_ok());
        cleanup(&tmp);
    }

    #[test]
    fn is_installed_true_for_managed_file() {
        let tmp = setup_temp_dir("is_installed_true");
        let home = tmp.to_string_lossy().to_string();
        let installer = FsWrapperInstaller::new(home);

        installer.install(false).unwrap();
        assert!(installer.is_installed());
        cleanup(&tmp);
    }

    #[test]
    fn is_installed_false_for_non_managed_file() {
        let tmp = setup_temp_dir("is_installed_non_managed");
        let home = tmp.to_string_lossy().to_string();

        let install_dir = tmp.join(".local/bin");
        std::fs::create_dir_all(&install_dir).unwrap();
        std::fs::write(install_dir.join("wl-paste"), "custom script").unwrap();

        let installer = FsWrapperInstaller::new(home);
        assert!(!installer.is_installed());
        cleanup(&tmp);
    }

    #[test]
    fn is_installed_false_when_no_file() {
        let tmp = setup_temp_dir("is_installed_false");
        let home = tmp.to_string_lossy().to_string();
        let installer = FsWrapperInstaller::new(home);

        assert!(!installer.is_installed());
        cleanup(&tmp);
    }
}
