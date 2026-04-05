//! Detect clipboard content changes by comparing MIME type lists.
//!
//! Pure function: no I/O, just comparison logic.

/// Check if the clipboard content has changed based on MIME type lists.
pub fn has_clipboard_changed(previous: &[String], current: &[String]) -> bool {
    previous != current
}

/// Check if the clipboard contains a BMP image.
pub fn has_bmp_image(types: &[String]) -> bool {
    types.iter().any(|t| t == "image/bmp")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(val: &str) -> String {
        val.to_string()
    }

    #[test]
    fn detects_change_when_types_differ() {
        let prev = vec![s("text/plain")];
        let curr = vec![s("image/bmp"), s("image/png")];
        assert!(has_clipboard_changed(&prev, &curr));
    }

    #[test]
    fn no_change_when_types_same() {
        let prev = vec![s("image/bmp")];
        let curr = vec![s("image/bmp")];
        assert!(!has_clipboard_changed(&prev, &curr));
    }

    #[test]
    fn detects_change_from_empty() {
        let prev: Vec<String> = vec![];
        let curr = vec![s("image/bmp")];
        assert!(has_clipboard_changed(&prev, &curr));
    }

    #[test]
    fn detects_change_to_empty() {
        let prev = vec![s("image/bmp")];
        let curr: Vec<String> = vec![];
        assert!(has_clipboard_changed(&prev, &curr));
    }

    #[test]
    fn has_bmp_when_present() {
        let types = vec![s("text/plain"), s("image/bmp")];
        assert!(has_bmp_image(&types));
    }

    #[test]
    fn no_bmp_when_absent() {
        let types = vec![s("text/plain"), s("image/png")];
        assert!(!has_bmp_image(&types));
    }

    #[test]
    fn no_bmp_when_empty() {
        let types: Vec<String> = vec![];
        assert!(!has_bmp_image(&types));
    }
}
