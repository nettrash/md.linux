//! Port of md.win `tests/Md.App.Logic.Tests/CommandAdversarialTests.cs`: the traps §2 leaves
//! behind — the snapshot equality the whole refresh path hangs on, the two "document" flags the
//! Book window and a document window read differently, the double fire at its *reported* gap, and
//! the Zen nudge that must never become the window's remembered mode.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use md_core::document::page_size::PageSize;
use md_core::document::view_mode::ViewMode;
use md_core::markdown::block::{NoteEntry, OutlineEntry};
use md_logic::commands::command_enablement::{has_active_document, is_checked, is_enabled};
use md_logic::commands::command_table;
use md_logic::commands::{
    CommandArgument, CommandDispatcher, CommandId, DiagramRef, RecentEntry, ShellSnapshot, WindowTitle,
};
use md_logic::seams::WindowId;
use md_logic::testing::*;

const NONE: &CommandArgument = &CommandArgument::None;

fn none() -> ShellSnapshot {
    ShellSnapshot::empty()
}

fn document() -> ShellSnapshot {
    ShellSnapshot { has_document: true, ..none() }
}

// ── The snapshot's equality is load-bearing ───────────────────────────────────────────────

/// md.win enumerates the record's constructor parameters by reflection, mutates each one away
/// from `Empty` and asserts the hand-written `Equals` notices — because the menu bar never redraws
/// for a value equality forgets. Here the equality is derived, so it cannot forget a field; what
/// is still pinned is that every field differs from `empty()` in the value below, and the
/// exhaustive destructuring makes a field added to the struct fail to compile until it has a row.
#[test]
fn every_field_the_snapshot_carries_is_part_of_its_equality() {
    let ShellSnapshot {
        has_document: _,
        is_book_window: _,
        is_editing_article: _,
        is_saved: _,
        is_dirty: _,
        has_book: _,
        displayed_mode: _,
        zen_active: _,
        zen_reading: _,
        outline: _,
        notes: _,
        diagrams: _,
        can_previous: _,
        can_next: _,
        editor_visible: _,
        can_undo: _,
        can_redo: _,
        recent_entries: _,
        window_titles: _,
        has_selection: _,
        has_find_query: _,
        pdf_page_size_id: _,
        sidebar_open: _,
        is_full_screen: _,
        find_bar_open: _,
        continue_lists: _,
        capitalize_sentences: _,
        wrap_selection: _,
    } = ShellSnapshot::empty();

    let base = ShellSnapshot::empty;
    let mutations: Vec<(&str, ShellSnapshot)> = vec![
        ("has_document", ShellSnapshot { has_document: true, ..base() }),
        ("is_book_window", ShellSnapshot { is_book_window: true, ..base() }),
        ("is_editing_article", ShellSnapshot { is_editing_article: true, ..base() }),
        ("is_saved", ShellSnapshot { is_saved: true, ..base() }),
        ("is_dirty", ShellSnapshot { is_dirty: true, ..base() }),
        ("has_book", ShellSnapshot { has_book: true, ..base() }),
        // empty()'s is ViewMode::WINDOW_DEFAULT == Split.
        ("displayed_mode", ShellSnapshot { displayed_mode: ViewMode::Preview, ..base() }),
        ("zen_active", ShellSnapshot { zen_active: true, ..base() }),
        ("zen_reading", ShellSnapshot { zen_reading: true, ..base() }),
        ("outline", ShellSnapshot { outline: vec![OutlineEntry::new(1, "Title", "title", 0)], ..base() }),
        ("notes", ShellSnapshot { notes: vec![NoteEntry::new("A note", 3)], ..base() }),
        ("diagrams", ShellSnapshot { diagrams: vec![DiagramRef::new(0, "Mermaid: flow")], ..base() }),
        ("can_previous", ShellSnapshot { can_previous: true, ..base() }),
        ("can_next", ShellSnapshot { can_next: true, ..base() }),
        ("editor_visible", ShellSnapshot { editor_visible: true, ..base() }),
        ("can_undo", ShellSnapshot { can_undo: true, ..base() }),
        ("can_redo", ShellSnapshot { can_redo: true, ..base() }),
        (
            "recent_entries",
            ShellSnapshot { recent_entries: vec![RecentEntry::new("tok", "notes.md", "/home/n/Documents")], ..base() },
        ),
        ("window_titles", ShellSnapshot { window_titles: vec![WindowTitle::new(WindowId(0), "Book", true)], ..base() }),
        ("has_selection", ShellSnapshot { has_selection: true, ..base() }),
        ("has_find_query", ShellSnapshot { has_find_query: true, ..base() }),
        // empty()'s is PageSize::DEFAULT_ID == "a4".
        ("pdf_page_size_id", ShellSnapshot { pdf_page_size_id: PageSize::US_LETTER.id.to_string(), ..base() }),
        ("sidebar_open", ShellSnapshot { sidebar_open: true, ..base() }),
        ("is_full_screen", ShellSnapshot { is_full_screen: true, ..base() }),
        ("find_bar_open", ShellSnapshot { find_bar_open: true, ..base() }),
        // empty()'s three settings are the defaults, true.
        ("continue_lists", ShellSnapshot { continue_lists: false, ..base() }),
        ("wrap_selection", ShellSnapshot { wrap_selection: false, ..base() }),
        ("capitalize_sentences", ShellSnapshot { capitalize_sentences: false, ..base() }),
    ];
    assert_eq!(mutations.len(), 28);

    let baseline = ShellSnapshot::empty();
    for (name, mutated) in mutations {
        assert!(baseline != mutated, "ShellSnapshot equality ignores {name}: the menu bar would never refresh for it");
        assert!(mutated != baseline, "ShellSnapshot equality ignores {name} in the other direction");
    }
}

// ── The two "document" flags (§2.1's doc legend) ──────────────────────────────────────────

/// §2.1: `doc` is "document windows always; the Book window only while session.Stage ==
/// Editing". Which flag answers depends on the window, so neither a Book window that also sets
/// `has_document` (it does host an article) nor a document window carrying a stale
/// `is_editing_article` may reach the File menu.
#[test]
fn the_book_windows_document_is_its_article_stage_and_not_its_has_document_flag() {
    let book_window_hosting_something = ShellSnapshot { is_book_window: true, has_document: true, ..none() };
    assert!(!has_active_document(&book_window_hosting_something));
    assert!(!is_enabled(CommandId::Save, &book_window_hosting_something));
    assert!(!is_enabled(CommandId::ExportPdf, &book_window_hosting_something));

    let book_window_editing = ShellSnapshot { is_book_window: true, is_editing_article: true, ..none() };
    assert!(has_active_document(&book_window_editing));
    assert!(is_enabled(CommandId::Save, &book_window_editing));

    // A document window never reads is_editing_article — that field is the Book window's alone.
    let document_window_with_a_stale_flag = ShellSnapshot { is_editing_article: true, ..none() };
    assert!(!has_active_document(&document_window_with_a_stale_flag));
    assert!(!is_enabled(CommandId::Save, &document_window_with_a_stale_flag));
    assert!(has_active_document(&document()));
}

/// §2.1: `zen` is "document windows only", and §2.5 keeps the row present but disabled in the
/// Book window. The Book window publishing a document is exactly the case that separates "not a
/// document window" from "no document".
#[test]
fn zen_is_refused_by_the_book_window_even_while_it_is_editing_an_article() {
    let book_window_editing =
        ShellSnapshot { is_book_window: true, has_document: true, is_editing_article: true, ..none() };
    assert!(!is_enabled(CommandId::ZenMode, &book_window_editing));
    assert!(command_table::spec(CommandId::ZenMode).path.is_some()); // present in the bar all the same
    assert!(is_enabled(CommandId::ZenMode, &document()));
}

// ── The double fire (§2.9) ────────────────────────────────────────────────────────────────

fn dispatcher_over(scheduler: &FakeScheduler, snapshot: &Rc<RefCell<ShellSnapshot>>) -> CommandDispatcher {
    let snapshot = snapshot.clone();
    CommandDispatcher::new(scheduler.clock().clone(), move || snapshot.borrow().clone())
}

/// md.win's day-1 Windows check as a unit test: Ctrl+1 with the preview focused must fire once,
/// with microsoft-ui-xaml #6231's second activation ~100 ms after the first. LINUX: WebKitGTK was
/// measured firing once (linux-facts §1); the guard is pinned all the same, at the same gap.
#[test]
fn ctrl1_fires_once_when_the_web_view_forwards_it_at_the_gap_the_bug_reports() {
    let scheduler = FakeScheduler::new();
    let snapshot = Rc::new(RefCell::new(document()));
    let dispatcher = dispatcher_over(&scheduler, &snapshot);
    let runs = Rc::new(std::cell::Cell::new(0));
    {
        let runs = runs.clone();
        dispatcher.register(CommandId::ViewEdit, move || runs.set(runs.get() + 1));
    }

    assert!(dispatcher.try_invoke(CommandId::ViewEdit, NONE));
    scheduler.clock().advance(Duration::from_millis(100));
    // Swallowed, but still reported handled so nothing downstream acts on the duplicate either.
    assert!(dispatcher.try_invoke(CommandId::ViewEdit, NONE));
    assert_eq!(runs.get(), 1);

    scheduler.clock().advance(Duration::from_millis(400));
    assert!(dispatcher.try_invoke(CommandId::ViewEdit, NONE));
    assert_eq!(runs.get(), 2);
}

/// The guard is keyed on the command alone, never on the argument, which is what §2.9 asks for —
/// the forwarded duplicate carries the same argument (accelerators carry none), and no dynamic row
/// can be clicked twice inside 150 ms because the menu has to be reopened between clicks. Pinned so
/// that widening the key later is a decision and not a slip.
#[test]
fn the_guard_is_keyed_on_the_command_and_not_on_the_row_the_argument_names() {
    let scheduler = FakeScheduler::new();
    let snapshot = Rc::new(RefCell::new(ShellSnapshot {
        recent_entries: vec![RecentEntry::new("a", "a.md", "f"), RecentEntry::new("b", "b.md", "f")],
        ..document()
    }));
    let dispatcher = dispatcher_over(&scheduler, &snapshot);
    let opened = Rc::new(RefCell::new(Vec::new()));
    {
        let opened = opened.clone();
        dispatcher.register_with_argument(CommandId::OpenRecentEntry, move |a| opened.borrow_mut().push(a.clone()));
    }
    let a = CommandArgument::Text("a".into());
    let b = CommandArgument::Text("b".into());

    assert!(dispatcher.try_invoke(CommandId::OpenRecentEntry, &a));
    assert!(dispatcher.try_invoke(CommandId::OpenRecentEntry, &b));
    assert_eq!(*opened.borrow(), vec![a.clone()]);

    scheduler.clock().advance(CommandDispatcher::DOUBLE_FIRE_WINDOW);
    assert!(dispatcher.try_invoke(CommandId::OpenRecentEntry, &b));
    assert_eq!(*opened.borrow(), vec![a, b]);
}

// ── Zen's mode nudge is not the window's mode (§2.5) ──────────────────────────────────────

/// In Zen, Ctrl+3 means "read" and Ctrl+1/Ctrl+2 mean "write"; the window's own `displayed_mode`
/// is untouched, so leaving Zen returns to Split rather than to whatever the last Zen keystroke
/// looked like. Split is never ticked while Zen is on (Mac §2.2).
#[test]
fn a_zen_nudge_is_never_remembered_as_the_windows_mode() {
    let split = ShellSnapshot { displayed_mode: ViewMode::Split, ..document() };
    let zen_reading = ShellSnapshot { zen_active: true, zen_reading: true, ..split };

    assert_eq!(zen_reading.published_mode(), ViewMode::Preview);
    assert_eq!(zen_reading.displayed_mode, ViewMode::Split);
    assert!(!is_checked(CommandId::ViewSplit, &zen_reading));
    assert!(is_checked(CommandId::ViewPreview, &zen_reading));

    let zen_writing = ShellSnapshot { zen_reading: false, ..zen_reading };
    assert_eq!(zen_writing.published_mode(), ViewMode::Edit);
    assert!(!is_checked(CommandId::ViewSplit, &zen_writing));

    // Leaving Zen: the window is back in the mode it never left.
    let left = ShellSnapshot { zen_active: false, ..zen_writing };
    assert_eq!(left.published_mode(), ViewMode::Split);
    assert!(is_checked(CommandId::ViewSplit, &left));
}

// ── Esc, and the chords that must fall through ────────────────────────────────────────────

/// §2.9: Esc is a window accelerator, so it must report "not handled" whenever the editor should
/// keep it. An open find bar claims Esc even with an empty query — closing an empty find bar is
/// exactly what Esc is for there — which `has_find_query` would not have said.
#[test]
fn esc_falls_through_to_the_editor_until_zen_or_an_empty_find_bar_claims_it() {
    let scheduler = FakeScheduler::new();
    let snapshot = Rc::new(RefCell::new(ShellSnapshot { editor_visible: true, ..document() }));
    let dispatcher = dispatcher_over(&scheduler, &snapshot);
    let runs = Rc::new(std::cell::Cell::new(0));
    {
        let runs = runs.clone();
        dispatcher.register(CommandId::Escape, move || runs.set(runs.get() + 1));
    }

    assert!(!dispatcher.try_invoke(CommandId::Escape, NONE));
    assert_eq!(runs.get(), 0);

    let with_bar = ShellSnapshot { find_bar_open: true, has_find_query: false, ..snapshot.borrow().clone() };
    *snapshot.borrow_mut() = with_bar;
    assert!(dispatcher.try_invoke(CommandId::Escape, NONE));
    assert_eq!(runs.get(), 1);

    scheduler.clock().advance(Duration::from_secs(1));
    *snapshot.borrow_mut() = ShellSnapshot { editor_visible: true, zen_active: true, ..document() };
    assert!(dispatcher.try_invoke(CommandId::Escape, NONE));
    assert_eq!(runs.get(), 2);
}

/// The shell registers every root accelerator on the window. Against a window with nothing open,
/// every one of them that §2 disables must answer false so the key reaches the focused widget, and
/// every one §2 leaves enabled must run — one assert per chord, so a table row that silently swaps
/// sides is visible.
#[test]
fn every_root_accelerator_answers_handled_exactly_when_section_2_enables_it() {
    let scheduler = FakeScheduler::new();
    let snapshot = Rc::new(RefCell::new(ShellSnapshot::empty()));
    let dispatcher = dispatcher_over(&scheduler, &snapshot);
    let ran = Rc::new(RefCell::new(Vec::new()));
    for spec in command_table::root_accelerators() {
        let (id, ran) = (spec.id, ran.clone());
        dispatcher.register(id, move || ran.borrow_mut().push(id));
    }

    for spec in command_table::root_accelerators() {
        let expected = is_enabled(spec.id, &snapshot.borrow());
        assert_eq!(dispatcher.try_invoke(spec.id, NONE), expected, "{:?}", spec.id);
    }

    // With nothing open these are the chords that must NOT be swallowed by the window.
    let ran = ran.borrow();
    for id in
        [CommandId::Save, CommandId::Print, CommandId::Find, CommandId::Replace, CommandId::Escape, CommandId::ZenMode]
    {
        assert!(!ran.contains(&id), "{id:?}");
    }
    for id in [CommandId::New, CommandId::Close, CommandId::FullScreen, CommandId::ShowBook, CommandId::Help] {
        assert!(ran.contains(&id), "{id:?}");
    }
}
