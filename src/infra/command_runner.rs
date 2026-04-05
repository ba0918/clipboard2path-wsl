//! Command execution abstraction for testability.

use std::process::Command;

/// Trait for executing system commands (enables DI for testing).
pub trait CommandRunner {
    /// Run a command and return stdout on success, or an error message on failure.
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String>;
}

/// Real implementation using `std::process::Command`.
pub struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| format!("failed to execute '{program}': {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let msg = if stderr.is_empty() { &stdout } else { &stderr };
            Err(format!(
                "'{program}' exited with {}: {msg}",
                output.status
            ))
        }
    }
}

/// Mock implementation for testing. Records calls and returns configured responses.
#[cfg(test)]
pub mod testing {
    use super::CommandRunner;
    use std::cell::RefCell;

    /// A mock command runner that records invocations and returns pre-configured results.
    pub struct MockCommandRunner {
        /// Recorded (program, args) calls.
        pub calls: RefCell<Vec<(String, Vec<String>)>>,
        /// Pre-configured responses: each `run()` call pops from the front.
        responses: RefCell<Vec<Result<String, String>>>,
    }

    impl MockCommandRunner {
        pub fn new(responses: Vec<Result<String, String>>) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                responses: RefCell::new(responses),
            }
        }

        /// Get the recorded calls.
        pub fn get_calls(&self) -> Vec<(String, Vec<String>)> {
            self.calls.borrow().clone()
        }
    }

    impl CommandRunner for MockCommandRunner {
        fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
            self.calls.borrow_mut().push((
                program.to_string(),
                args.iter().map(|a| a.to_string()).collect(),
            ));

            let mut responses = self.responses.borrow_mut();
            if responses.is_empty() {
                Ok(String::new())
            } else {
                responses.remove(0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::MockCommandRunner;
    use super::CommandRunner;

    #[test]
    fn mock_records_calls() {
        let mock = MockCommandRunner::new(vec![Ok("ok".to_string())]);
        let _ = mock.run("systemctl", &["--user", "enable", "clipboard2path"]);

        let calls = mock.get_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "systemctl");
        assert_eq!(calls[0].1, vec!["--user", "enable", "clipboard2path"]);
    }

    #[test]
    fn mock_returns_configured_responses_in_order() {
        let mock = MockCommandRunner::new(vec![
            Ok("first".to_string()),
            Err("second-err".to_string()),
            Ok("third".to_string()),
        ]);

        assert_eq!(mock.run("a", &[]), Ok("first".to_string()));
        assert_eq!(mock.run("b", &[]), Err("second-err".to_string()));
        assert_eq!(mock.run("c", &[]), Ok("third".to_string()));
    }

    #[test]
    fn mock_returns_ok_empty_when_responses_exhausted() {
        let mock = MockCommandRunner::new(vec![]);
        assert_eq!(mock.run("x", &[]), Ok(String::new()));
    }

    #[test]
    fn mock_records_multiple_calls() {
        let mock = MockCommandRunner::new(vec![Ok(String::new()), Ok(String::new())]);
        let _ = mock.run("cmd1", &["a"]);
        let _ = mock.run("cmd2", &["b", "c"]);

        let calls = mock.get_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], ("cmd1".to_string(), vec!["a".to_string()]));
        assert_eq!(
            calls[1],
            ("cmd2".to_string(), vec!["b".to_string(), "c".to_string()])
        );
    }
}
