//! Command execution abstraction for testability.

use std::process::Command;

/// Captured result of a command execution, preserving the terminal state.
///
/// Unlike [`CommandRunner::run`], this keeps the exit code and stderr even for
/// non-zero exits, so query commands (e.g. `systemctl is-active`) can inspect
/// the outcome instead of losing it to an error string.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandOutput {
    /// Exit code, or `None` when the process was terminated by a signal.
    pub exit_code: Option<i32>,
    /// Trimmed standard output.
    pub stdout: String,
    /// Trimmed standard error.
    pub stderr: String,
}

/// Trait for executing system commands (enables DI for testing).
pub trait CommandRunner {
    /// Run a command and return stdout on success, or an error message on failure.
    ///
    /// Contract: success (exit 0) → `Ok(stdout)`; non-zero exit or spawn failure → `Err`.
    fn run(&self, program: &str, args: &[&str]) -> Result<String, String>;

    /// Run a command and capture its terminal state without treating a non-zero
    /// exit as an error.
    ///
    /// Contract: `Err` only when the process fails to spawn. A non-zero exit or a
    /// signal termination returns `Ok(CommandOutput)` with the state preserved.
    fn run_capturing(&self, program: &str, args: &[&str]) -> Result<CommandOutput, String>;
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
            Err(format!("'{program}' exited with {}: {msg}", output.status))
        }
    }

    fn run_capturing(&self, program: &str, args: &[&str]) -> Result<CommandOutput, String> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|e| format!("failed to execute '{program}': {e}"))?;

        Ok(CommandOutput {
            exit_code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

/// Mock implementation for testing. Records calls and returns configured responses.
#[cfg(test)]
pub mod testing {
    use super::{CommandOutput, CommandRunner};
    use std::cell::RefCell;

    /// A mock command runner that records invocations and returns pre-configured results.
    pub struct MockCommandRunner {
        /// Recorded (program, args) calls (shared across `run` and `run_capturing`).
        pub calls: RefCell<Vec<(String, Vec<String>)>>,
        /// Pre-configured `run()` responses: each call pops from the front.
        responses: RefCell<Vec<Result<String, String>>>,
        /// Pre-configured `run_capturing()` responses: each call pops from the front.
        capturing_responses: RefCell<Vec<Result<CommandOutput, String>>>,
    }

    impl MockCommandRunner {
        pub fn new(responses: Vec<Result<String, String>>) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                responses: RefCell::new(responses),
                capturing_responses: RefCell::new(Vec::new()),
            }
        }

        /// Configure the `run_capturing()` responses.
        pub fn with_capturing(mut self, responses: Vec<Result<CommandOutput, String>>) -> Self {
            self.capturing_responses = RefCell::new(responses);
            self
        }

        /// Get the recorded calls.
        pub fn get_calls(&self) -> Vec<(String, Vec<String>)> {
            self.calls.borrow().clone()
        }

        fn record(&self, program: &str, args: &[&str]) {
            self.calls.borrow_mut().push((
                program.to_string(),
                args.iter().map(|a| a.to_string()).collect(),
            ));
        }
    }

    impl CommandRunner for MockCommandRunner {
        fn run(&self, program: &str, args: &[&str]) -> Result<String, String> {
            self.record(program, args);

            let mut responses = self.responses.borrow_mut();
            if responses.is_empty() {
                Ok(String::new())
            } else {
                responses.remove(0)
            }
        }

        fn run_capturing(&self, program: &str, args: &[&str]) -> Result<CommandOutput, String> {
            self.record(program, args);

            let mut responses = self.capturing_responses.borrow_mut();
            if responses.is_empty() {
                Ok(CommandOutput {
                    exit_code: Some(0),
                    stdout: String::new(),
                    stderr: String::new(),
                })
            } else {
                responses.remove(0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::MockCommandRunner;
    use super::{CommandOutput, CommandRunner, RealCommandRunner};

    #[test]
    fn run_success_returns_stdout() {
        let runner = RealCommandRunner;
        let result = runner.run("sh", &["-c", "echo hello"]);
        assert_eq!(result, Ok("hello".to_string()));
    }

    #[test]
    fn run_non_zero_exit_returns_err() {
        let runner = RealCommandRunner;
        let result = runner.run("sh", &["-c", "exit 3"]);
        assert!(result.is_err());
    }

    #[test]
    fn run_spawn_failure_returns_err() {
        let runner = RealCommandRunner;
        let result = runner.run("clipboard2path_nonexistent_cmd_xyz", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn run_capturing_holds_exit_code_stdout_stderr() {
        let runner = RealCommandRunner;
        let out = runner
            .run_capturing("sh", &["-c", "echo out; echo err >&2; exit 2"])
            .unwrap();
        assert_eq!(out.exit_code, Some(2));
        assert_eq!(out.stdout, "out");
        assert_eq!(out.stderr, "err");
    }

    #[test]
    fn run_capturing_success_exit_code_zero() {
        let runner = RealCommandRunner;
        let out = runner.run_capturing("sh", &["-c", "echo ok"]).unwrap();
        assert_eq!(out.exit_code, Some(0));
        assert_eq!(out.stdout, "ok");
    }

    #[test]
    fn run_capturing_non_zero_is_ok_not_err() {
        let runner = RealCommandRunner;
        let out = runner.run_capturing("sh", &["-c", "exit 5"]).unwrap();
        assert_eq!(out.exit_code, Some(5));
    }

    #[test]
    fn run_capturing_separates_stderr_from_stdout() {
        let runner = RealCommandRunner;
        let out = runner
            .run_capturing("sh", &["-c", "echo to_out; echo to_err >&2"])
            .unwrap();
        assert_eq!(out.stdout, "to_out");
        assert_eq!(out.stderr, "to_err");
    }

    #[test]
    fn run_capturing_signal_termination_exit_code_none() {
        let runner = RealCommandRunner;
        let out = runner
            .run_capturing("sh", &["-c", "kill -KILL $$"])
            .unwrap();
        assert_eq!(out.exit_code, None);
    }

    #[test]
    fn run_capturing_spawn_failure_returns_err() {
        let runner = RealCommandRunner;
        let result = runner.run_capturing("clipboard2path_nonexistent_cmd_xyz", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn mock_responds_to_run_and_run_capturing() {
        let mock =
            MockCommandRunner::new(vec![Ok("run-out".to_string())]).with_capturing(vec![Ok(
                CommandOutput {
                    exit_code: Some(0),
                    stdout: "cap-out".to_string(),
                    stderr: String::new(),
                },
            )]);

        assert_eq!(mock.run("a", &[]), Ok("run-out".to_string()));
        let cap = mock.run_capturing("b", &[]).unwrap();
        assert_eq!(cap.stdout, "cap-out");

        // Both calls are recorded.
        assert_eq!(mock.get_calls().len(), 2);
    }

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
