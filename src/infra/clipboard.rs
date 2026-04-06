//! Clipboard reading via wl-paste (read-only).

use std::fmt;
use std::process::Command;

/// Error type for clipboard operations.
#[derive(Debug)]
pub enum ClipboardError {
    /// The clipboard tool (wl-paste) was not found.
    ToolNotFound(String),
    /// The clipboard tool exited with a non-zero status.
    CommandFailed { tool: String, stderr: String },
    /// Failed to spawn the clipboard tool process.
    SpawnFailed(String),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClipboardError::ToolNotFound(tool) => {
                write!(
                    f,
                    "{tool} not found. Install with: sudo apt install wl-clipboard"
                )
            }
            ClipboardError::CommandFailed { tool, stderr } => {
                write!(f, "{tool} failed: {stderr}")
            }
            ClipboardError::SpawnFailed(msg) => write!(f, "failed to spawn process: {msg}"),
        }
    }
}

/// Read image data from the clipboard.
pub trait ClipboardReader {
    fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError>;
    /// List available MIME types in the clipboard.
    fn list_types(&self) -> Result<Vec<String>, ClipboardError>;
}

/// Implementation using wl-paste (Wayland clipboard tool, read-only).
pub struct WlClipboard;

impl ClipboardReader for WlClipboard {
    fn list_types(&self) -> Result<Vec<String>, ClipboardError> {
        let output = Command::new("wl-paste")
            .arg("--list-types")
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ClipboardError::ToolNotFound("wl-paste".to_string())
                } else {
                    ClipboardError::SpawnFailed(e.to_string())
                }
            })?;

        if !output.status.success() {
            return Err(ClipboardError::CommandFailed {
                tool: "wl-paste".to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        let types = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(types)
    }

    fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError> {
        let output = Command::new("wl-paste")
            .arg("--type")
            .arg("image/bmp")
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ClipboardError::ToolNotFound("wl-paste".to_string())
                } else {
                    ClipboardError::SpawnFailed(e.to_string())
                }
            })?;

        if !output.status.success() {
            return Err(ClipboardError::CommandFailed {
                tool: "wl-paste".to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        Ok(output.stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Unit tests for the trait contracts — we test with mock implementations
    // since wl-paste/wl-copy may not be available in CI.

    struct MockReader {
        data: Result<Vec<u8>, &'static str>,
    }

    impl ClipboardReader for MockReader {
        fn read_image_bmp(&self) -> Result<Vec<u8>, ClipboardError> {
            match &self.data {
                Ok(d) => Ok(d.clone()),
                Err(msg) => Err(ClipboardError::CommandFailed {
                    tool: "mock".to_string(),
                    stderr: msg.to_string(),
                }),
            }
        }

        fn list_types(&self) -> Result<Vec<String>, ClipboardError> {
            Ok(vec!["image/bmp".to_string()])
        }
    }

    #[test]
    fn mock_reader_returns_data() {
        let reader = MockReader {
            data: Ok(vec![1, 2, 3]),
        };
        let result = reader.read_image_bmp().unwrap();
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[test]
    fn mock_reader_returns_error() {
        let reader = MockReader {
            data: Err("no image"),
        };
        let result = reader.read_image_bmp();
        assert!(result.is_err());
    }
}
