//! CLI argument parsing (pure functions).

use std::fmt;
use std::path::PathBuf;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Parsed CLI arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct CliArgs {
    /// Run in single-shot mode (no daemon loop).
    pub once: bool,
    /// Polling interval in milliseconds.
    pub interval_ms: u64,
    /// Output directory for saved PNGs.
    pub output_dir: PathBuf,
    /// Maximum number of files to keep.
    pub max_files: usize,
    /// Show help.
    pub help: bool,
    /// Show version.
    pub version: bool,
}

impl Default for CliArgs {
    fn default() -> Self {
        Self {
            once: false,
            interval_ms: 500,
            output_dir: PathBuf::from("/tmp"),
            max_files: 100,
            help: false,
            version: false,
        }
    }
}

/// CLI parsing error.
#[derive(Debug, PartialEq)]
pub enum CliError {
    MissingValue(String),
    InvalidValue { flag: String, value: String },
    UnknownFlag(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::MissingValue(flag) => write!(f, "missing value for {flag}"),
            CliError::InvalidValue { flag, value } => {
                write!(f, "invalid value '{value}' for {flag}")
            }
            CliError::UnknownFlag(flag) => write!(f, "unknown flag: {flag}"),
        }
    }
}

/// Parse CLI arguments from a string iterator.
///
/// Pure function: takes args iterator, returns parsed config or error.
/// The first argument (program name) should already be skipped.
pub fn parse_args(args: &[String]) -> Result<CliArgs, CliError> {
    let mut result = CliArgs::default();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--once" => result.once = true,
            "--help" | "-h" => result.help = true,
            "--version" | "-v" => result.version = true,
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
                result.output_dir = PathBuf::from(val);
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
            other => return Err(CliError::UnknownFlag(other.to_string())),
        }
        i += 1;
    }

    Ok(result)
}

/// Generate help text.
pub fn help_text() -> String {
    format!(
        "clipboard2path-wsl {VERSION}
WSL2 clipboard image to file path converter

USAGE:
    clipboard2path-wsl [OPTIONS]

OPTIONS:
    --once              Run once and exit (no daemon loop)
    --interval <ms>     Polling interval in ms (default: 500)
    --output-dir <path> Output directory (default: /tmp)
    --max-files <n>     Maximum files to keep (default: 100)
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

    #[test]
    fn default_args() {
        let result = parse_args(&[]).unwrap();
        assert!(!result.once);
        assert_eq!(result.interval_ms, 500);
        assert_eq!(result.output_dir, PathBuf::from("/tmp"));
        assert_eq!(result.max_files, 100);
    }

    #[test]
    fn once_flag() {
        let result = parse_args(&args(&["--once"])).unwrap();
        assert!(result.once);
    }

    #[test]
    fn interval_flag() {
        let result = parse_args(&args(&["--interval", "1000"])).unwrap();
        assert_eq!(result.interval_ms, 1000);
    }

    #[test]
    fn output_dir_flag() {
        let result = parse_args(&args(&["--output-dir", "/home/user/images"])).unwrap();
        assert_eq!(result.output_dir, PathBuf::from("/home/user/images"));
    }

    #[test]
    fn max_files_flag() {
        let result = parse_args(&args(&["--max-files", "50"])).unwrap();
        assert_eq!(result.max_files, 50);
    }

    #[test]
    fn help_flag() {
        let result = parse_args(&args(&["--help"])).unwrap();
        assert!(result.help);
    }

    #[test]
    fn version_flag() {
        let result = parse_args(&args(&["-v"])).unwrap();
        assert!(result.version);
    }

    #[test]
    fn combined_flags() {
        let result =
            parse_args(&args(&["--once", "--interval", "200", "--max-files", "10"])).unwrap();
        assert!(result.once);
        assert_eq!(result.interval_ms, 200);
        assert_eq!(result.max_files, 10);
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
}
