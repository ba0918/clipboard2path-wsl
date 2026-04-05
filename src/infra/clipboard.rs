use std::fmt;
use std::process::Command;

/// Error type for clipboard operations.
#[derive(Debug)]
pub enum ClipboardError {
    /// The clipboard tool (wl-paste/wl-copy) was not found.
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
}

/// Write text to the clipboard.
pub trait ClipboardWriter {
    fn write_text(&self, text: &str) -> Result<(), ClipboardError>;
}

/// Implementation using wl-paste / wl-copy (Wayland clipboard tools).
pub struct WlClipboard;

impl ClipboardReader for WlClipboard {
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

impl ClipboardWriter for WlClipboard {
    fn write_text(&self, text: &str) -> Result<(), ClipboardError> {
        use std::io::Write;

        let mut child = Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    ClipboardError::ToolNotFound("wl-copy".to_string())
                } else {
                    ClipboardError::SpawnFailed(e.to_string())
                }
            })?;

        if let Some(ref mut stdin) = child.stdin {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| ClipboardError::SpawnFailed(e.to_string()))?;
        }

        let status = child
            .wait()
            .map_err(|e| ClipboardError::SpawnFailed(e.to_string()))?;

        if !status.success() {
            return Err(ClipboardError::CommandFailed {
                tool: "wl-copy".to_string(),
                stderr: "non-zero exit code".to_string(),
            });
        }

        Ok(())
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
    }

    struct MockWriter {
        written: std::cell::RefCell<Option<String>>,
    }

    impl ClipboardWriter for MockWriter {
        fn write_text(&self, text: &str) -> Result<(), ClipboardError> {
            *self.written.borrow_mut() = Some(text.to_string());
            Ok(())
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

    #[test]
    fn mock_writer_stores_text() {
        let writer = MockWriter {
            written: std::cell::RefCell::new(None),
        };
        writer.write_text("/tmp/test.png").unwrap();
        assert_eq!(*writer.written.borrow(), Some("/tmp/test.png".to_string()));
    }
}
