//! Systemd service installation and management.

use std::fmt;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::domain::systemd_unit;
use crate::infra::command_runner::CommandRunner;

/// Error type for systemd installation operations.
#[derive(Debug)]
pub enum InstallError {
    IoError(String),
    CommandError(String),
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstallError::IoError(msg) => write!(f, "systemd install error: {msg}"),
            InstallError::CommandError(msg) => write!(f, "systemd command error: {msg}"),
        }
    }
}

/// Trait for systemd service management (enables DI for testing).
pub trait SystemdInstaller {
    /// Install the systemd unit file and enable the service.
    fn install(&self, unit_content: &str, home: &str) -> Result<(), InstallError>;
    /// Stop, disable, and remove the systemd unit file.
    fn uninstall(&self, home: &str) -> Result<(), InstallError>;
    /// Check if the service is active. Returns status string (e.g. "active", "inactive").
    fn is_active(&self) -> Result<String, String>;
    /// Check if the unit file is installed on disk.
    fn is_installed(&self, home: &str) -> bool;
}

/// Filesystem-based systemd installer with injected CommandRunner.
pub struct FsSystemdInstaller<R: CommandRunner> {
    runner: R,
}

impl<R: CommandRunner> FsSystemdInstaller<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl<R: CommandRunner> SystemdInstaller for FsSystemdInstaller<R> {
    fn install(&self, unit_content: &str, home: &str) -> Result<(), InstallError> {
        let path = systemd_unit::unit_install_path(home);
        let path_ref = Path::new(&path);

        // Create parent directory
        if let Some(parent) = path_ref.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| InstallError::IoError(format!("failed to create directory: {e}")))?;
        }

        // Write unit file
        fs::write(path_ref, unit_content)
            .map_err(|e| InstallError::IoError(format!("failed to write {path}: {e}")))?;

        // Set permissions to 0o644
        fs::set_permissions(path_ref, fs::Permissions::from_mode(0o644))
            .map_err(|e| InstallError::IoError(format!("failed to set permissions: {e}")))?;

        // daemon-reload
        self.runner
            .run("systemctl", &["--user", "daemon-reload"])
            .map_err(InstallError::CommandError)?;

        // enable --now (idempotent: already enabled is fine)
        self.runner
            .run(
                "systemctl",
                &["--user", "enable", "--now", systemd_unit::SERVICE_NAME],
            )
            .map_err(InstallError::CommandError)?;

        Ok(())
    }

    fn uninstall(&self, home: &str) -> Result<(), InstallError> {
        // stop (ignore failure — may not be running)
        let _ = self
            .runner
            .run("systemctl", &["--user", "stop", systemd_unit::SERVICE_NAME]);

        // disable (ignore failure — may not be enabled)
        let _ = self.runner.run(
            "systemctl",
            &["--user", "disable", systemd_unit::SERVICE_NAME],
        );

        // Remove unit file (ignore if not present)
        let path = systemd_unit::unit_install_path(home);
        if Path::new(&path).exists() {
            fs::remove_file(&path)
                .map_err(|e| InstallError::IoError(format!("failed to remove {path}: {e}")))?;
        }

        // daemon-reload
        self.runner
            .run("systemctl", &["--user", "daemon-reload"])
            .map_err(InstallError::CommandError)?;

        Ok(())
    }

    fn is_active(&self) -> Result<String, String> {
        // `systemctl is-active` prints the state to stdout and encodes it in the exit
        // code (0 = active, non-zero = inactive/failed/...). run_capturing() preserves
        // both, so we judge on the terminal state instead of parsing an error string.
        let output = self.runner.run_capturing(
            "systemctl",
            &["--user", "is-active", systemd_unit::SERVICE_NAME],
        )?;

        if output.exit_code == Some(0) {
            return Ok("active".to_string());
        }

        for state in ["inactive", "failed", "activating"] {
            if output.stdout == state {
                return Ok(state.to_string());
            }
        }

        // An unexpected termination (unknown state word, or a signal kill) must not be
        // silently reported as "unknown"; surface a diagnostic the caller can act on.
        Err(format!(
            "unexpected 'systemctl is-active' result (exit_code: {:?}, stdout: {:?}, stderr: {:?})",
            output.exit_code, output.stdout, output.stderr
        ))
    }

    fn is_installed(&self, home: &str) -> bool {
        let path = systemd_unit::unit_install_path(home);
        Path::new(&path).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::command_runner::CommandOutput;
    use crate::infra::command_runner::testing::MockCommandRunner;

    #[test]
    fn install_creates_dir_writes_file_and_runs_commands() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_systemd_install");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let home = tmp.to_string_lossy().to_string();
        let mock = MockCommandRunner::new(vec![
            Ok(String::new()), // daemon-reload
            Ok(String::new()), // enable --now
        ]);
        let installer = FsSystemdInstaller::new(mock);
        let result = installer.install("[Unit]\nDescription=test\n", &home);
        assert!(result.is_ok());

        // Verify file was written
        let unit_path = systemd_unit::unit_install_path(&home);
        assert!(Path::new(&unit_path).exists());

        // Verify file content
        let content = fs::read_to_string(&unit_path).unwrap();
        assert!(content.contains("[Unit]"));

        // Verify permissions
        let metadata = fs::metadata(&unit_path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o644);

        // Verify command order: daemon-reload then enable --now
        let calls = installer.runner.get_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, vec!["--user", "daemon-reload"]);
        assert_eq!(
            calls[1].1,
            vec!["--user", "enable", "--now", "clipboard2path.service"]
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_twice_succeeds_idempotent() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_systemd_idempotent");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let home = tmp.to_string_lossy().to_string();

        // First install
        let mock1 = MockCommandRunner::new(vec![Ok(String::new()), Ok(String::new())]);
        let installer1 = FsSystemdInstaller::new(mock1);
        assert!(installer1.install("content-v1", &home).is_ok());

        // Second install (overwrites)
        let mock2 = MockCommandRunner::new(vec![Ok(String::new()), Ok(String::new())]);
        let installer2 = FsSystemdInstaller::new(mock2);
        assert!(installer2.install("content-v2", &home).is_ok());

        // Verify content is updated
        let unit_path = systemd_unit::unit_install_path(&home);
        let content = fs::read_to_string(&unit_path).unwrap();
        assert_eq!(content, "content-v2");

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn uninstall_runs_stop_disable_remove_reload() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_systemd_uninstall");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let home = tmp.to_string_lossy().to_string();

        // Pre-create the unit file
        let unit_path = systemd_unit::unit_install_path(&home);
        fs::create_dir_all(Path::new(&unit_path).parent().unwrap()).unwrap();
        fs::write(&unit_path, "test").unwrap();

        let mock = MockCommandRunner::new(vec![
            Ok(String::new()), // stop
            Ok(String::new()), // disable
            Ok(String::new()), // daemon-reload
        ]);
        let installer = FsSystemdInstaller::new(mock);
        let result = installer.uninstall(&home);
        assert!(result.is_ok());

        // Verify file removed
        assert!(!Path::new(&unit_path).exists());

        // Verify command order
        let calls = installer.runner.get_calls();
        assert_eq!(calls.len(), 3);
        assert!(calls[0].1.contains(&"stop".to_string()));
        assert!(calls[1].1.contains(&"disable".to_string()));
        assert_eq!(calls[2].1, vec!["--user", "daemon-reload"]);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn uninstall_no_file_succeeds_idempotent() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_systemd_uninstall_nofile");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let home = tmp.to_string_lossy().to_string();

        let mock = MockCommandRunner::new(vec![
            Err("not loaded".to_string()), // stop fails — ignored
            Err("not loaded".to_string()), // disable fails — ignored
            Ok(String::new()),             // daemon-reload
        ]);
        let installer = FsSystemdInstaller::new(mock);
        let result = installer.uninstall(&home);
        assert!(result.is_ok());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn is_installed_true_when_file_exists() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_systemd_is_installed");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let home = tmp.to_string_lossy().to_string();
        let unit_path = systemd_unit::unit_install_path(&home);
        fs::create_dir_all(Path::new(&unit_path).parent().unwrap()).unwrap();
        fs::write(&unit_path, "test").unwrap();

        let mock = MockCommandRunner::new(vec![]);
        let installer = FsSystemdInstaller::new(mock);
        assert!(installer.is_installed(&home));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn is_installed_false_when_no_file() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_systemd_not_installed");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let home = tmp.to_string_lossy().to_string();
        let mock = MockCommandRunner::new(vec![]);
        let installer = FsSystemdInstaller::new(mock);
        assert!(!installer.is_installed(&home));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn is_active_exit_zero_is_active() {
        let mock = MockCommandRunner::new(vec![]).with_capturing(vec![Ok(CommandOutput {
            exit_code: Some(0),
            stdout: "active".to_string(),
            stderr: String::new(),
        })]);
        let installer = FsSystemdInstaller::new(mock);
        assert_eq!(installer.is_active(), Ok("active".to_string()));
    }

    #[test]
    fn is_active_exit3_stdout_inactive_is_inactive() {
        let mock = MockCommandRunner::new(vec![]).with_capturing(vec![Ok(CommandOutput {
            exit_code: Some(3),
            stdout: "inactive".to_string(),
            stderr: String::new(),
        })]);
        let installer = FsSystemdInstaller::new(mock);
        assert_eq!(installer.is_active(), Ok("inactive".to_string()));
    }

    #[test]
    fn is_active_exit3_stdout_failed_is_failed() {
        let mock = MockCommandRunner::new(vec![]).with_capturing(vec![Ok(CommandOutput {
            exit_code: Some(3),
            stdout: "failed".to_string(),
            stderr: String::new(),
        })]);
        let installer = FsSystemdInstaller::new(mock);
        assert_eq!(installer.is_active(), Ok("failed".to_string()));
    }

    #[test]
    fn is_active_unknown_state_returns_diagnostic_error() {
        let mock = MockCommandRunner::new(vec![]).with_capturing(vec![Ok(CommandOutput {
            exit_code: Some(4),
            stdout: String::new(),
            stderr: "Failed to connect to bus".to_string(),
        })]);
        let installer = FsSystemdInstaller::new(mock);
        let result = installer.is_active();
        assert!(
            result.is_err(),
            "unknown state must not be silent 'unknown'"
        );
        let msg = result.unwrap_err();
        assert!(
            msg.contains('4'),
            "diagnostic must include exit code: {msg}"
        );
        assert!(
            msg.contains("Failed to connect to bus"),
            "diagnostic must include stderr: {msg}"
        );
    }

    #[test]
    fn is_active_signal_termination_returns_diagnostic_error() {
        let mock = MockCommandRunner::new(vec![]).with_capturing(vec![Ok(CommandOutput {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
        })]);
        let installer = FsSystemdInstaller::new(mock);
        assert!(installer.is_active().is_err());
    }

    #[test]
    fn is_active_spawn_failure_propagates_error() {
        let mock = MockCommandRunner::new(vec![])
            .with_capturing(vec![Err("failed to execute 'systemctl'".to_string())]);
        let installer = FsSystemdInstaller::new(mock);
        assert!(installer.is_active().is_err());
    }

    #[test]
    fn install_enable_non_zero_returns_install_error() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_install_enable_fail");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let home = tmp.to_string_lossy().to_string();

        let mock = MockCommandRunner::new(vec![
            Ok(String::new()),                // daemon-reload
            Err("enable failed".to_string()), // enable --now
        ]);
        let installer = FsSystemdInstaller::new(mock);
        assert!(matches!(
            installer.install("content", &home),
            Err(InstallError::CommandError(_))
        ));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn install_daemon_reload_non_zero_returns_install_error() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_install_reload_fail");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let home = tmp.to_string_lossy().to_string();

        let mock = MockCommandRunner::new(vec![
            Err("reload failed".to_string()), // daemon-reload
        ]);
        let installer = FsSystemdInstaller::new(mock);
        assert!(matches!(
            installer.install("content", &home),
            Err(InstallError::CommandError(_))
        ));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn uninstall_daemon_reload_non_zero_returns_install_error() {
        let tmp = std::env::temp_dir().join("clipboard2path_test_uninstall_reload_fail");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let home = tmp.to_string_lossy().to_string();

        let mock = MockCommandRunner::new(vec![
            Err("stop failed".to_string()),    // stop — ignored
            Err("disable failed".to_string()), // disable — ignored
            Err("reload failed".to_string()),  // daemon-reload — propagates
        ]);
        let installer = FsSystemdInstaller::new(mock);
        assert!(matches!(
            installer.uninstall(&home),
            Err(InstallError::CommandError(_))
        ));

        let _ = fs::remove_dir_all(&tmp);
    }
}
