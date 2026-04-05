/// Determine if running on WSL2 by inspecting /proc/version content.
///
/// Pure function: takes the content string, returns a bool.
pub fn is_wsl2(proc_version: &str) -> bool {
    let lower = proc_version.to_ascii_lowercase();
    lower.contains("microsoft") || lower.contains("wsl")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_wsl2_microsoft() {
        let version = "Linux version 5.15.90.1-microsoft-standard-WSL2 (root@1234) (gcc) #1 SMP";
        assert!(is_wsl2(version));
    }

    #[test]
    fn detects_wsl2_wsl_keyword() {
        let version = "Linux version 5.15.90.1-WSL2 (root@abc) (gcc) #1 SMP";
        assert!(is_wsl2(version));
    }

    #[test]
    fn rejects_normal_linux() {
        let version =
            "Linux version 6.1.0-18-amd64 (debian-kernel@lists.debian.org) (gcc-12) #1 SMP";
        assert!(!is_wsl2(version));
    }

    #[test]
    fn case_insensitive() {
        let version = "Linux version 5.15.90.1-MICROSOFT-standard-WSL2";
        assert!(is_wsl2(version));
    }

    #[test]
    fn empty_string_returns_false() {
        assert!(!is_wsl2(""));
    }
}
