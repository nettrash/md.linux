//! Port of md.win `tests/Md.App.Logic.Tests/CommandEnablementTests.cs`: every *Enabled* cell of
//! shell-design §2, one case each, plus the tick rules of §2.5 and the submenu rule of §2.2/§2.6.
//! The snapshots are built from `ShellSnapshot::empty()` with exactly the one flag each row is
//! about, so a case that passes for the wrong reason is visible.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use md_core::document::page_size::PageSize;
use md_core::document::view_mode::ViewMode;
use md_core::markdown::block::{NoteEntry, OutlineEntry};
use md_logic::commands::command_enablement::{
    has_active_document, is_checked, is_enabled, is_row_checked, is_submenu_enabled,
};
use md_logic::commands::command_table;
use md_logic::commands::{
    ActionScope, CommandArgument, CommandId, DiagramRef, MenuPath, RecentEntry, ShellSnapshot, WindowTitle,
};
use md_logic::seams::WindowId;

fn none() -> ShellSnapshot {
    ShellSnapshot::empty()
}

fn document() -> ShellSnapshot {
    ShellSnapshot { has_document: true, ..none() }
}

fn saved_document() -> ShellSnapshot {
    ShellSnapshot { is_saved: true, ..document() }
}

fn book_window_idle() -> ShellSnapshot {
    ShellSnapshot { is_book_window: true, ..none() }
}

fn book_window_editing() -> ShellSnapshot {
    ShellSnapshot { is_editing_article: true, ..book_window_idle() }
}

fn one_heading() -> Vec<OutlineEntry> {
    vec![OutlineEntry::new(1, "Title", "title", 0)]
}

fn one_note() -> Vec<NoteEntry> {
    vec![NoteEntry::new("A note", 3)]
}

fn one_diagram() -> Vec<DiagramRef> {
    vec![DiagramRef::new(0, "Mermaid: flow")]
}

fn one_recent() -> Vec<RecentEntry> {
    vec![RecentEntry::new("tok", "notes.md", "/home/n/Documents")]
}

// ── "always" ──────────────────────────────────────────────────────────────────────────────

#[test]
fn always_enabled_even_with_nothing_open() {
    for id in [
        CommandId::New,
        CommandId::Open,
        CommandId::OpenTextBundleFolder,
        CommandId::Example,
        CommandId::ExampleBook,
        CommandId::Close,
        CommandId::PdfPageSize,
        CommandId::Quit, // LINUX: md.win's Exit
        CommandId::FullScreen,
        CommandId::NewBook,
        CommandId::OpenBook,
        CommandId::ShowBook,
        CommandId::Minimize,
        CommandId::Zoom,
        CommandId::Help,
        CommandId::PrivacyPolicy,
        CommandId::About,
    ] {
        assert!(is_enabled(id, &none()), "{id:?}");
    }
}

// ── "doc" ─────────────────────────────────────────────────────────────────────────────────

#[test]
fn needs_an_active_document() {
    // LINUX: md.win's ShareSource and ShareRenderedPdf rows do not exist.
    for id in [
        CommandId::Save,
        CommandId::Duplicate,
        CommandId::Print,
        CommandId::ExportPdf,
        CommandId::ExportHtml,
        CommandId::ExportEpub,
        CommandId::ExportLaTeX,
        CommandId::ExportTextBundle,
        CommandId::ViewEdit,
        CommandId::ViewSplit,
        CommandId::ViewPreview,
    ] {
        assert!(!is_enabled(id, &none()), "{id:?}");
        assert!(is_enabled(id, &document()), "{id:?}");
        // The Book window publishes a document only while its detail session is editing an article.
        assert!(!is_enabled(id, &book_window_idle()), "{id:?}");
        assert!(is_enabled(id, &book_window_editing()), "{id:?}");
    }
}

/// §2.2's `doc` everywhere except the Book window, where an article is its file inside the book
/// folder and there is nowhere else for it to be saved *as* (§8.2, §8.5). The row stays in the bar
/// — every window carries the same eight menus (§2.1) — it is simply never live there.
#[test]
fn save_as_is_a_document_window_row_and_is_refused_by_the_book_window() {
    assert!(!is_enabled(CommandId::SaveAs, &none()));
    assert!(is_enabled(CommandId::SaveAs, &document()));
    assert!(!is_enabled(CommandId::SaveAs, &book_window_idle()));
    assert!(!is_enabled(CommandId::SaveAs, &book_window_editing()));
    // Not "because there is no document": the Book window IS publishing one here, and Save, Print
    // and every Export row are live on the same snapshot.
    assert!(has_active_document(&book_window_editing()));
    assert!(is_enabled(CommandId::Save, &book_window_editing()));
}

// ── "saved" and "dirty" ───────────────────────────────────────────────────────────────────

#[test]
fn needs_a_path_on_disk() {
    for id in [CommandId::Rename, CommandId::MoveTo] {
        assert!(!is_enabled(id, &none()));
        assert!(!is_enabled(id, &document()));
        assert!(is_enabled(id, &saved_document()));
    }
}

#[test]
fn revert_to_saved_needs_both_a_path_and_unsaved_changes() {
    assert!(!is_enabled(CommandId::RevertToSaved, &saved_document()));
    assert!(!is_enabled(CommandId::RevertToSaved, &ShellSnapshot { is_dirty: true, ..document() }));
    assert!(is_enabled(CommandId::RevertToSaved, &ShellSnapshot { is_dirty: true, ..saved_document() }));
}

// ── the dynamic rows' own gates ───────────────────────────────────────────────────────────

#[test]
fn open_recent_needs_entries() {
    for id in [CommandId::OpenRecentEntry, CommandId::ClearRecent] {
        assert!(!is_enabled(id, &none()));
        assert!(is_enabled(id, &ShellSnapshot { recent_entries: one_recent(), ..none() }));
    }
}

#[test]
fn diagram_as_svg_needs_a_diagram() {
    assert!(!is_enabled(CommandId::ExportDiagramSvg, &document()));
    assert!(is_enabled(CommandId::ExportDiagramSvg, &ShellSnapshot { diagrams: one_diagram(), ..document() }));
}

#[test]
fn contents_needs_an_outline_and_notes_need_a_note() {
    assert!(!is_enabled(CommandId::Contents, &document()));
    assert!(is_enabled(CommandId::Contents, &ShellSnapshot { outline: one_heading(), ..document() }));
    assert!(!is_enabled(CommandId::Notes, &document()));
    assert!(is_enabled(CommandId::Notes, &ShellSnapshot { notes: one_note(), ..document() }));
}

#[test]
fn the_window_list_needs_a_window() {
    assert!(!is_enabled(CommandId::ActivateWindow, &none()));
    let one = ShellSnapshot { window_titles: vec![WindowTitle::new(WindowId(1), "Untitled", true)], ..none() };
    assert!(is_enabled(CommandId::ActivateWindow, &one));
}

// ── Edit (§2.4) ───────────────────────────────────────────────────────────────────────────

#[test]
fn needs_the_editor_pane_on_screen() {
    // LINUX: md.win's list less Find… and Replace…, which nudge the editor back instead (below).
    for id in [CommandId::Paste, CommandId::SelectAll] {
        assert!(!is_enabled(id, &document()), "{id:?}");
        assert!(is_enabled(id, &ShellSnapshot { editor_visible: true, ..document() }), "{id:?}");
    }
}

/// LINUX DECISION (shell-design §2.5, the Mac's 1.5 `showEditor`): Find… and Find and Replace…
/// need an editing surface, not an editor on screen — in Preview or Zen's reading half they first
/// nudge the editor back. With no document there is nothing to find in.
#[test]
fn find_and_replace_need_an_editing_surface_and_nudge_the_editor_back() {
    for id in [CommandId::Find, CommandId::Replace] {
        assert!(!is_enabled(id, &none()), "{id:?}");
        assert!(!is_enabled(id, &book_window_idle()), "{id:?}");
        assert!(is_enabled(id, &document()), "{id:?}"); // Preview: the editor is off screen
        assert!(is_enabled(id, &ShellSnapshot { editor_visible: true, ..document() }), "{id:?}");
        assert!(is_enabled(id, &ShellSnapshot { zen_active: true, zen_reading: true, ..document() }), "{id:?}");
        assert!(is_enabled(id, &book_window_editing()), "{id:?}");
    }
}

/// Cut, Copy and Delete act on the selection and on nothing else; enabled on "editor visible"
/// alone they were rows that lit up and then did nothing, the silent no-op the macOS Preview-only
/// Find was; the Mac's own rows grey out the same way (NSTextView validates cut: / copy: /
/// delete: on the selection).
#[test]
fn needs_the_editor_pane_on_screen_and_a_selection_in_it() {
    for id in [CommandId::Cut, CommandId::Copy, CommandId::Delete] {
        assert!(!is_enabled(id, &document()));
        assert!(!is_enabled(id, &ShellSnapshot { editor_visible: true, ..document() }));
        // A selection left behind in an editor that is no longer on screen (Ctrl+3) is not one.
        assert!(!is_enabled(id, &ShellSnapshot { has_selection: true, ..document() }));
        assert!(is_enabled(id, &ShellSnapshot { editor_visible: true, has_selection: true, ..document() }));
    }
}

#[test]
fn undo_and_redo_need_the_editor_and_a_stack() {
    let editing = ShellSnapshot { editor_visible: true, ..document() };
    assert!(!is_enabled(CommandId::Undo, &editing));
    assert!(is_enabled(CommandId::Undo, &ShellSnapshot { can_undo: true, ..editing.clone() }));
    assert!(!is_enabled(CommandId::Undo, &ShellSnapshot { can_undo: true, ..document() }));

    assert!(!is_enabled(CommandId::Redo, &editing));
    assert!(is_enabled(CommandId::Redo, &ShellSnapshot { can_redo: true, ..editing }));
    assert!(!is_enabled(CommandId::Redo, &ShellSnapshot { can_redo: true, ..document() }));
}

/// Find Next / Find Previous search the editor's text and select the hit in the editor — so with no
/// editor on screen (Preview, Zen's reading half) they used to select a hit inside a hidden widget
/// and change nothing anyone could see, while the rows stayed lit. The query is kept, so the rows
/// come back the moment the editor does.
#[test]
fn find_next_needs_a_query_and_an_editor_on_screen() {
    for id in [CommandId::FindNext, CommandId::FindPrevious] {
        assert!(!is_enabled(id, &ShellSnapshot { editor_visible: true, ..document() }));
        assert!(!is_enabled(id, &ShellSnapshot { has_find_query: true, ..document() }));
        assert!(is_enabled(id, &ShellSnapshot { editor_visible: true, has_find_query: true, ..document() }));
        // Zen's writing half shows the editor and no bar: Ctrl+G still walks the text there.
        assert!(is_enabled(
            id,
            &ShellSnapshot { editor_visible: true, has_find_query: true, zen_active: true, ..document() }
        ));
    }
}

/// Ctrl+E copies the editor's selection into the find bar and shows the bar. A selection left in
/// an editor that Ctrl+3 has taken off screen is still a selection to the widget, so the row used to
/// light up in Preview and open the bar over no editor at all — the state the layout rule of §3.5
/// exists to forbid.
#[test]
fn use_selection_for_find_needs_a_selection_in_an_editor_on_screen() {
    let id = CommandId::UseSelectionForFind;
    assert!(!is_enabled(id, &ShellSnapshot { editor_visible: true, ..document() }));
    assert!(!is_enabled(id, &ShellSnapshot { has_selection: true, ..document() }));
    assert!(is_enabled(id, &ShellSnapshot { editor_visible: true, has_selection: true, ..document() }));
}

/// md.win: The_rows_that_open_the_find_bar_are_dead_in_zen — WinUI's Zen had no room for the bar.
/// LINUX DECISION (§3.6): the bar sits at the top of the editor column and goes wherever the
/// editor goes, Zen's writing half included, as the Mac's NSTextView find bar does.
#[test]
fn the_rows_that_open_the_find_bar_live_in_zen_too() {
    let writing = ShellSnapshot { editor_visible: true, has_selection: true, has_find_query: true, ..document() };
    for id in [CommandId::Find, CommandId::Replace, CommandId::UseSelectionForFind] {
        assert!(is_enabled(id, &writing));
        assert!(is_enabled(id, &ShellSnapshot { zen_active: true, ..writing.clone() }));
    }
}

/// md.win: The_find_family_is_never_live_in_the_book_window — its Book window had no find bar.
/// LINUX DECISION (§3.6, §14): the bar belongs to the article panes, so the Book window has it,
/// as the Mac's book workspace does; every find row follows the same rule in both windows.
#[test]
fn the_find_family_is_live_in_the_book_window_as_in_a_document_window() {
    for id in [
        CommandId::Find,
        CommandId::FindNext,
        CommandId::FindPrevious,
        CommandId::Replace,
        CommandId::UseSelectionForFind,
    ] {
        // Everything §2.4 asks of each, at once — in a document window it is enabled.
        assert!(is_enabled(
            id,
            &ShellSnapshot { editor_visible: true, has_selection: true, has_find_query: true, ..document() }
        ));

        let book_window =
            ShellSnapshot { editor_visible: true, has_selection: true, has_find_query: true, ..book_window_editing() };
        assert!(is_enabled(id, &book_window));

        // The Edit rows that DO have a handler there are unaffected — this is about the find bar,
        // not about the Book window's editor.
        assert!(is_enabled(CommandId::Copy, &book_window));
        assert!(is_enabled(CommandId::SelectAll, &book_window));
    }
}

// ── View (§2.5) ───────────────────────────────────────────────────────────────────────────

#[test]
fn zen_is_a_document_window_command_present_but_disabled_in_the_book_window() {
    assert!(is_enabled(CommandId::ZenMode, &document()));
    assert!(!is_enabled(CommandId::ZenMode, &book_window_editing()));
    assert!(!is_enabled(CommandId::ZenMode, &none()));
    assert!(command_table::spec(CommandId::ZenMode).path.is_some()); // present in every window's View menu
}

#[test]
fn show_sidebar_belongs_to_the_book_window() {
    assert!(!is_enabled(CommandId::ShowSidebar, &document()));
    assert!(is_enabled(CommandId::ShowSidebar, &book_window_idle()));
}

// ── Book (§2.6) ───────────────────────────────────────────────────────────────────────────

#[test]
fn needs_an_open_book() {
    // LINUX: md.win's ShareBookPdf row does not exist.
    for id in [
        CommandId::CloseBook,
        CommandId::PrintBook,
        CommandId::ExportBookPdf,
        CommandId::ExportBookEpub,
        CommandId::ExportBookLaTeX,
    ] {
        assert!(!is_enabled(id, &document()), "{id:?}");
        assert!(is_enabled(id, &ShellSnapshot { has_book: true, ..none() }), "{id:?}");
    }
}

// ── Go (§2.7) ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_stepper_is_the_book_windows() {
    let book = ShellSnapshot { can_previous: true, can_next: true, ..book_window_editing() };
    assert!(is_enabled(CommandId::PreviousArticle, &book));
    assert!(is_enabled(CommandId::NextArticle, &book));
    assert!(!is_enabled(CommandId::PreviousArticle, &book_window_editing()));
    assert!(!is_enabled(CommandId::NextArticle, &book_window_editing()));
    // A document window publishes no stepper, even one hosting a book article.
    assert!(!is_enabled(CommandId::PreviousArticle, &ShellSnapshot { can_previous: true, ..document() }));
    assert!(!is_enabled(CommandId::NextArticle, &ShellSnapshot { can_next: true, ..document() }));
}

// ── Escape (§2.9) ─────────────────────────────────────────────────────────────────────────

#[test]
fn escape_belongs_to_the_editor_unless_zen_or_the_find_bar_wants_it() {
    assert!(!is_enabled(CommandId::Escape, &ShellSnapshot { editor_visible: true, ..document() }));
    assert!(is_enabled(CommandId::Escape, &ShellSnapshot { zen_active: true, ..document() }));
    assert!(is_enabled(CommandId::Escape, &ShellSnapshot { find_bar_open: true, ..document() }));
    // LINUX: the Book window's find bar too (§3.6).
    assert!(is_enabled(CommandId::Escape, &ShellSnapshot { find_bar_open: true, ..book_window_editing() }));
}

// ── LINUX: an alert up in the window (§2.2) ───────────────────────────────────────────────

/// Everything §2's tables can ask for, at once, in a document window.
fn everything() -> ShellSnapshot {
    ShellSnapshot {
        is_saved: true,
        is_dirty: true,
        has_book: true,
        outline: one_heading(),
        notes: one_note(),
        diagrams: one_diagram(),
        editor_visible: true,
        can_undo: true,
        can_redo: true,
        recent_entries: one_recent(),
        window_titles: vec![WindowTitle::new(WindowId(1), "notes.md", true)],
        has_selection: true,
        has_find_query: true,
        find_bar_open: true,
        ..document()
    }
}

#[test]
fn an_open_dialog_disables_every_window_command_and_leaves_the_applications_alone() {
    // adw::AlertDialog is presented inside the window, so its shortcuts would still reach the
    // window behind it; the Mac's sheet blocks the window's menu items the same way.
    let blocked = ShellSnapshot { dialog_open: true, ..everything() };
    for id in CommandId::ALL {
        match id.scope() {
            ActionScope::Window => assert!(!is_enabled(id, &blocked), "{id:?} is live behind a dialog"),
            ActionScope::App => assert_eq!(is_enabled(id, &blocked), is_enabled(id, &everything()), "{id:?}"),
        }
    }
    // The same snapshot without the dialog lights the window's rows.
    for id in [CommandId::Save, CommandId::Copy, CommandId::Find, CommandId::Bold, CommandId::Escape] {
        assert!(is_enabled(id, &everything()), "{id:?}");
    }
    // Ctrl+N and Ctrl+Q still work, and so do the settings toggles.
    for id in [CommandId::New, CommandId::Quit, CommandId::ContinueLists, CommandId::ShowBook] {
        assert!(is_enabled(id, &blocked), "{id:?}");
    }
}

#[test]
fn an_open_dialog_greys_the_windows_submenus_but_not_the_ticks() {
    let blocked = ShellSnapshot { dialog_open: true, zen_active: true, ..everything() };
    assert!(!is_submenu_enabled(&MenuPath::submenu("Edit", "Find"), &blocked));
    assert!(!is_submenu_enabled(&MenuPath::submenu("Format", "Heading"), &blocked));
    assert!(!is_submenu_enabled(&MenuPath::nested("File", "Export", "Diagram as SVG"), &blocked));
    // Export ▸ itself stays live: the PDF Page Size picker inside it is the application's setting.
    assert!(is_submenu_enabled(&MenuPath::submenu("File", "Export"), &blocked));
    // Open Recent's rows are the application's.
    assert!(is_submenu_enabled(&MenuPath::submenu("File", "Open Recent"), &blocked));
    // A tick is state, not a command: it stays what it is while the row is greyed.
    assert!(is_checked(CommandId::ZenMode, &blocked));
}

// ── Submenus (§2.2, §2.6) ─────────────────────────────────────────────────────────────────

#[test]
fn share_goes_with_the_document_and_export_is_never_disabled() {
    // LINUX: there is no File ▸ Share ▸ to go with the document — a path naming it holds no row,
    // so it is never live; Export ▸ keeps md.win's rule.
    let share = MenuPath::submenu("File", "Share");
    let export = MenuPath::submenu("File", "Export");
    assert!(!is_submenu_enabled(&share, &none()));
    assert!(!is_submenu_enabled(&share, &document()));
    // PDF Page Size inside it is a setting, so Export is live with no document at all.
    assert!(is_submenu_enabled(&export, &none()));
    assert!(is_submenu_enabled(&export, &document()));
}

#[test]
fn the_other_submenus_follow_their_contents() {
    let diagrams = MenuPath::nested("File", "Export", "Diagram as SVG");
    assert!(!is_submenu_enabled(&diagrams, &document()));
    assert!(is_submenu_enabled(&diagrams, &ShellSnapshot { diagrams: one_diagram(), ..document() }));
    assert!(is_submenu_enabled(&MenuPath::nested("File", "Export", "PDF Page Size"), &none()));
    let book_export = MenuPath::submenu("Book", "Export Book");
    assert!(!is_submenu_enabled(&book_export, &document()));
    assert!(is_submenu_enabled(&book_export, &ShellSnapshot { has_book: true, ..none() }));
    let recent = MenuPath::submenu("File", "Open Recent");
    assert!(!is_submenu_enabled(&recent, &none()));
    assert!(is_submenu_enabled(&recent, &ShellSnapshot { recent_entries: one_recent(), ..none() }));
    assert!(is_submenu_enabled(&MenuPath::submenu("File", "Examples"), &none()));
    let contents = MenuPath::submenu("Go", "Contents");
    assert!(!is_submenu_enabled(&contents, &document()));
    assert!(is_submenu_enabled(&contents, &ShellSnapshot { outline: one_heading(), ..document() }));
    // Edit ▸ Typing holds settings, live everywhere (smart-typing.md §3.1): never greyed.
    assert!(is_submenu_enabled(&MenuPath::submenu("Edit", "Typing"), &none()));
    assert!(is_submenu_enabled(&MenuPath::submenu("Edit", "Typing"), &book_window_idle()));
}

// ── Ticks (§2.5) ──────────────────────────────────────────────────────────────────────────

#[test]
fn the_view_tick_follows_the_displayed_mode() {
    for (mode, edit, split, preview) in [
        (ViewMode::Edit, true, false, false),
        (ViewMode::Split, false, true, false),
        (ViewMode::Preview, false, false, true),
    ] {
        let s = ShellSnapshot { displayed_mode: mode, ..document() };
        assert_eq!(is_checked(CommandId::ViewEdit, &s), edit);
        assert_eq!(is_checked(CommandId::ViewSplit, &s), split);
        assert_eq!(is_checked(CommandId::ViewPreview, &s), preview);
    }
}

#[test]
fn in_zen_the_tick_is_write_or_read_and_split_is_never_ticked() {
    // Whatever the window's own mode was, Zen publishes edit or preview (§2.5).
    let writing = ShellSnapshot { displayed_mode: ViewMode::Split, zen_active: true, zen_reading: false, ..document() };
    assert!(is_checked(CommandId::ViewEdit, &writing));
    assert!(!is_checked(CommandId::ViewSplit, &writing));
    assert!(!is_checked(CommandId::ViewPreview, &writing));

    let reading = ShellSnapshot { zen_reading: true, ..writing };
    assert!(!is_checked(CommandId::ViewEdit, &reading));
    assert!(!is_checked(CommandId::ViewSplit, &reading));
    assert!(is_checked(CommandId::ViewPreview, &reading));
}

#[test]
fn zen_and_sidebar_tick_themselves() {
    assert!(!is_checked(CommandId::ZenMode, &document()));
    assert!(is_checked(CommandId::ZenMode, &ShellSnapshot { zen_active: true, ..document() }));
    assert!(!is_checked(CommandId::ShowSidebar, &book_window_idle()));
    assert!(is_checked(CommandId::ShowSidebar, &ShellSnapshot { sidebar_open: true, ..book_window_idle() }));
}

#[test]
fn the_typing_toggles_tick_their_settings() {
    // smart-typing.md §3.1: default true; the tick reads the snapshot, which reads the store.
    assert!(is_checked(CommandId::ContinueLists, &document()));
    assert!(is_checked(CommandId::CapitalizeSentences, &document()));
    let lists_off = ShellSnapshot { continue_lists: false, ..document() };
    assert!(!is_checked(CommandId::ContinueLists, &lists_off));
    assert!(is_checked(CommandId::CapitalizeSentences, &lists_off));
    let capitals_off = ShellSnapshot { capitalize_sentences: false, ..document() };
    assert!(!is_checked(CommandId::CapitalizeSentences, &capitals_off));
    assert!(is_checked(CommandId::ContinueLists, &capitals_off));
}

#[test]
fn the_typing_toggles_are_live_in_every_window() {
    // A setting, like the PDF page size: no document, the Book window idle, or an editor hidden —
    // still switchable.
    for id in [CommandId::ContinueLists, CommandId::CapitalizeSentences] {
        assert!(is_enabled(id, &none()));
        assert!(is_enabled(id, &document()));
        assert!(is_enabled(
            id,
            &ShellSnapshot { displayed_mode: ViewMode::Preview, editor_visible: false, ..document() }
        ));
        assert!(is_enabled(id, &book_window_idle()));
        assert!(is_enabled(id, &book_window_editing()));
    }
}

#[test]
fn nothing_else_is_ever_ticked() {
    let toggles = [
        CommandId::ViewEdit,
        CommandId::ViewSplit,
        CommandId::ViewPreview,
        CommandId::ZenMode,
        CommandId::ShowSidebar,
        CommandId::ContinueLists,
        CommandId::CapitalizeSentences,
        CommandId::WrapSelection,
    ];
    let full = ShellSnapshot {
        zen_active: true,
        zen_reading: true,
        sidebar_open: true,
        is_book_window: true,
        is_full_screen: true,
        ..document()
    };
    for id in CommandId::ALL.into_iter().filter(|id| !toggles.contains(id)) {
        assert!(!is_checked(id, &full), "{id:?}");
    }
}

#[test]
fn the_radio_ticks_the_stored_page_size_and_the_window_row_ticks_this_window() {
    let s = ShellSnapshot { pdf_page_size_id: PageSize::A5.id.to_string(), ..none() };
    assert!(is_row_checked(CommandId::PdfPageSize, &CommandArgument::Text("a5".into()), &s));
    assert!(!is_row_checked(CommandId::PdfPageSize, &CommandArgument::Text("a4".into()), &s));
    // Exact, like PageSize::named.
    assert!(!is_row_checked(CommandId::PdfPageSize, &CommandArgument::Text("A5".into()), &s));

    let (mine, other) = (WindowId(7), WindowId(8));
    let windows = ShellSnapshot {
        window_titles: vec![WindowTitle::new(mine, "notes", true), WindowTitle::new(other, "Book", false)],
        ..none()
    };
    assert!(is_row_checked(CommandId::ActivateWindow, &CommandArgument::Window(mine), &windows));
    assert!(!is_row_checked(CommandId::ActivateWindow, &CommandArgument::Window(other), &windows));
}

// ── The snapshot itself ───────────────────────────────────────────────────────────────────

fn hash_of(s: &ShellSnapshot) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn snapshots_compare_their_lists_by_value_so_an_unchanged_outline_does_not_rebuild_the_menu() {
    // §2.9: the Go and Diagram submenus are rebuilt on snapshot change, and the derived-text tick
    // hands over freshly parsed but identical lists 250 ms after every edit.
    let a = ShellSnapshot {
        outline: vec![OutlineEntry::new(1, "Title", "title", 0)],
        diagrams: vec![DiagramRef::new(0, "Plot")],
        ..document()
    };
    let b = ShellSnapshot {
        outline: vec![OutlineEntry::new(1, "Title", "title", 0)],
        diagrams: vec![DiagramRef::new(0, "Plot")],
        ..document()
    };
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
    assert_ne!(a, ShellSnapshot { outline: vec![OutlineEntry::new(2, "Title", "title", 0)], ..b.clone() });
    assert_ne!(a, ShellSnapshot { diagrams: vec![], ..b });
}

#[test]
fn the_empty_snapshot_is_the_default_page_size_and_the_default_mode() {
    let empty = ShellSnapshot::empty();
    assert_eq!(empty.pdf_page_size_id, PageSize::DEFAULT_ID);
    assert_eq!(empty.displayed_mode, ViewMode::WINDOW_DEFAULT);
    assert!(empty.outline.is_empty());
    assert!(empty.window_titles.is_empty());
    assert_eq!(ShellSnapshot::default(), empty);
}

#[test]
fn every_command_has_an_enablement_rule() {
    // md.win throws for an id with no rule; here the match is exhaustive, so this only proves
    // nothing panics.
    for id in CommandId::ALL {
        is_enabled(id, &none());
    }
}
