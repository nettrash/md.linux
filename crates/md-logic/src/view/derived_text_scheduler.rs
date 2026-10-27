//! Port of md.win `View/DerivedTextScheduler.cs`.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::time::Duration;

use md_core::book::writing_stats;
use md_core::export::diagram_svg::{self, Diagram};
use md_core::markdown::parser;

use super::derived_text::DerivedText;
use crate::event::Event;
use crate::seams::{Scheduler, TimerHandle, UiThread, WordCounter};

/// Where the computation runs: handed the work, it runs it (now, or later).
pub type RunOffThread = Box<dyn Fn(Box<dyn FnOnce()>)>;

/// Keeps [`DerivedText`] current without putting a parse on the typing path (shell-design §5.5;
/// macOS §6.10). The Mac's `.task(id: document.text)`: the very first fill is immediate, every later
/// one waits **250 ms** and is cancelled outright by the next keystroke.
///
/// Nothing here is async: the delay is a [`Scheduler`] timer and the hop off and back on to the UI
/// thread is a pair of closures, so a test drives the whole thing with a fake clock and an inline
/// runner. A result that lands after a newer edit started is dropped by its generation number
/// rather than by cancelling the work — the parse is cheap and a half-parsed document is never
/// published.
///
/// LINUX DECISION: the computation runs **on the UI thread** after the debounce (the default
/// [`RunOffThread`] runs the work inline). md.win hops to the thread pool with `Task.Run`; here the
/// seams are single-threaded (`Rc<dyn WordCounter>`, `UiThread::post` takes a non-`Send` closure,
/// and GLib's cross-thread invoke wants `Send`), and the Mac's `.task` on a SwiftUI view runs on
/// the main actor too. The hop stays injectable, so the generation rule is still tested.
pub struct DerivedTextScheduler {
    me: Weak<DerivedTextScheduler>,
    scheduler: Rc<dyn Scheduler>,
    ui: Rc<dyn UiThread>,
    words: Rc<dyn WordCounter>,
    diagrams: Box<dyn Fn(&str) -> Vec<Diagram>>,
    off_thread: RunOffThread,
    pending: RefCell<Option<TimerHandle>>,
    generation: Cell<u64>,
    has_computed: Cell<bool>,
    current: RefCell<DerivedText>,
    changed: Event<DerivedText>,
}

impl fmt::Debug for DerivedTextScheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DerivedTextScheduler")
            .field("generation", &self.generation.get())
            .field("has_computed", &self.has_computed.get())
            .finish()
    }
}

impl DerivedTextScheduler {
    /// The trailing debounce; the first computation does not wait.
    pub const DELAY: Duration = Duration::from_millis(250);

    /// The scheduler every window wants: md-core's own `diagram_svg::diagrams`, computed inline.
    pub fn new(
        scheduler: Rc<dyn Scheduler>,
        ui: Rc<dyn UiThread>,
        words: Rc<dyn WordCounter>,
    ) -> Rc<DerivedTextScheduler> {
        Self::with(scheduler, ui, words, diagram_svg::diagrams, |work: Box<dyn FnOnce()>| work())
    }

    /// `diagrams` lists the Export ▸ Diagram as SVG rows (a test passes its own to count the
    /// calls); `off_thread` runs each computation (a test queues them to reorder their landing).
    pub fn with(
        scheduler: Rc<dyn Scheduler>,
        ui: Rc<dyn UiThread>,
        words: Rc<dyn WordCounter>,
        diagrams: impl Fn(&str) -> Vec<Diagram> + 'static,
        off_thread: impl Fn(Box<dyn FnOnce()>) + 'static,
    ) -> Rc<DerivedTextScheduler> {
        Rc::new_cyclic(|me| DerivedTextScheduler {
            me: me.clone(),
            scheduler,
            ui,
            words,
            diagrams: Box::new(diagrams),
            off_thread: Box::new(off_thread),
            pending: RefCell::new(None),
            generation: Cell::new(0),
            has_computed: Cell::new(false),
            current: RefCell::new(DerivedText::empty()),
            changed: Event::new(),
        })
    }

    /// The last published value; [`DerivedText::empty`] until the first one lands.
    pub fn current(&self) -> DerivedText {
        self.current.borrow().clone()
    }

    /// Args: the value just published.
    pub fn changed(&self) -> &Event<DerivedText> {
        &self.changed
    }

    /// The document text changed (and, with the same text, "the window appeared"). Cancels a
    /// pending computation and starts the next one — immediately the first time, after
    /// [`DerivedTextScheduler::DELAY`] afterwards.
    pub fn text_changed(&self, text: &str) {
        let previous = self.pending.borrow_mut().take();
        drop(previous);
        if !self.has_computed.get() {
            // The flag flips when the first computation STARTS, not when it lands: a burst of
            // keystrokes before the first result must not all skip the debounce.
            self.has_computed.set(true);
            self.start(text.to_string());
            return;
        }
        let me = self.me.clone();
        let text = text.to_string();
        let timer = self.scheduler.after(
            Self::DELAY,
            Box::new(move || {
                if let Some(this) = me.upgrade() {
                    // The timer that fired is the pending one; forget its (spent) handle.
                    let spent = this.pending.borrow_mut().take();
                    if let Some(spent) = spent {
                        spent.detach();
                    }
                    this.start(text);
                }
            }),
        );
        *self.pending.borrow_mut() = Some(timer);
    }

    /// Drop a pending computation and ignore any in flight (the window is closing).
    pub fn cancel(&self) {
        let previous = self.pending.borrow_mut().take();
        drop(previous);
        self.generation.set(self.generation.get() + 1);
    }

    /// The computation itself, for the callers that need it synchronously (an export).
    pub fn compute(&self, text: &str) -> DerivedText {
        DerivedText::new(
            self.words.count(text),
            writing_stats::characters(text),
            parser::outline(text),
            parser::notes(text),
            (self.diagrams)(text),
        )
    }

    fn start(&self, text: String) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let me = self.me.clone();
        (self.off_thread)(Box::new(move || {
            let Some(this) = me.upgrade() else { return };
            let derived = this.compute(&text);
            let me = this.me.clone();
            this.ui.post(Box::new(move || {
                let Some(this) = me.upgrade() else { return };
                if generation != this.generation.get() {
                    return;
                }
                *this.current.borrow_mut() = derived.clone();
                this.changed.emit(&derived);
            }));
        }));
    }
}
