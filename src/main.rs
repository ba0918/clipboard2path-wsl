mod domain;
mod infra;
mod service;

use std::path::Path;
use std::process;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime};

use domain::cleanup::{self, FileEntry};
use domain::cli::{self, Command, Verbosity, WatchArgs};
use domain::runtime_dir;
use domain::shell_detect;
use domain::systemd_unit;
use domain::wsl_detect;
use infra::change_signal::{ChangeSignal, X11ChangeSignal};
use infra::clipboard::WlClipboard;
use infra::command_runner::RealCommandRunner;
use infra::file_system::RealFileWriter;
use infra::lifecycle::{DaemonLifecycle, FsDaemonLifecycle};
use infra::login_shell::fetch_login_shell;
use infra::shell_installer::FsShellInstaller;
use infra::systemd_installer::FsSystemdInstaller;
use infra::wrapper_installer::{FsWrapperInstaller, WrapperInstaller};
use service::converter::{ConvertService, SystemTimestamp};
use service::daemon::{self, PollResult};
use service::setup::SetupService;

fn main() {
    // Parse CLI arguments
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let command = match cli::parse_args(&raw_args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    match command {
        Command::Help => {
            println!("{}", cli::help_text());
        }
        Command::Version => {
            println!("{}", cli::version_text());
        }
        Command::Init(args) => run_init(args),
        Command::Uninstall(args) => run_uninstall(args),
        Command::Status => run_status(),
        Command::Watch(args) => run_watch(args),
    }
}

fn run_init(args: cli::InitArgs) {
    let shell = resolve_shell(args.shell.as_deref());
    let home = resolve_home();

    let shell_installer = FsShellInstaller::new(home.clone());
    let systemd_installer = FsSystemdInstaller::new(RealCommandRunner);
    let wrapper_installer = FsWrapperInstaller::new(home.clone());
    let setup = SetupService::new(
        &shell_installer,
        &systemd_installer,
        &wrapper_installer,
        home.clone(),
    );

    // Generate the systemd unit (env I/O) only when the service is enabled.
    let unit_content: Result<String, String> = if args.no_service {
        Ok(String::new())
    } else {
        generate_unit_content()
    };
    let unit_ref: Result<&str, &str> = match &unit_content {
        Ok(s) => Ok(s.as_str()),
        Err(e) => Err(e.as_str()),
    };

    let outcome = setup.run_init(shell, args.no_service, args.force, unit_ref);
    print_setup_outcome(&outcome);

    if !outcome.has_error {
        eprintln!();
        eprintln!("Next steps:");
        eprintln!("  1. Restart your shell (or run: exec $SHELL)");
        if !args.no_service {
            eprintln!(
                "  2. Verify: systemctl --user status {}",
                systemd_unit::SERVICE_NAME.trim_end_matches(".service")
            );
            // Show which wl-paste is resolved
            if let Some(resolved) = resolve_wl_paste_path() {
                eprintln!("  3. wl-paste resolves to: {resolved}");
            }
        }
    }

    if outcome.has_error {
        process::exit(1);
    }
}

fn run_uninstall(args: cli::UninstallArgs) {
    let shell = resolve_shell(args.shell.as_deref());
    let home = resolve_home();

    let shell_installer = FsShellInstaller::new(home.clone());
    let systemd_installer = FsSystemdInstaller::new(RealCommandRunner);
    let wrapper_installer = FsWrapperInstaller::new(home.clone());
    let setup = SetupService::new(
        &shell_installer,
        &systemd_installer,
        &wrapper_installer,
        home.clone(),
    );

    let outcome = setup.run_uninstall(shell, args.no_service);
    print_setup_outcome(&outcome);

    if outcome.has_error {
        process::exit(1);
    }
}

/// Print an init/uninstall outcome to stderr: errors first (as processing produced
/// them), then the result summary — the ordering the CLI has always used.
fn print_setup_outcome(outcome: &service::setup::SetupOutcome) {
    for line in &outcome.errors {
        eprintln!("{line}");
    }
    for line in &outcome.results {
        eprintln!("{line}");
    }
}

/// Generate the systemd unit file content from the running environment.
fn generate_unit_content() -> Result<String, String> {
    // Get binary path
    let exec_path = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("failed to resolve binary path: {e}"))?;
    let exec_path_str = exec_path.to_string_lossy();

    // Get UID from /proc/self/status
    let proc_status = std::fs::read_to_string("/proc/self/status")
        .map_err(|e| format!("failed to read /proc/self/status: {e}"))?;
    let uid = systemd_unit::parse_uid_from_proc_status(&proc_status)
        .map_err(|e| format!("failed to parse UID: {e}"))?;

    // Generate unit file content
    systemd_unit::generate_unit(&exec_path_str, uid)
        .map_err(|e| format!("binary path contains unsafe characters: {e}"))
}

/// Resolve the path that `which wl-paste` returns.
fn resolve_wl_paste_path() -> Option<String> {
    let output = std::process::Command::new("which")
        .arg("wl-paste")
        .output()
        .ok()?;
    let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if resolved.is_empty() {
        None
    } else {
        Some(resolved)
    }
}

/// Resolve $HOME or exit.
fn resolve_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| {
        eprintln!("error: $HOME is not set");
        process::exit(1);
    })
}

fn run_status() {
    let home = resolve_home();

    let shell_installer = FsShellInstaller::new(home.clone());
    let systemd_installer = FsSystemdInstaller::new(RealCommandRunner);
    let wrapper_installer = FsWrapperInstaller::new(home.clone());
    let setup = SetupService::new(
        &shell_installer,
        &systemd_installer,
        &wrapper_installer,
        home.clone(),
    );

    // Env I/O resolved here; SetupService only queries the installers.
    let shell = detect_shell_auto().ok();

    let wl_paste_resolved = if wrapper_installer.is_installed() {
        resolve_wl_paste_path()
    } else {
        None
    };

    let latest_image = read_latest_image();

    let lines = setup.run_status(shell, wl_paste_resolved.as_deref(), latest_image.as_deref());
    for line in &lines {
        println!("{line}");
    }
}

/// Read the latest saved image path from the runtime directory, if any.
fn read_latest_image() -> Option<String> {
    let xdg = std::env::var("XDG_RUNTIME_DIR").ok();
    let dir = runtime_dir::resolve_runtime_dir(xdg.as_deref()).ok()?;
    let content = std::fs::read_to_string(dir.join("latest-path")).ok()?;
    let path = content.trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}

/// Resolve shell type from an explicit name, or auto-detect ($SHELL + login shell).
fn resolve_shell(name: Option<&str>) -> shell_detect::ShellType {
    match name {
        Some(n) => match shell_detect::parse_shell_name(n) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
        },
        None => match detect_shell_auto() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
        },
    }
}

/// Auto-detect the shell from `$SHELL`, falling back to the login shell (`getent`).
///
/// Unifies init / uninstall / status on the same resolution rule.
fn detect_shell_auto() -> Result<shell_detect::ShellType, shell_detect::ShellDetectError> {
    let env_shell = std::env::var("SHELL").unwrap_or_default();
    let login = current_username().and_then(|user| fetch_login_shell(&RealCommandRunner, &user));
    shell_detect::resolve_shell_with_fallback(&env_shell, login.as_deref())
}

/// Determine the current username from the environment (for the `getent` lookup).
fn current_username() -> Option<String> {
    std::env::var("USER")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("LOGNAME").ok().filter(|s| !s.is_empty()))
}

fn run_watch(args: WatchArgs) {
    let verbosity = args.verbosity;

    // WSL2 check
    let proc_version = match std::fs::read_to_string("/proc/version") {
        Ok(content) => content,
        Err(e) => {
            eprintln!("error: failed to read /proc/version: {e}");
            process::exit(1);
        }
    };

    if !wsl_detect::is_wsl2(&proc_version) {
        eprintln!("error: this tool requires WSL2 environment");
        process::exit(1);
    }

    // Resolve runtime directory
    let xdg = std::env::var("XDG_RUNTIME_DIR").ok();
    let runtime_dir = match runtime_dir::resolve_runtime_dir(xdg.as_deref()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    // Determine output directory: explicit --output-dir overrides runtime_dir
    let base_dir = match args.output_dir {
        None => runtime_dir.clone(),
        Some(ref dir) => match infra::file_system::validate_output_dir(dir) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: invalid output directory: {e}");
                process::exit(1);
            }
        },
    };

    // Daemon lifecycle: setup directory
    let lifecycle = FsDaemonLifecycle;
    if let Err(e) = lifecycle.setup(&base_dir) {
        eprintln!("error: {e}");
        process::exit(1);
    }

    // SIGTERM/SIGINT handler: teardown on shutdown
    let teardown_dir = Arc::new(base_dir.clone());
    {
        let dir = Arc::clone(&teardown_dir);
        ctrlc::set_handler(move || {
            let lc = FsDaemonLifecycle;
            let _ = lc.teardown(&dir);
            process::exit(0);
        })
        .expect("failed to set signal handler");
    }

    // DI assembly
    let notifier = infra::path_notifier::FilePathNotifier::new(base_dir.clone());
    let service = ConvertService::new(WlClipboard, RealFileWriter, SystemTimestamp, notifier);

    if args.once {
        run_once(&service, &base_dir, verbosity);
        return;
    }

    run_cleanup(&base_dir, args.max_files, verbosity);

    let mut seed_baseline = false;
    if !args.poll {
        match X11ChangeSignal::connect() {
            Ok(mut signal) => {
                run_event_daemon(&service, &mut signal, &base_dir, args.max_files, verbosity);
                // run_event_daemon returns only when the X11 connection is lost
                // (e.g. XWayland restart) — degrade to polling instead of dying.
                log_info(
                    verbosity,
                    "daemon: event signal lost, falling back to polling",
                );
                // The event path already converted the current clipboard;
                // without a baseline, polling's first pass would save it again.
                seed_baseline = true;
            }
            Err(e) => {
                log_info(
                    verbosity,
                    &format!("daemon: X11 event mode unavailable ({e}), falling back to polling"),
                );
            }
        }
    }

    run_daemon(
        &service,
        &base_dir,
        args.interval_ms,
        args.max_files,
        verbosity,
        seed_baseline,
    );
}

/// Log a message at Normal+ verbosity.
fn log_info(verbosity: Verbosity, msg: &str) {
    if verbosity != Verbosity::Quiet {
        eprintln!("{msg}");
    }
}

/// Log a message at Verbose verbosity only.
fn log_verbose(verbosity: Verbosity, msg: &str) {
    if verbosity == Verbosity::Verbose {
        eprintln!("{msg}");
    }
}

fn run_once<C, F, T, N>(service: &ConvertService<C, F, T, N>, base_dir: &Path, verbosity: Verbosity)
where
    C: infra::clipboard::ClipboardReader,
    F: infra::file_system::FileWriter,
    T: service::converter::TimestampProvider,
    N: infra::path_notifier::PathNotifier,
{
    match service.convert_once(base_dir) {
        Ok(path) => {
            log_info(verbosity, &format!("saved: {}", path.display()));
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

fn run_cleanup(base_dir: &Path, max_files: usize, verbosity: Verbosity) {
    // Collect file entries
    let mut entries: Vec<FileEntry> = match std::fs::read_dir(base_dir) {
        Ok(dir) => dir
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(cleanup::is_clipboard_png)
            })
            .filter_map(|e| {
                let metadata = e.metadata().ok()?;
                let age = SystemTime::now()
                    .duration_since(metadata.modified().ok()?)
                    .ok()?;
                Some(FileEntry {
                    path: e.path(),
                    age,
                })
            })
            .collect(),
        Err(_) => return,
    };

    // Sort oldest first, then remove excess by count
    entries.sort_by_key(|e| std::cmp::Reverse(e.age));

    for path in cleanup::files_to_clean_by_count(&entries, max_files) {
        let _ = std::fs::remove_file(&path);
        log_verbose(verbosity, &format!("cleanup: removed {}", path.display()));
    }
}

/// Run the polling loop. With `seed_baseline`, the current clipboard state is
/// treated as already processed (used when taking over from the event path).
fn run_daemon<C, F, T, N>(
    service: &ConvertService<C, F, T, N>,
    base_dir: &Path,
    interval_ms: u64,
    max_files: usize,
    verbosity: Verbosity,
    seed_baseline: bool,
) where
    C: infra::clipboard::ClipboardReader,
    F: infra::file_system::FileWriter,
    T: service::converter::TimestampProvider,
    N: infra::path_notifier::PathNotifier,
{
    let poll_interval = Duration::from_millis(interval_ms);

    let mut previous_types: Vec<String> = if seed_baseline {
        service.reader().list_types().unwrap_or_default()
    } else {
        Vec::new()
    };

    log_info(
        verbosity,
        &format!(
            "daemon: watching clipboard (interval: {interval_ms}ms, output: {})",
            base_dir.display()
        ),
    );

    loop {
        let result = daemon::poll_once(service, &mut previous_types, base_dir);
        report_poll_result(result, base_dir, max_files, verbosity);
        thread::sleep(poll_interval);
    }
}

fn run_event_daemon<C, F, T, N, S>(
    service: &ConvertService<C, F, T, N>,
    signal: &mut S,
    base_dir: &Path,
    max_files: usize,
    verbosity: Verbosity,
) where
    C: infra::clipboard::ClipboardReader,
    F: infra::file_system::FileWriter,
    T: service::converter::TimestampProvider,
    N: infra::path_notifier::PathNotifier,
    S: ChangeSignal,
{
    log_info(
        verbosity,
        &format!(
            "daemon: watching clipboard (event-driven via XFixes, output: {})",
            base_dir.display()
        ),
    );

    // An image copied before startup has no future event to announce it —
    // process the current clipboard state once.
    let initial = daemon::on_change_event(service, base_dir);
    report_poll_result(initial, base_dir, max_files, verbosity);

    loop {
        match signal.wait_change() {
            Ok(()) => {
                let result = daemon::on_change_event(service, base_dir);
                report_poll_result(result, base_dir, max_files, verbosity);
            }
            Err(e) => {
                eprintln!("event signal error: {e}");
                return;
            }
        }
    }
}

/// Log a poll result and run cleanup after successful conversions.
fn report_poll_result(result: PollResult, base_dir: &Path, max_files: usize, verbosity: Verbosity) {
    match result {
        PollResult::Converted(path) => {
            log_info(verbosity, &format!("saved: {}", path.display()));
            run_cleanup(base_dir, max_files, verbosity);
        }
        PollResult::ConvertError(e) => {
            eprintln!("error: {e}");
        }
        PollResult::ClipboardError(e) => {
            eprintln!("clipboard error: {e}");
        }
        PollResult::NoBmpImage => {
            log_verbose(verbosity, "skipped: no BMP image in clipboard");
        }
        PollResult::NoChange => {}
    }
}
