//! Port of md.win `tests/Md.App.Logic.Tests/ViewModeControllerTests.cs`: shell-design §5.2 and
//! §5.6 — the consequences the Mac's `setMode` / `applyViewModeMemory` produce, each named after
//! the sentence in the design it pins. (The rules themselves — the open truth table, the codec, the
//! MRU, the nudge — are md-core's `view_mode` tests; these are the window's use of them.)
//!
//! md.win's `BookArticleOpens` is a process-wide static, so its suite runs in a serial
//! collection; md-core's is a value, and each test owns one.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use md_core::document::book_article_opens::{BookArticleOpens, MARK_LIFETIME};
use md_core::document::view_mode::ViewMode;
use md_core::document::view_mode_memory::{self, ViewModeStore};
use md_core::markdown::block::{NoteEntry, OutlineEntry};
use md_logic::preview::JumpId;
use md_logic::settings::settings_keys;
use md_logic::testing::*;
use md_logic::view::{DocumentWindowState, SettingsViewModeStore, ViewModeController, ZenController};

// Paths that do not exist: md-core's canonical_path resolves links only for a path that names
// something on disk, so these canonicalise lexically and nothing else.
const PATH1: &str = "/nonexistent/md.linux.tests/one.md";
const PATH2: &str = "/nonexistent/md.linux.tests/two.md";

struct Fixture {
    settings: Rc<FakeSettingsStore>,
    state: Rc<DocumentWindowState>,
    opens: Rc<RefCell<BookArticleOpens>>,
    controller: ViewModeController,
}

impl Fixture {
    fn new() -> Self {
        Self::with_opens(BookArticleOpens::new())
    }

    fn with_opens(opens: BookArticleOpens) -> Self {
        let settings = Rc::new(FakeSettingsStore::new());
        let state = Rc::new(DocumentWindowState::new());
        let opens = Rc::new(RefCell::new(opens));
        let controller = ViewModeController::new(state.clone(), store(&settings), opens.clone());
        Fixture { settings, state, opens, controller }
    }

    fn remembered(&self, path: &str) -> String {
        view_mode_memory::lookup(&view_mode_memory::identity_for(path), store(&self.settings).as_ref())
            .map(|m| m.raw_value().to_string())
            .unwrap_or_else(|| "-".to_string())
    }

    fn seed(&self, path: &str, mode: ViewMode) {
        view_mode_memory::remember(mode, &view_mode_memory::identity_for(path), store(&self.settings).as_ref());
    }

    /// md.win's `_settings.Writes.Clear()`: the writes made from here on.
    fn mark(&self) -> usize {
        self.settings.writes().len()
    }

    fn writes_since(&self, mark: usize) -> usize {
        self.settings.writes().len() - mark
    }
}

fn store(settings: &Rc<FakeSettingsStore>) -> Rc<dyn ViewModeStore> {
    Rc::new(SettingsViewModeStore::new(settings.clone()))
}

// ── 1. First appearance decides and stores ────────────────────────────────────────────────

#[test]
fn first_appearance_of_a_file_with_nothing_remembered_decides_and_stores_the_result() {
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something");

    assert_eq!(f.state.stored_mode(), ViewMode::Split);
    assert_eq!(f.remembered(PATH1), "split");
}

#[test]
fn an_empty_file_opens_in_edit_and_that_is_what_gets_stored() {
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "");

    assert_eq!(f.state.stored_mode(), ViewMode::Edit);
    assert_eq!(f.remembered(PATH1), "edit");
}

#[test]
fn a_remembered_mode_wins_over_the_open_rule() {
    let f = Fixture::new();
    f.seed(PATH1, ViewMode::Preview);

    f.controller.apply_memory(Some(PATH1), "# Something");

    assert_eq!(f.state.stored_mode(), ViewMode::Preview);
}

// ── 2. Documents with no file ─────────────────────────────────────────────────────────────

#[test]
fn an_example_has_content_and_no_file_so_it_opens_in_split_and_stores_nothing() {
    let f = Fixture::new();
    f.controller.apply_memory(None, "# Welcome");

    assert_eq!(f.state.stored_mode(), ViewMode::Split);
    assert!(f.settings.writes().is_empty());
}

#[test]
fn an_empty_untitled_document_opens_in_edit_and_stores_nothing() {
    let f = Fixture::new();
    f.controller.apply_memory(None, "");

    assert_eq!(f.state.stored_mode(), ViewMode::Edit);
    assert!(f.settings.writes().is_empty());
}

#[test]
fn picking_a_mode_while_untitled_stores_nothing() {
    let f = Fixture::new();
    f.controller.apply_memory(None, "");

    f.controller.select(ViewMode::Preview);

    assert_eq!(f.state.stored_mode(), ViewMode::Preview);
    assert!(f.settings.writes().is_empty());
}

// ── 3. The first save migrates; it does not re-decide ─────────────────────────────────────

#[test]
fn the_first_save_of_an_untitled_document_migrates_the_raw_preference_instead_of_re_deciding() {
    let f = Fixture::new();
    f.controller.apply_memory(None, "");
    f.controller.select(ViewMode::Preview); // the writer is reading; the open rule would say Split

    f.controller.apply_memory(Some(PATH1), "# Something");

    assert_eq!(f.state.stored_mode(), ViewMode::Preview);
    assert_eq!(f.remembered(PATH1), "preview");
}

#[test]
fn the_migrate_branch_leaves_a_standing_nudge_alone() {
    let f = Fixture::new();
    f.controller.apply_memory(None, "# Something");
    f.controller.select(ViewMode::Preview);
    f.controller.jump_to_note(&NoteEntry::new("a note", 3));
    assert_eq!(f.state.navigation_mode(), Some(ViewMode::Edit));

    f.controller.apply_memory(Some(PATH1), "# Something");

    assert_eq!(f.state.navigation_mode(), Some(ViewMode::Edit));
    assert_eq!(f.state.stored_mode(), ViewMode::Preview);
}

// ── 4. Save As / Rename / Move To re-decide for the new identity ──────────────────────────

#[test]
fn save_as_of_an_already_saved_document_re_runs_the_open_rule_for_the_new_identity() {
    let f = Fixture::new();
    f.seed(PATH2, ViewMode::Preview);
    f.controller.apply_memory(Some(PATH1), "# Something");
    f.controller.select(ViewMode::Edit);

    f.controller.apply_memory(Some(PATH2), "# Something");

    assert_eq!(f.state.stored_mode(), ViewMode::Preview);
    assert_eq!(f.remembered(PATH1), "edit"); // the old file keeps what it was left in
}

#[test]
fn save_as_clears_a_standing_nudge() {
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something");
    f.controller.select(ViewMode::Preview);
    f.controller.jump_to_note(&NoteEntry::new("a note", 1));
    assert_eq!(f.state.navigation_mode(), Some(ViewMode::Edit));

    f.controller.apply_memory(Some(PATH2), "# Something");

    assert_eq!(f.state.navigation_mode(), None);
}

// ── 5. Saving back over the same path is a no-op ──────────────────────────────────────────

#[test]
fn saving_over_the_same_path_neither_re_decides_nor_writes() {
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something");
    f.controller.select(ViewMode::Preview);
    f.seed(PATH1, ViewMode::Edit); // someone else's window remembered Edit
    let mark = f.mark();

    f.controller.apply_memory(Some(PATH1), "# Something");

    assert_eq!(f.state.stored_mode(), ViewMode::Preview);
    assert_eq!(f.writes_since(mark), 0);
}

/// md.win: on Windows two spellings that differ in case are one file (NTFS), and md-core folds
/// case so both reach one entry; the test returns early elsewhere. LINUX: names are compared byte
/// for byte, so the two spellings of one path that matter are the lexical ones — `.` and `..` —
/// which md-core's canonicalisation collapses. Same assertions, Linux's spellings.
#[test]
fn two_spellings_of_one_path_are_one_identity() {
    let f = Fixture::new();
    let plain = "/nonexistent/md.linux.tests/case.md";
    let dotted = "/nonexistent/./md.linux.tests/sub/../case.md";
    assert_eq!(view_mode_memory::identity_for(plain), view_mode_memory::identity_for(dotted));

    f.controller.apply_memory(Some(plain), "# Something");
    f.controller.select(ViewMode::Preview);
    let mark = f.mark();

    f.controller.apply_memory(Some(dotted), "# Something");

    assert_eq!(f.state.stored_mode(), ViewMode::Preview);
    assert_eq!(f.writes_since(mark), 0);
}

// ── 6. The book exemption is claimed once and is sticky ───────────────────────────────────

#[test]
fn an_article_the_book_opened_is_exempt_from_the_memory_both_ways() {
    let f = Fixture::new();
    f.seed(PATH1, ViewMode::Preview);
    f.opens.borrow_mut().mark(PATH1);

    f.controller.apply_memory(Some(PATH1), "# Something");

    assert!(f.state.is_book_article());
    assert_eq!(f.state.stored_mode(), ViewMode::Split); // the window default, not the remembered Preview
    let mark = f.mark();

    f.controller.select(ViewMode::Edit);

    assert_eq!(f.state.stored_mode(), ViewMode::Edit);
    assert_eq!(f.writes_since(mark), 0);
    assert_eq!(f.remembered(PATH1), "preview"); // untouched
}

#[test]
fn the_book_exemption_is_sticky_through_save_as() {
    let f = Fixture::new();
    f.seed(PATH2, ViewMode::Preview);
    f.opens.borrow_mut().mark(PATH1);
    f.controller.apply_memory(Some(PATH1), "# Something");
    f.controller.select(ViewMode::Edit);
    let mark = f.mark();

    f.controller.apply_memory(Some(PATH2), "# Something");

    assert!(f.state.is_book_article());
    assert_eq!(f.state.stored_mode(), ViewMode::Edit); // no re-decision to PATH2's Preview
    assert_eq!(f.writes_since(mark), 0);
}

#[test]
fn the_mark_is_claimed_by_exactly_one_window() {
    let f = Fixture::new();
    f.opens.borrow_mut().mark(PATH1);
    f.controller.apply_memory(Some(PATH1), "# Something");
    assert!(f.state.is_book_article());

    let second = Rc::new(DocumentWindowState::new());
    ViewModeController::new(second.clone(), store(&f.settings), f.opens.clone())
        .apply_memory(Some(PATH1), "# Something");

    assert!(!second.is_book_article());
}

// ── 7. Nudges are transient; Zen stores nothing ───────────────────────────────────────────

#[test]
fn a_note_jump_nudges_preview_to_edit_and_stores_nothing() {
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something");
    f.controller.select(ViewMode::Preview);
    let mark = f.mark();

    f.controller.jump_to_note(&NoteEntry::new("a note", 7));

    assert_eq!(f.state.navigation_mode(), Some(ViewMode::Edit));
    assert_eq!(f.state.effective_mode(), ViewMode::Edit);
    assert_eq!(f.state.stored_mode(), ViewMode::Preview);
    assert_eq!(f.writes_since(mark), 0);
    assert_eq!(f.remembered(PATH1), "preview");
}

#[test]
fn a_nudge_is_cleared_by_the_pick_that_follows_it() {
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something");
    f.controller.select(ViewMode::Preview);
    f.controller.jump_to_note(&NoteEntry::new("a note", 7));

    f.controller.select(ViewMode::Split);

    assert_eq!(f.state.navigation_mode(), None);
    assert_eq!(f.state.effective_mode(), ViewMode::Split);
}

#[test]
fn a_second_note_jump_does_not_cancel_the_nudge_the_first_one_set() {
    // macOS §6.9: "The nudge is assigned only when non-nil — writing nil back would cancel a nudge
    // from the previous note jump." Once the first jump has nudged Preview → Edit the displayed
    // mode IS Edit, so the second jump's nudge is none; assigning that would throw the reader
    // straight back into Preview with a caret they cannot see.
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something");
    f.controller.select(ViewMode::Preview);
    f.controller.jump_to_note(&NoteEntry::new("first", 3));
    assert_eq!(f.state.navigation_mode(), Some(ViewMode::Edit));

    f.controller.jump_to_note(&NoteEntry::new("second", 9));

    assert_eq!(f.state.navigation_mode(), Some(ViewMode::Edit));
    assert_eq!(f.state.effective_mode(), ViewMode::Edit);
    assert_eq!(f.state.stored_mode(), ViewMode::Preview); // the preference is still untouched
    assert_eq!(f.state.editor_jump().unwrap().line, 9);
}

#[test]
fn a_stale_book_mark_does_not_exempt_the_next_ordinary_open() {
    // MARK_LIFETIME is 10 s precisely so that a mark nobody claimed — the navigator opened the
    // article, the user closed the window, the file is opened again an hour later — cannot silently
    // exempt that ordinary open from the memory.
    let start = Instant::now();
    let now = Arc::new(Mutex::new(start));
    let clock = now.clone();
    let f = Fixture::with_opens(BookArticleOpens::with_clock(move || *clock.lock().unwrap()));
    f.seed(PATH1, ViewMode::Preview);
    f.opens.borrow_mut().mark(PATH1);
    *now.lock().unwrap() = start + MARK_LIFETIME + Duration::from_secs(1);

    f.controller.apply_memory(Some(PATH1), "# Something");

    assert!(!f.state.is_book_article());
    assert_eq!(f.state.stored_mode(), ViewMode::Preview); // the remembered mode, not the default
}

// LINUX: Edit ▸ Find in a window that shows no editor — the Mac's `showEditor` (shell-design
// §2.5), the Notes jump's nudge without the jump.

#[test]
fn find_in_preview_nudges_the_editor_back_and_stores_nothing() {
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something");
    f.controller.select(ViewMode::Preview);
    let mark = f.mark();

    f.controller.show_editor();

    assert_eq!(f.state.navigation_mode(), Some(ViewMode::Edit));
    assert_eq!(f.state.effective_mode(), ViewMode::Edit);
    assert_eq!(f.state.stored_mode(), ViewMode::Preview);
    assert_eq!(f.writes_since(mark), 0);
    assert_eq!(f.remembered(PATH1), "preview");
    assert!(f.state.editor_jump().is_none()); // a nudge, not a jump

    // Choosing a mode for real ends it, exactly like the note jump's.
    f.controller.select(ViewMode::Preview);
    assert_eq!(f.state.navigation_mode(), None);
    assert_eq!(f.state.effective_mode(), ViewMode::Preview);
}

#[test]
fn find_with_the_editor_on_screen_changes_nothing() {
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something"); // Split
    f.controller.show_editor();
    assert_eq!(f.state.navigation_mode(), None);
    assert_eq!(f.state.effective_mode(), ViewMode::Split);

    f.controller.select(ViewMode::Edit);
    f.controller.show_editor();
    assert_eq!(f.state.navigation_mode(), None);
    assert_eq!(f.state.effective_mode(), ViewMode::Edit);
}

#[test]
fn find_in_zens_reading_half_turns_zen_to_writing_and_stores_nothing() {
    // The Mac: `if zenActive { zenReading = false }` — Zen's own switch, per-window state.
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something");
    f.state.set_zen_active(true);
    f.controller.select(ViewMode::Preview); // in Zen: read
    assert!(f.state.zen_reading());
    let mark = f.mark();

    f.controller.show_editor();

    assert!(!f.state.zen_reading());
    assert!(f.state.zen_active());
    assert_eq!(f.state.navigation_mode(), None);
    assert_eq!(f.state.stored_mode(), ViewMode::Split);
    assert_eq!(f.writes_since(mark), 0);
}

#[test]
fn a_note_jump_from_split_or_edit_does_not_nudge() {
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something"); // Split

    f.controller.jump_to_note(&NoteEntry::new("a note", 2));
    assert_eq!(f.state.navigation_mode(), None);

    f.controller.select(ViewMode::Edit);
    f.controller.jump_to_note(&NoteEntry::new("a note", 2));
    assert_eq!(f.state.navigation_mode(), None);
}

#[test]
fn a_heading_jump_never_nudges_and_moves_only_the_panes_that_are_showing() {
    let f = Fixture::new();
    let heading = OutlineEntry::new(1, "Title", "title", 4);

    f.controller.apply_memory(Some(PATH1), "# Title"); // Split: both panes
    f.controller.jump_to_heading(&heading);
    assert!(f.state.preview_navigation().is_some());
    assert!(f.state.editor_jump().is_some());
    assert_eq!(f.state.navigation_mode(), None);

    f.state.set_preview_navigation(None);
    f.state.set_editor_jump(None);
    f.controller.select(ViewMode::Preview);
    f.controller.jump_to_heading(&heading);
    assert!(f.state.preview_navigation().is_some());
    assert!(f.state.editor_jump().is_none()); // a reader is never dropped into the source

    f.state.set_preview_navigation(None);
    f.controller.select(ViewMode::Edit);
    f.controller.jump_to_heading(&heading);
    assert!(f.state.preview_navigation().is_none());
    assert!(f.state.editor_jump().is_some());
}

#[test]
fn a_one_shot_request_is_cleared_only_by_its_own_id() {
    let f = Fixture::new();
    let ids = RefCell::new(VecDeque::from([JumpId(1), JumpId(2)]));
    let controller = ViewModeController::with_ids(f.state.clone(), store(&f.settings), f.opens.clone(), move || {
        ids.borrow_mut().pop_front().unwrap()
    });
    f.state.set_stored_mode(ViewMode::Edit);

    controller.jump_to_note(&NoteEntry::new("first", 1));
    let first = f.state.editor_jump().unwrap().id;
    controller.jump_to_note(&NoteEntry::new("second", 9));

    f.state.editor_jump_handled(first); // the older pane reports back late
    assert_eq!(f.state.editor_jump().unwrap().line, 9);

    f.state.editor_jump_handled(f.state.editor_jump().unwrap().id);
    assert_eq!(f.state.editor_jump(), None);
}

#[test]
fn zen_swallows_the_view_mode_commands_and_stores_nothing() {
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something"); // Split, stored
    let zen = ZenController::new(f.state.clone(), None);
    zen.set_active(true);
    let mark = f.mark();

    f.controller.select(ViewMode::Preview);
    assert!(f.state.zen_reading());
    f.controller.select(ViewMode::Edit);
    assert!(!f.state.zen_reading());
    f.controller.select(ViewMode::Split);
    assert!(!f.state.zen_reading());

    assert_eq!(f.state.stored_mode(), ViewMode::Split);
    assert_eq!(f.writes_since(mark), 0);
    assert_eq!(f.remembered(PATH1), "split");
}

// ── 8. The traps the porting reports name ─────────────────────────────────────────────────

#[test]
fn a_view_mode_command_that_fires_twice_leaves_one_entry_and_one_mode() {
    // If the double-fire guard ever slips, the damage must stop at a duplicated write: not two
    // entries for one file, not a mode that lands somewhere else on the second pass.
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something");
    f.seed(PATH2, ViewMode::Preview); // a neighbour, so "one entry" is a real count

    f.controller.select(ViewMode::Edit);
    f.controller.select(ViewMode::Edit);

    assert_eq!(f.state.stored_mode(), ViewMode::Edit);
    assert_eq!(f.state.effective_mode(), ViewMode::Edit);
    assert_eq!(f.remembered(PATH1), "edit");
    assert_eq!(f.remembered(PATH2), "preview");
    let entries = view_mode_memory::entries(store(&f.settings).as_ref());
    assert_eq!(entries.len(), 2);
    let id1 = view_mode_memory::identity_for(PATH1);
    assert_eq!(entries.iter().filter(|e| e.identity == id1).count(), 1);
}

#[test]
fn a_doubled_zen_switch_is_also_idempotent_and_still_stores_nothing() {
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something");
    let zen = ZenController::new(f.state.clone(), None);
    zen.set_active(true);
    let mark = f.mark();

    f.controller.select(ViewMode::Preview);
    f.controller.select(ViewMode::Preview);

    assert!(f.state.zen_reading());
    assert_eq!(f.state.stored_mode(), ViewMode::Split);
    assert_eq!(f.writes_since(mark), 0);
}

#[test]
fn a_zen_round_trip_leaves_the_preference_the_memory_and_a_standing_nudge_exactly_as_they_were() {
    // The window wires ViewModeController and ZenController to one state, and nothing else does;
    // this is the only test that drives both. Zen is view state — it must not be able to reach
    // md.viewModeMemory, md.viewMode or a navigation nudge, on the way in or on the way out.
    let f = Fixture::new();
    f.controller.apply_memory(Some(PATH1), "# Something");
    f.controller.select(ViewMode::Preview);
    f.controller.jump_to_note(&NoteEntry::new("a note", 4)); // nudge Preview -> Edit
    let zen = ZenController::new(f.state.clone(), None);
    let mark = f.mark();

    zen.toggle();
    f.controller.select(ViewMode::Preview); // Ctrl+3 in Zen: read
    f.controller.select(ViewMode::Edit); // Ctrl+1 in Zen: write
    zen.presenter_changed(false); // the user hit F11

    assert!(!f.state.zen_active());
    assert!(!f.state.zen_reading());
    assert_eq!(f.state.stored_mode(), ViewMode::Preview);
    assert_eq!(f.state.navigation_mode(), Some(ViewMode::Edit)); // the nudge survived the round trip
    assert_eq!(f.state.effective_mode(), ViewMode::Edit);
    assert_eq!(f.writes_since(mark), 0);
    assert_eq!(f.remembered(PATH1), "preview");
}

/// md.win (Windows only, returns early elsewhere): a case-only rename keeps the identity because
/// md-core folds case on Windows. LINUX: ext4/btrfs/xfs compare names byte for byte, so a
/// case-only rename IS a different file (md-core's Linux rule, the Mac's too) — the open rule
/// re-runs for the new name and the old entry stays, exactly as for any rename.
#[test]
fn a_case_only_rename_keeps_the_remembered_mode_and_adds_no_second_entry() {
    let f = Fixture::new();
    let before = "/nonexistent/md.linux.tests/chapter one.md";
    let after = "/nonexistent/md.linux.tests/Chapter One.md";
    assert_ne!(view_mode_memory::identity_for(before), view_mode_memory::identity_for(after));

    f.controller.apply_memory(Some(before), "# Something");
    f.controller.select(ViewMode::Edit);

    f.controller.apply_memory(Some(after), "# Something"); // Rename ran; on Linux a new name

    assert_eq!(f.state.stored_mode(), ViewMode::Split); // the open rule, not the old Edit
    assert_eq!(f.remembered(before), "edit");
    assert_eq!(f.remembered(after), "split");
    assert_eq!(view_mode_memory::entries(store(&f.settings).as_ref()).len(), 2);
}

#[test]
fn a_rename_to_a_different_name_is_a_different_file_on_every_platform() {
    // The other half of the case-only test: a real rename changes the identity, so the open rule
    // re-runs for the new name (§5.2, "Save As / Rename / Move re-run the open rule").
    let f = Fixture::new();
    f.seed(PATH2, ViewMode::Preview);
    f.controller.apply_memory(Some(PATH1), "# Something");
    f.controller.select(ViewMode::Edit);

    f.controller.apply_memory(Some(PATH2), "# Something");

    assert_eq!(f.state.stored_mode(), ViewMode::Preview);
    assert_eq!(f.remembered(PATH1), "edit"); // the old name keeps what it had
}

#[test]
fn the_memory_store_writes_the_one_key_the_settings_inventory_names() {
    let f = Fixture::new();
    assert_eq!(settings_keys::VIEW_MODE_MEMORY, view_mode_memory::SETTINGS_KEY);

    f.controller.apply_memory(Some(PATH1), "# Something");

    let keys: std::collections::BTreeSet<String> = f.settings.writes().into_iter().map(|(k, _)| k).collect();
    assert_eq!(keys.into_iter().collect::<Vec<_>>(), [settings_keys::VIEW_MODE_MEMORY]);
    let stored = f.settings.get_string(settings_keys::VIEW_MODE_MEMORY).unwrap();
    assert!(stored.starts_with(view_mode_memory::HEADER));
}
