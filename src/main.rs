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
use infra::clipboard::WlClipboard;
use infra::command_runner::RealCommandRunner;
use infra::file_system::RealFileWriter;
use infra::lifecycle::{DaemonLifecycle, FsDaemonLifecycle};
use infra::shell_installer::{FsShellInstaller, ShellInstaller};
use infra::systemd_installer::{FsSystemdInstaller, SystemdInstaller};
use service::converter::{ConvertService, SystemTimestamp};
use service::daemon::{self, PollResult};

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
    let mut has_error = false;

    // Shell hook installation (force=true for idempotent updates)
    let shell_installer = FsShellInstaller::new(home.clone());
    match shell_installer.install(shell, true) {
        Ok(path) => {
            eprintln!("installed: shell hook for {shell} at {path}");
        }
        Err(e) => {
            eprintln!("error: shell hook: {e}");
            has_error = true;
        }
    }

    // Systemd service installation
    if !args.no_service {
        match install_systemd_service(&home) {
            Ok(()) => {
                eprintln!("installed: systemd service");
            }
            Err(e) => {
                eprintln!("error: systemd service: {e}");
                has_error = true;
            }
        }
    } else {
        eprintln!("skipped: systemd service (--no-service)");
    }

    if has_error {
        process::exit(1);
    }
}

fn run_uninstall(args: cli::UninstallArgs) {
    let shell = resolve_shell(args.shell.as_deref());
    let home = resolve_home();
    let mut has_error = false;

    // Shell hook removal
    let shell_installer = FsShellInstaller::new(home.clone());
    match shell_installer.uninstall(shell) {
        Ok(path) => {
            eprintln!("uninstalled: shell hook for {shell} from {path}");
        }
        Err(e) => {
            eprintln!("error: shell hook: {e}");
            has_error = true;
        }
    }

    // Systemd service removal
    if !args.no_service {
        let runner = RealCommandRunner;
        let systemd_installer = FsSystemdInstaller::new(runner);
        match systemd_installer.uninstall(&home) {
            Ok(()) => {
                eprintln!("uninstalled: systemd service");
            }
            Err(e) => {
                eprintln!("error: systemd service: {e}");
                has_error = true;
            }
        }
    } else {
        eprintln!("skipped: systemd service (--no-service)");
    }

    if has_error {
        process::exit(1);
    }
}

/// Install the systemd user service.
fn install_systemd_service(home: &str) -> Result<(), String> {
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
    let unit_content = systemd_unit::generate_unit(&exec_path_str, uid);

    // Install via systemd installer
    let runner = RealCommandRunner;
    let installer = FsSystemdInstaller::new(runner);
    installer
        .install(&unit_content, home)
        .map_err(|e| e.to_string())
}

/// Resolve $HOME or exit.
fn resolve_home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| {
        eprintln!("error: $HOME is not set");
        process::exit(1);
    })
}

fn run_status() {
    // Placeholder — full implementation in Step 7
    eprintln!("status: not yet implemented");
    process::exit(1);
}

/// Resolve shell type from explicit name or $SHELL env var.
fn resolve_shell(name: Option<&str>) -> shell_detect::ShellType {
    match name {
        Some(n) => match shell_detect::parse_shell_name(n) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(1);
            }
        },
        None => {
            let shell_env = std::env::var("SHELL").unwrap_or_default();
            match shell_detect::detect_shell(&shell_env) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(1);
                }
            }
        }
    }
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
    let base_dir = if args.output_dir.as_os_str().is_empty() {
        runtime_dir.clone()
    } else {
        match domain::path_gen::validate_output_dir(&args.output_dir) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: invalid output directory: {e}");
                process::exit(1);
            }
        }
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
    } else {
        run_cleanup(&base_dir, args.max_files, verbosity);
        run_daemon(&service, &base_dir, args.interval_ms, verbosity);
    }
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

fn run_once<C, F, T, N>(
    service: &ConvertService<C, F, T, N>,
    base_dir: &Path,
    verbosity: Verbosity,
) where
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
    entries.sort_by(|a, b| b.age.cmp(&a.age));

    for path in cleanup::files_to_clean_by_count(&entries, max_files) {
        let _ = std::fs::remove_file(&path);
        log_verbose(verbosity, &format!("cleanup: removed {}", path.display()));
    }
}

fn run_daemon<C, F, T, N>(
    service: &ConvertService<C, F, T, N>,
    base_dir: &Path,
    interval_ms: u64,
    verbosity: Verbosity,
) where
    C: infra::clipboard::ClipboardReader,
    F: infra::file_system::FileWriter,
    T: service::converter::TimestampProvider,
    N: infra::path_notifier::PathNotifier,
{
    let poll_interval = Duration::from_millis(interval_ms);

    let mut previous_types: Vec<String> = Vec::new();

    log_info(
        verbosity,
        &format!(
            "daemon: watching clipboard (interval: {interval_ms}ms, output: {})",
            base_dir.display()
        ),
    );

    loop {
        let (result, new_types) = daemon::poll_once(service, &previous_types, base_dir);

        previous_types = new_types;

        match result {
            PollResult::Converted(path) => {
                log_info(verbosity, &format!("saved: {}", path.display()));
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

        thread::sleep(poll_interval);
    }
}
