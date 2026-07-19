//! Event-driven clipboard change signaling via X11 XFixes.
//!
//! WSLg mirrors every Windows-side copy to the X11 CLIPBOARD selection, and
//! XWayland delivers `XFixesSelectionNotify` on each owner change. Subscribing
//! to that event replaces polling entirely. The Wayland-native route
//! (`wl-paste --watch`) is not an option: WSLg's compositor does not implement
//! the data-control protocol it requires.
//!
//! Read-only by design: we subscribe to owner-change notifications only and
//! never fetch selection contents through X11 — data still flows via wl-paste.

use std::fmt;
use std::thread;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{self, ConnectionExt as _};
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

/// How long to wait after the first event before draining the burst.
/// A single Windows-side copy can set the clipboard twice in quick succession
/// (Snipping Tool measured at ~145ms apart); without settling, one copy would
/// trigger two conversions.
const SETTLE: Duration = Duration::from_millis(200);

/// Upper bound on events absorbed per settle window.
const DRAIN_LIMIT: usize = 64;

/// Error from the change-signal channel (connection setup or event wait).
#[derive(Debug)]
pub struct SignalError(String);

impl fmt::Display for SignalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl SignalError {
    pub(crate) fn new(context: &str, err: impl fmt::Display) -> Self {
        Self(format!("{context}: {err}"))
    }
}

/// Blocks until the clipboard changes.
pub trait ChangeSignal {
    /// Wait for the next clipboard change. Coalesces event bursts belonging
    /// to a single copy operation into one notification.
    fn wait_change(&mut self) -> Result<(), SignalError>;

    /// Discard queued change events without blocking. Used before a
    /// catch-up snapshot: content those events announce is about to be
    /// observed directly, so acting on them afterwards would double-convert.
    fn drain_pending(&mut self) -> Result<(), SignalError>;
}

/// XFixes-based implementation listening on the X11 CLIPBOARD selection.
pub struct X11ChangeSignal {
    conn: RustConnection,
}

impl X11ChangeSignal {
    /// Connect to the X server and subscribe to CLIPBOARD owner changes.
    ///
    /// Fails when no X server is reachable or XFixes is unavailable — the
    /// caller is expected to fall back to polling.
    pub fn connect() -> Result<Self, SignalError> {
        // Not just connect(None): systemd user services don't inherit
        // $DISPLAY from the login session, so under the shipped unit the
        // variable is unset and event mode would silently degrade to
        // polling. WSLg's X display is always :0 and this tool refuses to
        // run outside WSL2, so a fixed fallback display is sound.
        let (conn, screen_num) = x11rb::connect(None)
            .or_else(|_| x11rb::connect(Some(":0")))
            .map_err(|e| SignalError::new("X11 connect failed", e))?;
        let root = conn.setup().roots[screen_num].root;

        // Version negotiation is mandatory before any other XFixes request.
        conn.xfixes_query_version(5, 0)
            .map_err(|e| SignalError::new("XFixes version request failed", e))?
            .reply()
            .map_err(|e| SignalError::new("XFixes not available", e))?;

        let clipboard = conn
            .intern_atom(false, b"CLIPBOARD")
            .map_err(|e| SignalError::new("intern_atom failed", e))?
            .reply()
            .map_err(|e| SignalError::new("intern_atom failed", e))?
            .atom;

        // check() (not just flush) — a flush only sends the request, so a
        // server-side rejection would surface as an eternal wait_for_event.
        conn.xfixes_select_selection_input(
            root,
            clipboard,
            xfixes::SelectionEventMask::SET_SELECTION_OWNER
                | xfixes::SelectionEventMask::SELECTION_WINDOW_DESTROY
                | xfixes::SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )
        .map_err(|e| SignalError::new("selection subscribe failed", e))?
        .check()
        .map_err(|e| SignalError::new("selection subscribe rejected", e))?;

        Ok(Self { conn })
    }
}

impl X11ChangeSignal {
    /// Discard currently queued events. Bounded so a pathological event
    /// flood cannot pin the loop; the handler reads the latest clipboard
    /// state anyway, so leftover events only cause one extra (harmless)
    /// wakeup.
    fn drain_queued(&mut self) -> Result<(), SignalError> {
        for _ in 0..DRAIN_LIMIT {
            let drained = self
                .conn
                .poll_for_event()
                .map_err(|e| SignalError::new("event poll failed", e))?;
            if drained.is_none() {
                break;
            }
        }
        Ok(())
    }
}

impl ChangeSignal for X11ChangeSignal {
    fn wait_change(&mut self) -> Result<(), SignalError> {
        loop {
            let event = self
                .conn
                .wait_for_event()
                .map_err(|e| SignalError::new("event wait failed", e))?;
            if matches!(event, Event::XfixesSelectionNotify(_)) {
                break;
            }
        }

        // Coalesce the burst: absorb follow-up events from the same copy.
        thread::sleep(SETTLE);
        self.drain_queued()
    }

    fn drain_pending(&mut self) -> Result<(), SignalError> {
        self.drain_queued()
    }
}
