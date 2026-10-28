//! The [`OffThread`] fake (md.win's tests pass `work => work()` or queue the delegates by hand).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;

use crate::exec::{Completion, LocalBoxFuture, ready};
use crate::seams::{OffThread, OffThreadValue, OffThreadWork};

type Held = (OffThreadWork, Completion<Option<OffThreadValue>>);

/// [`OffThread`] that either runs each piece of work at once on the calling thread
/// ([`FakeOffThread::inline`]) or holds it until the test runs it ([`FakeOffThread::new`]) — oldest
/// first, newest first, or not at all ([`FakeOffThread::drop_oldest`] resolves it as work that never
/// finished). Held work lands only when the test both runs it and pumps the executor that polls the
/// waiting future, which is how a result reaches the UI thread in the app.
#[derive(Default)]
pub struct FakeOffThread {
    inline: bool,
    held: RefCell<VecDeque<Held>>,
    runs: Cell<usize>,
}

impl fmt::Debug for FakeOffThread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeOffThread")
            .field("inline", &self.inline)
            .field("held", &self.held.borrow().len())
            .field("runs", &self.runs.get())
            .finish()
    }
}

impl FakeOffThread {
    /// Holds every piece of work until the test runs it.
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs every piece of work as soon as it is handed over.
    pub fn inline() -> Self {
        FakeOffThread { inline: true, ..Self::default() }
    }

    /// Work handed over and not yet run or dropped.
    pub fn pending(&self) -> usize {
        self.held.borrow().len()
    }

    /// Pieces of work run so far, inline or held.
    pub fn runs(&self) -> usize {
        self.runs.get()
    }

    /// Runs the oldest held work and completes its future; false when nothing is held.
    pub fn run_oldest(&self) -> bool {
        let next = self.held.borrow_mut().pop_front();
        self.finish(next)
    }

    /// Runs the newest held work and completes its future; false when nothing is held.
    pub fn run_newest(&self) -> bool {
        let next = self.held.borrow_mut().pop_back();
        self.finish(next)
    }

    /// Runs everything held, oldest first.
    pub fn run_all(&self) {
        while self.run_oldest() {}
    }

    /// Resolves the oldest held work as never finished (`None`) without running it; false when
    /// nothing is held.
    pub fn drop_oldest(&self) -> bool {
        let next = self.held.borrow_mut().pop_front();
        match next {
            Some((_, done)) => {
                done.complete(None);
                true
            }
            None => false,
        }
    }

    fn finish(&self, next: Option<Held>) -> bool {
        // The queue's borrow is released before the work runs: it may hand over more work.
        let Some((work, done)) = next else { return false };
        self.runs.set(self.runs.get() + 1);
        done.complete(Some(work()));
        true
    }
}

impl OffThread for FakeOffThread {
    fn spawn_blocking(&self, work: OffThreadWork) -> LocalBoxFuture<'static, Option<OffThreadValue>> {
        if self.inline {
            self.runs.set(self.runs.get() + 1);
            return ready(Some(work()));
        }
        let done = Completion::new();
        let wait = done.wait();
        self.held.borrow_mut().push_back((work, done));
        wait
    }
}
