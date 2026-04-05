//! Debounce logic for self-trigger prevention.
//!
//! After writing a path to the clipboard, the next clipboard change event
//! should be ignored to prevent an infinite loop.

/// Check if enough time has elapsed since last write to consider a new event.
///
/// Pure function: compares timestamps, returns whether the event should be processed.
pub fn should_process_event(
    last_write_timestamp_ms: Option<u64>,
    current_timestamp_ms: u64,
    debounce_ms: u64,
) -> bool {
    match last_write_timestamp_ms {
        None => true,
        Some(last) => {
            if current_timestamp_ms < last {
                // Clock went backwards — treat as processable
                true
            } else {
                current_timestamp_ms - last >= debounce_ms
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_previous_write_allows_processing() {
        assert!(should_process_event(None, 1000, 500));
    }

    #[test]
    fn within_debounce_window_blocks_processing() {
        assert!(!should_process_event(Some(1000), 1200, 500));
    }

    #[test]
    fn after_debounce_window_allows_processing() {
        assert!(should_process_event(Some(1000), 1500, 500));
    }

    #[test]
    fn exactly_at_debounce_boundary_allows_processing() {
        assert!(should_process_event(Some(1000), 1500, 500));
    }

    #[test]
    fn clock_went_backwards_allows_processing() {
        assert!(should_process_event(Some(2000), 1000, 500));
    }

    #[test]
    fn zero_debounce_always_allows() {
        assert!(should_process_event(Some(1000), 1000, 0));
    }
}
