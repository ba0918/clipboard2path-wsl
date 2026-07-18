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

/// Resolve the shell type from `$SHELL`, falling back to the login shell.
///
/// `$SHELL` is authoritative: it reflects the shell the user is actually running.
/// The login shell (from `getent passwd`) is only a fallback for when `$SHELL` is
/// empty or names an unsupported shell — it never overrides a valid `$SHELL`, even
/// if the two disagree. Returns an error only when neither resolves to a supported
/// shell.
pub fn resolve_shell_with_fallback(
    env_shell: &str,
    login_shell: Option<&str>,
) -> Result<ShellType, ShellDetectError> {
    match detect_shell(env_shell) {
        Ok(shell) => Ok(shell),
        Err(env_err) => match login_shell.and_then(|login| detect_shell(login).ok()) {
            Some(shell) => Ok(shell),
            None => Err(env_err),
        },
    }
}

/// Extract the login shell from a `getent passwd` line.
///
/// The line is `name:passwd:uid:gid:gecos:home:shell`; the shell is the 7th field.
/// Returns `None` when the line is empty, has fewer than 7 fields, or names a
/// non-interactive shell (`nologin`/`false`) — i.e. "no usable login shell".
pub fn parse_login_shell(getent_line: &str) -> Option<String> {
    let line = getent_line.lines().next().unwrap_or("");
    let shell = line.split(':').nth(6)?.trim();
    if shell.is_empty() {
        return None;
    }
    let basename = shell.rsplit('/').next().unwrap_or(shell);
    if basename == "nologin" || basename == "false" {
        return None;
    }
    Some(shell.to_string())
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

    #[test]
    fn fallback_env_valid_login_valid_matching_returns_env() {
        let result = resolve_shell_with_fallback("/usr/bin/fish", Some("/usr/bin/fish"));
        assert_eq!(result, Ok(ShellType::Fish));
    }

    #[test]
    fn fallback_env_valid_login_valid_differ_prefers_env() {
        // $SHELL reflects the user's current choice; login must not override it.
        let result = resolve_shell_with_fallback("/usr/bin/fish", Some("/bin/bash"));
        assert_eq!(result, Ok(ShellType::Fish));
    }

    #[test]
    fn fallback_env_valid_login_none_returns_env() {
        let result = resolve_shell_with_fallback("/usr/bin/zsh", None);
        assert_eq!(result, Ok(ShellType::Zsh));
    }

    #[test]
    fn fallback_env_unsupported_login_valid_uses_login() {
        let result = resolve_shell_with_fallback("/bin/sh", Some("/usr/bin/fish"));
        assert_eq!(result, Ok(ShellType::Fish));
    }

    #[test]
    fn fallback_env_empty_login_valid_uses_login() {
        let result = resolve_shell_with_fallback("", Some("/bin/bash"));
        assert_eq!(result, Ok(ShellType::Bash));
    }

    #[test]
    fn fallback_env_unsupported_login_none_is_error() {
        let result = resolve_shell_with_fallback("/bin/sh", None);
        assert!(result.is_err());
    }

    #[test]
    fn fallback_env_empty_login_unsupported_is_error() {
        let result = resolve_shell_with_fallback("", Some("/usr/sbin/nologin"));
        assert!(result.is_err());
    }

    #[test]
    fn parse_login_shell_extracts_seventh_field() {
        let line = "user:x:1000:1000:User:/home/user:/usr/bin/fish";
        assert_eq!(parse_login_shell(line), Some("/usr/bin/fish".to_string()));
    }

    #[test]
    fn parse_login_shell_empty_output_is_none() {
        assert_eq!(parse_login_shell(""), None);
    }

    #[test]
    fn parse_login_shell_insufficient_fields_is_none() {
        // Missing the shell field (only 6 fields).
        let line = "user:x:1000:1000:User:/home/user";
        assert_eq!(parse_login_shell(line), None);
    }

    #[test]
    fn parse_login_shell_nologin_is_none() {
        let line = "user:x:1000:1000:User:/home/user:/usr/sbin/nologin";
        assert_eq!(parse_login_shell(line), None);
    }

    #[test]
    fn parse_login_shell_false_is_none() {
        let line = "user:x:1000:1000:User:/home/user:/bin/false";
        assert_eq!(parse_login_shell(line), None);
    }
}
