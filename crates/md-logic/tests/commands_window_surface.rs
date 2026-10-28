//! Port of md.win `tests/Md.App.Logic.Tests/WindowSurfaceTests.cs`. md.win's suite pins the
//! WinUI windows' *source* (`src/Md.App/Windows/*.cs`) against shell-design §1–§8: it reads the
//! files and asserts fragments. None of that source exists here — the GTK app gets its own
//! surface tests — so every source pin is ported as an ignored test naming what it checked, and the
//! assertions that concern md-logic data (enablement against the suite's two snapshots, the
//! accelerator table) are ported and run.

use md_core::markdown::block::{NoteEntry, OutlineEntry};
use md_logic::commands::command_enablement::is_enabled;
use md_logic::commands::command_table;
use md_logic::commands::{CommandId, DiagramRef, RecentEntry, ShellSnapshot, WindowTitle};
use md_logic::seams::WindowId;

/// A document window with everything §2's tables can ask for, Zen included.
fn document_snapshot() -> ShellSnapshot {
    ShellSnapshot {
        has_document: true,
        is_saved: true,
        is_dirty: true,
        has_book: true,
        outline: vec![OutlineEntry::new(1, "Title", "title", 0)],
        notes: vec![NoteEntry::new("A note", 3)],
        diagrams: vec![DiagramRef::new(0, "Mermaid: flow")],
        editor_visible: true,
        can_undo: true,
        can_redo: true,
        recent_entries: vec![RecentEntry::new("tok", "notes.md", "/home/n/Documents")],
        window_titles: vec![WindowTitle::new(WindowId(1), "notes.md", true)],
        has_selection: true,
        has_find_query: true,
        zen_active: true,
        ..ShellSnapshot::empty()
    }
}

/// The Book window with an article open, a book on disk and a stepper that can move both ways.
fn book_snapshot() -> ShellSnapshot {
    ShellSnapshot {
        is_book_window: true,
        is_editing_article: true,
        is_dirty: true,
        has_book: true,
        outline: vec![OutlineEntry::new(1, "Title", "title", 0)],
        notes: vec![NoteEntry::new("A note", 3)],
        diagrams: vec![DiagramRef::new(0, "Mermaid: flow")],
        can_previous: true,
        can_next: true,
        editor_visible: true,
        can_undo: true,
        can_redo: true,
        // §2.1: Open Recent's rows are every window's, the Book window included (§6.6).
        recent_entries: vec![RecentEntry::new("tok", "notes.md", "/home/n/Documents")],
        window_titles: vec![WindowTitle::new(WindowId(2), "Book", true)],
        has_selection: true,
        sidebar_open: true,
        ..ShellSnapshot::empty()
    }
}

// ── The source pins (WinUI only) ──────────────────────────────────────────────────────────

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_window_xaml_is_structure_and_names_only() {
    // md.win: DocumentWindow.xaml carries no bindings, styles or handlers (§11.2).
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_window_has_the_five_rows_section_1_3_names_in_order() {
    // md.win: DocumentWindow.xaml's menu bar, find bar, content, footer and info bar rows, in order.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_client_area_is_sized_in_physical_pixels_and_the_minimum_is_the_presenters() {
    // md.win: AppWindow.ResizeClient and OverlappedPresenter's minimum size.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn all_three_close_routes_are_present_and_closed_is_cleanup_only() {
    // md.win: Ctrl+W, the caption button and Exit reach one policy; Closed never vetoes.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn ctrl_w_and_the_system_close_button_run_the_same_policy_exactly_once() {
    // md.win: AppWindow.Closing and the Close command share RequestCloseAsync, guarded once.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn a_close_waits_for_an_export_or_print_in_flight() {
    // md.win: the close policy awaits the export pipeline's busy state.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn only_a_saved_document_gets_a_session_row() {
    // md.win: WindowManager writes session.json rows for saved documents only.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_session_is_written_on_every_window_close_and_from_the_windows_exit_found_open() {
    // md.win: Forget() and RequestExitAsync() call SaveSession() in that order.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn a_window_root_accepts_dropped_documents() {
    // md.win: AllowDrop + DragOver/Drop through ActivationRouter.Classify.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn zen_is_the_macs_fractions_applied_to_the_content_grid() {
    // md.win: the 1 : 4 : 1 by 4 : 92 : 4 star grid in DocumentWindow.xaml.cs (§5.4).
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_redirect_also_asks_win32_to_come_forward() {
    // md.win: single-instance redirection calls SetForegroundWindow.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_title_bar_is_tinted_rather_than_replaced() {
    // md.win: AppWindowTitleBar colours, ExtendsContentIntoTitleBar never set.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn both_windows_accept_a_dropped_document_document_window() {
    // md.win (theory row DocumentWindow.xaml.cs): the drop handlers.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn both_windows_accept_a_dropped_document_book_window() {
    // md.win (theory row BookWindow.xaml.cs): the drop handlers.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_book_window_publishes_and_wires_open_recent() {
    // md.win: BookWindow.RefreshRecent and its Open Recent registrations.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn both_windows_show_the_export_ring_in_their_footer() {
    // md.win: the footer's ProgressRing bound to the pipeline's BusyChanged.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_book_window_uses_the_shared_title_and_size_codecs() {
    // md.win: WindowTitle / WindowPlacement codecs instead of a private ParseSize.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn neither_window_positions_its_export_canvas_with_canvas_attached_properties() {
    // md.win: no Canvas.SetLeft/SetTop(ExportCanvas …) in either window.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn every_command_is_wired_in_a_window() {
    // md.win: every CommandTable id appears in a `.Register(CommandId.X` in the three window files.
}

/// md.win: the document window registers every id its snapshot can enable (a source scan,
/// WinUI only), and the three rows a document window cannot light up are the Book window's own.
/// The second half is md-logic's and is ported.
#[test]
fn the_document_window_wires_every_row_its_own_menus_can_enable() {
    for id in [CommandId::PreviousArticle, CommandId::NextArticle, CommandId::ShowSidebar] {
        assert!(!is_enabled(id, &document_snapshot()), "{id:?}");
    }
}

/// md.win: the Book window and the manager register every id the Book snapshot enables (a source
/// scan, WinUI only); and Open Recent belongs to every window (§2.1), so a Book window with entries
/// lights it up. The second half is ported.
#[test]
fn the_book_window_wires_every_row_its_own_menus_can_enable() {
    assert!(is_enabled(CommandId::OpenRecentEntry, &book_snapshot()));
    assert!(is_enabled(CommandId::ClearRecent, &book_snapshot()));
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_find_bars_replace_and_replace_all_travel_the_normal_document_path() {
    // md.win: OnReplaceRequested / OnReplaceAllRequested go through Panes.Editor.ReplaceRange.
}

/// §3.6: Ctrl+Z reaches the document only through the widget that holds focus — on Linux because
/// Undo's handler dispatches to that widget (§2.2) — so Replace All must hand focus back to the
/// editor by selecting what it rewrote. md.win pins that in the window's source (WinUI only); the
/// half that is md-logic's — Undo acts on the focused widget — is ported.
#[test]
fn replace_all_selects_what_it_rewrote_so_ctrl_z_reaches_the_document() {
    assert!(command_table::spec(CommandId::Undo).dispatches_to_focus);
}

/// md.win: Find… and Replace… are greyed the moment the editor leaves the screen. LINUX DECISION
/// (§2.5, the Mac's 1.5): the bar still leaves with the editor, but Find… and Find and Replace…
/// stay live and nudge the editor back first (`ViewModeController::show_editor`); the rows that
/// act on a hit or on the editor's selection are greyed until it is back.
#[test]
fn the_find_bar_leaves_the_screen_with_the_editor() {
    let preview = ShellSnapshot { editor_visible: false, zen_active: false, ..document_snapshot() };
    assert!(is_enabled(CommandId::Find, &preview));
    assert!(is_enabled(CommandId::Replace, &preview));
    assert!(!is_enabled(CommandId::FindNext, &preview));
    assert!(!is_enabled(CommandId::FindPrevious, &preview));
    assert!(!is_enabled(CommandId::UseSelectionForFind, &preview));
}

/// md.win: The_book_window_never_enables_the_four_rows_it_has_no_surface_for (Save As… and the
/// three find rows that open the bar). LINUX DECISION (§3.6): the find bar belongs to the article
/// panes, so the Book window has one, as the Mac's book workspace does; only Save As… stays dark
/// there.
#[test]
fn the_book_window_never_enables_save_as_and_has_the_find_bar() {
    let everything = ShellSnapshot { has_find_query: true, recent_entries: vec![], ..book_snapshot() };
    assert!(!is_enabled(CommandId::SaveAs, &everything));
    assert!(is_enabled(CommandId::Find, &everything));
    assert!(is_enabled(CommandId::Replace, &everything));
    assert!(is_enabled(CommandId::FindNext, &everything));
    assert!(is_enabled(CommandId::UseSelectionForFind, &everything));
    // The same rows in a document window, so this is about the Book window and not the snapshot.
    let writing = ShellSnapshot { zen_active: false, ..document_snapshot() };
    assert!(is_enabled(CommandId::SaveAs, &writing));
    assert!(is_enabled(CommandId::Find, &writing));
    assert!(is_enabled(CommandId::Replace, &writing));
    assert!(is_enabled(CommandId::UseSelectionForFind, &writing));
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn both_windows_give_the_editor_the_typing_settings_and_tick_them_from_the_store() {
    // md.win: UseSettings, TypingSettings.Read / IsKey / Toggle wiring in both windows.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn every_format_row_goes_through_the_pane() {
    // md.win: each FormattingAdapter command is registered to EditorPane.ApplyFormat.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn both_windows_route_undo_redo_and_paste_through_the_pane() {
    // md.win: the Edit rows call EditorPane.Undo/Redo/Paste, which announce to the hooks.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn both_windows_reset_the_editor_through_the_pane_on_a_new_undo_generation() {
    // md.win: UndoGeneration → EditorPane.ResetHistory().
}

/// md.win: the three find rows that open the bar are dead in Zen, and no find row is live with no
/// editor on screen. LINUX DECISION (§2.5, §3.6, the Mac's): Zen's writing half keeps the editor
/// and its bar, so every find row works there; with only the preview on screen (Preview, Zen
/// reading) Find… and Find and Replace… act by nudging the editor back, and the three rows that
/// would act on a hidden editor stay dark.
#[test]
fn every_find_row_the_enablement_lights_up_has_a_handler_that_acts() {
    let zen = ShellSnapshot { zen_active: true, editor_visible: true, ..document_snapshot() };
    for id in [
        CommandId::Find,
        CommandId::Replace,
        CommandId::FindNext,
        CommandId::FindPrevious,
        CommandId::UseSelectionForFind,
    ] {
        assert!(is_enabled(id, &zen), "{id:?} is dark in Zen's writing half, which shows the editor and its bar.");
    }
    for zen_reading in [false, true] {
        let preview = ShellSnapshot {
            zen_active: zen_reading,
            zen_reading,
            editor_visible: false,
            has_find_query: true,
            has_selection: true,
            ..document_snapshot()
        };
        for id in [CommandId::Find, CommandId::Replace] {
            assert!(is_enabled(id, &preview), "{id:?} must nudge the editor back, not grey out");
        }
        for id in [CommandId::FindNext, CommandId::FindPrevious, CommandId::UseSelectionForFind] {
            assert!(
                !is_enabled(id, &preview),
                "{id:?} is enabled with no editor on screen, where it can only act on a hidden widget."
            );
        }
    }
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn both_windows_republish_when_the_editors_selection_moves() {
    // md.win: SelectionChanged → Publish() in both windows.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn a_toggle_rows_tick_is_put_back_from_the_snapshot_after_every_click() {
    // md.win: MenuBarBuilder re-ticks toggle rows from IsChecked / IsRowChecked, posted.
}
