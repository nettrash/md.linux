//! Port of md.win `View/DerivedTextScheduler.cs`.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use md_core::book::writing_stats;
use md_core::export::diagram_svg::{self, Diagram};
use md_core::markdown::parser;

use super::derived_text::DerivedText;
use crate::event::Event;
use crate::exec::Spawner;
use crate::seams::{OffThread, Scheduler, TimerHandle, WordCounter};

/// The Export ▸ Diagram as SVG rows of a text — md-core's `diagram_svg::diagrams` unless a test
/// passes its own. Shared with the thread the computation runs on.
type DiagramLister = dyn Fn(&str) -> Vec<Diagram> + Send + Sync;

/// Keeps [`DerivedText`] current without putting a parse on the typing path (shell-design §5.5;
/// macOS §6.10). The Mac's `.task(id: document.text)`: the very first fill is immediate, every later
/// one waits **250 ms** and is cancelled outright by the next keystroke, and the computation itself
/// runs off the UI thread.
///
/// The delay is a [`Scheduler`] timer; the computation goes to an [`OffThread`] as a `Send`
/// closure over an owned copy of the text, and its result comes back as a future the [`Spawner`]
/// polls on the UI thread, where it is published. A result that lands after a newer edit started
/// is dropped by its generation number rather than by cancelling the work — the parse is cheap and
/// a half-parsed document is never published. Tests drive the whole thing with a fake clock, a
/// [`FakeOffThread`](crate::testing::FakeOffThread) and a [`LocalExecutor`](crate::exec::LocalExecutor).
///
/// LINUX: md.win's `RunOffThread` delegate (`Task.Run` by default) and `IUiThread.Post` become one
/// seam, [`OffThread`], whose future resolves where the spawner polls it: the app satisfies it with
/// `gio::spawn_blocking`, whose `JoinHandle` wakes the GLib main context (§15). The inputs are a
/// `String`, the outputs `Send` data, so the word counter and the diagram lister travel as `Arc`s
/// and nothing that belongs to the UI thread crosses.
pub struct DerivedTextScheduler {
    me: Weak<DerivedTextScheduler>,
    scheduler: Rc<dyn Scheduler>,
    spawner: Rc<dyn Spawner>,
    off_thread: Rc<dyn OffThread>,
    words: Arc<dyn WordCounter>,
    diagrams: Arc<DiagramLister>,
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

    /// The scheduler every window wants: md-core's own `diagram_svg::diagrams`. `spawner` polls the
    /// results on the UI thread (GLib's main context); `off_thread` runs the computations
    /// (`gio::spawn_blocking`).
    pub fn new(
        scheduler: Rc<dyn Scheduler>,
        spawner: Rc<dyn Spawner>,
        off_thread: Rc<dyn OffThread>,
        words: Arc<dyn WordCounter>,
    ) -> Rc<DerivedTextScheduler> {
        Self::with(scheduler, spawner, off_thread, words, diagram_svg::diagrams)
    }

    /// `diagrams` lists the Export ▸ Diagram as SVG rows (a test passes its own to count the
    /// calls).
    pub fn with(
        scheduler: Rc<dyn Scheduler>,
        spawner: Rc<dyn Spawner>,
        off_thread: Rc<dyn OffThread>,
        words: Arc<dyn WordCounter>,
        diagrams: impl Fn(&str) -> Vec<Diagram> + Send + Sync + 'static,
    ) -> Rc<DerivedTextScheduler> {
        Rc::new_cyclic(|me| DerivedTextScheduler {
            me: me.clone(),
            scheduler,
            spawner,
            off_thread,
            words,
            diagrams: Arc::new(diagrams),
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

    /// The computation itself, for the callers that need it synchronously (an export), on the
    /// calling thread.
    pub fn compute(&self, text: &str) -> DerivedText {
        compute(&*self.words, &*self.diagrams, text)
    }

    fn start(&self, text: String) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let (words, diagrams) = (self.words.clone(), self.diagrams.clone());
        let result = self.off_thread.run(move || compute(&*words, &*diagrams, &text));
        let me = self.me.clone();
        self.spawner.spawn_local(Box::pin(async move {
            // `None`: the work never finished (it panicked, or the pool dropped it). Nothing is
            // published; the next edit computes again.
            let Some(derived) = result.await else { return };
            let Some(this) = me.upgrade() else { return };
            if generation != this.generation.get() {
                return;
            }
            *this.current.borrow_mut() = derived.clone();
            this.changed.emit(&derived);
        }));
    }
}

/// md.win's `Compute`, free of the scheduler so it can run on any thread.
fn compute(words: &dyn WordCounter, diagrams: &DiagramLister, text: &str) -> DerivedText {
    DerivedText::new(
        words.count(text),
        writing_stats::characters(text),
        parser::outline(text),
        parser::notes(text),
        diagrams(text),
    )
}
