//! Port of md.win `Export/BusyRing.cs`: §7.1's footer progress ring, as policy — "exports on one
//! window are serialised; a progress ring appears in the footer after 500 ms". Two things stand
//! between [`ExportPipeline::busy_changed`] and a widget's visibility, and neither belongs in a
//! window's code, because neither can be tested there.
//!
//! **The thread.** md.win's pipeline raises `BusyChanged` after `ConfigureAwait(false)`, so on
//! Windows it can arrive on a pool thread. On GTK every future runs on the main context, so that
//! cannot happen here — but every notification is still posted through [`UiThread`], and posted
//! *always*: it keeps the order the ring sees the order the pipeline raised, and it keeps the
//! widget out of the pipeline's call stack (a `show` that re-entered the pipeline would otherwise
//! run inside its gate).
//!
//! **The delay.** Showing the ring the instant an export starts would flicker one for every LaTeX
//! and TextBundle export, which finish in a few milliseconds. So the ring is armed on a timer and
//! shown only if the export is *still* running when it fires; an export that finishes first shows
//! nothing at all, and — the case worth naming — is not followed by a stray hide of a ring that was
//! never shown.
//!
//! [`ExportPipeline::busy_changed`]: super::ExportPipeline::busy_changed

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use super::export_pipeline::ExportPipeline;
use crate::seams::{Scheduler, TimerHandle, UiThread};

struct Inner {
    scheduler: Rc<dyn Scheduler>,
    ui: Rc<dyn UiThread>,
    show: Box<dyn Fn(bool)>,
    delay: Duration,
    armed: RefCell<Option<TimerHandle>>,
    busy: Cell<bool>,
    shown: Cell<bool>,
}

/// The footer ring of one window. Connect [`BusyRing::busy_changed`] to the pipeline's
/// [`ExportPipeline::busy_changed`] event.
pub struct BusyRing {
    inner: Rc<Inner>,
}

impl std::fmt::Debug for BusyRing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BusyRing")
            .field("busy", &self.inner.busy.get())
            .field("shown", &self.inner.shown.get())
            .field("delay", &self.inner.delay)
            .finish()
    }
}

impl BusyRing {
    /// The ring §7.1 describes: [`ExportPipeline::BUSY_RING_DELAY`] after the export starts. `show`
    /// is applied on the UI thread — true shows the ring, false hides it — and only on a change.
    pub fn new(scheduler: Rc<dyn Scheduler>, ui: Rc<dyn UiThread>, show: impl Fn(bool) + 'static) -> Self {
        Self::with_delay(scheduler, ui, show, ExportPipeline::BUSY_RING_DELAY)
    }

    /// For the tests that pin the boundary; the app always uses [`BusyRing::new`]. (md.win clamps a
    /// negative delay to zero; a `Duration` cannot be negative.)
    pub fn with_delay(
        scheduler: Rc<dyn Scheduler>,
        ui: Rc<dyn UiThread>,
        show: impl Fn(bool) + 'static,
        delay: Duration,
    ) -> Self {
        BusyRing {
            inner: Rc::new(Inner {
                scheduler,
                ui,
                show: Box::new(show),
                delay,
                armed: RefCell::new(None),
                busy: Cell::new(false),
                shown: Cell::new(false),
            }),
        }
    }

    /// Whether the ring is on screen right now — what the last `show` call said.
    pub fn is_shown(&self) -> bool {
        self.inner.shown.get()
    }

    /// Connect this straight to [`ExportPipeline::busy_changed`]. The work is posted to the UI
    /// thread and nothing is read here.
    pub fn busy_changed(&self, busy: bool) {
        let weak = Rc::downgrade(&self.inner);
        self.inner.ui.post(Box::new(move || {
            if let Some(inner) = weak.upgrade() {
                Inner::apply(&inner, busy);
            }
        }));
    }

    /// The window is closing: drop the armed timer and forget the ring, without touching the
    /// widget — the footer is going with the window.
    pub fn cancel(&self) {
        let armed = self.inner.armed.borrow_mut().take();
        drop(armed);
        self.inner.busy.set(false);
        self.inner.shown.set(false);
    }
}

impl Inner {
    fn apply(this: &Rc<Inner>, busy: bool) {
        // Idempotent: a second true does NOT restart the clock, or a pipeline that announced twice
        // would push the ring past the point where it is worth showing.
        if busy == this.busy.get() {
            return;
        }
        this.busy.set(busy);

        if busy {
            let weak: Weak<Inner> = Rc::downgrade(this);
            let timer = this.scheduler.after(
                this.delay,
                Box::new(move || {
                    if let Some(inner) = weak.upgrade() {
                        inner.elapsed();
                    }
                }),
            );
            let previous = this.armed.borrow_mut().replace(timer);
            drop(previous);
            return;
        }

        let armed = this.armed.borrow_mut().take();
        drop(armed);
        // Only when it was actually shown: a fast export must not blink a ring off that was never on.
        if !this.shown.get() {
            return;
        }
        this.shown.set(false);
        (this.show)(false);
    }

    fn elapsed(&self) {
        // The timer has fired: its handle is spent (dropping it cancels nothing).
        let armed = self.armed.borrow_mut().take();
        drop(armed);
        if !self.busy.get() || self.shown.get() {
            return;
        }
        self.shown.set(true);
        (self.show)(true);
    }
}
