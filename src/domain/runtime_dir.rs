//! Runtime directory resolution (pure function).

use std::fmt;
use std::path::PathBuf;

/// Error type for runtime directory resolution.
#[derive(Debug, PartialEq)]
pub enum RuntimeDirError {
    /// `$XDG_RUNTIME_DIR` is not set.
    NotSet,
}

impl fmt::Display for RuntimeDirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuntimeDirError::NotSet => write!(
                f,
                "$XDG_RUNTIME_DIR is not set. Ensure systemd is enabled in WSL2, \
                 or specify --output-dir explicitly."
            ),
        }
    }
}

/// Resolve the runtime directory for clipboard2path data.
///
/// Pure function: takes the value of `$XDG_RUNTIME_DIR` (or `None` if unset),
/// returns the full path `$XDG_RUNTIME_DIR/clipboard2path/`.
pub fn resolve_runtime_dir(xdg_runtime_dir: Option<&str>) -> Result<PathBuf, RuntimeDirError> {
    match xdg_runtime_dir {
        Some(dir) if !dir.is_empty() => Ok(PathBuf::from(dir).join("clipboard2path")),
        _ => Err(RuntimeDirError::NotSet),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_with_xdg_runtime_dir() {
        let result = resolve_runtime_dir(Some("/run/user/1000")).unwrap();
        assert_eq!(result, PathBuf::from("/run/user/1000/clipboard2path"));
    }

    #[test]
    fn error_when_not_set() {
        let result = resolve_runtime_dir(None);
        assert_eq!(result, Err(RuntimeDirError::NotSet));
    }

    #[test]
    fn error_when_empty_string() {
        let result = resolve_runtime_dir(Some(""));
        assert_eq!(result, Err(RuntimeDirError::NotSet));
    }
}
