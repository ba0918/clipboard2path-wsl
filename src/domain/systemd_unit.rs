//! Systemd unit file generation (pure functions).

/// Service name constant.
pub const SERVICE_NAME: &str = "clipboard2path.service";

/// Generate the systemd user unit file content.
///
/// Pure function: takes the executable path and user ID,
/// returns a complete unit file as a string.
pub fn generate_unit(exec_path: &str, uid: u32) -> String {
    format!(
        "[Unit]
Description=clipboard2path-wsl — clipboard image to file path daemon
After=graphical-session.target

[Service]
Type=simple
ExecStart={exec_path}
Restart=on-failure
RestartSec=5
Environment=WAYLAND_DISPLAY=wayland-0
Environment=XDG_RUNTIME_DIR=/run/user/{uid}

[Install]
WantedBy=default.target
"
    )
}

/// Return the install path for the systemd unit file.
///
/// Pure function: takes the home directory, returns the full path.
pub fn unit_install_path(home: &str) -> String {
    format!("{home}/.config/systemd/user/{SERVICE_NAME}")
}

/// Parse UID from `/proc/self/status` content.
///
/// Looks for the `Uid:` line and extracts the real UID (first field).
/// Pure function: takes the file content, returns the parsed UID.
pub fn parse_uid_from_proc_status(content: &str) -> Result<u32, String> {
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            let uid_str = rest.split_whitespace().next().ok_or_else(|| {
                "Uid line found but no value present".to_string()
            })?;
            return uid_str
                .parse::<u32>()
                .map_err(|e| format!("failed to parse UID '{uid_str}': {e}"));
        }
    }
    Err("Uid line not found in proc status".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_name_is_correct() {
        assert_eq!(SERVICE_NAME, "clipboard2path.service");
    }

    #[test]
    fn generate_unit_contains_all_sections() {
        let unit = generate_unit("/usr/local/bin/clipboard2path-wsl", 1000);
        assert!(unit.contains("[Unit]"));
        assert!(unit.contains("[Service]"));
        assert!(unit.contains("[Install]"));
    }

    #[test]
    fn generate_unit_contains_exec_path() {
        let unit = generate_unit("/opt/bin/my-tool", 1000);
        assert!(unit.contains("ExecStart=/opt/bin/my-tool"));
    }

    #[test]
    fn generate_unit_contains_wayland_display() {
        let unit = generate_unit("/bin/test", 1000);
        assert!(unit.contains("Environment=WAYLAND_DISPLAY=wayland-0"));
    }

    #[test]
    fn generate_unit_contains_xdg_runtime_dir_with_uid() {
        let unit = generate_unit("/bin/test", 1234);
        assert!(unit.contains("Environment=XDG_RUNTIME_DIR=/run/user/1234"));
    }

    #[test]
    fn generate_unit_has_correct_uid_in_runtime_dir() {
        let unit = generate_unit("/bin/test", 5000);
        assert!(unit.contains("/run/user/5000"));
        assert!(!unit.contains("/run/user/1000"));
    }

    #[test]
    fn unit_install_path_returns_correct_path() {
        let path = unit_install_path("/home/user");
        assert_eq!(
            path,
            "/home/user/.config/systemd/user/clipboard2path.service"
        );
    }

    #[test]
    fn parse_uid_from_proc_status_typical() {
        let content = "Name:\tclipboard2path\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(parse_uid_from_proc_status(content), Ok(1000));
    }

    #[test]
    fn parse_uid_from_proc_status_different_uid() {
        let content = "Uid:\t5001\t5001\t5001\t5001\n";
        assert_eq!(parse_uid_from_proc_status(content), Ok(5001));
    }

    #[test]
    fn parse_uid_from_proc_status_missing_uid_line() {
        let content = "Name:\ttest\nGid:\t1000\n";
        assert!(parse_uid_from_proc_status(content).is_err());
    }

    #[test]
    fn parse_uid_from_proc_status_empty_value() {
        let content = "Uid:\t\n";
        // split_whitespace on empty/whitespace returns nothing
        assert!(parse_uid_from_proc_status(content).is_err());
    }

    #[test]
    fn parse_uid_from_proc_status_invalid_number() {
        let content = "Uid:\tabc\t1000\n";
        assert!(parse_uid_from_proc_status(content).is_err());
    }
}
