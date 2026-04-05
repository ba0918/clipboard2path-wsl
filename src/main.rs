mod domain;
mod infra;
mod service;

use std::path::Path;
use std::process;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use domain::cleanup::{self, FileEntry};
use domain::cli::{self, Verbosity};
use domain::wsl_detect;
use infra::clipboard::WlClipboard;
use infra::file_system::RealFileWriter;
use service::converter::{ConvertService, SystemTimestamp};
use service::daemon::{self, PollResult};

fn main() {
    // Parse CLI arguments
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let args = match cli::parse_args(&raw_args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    if args.help {
        println!("{}", cli::help_text());
        return;
    }

    if args.version {
        println!("{}", cli::version_text());
        return;
    }

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

    // Validate output directory
    let base_dir = match domain::path_gen::validate_output_dir(&args.output_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: invalid output directory: {e}");
            process::exit(1);
        }
    };

    // DI assembly
    let service = ConvertService::new(WlClipboard, WlClipboard, RealFileWriter, SystemTimestamp);

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

fn run_once<C, W, F, T>(service: &ConvertService<C, W, F, T>, base_dir: &Path, verbosity: Verbosity)
where
    C: infra::clipboard::ClipboardReader,
    W: infra::clipboard::ClipboardWriter,
    F: infra::file_system::FileWriter,
    T: service::converter::TimestampProvider,
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
    let max_age = Duration::from_secs(86400); // 24 hours

    // Collect file entries
    let entries: Vec<FileEntry> = match std::fs::read_dir(base_dir) {
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

    // Clean by age
    for path in cleanup::files_to_clean_by_age(&entries, max_age) {
        let _ = std::fs::remove_file(&path);
        log_verbose(verbosity, &format!("cleanup: removed {}", path.display()));
    }

    // Re-collect remaining entries for count-based cleanup
    let mut remaining: Vec<FileEntry> = entries.into_iter().filter(|e| e.age <= max_age).collect();
    remaining.sort_by(|a, b| b.age.cmp(&a.age)); // oldest first

    for path in cleanup::files_to_clean_by_count(&remaining, max_files) {
        let _ = std::fs::remove_file(&path);
        log_verbose(verbosity, &format!("cleanup: removed {}", path.display()));
    }
}

fn run_daemon<C, W, F, T>(
    service: &ConvertService<C, W, F, T>,
    base_dir: &Path,
    interval_ms: u64,
    verbosity: Verbosity,
) where
    C: infra::clipboard::ClipboardReader,
    W: infra::clipboard::ClipboardWriter,
    F: infra::file_system::FileWriter,
    T: service::converter::TimestampProvider,
{
    let debounce_ms: u64 = 1000;
    let poll_interval = Duration::from_millis(interval_ms);
    let epoch = Instant::now();

    let mut previous_types: Vec<String> = Vec::new();
    let mut last_write_ms: Option<u64> = None;

    log_info(
        verbosity,
        &format!(
            "daemon: watching clipboard (interval: {interval_ms}ms, output: {})",
            base_dir.display()
        ),
    );

    loop {
        let current_ms = epoch.elapsed().as_millis() as u64;

        let (result, new_types) = daemon::poll_once(
            service,
            &previous_types,
            last_write_ms,
            current_ms,
            debounce_ms,
            base_dir,
        );

        previous_types = new_types;

        match result {
            PollResult::Converted(path) => {
                last_write_ms = Some(epoch.elapsed().as_millis() as u64);
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
            PollResult::Debounced => {
                log_verbose(verbosity, "skipped: debounced");
            }
        }

        thread::sleep(poll_interval);
    }
}
