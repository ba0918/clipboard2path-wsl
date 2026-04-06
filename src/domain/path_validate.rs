//! Path validation for safe embedding in shell scripts and systemd units (pure functions).

use std::fmt;

/// Error type for path validation.
#[derive(Debug, PartialEq)]
pub enum PathValidateError {
    /// Path contains unsafe characters for embedding.
    UnsafeChar {
        path: String,
        ch: char,
        description: String,
    },
}

impl fmt::Display for PathValidateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathValidateError::UnsafeChar {
                path, description, ..
            } => {
                write!(f, "path contains unsafe character ({description}): {path}")
            }
        }
    }
}

/// Validate that a path is safe for embedding in shell scripts and systemd unit files.
///
/// Rejects paths containing:
/// - Shell special characters: `"`, `$`, `` ` ``, `\`, `'`
/// - Systemd specifiers: `%`
/// - Control characters: 0x00-0x1F (including `\n`, `\r`, `\0`)
///
/// Uses a denylist approach because Linux paths can contain a wide range of
/// valid UTF-8 characters, and an allowlist would unnecessarily restrict
/// legitimate paths.
pub fn validate_safe_path(path: &str) -> Result<(), PathValidateError> {
    for ch in path.chars() {
        let description = match ch {
            '"' => Some("double quote"),
            '$' => Some("dollar sign"),
            '`' => Some("backtick"),
            '\\' => Some("backslash"),
            '\'' => Some("single quote"),
            '%' => Some("percent (systemd specifier)"),
            c if c.is_ascii_control() => Some("control character"),
            _ => None,
        };

        if let Some(desc) = description {
            return Err(PathValidateError::UnsafeChar {
                path: path.to_string(),
                ch,
                description: desc.to_string(),
            });
        }
    }

    Ok(())
}

/// Boolean version of `validate_safe_path`.
pub fn is_safe_for_shell(path: &str) -> bool {
    validate_safe_path(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- validate_safe_path: normal paths ---

    #[test]
    fn accepts_normal_path() {
        assert!(validate_safe_path("/usr/local/bin/clipboard2path-wsl").is_ok());
    }

    #[test]
    fn accepts_path_with_dots_and_underscores() {
        assert!(validate_safe_path("/home/user/.config/my_tool/v1.0").is_ok());
    }

    #[test]
    fn accepts_path_with_tilde() {
        // tilde is safe in quoted contexts
        assert!(validate_safe_path("/home/user/~backup").is_ok());
    }

    #[test]
    fn accepts_path_with_spaces() {
        assert!(validate_safe_path("/home/user/my folder/tool").is_ok());
    }

    #[test]
    fn accepts_path_with_unicode() {
        assert!(validate_safe_path("/home/user/ドキュメント/tool").is_ok());
    }

    // --- validate_safe_path: shell special characters ---

    #[test]
    fn rejects_double_quote() {
        let result = validate_safe_path("/path/with\"quote");
        assert!(matches!(
            result,
            Err(PathValidateError::UnsafeChar { ch: '"', .. })
        ));
    }

    #[test]
    fn rejects_dollar_sign() {
        let result = validate_safe_path("/path/$HOME/bin");
        assert!(matches!(
            result,
            Err(PathValidateError::UnsafeChar { ch: '$', .. })
        ));
    }

    #[test]
    fn rejects_backtick() {
        let result = validate_safe_path("/path/with`cmd`");
        assert!(matches!(
            result,
            Err(PathValidateError::UnsafeChar { ch: '`', .. })
        ));
    }

    #[test]
    fn rejects_backslash() {
        let result = validate_safe_path("/path/with\\escape");
        assert!(matches!(
            result,
            Err(PathValidateError::UnsafeChar { ch: '\\', .. })
        ));
    }

    #[test]
    fn rejects_single_quote() {
        let result = validate_safe_path("/path/with'quote");
        assert!(matches!(
            result,
            Err(PathValidateError::UnsafeChar { ch: '\'', .. })
        ));
    }

    // --- validate_safe_path: systemd specifier ---

    #[test]
    fn rejects_percent() {
        let result = validate_safe_path("/path/with%n");
        assert!(matches!(
            result,
            Err(PathValidateError::UnsafeChar { ch: '%', .. })
        ));
    }

    // --- validate_safe_path: control characters ---

    #[test]
    fn rejects_newline() {
        let result = validate_safe_path("/path/with\nnewline");
        assert!(matches!(
            result,
            Err(PathValidateError::UnsafeChar { ch: '\n', .. })
        ));
    }

    #[test]
    fn rejects_carriage_return() {
        let result = validate_safe_path("/path/with\rreturn");
        assert!(matches!(
            result,
            Err(PathValidateError::UnsafeChar { ch: '\r', .. })
        ));
    }

    #[test]
    fn rejects_null_byte() {
        let result = validate_safe_path("/path/with\0null");
        assert!(matches!(
            result,
            Err(PathValidateError::UnsafeChar { ch: '\0', .. })
        ));
    }

    #[test]
    fn rejects_tab() {
        let result = validate_safe_path("/path/with\ttab");
        assert!(matches!(result, Err(PathValidateError::UnsafeChar { .. })));
    }

    // --- is_safe_for_shell ---

    #[test]
    fn is_safe_returns_true_for_normal_path() {
        assert!(is_safe_for_shell("/usr/bin/tool"));
    }

    #[test]
    fn is_safe_returns_false_for_unsafe_path() {
        assert!(!is_safe_for_shell("/path/$HOME"));
    }
}
