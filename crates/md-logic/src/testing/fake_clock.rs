//! Port of md.win `FakeClock`.

use std::cell::Cell;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::seams::Clock;

/// A clock that moves only when told. Shared (as an `Rc`) by [`FakeScheduler`] and
/// [`FakeFileSystem`] when a test hands both the same instance.
///
/// md.win's clock is one `DateTimeOffset`; the Rust seams need two readings of it: the monotonic
/// [`Clock::now`] (an `Instant`, for the debounces) and the wall-clock [`FakeClock::wall_now`] (a
/// `SystemTime`, for file stamps). Both are the epoch plus [`FakeClock::elapsed`], so they always
/// move together.
///
/// [`FakeScheduler`]: super::FakeScheduler
/// [`FakeFileSystem`]: super::FakeFileSystem
pub struct FakeClock {
    start: Instant,
    elapsed: Cell<Duration>,
}

impl Default for FakeClock {
    fn default() -> Self {
        FakeClock { start: Instant::now(), elapsed: Cell::new(Duration::ZERO) }
    }
}

impl std::fmt::Debug for FakeClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeClock").field("elapsed", &self.elapsed.get()).finish()
    }
}

impl FakeClock {
    /// 2026-01-01 12:00:00 UTC as seconds since the Unix epoch — md.win's `FakeClock.Epoch`.
    pub const EPOCH_UNIX_SECONDS: u64 = 1_767_268_800;

    pub fn new() -> Self {
        Self::default()
    }

    /// The wall-clock epoch, 2026-01-01 12:00:00 UTC.
    pub fn epoch() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(Self::EPOCH_UNIX_SECONDS)
    }

    /// The `Instant` this clock reads at the epoch (its [`Clock::now`] before any advance).
    pub fn start(&self) -> Instant {
        self.start
    }

    /// How far the clock has moved since the epoch.
    pub fn elapsed(&self) -> Duration {
        self.elapsed.get()
    }

    /// Moves the clock to `elapsed` after the epoch — md.win's `Now` setter (it may go back).
    pub fn set_elapsed(&self, elapsed: Duration) {
        self.elapsed.set(elapsed);
    }

    pub fn advance(&self, by: Duration) {
        self.elapsed.set(self.elapsed.get() + by);
    }

    /// The wall clock: [`FakeClock::epoch`] + [`FakeClock::elapsed`].
    pub fn wall_now(&self) -> SystemTime {
        Self::epoch() + self.elapsed.get()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.start + self.elapsed.get()
    }
}
