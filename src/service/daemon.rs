//! Daemon poll loop logic.

use std::path::Path;

use crate::domain::clipboard_change;
use crate::infra::clipboard::ClipboardReader;
use crate::infra::file_system::FileWriter;
use crate::infra::path_notifier::PathNotifier;
use crate::service::converter::{ConvertService, TimestampProvider};

/// Result of a single poll iteration.
#[derive(Debug, PartialEq)]
pub enum PollResult {
    /// Clipboard changed and conversion succeeded.
    Converted(std::path::PathBuf),
    /// Clipboard changed but no BMP image present — skipped.
    NoBmpImage,
    /// Clipboard unchanged — no action taken.
    NoChange,
    /// Conversion failed.
    ConvertError(String),
    /// Clipboard read error.
    ClipboardError(String),
}

/// Execute a single poll iteration.
///
/// This is the core logic extracted as a testable function.
/// The daemon loop calls this repeatedly.
///
/// No debounce needed — we no longer write to the clipboard,
/// so self-triggering cannot occur.
pub fn poll_once<C, F, T, N>(
    service: &ConvertService<C, F, T, N>,
    previous_types: &mut Vec<String>,
    base_dir: &Path,
) -> PollResult
where
    C: ClipboardReader,
    F: FileWriter,
    T: TimestampProvider,
    N: PathNotifier,
{
    // 1. List current types
    let current_types = match service.reader().list_types() {
        Ok(types) => types,
        Err(e) => {
            // On error, keep previous_types unchanged (no clone needed)
            return PollResult::ClipboardError(e.to_string());
        }
    };

    // 2. Check for change
    if !clipboard_change::has_clipboard_changed(previous_types, &current_types) {
        previous_types.clear();
        previous_types.extend(current_types);
        return PollResult::NoChange;
    }

    // 3. Handle the changed clipboard
    let result = handle_changed_clipboard(service, &current_types, base_dir);

    previous_types.clear();
    previous_types.extend(current_types);
    result
}

/// Process one clipboard change reported by an event-driven signal.
///
/// Skips the MIME-type comparison on purpose: two consecutive screenshot
/// copies produce identical type lists, so comparing would miss the second
/// one. The owner-change event itself is the evidence that a copy happened.
pub fn on_change_event<C, F, T, N>(
    service: &ConvertService<C, F, T, N>,
    base_dir: &Path,
) -> PollResult
where
    C: ClipboardReader,
    F: FileWriter,
    T: TimestampProvider,
    N: PathNotifier,
{
    let current_types = match service.reader().list_types() {
        Ok(types) => types,
        Err(e) => return PollResult::ClipboardError(e.to_string()),
    };

    handle_changed_clipboard(service, &current_types, base_dir)
}

/// Convert the clipboard content, or clear the notification when it holds
/// no BMP image. Shared tail of the polling and event-driven paths.
fn handle_changed_clipboard<C, F, T, N>(
    service: &ConvertService<C, F, T, N>,
    current_types: &[String],
    base_dir: &Path,
) -> PollResult
where
    C: ClipboardReader,
    F: FileWriter,
    T: TimestampProvider,
    N: PathNotifier,
{
    if !clipboard_change::has_bmp_image(current_types) {
        let _ = service.clear_notification();
        return PollResult::NoBmpImage;
    }

    match service.convert_once(base_dir) {
        Ok(path) => PollResult::Converted(path),
        Err(e) => PollResult::ConvertError(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::converter::ConvertService;
    use crate::service::test_helpers::*;

    // --- Tests ---

    #[test]
    fn poll_once_converts_when_bmp_present_and_changed() {
        let bmp = make_1x1_bmp();
        let service = ConvertService::new(
            MockClipboardReader {
                types: vec!["image/bmp".to_string()],
                bmp_data: bmp,
            },
            MockFileWriter,
            FixedTimestamp("1".into()),
            MockPathNotifier,
        );

        let mut prev = Vec::new();
        let result = poll_once(&service, &mut prev, Path::new("/tmp"));

        assert!(matches!(result, PollResult::Converted(_)));
        assert_eq!(prev, vec!["image/bmp".to_string()]);
    }

    #[test]
    fn poll_once_skips_when_no_change() {
        let service = ConvertService::new(
            MockClipboardReader {
                types: vec!["image/bmp".to_string()],
                bmp_data: vec![],
            },
            MockFileWriter,
            FixedTimestamp("1".into()),
            MockPathNotifier,
        );

        let mut prev = vec!["image/bmp".to_string()];
        let result = poll_once(&service, &mut prev, Path::new("/tmp"));

        assert_eq!(result, PollResult::NoChange);
    }

    #[test]
    fn poll_once_skips_when_no_bmp() {
        let service = ConvertService::new(
            MockClipboardReader {
                types: vec!["text/plain".to_string()],
                bmp_data: vec![],
            },
            MockFileWriter,
            FixedTimestamp("1".into()),
            MockPathNotifier,
        );

        let mut prev = Vec::new();
        let result = poll_once(&service, &mut prev, Path::new("/tmp"));

        assert_eq!(result, PollResult::NoBmpImage);
        assert_eq!(prev, vec!["text/plain".to_string()]);
    }

    #[test]
    fn on_change_event_converts_consecutive_copies_with_identical_types() {
        let bmp = make_1x1_bmp();
        let service = ConvertService::new(
            MockClipboardReader {
                types: vec!["image/bmp".to_string()],
                bmp_data: bmp,
            },
            MockFileWriter,
            FixedTimestamp("1".into()),
            MockPathNotifier,
        );

        // Two events with identical type lists = two separate copies
        // (e.g. screenshot A then screenshot B). Both must convert.
        let first = on_change_event(&service, Path::new("/tmp"));
        let second = on_change_event(&service, Path::new("/tmp"));

        assert!(matches!(first, PollResult::Converted(_)));
        assert!(matches!(second, PollResult::Converted(_)));
    }

    #[test]
    fn on_change_event_skips_when_clipboard_holds_no_image() {
        let service = ConvertService::new(
            MockClipboardReader {
                types: vec!["text/plain".to_string()],
                bmp_data: vec![],
            },
            MockFileWriter,
            FixedTimestamp("1".into()),
            MockPathNotifier,
        );

        let result = on_change_event(&service, Path::new("/tmp"));

        assert_eq!(result, PollResult::NoBmpImage);
    }

    #[test]
    fn poll_once_reuses_buffer() {
        let bmp = make_1x1_bmp();
        let service = ConvertService::new(
            MockClipboardReader {
                types: vec!["image/bmp".to_string()],
                bmp_data: bmp,
            },
            MockFileWriter,
            FixedTimestamp("1".into()),
            MockPathNotifier,
        );

        let mut prev = Vec::with_capacity(16);
        let ptr_before = prev.as_ptr();
        let _ = poll_once(&service, &mut prev, Path::new("/tmp"));

        // After first poll, buffer is populated
        assert_eq!(prev, vec!["image/bmp".to_string()]);

        // Second poll with same types: NoChange, buffer updated in-place
        let _ = poll_once(&service, &mut prev, Path::new("/tmp"));
        let ptr_after = prev.as_ptr();

        // Buffer pointer should be the same (reused, not reallocated)
        assert_eq!(ptr_before, ptr_after);
    }
}
