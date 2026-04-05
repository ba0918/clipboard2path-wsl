use std::fmt;
use std::path::{Path, PathBuf};

/// Error type for path operations.
#[derive(Debug, PartialEq)]
pub enum PathError {
    /// Path contains traversal components (e.g. "..")
    TraversalDetected,
    /// Directory does not exist
    DirNotFound(String),
    /// Path canonicalization failed
    CanonicalizeFailed(String),
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::TraversalDetected => write!(f, "path traversal detected"),
            PathError::DirNotFound(p) => write!(f, "directory not found: {p}"),
            PathError::CanonicalizeFailed(msg) => write!(f, "canonicalize failed: {msg}"),
        }
    }
}

/// Generate a save path for a clipboard image.
///
/// Pure function: combines base_dir and timestamp into a deterministic path.
/// Does NOT touch the file system — that is the caller's responsibility.
pub fn generate_save_path(base_dir: &Path, timestamp: &str) -> Result<PathBuf, PathError> {
    // Reject traversal in timestamp
    if timestamp.contains("..") || timestamp.contains('/') || timestamp.contains('\\') {
        return Err(PathError::TraversalDetected);
    }

    let filename = format!("clipboard-{timestamp}.png");
    let path = base_dir.join(&filename);

    // Verify the resulting path is still under base_dir
    // (defense-in-depth against crafted timestamps)
    if !path.starts_with(base_dir) {
        return Err(PathError::TraversalDetected);
    }

    Ok(path)
}

/// Validate that an output directory exists and is safe.
///
/// This function DOES touch the file system (canonicalize), so it belongs
/// at the boundary between domain and infrastructure. It is kept here
/// because the validation logic (traversal detection) is domain knowledge.
pub fn validate_output_dir(path: &Path) -> Result<PathBuf, PathError> {
    // Reject paths containing ".."
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(PathError::TraversalDetected);
        }
    }

    let canonical = path
        .canonicalize()
        .map_err(|e| PathError::CanonicalizeFailed(e.to_string()))?;

    if !canonical.is_dir() {
        return Err(PathError::DirNotFound(canonical.display().to_string()));
    }

    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // --- generate_save_path tests ---

    #[test]
    fn generates_expected_path() {
        let base = Path::new("/tmp");
        let result = generate_save_path(base, "20260406-120000").unwrap();
        assert_eq!(result, PathBuf::from("/tmp/clipboard-20260406-120000.png"));
    }

    #[test]
    fn rejects_traversal_in_timestamp() {
        let base = Path::new("/tmp");
        assert_eq!(
            generate_save_path(base, "../etc/passwd"),
            Err(PathError::TraversalDetected)
        );
    }

    #[test]
    fn rejects_slash_in_timestamp() {
        let base = Path::new("/tmp");
        assert_eq!(
            generate_save_path(base, "foo/bar"),
            Err(PathError::TraversalDetected)
        );
    }

    #[test]
    fn rejects_backslash_in_timestamp() {
        let base = Path::new("/tmp");
        assert_eq!(
            generate_save_path(base, "foo\\bar"),
            Err(PathError::TraversalDetected)
        );
    }

    // --- validate_output_dir tests ---

    #[test]
    fn validates_existing_dir() {
        // /tmp should always exist on Linux
        let result = validate_output_dir(Path::new("/tmp"));
        assert!(result.is_ok());
    }

    #[test]
    fn rejects_traversal_components() {
        let result = validate_output_dir(Path::new("/tmp/../etc"));
        assert_eq!(result, Err(PathError::TraversalDetected));
    }

    #[test]
    fn rejects_nonexistent_dir() {
        let result = validate_output_dir(Path::new("/nonexistent_dir_12345"));
        assert!(matches!(result, Err(PathError::CanonicalizeFailed(_))));
    }
}
