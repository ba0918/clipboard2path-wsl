//! Shell detection logic (pure function).

use std::fmt;

/// Supported shell types.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShellType {
    Fish,
    Bash,
    Zsh,
}

impl fmt::Display for ShellType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShellType::Fish => write!(f, "fish"),
            ShellType::Bash => write!(f, "bash"),
            ShellType::Zsh => write!(f, "zsh"),
        }
    }
}

/// Error when shell cannot be detected.
#[derive(Debug, PartialEq)]
pub enum ShellDetectError {
    /// The shell is not supported.
    Unsupported(String),
}

impl fmt::Display for ShellDetectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShellDetectError::Unsupported(shell) => {
                write!(f, "unsupported shell: {shell} (supported: fish, bash, zsh)")
            }
        }
    }
}

/// Detect shell type from the `$SHELL` environment variable value.
///
/// Pure function: takes the shell path string, returns the detected type.
/// Extracts the basename from the path before matching.
pub fn detect_shell(shell_env: &str) -> Result<ShellType, ShellDetectError> {
    // Extract basename from path (e.g., "/usr/bin/fish" -> "fish")
    let basename = shell_env.rsplit('/').next().unwrap_or(shell_env);
    parse_shell_name(basename)
}

/// Parse a shell name string (e.g., from CLI argument) into ShellType.
pub fn parse_shell_name(name: &str) -> Result<ShellType, ShellDetectError> {
    match name {
        "fish" => Ok(ShellType::Fish),
        "bash" => Ok(ShellType::Bash),
        "zsh" => Ok(ShellType::Zsh),
        other => Err(ShellDetectError::Unsupported(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_fish() {
        assert_eq!(detect_shell("/usr/bin/fish"), Ok(ShellType::Fish));
    }

    #[test]
    fn detects_bash() {
        assert_eq!(detect_shell("/bin/bash"), Ok(ShellType::Bash));
    }

    #[test]
    fn detects_zsh() {
        assert_eq!(detect_shell("/usr/bin/zsh"), Ok(ShellType::Zsh));
    }

    #[test]
    fn detects_from_basename_only() {
        assert_eq!(detect_shell("fish"), Ok(ShellType::Fish));
    }

    #[test]
    fn rejects_unsupported_shell() {
        let result = detect_shell("/bin/sh");
        assert_eq!(result, Err(ShellDetectError::Unsupported("sh".to_string())));
    }

    #[test]
    fn parse_shell_name_works() {
        assert_eq!(parse_shell_name("fish"), Ok(ShellType::Fish));
        assert_eq!(parse_shell_name("bash"), Ok(ShellType::Bash));
        assert_eq!(parse_shell_name("zsh"), Ok(ShellType::Zsh));
        assert!(parse_shell_name("csh").is_err());
    }
}
