//! Port of md.win `tests/Md.App.Logic.Tests/TypingHooksTests.cs`: [`TypingHooks`] driven the way
//! the editor pane drives it, over a [`FakeTextBox`] that raises the widget's events in the
//! widget's order — with the two things the adapter's tests never see in the loop: the session's
//! echo of every keystroke back into `set_text` (both windows re-render on every session change)
//! and the widget's own Undo / Redo, which raise the same change events typing does. The
//! [`Editor`] below is the pane's wiring, line for line md.win's; the GTK shell must make the same
//! calls in the same places (the contract is in `text::typing_hooks`' report). The eight
//! consequences of smart-typing.md §3.4 are pinned here through the box, and the state machine
//! itself ([`CapitalTracker`], [`TextEdit`]) on its own below them.
//!
//! LINUX: the buffer's line break is LF (GTK), so every text md.win spells with CR is spelled with
//! LF, and the pane's plain Enter types "\n"; keys are GDK keyval names. md.win's
//! `SelfTestTyping.Scenarios` live in its WinUI app; they are copied below as test data.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use md_core::text::utf16::utf16_len;
use md_logic::settings::TypingSettings;
use md_logic::testing::FakeTextBox;
use md_logic::text::{CapitalTracker, DeleteDirection, TextEdit, TypingHooks};

// ── the modelled pane ──────────────────────────────────────────────────────────────────────

struct Inner {
    dispatcher: RefCell<VecDeque<Box<dyn FnOnce()>>>,
    tb: FakeTextBox,
    hooks: TypingHooks,
    /// The session's text: what the window writes on an edit and echoes back into `set_text`.
    session_text: RefCell<String>,
    echoes: Cell<usize>,
}

/// The pane and its window, as modelled: the hooks fed the box's text and selection at each event,
/// a plan applied through `select` + `set_selected_text`, `text_changed` reported as an edit unless
/// it is our own assignment, the session's echo through `set_text`, and the flags settled on the
/// next main-loop turn. Every user action is its own input event, so the queued turn runs before
/// the next one.
struct Editor(Rc<Inner>);

fn sel(tb: &FakeTextBox) -> (isize, isize) {
    (tb.selection_start() as isize, tb.selection_length() as isize)
}

impl Editor {
    fn new() -> Editor {
        let inner = Rc::new(Inner {
            dispatcher: RefCell::new(VecDeque::new()),
            tb: FakeTextBox::new(),
            hooks: TypingHooks::new(),
            session_text: RefCell::new(String::new()),
            echoes: Cell::new(0),
        });
        let weak: Weak<Inner> = Rc::downgrade(&inner);
        {
            let weak = weak.clone();
            inner.tb.before_text_changing().connect(move |new_text| {
                let e = weak.upgrade().unwrap();
                let (start, length) = sel(&e.tb);
                e.hooks.before_text_changing(&e.tb.text(), start, length, new_text);
            });
        }
        {
            let weak = weak.clone();
            inner.tb.text_changing().connect(move |_| {
                let e = weak.upgrade().unwrap();
                let (start, length) = sel(&e.tb);
                if let Some(plan) = e.hooks.text_changing(&e.tb.text(), start, length) {
                    let caret = plan.position + utf16_len(&plan.replacement);
                    Editor::apply(&e, plan.position, utf16_len(&plan.inserted), &plan.replacement, caret);
                }
            });
        }
        {
            let weak = weak.clone();
            inner.tb.text_changed().connect(move |_| {
                let e = weak.upgrade().unwrap();
                if e.hooks.is_replacing() {
                    return;
                }
                Editor::text_edited(&e, e.tb.text());
            });
        }
        inner.tb.paste().connect(move |_| Editor::announce_paste(&weak.upgrade().unwrap()));
        Editor(inner)
    }

    fn tb(&self) -> &FakeTextBox {
        &self.0.tb
    }

    fn hooks(&self) -> &TypingHooks {
        &self.0.hooks
    }

    fn text(&self) -> String {
        self.0.tb.text()
    }

    fn tracker(&self) -> CapitalTracker {
        self.0.hooks.tracker()
    }

    fn session_text(&self) -> String {
        self.0.session_text.borrow().clone()
    }

    fn echoes(&self) -> usize {
        self.0.echoes.get()
    }

    // ── the window ─────────────────────────────────────────────────────────────────────────

    fn text_edited(e: &Inner, text: String) {
        // The window's guard, then the session's edit → changed → set_text(session text).
        if *e.session_text.borrow() == text {
            return;
        }
        *e.session_text.borrow_mut() = text.clone();
        e.echoes.set(e.echoes.get() + 1);
        Editor::set_text_on(e, &text);
    }

    /// The pane's `set_text`: the echo is not a replacement; a differing text is assigned with the
    /// flag up.
    fn set_text_on(e: &Inner, text: &str) {
        if !e.hooks.external_text(&e.tb.text(), text) {
            return;
        }
        e.hooks.replacing(|| e.tb.set_text(text));
    }

    fn set_text(&self, text: &str) {
        Editor::set_text_on(&self.0, text);
    }

    /// What the windows call on a new undo generation: the hooks first, then the widget's history.
    fn reset_history(&self) {
        self.0.hooks.reset();
        self.0.tb.clear_undo_redo_history();
    }

    // ── the pane ───────────────────────────────────────────────────────────────────────────

    fn apply(e: &Inner, location: usize, length: usize, replacement: &str, caret: usize) {
        e.hooks.applying(|| {
            e.tb.select(location, length);
            e.tb.set_selected_text(replacement);
            e.tb.select(caret, 0);
        });
    }

    /// The find bar's Replace / Replace All (shell-design §3.5) and a Format command: the same
    /// select + replace pair, announced to the hooks as an external edit rather than applied under
    /// `applying` alone, so the tracking is cleared the way a Revert clears it. The widget's history
    /// is left alone: a replace is one undoable step.
    fn replace_range(&self, start: usize, length: usize, replacement: &str) {
        let e = &self.0;
        e.hooks.external_edit(|| {
            e.tb.select(start, length);
            e.tb.set_selected_text(replacement);
            e.tb.select(start + utf16_len(replacement), 0);
        });
    }

    fn enqueue(e: &Inner, action: impl FnOnce() + 'static) {
        e.dispatcher.borrow_mut().push_back(Box::new(action));
    }

    fn announce_paste(e: &Rc<Inner>) {
        e.hooks.announce_paste();
        let settle = Rc::downgrade(e);
        Editor::enqueue(e, move || settle.upgrade().unwrap().hooks.settle_paste());
    }

    fn announce_history(&self) {
        self.0.hooks.announce_history();
        let settle = Rc::downgrade(&self.0);
        Editor::enqueue(&self.0, move || settle.upgrade().unwrap().hooks.settle_history());
    }

    /// Backspace / Delete: announced, settled a turn later; the widget deletes.
    fn announce_deletion(&self, direction: DeleteDirection) {
        self.0.hooks.announce_deletion(direction);
        let settle = Rc::downgrade(&self.0);
        Editor::enqueue(&self.0, move || settle.upgrade().unwrap().hooks.settle_deletion());
    }

    /// One keystroke per scalar; each is its own input event.
    fn type_text(&self, text: &str) {
        for c in text.chars() {
            self.0.tb.type_text(&c.to_string());
            self.turn();
        }
    }

    fn backspace(&self) {
        self.backspace_times(1);
    }

    fn backspace_times(&self, times: usize) {
        for _ in 0..times {
            self.announce_deletion(DeleteDirection::Backward);
            self.0.tb.backspace();
            self.turn();
        }
    }

    fn delete(&self) {
        self.announce_deletion(DeleteDirection::Forward);
        self.0.tb.delete();
        self.turn();
    }

    /// Ctrl+X and Edit ▸ Cut: the widget removes the selection itself; nothing is announced (§3.4:
    /// a deletion).
    fn cut(&self) {
        self.0.tb.cut();
        self.turn();
    }

    fn select(&self, start: usize, length: usize) {
        self.0.tb.select(start, length);
    }

    /// Ctrl+Z (announced, the widget undoes) and Edit ▸ Undo are the same two calls.
    fn undo(&self) {
        self.announce_history();
        self.0.tb.undo();
        self.turn();
    }

    fn redo(&self) {
        self.announce_history();
        self.0.tb.redo();
        self.turn();
    }

    /// Ctrl+V: the widget raises paste, then inserts.
    fn key_paste(&self, clipboard: &str) {
        self.0.tb.paste_text(clipboard, true);
        self.turn();
    }

    /// Edit ▸ Paste through the pane: announced by the pane, then the widget's paste, which may or
    /// may not raise the event.
    fn menu_paste(&self, clipboard: &str, control_raises_paste: bool) {
        Editor::announce_paste(&self.0);
        self.0.tb.paste_text(clipboard, control_raises_paste);
        self.turn();
    }

    /// Enter through the hooks; the keyboard's modifiers are the pane's business and are not
    /// modelled. LINUX: the widget's own newline is "\n".
    fn enter(&self) -> bool {
        let (start, length) = sel(&self.0.tb);
        let Some(edit) = self.0.hooks.enter(&self.0.tb.text(), start, length) else {
            self.0.tb.type_text("\n");
            self.turn();
            return false;
        };
        Editor::apply(&self.0, edit.location, edit.length, &edit.replacement, edit.caret);
        self.turn();
        true
    }

    /// The main-loop turn between two input events.
    fn turn(&self) {
        loop {
            let next = self.0.dispatcher.borrow_mut().pop_front();
            let Some(action) = next else { break };
            action();
        }
    }
}

#[track_caller]
fn assert_tracked(e: &Editor, at: usize, capital: &str) {
    let t = e.tracker();
    assert_eq!(t.capital_at(), Some(at));
    assert_eq!(t.capital(), Some(capital));
    assert!(!t.is_armed());
}

#[track_caller]
fn assert_armed(e: &Editor, at: usize) {
    let t = e.tracker();
    assert_eq!(t.armed_at(), Some(at));
    assert!(!t.is_tracking());
}

#[track_caller]
fn assert_idle(e: &Editor) {
    let t = e.tracker();
    assert!(!t.is_tracking());
    assert!(!t.is_armed());
}

// ── the in-app self-test's scenarios, proved against the model first ──────────────────────

/// One key, as a self-test injects it and the model types it.
#[derive(Clone, Copy)]
enum Key {
    /// Characters, one keystroke per scalar.
    Type(&'static str),
    /// Enter with no modifier.
    Enter,
    /// Backspace at the cursor.
    Backspace,
    /// Ctrl+Z.
    Undo,
}

/// A box holding `initial` with the cursor at `caret` and a fresh history, the two Edit ▸ Typing
/// switches as given, the keys pressed in order — and the text the box must then hold.
struct Scenario {
    name: &'static str,
    initial: &'static str,
    caret: usize,
    continue_lists: bool,
    capitalize_sentences: bool,
    keys: &'static [Key],
    expected: &'static str,
}

/// md.win's `Md.App.Services.SelfTestTyping.Scenarios`, with LF line ends (LINUX). Enter's
/// continuation, the capital, the override gesture, Ctrl+Z after a capital and each switch off —
/// the 1.5 behaviour the CHANGELOG promises, in the smallest texts that show it.
const SCENARIOS: [Scenario; 14] = [
    // Continue Lists and Tables (capital off, so only Enter is under test).
    Scenario {
        name: "enter.continuesABullet",
        initial: "",
        caret: 0,
        continue_lists: true,
        capitalize_sentences: false,
        keys: &[Key::Type("- a"), Key::Enter],
        expected: "- a\n- ",
    },
    Scenario {
        name: "enter.countsANumberedItemOn",
        initial: "",
        caret: 0,
        continue_lists: true,
        capitalize_sentences: false,
        keys: &[Key::Type("1. a"), Key::Enter],
        expected: "1. a\n2. ",
    },
    Scenario {
        name: "enter.onAnEmptyItemEndsTheList",
        initial: "",
        caret: 0,
        continue_lists: true,
        capitalize_sentences: false,
        keys: &[Key::Type("- a"), Key::Enter, Key::Enter],
        expected: "- a\n",
    },
    Scenario {
        name: "enter.switchOffIsAPlainNewline",
        initial: "",
        caret: 0,
        continue_lists: false,
        capitalize_sentences: false,
        keys: &[Key::Type("- a"), Key::Enter],
        expected: "- a\n",
    },
    // Capitalize Sentences (lists off, so only the letter is under test).
    Scenario {
        name: "capital.firstLetterOfTheDocument",
        initial: "",
        caret: 0,
        continue_lists: false,
        capitalize_sentences: true,
        keys: &[Key::Type("hello")],
        expected: "Hello",
    },
    Scenario {
        name: "capital.firstLetterOfALine",
        initial: "One\n",
        caret: 4,
        continue_lists: false,
        capitalize_sentences: true,
        keys: &[Key::Type("two")],
        expected: "One\nTwo",
    },
    Scenario {
        name: "capital.afterASentenceEnds",
        initial: "One. ",
        caret: 5,
        continue_lists: false,
        capitalize_sentences: true,
        keys: &[Key::Type("two")],
        expected: "One. Two",
    },
    Scenario {
        name: "capital.overrideDeleteAndRetypeStaysLowercase",
        initial: "",
        caret: 0,
        continue_lists: false,
        capitalize_sentences: true,
        keys: &[Key::Type("m"), Key::Backspace, Key::Type("md")],
        expected: "md",
    },
    Scenario {
        name: "capital.ctrlZAfterACapitalRestoresTheLowercaseLetter",
        initial: "",
        caret: 0,
        continue_lists: false,
        capitalize_sentences: true,
        keys: &[Key::Type("m"), Key::Undo],
        expected: "m",
    },
    // 2026-09-27, the manual checklist: after that Ctrl+Z, a new line's first word went in lowercase.
    Scenario {
        name: "capital.ctrlZThenANewLineStillCapitalizes",
        initial: "",
        caret: 0,
        continue_lists: false,
        capitalize_sentences: true,
        keys: &[Key::Type("m"), Key::Undo, Key::Enter, Key::Type("hello")],
        expected: "m\nHello",
    },
    // The cursor after that Ctrl+Z is collapsed after the letter: a d typed on is the second
    // letter, not its replacement.
    Scenario {
        name: "capital.ctrlZThenTypingOnKeepsTheLetter",
        initial: "",
        caret: 0,
        continue_lists: false,
        capitalize_sentences: true,
        keys: &[Key::Type("m"), Key::Undo, Key::Type("d")],
        expected: "md",
    },
    // The buffer's text holds the capital itself, not only its screen.
    Scenario {
        name: "capital.aCapitalTypedLastIsInTheText",
        initial: "One. ",
        caret: 5,
        continue_lists: false,
        capitalize_sentences: true,
        keys: &[Key::Type("t")],
        expected: "One. T",
    },
    Scenario {
        name: "capital.switchOffLeavesTheLetterAlone",
        initial: "",
        caret: 0,
        continue_lists: false,
        capitalize_sentences: false,
        keys: &[Key::Type("hello")],
        expected: "hello",
    },
    // Both on, the way a writer has them.
    Scenario {
        name: "both.aListItemsFirstLetter",
        initial: "",
        caret: 0,
        continue_lists: true,
        capitalize_sentences: true,
        keys: &[Key::Type("- one"), Key::Enter, Key::Type("two")],
        expected: "- One\n- Two",
    },
];

/// md.win plays these on the real editor in `md.exe --selftest` and through the modelled pane here,
/// so every expectation is what the tested state machine does; a GTK self-test can replay the same
/// keys on a real `GtkTextView`.
#[test]
fn every_self_test_scenario_holds_in_the_model() {
    for scenario in &SCENARIOS {
        let e = Editor::new();
        e.hooks().configure(TypingSettings::new(scenario.continue_lists, scenario.capitalize_sentences));
        e.set_text(scenario.initial);
        e.reset_history();
        e.select(scenario.caret, 0);
        for key in scenario.keys {
            match *key {
                Key::Type(text) => e.type_text(text),
                Key::Enter => {
                    e.enter();
                }
                Key::Backspace => e.backspace(),
                Key::Undo => e.undo(),
            }
        }
        assert_eq!(e.text(), scenario.expected, "{}", scenario.name);
    }
}

/// LINUX: md.win's "TheSelfTestScenariosHaveUniqueNamesAndCrLineEnds" — the buffer reports LF, so a
/// CR in a vector could only ever fail on a real `GtkTextView`.
#[test]
fn the_self_test_scenarios_have_unique_names_and_cr_line_ends() {
    let mut names: Vec<&str> = SCENARIOS.iter().map(|s| s.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), SCENARIOS.len());
    for s in &SCENARIOS {
        assert!(!s.initial.contains('\r') && !s.expected.contains('\r'), "{}", s.name);
    }
}

// ── the echo (finding: set_text's echo disarmed the override after every keystroke) ───────

/// 1.6: a Format command is applied through the same `external_edit` as Replace, which clears the
/// tracking first. So wrapping a word md has just capitalized leaves no override armed where it
/// began — straight after the command or after its Undo — and the next word typed there is
/// capitalized: the bug the Apple ports had, pinned absent.
#[test]
fn a_formatting_edit_over_a_fresh_capital_leaves_no_override_behind() {
    let e = Editor::new();
    e.type_text("word");
    assert_eq!(e.text(), "Word");
    assert_tracked(&e, 0, "W");

    e.replace_range(0, 4, "**Word**");
    assert_eq!(e.text(), "**Word**");
    assert_idle(&e);
    e.undo();
    assert_eq!(e.text(), "Word");
    assert_idle(&e);

    e.select(0, 0);
    e.type_text("big");
    assert_eq!(e.text(), "BigWord");
}

/// The typed wrap: the widget inserts the `*` over the selection (which arms the override), then
/// the wrap replaces it through `external_edit`, which disarms it.
#[test]
fn a_typed_wrap_over_a_fresh_capital_leaves_no_override_behind() {
    let e = Editor::new();
    e.type_text("word");
    e.select(0, 4);
    e.type_text("*");
    assert_armed(&e, 0);
    e.replace_range(0, 1, "*Word*");
    assert_eq!(e.text(), "*Word*");
    assert_idle(&e);
}

#[test]
fn the_echo_of_a_keystroke_leaves_the_capital_tracked() {
    let e = Editor::new();
    e.type_text("m");
    assert_eq!(e.text(), "M");
    assert_eq!(e.echoes(), 1); // the session echoed the capital back into set_text
    assert_eq!(e.session_text(), "M");
    assert_tracked(&e, 0, "M"); // and the tracking survived it
}

#[test]
fn the_echo_of_a_deletion_leaves_the_override_armed() {
    let e = Editor::new();
    e.type_text("m");
    e.backspace();
    assert_eq!(e.text(), "");
    assert_eq!(e.session_text(), ""); // echoed, as identical text
    assert_armed(&e, 0);
}

#[test]
fn a_real_external_replacement_clears_the_tracking_and_the_override() {
    // Revert, Reload from Disk, an article switch: the text differs, both are gone (§3.4).
    let e = Editor::new();
    e.type_text("m");
    assert!(e.tracker().is_tracking());
    e.set_text("other text");
    assert_eq!(e.text(), "other text");
    assert_idle(&e);
    assert!(!e.tb().can_undo()); // the assignment cleared the history, as the design accepts for these

    e.set_text(""); // an armed override goes the same way
    e.type_text("m");
    assert_eq!(e.text(), "M");
    e.backspace();
    assert_armed(&e, 0);
    e.set_text("replaced again");
    assert_idle(&e);
}

#[test]
fn an_article_switch_to_identical_text_clears_the_tracking_and_the_override_too() {
    // A book with two articles whose text is the same, or Revert / Reload of a file the disk agrees
    // with: an external replacement (§3.4) that set_text sees as the echo's identical text. The
    // windows' signal is the session's undo generation, and it goes through the pane's
    // reset_history, which tells the hooks before it clears the widget's own history; set_text
    // itself stays the echo's gate.
    let e = Editor::new();
    e.type_text("m");
    e.backspace();
    assert_armed(&e, 0);
    e.reset_history(); // a new undo generation
    e.set_text(""); // the other article holds the same text: no assignment
    assert_idle(&e);
    assert!(!e.tb().can_undo());
    e.type_text("m");
    assert_eq!(e.text(), "M"); // judged afresh in the other article

    e.type_text("d"); // a tracked capital goes the same way
    assert_tracked(&e, 0, "M");
    e.reset_history();
    e.set_text("Md");
    assert_idle(&e);
    assert_eq!(e.text(), "Md");
}

#[test]
fn reset_forgets_the_tracking_and_the_plan_and_leaves_the_flags_alone() {
    // The flags describe the input in flight — a composition, a history step, a paste — which a
    // re-render in the middle of one does not end.
    let hooks = TypingHooks::new();
    hooks.before_text_changing("", 0, 0, "m");
    assert!(hooks.has_pending_plan());
    hooks.tracker_mut().produced(0, "M");
    hooks.composition_started();
    hooks.announce_history();
    hooks.announce_paste();
    hooks.reset();
    assert!(!hooks.has_pending_plan());
    assert!(!hooks.tracker().is_tracking());
    assert!(!hooks.tracker().is_armed());
    assert!(hooks.is_composing());
    assert!(hooks.is_history());
    assert!(hooks.is_pasting());
}

#[test]
fn the_echo_is_not_an_edit_and_the_assignment_is_not_one() {
    let e = Editor::new();
    e.type_text("m");
    let echoes = e.echoes();
    e.set_text("M"); // a second echo of the same text
    assert_eq!(e.echoes(), echoes);
    e.set_text("Reverted");
    assert_eq!(e.echoes(), echoes); // our own assignment is not reported as an edit
}

// ── smart-typing.md §3.4, the eight consequences, through the box ──────────────────────────

#[test]
fn consequence1_type_md_backspace_twice_and_type_md_again() {
    let e = Editor::new();
    e.type_text("md");
    assert_eq!(e.text(), "Md");
    assert_tracked(&e, 0, "M");
    e.backspace_times(2);
    assert_eq!(e.text(), "");
    assert_armed(&e, 0); // the second Backspace removed the capital
    e.type_text("md");
    assert_eq!(e.text(), "md");
    assert_idle(&e); // spent by the m; the d was elsewhere anyway
    assert_eq!(e.session_text(), "md");
}

#[test]
fn consequence2_type_i_o_s_backspace_thrice_and_type_i_o_s_again() {
    let e = Editor::new();
    e.type_text("iOS");
    assert_eq!(e.text(), "IOS");
    assert_tracked(&e, 0, "I"); // the O and the S were typed capitals, after p
    e.backspace_times(3);
    assert_armed(&e, 0);
    e.type_text("iOS");
    assert_eq!(e.text(), "iOS");
    assert_idle(&e);
}

#[test]
fn consequence3_five_keystrokes_after_the_capital_then_five_backspaces_and_the_retype() {
    // The four insertions after the capital disarm nothing, because nothing is armed: the capital
    // is tracked, and only its removal arms.
    let e = Editor::new();
    e.type_text("md is");
    assert_eq!(e.text(), "Md is");
    assert_tracked(&e, 0, "M");
    e.backspace_times(4);
    assert_eq!(e.text(), "M");
    assert_tracked(&e, 0, "M"); // deletions after p leave it alone
    e.backspace();
    assert_armed(&e, 0);
    e.type_text("md is");
    assert_eq!(e.text(), "md is");
    assert_idle(&e);
}

#[test]
fn consequence4_select_the_whole_word_and_retype_it() {
    let e = Editor::new();
    e.type_text("md");
    e.select(0, 2);
    e.type_text("md");
    assert_eq!(e.text(), "md");
    assert_idle(&e); // the m over the selection armed and went in as typed; the d cleared
}

#[test]
fn consequence5_a_letter_typed_in_front_of_a_standing_capital_is_capitalized_and_becomes_the_tracked_one() {
    let e = Editor::new();
    e.type_text("m");
    e.select(0, 0);
    e.type_text("a");
    assert_eq!(e.text(), "AM"); // the M was not deleted: the insertion at p shifted it
    assert_tracked(&e, 0, "A"); // and the new capital is the tracked one; the old M is forgotten
    e.backspace();
    assert_eq!(e.text(), "M");
    assert_armed(&e, 0); // removing the A arms; the M at 0 is not md's memory any more
}

#[test]
fn consequence6_undo_restores_the_lowercase_letter_arms_the_override_and_typing_on_keeps_it() {
    let e = Editor::new();
    e.type_text("m");
    assert_eq!(e.text(), "M");
    e.undo();
    assert_eq!(e.text(), "m"); // the undo restores the lowercase (§3.5)
    assert_eq!(e.tb().selection_start(), 1);
    assert_armed(&e, 0); // the undo is an edit that removed the capital
    e.type_text("d");
    assert_eq!(e.text(), "md"); // an insertion elsewhere than q clears; nothing re-judges the m
    assert_idle(&e);
}

#[test]
fn consequence7_a_cut_before_the_capital_then_backspace_and_retype() {
    let e = Editor::new();
    e.type_text("Xxx. ");
    e.type_text("m");
    assert_eq!(e.text(), "Xxx. M");
    assert_tracked(&e, 5, "M");
    e.select(0, 5);
    e.cut();
    assert_eq!(e.text(), "M");
    assert_tracked(&e, 0, "M"); // offset tracking survives the cut
    e.select(1, 0); // the cut leaves the cursor before the M; ⌫ needs it after
    e.backspace();
    assert_eq!(e.text(), "");
    assert_armed(&e, 0);
    e.type_text("m");
    assert_eq!(e.text(), "m");
}

#[test]
fn consequence8_deleting_the_letter_after_the_capital_arms_nothing() {
    let e = Editor::new();
    e.type_text("m");
    e.type_text("d");
    assert_eq!(e.text(), "Md");
    e.backspace();
    assert_eq!(e.text(), "M");
    assert_tracked(&e, 0, "M"); // the capital still stands
    e.type_text("d");
    assert_eq!(e.text(), "Md");
    assert_tracked(&e, 0, "M");
}

// ── more of §3.4 through the box ───────────────────────────────────────────────────────────

#[test]
fn a_deletion_before_an_armed_override_shifts_it() {
    let e = Editor::new();
    e.type_text("Xxx. m");
    e.backspace();
    assert_eq!(e.text(), "Xxx. ");
    assert_armed(&e, 5);
    e.select(0, 5);
    e.cut();
    assert_eq!(e.text(), "");
    assert_armed(&e, 0); // deletions never clear; one before q shifts it
    e.type_text("m");
    assert_eq!(e.text(), "m");
}

#[test]
fn an_insertion_elsewhere_than_q_clears_the_override() {
    let e = Editor::new();
    e.type_text("Xxx. m");
    e.backspace();
    assert_armed(&e, 5);
    e.select(0, 0);
    e.type_text("y");
    assert_eq!(e.text(), "YXxx. "); // judged like any letter at a line start
    assert_tracked(&e, 0, "Y");
    e.select(6, 0);
    e.type_text("m");
    assert_eq!(e.text(), "YXxx. M"); // nothing was armed at 6 any more
}

#[test]
fn a_non_word_insertion_at_q_leaves_the_override_armed() {
    // A digit, a newline, a capital typed where the deleted capital was: not the retype, and not
    // "elsewhere" either — the override waits.
    let e = Editor::new();
    e.type_text("m");
    e.backspace();
    e.type_text("1");
    assert_eq!(e.text(), "1");
    assert_armed(&e, 0);
    e.select(0, 0);
    e.type_text("m");
    assert_eq!(e.text(), "m1"); // the retype, at last
    assert_idle(&e);
}

#[test]
fn a_capital_typed_over_the_deleted_capitals_slot_and_typed_on_from_is_judged_like_any_text() {
    let e = Editor::new();
    e.type_text("m");
    e.backspace();
    e.type_text("M"); // the writer types the capital themself
    assert_armed(&e, 0);
    e.type_text("d. x");
    assert_eq!(e.text(), "Md. X"); // the d cleared the override; the x is a sentence start
    assert_tracked(&e, 4, "X");
}

#[test]
fn a_paste_before_the_capital_shifts_it() {
    let e = Editor::new();
    e.type_text("m");
    e.select(0, 0);
    e.key_paste("ab");
    assert_eq!(e.text(), "abM"); // a paste is never capitalized, and sits before p
    assert_tracked(&e, 2, "M");
    e.select(3, 0);
    e.backspace();
    assert_armed(&e, 2);
    e.type_text("m");
    assert_eq!(e.text(), "abm");
}

#[test]
fn a_paste_over_the_capital_that_puts_it_back_keeps_it_standing() {
    // §3.4: a replacement covering p removes the capital "unless the inserted text puts that same
    // capital scalar back at the same offset".
    let e = Editor::new();
    e.type_text("m");
    e.select(0, 1);
    e.key_paste("Md");
    assert_eq!(e.text(), "Md");
    assert_tracked(&e, 0, "M");
    e.backspace_times(2);
    assert_armed(&e, 0);
    e.type_text("m");
    assert_eq!(e.text(), "m");
}

#[test]
fn a_paste_at_q_is_a_word_insertion_at_q_and_spends_the_override() {
    let e = Editor::new();
    e.type_text("m");
    e.backspace();
    e.key_paste("md");
    assert_eq!(e.text(), "md");
    assert_idle(&e);
}

#[test]
fn a_deletion_covering_the_capital_and_more_arms_at_its_start() {
    let e = Editor::new();
    e.type_text("md is");
    e.select(0, 3);
    e.delete();
    assert_eq!(e.text(), "is");
    assert_armed(&e, 0);
    e.type_text("m");
    assert_eq!(e.text(), "mis");
}

#[test]
fn forward_delete_of_the_capital_arms_the_override_too() {
    let e = Editor::new();
    e.type_text("md");
    e.select(0, 0);
    e.delete();
    assert_eq!(e.text(), "d");
    assert_armed(&e, 0);
    e.type_text("m");
    assert_eq!(e.text(), "md");
}

#[test]
fn backspace_of_the_capital_next_to_the_writers_identical_letter_arms_the_override() {
    // `I` (md's) then `I` (the writer's, Shift+I): the two units are the same, so the text alone
    // cannot say which one a Backspace or Delete removed — the smallest diff picks the last of the
    // run. The key says which: the deletion is announced, and the shrink is anchored at the cursor
    // on that side (§3.4: the edit that removed the capital arms at its start).
    let e = Editor::new();
    e.type_text("i");
    assert_eq!(e.text(), "I");
    e.type_text("I");
    assert_eq!(e.text(), "II");
    assert_tracked(&e, 0, "I");
    e.select(1, 0);
    e.backspace(); // [0, 1): md's capital
    assert_eq!(e.text(), "I");
    assert_armed(&e, 0);
    e.type_text("i");
    assert_eq!(e.text(), "iI");
    assert_idle(&e);
}

#[test]
fn delete_of_the_capital_next_to_the_writers_identical_letter_arms_the_override() {
    let e = Editor::new();
    e.type_text("i");
    e.type_text("I");
    assert_eq!(e.text(), "II");
    e.select(0, 0);
    e.delete(); // [0, 1): md's capital
    assert_eq!(e.text(), "I");
    assert_armed(&e, 0);
    e.type_text("i");
    assert_eq!(e.text(), "iI");
    assert_idle(&e);
}

#[test]
fn deleting_the_writers_identical_letter_next_to_the_capital_leaves_it_tracked() {
    // The mirror: the same two keys removing the writer's I, not md's — Delete with the cursor
    // between the two, Backspace with the cursor after them.
    let e = Editor::new();
    e.type_text("i");
    e.type_text("I");
    e.select(1, 0);
    e.delete(); // [1, 2): the writer's
    assert_eq!(e.text(), "I");
    assert_tracked(&e, 0, "I");

    e.type_text("I");
    assert_eq!(e.text(), "II");
    e.select(2, 0);
    e.backspace(); // [1, 2): the writer's again
    assert_eq!(e.text(), "I");
    assert_tracked(&e, 0, "I");
    e.type_text("i"); // after the standing capital: judged, not a sentence start
    assert_eq!(e.text(), "Ii");
    assert_tracked(&e, 0, "I");
}

#[test]
fn the_one_scalar_retype_over_the_tracked_capital_goes_in_as_typed_and_arms_the_override() {
    // §3.4's last rule: "inserted as typed — and, since it removed the capital, it arms the
    // override". The retype is not what spends it; the next word insertion at q is.
    let e = Editor::new();
    e.type_text("md is");
    e.select(0, 1);
    e.type_text("m");
    assert_eq!(e.text(), "md is");
    assert_armed(&e, 0);
    e.backspace();
    assert_eq!(e.text(), "d is");
    assert_armed(&e, 0);
    e.type_text("m");
    assert_eq!(e.text(), "md is");
    assert_idle(&e);
}

#[test]
fn the_one_scalar_retype_over_a_capital_md_did_not_write_is_as_typed_but_arms_nothing() {
    let e = Editor::new();
    e.type_text("m");
    e.backspace();
    e.type_text("T"); // the writer's own capital, at the deleted one's slot
    e.type_text("he");
    assert_eq!(e.text(), "The");
    assert_idle(&e);
    e.select(0, 1);
    e.type_text("t");
    assert_eq!(e.text(), "the"); // the retype rule needs no state
    assert_idle(&e); // and, having removed no capital of md's, arms nothing
    e.backspace();
    e.type_text("t");
    assert_eq!(e.text(), "The"); // so a Backspace and a retype are judged afresh
}

#[test]
fn the_enter_edit_is_tracked_like_any_other_edit() {
    // The exit rule on an empty item deletes the marker: a deletion ending at q shifts it.
    let e = Editor::new();
    e.type_text("- m");
    assert_eq!(e.text(), "- M");
    assert_tracked(&e, 2, "M");
    e.backspace();
    assert_armed(&e, 2);
    assert!(e.enter());
    assert_eq!(e.text(), "");
    assert_armed(&e, 0);
    e.type_text("m");
    assert_eq!(e.text(), "m");
}

#[test]
fn a_new_capital_replaces_the_tracked_one_and_the_old_one_is_forgotten_not_armed() {
    let e = Editor::new();
    e.type_text("m");
    e.type_text(". x");
    assert_eq!(e.text(), "M. X");
    assert_tracked(&e, 3, "X");
    e.select(1, 0);
    e.backspace(); // deletes the M — the capital md forgot
    assert_eq!(e.text(), ". X");
    assert_tracked(&e, 2, "X"); // shifted, not armed
    e.type_text("m");
    assert_eq!(e.text(), "M. X"); // judged afresh: a line start
    assert_tracked(&e, 0, "M");
}

#[test]
fn a_composition_is_tracked_though_never_judged() {
    let e = Editor::new();
    e.type_text("m");
    e.hooks().composition_started();
    e.type_text("x");
    assert_eq!(e.text(), "Mx");
    assert_tracked(&e, 0, "M");
    e.backspace_times(2);
    assert_armed(&e, 0);
    e.hooks().composition_ended();
    e.type_text("m");
    assert_eq!(e.text(), "m");
}

// ── history (finding: Undo / Redo were re-judged as typing) ────────────────────────────────

#[test]
fn redo_reinserts_the_letter_as_it_was_and_keeps_the_redo_stack() {
    let e = Editor::new();
    e.type_text("hello");
    assert_eq!(e.text(), "Hello");
    let units = e.tb().undo_units(); // h, h→H, e, l, l, o
    assert_eq!(units, 6);
    for _ in 0..units {
        e.undo();
    }
    assert_eq!(e.text(), "");
    assert_eq!(e.tb().redo_units(), units);
    assert_armed(&e, 0); // the undo of the capital armed it; the undo of the h collapsed q onto 0

    e.redo();
    assert_eq!(e.text(), "h"); // not H: a Redo is not typing
    assert_eq!(e.tb().redo_units(), units - 1); // and nothing new was edited over it
    assert_idle(&e); // a word insertion at q, tracked though not judged: spent
    for _ in 1..units {
        e.redo();
    }
    assert_eq!(e.text(), "Hello");
    assert!(!e.tb().can_redo());
    assert_idle(&e); // Redo restores the text, not md's memory of the capital
}

#[test]
fn undo_of_a_deletion_restores_the_text_that_was_there() {
    // "md rocks" through the override, the word deleted, then Ctrl+Z: the widget re-inserts "md"
    // at 0 with the cursor after it, which the change event cannot tell from typing.
    let e = Editor::new();
    e.type_text("m");
    e.backspace();
    e.type_text("md rocks");
    assert_eq!(e.text(), "md rocks");
    e.select(0, 2);
    e.delete();
    assert_eq!(e.text(), " rocks");
    e.undo();
    assert_eq!(e.text(), "md rocks"); // never "Md rocks": text that never existed
}

#[test]
fn undo_immediately_after_a_capital_restores_the_lowercase_letter_and_the_caret() {
    // §3.5 in the two-unit model: the widget's insertion and our replacement are two units, so one
    // Ctrl+Z takes back the capital alone, keeps the cursor, and arms the override at p.
    let e = Editor::new();
    e.type_text("h");
    assert_eq!(e.text(), "H");
    assert_eq!(e.tb().undo_units(), 2);
    e.undo();
    assert_eq!(e.text(), "h");
    assert_eq!(e.tb().selection_start(), 1);
    assert_eq!(e.tb().selection_length(), 0);
    assert_armed(&e, 0);
    e.undo();
    assert_eq!(e.text(), "");
    assert_armed(&e, 0); // a deletion never clears it
    e.type_text("h");
    assert_eq!(e.text(), "h");
}

#[test]
fn undo_then_backspace_and_retype_is_the_gesture_too() {
    let e = Editor::new();
    e.type_text("m");
    e.undo();
    assert_eq!(e.text(), "m");
    e.backspace();
    e.type_text("m");
    assert_eq!(e.text(), "m");
    assert_idle(&e);
}

#[test]
fn undo_of_the_capital_arms_the_override_wherever_it_sits_in_the_history() {
    // "hi": two Ctrl+Z give "h" with the cursor kept. The first undo deletes after p; the second is
    // the edit that removed the capital, and arms — nothing typed on had "disarmed" anything,
    // because nothing was armed while the capital stood.
    let e = Editor::new();
    e.type_text("hi");
    assert_eq!(e.text(), "Hi");
    assert_tracked(&e, 0, "H");
    e.undo();
    assert_eq!(e.text(), "H");
    assert_tracked(&e, 0, "H");
    e.undo();
    assert_eq!(e.text(), "h");
    assert_eq!(e.tb().selection_start(), 1);
    assert_armed(&e, 0);
    e.type_text("i");
    assert_eq!(e.text(), "hi"); // the h is not re-judged; the i clears
    assert_idle(&e);
}

#[test]
fn redo_of_the_capital_puts_the_text_back_and_leaves_the_override_armed() {
    // The decision for Redo: it is a non-word insertion at q — the text comes back, the override
    // stays, and the capital is not tracked again. Deleting it and retyping is still the
    // lowercase; typing on clears, as after any capital.
    let e = Editor::new();
    e.type_text("m");
    e.undo();
    e.redo();
    assert_eq!(e.text(), "M");
    assert_eq!(e.tb().selection_start(), 1);
    assert_armed(&e, 0);
    e.backspace();
    assert_eq!(e.text(), "");
    assert_armed(&e, 0);
    e.type_text("m");
    assert_eq!(e.text(), "m");
    assert_idle(&e);
}

#[test]
fn redo_of_the_capital_then_typing_on_clears_the_override() {
    let e = Editor::new();
    e.type_text("m");
    e.undo();
    e.redo();
    e.type_text("d");
    assert_eq!(e.text(), "Md");
    assert_idle(&e);
    e.backspace_times(2);
    e.type_text("m");
    assert_eq!(e.text(), "M"); // judged afresh: nothing remembered the redone capital
}

#[test]
fn a_history_step_with_nothing_to_do_does_not_skip_the_next_keystroke() {
    let e = Editor::new();
    e.undo(); // nothing to undo: no change, the flag settles on the turn
    assert!(!e.hooks().is_history());
    e.type_text("m");
    assert_eq!(e.text(), "M");
}

#[test]
fn the_history_flag_is_not_consumed_by_the_first_change_it_covers() {
    // One Undo may report more than one change; every one of them is history until the turn.
    let h = TypingHooks::new();
    h.announce_history();
    h.before_text_changing("", 0, 0, "h");
    assert!(!h.has_pending_plan());
    h.before_text_changing("h", 1, 0, "he");
    assert!(!h.has_pending_plan());
    assert!(h.is_history());
    h.settle_history();
    h.before_text_changing("", 0, 0, "h");
    assert!(h.has_pending_plan());
}

/// LINUX: md.win's VK-based theory over GDK keyval names. GtkTextView 4.22 binds Ctrl+Z (undo),
/// Ctrl+Y and Shift+Ctrl+Z (redo); with Shift held GTK reports the keyval `Z`.
#[test]
fn only_ctrl_z_and_ctrl_y_without_alt_announce_history() {
    let rows: [(&str, bool, bool, bool); 9] = [
        ("z", true, false, true),   // Ctrl+Z
        ("y", true, false, true),   // Ctrl+Y
        ("Z", true, false, true),   // LINUX: Shift+Ctrl+Z, GTK's redo
        ("Y", true, false, true),   // LINUX: Shift+Ctrl+Y
        ("z", false, false, false), // z
        ("y", false, false, false), // y
        ("z", true, true, false),   // a layout's Ctrl+Alt+Z letter is judged
        ("v", true, false, false),  // Ctrl+V is a paste, with its own flag
        ("a", true, false, false),
    ];
    for (key, control, alt, expected) in rows {
        assert_eq!(TypingHooks::is_history_chord(key, control, alt), expected, "{key} ctrl={control} alt={alt}");
    }
}

// ── paste (finding: Edit ▸ Paste did not arm the paste flag) ───────────────────────────────

#[test]
fn ctrl_v_is_never_capitalized() {
    let e = Editor::new();
    e.key_paste("nettrash");
    assert_eq!(e.text(), "nettrash");
    assert_idle(&e);
}

#[test]
fn edit_menu_paste_is_never_capitalized_whether_or_not_paste_from_clipboard_raises_the_event() {
    for control_raises_paste in [true, false] {
        let e = Editor::new();
        e.menu_paste("nettrash", control_raises_paste);
        assert_eq!(e.text(), "nettrash");
        assert!(!e.hooks().is_pasting()); // consumed, or settled on the turn
    }
}

#[test]
fn a_paste_nobody_announces_would_be_capitalized_which_is_why_the_edit_row_goes_through_the_pane() {
    let e = Editor::new();
    e.tb().paste_text("nettrash", false); // the widget's paste reached directly, no event
    e.turn();
    assert_eq!(e.text(), "Nettrash");
}

#[test]
fn an_empty_clipboard_does_not_skip_the_next_keystroke() {
    let e = Editor::new();
    e.menu_paste("", false);
    assert!(!e.hooks().is_pasting());
    e.type_text("m");
    assert_eq!(e.text(), "M");
}

// ── the rest of the pane's sequencing ──────────────────────────────────────────────────────

#[test]
fn our_own_replacement_is_not_judged_again() {
    // The capital is applied through the same events; it is tracked once, at 0, and the session
    // sees one edit per keystroke's final text.
    let e = Editor::new();
    e.type_text("m");
    assert_eq!(e.text(), "M");
    assert_tracked(&e, 0, "M");
    assert_eq!(e.session_text(), "M");
}

#[test]
fn a_composition_skips_both_features() {
    let e = Editor::new();
    e.hooks().composition_started();
    e.type_text("m");
    assert_eq!(e.text(), "m");
    e.type_text(" ");
    e.type_text("- a");
    assert!(!e.enter());
    assert_eq!(e.text(), "m - a\n");
    e.hooks().composition_ended();
    e.type_text("x");
    assert_eq!(e.text(), "m - a\nX");
}

#[test]
fn capitalize_sentences_off_is_never_judged_and_drops_the_gesture() {
    let e = Editor::new();
    e.type_text("m");
    assert!(e.tracker().is_tracking());
    e.hooks().configure(TypingSettings::new(true, false));
    assert_idle(&e);
    e.backspace();
    e.type_text("hello");
    assert_eq!(e.text(), "hello");
    assert_idle(&e);
    e.hooks().configure(TypingSettings::DEFAULTS);
    e.type_text(". x");
    assert_eq!(e.text(), "hello. X");
    assert_tracked(&e, 7, "X");
}

#[test]
fn enter_continues_the_list_and_the_capital_before_it_stays_tracked() {
    let e = Editor::new();
    e.type_text("- m");
    assert_eq!(e.text(), "- M");
    assert_tracked(&e, 2, "M");
    assert!(e.enter());
    assert_eq!(e.text(), "- M\n- "); // LINUX: the buffer's LF spelling of the continuation
    assert_tracked(&e, 2, "M"); // an edit after p leaves it alone
    assert_eq!(e.tb().selection_start(), 6);
    e.type_text("n");
    assert_eq!(e.text(), "- M\n- N");
    assert_tracked(&e, 6, "N");
}

#[test]
fn continue_lists_off_leaves_enter_to_the_control() {
    let e = Editor::new();
    e.hooks().configure(TypingSettings::new(false, true));
    e.type_text("- m");
    assert!(!e.enter());
    assert_eq!(e.text(), "- M\n");
    assert_tracked(&e, 2, "M"); // the widget's newline is tracked like any edit
}

/// md.win also pins `ArgumentNullException` for a null argument; a Rust `&str` cannot be null.
#[test]
fn external_text_checks_its_arguments() {
    let h = TypingHooks::new();
    assert!(!h.external_text("same", "same"));
    assert!(h.external_text("one", "two"));
}

#[test]
fn the_defaults_are_both_on() {
    let h = TypingHooks::new();
    assert!(h.continue_lists());
    assert!(h.capitalize_sentences());
    assert!(!h.is_composing());
    assert!(!h.is_pasting());
    assert!(!h.is_history());
    assert!(!h.is_replacing());
    assert!(!h.tracker().is_tracking());
    assert!(!h.tracker().is_armed());
}

#[test]
fn a_non_bmp_letter_is_one_keystroke_and_one_capital() {
    // Deseret 𐐨 (U+10428) → 𐐀 (U+10400): two units in, two units replaced, tracked at 0.
    let e = Editor::new();
    e.type_text("\u{10428}");
    assert_eq!(e.text(), "\u{10400}");
    assert_tracked(&e, 0, "\u{10400}");
    e.backspace();
    assert_eq!(e.text(), "");
    assert_armed(&e, 0);
    e.type_text("\u{10428}");
    assert_eq!(e.text(), "\u{10428}");
}

// ── §3.5 the find bar's Replace: an external edit, and one undo unit ───────────────────────

/// The replacement is the writer's own text, so it goes in exactly as typed. The first half of this
/// test proves the position IS one the capital machinery acts on — typed there, `beta` becomes
/// `Beta` — which is what makes the second half mean anything.
#[test]
fn a_replace_is_not_typing_and_is_never_capitalized() {
    let typed = Editor::new();
    typed.set_text("Ready. alpha");
    typed.select(7, 5);
    typed.type_text("beta");
    assert_eq!(typed.text(), "Ready. Beta"); // typing there capitalizes

    let replaced = Editor::new();
    replaced.set_text("Ready. alpha");
    replaced.replace_range(7, 5, "beta");
    assert_eq!(replaced.text(), "Ready. beta"); // replacing there does not
    assert_eq!(replaced.session_text(), "Ready. beta"); // and the session saw the edit
}

/// §3.4: a replace is an external replacement as far as the tracking goes — a Revert, a Reload, an
/// article switch. The capital md wrote is forgotten rather than followed through moved text, and
/// no override is armed: the writer did not delete that capital, the find bar did.
#[test]
fn a_replace_clears_the_tracked_capital_and_arms_nothing() {
    let e = Editor::new();
    e.type_text("m");
    assert_tracked(&e, 0, "M");

    e.replace_range(0, 1, "q");
    assert_eq!(e.text(), "q");
    assert_idle(&e);
}

#[test]
fn a_replace_over_an_armed_override_clears_that_too() {
    let e = Editor::new();
    e.type_text("m");
    e.backspace();
    assert_armed(&e, 0);

    e.replace_range(0, 0, "x");
    assert_eq!(e.text(), "x");
    assert_idle(&e);
}

/// The whole point of planning Replace All as ONE span: one replacement is one undo unit, so a
/// single Ctrl+Z puts every hit back at once.
///
/// LINUX: md.win plans the span with `Md.App.Logic.Documents.TextSearch.ReplaceAll`, which belongs
/// to the documents module's port; the same plan — the first hit's start to the last hit's end,
/// every hit replaced inside it — is spelled out here.
#[test]
fn replace_all_is_one_undo_unit_however_many_hits_it_covered() {
    const TEXT: &str = "alpha beta alpha gamma alpha";
    let e = Editor::new();
    e.set_text(TEXT);
    assert_eq!(e.tb().undo_units(), 0); // the assignment cleared the history

    let hits: Vec<usize> = TEXT.match_indices("alpha").map(|(i, _)| i).collect();
    assert_eq!(hits.len(), 3);
    let (start, end) = (hits[0], hits[2] + "alpha".len());
    let replacement = TEXT[start..end].replace("alpha", "X");
    e.replace_range(start, end - start, &replacement);

    assert_eq!(e.text(), "X beta X gamma X");
    assert_eq!(e.session_text(), "X beta X gamma X");
    assert_eq!(e.tb().undo_units(), 1); // one step for all three

    e.undo();
    assert_eq!(e.text(), TEXT);
    assert_eq!(e.session_text(), TEXT);
    assert_eq!(e.tb().undo_units(), 0);
}

/// The session's echo of a replace is identical text, like the echo of a keystroke: it must not be
/// assigned again (which would clear the undo history the replace just earned).
#[test]
fn the_echo_of_a_replace_leaves_the_undo_history_standing() {
    let e = Editor::new();
    e.set_text("alpha");
    e.replace_range(0, 5, "omega");
    assert_eq!(e.echoes(), 1);
    assert_eq!(e.session_text(), "omega");
    assert_eq!(e.tb().undo_units(), 1);
}

/// Typing after a replace is judged again: the external edit clears the state, it does not switch
/// the feature off.
#[test]
fn typing_after_a_replace_is_still_typing() {
    let e = Editor::new();
    e.set_text("Ready. alpha");
    e.replace_range(7, 5, "beta");
    e.select(e.tb().len(), 0);
    e.type_text(" Done. gamma");
    assert_eq!(e.text(), "Ready. beta Done. Gamma");
}

// ── the state machine, pure (§3.4) ─────────────────────────────────────────────────────────

fn te(start: usize, length: usize, inserted: &str) -> TextEdit {
    TextEdit::new(start, length, inserted)
}

fn armed(at: usize) -> CapitalTracker {
    let mut t = CapitalTracker::new();
    t.produced(at, "M");
    t.edited(&te(at, 1, ""), false);
    assert_eq!(t.armed_at(), Some(at));
    t
}

#[test]
fn the_tracker_starts_idle() {
    let t = CapitalTracker::new();
    assert!(!t.is_tracking());
    assert!(!t.is_armed());
    assert_eq!(t.capital_at(), None);
    assert_eq!(t.capital(), None);
    assert_eq!(t.armed_at(), None);
}

#[test]
fn produced_tracks_the_capital_and_forgets_whatever_was_there() {
    let mut t = CapitalTracker::new();
    t.produced(4, "M");
    assert_eq!(t.capital_at(), Some(4));
    assert_eq!(t.capital(), Some("M"));
    assert!(!t.is_armed());
    t.produced(7, "T"); // the old one is forgotten, not armed
    assert_eq!(t.capital_at(), Some(7));
    assert_eq!(t.capital(), Some("T"));
    assert!(!t.is_armed());

    let mut a = armed(3);
    a.produced(0, "A");
    assert!(!a.is_armed());
    assert_eq!(a.capital_at(), Some(0));
}

#[test]
fn an_edit_whose_range_ends_at_or_before_the_capital_shifts_it() {
    let mut t = CapitalTracker::new();
    t.produced(5, "M");
    assert!(!t.edited(&te(0, 0, "ab"), true)); // an insertion before it
    assert_eq!(t.capital_at(), Some(7));
    t.edited(&te(0, 5, ""), false); // a cut before it
    assert_eq!(t.capital_at(), Some(2));
    t.edited(&te(2, 0, "x"), true); // an insertion AT p is before it (§3.4: "included")
    assert_eq!(t.capital_at(), Some(3));
    t.edited(&te(0, 3, "y"), true); // a replacement ending at p
    assert_eq!(t.capital_at(), Some(1));
    assert_eq!(t.capital(), Some("M"));
    assert!(!t.is_armed());
}

#[test]
fn an_edit_after_the_capital_leaves_it_alone() {
    let mut t = CapitalTracker::new();
    t.produced(2, "M");
    t.edited(&te(3, 0, "d"), true);
    t.edited(&te(3, 1, ""), false);
    t.edited(&te(3, 4, "paste"), false);
    assert_eq!(t.capital_at(), Some(2));
    assert!(!t.is_armed());
}

#[test]
fn an_edit_covering_the_capital_removes_it_and_arms_at_the_edits_start() {
    let mut t = CapitalTracker::new();
    t.produced(5, "M");
    assert!(!t.edited(&te(5, 1, ""), false)); // Backspace, Delete
    assert!(!t.is_tracking());
    assert_eq!(t.capital(), None);
    assert_eq!(t.armed_at(), Some(5));

    t.produced(5, "M");
    t.edited(&te(2, 6, ""), false); // a wider deletion
    assert_eq!(t.armed_at(), Some(2));

    t.produced(5, "M");
    assert!(!t.edited(&te(5, 1, "1"), false)); // a non-word over it: armed, not the retype
    assert_eq!(t.armed_at(), Some(5));

    t.produced(3, "M");
    assert!(t.edited(&te(1, 3, "ab"), true)); // a word from before p that swallows it: at q, as typed
    assert_eq!(t.armed_at(), Some(1));
}

#[test]
fn the_undo_of_a_capital_is_the_replacement_that_removes_it() {
    let mut t = CapitalTracker::new();
    t.produced(0, "M");
    assert!(t.edited(&te(0, 1, "m"), true)); // M → m at p: removed, armed at p, and as typed
    assert!(!t.is_tracking());
    assert_eq!(t.armed_at(), Some(0));
    assert!(!t.edited(&te(0, 1, "M"), false)); // the Redo: a non-word insertion at q, stays armed
    assert_eq!(t.armed_at(), Some(0));
    assert!(!t.is_tracking());
}

#[test]
fn a_replacement_that_puts_the_capital_back_at_the_same_offset_keeps_it_tracked() {
    let mut t = CapitalTracker::new();
    t.produced(0, "M");
    t.edited(&te(0, 1, "Md"), false);
    assert_eq!(t.capital_at(), Some(0));
    assert!(!t.is_armed());

    t.produced(2, "M");
    t.edited(&te(0, 3, "xyM"), false);
    assert_eq!(t.capital_at(), Some(2));
    t.edited(&te(0, 3, "xyMz"), false);
    assert_eq!(t.capital_at(), Some(2));

    t.edited(&te(0, 3, "xM"), false); // the M is at 1 now, not 2: removed
    assert!(!t.is_tracking());
    assert_eq!(t.armed_at(), Some(0));
}

#[test]
fn a_two_unit_capital_is_tracked_whole() {
    let mut t = CapitalTracker::new();
    t.produced(2, "\u{10400}");
    t.edited(&te(4, 0, "x"), true); // after both units
    assert_eq!(t.capital_at(), Some(2));
    t.edited(&te(2, 2, ""), false); // the pair deleted as one
    assert_eq!(t.armed_at(), Some(2));

    t.produced(2, "\u{10400}");
    t.edited(&te(0, 4, "ab\u{10400}"), false); // put back whole at 2
    assert_eq!(t.capital_at(), Some(2));

    // An edit inside the pair (no keyboard makes one) destroys the scalar: removed, and q is the
    // edit's start, as the rule says. md.win inserts a lone low surrogate there, which a Rust string
    // cannot hold; any other text over the low half is the same edit to the tracker.
    t.edited(&te(3, 1, "x"), false);
    assert!(!t.is_tracking());
    assert_eq!(t.armed_at(), Some(3));
}

#[test]
fn while_armed_a_word_insertion_at_q_is_as_typed_once_and_spends_it() {
    let mut t = armed(0);
    assert!(t.edited(&te(0, 0, "m"), true));
    assert!(!t.is_armed());
    assert!(!t.edited(&te(0, 0, "m"), true)); // spent: judged by the pure function again
    assert!(armed(4).edited(&te(4, 3, "md"), true)); // over a selection that starts at q too
}

#[test]
fn while_armed_an_insertion_elsewhere_clears() {
    let mut t = armed(3);
    assert!(!t.edited(&te(0, 0, "x"), true));
    assert!(!t.is_armed());
    let mut t = armed(3);
    t.edited(&te(5, 0, " "), false);
    assert!(!t.is_armed());
    let mut t = armed(3);
    t.edited(&te(0, 2, "ab"), true); // a replacement that starts elsewhere, though it ends at q
    assert!(!t.is_armed());
    let mut t = armed(3);
    t.edited(&te(1, 4, "ab"), true); // one that covers q from before it
    assert!(!t.is_armed());
}

#[test]
fn while_armed_a_non_word_insertion_at_q_stays_armed() {
    let mut t = armed(3);
    assert!(!t.edited(&te(3, 0, "1"), false));
    assert!(!t.edited(&te(3, 0, "\n"), false)); // LINUX: the buffer's line break
    assert!(!t.edited(&te(3, 0, "M"), false));
    assert!(!t.edited(&te(3, 2, "paste"), false));
    assert_eq!(t.armed_at(), Some(3));
}

#[test]
fn while_armed_deletions_shift_or_collapse_q_and_never_clear() {
    let mut t = armed(5);
    t.edited(&te(0, 2, ""), false); // before q: shifted
    assert_eq!(t.armed_at(), Some(3));
    t.edited(&te(1, 1, ""), false);
    assert_eq!(t.armed_at(), Some(2));
    t.edited(&te(2, 3, ""), false); // starting at q: q stays
    assert_eq!(t.armed_at(), Some(2));
    t.edited(&te(3, 1, ""), false); // after q: nothing
    assert_eq!(t.armed_at(), Some(2));
    t.edited(&te(0, 4, ""), false); // covering q from before: q moves to its start
    assert_eq!(t.armed_at(), Some(0));
    assert!(t.is_armed());
}

#[test]
fn the_removing_edit_that_is_itself_a_word_insertion_at_q_goes_in_as_typed_and_stays_armed() {
    // Select "Md", type "md" (consequence 4); select "M", type "m" (the retype rule): both remove
    // the capital, both go in as typed, and the override they armed stands until a later word
    // insertion at q spends it or an insertion elsewhere clears it.
    let mut t = CapitalTracker::new();
    t.produced(0, "M");
    assert!(t.edited(&te(0, 2, "md"), true));
    assert_eq!(t.armed_at(), Some(0));
    assert!(!t.edited(&te(2, 0, "x"), true));
    assert!(!t.is_armed());

    t.produced(0, "M");
    assert!(t.edited(&te(0, 1, "m"), true));
    assert_eq!(t.armed_at(), Some(0));
    t.edited(&te(0, 1, ""), false);
    assert_eq!(t.armed_at(), Some(0));
    assert!(t.edited(&te(0, 0, "m"), true));
    assert!(!t.is_armed());
}

#[test]
fn clear_forgets_everything() {
    let mut t = CapitalTracker::new();
    t.produced(3, "M");
    t.clear();
    assert!(!t.is_tracking());
    assert_eq!(t.capital(), None);
    let mut a = armed(3);
    a.clear();
    assert!(!a.is_armed());
    assert!(!a.edited(&te(3, 0, "m"), true));
}

#[test]
fn a_no_op_edit_changes_nothing() {
    let mut t = CapitalTracker::new();
    t.produced(3, "M");
    assert!(!t.edited(&te(1, 0, ""), false));
    assert_eq!(t.capital_at(), Some(3));
    let mut a = armed(2);
    assert!(!a.edited(&te(2, 0, ""), true));
    assert_eq!(a.armed_at(), Some(2));
}

/// md.win rejects a negative position, an empty capital and a null one. The position is unsigned
/// and a `&str` cannot be null here; the empty capital still panics.
#[test]
#[should_panic(expected = "empty capital")]
fn produced_rejects_nonsense() {
    CapitalTracker::new().produced(0, "");
}

// ── the diff every edit is reduced to ──────────────────────────────────────────────────────

#[test]
fn between_finds_the_one_replacement() {
    let rows: [(&str, isize, isize, &str, usize, usize, &str); 21] = [
        ("", 0, 0, "m", 0, 0, "m"),      // the first letter
        ("M", 1, 0, "Md", 1, 0, "d"),    // typing on
        ("Md", 0, 2, "m", 0, 2, "m"),    // a letter over a selection
        ("Md", 0, 0, "aMd", 0, 0, "a"),  // a letter in front
        ("Md", 2, 0, "M", 1, 1, ""),     // Backspace
        ("M", 1, 0, "", 0, 1, ""),       // Backspace on the last unit
        ("Md", 0, 0, "d", 0, 1, ""),     // Delete
        ("Xxx. M", 0, 5, "M", 0, 5, ""), // Cut
        ("M", 1, 0, "m", 0, 1, "m"),     // Undo of a capital
        ("m", 1, 0, "M", 0, 1, "M"),     // Redo of one
        ("aa", 1, 0, "aaa", 1, 0, "a"),  // inside a run the selection says where
        ("aa", 2, 0, "aaa", 2, 0, "a"),
        ("aaa", 1, 0, "aa", 0, 1, ""), // a shrink inside a run, no key announced: the unit before the cursor
        ("aaa", 0, 0, "aa", 0, 1, ""), // at the start only Delete fits
        ("aaa", 3, 0, "a", 1, 2, ""),  // Ctrl+Backspace: the two before the cursor
        ("abc", 1, 0, "ac", 1, 1, ""), // only the unit after the cursor fits: Delete, whatever was announced
        ("ab", 5, 0, "axb", 1, 0, "x"), // a selection the text cannot hold: the smallest replacement
        ("ab", -1, 0, "axb", 1, 0, "x"),
        ("hello world", 0, 11, "bye", 0, 11, "bye"), // a paste over everything
        ("abc", 1, 1, "aXYc", 1, 1, "XY"),
        ("- x\n", 4, 0, "- x\nh", 4, 0, "h"), // LINUX: after an LF line end, as the buffer spells it
    ];
    for (old, start, length, new, edit_start, edit_length, inserted) in rows {
        assert_eq!(
            TextEdit::between(old, start, length, new, DeleteDirection::Unknown),
            Some(te(edit_start, edit_length, inserted)),
            "{old:?} {start}+{length} → {new:?}"
        );
    }
}

#[test]
fn between_anchors_a_caret_deletion_on_the_side_the_key_announced() {
    use DeleteDirection::{Backward, Forward, Unknown};
    let rows: [(&str, isize, &str, DeleteDirection, usize); 9] = [
        ("II", 1, "I", Backward, 0),                           // md's I, before the cursor
        ("II", 1, "I", Forward, 1),                            // the writer's, after it
        ("II", 1, "I", Unknown, 0),                            // nobody announced: Backspace assumed
        ("II", 0, "I", Backward, 0), // announced Backspace at the start: only Delete fits, and the text is what happened
        ("II", 2, "I", Forward, 1),  // announced Delete at the end: only Backspace fits
        ("aaa", 2, "a", Backward, 0), // Ctrl+Backspace inside a run
        ("aaa", 1, "a", Forward, 1), // Ctrl+Delete inside a run
        ("a\u{10400}\u{10400}", 3, "a\u{10400}", Backward, 1), // a pair as one, before the cursor
        ("a\u{10400}\u{10400}", 1, "a\u{10400}", Forward, 1), // and after it
    ];
    for (old, caret, new, direction, edit_start) in rows {
        let removed = utf16_len(old) - utf16_len(new);
        assert_eq!(
            TextEdit::between(old, caret, 0, new, direction),
            Some(te(edit_start, removed, "")),
            "{old:?} at {caret} {direction:?}"
        );
    }
}

#[test]
fn the_direction_never_changes_an_edit_that_is_not_a_caret_shrink() {
    // A lingering flag — Backspace over a selection, a key repeat that met a selection, a history
    // step in the same turn — anchors at the selection or falls to the smallest diff as before;
    // only a collapsed cursor's shrink reads it.
    assert_eq!(TextEdit::between("Xxx. M", 0, 5, "M", DeleteDirection::Backward), Some(te(0, 5, "")));
    assert_eq!(TextEdit::between("a", 1, 0, "ab", DeleteDirection::Backward), Some(te(1, 0, "b")));
    assert_eq!(TextEdit::between("M", 1, 0, "m", DeleteDirection::Forward), Some(te(0, 1, "m")));
    // A cursor the text cannot hold.
    assert_eq!(TextEdit::between("abc", 5, 0, "ac", DeleteDirection::Backward), Some(te(1, 1, "")));
}

/// LINUX: md.win's VK-based theory over GDK keyval names (and the keypad's Delete, which
/// GtkTextView binds like Delete).
#[test]
fn only_backspace_and_delete_announce_a_side() {
    let rows: [(&str, DeleteDirection); 6] = [
        ("BackSpace", DeleteDirection::Backward),
        ("Delete", DeleteDirection::Forward),
        ("KP_Delete", DeleteDirection::Forward), // LINUX
        ("z", DeleteDirection::Unknown),
        ("Return", DeleteDirection::Unknown),
        ("x", DeleteDirection::Unknown), // Ctrl+X is a selection, which the text settles
    ];
    for (key, expected) in rows {
        assert_eq!(TypingHooks::deletion_key(key), expected, "{key}");
    }
}

#[test]
fn the_deletion_flag_settles_a_turn_later_and_is_not_consumed() {
    // A Backspace at the start of the text reports no change: the next keystroke is judged as
    // usual, and a Delete key with nothing after the cursor likewise.
    let e = Editor::new();
    e.backspace(); // nothing to delete
    assert_eq!(e.hooks().deleting(), DeleteDirection::Unknown);
    e.type_text("m");
    assert_eq!(e.text(), "M");
    e.delete();
    assert_eq!(e.text(), "M");
    assert_eq!(e.hooks().deleting(), DeleteDirection::Unknown);

    let hooks = TypingHooks::new();
    hooks.announce_deletion(DeleteDirection::Forward);
    hooks.before_text_changing("II", 1, 0, "I");
    assert_eq!(hooks.deleting(), DeleteDirection::Forward); // still up for a second change in the same step
    hooks.settle_deletion();
    assert_eq!(hooks.deleting(), DeleteDirection::Unknown);
}

#[test]
fn between_is_null_for_identical_text() {
    assert_eq!(TextEdit::between("", 0, 0, "", DeleteDirection::Unknown), None);
    assert_eq!(TextEdit::between("Md is", 2, 0, "Md is", DeleteDirection::Unknown), None);
}

#[test]
fn between_never_splits_a_surrogate_pair() {
    use DeleteDirection::Unknown;
    // The two Deseret letters share a high surrogate: the smallest unit diff would start at 1.
    assert_eq!(TextEdit::between("\u{10400}", 2, 0, "\u{10428}", Unknown), Some(te(0, 2, "\u{10428}")));
    assert_eq!(TextEdit::between("\u{10428}x", 3, 0, "\u{10400}x", Unknown), Some(te(0, 2, "\u{10400}")));
    // Two pairs sharing a low surrogate: the suffix stops before it.
    assert_eq!(TextEdit::between("a\u{10400}", 3, 0, "a\u{1D400}", Unknown), Some(te(1, 2, "\u{1D400}")));
    // A pair deleted whole is two units, one edit.
    assert_eq!(TextEdit::between("\u{10400}", 2, 0, "", Unknown), Some(te(0, 2, "")));
}

#[test]
fn the_edit_knows_its_shape() {
    let insertion = te(3, 0, "ab");
    assert_eq!(insertion.end(), 3);
    assert_eq!(insertion.delta(), 2);
    assert!(insertion.is_insertion());
    assert!(!insertion.is_deletion());

    let deletion = te(3, 2, "");
    assert_eq!(deletion.end(), 5);
    assert_eq!(deletion.delta(), -2);
    assert!(!deletion.is_insertion());
    assert!(deletion.is_deletion());

    let replacement = te(3, 2, "xyz");
    assert_eq!(replacement.delta(), 1);
    assert!(replacement.is_insertion());
    assert!(!replacement.is_deletion());
}

#[test]
#[ignore = "md.win pins ArgumentNullException for a null text; a Rust &str cannot be null"]
fn between_checks_its_arguments() {}

#[test]
fn a_capital_still_applies_when_the_box_reports_the_caret_already_moved() {
    // A real keystroke may report its selection after the insertion; the text alone still shows
    // where the letter went.
    let hooks = TypingHooks::new();
    hooks.configure(TypingSettings::new(false, true));
    hooks.before_text_changing("One. ", 6, 0, "One. t");
    let plan = hooks.text_changing("One. t", 6, 0).expect("the capital is planned at the letter");
    assert_eq!(plan.position, 5);
    assert_eq!(plan.replacement, "T");
    assert!(hooks.trace()[0].starts_with("plan: T at 5"));
    assert!(hooks.trace()[1].starts_with("applied: T at 5"));

    hooks.clear_trace();
    hooks.before_text_changing("One. T", 6, 0, "One. Tw"); // mid-word: declined, and says so
    assert_eq!(hooks.text_changing("One. Tw", 7, 0), None);
    assert!(hooks.trace()[0].starts_with("declined \"w\" at 6"), "{:?}", hooks.trace());
    assert_eq!(hooks.trace()[0], "declined \"w\" at 6 (selection 6+0, length 6\u{2192}7)");
}

#[test]
fn undo_of_a_capital_then_a_new_line_leaves_the_next_sentence_capitalized() {
    // 2026-09-27, md.win's manual checklist: "m" → "M", Ctrl+Z → "m", Enter, then "hello world"
    // went in lowercase. The override armed by the undo belongs to the m alone.
    let e = Editor::new();
    e.type_text("m");
    e.undo();
    assert_eq!(e.text(), "m");
    e.enter();
    e.type_text("hello world");
    assert_eq!(e.text(), "m\nHello world");
}

#[test]
fn a_deferred_capital_still_applies_while_the_letter_is_where_the_plan_put_it() {
    let hooks = TypingHooks::new();
    hooks.before_text_changing("One. ", 5, 0, "One. t");
    let plan = hooks.text_changing("One. t", 6, 0).unwrap();
    assert!(hooks.still_applies(&plan, "One. t", 6, 0));
    assert_eq!(hooks.tracker().capital_at(), Some(5));
}

#[test]
fn a_deferred_capital_is_dropped_when_a_second_key_got_there_first() {
    // "One. t" then "w" before the turn: the t is still at 5, but the cursor is past the w, and the
    // capital's cursor placement would put it back between T and w.
    let hooks = TypingHooks::new();
    hooks.before_text_changing("One. ", 5, 0, "One. t");
    let plan = hooks.text_changing("One. t", 6, 0).unwrap();
    assert!(!hooks.still_applies(&plan, "One. tw", 7, 0));
    assert!(!hooks.tracker().is_tracking());
    assert!(!TypingHooks::new().still_applies(&plan, "One. t", 5, 1)); // a selection over it: not ours to replace either
}

#[test]
fn a_deferred_capital_whose_letter_moved_is_dropped_and_its_tracking_forgotten() {
    let hooks = TypingHooks::new();
    hooks.before_text_changing("One. ", 5, 0, "One. t");
    let plan = hooks.text_changing("One. t", 6, 0).unwrap();
    assert!(!hooks.still_applies(&plan, "One. ", 5, 0));
    assert!(!hooks.tracker().is_tracking());
    assert!(hooks.last_decision().starts_with("dropped: the text moved on"));
}

#[test]
fn the_undone_capital_left_selected_gets_the_caret_after_it() {
    let e = Editor::new();
    e.type_text("m");
    e.undo();
    assert_armed(&e, 0);
    assert_eq!(e.hooks().caret_after_history("m", 0, 1), Some(1));
    assert_eq!(e.hooks().caret_after_history("m", 1, 0), None); // already collapsed
    assert_eq!(e.hooks().caret_after_history("mo", 0, 2), None); // more than the letter: the selection stands
}

#[test]
fn an_ordinary_history_step_keeps_its_selection() {
    let e = Editor::new();
    e.type_text("hi");
    e.undo(); // takes back the i; the capital still stands
    assert_tracked(&e, 0, "H");
    assert_eq!(e.hooks().caret_after_history("H", 0, 1), None);
}

#[test]
fn the_caret_after_an_undone_non_bmp_capital_skips_both_units() {
    let e = Editor::new();
    e.type_text("\u{10428}"); // DESERET SMALL LETTER LONG I → 𐐀
    e.undo();
    assert_armed(&e, 0);
    assert_eq!(e.hooks().caret_after_history("\u{10428}", 0, 2), Some(2));
    assert_eq!(e.hooks().caret_after_history("\u{10428}", 0, 1), None);
}
