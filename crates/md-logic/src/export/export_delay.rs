//! Port of md.win `Export/ExportDelay.cs`: every wait the export path takes — the poller's 250 ms,
//! the snapshotter's 300 ms repaint, the cover fetch's 15 s — expressed through [`Scheduler`] so a
//! test can advance it instead of sleeping, and so every one of them lands on the UI thread.
//!
//! LINUX DECISIONS:
//! - The scheduler is required. md.win falls back to `Task.Delay` without one; std has no timer
//!   that does not need a thread, and the app always passes GLib's main-loop scheduler anyway.
//! - [`CancelToken`] (exec.rs) is a flag that cannot wake anything — C# registers a callback on its
//!   token. Cancellation is therefore seen when the wait is next polled: at once when it is already
//!   cancelled at the start (no timer is armed at all), and otherwise at the latest when the timer
//!   fires. Either way the future finishes with [`Cancelled`] and its timer is cancelled (the
//!   handle is dropped) the moment that is noticed. Dropping the future cancels the timer too —
//!   Rust's own form of cancellation.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use crate::exec::{CancelToken, Cancelled};
use crate::seams::{Scheduler, TimerHandle};

#[derive(Default)]
struct Shared {
    fired: Cell<bool>,
    waker: RefCell<Option<Waker>>,
}

/// One cancellable wait on a [`Scheduler`] timer; resolves `Ok(())` when the timer fires and
/// `Err(Cancelled)` once `cancel` is seen.
#[must_use = "a delay does nothing unless awaited"]
pub struct Delay {
    shared: Rc<Shared>,
    timer: Option<TimerHandle>,
    cancel: CancelToken,
}

impl fmt::Debug for Delay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Delay").field("fired", &self.shared.fired.get()).field("armed", &self.timer.is_some()).finish()
    }
}

/// Wait `delay` on `scheduler`, or until `cancel` is seen (md.win's `ExportDelay.For`). A token that
/// is already cancelled arms no timer: the first poll answers [`Cancelled`] (C#'s
/// `ThrowIfCancellationRequested` before the wait).
pub fn delay(scheduler: &dyn Scheduler, delay: Duration, cancel: &CancelToken) -> Delay {
    let shared = Rc::new(Shared::default());
    let timer = if cancel.is_cancelled() {
        None
    } else {
        let fired = Rc::downgrade(&shared);
        Some(scheduler.after(
            delay,
            Box::new(move || {
                if let Some(shared) = fired.upgrade() {
                    shared.fired.set(true);
                    // Take the waker first: waking may poll the future on this very call.
                    let waker = shared.waker.borrow_mut().take();
                    if let Some(waker) = waker {
                        waker.wake();
                    }
                }
            }),
        ))
    };
    Delay { shared, timer, cancel: cancel.clone() }
}

impl Future for Delay {
    type Output = Result<(), Cancelled>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.cancel.is_cancelled() {
            // Dropping the handle stops the timer, so a cancelled wait never fires later.
            self.timer = None;
            return Poll::Ready(Err(Cancelled));
        }
        if self.shared.fired.get() {
            if let Some(timer) = self.timer.take() {
                timer.detach();
            }
            return Poll::Ready(Ok(()));
        }
        *self.shared.waker.borrow_mut() = Some(cx.waker().clone());
        Poll::Pending
    }
}
