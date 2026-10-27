//! Port of md.win `tests/Md.App.Logic.Tests/DerivedTextSchedulerTests.cs` (`DerivedTextSchedulerTests`
//! and `FooterTextTests`): shell-design §5.5 — the first fill is immediate, every later one waits
//! 250 ms and is cancelled by the next keystroke, a result that a newer edit has overtaken is
//! dropped, and the footer sentence is the Mac's word for word.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use md_core::export::diagram_svg::Diagram;
use md_logic::strings;
use md_logic::testing::*;
use md_logic::view::{DerivedText, DerivedTextScheduler, DocumentWindowState};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

type Queue = Rc<RefCell<Vec<Box<dyn FnOnce()>>>>;

struct Fixture {
    scheduler: Rc<FakeScheduler>,
    ui: Rc<FakeUiThread>,
    words: Rc<FakeWordCounter>,
    published: Rc<RefCell<Vec<DerivedText>>>,
}

impl Fixture {
    fn new() -> Self {
        Fixture {
            scheduler: Rc::new(FakeScheduler::new()),
            ui: Rc::new(FakeUiThread::new()),
            words: Rc::new(FakeWordCounter::new()),
            published: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// The computation runs where it is asked (inline, unless a test queues it).
    fn build(&self) -> Rc<DerivedTextScheduler> {
        self.build_with(md_core::export::diagram_svg::diagrams, None)
    }

    fn build_with(
        &self,
        diagrams: impl Fn(&str) -> Vec<Diagram> + 'static,
        queue: Option<Queue>,
    ) -> Rc<DerivedTextScheduler> {
        let derived = DerivedTextScheduler::with(
            self.scheduler.clone(),
            self.ui.clone(),
            self.words.clone(),
            diagrams,
            move |work: Box<dyn FnOnce()>| match &queue {
                Some(queue) => queue.borrow_mut().push(work),
                None => work(),
            },
        );
        let published = self.published.clone();
        derived.changed().connect(move |value| published.borrow_mut().push(value.clone()));
        derived
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

    derived.text_changed("# Title\n\nHello world");

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
    derived.text_changed("one");
    f.clear();

    derived.text_changed("one two");
    assert!(f.published().is_empty());
    assert_eq!(f.scheduler.pending_delays(), vec![ms(250)]);

    f.scheduler.advance(ms(249));
    assert!(f.published().is_empty());

    f.scheduler.advance(ms(1));
    let published = f.published();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].words, 2);
}

#[test]
fn the_next_keystroke_cancels_the_pending_computation() {
    let f = Fixture::new();
    let derived = f.build();
    derived.text_changed("one");
    f.clear();

    derived.text_changed("one two");
    f.scheduler.advance(ms(200));
    derived.text_changed("one two three");
    f.scheduler.advance(ms(200));

    assert!(f.published().is_empty());
    assert_eq!(f.scheduler.pending_timers(), 1);

    f.scheduler.advance(ms(50));
    let published = f.published();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].words, 3);
}

#[test]
fn a_burst_before_the_first_result_still_only_computes_once() {
    let f = Fixture::new();
    let derived = f.build();

    derived.text_changed("a");
    derived.text_changed("ab");
    derived.text_changed("abc");

    assert_eq!(f.published().len(), 1); // the immediate one
    f.scheduler.advance(ms(250));
    assert_eq!(f.published().len(), 2);
    assert_eq!(f.words.calls(), ["a", "abc"]);
}

#[test]
fn a_result_overtaken_by_a_newer_edit_is_dropped() {
    let f = Fixture::new();
    let queued: Queue = Rc::new(RefCell::new(Vec::new()));
    let derived = f.build_with(md_core::export::diagram_svg::diagrams, Some(queued.clone()));

    derived.text_changed("two stale words");
    derived.text_changed("fresh"); // the second is debounced
    f.scheduler.advance(ms(250));
    assert_eq!(queued.borrow().len(), 2);

    let mut works: Vec<Box<dyn FnOnce()>> = queued.borrow_mut().drain(..).collect();
    let older = works.remove(0);
    let newer = works.remove(0);
    newer(); // the newer computation lands first
    older(); // the older one comes back late

    assert_eq!(f.published().len(), 1);
    assert_eq!(f.words.calls(), ["fresh", "two stale words"]);
    assert_eq!(derived.current().words, 1);
}

#[test]
fn cancel_drops_both_the_pending_timer_and_anything_in_flight() {
    let f = Fixture::new();
    let queued: Queue = Rc::new(RefCell::new(Vec::new()));
    let derived = f.build_with(md_core::export::diagram_svg::diagrams, Some(queued.clone()));
    derived.text_changed("one");
    f.clear();
    derived.text_changed("one two");
    f.scheduler.advance(ms(250));

    derived.cancel();
    let works: Vec<Box<dyn FnOnce()>> = queued.borrow_mut().drain(..).collect();
    for work in works {
        work();
    }

    assert!(f.published().is_empty());
    assert_eq!(f.scheduler.pending_timers(), 0);
}

#[test]
fn characters_are_grapheme_clusters_and_notes_and_diagrams_come_along() {
    let f = Fixture::new();
    let derived = f.build_with(
        |_| {
            vec![Diagram {
                ordinal: 0,
                engine: "mermaid".into(),
                source: "graph TD;".into(),
                menu_title: "Diagram 1".into(),
                layout: None,
            }]
        },
        None,
    );

    derived.text_changed("e\u{0301}\u{1F469}\u{200D}\u{1F467}\n\n<!-- note: private -->\n");

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
    let f = Fixture::new();
    f.ui.set_run_inline(false);
    let derived = f.build();

    derived.text_changed("one two");

    assert!(f.published().is_empty());
    assert_eq!(f.ui.pending_posts(), 1);
    f.ui.run_posted();
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
}

#[test]
fn the_default_scheduler_computes_inline_with_md_cores_diagrams() {
    // LINUX DECISION (see DerivedTextScheduler): no thread hop — the work runs where it is asked.
    let f = Fixture::new();
    let derived = DerivedTextScheduler::new(f.scheduler.clone(), f.ui.clone(), f.words.clone());

    derived.text_changed("```mermaid\ngraph TD;\n```\n");

    assert_eq!(derived.current().diagrams.len(), 1);
    assert_eq!(derived.current().diagrams[0].engine, "mermaid");
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

    derived.text_changed("one two");
    assert_eq!(*footer.borrow(), ["2 words · 7 characters"]); // the first fill is immediate

    derived.text_changed("one two t");
    derived.text_changed("one two th");
    derived.text_changed("one two three");
    assert_eq!(footer.borrow().len(), 1); // the debounce is still running

    f.scheduler.advance(DerivedTextScheduler::DELAY);

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

    derived.text_changed("one two");
    derived.text_changed("one two");
    f.scheduler.advance(DerivedTextScheduler::DELAY);

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
