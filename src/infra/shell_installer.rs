//! Shell hook file installation/removal.

use std::fmt;
use std::path::Path;

use crate::domain::shell_detect::ShellType;
use crate::domain::shell_hook;

/// Error type for shell hook installation.
#[derive(Debug)]
pub enum InstallError {
    /// I/O error.
    IoError(String),
    /// Hook already exists (and --force not specified).
    AlreadyExists(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::IoError(msg) => write!(f, "install error: {msg}"),
            InstallError::AlreadyExists(path) => {
                write!(
                    f,
                    "hook already exists at {path} (use --force to overwrite)"
                )
            }
        }
    }
}

/// Trait for shell hook installation (enables testing with mock filesystem).
pub trait ShellInstaller {
    fn install(&self, shell: ShellType, force: bool) -> Result<String, InstallError>;
    fn uninstall(&self, shell: ShellType) -> Result<String, InstallError>;
    fn is_installed(&self, shell: ShellType) -> bool;
}

/// Real filesystem-based installer.
pub struct FsShellInstaller {
    home_dir: String,
}

impl FsShellInstaller {
    pub fn new(home_dir: String) -> Self {
        Self { home_dir }
    }
}

impl ShellInstaller for FsShellInstaller {
    fn install(&self, shell: ShellType, force: bool) -> Result<String, InstallError> {
        let hook_content = shell_hook::generate_hook(shell);

        match shell {
            ShellType::Fish => install_fish_hook(&self.home_dir, &hook_content, force),
            ShellType::Bash | ShellType::Zsh => {
                install_rc_hook(&self.home_dir, shell, &hook_content, force)
            }
        }
    }

    fn uninstall(&self, shell: ShellType) -> Result<String, InstallError> {
        match shell {
            ShellType::Fish => uninstall_fish_hook(&self.home_dir),
            ShellType::Bash | ShellType::Zsh => uninstall_rc_hook(&self.home_dir, shell),
        }
    }

    fn is_installed(&self, shell: ShellType) -> bool {
        match shell {
            ShellType::Fish => {
                let target = shell_hook::hook_install_path(ShellType::Fish, &self.home_dir);
                Path::new(&target).exists()
            }
            ShellType::Bash | ShellType::Zsh => {
                let rc_path = shell_hook::hook_install_path(shell, &self.home_dir);
                let content = std::fs::read_to_string(&rc_path).unwrap_or_default();
                content.contains(shell_hook::HOOK_MARKER)
            }
        }
    }
}

/// Fish: write the hook directly to functions/fish_clipboard_paste.fish
fn install_fish_hook(home: &str, content: &str, force: bool) -> Result<String, InstallError> {
    let target = shell_hook::hook_install_path(ShellType::Fish, home);
    let target_path = Path::new(&target);

    if target_path.exists() && !force {
        return Err(InstallError::AlreadyExists(target));
    }

    crate::infra::file_system::install_file(target_path, content.as_bytes(), 0o644)
        .map_err(|e| InstallError::IoError(e.to_string()))?;

    Ok(target)
}

/// Bash/Zsh: write hook to a separate file and add source line to rc file.
fn install_rc_hook(
    home: &str,
    shell: ShellType,
    content: &str,
    force: bool,
) -> Result<String, InstallError> {
    // Write the hook script to a dedicated file (atomically, 0o644).
    let hook_file = format!("{home}/.config/clipboard2path/hook.{shell}");
    crate::infra::file_system::install_file(Path::new(&hook_file), content.as_bytes(), 0o644)
        .map_err(|e| InstallError::IoError(e.to_string()))?;

    // Add source line to rc file
    let rc_path = shell_hook::hook_install_path(shell, home);
    let existing = std::fs::read_to_string(&rc_path).unwrap_or_default();

    let source_line = shell_hook::generate_source_line(&hook_file)
        .map_err(|e| InstallError::IoError(format!("unsafe hook file path: {e}")))?;

    if existing.contains(shell_hook::HOOK_MARKER) {
        if !force {
            return Err(InstallError::AlreadyExists(rc_path));
        }
        // Remove existing hook lines before re-adding
        let cleaned = remove_hook_lines(&existing);
        let new_content = format!("{}\n{}", cleaned.trim_end(), source_line);
        std::fs::write(&rc_path, new_content)
            .map_err(|e| InstallError::IoError(format!("failed to write {rc_path}: {e}")))?;
    } else {
        // Append to rc file
        let new_content = format!("{}\n{}", existing.trim_end(), source_line);
        std::fs::write(&rc_path, new_content)
            .map_err(|e| InstallError::IoError(format!("failed to write {rc_path}: {e}")))?;
    }

    Ok(rc_path)
}

fn uninstall_fish_hook(home: &str) -> Result<String, InstallError> {
    let target = shell_hook::hook_install_path(ShellType::Fish, home);
    if Path::new(&target).exists() {
        std::fs::remove_file(&target)
            .map_err(|e| InstallError::IoError(format!("failed to remove {target}: {e}")))?;
    }

    // Also remove legacy hook (v0.2.0 installed to functions/)
    let legacy = format!("{home}/.config/fish/functions/fish_clipboard_paste.fish");
    if Path::new(&legacy).exists() {
        let _ = std::fs::remove_file(&legacy);
    }

    Ok(target)
}

fn uninstall_rc_hook(home: &str, shell: ShellType) -> Result<String, InstallError> {
    let rc_path = shell_hook::hook_install_path(shell, home);
    let existing = std::fs::read_to_string(&rc_path).unwrap_or_default();

    if existing.contains(shell_hook::HOOK_MARKER) {
        let cleaned = remove_hook_lines(&existing);
        std::fs::write(&rc_path, cleaned.trim_end().to_string() + "\n")
            .map_err(|e| InstallError::IoError(format!("failed to write {rc_path}: {e}")))?;
    }

    // Also remove the hook file
    let hook_file = format!("{home}/.config/clipboard2path/hook.{shell}");
    if Path::new(&hook_file).exists() {
        let _ = std::fs::remove_file(&hook_file);
    }

    Ok(rc_path)
}

/// Remove lines containing the hook marker and the source line following it.
fn remove_hook_lines(content: &str) -> String {
    let mut result = String::new();
    let mut skip_next = false;

    for line in content.lines() {
        if line.contains(shell_hook::HOOK_MARKER) {
            skip_next = true;
            continue;
        }
        if skip_next {
            skip_next = false;
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_hook_lines_removes_marker_and_source() {
        let content = "line1\n# clipboard2path-wsl shell hook\nsource \"/path\"\nline3\n";
        let result = remove_hook_lines(content);
        assert_eq!(result, "line1\nline3\n");
    }

    #[test]
    fn remove_hook_lines_no_marker_unchanged() {
        let content = "line1\nline2\n";
        let result = remove_hook_lines(content);
        assert_eq!(result, "line1\nline2\n");
    }

    #[test]
    fn install_fish_creates_file() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_fish_install");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home);
        let result = installer.install(ShellType::Fish, false);
        assert!(result.is_ok());

        let target = result.unwrap();
        assert!(Path::new(&target).exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_fish_sets_644_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = std::env::temp_dir().join("clipboard2path_test_fish_perms");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home);
        let target = installer.install(ShellType::Fish, false).unwrap();

        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "fish hook should have 0o644 permissions");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_bash_hook_file_sets_644_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = std::env::temp_dir().join("clipboard2path_test_bash_perms");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let bashrc = tmp.join(".bashrc");
        std::fs::write(&bashrc, "existing\n").unwrap();

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home.clone());
        installer.install(ShellType::Bash, false).unwrap();

        let hook_file = format!("{home}/.config/clipboard2path/hook.bash");
        let mode = std::fs::metadata(&hook_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o644, "bash hook file should have 0o644 permissions");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_fish_rejects_existing_without_force() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_fish_no_force");
        let _ = std::fs::remove_dir_all(&tmp);

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home);

        // First install
        installer.install(ShellType::Fish, false).unwrap();
        // Second install without force should fail
        let result = installer.install(ShellType::Fish, false);
        assert!(matches!(result, Err(InstallError::AlreadyExists(_))));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_fish_force_overwrites() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_fish_force");
        let _ = std::fs::remove_dir_all(&tmp);

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home);

        installer.install(ShellType::Fish, false).unwrap();
        let result = installer.install(ShellType::Fish, true);
        assert!(result.is_ok());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn uninstall_fish_removes_file() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_fish_uninstall");
        let _ = std::fs::remove_dir_all(&tmp);

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home);

        installer.install(ShellType::Fish, false).unwrap();
        let target = installer.uninstall(ShellType::Fish).unwrap();
        assert!(!Path::new(&target).exists());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn is_installed_fish_true_when_file_exists() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_fish_is_installed_true");
        let _ = std::fs::remove_dir_all(&tmp);

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home);
        installer.install(ShellType::Fish, false).unwrap();

        assert!(installer.is_installed(ShellType::Fish));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn is_installed_fish_false_when_no_file() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_fish_is_installed_false");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home);

        assert!(!installer.is_installed(ShellType::Fish));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn is_installed_bash_true_when_marker_present() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_bash_is_installed_true");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let bashrc = tmp.join(".bashrc");
        std::fs::write(&bashrc, "existing\n").unwrap();

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home);
        installer.install(ShellType::Bash, false).unwrap();

        assert!(installer.is_installed(ShellType::Bash));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn is_installed_bash_false_when_no_marker() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_bash_is_installed_false");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let bashrc = tmp.join(".bashrc");
        std::fs::write(&bashrc, "just normal content\n").unwrap();

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home);

        assert!(!installer.is_installed(ShellType::Bash));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn is_installed_zsh_false_when_no_rc_file() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_zsh_is_installed_false");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home);

        assert!(!installer.is_installed(ShellType::Zsh));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_bash_adds_source_line() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_bash_install");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Create an existing .bashrc
        let bashrc = tmp.join(".bashrc");
        std::fs::write(&bashrc, "existing content\n").unwrap();

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home);
        installer.install(ShellType::Bash, false).unwrap();

        let content = std::fs::read_to_string(&bashrc).unwrap();
        assert!(content.contains(shell_hook::HOOK_MARKER));
        assert!(content.contains("source"));
        assert!(content.contains("existing content"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn uninstall_bash_removes_source_line() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_bash_uninstall");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let bashrc = tmp.join(".bashrc");
        std::fs::write(&bashrc, "existing content\n").unwrap();

        let home = tmp.to_string_lossy().to_string();
        let installer = FsShellInstaller::new(home.clone());
        installer.install(ShellType::Bash, false).unwrap();
        installer.uninstall(ShellType::Bash).unwrap();

        let content = std::fs::read_to_string(&bashrc).unwrap();
        assert!(!content.contains(shell_hook::HOOK_MARKER));
        assert!(content.contains("existing content"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
