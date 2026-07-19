//! Watch-mode orchestration: an event ⇄ polling state machine.
//!
//! The daemon prefers event-driven detection (X11 XFixes) and degrades to
//! polling when X11 is unreachable — but unlike a one-way fallback, it keeps
//! retrying the connection with capped exponential backoff, because the
//! polling detector cannot see consecutive copies with identical MIME type
//! lists. Staying in polling would degrade correctness, not just latency.
//!
//! All I/O flows through injected closures (connector / sleeper / observer),
//! so every transition is testable without an X server or a real clock.

use std::ops::ControlFlow;
use std::path::Path;
use std::time::Duration;

use crate::domain::backoff::next_backoff;
use crate::infra::change_signal::{ChangeSignal, SignalError};
use crate::infra::clipboard::ClipboardReader;
use crate::infra::file_system::FileWriter;
use crate::infra::path_notifier::PathNotifier;
use crate::service::converter::{ConvertService, TimestampProvider};
use crate::service::daemon::{self, PollResult};

/// Notification to the observer. The service layer never prints; the caller
/// (main.rs) turns these into log lines and runs post-conversion cleanup.
pub enum WatchNotice<'a> {
    /// Event-driven mode established (initial connect or reconnect).
    EventModeEntered,
    /// The event connection died; polling takes over.
    EventSignalLost(&'a SignalError),
    /// An event-mode connection attempt failed (initial or retry).
    ConnectFailed(&'a SignalError),
    /// Polling mode begins (fires once per polling episode, not per retry).
    PollingModeEntered,
    /// One observation of the clipboard completed.
    Polled(&'a PollResult),
}

/// Run the watch loop until the observer returns `ControlFlow::Break`.
///
/// `connector` opens a fresh event subscription; `sleeper` performs the
/// poll-interval waits (elapsed time is accounted from the durations passed
/// to it — no wall clock is read, keeping the schedule testable).
pub fn run_watch_loop<C, F, T, N, S>(
    service: &ConvertService<C, F, T, N>,
    base_dir: &Path,
    poll_interval: Duration,
    force_poll: bool,
    mut connector: impl FnMut() -> Result<S, SignalError>,
    mut sleeper: impl FnMut(Duration),
    mut observer: impl FnMut(WatchNotice<'_>) -> ControlFlow<()>,
) where
    C: ClipboardReader,
    F: FileWriter,
    T: TimestampProvider,
    N: PathNotifier,
    S: ChangeSignal,
{
    let mut carry: Vec<String> = Vec::new();
    // The very first observation skips the type comparison regardless of
    // mode: with an empty carry a comparison would report NoChange for an
    // empty clipboard and leave a stale latest-path from a previous run.
    let mut first_observation = true;
    let mut attempt: u32 = 0;
    let mut in_polling = false;

    loop {
        if !force_poll {
            match connector() {
                Ok(mut signal) => {
                    in_polling = false;
                    if observer(WatchNotice::EventModeEntered).is_break() {
                        return;
                    }

                    // Drain queued events before the catch-up snapshot: an
                    // event fired pre-snapshot would re-convert content the
                    // snapshot already handles. Draining first biases the
                    // unavoidable subscribe race toward a duplicate save,
                    // never a missed copy.
                    let mut lost: Option<SignalError> = signal.drain_pending().err();

                    if lost.is_none() {
                        // Catch-up: compare against the carried state so
                        // content already handled by the previous mode is
                        // not saved twice. A clipboard read error here is
                        // transient — the X11 connection is healthy, so
                        // stay in event mode.
                        let result = observe_once(
                            service,
                            &mut carry,
                            base_dir,
                            &mut first_observation,
                            true,
                        );
                        if observer(WatchNotice::Polled(&result)).is_break() {
                            return;
                        }

                        loop {
                            match signal.wait_change() {
                                Ok(()) => {
                                    // A delivered event proves the subscription
                                    // works — only now reset the backoff. A
                                    // reset on connect would pin a flapping
                                    // connection at the shortest delay.
                                    attempt = 0;
                                    let result = observe_once(
                                        service,
                                        &mut carry,
                                        base_dir,
                                        &mut first_observation,
                                        false,
                                    );
                                    if observer(WatchNotice::Polled(&result)).is_break() {
                                        return;
                                    }
                                }
                                Err(e) => {
                                    lost = Some(e);
                                    break;
                                }
                            }
                        }
                    }

                    let e = lost.expect("event mode exits only on a signal error");
                    if observer(WatchNotice::EventSignalLost(&e)).is_break() {
                        return;
                    }
                }
                Err(e) => {
                    if observer(WatchNotice::ConnectFailed(&e)).is_break() {
                        return;
                    }
                }
            }
        }

        if !in_polling {
            in_polling = true;
            if observer(WatchNotice::PollingModeEntered).is_break() {
                return;
            }
        }

        let backoff = next_backoff(attempt);
        attempt = attempt.saturating_add(1);
        let mut elapsed = Duration::ZERO;
        loop {
            let result = observe_once(service, &mut carry, base_dir, &mut first_observation, true);
            if observer(WatchNotice::Polled(&result)).is_break() {
                return;
            }
            sleeper(poll_interval);
            elapsed += poll_interval;
            // Evaluated after the poll: even when the interval exceeds the
            // backoff, the clipboard stays watched between reconnects.
            if !force_poll && elapsed >= backoff {
                break;
            }
        }
    }
}

/// One clipboard observation. `compare` selects the polling semantics
/// (skip when the type list is unchanged); the first observation of the
/// process is always comparison-free (see `run_watch_loop`).
fn observe_once<C, F, T, N>(
    service: &ConvertService<C, F, T, N>,
    carry: &mut Vec<String>,
    base_dir: &Path,
    first_observation: &mut bool,
    compare: bool,
) -> PollResult
where
    C: ClipboardReader,
    F: FileWriter,
    T: TimestampProvider,
    N: PathNotifier,
{
    let result = if compare && !*first_observation {
        daemon::poll_once(service, carry, base_dir)
    } else {
        daemon::process_event(service, carry, base_dir)
    };
    *first_observation = false;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::converter::ConvertService;
    use crate::service::test_helpers::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    // --- Scripted mocks -------------------------------------------------

    /// One scripted reaction of a signal's wait_change call.
    enum WaitStep {
        /// Report a change event (the shared clipboard already holds the
        /// content it announces).
        Change,
        /// The connection dies.
        Lose,
    }

    struct ScriptedSignal {
        waits: VecDeque<WaitStep>,
        wait_calls: Rc<RefCell<usize>>,
    }

    impl ChangeSignal for ScriptedSignal {
        fn wait_change(&mut self) -> Result<(), SignalError> {
            *self.wait_calls.borrow_mut() += 1;
            match self.waits.pop_front() {
                Some(WaitStep::Change) => Ok(()),
                Some(WaitStep::Lose) | None => Err(SignalError::new("mock", "connection lost")),
            }
        }

        fn drain_pending(&mut self) -> Result<(), SignalError> {
            Ok(())
        }
    }

    /// One scripted connector outcome: optionally mutate the clipboard on
    /// success (simulating a copy that happened while polling).
    type ConnectStep = Result<(Option<Vec<String>>, ScriptedSignal), ()>;

    /// Owned mirror of WatchNotice for recording (the real enum borrows).
    fn notice_label(notice: &WatchNotice<'_>) -> String {
        match notice {
            WatchNotice::EventModeEntered => "event".to_string(),
            WatchNotice::EventSignalLost(_) => "lost".to_string(),
            WatchNotice::ConnectFailed(_) => "connect-failed".to_string(),
            WatchNotice::PollingModeEntered => "polling".to_string(),
            WatchNotice::Polled(r) => match r {
                PollResult::Converted(_) => "converted".to_string(),
                PollResult::NoChange => "no-change".to_string(),
                PollResult::NoBmpImage => "no-bmp".to_string(),
                PollResult::ClipboardError(_) => "clip-error".to_string(),
                PollResult::ConvertError(_) => "convert-error".to_string(),
            },
        }
    }

    struct Harness {
        clipboard: Rc<RefCell<Vec<String>>>,
        connects: Rc<RefCell<VecDeque<ConnectStep>>>,
        connect_calls: Rc<RefCell<usize>>,
        wait_calls: Rc<RefCell<usize>>,
        notices: Rc<RefCell<Vec<String>>>,
        notifier_events: Rc<RefCell<Vec<String>>>,
    }

    impl Harness {
        fn new(initial_types: &[&str]) -> Self {
            Self {
                clipboard: Rc::new(RefCell::new(
                    initial_types.iter().map(|s| s.to_string()).collect(),
                )),
                connects: Rc::new(RefCell::new(VecDeque::new())),
                connect_calls: Rc::new(RefCell::new(0)),
                wait_calls: Rc::new(RefCell::new(0)),
                notices: Rc::new(RefCell::new(Vec::new())),
                notifier_events: Rc::new(RefCell::new(Vec::new())),
            }
        }

        fn signal(&self, waits: Vec<WaitStep>) -> ScriptedSignal {
            ScriptedSignal {
                waits: waits.into(),
                wait_calls: Rc::clone(&self.wait_calls),
            }
        }

        fn push_connect(&self, result: Result<ScriptedSignal, ()>) {
            self.connects
                .borrow_mut()
                .push_back(result.map(|s| (None, s)));
        }

        /// Queue a successful connect that first mutates the clipboard —
        /// simulating a copy that happened while the daemon was polling.
        fn push_connect_with_types(&self, types: &[&str], signal: ScriptedSignal) {
            self.connects.borrow_mut().push_back(Ok((
                Some(types.iter().map(|s| s.to_string()).collect()),
                signal,
            )));
        }

        /// Run the loop until `max_notices` notices were recorded.
        fn run(&self, force_poll: bool, interval_ms: u64, max_notices: usize) {
            let service = ConvertService::new(
                SharedClipboardReader {
                    types: Rc::clone(&self.clipboard),
                    bmp_data: make_1x1_bmp(),
                },
                MockFileWriter,
                FixedTimestamp("1".into()),
                RecordingPathNotifier {
                    events: Rc::clone(&self.notifier_events),
                },
            );

            let connects = Rc::clone(&self.connects);
            let connect_calls = Rc::clone(&self.connect_calls);
            let notices = Rc::clone(&self.notices);

            run_watch_loop(
                &service,
                Path::new("/tmp"),
                Duration::from_millis(interval_ms),
                force_poll,
                {
                    let clipboard = Rc::clone(&self.clipboard);
                    move || {
                        *connect_calls.borrow_mut() += 1;
                        match connects.borrow_mut().pop_front() {
                            Some(Ok((new_types, signal))) => {
                                if let Some(types) = new_types {
                                    *clipboard.borrow_mut() = types;
                                }
                                Ok(signal)
                            }
                            Some(Err(())) | None => {
                                Err(SignalError::new("mock", "connect refused"))
                            }
                        }
                    }
                },
                |_| {},
                move |notice| {
                    let mut log = notices.borrow_mut();
                    log.push(notice_label(&notice));
                    if log.len() >= max_notices {
                        ControlFlow::Break(())
                    } else {
                        ControlFlow::Continue(())
                    }
                },
            );
        }

        fn notices(&self) -> Vec<String> {
            self.notices.borrow().clone()
        }

        /// Count of polls between consecutive connect attempts, derived from
        /// the notice log: polls after a "connect-failed" until the next one.
        fn polls_between_connect_failures(&self) -> Vec<usize> {
            let notices = self.notices();
            let mut counts = Vec::new();
            let mut current: Option<usize> = None;
            for n in &notices {
                match n.as_str() {
                    "connect-failed" => {
                        if let Some(c) = current.take() {
                            counts.push(c);
                        }
                        current = Some(0);
                    }
                    "converted" | "no-change" | "no-bmp" | "clip-error" | "convert-error" => {
                        if let Some(c) = current.as_mut() {
                            *c += 1;
                        }
                    }
                    _ => {}
                }
            }
            counts
        }
    }

    // --- Tests ----------------------------------------------------------

    #[test]
    fn connect_failure_enters_polling_once_and_keeps_watching() {
        let h = Harness::new(&["image/bmp"]);
        // Connect script empty → every attempt fails.
        h.run(false, 500, 12);

        let notices = h.notices();
        assert_eq!(notices[0], "connect-failed");
        assert_eq!(notices[1], "polling");
        // Polling banner fires once per episode, not per failed retry.
        assert_eq!(notices.iter().filter(|n| *n == "polling").count(), 1);
        // The clipboard kept being observed between retries.
        assert!(notices.iter().filter(|n| *n == "no-change").count() >= 2);
        assert!(*h.connect_calls.borrow() >= 2);
    }

    #[test]
    fn startup_with_event_mode_converts_current_clipboard() {
        let h = Harness::new(&["image/bmp"]);
        h.push_connect(Ok(h.signal(vec![WaitStep::Change])));
        h.run(false, 500, 3);

        // Banner first (log order), then the comparison-free startup
        // observation converts the pre-existing image, then the event-driven
        // conversion for the identical type list (blind-spot fix).
        assert_eq!(h.notices(), vec!["event", "converted", "converted"]);
    }

    #[test]
    fn startup_with_empty_clipboard_clears_stale_notification() {
        let h = Harness::new(&[]);
        h.run(true, 500, 2);

        assert_eq!(h.notices()[0], "polling");
        assert_eq!(h.notices()[1], "no-bmp");
        assert_eq!(h.notifier_events.borrow().as_slice(), ["clear"]);
    }

    #[test]
    fn signal_loss_falls_back_and_reconnects() {
        let h = Harness::new(&["image/bmp"]);
        h.push_connect(Ok(h.signal(vec![WaitStep::Lose])));
        h.push_connect(Ok(h.signal(vec![])));
        h.run(false, 500, 8);

        let notices = h.notices();
        // event → catch-up converted → lost → polling → (2 polls @ backoff 1s)
        // → reconnect → event again
        assert_eq!(
            &notices[..7],
            &[
                "event",
                "converted",
                "lost",
                "polling",
                "no-change",
                "no-change",
                "event"
            ]
        );
    }

    #[test]
    fn content_handled_by_event_mode_is_not_resaved_by_polling() {
        let h = Harness::new(&["image/bmp"]);
        h.push_connect(Ok(h.signal(vec![WaitStep::Change, WaitStep::Lose])));
        h.run(false, 500, 7);

        let notices = h.notices();
        // After the event conversions, every polling observation of the same
        // type list must be no-change (no duplicate save on the transition).
        assert_eq!(&notices[..4], &["event", "converted", "converted", "lost"]);
        assert!(
            notices[4..]
                .iter()
                .all(|n| n == "polling" || n == "no-change"),
            "unexpected notices after transition: {notices:?}"
        );
    }

    #[test]
    fn catch_up_detects_change_missed_while_polling() {
        let h = Harness::new(&["text/plain"]);
        // First connect fails → polling observes text. The clipboard turns
        // into an image right before the successful reconnect (i.e. while
        // the daemon was polling) — the catch-up comparison must pick it up.
        h.push_connect(Err(()));
        h.push_connect_with_types(&["image/bmp"], h.signal(vec![]));
        h.run(false, 500, 6);

        assert_eq!(
            h.notices(),
            // no-bmp = startup pass over the text clipboard (clears stale
            // path), then one no-change poll completes the 1s backoff.
            vec![
                "connect-failed",
                "polling",
                "no-bmp",
                "no-change",
                "event",
                "converted"
            ]
        );
    }

    #[test]
    fn backoff_grows_while_connects_keep_failing() {
        let h = Harness::new(&["image/bmp"]);
        h.run(false, 500, 20);

        // interval 500ms: backoff 1s → 2 polls, 2s → 4 polls, 4s → 8 polls
        let polls = h.polls_between_connect_failures();
        assert!(polls.len() >= 2, "not enough retry rounds: {polls:?}");
        assert_eq!(&polls[..2], &[2, 4]);
    }

    #[test]
    fn backoff_keeps_growing_while_connects_flap_without_events() {
        let h = Harness::new(&["image/bmp"]);
        // Flapping: connect succeeds but the signal dies before delivering
        // anything — the backoff must keep growing (a reset on connect
        // would pin the retry rate at the shortest delay).
        h.push_connect(Ok(h.signal(vec![WaitStep::Lose])));
        h.push_connect(Ok(h.signal(vec![WaitStep::Lose])));
        h.run(false, 500, 24);

        let expected = [
            "event",
            "converted", // startup pass
            "lost",
            "polling",
            "no-change", // attempt 0: backoff 1s → 2 polls @500ms
            "no-change",
            "event",     // 2nd flapping connect
            "no-change", // catch-up (unchanged types)
            "lost",
            "polling",
            "no-change", // attempt 1: backoff 2s → 4 polls
            "no-change",
            "no-change",
            "no-change",
            "connect-failed", // script exhausted
            "no-change",      // attempt 2: backoff 4s → 8 polls
            "no-change",
            "no-change",
            "no-change",
            "no-change",
            "no-change",
            "no-change",
            "no-change",
            "connect-failed",
        ];
        assert_eq!(h.notices(), expected);
    }

    #[test]
    fn backoff_resets_after_a_delivered_event() {
        let h = Harness::new(&["image/bmp"]);
        // Two failed connects grow the backoff to 2s, then a working
        // connection delivers one real event before dying. The next polling
        // round must be back at the shortest backoff (1s → 2 polls @500ms).
        h.push_connect(Err(()));
        h.push_connect(Err(()));
        h.push_connect(Ok(h.signal(vec![WaitStep::Change, WaitStep::Lose])));
        h.run(false, 500, 17);

        let expected = [
            "connect-failed",
            "polling",
            "converted", // startup pass
            "no-change", // attempt 0: backoff 1s → 2 polls
            "connect-failed",
            "no-change", // attempt 1: backoff 2s → 4 polls
            "no-change",
            "no-change",
            "no-change",
            "event",
            "no-change", // catch-up (unchanged types)
            "converted", // delivered event → backoff reset
            "lost",
            "polling",
            "no-change", // back to 2 polls: reset took effect
            "no-change",
            "connect-failed",
        ];
        assert_eq!(h.notices(), expected);
    }

    #[test]
    fn reconnect_waits_for_at_least_one_poll_when_interval_exceeds_backoff() {
        let h = Harness::new(&["image/bmp"]);
        // interval 2000ms > first backoff 1000ms
        h.run(false, 2000, 8);

        let rounds = h.polls_between_connect_failures();
        assert!(!rounds.is_empty(), "no retry rounds observed");
        for polls in rounds {
            assert!(polls >= 1, "connect retried without polling in between");
        }
    }

    #[test]
    fn clipboard_error_during_catch_up_stays_in_event_mode() {
        let h = Harness::new(&["image/bmp"]);
        h.push_connect(Ok(h.signal(vec![WaitStep::Change])));

        let service = ConvertService::new(
            FailingListClipboardReader,
            MockFileWriter,
            FixedTimestamp("1".into()),
            RecordingPathNotifier {
                events: Rc::clone(&h.notifier_events),
            },
        );
        let connects = Rc::clone(&h.connects);
        let notices = Rc::clone(&h.notices);
        run_watch_loop(
            &service,
            Path::new("/tmp"),
            Duration::from_millis(500),
            false,
            move || match connects.borrow_mut().pop_front() {
                Some(Ok((_, signal))) => Ok(signal),
                _ => Err(SignalError::new("mock", "connect refused")),
            },
            |_| {},
            move |notice| {
                let mut log = notices.borrow_mut();
                log.push(notice_label(&notice));
                if log.len() >= 4 {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        );

        // The failing catch-up did not tear down the connection: the next
        // observation is still event-driven (wait_change was consumed).
        assert_eq!(
            h.notices(),
            vec!["event", "clip-error", "clip-error", "lost"]
        );
        assert_eq!(*h.wait_calls.borrow(), 2);
    }

    #[test]
    fn force_poll_never_touches_the_connector() {
        let h = Harness::new(&["image/bmp"]);
        h.run(true, 500, 6);

        assert_eq!(*h.connect_calls.borrow(), 0);
        let notices = h.notices();
        assert_eq!(notices[0], "polling");
        assert_eq!(notices[1], "converted");
        assert!(notices[2..].iter().all(|n| n == "no-change"));
    }

    #[test]
    fn observer_break_stops_the_loop() {
        let h = Harness::new(&["image/bmp"]);
        h.run(true, 500, 1);

        assert_eq!(h.notices().len(), 1);
    }
}
