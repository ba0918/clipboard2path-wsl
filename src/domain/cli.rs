//! CLI argument parsing (pure functions).

use std::fmt;
use std::path::PathBuf;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Minimum polling interval in milliseconds.
pub const MIN_INTERVAL_MS: u64 = 100;
/// Maximum polling interval in milliseconds.
pub const MAX_INTERVAL_MS: u64 = 60_000;
/// Default polling interval in milliseconds.
pub const DEFAULT_INTERVAL_MS: u64 = 500;
/// Minimum number of files to keep.
pub const MIN_MAX_FILES: usize = 1;
/// Default number of files to keep.
pub const DEFAULT_MAX_FILES: usize = 20;

/// Verbosity level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verbosity {
    Quiet,
    Normal,
    Verbose,
}

/// Top-level parsed command.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Watch clipboard (daemon or single-shot mode). Default when no subcommand given.
    Watch(WatchArgs),
    /// Install shell hook.
    Init(InitArgs),
    /// Remove shell hook.
    Uninstall(UninstallArgs),
    /// Show current status.
    Status,
    /// Show help.
    Help,
    /// Show version.
    Version,
}

/// Arguments for the watch (daemon) subcommand.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchArgs {
    /// Run in single-shot mode (no daemon loop).
    pub once: bool,
    /// Polling interval in milliseconds.
    pub interval_ms: u64,
    /// Output directory for saved PNGs (None = use runtime dir).
    pub output_dir: Option<PathBuf>,
    /// Maximum number of files to keep.
    pub max_files: usize,
    /// Verbosity level.
    pub verbosity: Verbosity,
}

impl Default for WatchArgs {
    fn default() -> Self {
        Self {
            once: false,
            interval_ms: DEFAULT_INTERVAL_MS,
            output_dir: None,
            max_files: DEFAULT_MAX_FILES,
            verbosity: Verbosity::Normal,
        }
    }
}

/// Arguments for the init subcommand.
#[derive(Debug, Clone, PartialEq)]
pub struct InitArgs {
    /// Optional shell name override (auto-detect from $SHELL if None).
    pub shell: Option<String>,
    /// Force overwrite existing hook.
    pub force: bool,
    /// Skip systemd service installation.
    pub no_service: bool,
}

/// Arguments for the uninstall subcommand.
#[derive(Debug, Clone, PartialEq)]
pub struct UninstallArgs {
    /// Optional shell name override (auto-detect from $SHELL if None).
    pub shell: Option<String>,
    /// Skip systemd service removal.
    pub no_service: bool,
}

/// CLI parsing error.
#[derive(Debug, PartialEq)]
pub enum CliError {
    MissingValue(String),
    InvalidValue {
        flag: String,
        value: String,
    },
    UnknownFlag(String),
    OutOfRange {
        flag: String,
        min: String,
        max: String,
        actual: String,
    },
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::MissingValue(flag) => write!(f, "missing value for {flag}"),
            CliError::InvalidValue { flag, value } => {
                write!(f, "invalid value '{value}' for {flag}")
            }
            CliError::UnknownFlag(flag) => write!(f, "unknown flag: {flag}"),
            CliError::OutOfRange {
                flag,
                min,
                max,
                actual,
            } => {
                write!(f, "{flag} must be between {min} and {max} (got {actual})")
            }
        }
    }
}

/// Parse CLI arguments from a string slice.
///
/// Pure function: takes args (program name already skipped), returns parsed command.
pub fn parse_args(args: &[String]) -> Result<Command, CliError> {
    if args.is_empty() {
        return Ok(Command::Watch(WatchArgs::default()));
    }

    // Check for global flags first
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Command::Help);
    }
    if args.iter().any(|a| a == "--version" || a == "-v") {
        return Ok(Command::Version);
    }

    // Check for subcommands
    match args[0].as_str() {
        "init" => parse_init_args(&args[1..]),
        "uninstall" => parse_uninstall_args(&args[1..]),
        "status" => Ok(Command::Status),
        _ => parse_watch_args(args),
    }
}

fn parse_watch_args(args: &[String]) -> Result<Command, CliError> {
    let mut result = WatchArgs::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--once" => result.once = true,
            "--interval" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| CliError::MissingValue("--interval".to_string()))?;
                result.interval_ms = val.parse().map_err(|_| CliError::InvalidValue {
                    flag: "--interval".to_string(),
                    value: val.clone(),
                })?;
            }
            "--output-dir" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| CliError::MissingValue("--output-dir".to_string()))?;
                result.output_dir = Some(PathBuf::from(val));
            }
            "--max-files" => {
                i += 1;
                let val = args
                    .get(i)
                    .ok_or_else(|| CliError::MissingValue("--max-files".to_string()))?;
                result.max_files = val.parse().map_err(|_| CliError::InvalidValue {
                    flag: "--max-files".to_string(),
                    value: val.clone(),
                })?;
            }
            "--verbose" => result.verbosity = Verbosity::Verbose,
            "--quiet" | "-q" => result.verbosity = Verbosity::Quiet,
            other => return Err(CliError::UnknownFlag(other.to_string())),
        }
        i += 1;
    }

    // Range validation
    if result.interval_ms < MIN_INTERVAL_MS || result.interval_ms > MAX_INTERVAL_MS {
        return Err(CliError::OutOfRange {
            flag: "--interval".to_string(),
            min: MIN_INTERVAL_MS.to_string(),
            max: MAX_INTERVAL_MS.to_string(),
            actual: result.interval_ms.to_string(),
        });
    }

    if result.max_files < MIN_MAX_FILES {
        return Err(CliError::OutOfRange {
            flag: "--max-files".to_string(),
            min: MIN_MAX_FILES.to_string(),
            max: "unlimited".to_string(),
            actual: result.max_files.to_string(),
        });
    }

    Ok(Command::Watch(result))
}

fn parse_init_args(args: &[String]) -> Result<Command, CliError> {
    let mut shell = None;
    let mut force = false;
    let mut no_service = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--force" | "-f" => force = true,
            "--no-service" => no_service = true,
            other if !other.starts_with('-') && shell.is_none() => {
                shell = Some(other.to_string());
            }
            other => return Err(CliError::UnknownFlag(other.to_string())),
        }
        i += 1;
    }

    Ok(Command::Init(InitArgs {
        shell,
        force,
        no_service,
    }))
}

fn parse_uninstall_args(args: &[String]) -> Result<Command, CliError> {
    let mut shell = None;
    let mut no_service = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--no-service" => no_service = true,
            other if !other.starts_with('-') && shell.is_none() => {
                shell = Some(other.to_string());
            }
            other => return Err(CliError::UnknownFlag(other.to_string())),
        }
        i += 1;
    }

    Ok(Command::Uninstall(UninstallArgs { shell, no_service }))
}

/// Generate help text.
pub fn help_text() -> String {
    format!(
        "clipboard2path-wsl {VERSION}
WSL2 clipboard image to file path converter

USAGE:
    clipboard2path-wsl [COMMAND] [OPTIONS]

COMMANDS:
    (default)           Watch clipboard and convert images (daemon mode)
    init [SHELL]        Install shell hook and systemd service
    uninstall [SHELL]   Remove shell hook and systemd service
    status              Show current status

WATCH OPTIONS:
    --once              Run once and exit (no daemon loop)
    --interval <ms>     Polling interval in ms (100-60000, default: {DEFAULT_INTERVAL_MS})
    --output-dir <path> Output directory (default: $XDG_RUNTIME_DIR/clipboard2path)
    --max-files <n>     Maximum files to keep (min: {MIN_MAX_FILES}, default: {DEFAULT_MAX_FILES})
    --verbose           Show detailed output
    -q, --quiet         Suppress all non-error output

INIT OPTIONS:
    -f, --force         Force overwrite existing hook
    --no-service        Skip systemd service installation

UNINSTALL OPTIONS:
    --no-service        Skip systemd service removal

GLOBAL OPTIONS:
    -h, --help          Show this help
    -v, --version       Show version"
    )
}

/// Generate version text.
pub fn version_text() -> String {
    format!("clipboard2path-wsl {VERSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    // --- Watch (default) subcommand ---

    #[test]
    fn default_args_returns_watch() {
        let result = parse_args(&[]).unwrap();
        let Command::Watch(w) = result else {
            panic!("expected Watch");
        };
        assert!(!w.once);
        assert_eq!(w.interval_ms, 500);
        assert_eq!(w.output_dir, None);
        assert_eq!(w.max_files, 20);
    }

    #[test]
    fn once_flag() {
        let Command::Watch(w) = parse_args(&args(&["--once"])).unwrap() else {
            panic!("expected Watch");
        };
        assert!(w.once);
    }

    #[test]
    fn interval_flag() {
        let Command::Watch(w) = parse_args(&args(&["--interval", "1000"])).unwrap() else {
            panic!("expected Watch");
        };
        assert_eq!(w.interval_ms, 1000);
    }

    #[test]
    fn output_dir_flag() {
        let Command::Watch(w) = parse_args(&args(&["--output-dir", "/home/user/images"])).unwrap()
        else {
            panic!("expected Watch");
        };
        assert_eq!(w.output_dir, Some(PathBuf::from("/home/user/images")));
    }

    #[test]
    fn max_files_flag() {
        let Command::Watch(w) = parse_args(&args(&["--max-files", "50"])).unwrap() else {
            panic!("expected Watch");
        };
        assert_eq!(w.max_files, 50);
    }

    #[test]
    fn combined_watch_flags() {
        let Command::Watch(w) =
            parse_args(&args(&["--once", "--interval", "200", "--max-files", "10"])).unwrap()
        else {
            panic!("expected Watch");
        };
        assert!(w.once);
        assert_eq!(w.interval_ms, 200);
        assert_eq!(w.max_files, 10);
    }

    #[test]
    fn verbose_flag() {
        let Command::Watch(w) = parse_args(&args(&["--verbose"])).unwrap() else {
            panic!("expected Watch");
        };
        assert_eq!(w.verbosity, Verbosity::Verbose);
    }

    #[test]
    fn quiet_flag() {
        let Command::Watch(w) = parse_args(&args(&["-q"])).unwrap() else {
            panic!("expected Watch");
        };
        assert_eq!(w.verbosity, Verbosity::Quiet);
    }

    #[test]
    fn missing_interval_value() {
        let result = parse_args(&args(&["--interval"]));
        assert_eq!(
            result,
            Err(CliError::MissingValue("--interval".to_string()))
        );
    }

    #[test]
    fn invalid_interval_value() {
        let result = parse_args(&args(&["--interval", "abc"]));
        assert_eq!(
            result,
            Err(CliError::InvalidValue {
                flag: "--interval".to_string(),
                value: "abc".to_string(),
            })
        );
    }

    #[test]
    fn unknown_flag() {
        let result = parse_args(&args(&["--unknown"]));
        assert_eq!(result, Err(CliError::UnknownFlag("--unknown".to_string())));
    }

    // --- Range validation ---

    #[test]
    fn interval_zero_rejected() {
        let result = parse_args(&args(&["--interval", "0"]));
        assert!(matches!(
            result,
            Err(CliError::OutOfRange { flag, .. }) if flag == "--interval"
        ));
    }

    #[test]
    fn interval_below_min_rejected() {
        let result = parse_args(&args(&["--interval", "99"]));
        assert!(matches!(
            result,
            Err(CliError::OutOfRange { flag, .. }) if flag == "--interval"
        ));
    }

    #[test]
    fn interval_at_min_accepted() {
        let Command::Watch(w) = parse_args(&args(&["--interval", "100"])).unwrap() else {
            panic!("expected Watch");
        };
        assert_eq!(w.interval_ms, 100);
    }

    #[test]
    fn interval_above_max_rejected() {
        let result = parse_args(&args(&["--interval", "60001"]));
        assert!(matches!(
            result,
            Err(CliError::OutOfRange { flag, .. }) if flag == "--interval"
        ));
    }

    #[test]
    fn interval_at_max_accepted() {
        let Command::Watch(w) = parse_args(&args(&["--interval", "60000"])).unwrap() else {
            panic!("expected Watch");
        };
        assert_eq!(w.interval_ms, 60000);
    }

    #[test]
    fn max_files_zero_rejected() {
        let result = parse_args(&args(&["--max-files", "0"]));
        assert!(matches!(
            result,
            Err(CliError::OutOfRange { flag, .. }) if flag == "--max-files"
        ));
    }

    #[test]
    fn max_files_one_accepted() {
        let Command::Watch(w) = parse_args(&args(&["--max-files", "1"])).unwrap() else {
            panic!("expected Watch");
        };
        assert_eq!(w.max_files, 1);
    }

    // --- Help / Version ---

    #[test]
    fn help_flag() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Command::Help);
    }

    #[test]
    fn help_flag_short() {
        assert_eq!(parse_args(&args(&["-h"])).unwrap(), Command::Help);
    }

    #[test]
    fn version_flag() {
        assert_eq!(parse_args(&args(&["-v"])).unwrap(), Command::Version);
    }

    // --- Init subcommand ---

    #[test]
    fn init_no_args() {
        let Command::Init(a) = parse_args(&args(&["init"])).unwrap() else {
            panic!("expected Init");
        };
        assert_eq!(a.shell, None);
        assert!(!a.force);
        assert!(!a.no_service);
    }

    #[test]
    fn init_with_shell() {
        let Command::Init(a) = parse_args(&args(&["init", "fish"])).unwrap() else {
            panic!("expected Init");
        };
        assert_eq!(a.shell, Some("fish".to_string()));
    }

    #[test]
    fn init_with_force() {
        let Command::Init(a) = parse_args(&args(&["init", "--force"])).unwrap() else {
            panic!("expected Init");
        };
        assert!(a.force);
    }

    #[test]
    fn init_with_shell_and_force() {
        let Command::Init(a) = parse_args(&args(&["init", "bash", "-f"])).unwrap() else {
            panic!("expected Init");
        };
        assert_eq!(a.shell, Some("bash".to_string()));
        assert!(a.force);
    }

    #[test]
    fn init_no_service_flag() {
        let Command::Init(a) = parse_args(&args(&["init", "--no-service"])).unwrap() else {
            panic!("expected Init");
        };
        assert!(a.no_service);
        assert!(!a.force);
    }

    #[test]
    fn init_no_service_with_shell() {
        let Command::Init(a) =
            parse_args(&args(&["init", "fish", "--no-service", "--force"])).unwrap()
        else {
            panic!("expected Init");
        };
        assert_eq!(a.shell, Some("fish".to_string()));
        assert!(a.no_service);
        assert!(a.force);
    }

    // --- Uninstall subcommand ---

    #[test]
    fn uninstall_no_args() {
        let Command::Uninstall(a) = parse_args(&args(&["uninstall"])).unwrap() else {
            panic!("expected Uninstall");
        };
        assert_eq!(a.shell, None);
        assert!(!a.no_service);
    }

    #[test]
    fn uninstall_with_shell() {
        let Command::Uninstall(a) = parse_args(&args(&["uninstall", "zsh"])).unwrap() else {
            panic!("expected Uninstall");
        };
        assert_eq!(a.shell, Some("zsh".to_string()));
    }

    #[test]
    fn uninstall_no_service_flag() {
        let Command::Uninstall(a) = parse_args(&args(&["uninstall", "--no-service"])).unwrap()
        else {
            panic!("expected Uninstall");
        };
        assert!(a.no_service);
    }

    // --- Status subcommand ---

    #[test]
    fn status_command() {
        assert_eq!(parse_args(&args(&["status"])).unwrap(), Command::Status);
    }

    // --- Help text ---

    #[test]
    fn help_text_contains_status() {
        let help = help_text();
        assert!(help.contains("status"));
    }

    #[test]
    fn help_text_contains_no_service() {
        let help = help_text();
        assert!(help.contains("--no-service"));
    }
}
