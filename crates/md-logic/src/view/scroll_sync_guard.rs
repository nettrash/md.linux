//! Port of md.win `View/ScrollSyncGuard.cs`.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::seams::Clock;

/// "Is this scroll event our own echo?" for the editor half of the sync (shell-design §3.4).
/// AppKit posts the bounds-changed notification synchronously, so the Mac needs nothing but a flag
/// around the programmatic scroll. On GTK the editor's `GtkAdjustment::value-changed` fires inside
/// `set_value` (synchronously, like the Mac), so the plain flag is the shipping configuration; a
/// kinetic or animated scroll that reported later would let the echo through, and the timestamp
/// window is the contingency for that — the same shape as the 300 ms the preview's injected
/// script already uses.
pub struct ScrollSyncGuard {
    clock: Rc<dyn Clock>,
    window: Duration,
    last_applied: Cell<Option<Instant>>,
    depth: Cell<usize>,
}

impl fmt::Debug for ScrollSyncGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScrollSyncGuard").field("window", &self.window).field("depth", &self.depth.get()).finish()
    }
}

impl ScrollSyncGuard {
    /// 50 ms — long enough for a deferred `value-changed`, far short of a human scroll gesture.
    pub const DEFAULT_WINDOW: Duration = Duration::from_millis(50);

    /// The plain flag (no window) — the Mac's behaviour and what the app ships with.
    pub fn new(clock: Rc<dyn Clock>) -> Self {
        Self::with_window(clock, Duration::ZERO)
    }

    /// `window` = zero reduces the guard to the plain flag.
    pub fn with_window(clock: Rc<dyn Clock>, window: Duration) -> Self {
        ScrollSyncGuard { clock, window, last_applied: Cell::new(None), depth: Cell::new(0) }
    }

    /// True while a programmatic scroll is being applied.
    pub fn is_applying(&self) -> bool {
        self.depth.get() > 0
    }

    /// Wrap the `set_value` call: `let _scope = guard.applying(); adjustment.set_value(…);`.
    /// Re-entrant, so a nested apply cannot clear the flag early.
    pub fn applying(&self) -> ApplyingScope<'_> {
        self.depth.set(self.depth.get() + 1);
        self.last_applied.set(Some(self.clock.now()));
        ApplyingScope { owner: self }
    }

    /// True when a `value-changed` arriving now is our own doing: either the apply is still in
    /// flight, or it finished inside the window.
    pub fn is_echo(&self) -> bool {
        if self.depth.get() > 0 {
            return true;
        }
        match self.last_applied.get() {
            Some(applied) if !self.window.is_zero() => {
                self.clock.now().saturating_duration_since(applied) < self.window
            }
            _ => false,
        }
    }
}

/// The scope [`ScrollSyncGuard::applying`] returns: dropping it ends the apply and stamps the time
/// the window is measured from. (md.win's `IDisposable` may be disposed twice; a Rust value is
/// dropped exactly once.)
#[must_use = "the apply ends when the scope is dropped"]
pub struct ApplyingScope<'a> {
    owner: &'a ScrollSyncGuard,
}

impl fmt::Debug for ApplyingScope<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApplyingScope")
    }
}

impl Drop for ApplyingScope<'_> {
    fn drop(&mut self) {
        self.owner.depth.set(self.owner.depth.get() - 1);
        self.owner.last_applied.set(Some(self.owner.clock.now()));
    }
}
