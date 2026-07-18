//! Login shell acquisition via `getent passwd` (infra side of shell fallback).

use crate::domain::shell_detect;
use crate::infra::command_runner::CommandRunner;

/// Fetch the user's login shell by running `getent passwd <user>`.
///
/// Returns `None` on any failure mode — non-zero exit (user not found), empty
/// output, a malformed line, or a non-interactive login shell — so the caller can
/// safely treat "no usable login shell" uniformly. Never panics.
pub fn fetch_login_shell<R: CommandRunner>(runner: &R, user: &str) -> Option<String> {
    let output = runner.run("getent", &["passwd", user]).ok()?;
    shell_detect::parse_login_shell(&output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infra::command_runner::testing::MockCommandRunner;

    #[test]
    fn returns_login_shell_from_getent_output() {
        let mock = MockCommandRunner::new(vec![Ok(
            "user:x:1000:1000:User:/home/user:/usr/bin/fish".to_string(),
        )]);
        assert_eq!(
            fetch_login_shell(&mock, "user"),
            Some("/usr/bin/fish".to_string())
        );
        // The command is invoked as `getent passwd <user>`.
        let calls = mock.get_calls();
        assert_eq!(calls[0].0, "getent");
        assert_eq!(calls[0].1, vec!["passwd", "user"]);
    }

    #[test]
    fn returns_none_when_getent_fails() {
        let mock = MockCommandRunner::new(vec![Err("exit status 2".to_string())]);
        assert_eq!(fetch_login_shell(&mock, "ghost"), None);
    }

    #[test]
    fn returns_none_on_empty_output() {
        let mock = MockCommandRunner::new(vec![Ok(String::new())]);
        assert_eq!(fetch_login_shell(&mock, "user"), None);
    }

    #[test]
    fn returns_none_on_nologin() {
        let mock = MockCommandRunner::new(vec![Ok(
            "svc:x:998:998::/var/lib/svc:/usr/sbin/nologin".to_string(),
        )]);
        assert_eq!(fetch_login_shell(&mock, "svc"), None);
    }
}
