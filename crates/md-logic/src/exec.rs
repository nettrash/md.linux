//! Asynchrony for a single-threaded shell, std only.
//!
//! md.win's logic awaits dialogs, pickers and the export renderer (`Task<T>`). Here every such
//! seam returns a [`LocalBoxFuture`]; the GTK app drives them on GLib's main context
//! (`glib::MainContext::spawn_local`) and the tests on [`LocalExecutor`], which runs spawned
//! futures until none can make progress — the test then answers a fake (a dialog, a timer) and
//! pumps again. Nothing here is `Send`: like WinUI's dispatcher and GTK's main loop, everything
//! runs on the one UI thread — except the work handed to an [`OffThread`], which crosses to
//! another thread and back as plain `Send` data ([`ThreadOffThread`] is the std-only runner).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::panic::{self, AssertUnwindSafe};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Wake, Waker};

use crate::seams::{OffThread, OffThreadValue, OffThreadWork};

/// A boxed, non-`Send` future — the return type of every asynchronous seam.
pub type LocalBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Where md-logic starts work that outlives the call that began it (an export, a command whose
/// handler awaits a dialog). App: GLib's main context; tests: [`LocalExecutor`].
pub trait Spawner {
    fn spawn_local(&self, future: LocalBoxFuture<'static, ()>);
}

/// C#'s `CancellationToken` for one thread: cloned handles share one flag.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Rc<Cell<bool>>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.set(true);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.get()
    }
}

/// The error a cancelled asynchronous operation finishes with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the operation was cancelled")
    }
}

impl std::error::Error for Cancelled {}

struct Flag(AtomicBool);

impl Wake for Flag {
    fn wake(self: Arc<Self>) {
        self.0.store(true, Ordering::SeqCst);
    }
}

struct Spawned {
    future: LocalBoxFuture<'static, ()>,
    woken: Arc<Flag>,
}

/// A run-until-stalled executor for tests (and for any code path that must finish a future the
/// fakes can complete synchronously).
#[derive(Clone, Default)]
pub struct LocalExecutor {
    queue: Rc<RefCell<VecDeque<Spawned>>>,
}

impl fmt::Debug for LocalExecutor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalExecutor").field("pending", &self.queue.borrow().len()).finish()
    }
}

impl LocalExecutor {
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of spawned futures that have not finished.
    pub fn pending(&self) -> usize {
        self.queue.borrow().len()
    }

    /// Polls every spawned future until none was woken since its last poll. Futures spawned while
    /// running are polled in the same call.
    pub fn run_until_stalled(&self) {
        loop {
            let mut progressed = false;
            let count = self.queue.borrow().len();
            for _ in 0..count {
                let Some(mut task) = self.queue.borrow_mut().pop_front() else { break };
                if !task.woken.0.swap(false, Ordering::SeqCst) {
                    self.queue.borrow_mut().push_back(task);
                    continue;
                }
                progressed = true;
                let waker = Waker::from(task.woken.clone());
                let mut context = Context::from_waker(&waker);
                if task.future.as_mut().poll(&mut context).is_pending() {
                    self.queue.borrow_mut().push_back(task);
                }
            }
            if !progressed {
                break;
            }
        }
    }

    /// Runs `future` to completion on this executor, also driving everything spawned. Panics if
    /// it stalls — in a test that means a fake was never answered.
    pub fn block_on<T: 'static>(&self, future: impl Future<Output = T> + 'static) -> T {
        let slot: Rc<RefCell<Option<T>>> = Rc::new(RefCell::new(None));
        let out = slot.clone();
        self.spawn_local(Box::pin(async move {
            *out.borrow_mut() = Some(future.await);
        }));
        self.run_until_stalled();
        slot.borrow_mut().take().expect("block_on: the future stalled (an unanswered fake?)")
    }
}

impl Spawner for LocalExecutor {
    fn spawn_local(&self, future: LocalBoxFuture<'static, ()>) {
        self.queue.borrow_mut().push_back(Spawned { future, woken: Arc::new(Flag(AtomicBool::new(true))) });
    }
}

/// C#'s `TaskCompletionSource<T>`: a future completed from outside. Fakes use it to hold a
/// dialog open until the test answers it.
pub struct Completion<T> {
    state: Rc<RefCell<CompletionState<T>>>,
}

struct CompletionState<T> {
    value: Option<T>,
    waker: Option<Waker>,
}

impl<T> Clone for Completion<T> {
    fn clone(&self) -> Self {
        Completion { state: self.state.clone() }
    }
}

impl<T> fmt::Debug for Completion<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Completion").field("completed", &self.state.borrow().value.is_some()).finish()
    }
}

impl<T> Default for Completion<T> {
    fn default() -> Self {
        Completion { state: Rc::new(RefCell::new(CompletionState { value: None, waker: None })) }
    }
}

impl<T: 'static> Completion<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Completes the future (a second call replaces a value not yet taken).
    pub fn complete(&self, value: T) {
        let waker = {
            let mut state = self.state.borrow_mut();
            state.value = Some(value);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// A future that resolves with the value passed to [`Completion::complete`].
    pub fn wait(&self) -> LocalBoxFuture<'static, T> {
        let state = self.state.clone();
        Box::pin(std::future::poll_fn(move |cx| {
            let mut s = state.borrow_mut();
            match s.value.take() {
                Some(value) => Poll::Ready(value),
                None => {
                    s.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        }))
    }
}

/// A future that is already finished — what a fake returns for an immediate answer.
pub fn ready<T: 'static>(value: T) -> LocalBoxFuture<'static, T> {
    Box::pin(std::future::ready(value))
}

/// [`OffThread`] on `std::thread`: one short-lived thread per piece of work, the result handed
/// back through the future and the waker. Std only, so md-logic's own tests can show that what
/// crosses the seam really runs elsewhere; the GTK app uses GLib's pool (`gio::spawn_blocking`)
/// instead. Work that panics resolves to `None`, as a failed `JoinHandle` does; so does work no
/// thread could be started for.
#[derive(Clone, Copy, Debug, Default)]
pub struct ThreadOffThread;

#[derive(Default)]
struct ThreadSlot {
    /// `Some` once the thread is done: the work's value, or `None` if it panicked.
    finished: Option<Option<OffThreadValue>>,
    waker: Option<Waker>,
}

impl OffThread for ThreadOffThread {
    fn spawn_blocking(&self, work: OffThreadWork) -> LocalBoxFuture<'static, Option<OffThreadValue>> {
        let slot = Arc::new(Mutex::new(ThreadSlot::default()));
        let filled = slot.clone();
        let spawned = std::thread::Builder::new().name("md-off-thread".to_string()).spawn(move || {
            let value = panic::catch_unwind(AssertUnwindSafe(work)).ok();
            let waker = {
                let mut slot = filled.lock().unwrap_or_else(PoisonError::into_inner);
                slot.finished = Some(value);
                slot.waker.take()
            };
            if let Some(waker) = waker {
                waker.wake();
            }
        });
        if spawned.is_err() {
            return ready(None);
        }
        Box::pin(std::future::poll_fn(move |cx| {
            let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
            match slot.finished.take() {
                Some(value) => Poll::Ready(value),
                None => {
                    slot.waker = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_on_drives_ready_futures() {
        let executor = LocalExecutor::new();
        assert_eq!(executor.block_on(async { ready(41).await + 1 }), 42);
    }

    #[test]
    fn a_completion_holds_a_spawned_future_until_answered() {
        let executor = LocalExecutor::new();
        let dialog: Completion<bool> = Completion::new();
        let answer = Rc::new(Cell::new(None));
        {
            let (wait, answer) = (dialog.wait(), answer.clone());
            executor.spawn_local(Box::pin(async move { answer.set(Some(wait.await)) }));
        }
        executor.run_until_stalled();
        assert_eq!(answer.get(), None);
        assert_eq!(executor.pending(), 1);
        dialog.complete(true);
        executor.run_until_stalled();
        assert_eq!(answer.get(), Some(true));
        assert_eq!(executor.pending(), 0);
    }

    #[test]
    fn cancel_tokens_share_one_flag() {
        let token = CancelToken::new();
        let copy = token.clone();
        copy.cancel();
        assert!(token.is_cancelled());
    }
}
