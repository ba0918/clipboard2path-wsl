//! wl-paste wrapper script generation (pure functions).
//!
//! Generates a bash wrapper script that intercepts `wl-paste --type image/png`
//! requests and returns the daemon's saved PNG instead of going through WSLg's
//! BMP-only clipboard bridge.

/// Marker comment embedded in the wrapper script for ownership detection.
pub const WRAPPER_MARKER: &str = "MANAGED BY clipboard2path-wsl";

/// Default path to the real wl-paste binary.
pub const DEFAULT_REAL_WL_PASTE: &str = "/usr/bin/wl-paste";

/// Default install directory for the wrapper.
pub const WRAPPER_DIR: &str = ".local/bin";

/// Wrapper script filename.
pub const WRAPPER_FILENAME: &str = "wl-paste";

/// Generate the wl-paste wrapper script.
///
/// Pure function: takes the path to the real wl-paste binary,
/// returns the complete bash script as a string.
pub fn generate_wrapper(real_wl_paste: &str) -> String {
    format!(
        r#"#!/bin/bash
# clipboard2path-wsl wl-paste wrapper
# {WRAPPER_MARKER} — DO NOT EDIT
# Bridges daemon's saved PNG to applications requesting image/png

REAL_WL_PASTE="{real_wl_paste}"
LATEST_PNG="${{XDG_RUNTIME_DIR}}/clipboard2path/latest.png"

# Bail out immediately if XDG_RUNTIME_DIR is unset (match daemon behavior)
[ -z "$XDG_RUNTIME_DIR" ] && exec "$REAL_WL_PASTE" "$@"

# Bail out if --watch, --primary, --seat, or unknown long options are present
# (only intercept simple single-shot --type image/png requests)
for arg in "$@"; do
    case "$arg" in
        --watch|-w|--primary|-p|--seat|-s) exec "$REAL_WL_PASTE" "$@" ;;
    esac
done

# Detect --type image/png or -t image/png request (space or = separated)
want_png=0
prev=""
for arg in "$@"; do
    case "$arg" in
        --type=image/png|-t=image/png) want_png=1; break ;;
    esac
    if [ "$prev" = "--type" ] || [ "$prev" = "-t" ]; then
        [ "$arg" = "image/png" ] && want_png=1 && break
    fi
    prev="$arg"
done

if [ "$want_png" = "1" ] && [ -L "$LATEST_PNG" ] && [ -f "$LATEST_PNG" ]; then
    cat "$LATEST_PNG"
    exit 0
fi

exec "$REAL_WL_PASTE" "$@"
"#
    )
}

/// Return the full install path for the wrapper script.
pub fn wrapper_install_path(home_dir: &str) -> String {
    format!("{home_dir}/{WRAPPER_DIR}/{WRAPPER_FILENAME}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_wrapper_contains_shebang() {
        let script = generate_wrapper(DEFAULT_REAL_WL_PASTE);
        assert!(script.starts_with("#!/bin/bash"));
    }

    #[test]
    fn generate_wrapper_contains_managed_marker() {
        let script = generate_wrapper(DEFAULT_REAL_WL_PASTE);
        assert!(script.contains(WRAPPER_MARKER));
    }

    #[test]
    fn generate_wrapper_contains_real_wl_paste_path() {
        let script = generate_wrapper("/custom/path/wl-paste");
        assert!(script.contains(r#"REAL_WL_PASTE="/custom/path/wl-paste""#));
    }

    #[test]
    fn generate_wrapper_contains_watch_primary_seat_delegation() {
        let script = generate_wrapper(DEFAULT_REAL_WL_PASTE);
        assert!(script.contains("--watch|-w|--primary|-p|--seat|-s"));
        assert!(script.contains(r#"exec "$REAL_WL_PASTE" "$@""#));
    }

    #[test]
    fn generate_wrapper_contains_type_image_png_detection() {
        let script = generate_wrapper(DEFAULT_REAL_WL_PASTE);
        assert!(script.contains("--type=image/png|-t=image/png"));
        assert!(script.contains(r#""$prev" = "--type""#));
        assert!(script.contains(r#""$prev" = "-t""#));
    }

    #[test]
    fn generate_wrapper_contains_latest_png_symlink_read() {
        let script = generate_wrapper(DEFAULT_REAL_WL_PASTE);
        assert!(script.contains("LATEST_PNG="));
        assert!(script.contains("latest.png"));
        assert!(script.contains(r#"[ -L "$LATEST_PNG" ]"#));
        assert!(script.contains(r#"[ -f "$LATEST_PNG" ]"#));
        assert!(script.contains(r#"cat "$LATEST_PNG""#));
    }

    #[test]
    fn generate_wrapper_contains_xdg_runtime_dir_check() {
        let script = generate_wrapper(DEFAULT_REAL_WL_PASTE);
        assert!(script.contains(r#"[ -z "$XDG_RUNTIME_DIR" ]"#));
    }

    #[test]
    fn generate_wrapper_contains_exec_fallback() {
        let script = generate_wrapper(DEFAULT_REAL_WL_PASTE);
        // Last meaningful line should be exec fallback
        let lines: Vec<&str> = script.trim().lines().collect();
        let last = lines.last().unwrap();
        assert!(last.contains(r#"exec "$REAL_WL_PASTE" "$@""#));
    }

    #[test]
    fn wrapper_install_path_correct() {
        let path = wrapper_install_path("/home/user");
        assert_eq!(path, "/home/user/.local/bin/wl-paste");
    }
}
