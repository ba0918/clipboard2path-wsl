//! Temporary file cleanup logic (pure functions).

use std::path::PathBuf;
use std::time::Duration;

/// A file entry with its age for cleanup evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub age: Duration,
}

/// Determine which files should be deleted based on age threshold.
///
/// Pure function: takes a list of file entries and a max age, returns
/// the paths that exceed the threshold.
pub fn files_to_clean_by_age(entries: &[FileEntry], max_age: Duration) -> Vec<PathBuf> {
    entries
        .iter()
        .filter(|e| e.age > max_age)
        .map(|e| e.path.clone())
        .collect()
}

/// Determine which files should be deleted to stay under a max count.
///
/// Files are assumed to be sorted oldest-first. Returns excess files
/// (the oldest ones beyond the limit).
pub fn files_to_clean_by_count(entries: &[FileEntry], max_count: usize) -> Vec<PathBuf> {
    if entries.len() <= max_count {
        return Vec::new();
    }
    let excess = entries.len() - max_count;
    entries[..excess].iter().map(|e| e.path.clone()).collect()
}

/// Check if a filename matches our clipboard PNG pattern.
pub fn is_clipboard_png(filename: &str) -> bool {
    filename.starts_with("clipboard-") && filename.ends_with(".png")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn entry(name: &str, secs: u64) -> FileEntry {
        FileEntry {
            path: PathBuf::from(format!("/tmp/{name}")),
            age: Duration::from_secs(secs),
        }
    }

    #[test]
    fn age_cleanup_removes_old_files() {
        let entries = vec![
            entry("clipboard-1.png", 3600),  // 1 hour old
            entry("clipboard-2.png", 90000), // 25 hours old
            entry("clipboard-3.png", 100),   // recent
        ];
        let max_age = Duration::from_secs(86400); // 24 hours
        let result = files_to_clean_by_age(&entries, max_age);
        assert_eq!(result, vec![PathBuf::from("/tmp/clipboard-2.png")]);
    }

    #[test]
    fn age_cleanup_empty_when_all_recent() {
        let entries = vec![entry("clipboard-1.png", 100)];
        let max_age = Duration::from_secs(86400);
        assert!(files_to_clean_by_age(&entries, max_age).is_empty());
    }

    #[test]
    fn count_cleanup_removes_excess_oldest() {
        let entries = vec![
            entry("clipboard-1.png", 300), // oldest
            entry("clipboard-2.png", 200),
            entry("clipboard-3.png", 100), // newest
        ];
        let result = files_to_clean_by_count(&entries, 2);
        assert_eq!(result, vec![PathBuf::from("/tmp/clipboard-1.png")]);
    }

    #[test]
    fn count_cleanup_empty_when_under_limit() {
        let entries = vec![entry("clipboard-1.png", 100)];
        assert!(files_to_clean_by_count(&entries, 5).is_empty());
    }

    #[test]
    fn is_clipboard_png_matches() {
        assert!(is_clipboard_png("clipboard-12345.png"));
        assert!(is_clipboard_png("clipboard-20260406-120000.png"));
    }

    #[test]
    fn is_clipboard_png_rejects_non_matching() {
        assert!(!is_clipboard_png("other.png"));
        assert!(!is_clipboard_png("clipboard-12345.jpg"));
        assert!(!is_clipboard_png("test-clipboard-12345.png"));
    }
}
