//! Port of md.win `tests/Md.App.Logic.Tests/DocumentWindowStateTests.cs`: shell-design §5.1 — the
//! per-window state itself: defaults, the three-valued identity memo, and what counts as a change.

use std::cell::Cell;
use std::rc::Rc;

use md_core::document::view_mode::ViewMode;
use md_core::export::diagram_svg::Diagram;
use md_core::markdown::block::{NoteEntry, OutlineEntry};
use md_logic::preview::{EditorJump, JumpId, PreviewNavigation};
use md_logic::view::{DerivedText, DocumentWindowState, IdentityMemo};

fn counting(state: &DocumentWindowState) -> Rc<Cell<usize>> {
    let changes = Rc::new(Cell::new(0));
    let c = changes.clone();
    state.changed().connect(move |_| c.set(c.get() + 1));
    changes
}

fn diagram() -> Diagram {
    Diagram {
        ordinal: 0,
        engine: "mermaid".into(),
        source: "graph".into(),
        menu_title: "Diagram 1".into(),
        layout: None,
    }
}

#[test]
fn a_fresh_window_is_split_with_nothing_else_set() {
    let state = DocumentWindowState::new();
    assert_eq!(state.stored_mode(), ViewMode::Split);
    assert_eq!(state.stored_mode(), ViewMode::WINDOW_DEFAULT);
    assert_eq!(state.effective_mode(), ViewMode::Split);
    assert!(!state.zen_active());
    assert!(!state.zen_reading());
    assert!(!state.is_book_article());
    assert_eq!(state.navigation_mode(), None);
    assert_eq!(state.preview_navigation(), None);
    assert_eq!(state.editor_jump(), None);
    assert_eq!(state.derived(), DerivedText::empty());
    assert!(!state.last_identity().ran());
}

#[test]
fn a_desktop_window_is_always_wide_so_nothing_is_ever_coerced() {
    let state = DocumentWindowState::new();
    const { assert!(DocumentWindowState::IS_WIDE) };

    state.set_stored_mode(ViewMode::Split);

    assert_eq!(state.effective_mode(), ViewMode::Split);
}

#[test]
fn the_nudge_is_what_the_window_shows_while_it_stands() {
    let state = DocumentWindowState::new();
    state.set_stored_mode(ViewMode::Preview);
    state.set_navigation_mode(Some(ViewMode::Edit));
    assert_eq!(state.effective_mode(), ViewMode::Edit);
    assert_eq!(state.stored_mode(), ViewMode::Preview);

    state.set_navigation_mode(None);
    assert_eq!(state.effective_mode(), ViewMode::Preview);
}

#[test]
fn changed_fires_once_for_a_real_change_and_not_at_all_for_a_rewrite() {
    let state = DocumentWindowState::new();
    let changes = counting(&state);

    state.set_stored_mode(ViewMode::Edit);
    state.set_stored_mode(ViewMode::Edit);

    assert_eq!(changes.get(), 1);
}

#[test]
fn the_derived_guard_is_structural_so_an_unchanged_recomputation_is_silent() {
    // Two runs of the scheduler over the same text build fresh lists; md.win's record compared
    // them by reference until DerivedText got its own Equals. The derived PartialEq here compares
    // the Vecs element by element, so the second, equal value is no change.
    let state = DocumentWindowState::new();
    let changes = counting(&state);
    let first =
        DerivedText::new(2, 7, vec![OutlineEntry::new(1, "T", "t", 0)], vec![NoteEntry::new("n", 3)], vec![diagram()]);
    let second =
        DerivedText::new(2, 7, vec![OutlineEntry::new(1, "T", "t", 0)], vec![NoteEntry::new("n", 3)], vec![diagram()]);
    assert_eq!(first, second);

    state.set_derived(first.clone());
    state.set_derived(second);

    assert_eq!(changes.get(), 1);
    // md.win asserts the first object is still the stored one (Assert.Same); a Rust value has no
    // identity beyond its contents, and the single change above is what proves it was kept.
    assert_eq!(state.derived(), first);
}

#[test]
fn a_derived_text_that_really_differs_is_still_announced() {
    let state = DocumentWindowState::new();
    let changes = counting(&state);
    let outline = OutlineEntry::new(1, "T", "t", 0);

    state.set_derived(DerivedText::new(2, 7, vec![outline.clone()], vec![], vec![]));
    state.set_derived(DerivedText::new(2, 8, vec![outline], vec![], vec![])); // one more character
    state.set_derived(DerivedText::new(2, 8, vec![], vec![], vec![])); // the heading went away
    state.set_derived(DerivedText::new(2, 8, vec![], vec![NoteEntry::new("n", 1)], vec![])); // a note appeared

    assert_eq!(changes.get(), 4);
}

#[test]
fn the_book_exemption_is_one_way_and_announces_itself_once() {
    let state = DocumentWindowState::new();
    let changes = counting(&state);

    state.mark_book_article();
    state.mark_book_article();

    assert!(state.is_book_article());
    assert_eq!(changes.get(), 1);
}

#[test]
fn the_identity_memo_has_three_states_and_the_middle_one_is_what_makes_the_first_save_a_migration() {
    assert!(!IdentityMemo::NEVER_RAN.ran());
    assert!(!IdentityMemo::NEVER_RAN.ran_with_no_identity());
    assert_eq!(IdentityMemo::default(), IdentityMemo::NEVER_RAN);

    let untitled = IdentityMemo::of(None);
    assert!(untitled.ran());
    assert!(untitled.ran_with_no_identity());
    assert_eq!(untitled.value(), None);

    let saved = IdentityMemo::of(Some("0123456789abcdef".into()));
    assert!(saved.ran());
    assert!(!saved.ran_with_no_identity());
    assert_eq!(saved.value(), Some("0123456789abcdef"));
}

#[test]
fn a_one_shot_request_survives_a_stale_handled_callback() {
    let state = DocumentWindowState::new();
    let first = EditorJump::new(JumpId::next(), 3);
    let second = EditorJump::new(JumpId::next(), 9);
    state.set_editor_jump(Some(first.clone()));
    state.set_editor_jump(Some(second.clone()));

    state.editor_jump_handled(first.id);
    assert_eq!(state.editor_jump(), Some(second.clone()));

    state.editor_jump_handled(second.id);
    assert_eq!(state.editor_jump(), None);

    let nav = PreviewNavigation::new(JumpId::next(), "slug");
    state.set_preview_navigation(Some(nav.clone()));
    state.preview_navigation_handled(JumpId::next());
    assert_eq!(state.preview_navigation(), Some(nav.clone()));
    state.preview_navigation_handled(nav.id);
    assert_eq!(state.preview_navigation(), None);
}
