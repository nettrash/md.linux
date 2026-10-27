//! The book exemption (md.win `BookArticleOpens`, the last part of `Document/ViewMode.cs`).
//!
//! Writer mode opens an article through the same path as any document, so the navigator marks the
//! file just before opening it and the window that lands on it claims the mark and becomes exempt
//! from the per-file memory both ways — sticky for the window's life, Save As included (Android
//! clears its flag on Save As; the Mac is the source of truth). A mark older than
//! [`MARK_LIFETIME`] is discarded: re-activating the already-open article never fires the claim,
//! and a stale mark would silently exempt the next ordinary open of that file.
//!
//! md.win keeps this as a static class with a settable static clock. Here it is a value the app
//! owns ONE of (it must be shared by the book window that marks and the document window that
//! claims), so tests each get their own and run in parallel; the clock is injected per instance.
//! The clock is monotonic ([`Instant`]) rather than md.win's UTC wall clock: a 10-second expiry
//! must not jump when the system time is set.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::view_mode_memory;

/// How long a mark may wait for the window that claims it.
pub const MARK_LIFETIME: Duration = Duration::from_secs(10);

/// The pending marks, keyed by [`view_mode_memory::identity_for`] of the article path.
pub struct BookArticleOpens {
    pending: HashMap<String, Instant>,
    now: Box<dyn Fn() -> Instant + Send + Sync>,
}

impl Default for BookArticleOpens {
    fn default() -> Self {
        Self::new()
    }
}

impl BookArticleOpens {
    /// Marks timed by the real monotonic clock.
    pub fn new() -> Self {
        Self::with_clock(Instant::now)
    }

    /// Marks timed by `now` — injectable so the expiry is testable without sleeping.
    pub fn with_clock(now: impl Fn() -> Instant + Send + Sync + 'static) -> Self {
        BookArticleOpens { pending: HashMap::new(), now: Box::new(now) }
    }

    /// Called just before writer mode opens an article.
    pub fn mark(&mut self, path: &str) {
        self.drop_expired();
        let now = (self.now)();
        self.pending.insert(view_mode_memory::identity_for(path), now);
    }

    /// True when this open came from the book. Consumes the mark, so the next open of the same
    /// file is ordinary.
    pub fn claim_open(&mut self, path: &str) -> bool {
        self.drop_expired();
        self.pending.remove(&view_mode_memory::identity_for(path)).is_some()
    }

    pub fn reset(&mut self) {
        self.pending.clear();
    }

    /// A mark exactly at the lifetime is gone: Swift keeps `pending.filter { $0.value > cutoff }`.
    fn drop_expired(&mut self) {
        let now = (self.now)();
        self.pending.retain(|_, marked| now.saturating_duration_since(*marked) < MARK_LIFETIME);
    }
}
