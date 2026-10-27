//! Port of md.win `FakeUiThread`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;

use crate::seams::UiThread;

/// [`UiThread`] that runs posts inline (the default) or queues them for
/// [`FakeUiThread::run_posted`].
pub struct FakeUiThread {
    posted: RefCell<VecDeque<Box<dyn FnOnce()>>>,
    run_inline: Cell<bool>,
    is_current: Cell<bool>,
    post_count: Cell<usize>,
}

impl Default for FakeUiThread {
    fn default() -> Self {
        FakeUiThread {
            posted: RefCell::new(VecDeque::new()),
            run_inline: Cell::new(true),
            is_current: Cell::new(true),
            post_count: Cell::new(0),
        }
    }
}

impl fmt::Debug for FakeUiThread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeUiThread")
            .field("run_inline", &self.run_inline.get())
            .field("is_current", &self.is_current.get())
            .field("pending_posts", &self.pending_posts())
            .finish()
    }
}

impl FakeUiThread {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn run_inline(&self) -> bool {
        self.run_inline.get()
    }

    /// Inline (true, the default) or queued until [`FakeUiThread::run_posted`].
    pub fn set_run_inline(&self, inline: bool) {
        self.run_inline.set(inline);
    }

    /// What [`UiThread::is_current`] answers (default true).
    pub fn set_is_current(&self, current: bool) {
        self.is_current.set(current);
    }

    /// Every post, inline or queued.
    pub fn post_count(&self) -> usize {
        self.post_count.get()
    }

    pub fn pending_posts(&self) -> usize {
        self.posted.borrow().len()
    }

    /// Runs the queued posts, FIFO, including ones posted while draining; returns how many ran.
    pub fn run_posted(&self) -> usize {
        let mut ran = 0;
        loop {
            let next = self.posted.borrow_mut().pop_front();
            let Some(action) = next else { break };
            action();
            ran += 1;
        }
        ran
    }
}

impl UiThread for FakeUiThread {
    fn post(&self, action: Box<dyn FnOnce()>) {
        self.post_count.set(self.post_count.get() + 1);
        if self.run_inline.get() {
            action();
        } else {
            self.posted.borrow_mut().push_back(action);
        }
    }

    fn is_current(&self) -> bool {
        self.is_current.get()
    }
}
