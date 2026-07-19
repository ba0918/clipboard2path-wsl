//! Reconnect backoff schedule (pure function).

use std::time::Duration;

const BASE_SECS: u64 = 1;
const CAP_SECS: u64 = 60;

/// Delay before the reconnect attempt number `attempt` (0-based):
/// 1s, 2s, 4s, ... capped at 60s.
pub fn next_backoff(attempt: u32) -> Duration {
    // checked_shl instead of `<<`: attempt is caller-controlled and grows
    // unbounded while the connection stays down; a plain shift would overflow.
    let secs = BASE_SECS
        .checked_shl(attempt)
        .map_or(CAP_SECS, |s| s.min(CAP_SECS));
    Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_from_one_second() {
        assert_eq!(next_backoff(0), Duration::from_secs(1));
        assert_eq!(next_backoff(1), Duration::from_secs(2));
        assert_eq!(next_backoff(2), Duration::from_secs(4));
        assert_eq!(next_backoff(5), Duration::from_secs(32));
    }

    #[test]
    fn caps_at_sixty_seconds() {
        assert_eq!(next_backoff(6), Duration::from_secs(60));
        assert_eq!(next_backoff(30), Duration::from_secs(60));
    }

    #[test]
    fn extreme_attempts_do_not_overflow() {
        assert_eq!(next_backoff(u32::MAX), Duration::from_secs(60));
    }
}
