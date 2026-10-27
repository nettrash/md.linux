//! Port of md.win `FakeScheduler`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::fake_clock::FakeClock;
use crate::seams::{Clock, Scheduler, TimerHandle};

struct Timer {
    seq: u64,
    /// Due time as elapsed-since-epoch on the clock.
    due: Duration,
    action: Box<dyn FnOnce()>,
    cancelled: Rc<Cell<bool>>,
}

#[derive(Default)]
struct State {
    timers: Vec<Timer>,
    posted: VecDeque<Box<dyn FnOnce()>>,
    seq: u64,
}

/// [`Scheduler`] with manual time. [`FakeScheduler::advance`] fires every timer whose due time
/// falls inside the window, earliest first (registration order breaks ties), moving the clock to
/// each due time before running it — so a timer that a callback schedules is fired in the same
/// call when it also falls due, as a real main-loop timer would. Posted actions run only on
/// [`FakeScheduler::run_posted`], FIFO, including ones posted while draining.
///
/// Dropping (or cancelling) the [`TimerHandle`] marks the timer cancelled: it never fires and no
/// longer counts as pending. A detached handle lets it fire.
pub struct FakeScheduler {
    clock: Rc<FakeClock>,
    state: RefCell<State>,
}

impl Default for FakeScheduler {
    fn default() -> Self {
        Self::with_clock(Rc::new(FakeClock::new()))
    }
}

impl fmt::Debug for FakeScheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeScheduler")
            .field("elapsed", &self.clock.elapsed())
            .field("pending_timers", &self.pending_timers())
            .field("pending_posts", &self.pending_posts())
            .finish()
    }
}

impl FakeScheduler {
    /// A scheduler with its own clock.
    pub fn new() -> Self {
        Self::default()
    }

    /// A scheduler on a clock the test shares (with a [`FakeFileSystem`](super::FakeFileSystem),
    /// say).
    pub fn with_clock(clock: Rc<FakeClock>) -> Self {
        FakeScheduler { clock, state: RefCell::new(State::default()) }
    }

    pub fn clock(&self) -> &Rc<FakeClock> {
        &self.clock
    }

    /// The clock's monotonic reading.
    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    /// Timers that have neither fired nor been cancelled.
    pub fn pending_timers(&self) -> usize {
        self.state.borrow().timers.iter().filter(|t| !t.cancelled.get()).count()
    }

    pub fn pending_posts(&self) -> usize {
        self.state.borrow().posted.len()
    }

    /// Delays every live timer still waits for, relative to now, earliest first — for "the
    /// debounce was restarted" assertions.
    pub fn pending_delays(&self) -> Vec<Duration> {
        let now = self.clock.elapsed();
        let state = self.state.borrow();
        let mut live: Vec<&Timer> = state.timers.iter().filter(|t| !t.cancelled.get()).collect();
        live.sort_by_key(|t| (t.due, t.seq));
        live.iter().map(|t| t.due.saturating_sub(now)).collect()
    }

    /// Moves the clock forward by `by`, firing due timers on the way. (md.win throws for a
    /// negative span; a `Duration` cannot be one.)
    pub fn advance(&self, by: Duration) {
        let target = self.clock.elapsed() + by;
        loop {
            let next = {
                let mut state = self.state.borrow_mut();
                let index = state
                    .timers
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| !t.cancelled.get() && t.due <= target)
                    .min_by_key(|(_, t)| (t.due, t.seq))
                    .map(|(i, _)| i);
                index.map(|i| state.timers.remove(i))
            };
            let Some(timer) = next else { break };
            if timer.due > self.clock.elapsed() {
                self.clock.set_elapsed(timer.due);
            }
            // No borrow is held: the action may schedule, cancel or post.
            (timer.action)();
        }
        self.state.borrow_mut().timers.retain(|t| !t.cancelled.get());
        self.clock.set_elapsed(target);
    }

    /// Fires everything that is due right now (a zero-length advance).
    pub fn run_due(&self) {
        self.advance(Duration::ZERO);
    }

    /// Drains the posted queue, FIFO; actions posted while draining run in the same call. Returns
    /// how many ran.
    ///
    /// # Panics
    ///
    /// After 10 000 actions in one call: an action keeps re-posting itself (md.win throws
    /// `InvalidOperationException`).
    pub fn run_posted(&self) -> usize {
        let mut ran = 0;
        loop {
            let next = self.state.borrow_mut().posted.pop_front();
            let Some(action) = next else { break };
            ran += 1;
            assert!(ran <= 10_000, "Post loop: an action keeps re-posting itself");
            action();
        }
        ran
    }
}

impl Scheduler for FakeScheduler {
    fn after(&self, delay: Duration, action: Box<dyn FnOnce()>) -> TimerHandle {
        let cancelled = Rc::new(Cell::new(false));
        {
            let mut state = self.state.borrow_mut();
            let seq = state.seq;
            state.seq += 1;
            let due = self.clock.elapsed() + delay;
            state.timers.push(Timer { seq, due, action, cancelled: cancelled.clone() });
        }
        // Once the timer has fired it is out of the list, so setting the flag is a no-op.
        TimerHandle::new(move || cancelled.set(true))
    }

    fn post(&self, action: Box<dyn FnOnce()>) {
        self.state.borrow_mut().posted.push_back(action);
    }
}
