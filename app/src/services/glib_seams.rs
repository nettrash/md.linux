//! md-logic's timing and threading seams over GLib's main context (shell-design §1.7): the
//! [`Scheduler`] and [`Clock`] the autosave, the re-render debounce and the derived-text tick run
//! on, the [`UiThread`], and the [`Spawner`] md-logic's asynchronous commands and exports are
//! driven by. Everything runs on the GTK main thread, so none of this is `Send`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use md_logic::exec::{LocalBoxFuture, Spawner};
use md_logic::seams::{Clock, Scheduler, TimerHandle, UiThread};

/// The monotonic wall clock: `Instant::now()`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// One-shot timers on the default main context (`glib::timeout_add_local_once`) and "next turn"
/// posts; also the [`Clock`]. Dropping a [`TimerHandle`] before the timer fires removes its
/// source; after it fired, the drop is a no-op (removing a finished source would be a GLib
/// critical).
#[derive(Clone, Copy, Debug, Default)]
pub struct GlibScheduler;

impl Scheduler for GlibScheduler {
    fn after(&self, delay: Duration, action: Box<dyn FnOnce()>) -> TimerHandle {
        let pending: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
        let fired = Rc::clone(&pending);
        let id = glib::timeout_add_local_once(delay, move || {
            // The source is finished once this returns: forget its id first, so a handle dropped
            // by the action itself does not try to remove it.
            fired.set(None);
            action();
        });
        pending.set(Some(id));
        TimerHandle::new(move || {
            if let Some(id) = pending.take() {
                id.remove();
            }
        })
    }

    fn post(&self, action: Box<dyn FnOnce()>) {
        post(action);
    }
}

impl Clock for GlibScheduler {
    fn now(&self) -> Instant {
        SystemClock.now()
    }
}

/// The GTK main thread as a destination. Every GTK callback already arrives on it; `post` defers
/// to the next main-loop turn like the scheduler's.
#[derive(Clone, Copy, Debug, Default)]
pub struct GlibUiThread;

impl UiThread for GlibUiThread {
    fn post(&self, action: Box<dyn FnOnce()>) {
        post(action);
    }

    fn is_current(&self) -> bool {
        glib::MainContext::default().is_owner()
    }
}

/// md-logic's [`Spawner`] on the default main context (`MainContext::spawn_local`). The join
/// handle is dropped: the future runs to completion on its own (dropping a GLib `JoinHandle` does
/// not cancel it).
#[derive(Clone, Copy, Debug, Default)]
pub struct GlibSpawner;

impl Spawner for GlibSpawner {
    fn spawn_local(&self, future: LocalBoxFuture<'static, ()>) {
        drop(glib::MainContext::default().spawn_local(future));
    }
}

/// Runs `action` on the next main-loop turn at `Priority::DEFAULT` — before the frame clock's
/// layout and paint (`GDK_PRIORITY_REDRAW`), like WinUI's normal-priority `TryEnqueue`, rather than
/// at `DEFAULT_IDLE`, after them.
fn post(action: Box<dyn FnOnce()>) {
    let mut action = Some(action);
    glib::idle_add_local_full(glib::Priority::DEFAULT, move || {
        if let Some(action) = action.take() {
            action();
        }
        glib::ControlFlow::Break
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// The default main context is process-wide and has one owner at a time, while the test
    /// harness runs tests on parallel threads: the tests that use it take turns.
    static MAIN_CONTEXT: Mutex<()> = Mutex::new(());

    fn take_turn() -> MutexGuard<'static, ()> {
        MAIN_CONTEXT.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Iterates the default context until `done` or `limit`.
    fn pump_until(limit: Duration, done: impl Fn() -> bool) -> bool {
        let context = glib::MainContext::default();
        let start = Instant::now();
        while !done() && start.elapsed() < limit {
            context.iteration(false);
            std::thread::sleep(Duration::from_millis(1));
        }
        done()
    }

    #[test]
    fn a_timer_fires_once_and_a_dropped_one_never_does() {
        let _turn = take_turn();
        let context = glib::MainContext::default();
        let _owner = context.acquire().expect("the test thread owns its default context");
        let scheduler = GlibScheduler;
        let fired = Rc::new(Cell::new(0));
        let cancelled = Rc::new(Cell::new(false));
        {
            let fired = Rc::clone(&fired);
            scheduler.after(Duration::from_millis(5), Box::new(move || fired.set(fired.get() + 1))).detach();
        }
        {
            let cancelled = Rc::clone(&cancelled);
            drop(scheduler.after(Duration::from_millis(5), Box::new(move || cancelled.set(true))));
        }
        assert!(pump_until(Duration::from_secs(5), || fired.get() == 1));
        pump_until(Duration::from_millis(30), || false);
        assert_eq!(fired.get(), 1);
        assert!(!cancelled.get());
    }

    #[test]
    fn dropping_a_handle_after_the_timer_fired_is_harmless() {
        let _turn = take_turn();
        let context = glib::MainContext::default();
        let _owner = context.acquire().expect("the test thread owns its default context");
        let fired = Rc::new(Cell::new(false));
        let handle = {
            let fired = Rc::clone(&fired);
            GlibScheduler.after(Duration::from_millis(1), Box::new(move || fired.set(true)))
        };
        assert!(pump_until(Duration::from_secs(5), || fired.get()));
        handle.cancel();
    }

    #[test]
    fn posts_run_in_order_on_a_later_turn() {
        let _turn = take_turn();
        let context = glib::MainContext::default();
        let _owner = context.acquire().expect("the test thread owns its default context");
        let log = Rc::new(std::cell::RefCell::new(Vec::new()));
        for n in 0..3 {
            let log = Rc::clone(&log);
            if n % 2 == 0 {
                GlibScheduler.post(Box::new(move || log.borrow_mut().push(n)));
            } else {
                GlibUiThread.post(Box::new(move || log.borrow_mut().push(n)));
            }
        }
        assert!(log.borrow().is_empty());
        assert!(GlibUiThread.is_current());
        assert!(pump_until(Duration::from_secs(5), || log.borrow().len() == 3));
        assert_eq!(*log.borrow(), vec![0, 1, 2]);
    }

    #[test]
    fn spawned_futures_run_on_the_main_context() {
        let _turn = take_turn();
        let context = glib::MainContext::default();
        let _owner = context.acquire().expect("the test thread owns its default context");
        let done = Rc::new(Cell::new(false));
        {
            let done = Rc::clone(&done);
            GlibSpawner.spawn_local(Box::pin(async move {
                glib::timeout_future(Duration::from_millis(1)).await;
                done.set(true);
            }));
        }
        assert!(pump_until(Duration::from_secs(5), || done.get()));
    }
}
