//! Setup orchestration: init / uninstall / status.
//!
//! Coordinates the shell hook, systemd service, and wl-paste wrapper installers
//! and returns a structured [`SetupOutcome`] (or status lines) instead of printing.
//! Presentation — stream routing (stdout/stderr), the "Next steps" block, and the
//! process exit code — is the caller's (main.rs) responsibility. Environment I/O
//! (unit content generation, `$SHELL` detection, `which wl-paste`, the latest-image
//! lookup) is likewise done by the caller and passed in, keeping this layer mockable.

use crate::domain::shell_detect::ShellType;
use crate::domain::wl_paste_wrapper;
use crate::infra::shell_installer::ShellInstaller;
use crate::infra::systemd_installer::SystemdInstaller;
use crate::infra::wrapper_installer::WrapperInstaller;

/// Structured result of an init/uninstall run.
///
/// `errors` and `results` both render to stderr; the caller prints `errors`
/// first (as the original code did during processing), then `results`.
#[derive(Debug, Default, PartialEq)]
pub struct SetupOutcome {
    /// Error/warning lines (e.g. `error: shell hook: ...`), in component order.
    pub errors: Vec<String>,
    /// Success/skip summary lines (e.g. `✔ shell hook installed (fish)`).
    pub results: Vec<String>,
    /// Whether any component failed (drives the caller's exit code).
    pub has_error: bool,
}

/// Orchestrates the three installers. Borrows them so callers (and tests) retain
/// ownership and can inspect recorded calls afterward.
pub struct SetupService<'a, SH, SY, W>
where
    SH: ShellInstaller,
    SY: SystemdInstaller,
    W: WrapperInstaller,
{
    shell: &'a SH,
    systemd: &'a SY,
    wrapper: &'a W,
    home: String,
}

impl<'a, SH, SY, W> SetupService<'a, SH, SY, W>
where
    SH: ShellInstaller,
    SY: SystemdInstaller,
    W: WrapperInstaller,
{
    pub fn new(shell: &'a SH, systemd: &'a SY, wrapper: &'a W, home: String) -> Self {
        Self {
            shell,
            systemd,
            wrapper,
            home,
        }
    }

    /// Orchestrate `init`: install shell hook, systemd service, and wrapper.
    ///
    /// `unit_content` carries the pre-generated systemd unit (`Err` if generation
    /// failed in the caller); it is consulted only when `no_service` is false.
    /// Components are best-effort: a failure is recorded but the run continues.
    pub fn run_init(
        &self,
        shell: ShellType,
        no_service: bool,
        force: bool,
        unit_content: Result<&str, &str>,
    ) -> SetupOutcome {
        let mut outcome = SetupOutcome::default();

        // Shell hook (force=true for idempotent updates).
        match self.shell.install(shell, true) {
            Ok(_path) => outcome
                .results
                .push(format!("\u{2714} shell hook installed ({shell})")),
            Err(e) => {
                outcome.errors.push(format!("error: shell hook: {e}"));
                outcome.has_error = true;
            }
        }

        // Systemd service. A unit-generation failure (Err) surfaces as the same
        // "systemd service" error the old install_systemd_service returned.
        if no_service {
            outcome
                .results
                .push("- systemd service skipped (--no-service)".to_string());
        } else {
            let installed = unit_content.map_err(|e| e.to_string()).and_then(|content| {
                self.systemd
                    .install(content, &self.home)
                    .map_err(|e| e.to_string())
            });
            match installed {
                Ok(()) => outcome
                    .results
                    .push("\u{2714} systemd service installed and started".to_string()),
                Err(e) => {
                    outcome.errors.push(format!("error: systemd service: {e}"));
                    outcome.has_error = true;
                }
            }
        }

        // wl-paste wrapper (only when the service is enabled).
        if !no_service {
            match self.wrapper.install(force) {
                Ok(path) => outcome
                    .results
                    .push(format!("\u{2714} wl-paste wrapper installed ({path})")),
                Err(e) => {
                    outcome.errors.push(format!("error: wl-paste wrapper: {e}"));
                    outcome.has_error = true;
                }
            }
        }

        outcome
    }

    /// Orchestrate `uninstall`. Best-effort: attempt every removal, aggregate results.
    pub fn run_uninstall(&self, shell: ShellType, no_service: bool) -> SetupOutcome {
        let mut outcome = SetupOutcome::default();

        // Shell hook removal.
        match self.shell.uninstall(shell) {
            Ok(_path) => outcome
                .results
                .push(format!("\u{2714} shell hook removed ({shell})")),
            Err(e) => {
                outcome.errors.push(format!("error: shell hook: {e}"));
                outcome.has_error = true;
            }
        }

        // Systemd service removal.
        if no_service {
            outcome
                .results
                .push("- systemd service skipped (--no-service)".to_string());
        } else {
            match self.systemd.uninstall(&self.home) {
                Ok(()) => outcome
                    .results
                    .push("\u{2714} systemd service stopped and removed".to_string()),
                Err(e) => {
                    outcome.errors.push(format!("error: systemd service: {e}"));
                    outcome.has_error = true;
                }
            }
        }

        // wl-paste wrapper removal (always attempted).
        match self.wrapper.uninstall() {
            Ok(()) => {
                if self.wrapper.is_installed() {
                    outcome
                        .errors
                        .push("warning: wl-paste wrapper removal may have failed".to_string());
                } else {
                    outcome
                        .results
                        .push("\u{2714} wl-paste wrapper removed".to_string());
                }
            }
            Err(e) => {
                outcome.errors.push(format!("error: wl-paste wrapper: {e}"));
                outcome.has_error = true;
            }
        }

        outcome
    }

    /// Collect status lines (all rendered to stdout by the caller).
    ///
    /// `shell` is the caller-detected shell (`None` = unrecognized `$SHELL`).
    /// `wl_paste_resolved` / `latest_image` are caller-provided env lookups.
    pub fn run_status(
        &self,
        shell: Option<ShellType>,
        wl_paste_resolved: Option<&str>,
        latest_image: Option<&str>,
    ) -> Vec<String> {
        let mut lines = vec!["clipboard2path-wsl status:".to_string()];

        // Service status.
        if self.systemd.is_installed(&self.home) {
            match self.systemd.is_active() {
                Ok(status) => lines.push(format!("  service: {status}")),
                Err(e) => lines.push(format!("  service: error ({e})")),
            }
        } else {
            lines.push("  service: not installed".to_string());
        }

        // Shell hook status.
        match shell {
            Some(sh) => {
                if self.shell.is_installed(sh) {
                    lines.push(format!("  shell hook: installed ({sh})"));
                } else {
                    lines.push("  shell hook: not installed".to_string());
                }
            }
            None => lines.push("  shell hook: unknown shell".to_string()),
        }

        // wl-paste wrapper status.
        if self.wrapper.is_installed() {
            let wrapper_path = wl_paste_wrapper::wrapper_install_path(&self.home);
            lines.push(format!("  wl-paste wrapper: installed ({wrapper_path})"));
            if let Some(resolved) = wl_paste_resolved {
                lines.push(format!("  wl-paste resolves to: {resolved}"));
            }
        } else {
            lines.push("  wl-paste wrapper: not installed".to_string());
        }

        // Latest image path.
        match latest_image {
            Some(path) if !path.is_empty() => lines.push(format!("  latest image: {path}")),
            _ => lines.push("  latest image: (none)".to_string()),
        }

        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::shell_installer::InstallError as ShellInstallError;
    use crate::infra::systemd_installer::InstallError as SystemdInstallError;
    use crate::infra::wrapper_installer::WrapperError;
    use std::cell::RefCell;

    struct MockShell {
        install_ok: bool,
        uninstall_ok: bool,
        installed: bool,
        calls: RefCell<Vec<String>>,
    }
    impl MockShell {
        fn new() -> Self {
            Self {
                install_ok: true,
                uninstall_ok: true,
                installed: true,
                calls: RefCell::new(Vec::new()),
            }
        }
    }
    impl ShellInstaller for MockShell {
        fn install(&self, shell: ShellType, force: bool) -> Result<String, ShellInstallError> {
            self.calls
                .borrow_mut()
                .push(format!("install:{shell}:{force}"));
            if self.install_ok {
                Ok("hook-path".to_string())
            } else {
                Err(ShellInstallError::IoError("shell boom".to_string()))
            }
        }
        fn uninstall(&self, shell: ShellType) -> Result<String, ShellInstallError> {
            self.calls.borrow_mut().push(format!("uninstall:{shell}"));
            if self.uninstall_ok {
                Ok("hook-path".to_string())
            } else {
                Err(ShellInstallError::IoError("shell rm boom".to_string()))
            }
        }
        fn is_installed(&self, _shell: ShellType) -> bool {
            self.installed
        }
    }

    struct MockSystemd {
        install_ok: bool,
        uninstall_ok: bool,
        installed: bool,
        active: Result<String, String>,
        calls: RefCell<Vec<String>>,
    }
    impl MockSystemd {
        fn new() -> Self {
            Self {
                install_ok: true,
                uninstall_ok: true,
                installed: true,
                active: Ok("active".to_string()),
                calls: RefCell::new(Vec::new()),
            }
        }
    }
    impl SystemdInstaller for MockSystemd {
        fn install(&self, unit_content: &str, _home: &str) -> Result<(), SystemdInstallError> {
            self.calls
                .borrow_mut()
                .push(format!("install:{unit_content}"));
            if self.install_ok {
                Ok(())
            } else {
                Err(SystemdInstallError::CommandError(
                    "systemd boom".to_string(),
                ))
            }
        }
        fn uninstall(&self, _home: &str) -> Result<(), SystemdInstallError> {
            self.calls.borrow_mut().push("uninstall".to_string());
            if self.uninstall_ok {
                Ok(())
            } else {
                Err(SystemdInstallError::CommandError(
                    "systemd rm boom".to_string(),
                ))
            }
        }
        fn is_active(&self) -> Result<String, String> {
            self.active.clone()
        }
        fn is_installed(&self, _home: &str) -> bool {
            self.installed
        }
    }

    struct MockWrapper {
        install_ok: bool,
        uninstall_ok: bool,
        installed: bool,
        calls: RefCell<Vec<String>>,
    }
    impl MockWrapper {
        fn new() -> Self {
            Self {
                install_ok: true,
                uninstall_ok: true,
                installed: true,
                calls: RefCell::new(Vec::new()),
            }
        }
    }
    impl WrapperInstaller for MockWrapper {
        fn install(&self, force: bool) -> Result<String, WrapperError> {
            self.calls.borrow_mut().push(format!("install:{force}"));
            if self.install_ok {
                Ok("wrapper-path".to_string())
            } else {
                Err(WrapperError::IoError("wrapper boom".to_string()))
            }
        }
        fn uninstall(&self) -> Result<(), WrapperError> {
            self.calls.borrow_mut().push("uninstall".to_string());
            if self.uninstall_ok {
                Ok(())
            } else {
                Err(WrapperError::IoError("wrapper rm boom".to_string()))
            }
        }
        fn is_installed(&self) -> bool {
            self.installed
        }
    }

    fn service<'a>(
        shell: &'a MockShell,
        systemd: &'a MockSystemd,
        wrapper: &'a MockWrapper,
    ) -> SetupService<'a, MockShell, MockSystemd, MockWrapper> {
        SetupService::new(shell, systemd, wrapper, "/home/test".to_string())
    }

    #[test]
    fn init_installs_hook_service_and_wrapper() {
        let shell = MockShell::new();
        let systemd = MockSystemd::new();
        let wrapper = MockWrapper::new();
        let outcome =
            service(&shell, &systemd, &wrapper).run_init(ShellType::Fish, false, false, Ok("unit"));

        assert!(!outcome.has_error);
        assert_eq!(shell.calls.borrow().len(), 1);
        assert_eq!(systemd.calls.borrow().len(), 1);
        assert_eq!(wrapper.calls.borrow().len(), 1);
        assert!(outcome.results.iter().any(|l| l.contains("shell hook")));
        assert!(
            outcome
                .results
                .iter()
                .any(|l| l.contains("systemd service"))
        );
        assert!(
            outcome
                .results
                .iter()
                .any(|l| l.contains("wl-paste wrapper"))
        );
    }

    #[test]
    fn init_no_service_skips_service_and_wrapper() {
        let shell = MockShell::new();
        let systemd = MockSystemd::new();
        let wrapper = MockWrapper::new();
        let outcome =
            service(&shell, &systemd, &wrapper).run_init(ShellType::Bash, true, false, Ok(""));

        assert!(!outcome.has_error);
        assert_eq!(systemd.calls.borrow().len(), 0, "service must be skipped");
        assert_eq!(wrapper.calls.borrow().len(), 0, "wrapper must be skipped");
        assert!(
            outcome
                .results
                .iter()
                .any(|l| l.contains("skipped (--no-service)"))
        );
    }

    #[test]
    fn init_unit_generation_failure_is_systemd_error() {
        let shell = MockShell::new();
        let systemd = MockSystemd::new();
        let wrapper = MockWrapper::new();
        let outcome = service(&shell, &systemd, &wrapper).run_init(
            ShellType::Fish,
            false,
            false,
            Err("failed to resolve binary path"),
        );

        assert!(outcome.has_error);
        assert_eq!(
            systemd.calls.borrow().len(),
            0,
            "install must not run when unit generation failed"
        );
        assert!(
            outcome
                .errors
                .iter()
                .any(|l| l.contains("systemd service") && l.contains("resolve binary path"))
        );
    }

    #[test]
    fn init_installer_failure_is_aggregated() {
        let mut shell = MockShell::new();
        shell.install_ok = false;
        let systemd = MockSystemd::new();
        let wrapper = MockWrapper::new();
        let outcome =
            service(&shell, &systemd, &wrapper).run_init(ShellType::Fish, false, false, Ok("unit"));

        assert!(outcome.has_error);
        // best-effort: service and wrapper still attempted despite the hook failure.
        assert_eq!(systemd.calls.borrow().len(), 1);
        assert_eq!(wrapper.calls.borrow().len(), 1);
        assert!(outcome.errors.iter().any(|l| l.contains("shell hook")));
    }

    #[test]
    fn uninstall_removes_all_components() {
        let shell = MockShell::new();
        let systemd = MockSystemd::new();
        let wrapper = MockWrapper::new();
        let outcome = service(&shell, &systemd, &wrapper).run_uninstall(ShellType::Fish, false);

        assert!(!outcome.has_error);
        assert_eq!(shell.calls.borrow()[0], "uninstall:fish");
        assert_eq!(systemd.calls.borrow()[0], "uninstall");
        assert_eq!(wrapper.calls.borrow()[0], "uninstall");
    }

    #[test]
    fn uninstall_is_best_effort_and_aggregates_failures() {
        let mut shell = MockShell::new();
        shell.uninstall_ok = false;
        let mut systemd = MockSystemd::new();
        systemd.uninstall_ok = false;
        let wrapper = MockWrapper::new();
        let outcome = service(&shell, &systemd, &wrapper).run_uninstall(ShellType::Fish, false);

        assert!(outcome.has_error);
        // Every component was attempted despite earlier failures (no fail-fast).
        assert_eq!(shell.calls.borrow().len(), 1);
        assert_eq!(systemd.calls.borrow().len(), 1);
        assert_eq!(wrapper.calls.borrow().len(), 1);
        assert!(outcome.errors.iter().any(|l| l.contains("shell hook")));
        assert!(outcome.errors.iter().any(|l| l.contains("systemd service")));
    }

    #[test]
    fn uninstall_no_service_skips_service() {
        let shell = MockShell::new();
        let systemd = MockSystemd::new();
        let wrapper = MockWrapper::new();
        let outcome = service(&shell, &systemd, &wrapper).run_uninstall(ShellType::Fish, true);

        assert!(!outcome.has_error);
        assert_eq!(systemd.calls.borrow().len(), 0);
        assert!(
            outcome
                .results
                .iter()
                .any(|l| l.contains("skipped (--no-service)"))
        );
    }

    #[test]
    fn status_collects_component_states() {
        let shell = MockShell::new();
        let systemd = MockSystemd::new();
        let wrapper = MockWrapper::new();
        let lines = service(&shell, &systemd, &wrapper).run_status(
            Some(ShellType::Fish),
            Some("/home/test/.local/bin/wl-paste"),
            Some("/run/user/1000/clip.png"),
        );

        let joined = lines.join("\n");
        assert!(joined.contains("service: active"));
        assert!(joined.contains("shell hook: installed (fish)"));
        assert!(joined.contains("wl-paste wrapper: installed"));
        assert!(joined.contains("wl-paste resolves to: /home/test/.local/bin/wl-paste"));
        assert!(joined.contains("latest image: /run/user/1000/clip.png"));
    }

    #[test]
    fn status_reports_unknown_shell_when_none() {
        let shell = MockShell::new();
        let systemd = MockSystemd::new();
        let wrapper = MockWrapper::new();
        let lines = service(&shell, &systemd, &wrapper).run_status(None, None, None);

        let joined = lines.join("\n");
        assert!(joined.contains("shell hook: unknown shell"));
        assert!(joined.contains("latest image: (none)"));
    }

    // (bottom marker import removed; wl_paste_wrapper used once run_status lands)
    #[test]
    fn status_reports_not_installed_components() {
        let mut shell = MockShell::new();
        shell.installed = false;
        let mut systemd = MockSystemd::new();
        systemd.installed = false;
        let mut wrapper = MockWrapper::new();
        wrapper.installed = false;
        let lines =
            service(&shell, &systemd, &wrapper).run_status(Some(ShellType::Zsh), None, None);

        let joined = lines.join("\n");
        assert!(joined.contains("service: not installed"));
        assert!(joined.contains("shell hook: not installed"));
        assert!(joined.contains("wl-paste wrapper: not installed"));
    }
}
