//! Port of md.win `tests/Md.App.Logic.Tests/DerivedTextSchedulerTests.cs` (`DerivedTextSchedulerTests`
//! and `FooterTextTests`): shell-design §5.5 — the first fill is immediate, every later one waits
//! 250 ms and is cancelled by the next keystroke, a result that a newer edit has overtaken is
//! dropped, and the footer sentence is the Mac's word for word.
//!
//! LINUX: the computation goes through the `OffThread` seam and comes back as a future the
//! spawner polls (§15), so the fixture pumps a `LocalExecutor` — the UI thread's main loop — after
//! every step, and `FakeOffThread` stands in for md.win's inline / queued `RunOffThread` delegates.
//! The tests after md.win's prove the hop on real threads with `ThreadOffThread`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use md_core::export::diagram_svg::Diagram;
use md_logic::strings;
use md_logic::testing::*;
use md_logic::view::{DerivedText, DerivedTextScheduler, DocumentWindowState};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

struct Fixture {
    scheduler: Rc<FakeScheduler>,
    executor: LocalExecutor,
    off_thread: Rc<FakeOffThread>,
    words: Arc<FakeWordCounter>,
    published: Rc<RefCell<Vec<DerivedText>>>,
}

impl Fixture {
    /// The computation runs as soon as it is handed over (md.win's `work => work()`).
    fn new() -> Self {
        Self::over(FakeOffThread::inline())
    }

    /// The computation waits until the test runs it (md.win's queued delegates).
    fn holding() -> Self {
        Self::over(FakeOffThread::new())
    }

    fn over(off_thread: FakeOffThread) -> Self {
        Fixture {
            scheduler: Rc::new(FakeScheduler::new()),
            executor: LocalExecutor::new(),
            off_thread: Rc::new(off_thread),
            words: Arc::new(FakeWordCounter::new()),
            published: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn build(&self) -> Rc<DerivedTextScheduler> {
        self.build_with(md_core::export::diagram_svg::diagrams)
    }

    fn build_with(&self, diagrams: impl Fn(&str) -> Vec<Diagram> + Send + Sync + 'static) -> Rc<DerivedTextScheduler> {
        let derived = DerivedTextScheduler::with(
            self.scheduler.clone(),
            Rc::new(self.executor.clone()),
            self.off_thread.clone(),
            self.words.clone(),
            diagrams,
        );
        let published = self.published.clone();
        derived.changed().connect(move |value| published.borrow_mut().push(value.clone()));
        derived
    }

    /// One turn of the UI thread's main loop: every result that has landed is published.
    fn pump(&self) {
        self.executor.run_until_stalled();
    }

    fn text_changed(&self, derived: &DerivedTextScheduler, text: &str) {
        derived.text_changed(text);
        self.pump();
    }

    fn advance(&self, by: Duration) {
        self.scheduler.advance(by);
        self.pump();
    }

    fn published(&self) -> Vec<DerivedText> {
        self.published.borrow().clone()
    }

    fn clear(&self) {
        self.published.borrow_mut().clear();
    }
}

#[test]
fn the_first_computation_is_immediate() {
    let f = Fixture::new();
    let derived = f.build();

    f.text_changed(&derived, "# Title\n\nHello world");

    assert_eq!(f.published().len(), 1);
    assert_eq!(f.scheduler.pending_timers(), 0);
    assert_eq!(derived.current().words, 3);
    assert_eq!(derived.current().outline.len(), 1);
    assert_eq!(derived.current().outline[0].text, "Title");
}

#[test]
fn every_later_computation_waits_two_hundred_and_fifty_milliseconds() {
    let f = Fixture::new();
    let derived = f.build();
    f.text_changed(&derived, "one");
    f.clear();

    f.text_changed(&derived, "one two");
    assert!(f.published().is_empty());
    assert_eq!(f.scheduler.pending_delays(), vec![ms(250)]);

    f.advance(ms(249));
    assert!(f.published().is_empty());

    f.advance(ms(1));
    let published = f.published();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].words, 2);
}

#[test]
fn the_next_keystroke_cancels_the_pending_computation() {
    let f = Fixture::new();
    let derived = f.build();
    f.text_changed(&derived, "one");
    f.clear();

    f.text_changed(&derived, "one two");
    f.advance(ms(200));
    f.text_changed(&derived, "one two three");
    f.advance(ms(200));

    assert!(f.published().is_empty());
    assert_eq!(f.scheduler.pending_timers(), 1);

    f.advance(ms(50));
    let published = f.published();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].words, 3);
}

#[test]
fn a_burst_before_the_first_result_still_only_computes_once() {
    let f = Fixture::new();
    let derived = f.build();

    f.text_changed(&derived, "a");
    f.text_changed(&derived, "ab");
    f.text_changed(&derived, "abc");

    assert_eq!(f.published().len(), 1); // the immediate one
    f.advance(ms(250));
    assert_eq!(f.published().len(), 2);
    assert_eq!(f.words.calls(), ["a", "abc"]);
}

#[test]
fn a_result_overtaken_by_a_newer_edit_is_dropped() {
    let f = Fixture::holding();
    let derived = f.build();

    f.text_changed(&derived, "two stale words");
    f.text_changed(&derived, "fresh"); // the second is debounced
    f.advance(ms(250));
    assert_eq!(f.off_thread.pending(), 2);

    f.off_thread.run_newest(); // the newer computation lands first
    f.pump();
    f.off_thread.run_oldest(); // the older one comes back late
    f.pump();

    assert_eq!(f.published().len(), 1);
    assert_eq!(f.words.calls(), ["fresh", "two stale words"]);
    assert_eq!(derived.current().words, 1);
}

#[test]
fn cancel_drops_both_the_pending_timer_and_anything_in_flight() {
    let f = Fixture::holding();
    let derived = f.build();
    f.text_changed(&derived, "one");
    f.off_thread.run_all();
    f.pump();
    f.clear();
    f.text_changed(&derived, "one two");
    f.advance(ms(250));

    derived.cancel();
    f.off_thread.run_all();
    f.pump();

    assert!(f.published().is_empty());
    assert_eq!(f.scheduler.pending_timers(), 0);
}

#[test]
fn characters_are_grapheme_clusters_and_notes_and_diagrams_come_along() {
    let f = Fixture::new();
    let derived = f.build_with(|_| {
        vec![Diagram {
            ordinal: 0,
            engine: "mermaid".into(),
            source: "graph TD;".into(),
            menu_title: "Diagram 1".into(),
            layout: None,
        }]
    });

    f.text_changed(&derived, "e\u{0301}\u{1F469}\u{200D}\u{1F467}\n\n<!-- note: private -->\n");

    let current = derived.current();
    // 32 UTF-16 units, 27 grapheme clusters: "e + U+0301" is one and the ZWJ family is one.
    assert_eq!(current.characters, 27);
    assert_eq!(current.diagrams.len(), 1);
    assert_eq!(current.diagrams[0].menu_title, "Diagram 1");
    assert_eq!(current.notes.len(), 1);
    assert_eq!(current.notes[0].text, "private");
}

#[test]
fn the_result_is_published_on_the_ui_thread() {
    // LINUX: the result is a future the UI thread's spawner polls — computed is not published
    // until the main loop turns.
    let f = Fixture::holding();
    let derived = f.build();

    derived.text_changed("one two");
    assert!(f.published().is_empty());
    assert_eq!(f.off_thread.pending(), 1);

    f.off_thread.run_oldest();
    assert!(f.published().is_empty());

    f.pump();
    assert_eq!(f.published().len(), 1);
}

#[test]
fn compute_is_available_synchronously_for_the_callers_that_cannot_wait() {
    let f = Fixture::new();
    let derived = f.build();

    let value = derived.compute("# Heading\n\nfour more words here");

    assert_eq!(value.words, 5); // the fake counter counts "#" out and the five words in
    assert_eq!(value.outline.len(), 1);
    assert_eq!(value.outline[0].text, "Heading");
    assert_eq!(derived.current(), DerivedText::empty());
    assert_eq!(f.off_thread.runs(), 0); // on the calling thread, not handed over
}

#[test]
fn the_plain_constructor_lists_md_cores_diagrams() {
    // md.win: The_default_scheduler… — Core's DiagramSvg.Diagrams when no lister is passed.
    let f = Fixture::new();
    let derived = DerivedTextScheduler::new(
        f.scheduler.clone(),
        Rc::new(f.executor.clone()),
        f.off_thread.clone(),
        f.words.clone(),
    );

    f.text_changed(&derived, "```mermaid\ngraph TD;\n```\n");

    assert_eq!(derived.current().diagrams.len(), 1);
    assert_eq!(derived.current().diagrams[0].engine, "mermaid");
}

// ── LINUX: the hop off the UI thread and back (§5.5, §15) ─────────────────────────────────

#[test]
fn work_that_never_finishes_publishes_nothing_and_the_next_edit_computes_again() {
    // gio::spawn_blocking's JoinHandle fails when the work panics; the seam answers None.
    let f = Fixture::holding();
    let derived = f.build();

    f.text_changed(&derived, "one");
    assert!(f.off_thread.drop_oldest());
    f.pump();
    assert!(f.published().is_empty());
    assert_eq!(derived.current(), DerivedText::empty());

    f.text_changed(&derived, "one two");
    f.advance(DerivedTextScheduler::DELAY);
    f.off_thread.run_all();
    f.pump();
    assert_eq!(f.published().len(), 1);
    assert_eq!(derived.current().words, 2);
}

#[test]
fn the_computation_runs_on_another_thread_and_is_published_on_this_one() {
    let ui = thread::current().id();
    let scheduler = Rc::new(FakeScheduler::new());
    let executor = LocalExecutor::new();
    let words = Arc::new(FakeWordCounter::new());
    let counted_on: Arc<Mutex<Vec<ThreadId>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let counted_on = counted_on.clone();
        words.set_override(move |text| {
            counted_on.lock().unwrap().push(thread::current().id());
            FakeWordCounter::default_count(text)
        });
    }
    let derived = DerivedTextScheduler::new(
        scheduler.clone(),
        Rc::new(executor.clone()),
        Rc::new(ThreadOffThread),
        words.clone(),
    );
    let published_on = Rc::new(RefCell::new(Vec::new()));
    {
        let published_on = published_on.clone();
        derived.changed().connect(move |value| published_on.borrow_mut().push((thread::current().id(), value.words)));
    }

    derived.text_changed("# Title\n\nthree more words");
    let deadline = Instant::now() + Duration::from_secs(10);
    while published_on.borrow().is_empty() {
        assert!(Instant::now() < deadline, "the result never came back");
        thread::sleep(ms(1));
        executor.run_until_stalled();
    }

    let counted_on = counted_on.lock().unwrap().clone();
    assert_eq!(counted_on.len(), 1);
    assert_ne!(counted_on[0], ui, "the words were counted on the UI thread");
    assert_eq!(*published_on.borrow(), [(ui, 4)]); // "Title" and the three words
    assert_eq!(derived.current().outline[0].text, "Title");
    assert_eq!(words.calls(), ["# Title\n\nthree more words"]);
}

// ── The chain the window actually builds ──────────────────────────────────────────────────

#[test]
fn the_windows_whole_chain_is_one_publish_per_debounce_window() {
    // What the document window wires: scheduler.changed → state.set_derived → state.changed →
    // footer. A keystroke must not reach the footer, and one debounce window must not reach it twice.
    let f = Fixture::new();
    let state = Rc::new(DocumentWindowState::new());
    let footer = Rc::new(RefCell::new(Vec::new()));
    let derived = f.build();
    {
        let state = state.clone();
        derived.changed().connect(move |value| state.set_derived(value.clone()));
    }
    {
        let (footer, s) = (footer.clone(), Rc::downgrade(&state));
        state.changed().connect(move |_| {
            let state = s.upgrade().unwrap();
            let d = state.derived();
            footer.borrow_mut().push(strings::footer(d.words, d.characters));
        });
    }

    f.text_changed(&derived, "one two");
    assert_eq!(*footer.borrow(), ["2 words · 7 characters"]); // the first fill is immediate

    f.text_changed(&derived, "one two t");
    f.text_changed(&derived, "one two th");
    f.text_changed(&derived, "one two three");
    assert_eq!(footer.borrow().len(), 1); // the debounce is still running

    f.advance(DerivedTextScheduler::DELAY);

    assert_eq!(*footer.borrow(), ["2 words · 7 characters", "3 words · 13 characters"]);
}

#[test]
fn a_repeat_of_the_same_text_does_not_disturb_the_footer() {
    // A window re-shown, or an external replace that changes nothing, runs the chain again with
    // identical numbers; the state's equality guard swallows it.
    let f = Fixture::new();
    let state = Rc::new(DocumentWindowState::new());
    let updates = Rc::new(std::cell::Cell::new(0));
    let derived = f.build();
    {
        let state = state.clone();
        derived.changed().connect(move |value| state.set_derived(value.clone()));
    }
    {
        let updates = updates.clone();
        state.changed().connect(move |_| updates.set(updates.get() + 1));
    }

    f.text_changed(&derived, "one two");
    f.text_changed(&derived, "one two");
    f.advance(DerivedTextScheduler::DELAY);

    assert_eq!(updates.get(), 1);
}

// ── FooterTextTests ───────────────────────────────────────────────────────────────────────

#[test]
fn the_footer_sentence_is_the_macs_word_for_word() {
    assert_eq!(strings::footer(0, 0), "0 words · 0 characters");
    assert_eq!(strings::footer(1, 1), "1 words · 1 characters"); // no pluralisation, as on the Mac
    assert_eq!(strings::footer(1234, 5678), "1234 words · 5678 characters");
}

#[test]
fn the_separator_is_a_middle_dot_and_not_any_other_punctuation() {
    let text = strings::footer(2, 3);

    assert!(text.contains(" \u{00B7} "));
    assert!(!text.contains(','));
    assert!(!text.contains('\u{2022}')); // bullet
    assert!(!text.contains("\u{00B7}\u{00B7}"));
}

#[test]
fn the_numbers_are_the_derived_texts_and_nothing_is_recomputed_for_the_footer() {
    // What the footer does, minus GTK: the two fields, in this order.
    let derived = DerivedText::new(7, 42, vec![], vec![], vec![]);

    assert_eq!(strings::footer(derived.words, derived.characters), "7 words · 42 characters");
}

#[test]
fn the_numbers_are_formatted_the_same_whatever_the_machines_culture() {
    // md.win switches CurrentCulture to de-DE; Rust's integer formatting has no locale at all, so
    // the assertion is the whole test: no thousands separator, as on the Mac.
    assert_eq!(strings::footer(12345, 67890), "12345 words · 67890 characters");
}
