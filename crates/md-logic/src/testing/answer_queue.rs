//! The scripted-answer queue the dialog fakes share (md.win's `Queue<T>` properties, plus held
//! answers).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;

use crate::exec::{Completion, LocalBoxFuture, ready};

/// One scripted answer: given at once, or held open until the test completes it.
pub enum Scripted<T> {
    Ready(T),
    Held(Completion<T>),
}

impl<T> fmt::Debug for Scripted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scripted::Ready(_) => f.write_str("Ready"),
            Scripted::Held(c) => write!(f, "Held({c:?})"),
        }
    }
}

/// A FIFO of [`Scripted`] answers. Usable on its own by a test's ad-hoc seam implementation.
pub struct AnswerQueue<T> {
    queue: RefCell<VecDeque<Scripted<T>>>,
}

impl<T> Default for AnswerQueue<T> {
    fn default() -> Self {
        AnswerQueue { queue: RefCell::new(VecDeque::new()) }
    }
}

impl<T> fmt::Debug for AnswerQueue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnswerQueue").field("queued", &self.queue.borrow().len()).finish()
    }
}

impl<T: 'static> AnswerQueue<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues an answer given at once.
    pub fn push(&self, answer: T) {
        self.queue.borrow_mut().push_back(Scripted::Ready(answer));
    }

    /// Queues a held answer; the returned completion releases it.
    pub fn hold(&self) -> Completion<T> {
        let completion = Completion::new();
        self.queue.borrow_mut().push_back(Scripted::Held(completion.clone()));
        completion
    }

    /// Answers scripted but not yet asked for.
    pub fn len(&self) -> usize {
        self.queue.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.borrow().is_empty()
    }

    /// The next answer as a future: the queued one, else `fallback()` at once.
    pub fn next_or(&self, fallback: impl FnOnce() -> T) -> LocalBoxFuture<'static, T> {
        let next = self.queue.borrow_mut().pop_front();
        match next {
            Some(Scripted::Ready(value)) => ready(value),
            Some(Scripted::Held(completion)) => completion.wait(),
            None => ready(fallback()),
        }
    }
}
