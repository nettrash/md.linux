//! The `shell` self-test group (shell-design §13.3): app starts; one window; eight menus in order;
//! every `CommandTable` row once with its title; accels as §2.9; ticks follow the snapshot; Quit
//! keeps drafts and the relaunch restores them; a disabled action's chord reaches the focused
//! widget; the dialog-open flag; the second-launch hand-off; `--version`; every live command wired.
//!
//! Checks that need another package's real code (P2's file system to open a saved document, P3's
//! editor to type into) probe for it first and print `SKIP` with the reason while the package's S0
//! stub is in; at the integration gate they run.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use md_core::book::article_file_system::FileStamp;
use md_core::document::view_mode::ViewMode;
use md_core::markdown::block::{NoteEntry, OutlineEntry};
use md_logic::commands::{
    ActionScope, CommandId, CommandKind, DiagramRef, MenuNode, RecentEntry, ShellSnapshot, WindowTitle,
    command_enablement, command_table,
};
use md_logic::seams::{DocumentRegistry, FileSystem, WindowId};
use md_logic::strings;

use crate::menus::{accels, actions, menu_bar};
use crate::selftest::Harness;
use crate::windows::document_window::DocumentWindow;
use crate::windows::host::WindowHost;

/// The actions whose rows are data (the snapshot's lists), not table rows.
const DYNAMIC_ACTIONS: [&str; 7] = [
    "app.open-recent-entry",
    "app.example",
    "app.pdf-page-size",
    "win.export-diagram-svg",
    "win.contents",
    "win.notes",
    "win.activate-window",
];

/// The text the Quit → relaunch checks carry through a draft.
const DRAFT_TEXT: &str = "# Draft from the self-test\n\nПривет — 😀 kept across a relaunch.\n";

/// The double-fire guard swallows a second activation of one command inside 150 ms.
const BETWEEN: Duration = Duration::from_millis(220);

fn skip(name: &str, reason: &str) {
    println!("SKIP shell.{name} — {reason}");
}

pub async fn run(h: &Harness) {
    let shell = h.shell();
    h.check(
        "app_starts",
        shell.app.is_registered(),
        format!("application id {}", shell.app.application_id().unwrap_or_default()),
    );

    let documents = shell.windows.documents();
    let Some(first) = documents.first().cloned() else {
        h.check("one_window", false, "no document window");
        return;
    };
    let title = first.adw_window().title().map(|t| t.to_string()).unwrap_or_default();
    h.check(
        "one_window",
        documents.len() == 1 && shell.app.windows().len() == 1 && title == strings::UNTITLED,
        format!("{} document window(s), {} toplevel(s), title {title:?}", documents.len(), shell.app.windows().len()),
    );

    check_menus(h, &first);
    check_accelerators(h);
    check_wiring(h, &first);
    h.screenshot("one-window").await;
    open_file_menu(h, &first).await;

    // A second window for the per-window ticks.
    let second_id = shell.windows.open_untitled(None, false);
    h.sleep(Duration::from_millis(400)).await;
    let Some(second) = shell.windows.documents().into_iter().find(|w| w.id() == second_id) else {
        h.check("second_window", false, "open_untitled made no window");
        return;
    };
    h.check("second_window", second.adw_window().title().is_some_and(|t| t == "Untitled 2"), "titled \"Untitled 2\"");

    check_ticks(h, &first, &second).await;
    check_window_list(h, &first, &second).await;
    check_dialog_open(h, &first, &second).await;
    check_chords(h, &first).await;
    let saved = open_saved_document(h).await;
    let draft = check_quit_keeps_drafts(h, saved.as_ref()).await;
    if let Some(draft) = &draft {
        check_relaunch(h, draft, saved.as_ref()).await;
    }
    check_version(h).await;
    check_about(h).await;
    check_close_policy(h, &second, draft.as_ref()).await;

    // Leave one window for the groups after this one.
    for window in shell.windows.documents() {
        if window.id() != first.id() {
            if let Some(path) = window.draft_path() {
                shell.windows.drafts().delete(&path);
            }
            shell.windows.close_window(&window);
        }
    }
    h.sleep(Duration::from_millis(300)).await;
}

// ---- menus ----------------------------------------------------------------------------------------

/// "File ▸ Export ▸ PDF…|win.export-pdf" for every static row and "File ▸ Open Recent|" for every
/// submenu, in md-logic's order.
fn expected_rows() -> Vec<String> {
    fn walk(nodes: &[MenuNode], prefix: &str, out: &mut Vec<String>) {
        for node in nodes {
            let path = format!("{prefix} \u{25B8} {}", node.title);
            match node.command {
                None => {
                    out.push(format!("{path}|"));
                    walk(&node.children, &path, out);
                }
                Some(spec) if matches!(spec.kind, CommandKind::Item | CommandKind::Toggle) => {
                    out.push(format!("{path}|{}", actions::detailed_name(spec.id).unwrap_or_default()));
                }
                Some(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    for root in command_table::menus() {
        walk(&root.children, root.title, &mut out);
    }
    out
}

/// The same list read back from a GTK menu model.
fn actual_rows(model: &gio::MenuModel, prefix: &str, out: &mut Vec<String>) {
    for i in 0..model.n_items() {
        if let Some(section) = model.item_link(i, gio::MENU_LINK_SECTION) {
            actual_rows(&section, prefix, out);
            continue;
        }
        let label = model
            .item_attribute_value(i, gio::MENU_ATTRIBUTE_LABEL, Some(glib::VariantTy::STRING))
            .and_then(|v| v.str().map(str::to_string))
            .unwrap_or_default();
        let path = format!("{prefix} \u{25B8} {}", menu_bar::displayed(&label));
        if let Some(submenu) = model.item_link(i, gio::MENU_LINK_SUBMENU) {
            out.push(format!("{path}|"));
            actual_rows(&submenu, &path, out);
            continue;
        }
        let action = model
            .item_attribute_value(i, gio::MENU_ATTRIBUTE_ACTION, Some(glib::VariantTy::STRING))
            .and_then(|v| v.str().map(str::to_string))
            .unwrap_or_default();
        if DYNAMIC_ACTIONS.contains(&action.as_str()) {
            continue;
        }
        if action == format!("win.{}", actions::UNAVAILABLE) {
            // A submenu with nothing live inside, greyed in place.
            out.push(format!("{path}|-"));
            continue;
        }
        let target =
            model.item_attribute_value(i, gio::MENU_ATTRIBUTE_TARGET, None).and_then(|v| v.str().map(str::to_string));
        let detailed = match target {
            Some(target) => format!("{action}::{target}"),
            None => action,
        };
        out.push(format!("{path}|{detailed}"));
    }
}

fn check_menus(h: &Harness, window: &DocumentWindow) {
    let Some(model) = window.menu_model() else {
        h.check("eight_menus", false, "the menu bar has no model");
        return;
    };
    let titles: Vec<String> = (0..model.n_items())
        .map(|i| {
            model
                .item_attribute_value(i, gio::MENU_ATTRIBUTE_LABEL, Some(glib::VariantTy::STRING))
                .and_then(|v| v.str().map(str::to_string))
                .unwrap_or_default()
        })
        .collect();
    let shown: Vec<String> = titles.iter().map(|t| menu_bar::displayed(t)).collect();
    h.check("eight_menus", shown == command_table::MENU_TITLES && titles[2] == "F_ormat", titles.join(" · "));

    let mut actual = Vec::new();
    for i in 0..model.n_items() {
        if let Some(menu) = model.item_link(i, gio::MENU_LINK_SUBMENU) {
            actual_rows(&menu, &shown[i as usize], &mut actual);
        }
    }
    // A greyed submenu stands for its whole subtree.
    let collapsed: Vec<String> =
        actual.iter().filter_map(|r| r.strip_suffix("|-")).map(|p| format!("{p} \u{25B8} ")).collect();
    let greyed = collapsed.len();
    let actual: Vec<String> =
        actual.into_iter().map(|r| r.strip_suffix("|-").map_or(r.clone(), |p| format!("{p}|"))).collect();
    let expected: Vec<String> =
        expected_rows().into_iter().filter(|r| !collapsed.iter().any(|c| r.starts_with(c.as_str()))).collect();
    let first_difference = expected
        .iter()
        .zip(actual.iter())
        .find(|(e, a)| e != a)
        .map(|(e, a)| format!("expected {e:?}, found {a:?}"))
        .unwrap_or_else(|| format!("{} rows as expected, {greyed} empty submenus greyed", actual.len()));
    h.check("rows_once_with_titles", expected == actual, first_difference);

    let quit = actual.iter().any(|r| r == "File \u{25B8} Quit|app.quit");
    let find = actual.iter().any(|r| r == "Edit \u{25B8} Find \u{25B8} Find and Replace\u{2026}|win.replace");
    let wrap = actual.iter().any(|r| r.contains("Wrap Selection When Typing * _ ` $|app.wrap-selection"));
    h.check("linux_titles", quit && find && wrap, "Quit, Edit ▸ Find ▸ Find and Replace…, the underscore kept");
}

fn check_accelerators(h: &Harness) {
    let app = h.shell().app.clone();
    let mut wrong = Vec::new();
    let mut unparsed = Vec::new();
    let mut count = 0;
    for spec in command_table::root_accelerators() {
        let Some(detailed) = actions::detailed_name(spec.id) else {
            wrong.push(format!("{:?} has no action", spec.id));
            continue;
        };
        let installed: Vec<String> = app.accels_for_action(&detailed).iter().map(|a| a.to_string()).collect();
        let wanted = accels::accelerators(spec);
        if installed != wanted {
            wrong.push(format!("{detailed}: {installed:?} ≠ {wanted:?}"));
        }
        for accel in &wanted {
            count += 1;
            match gtk::accelerator_parse(accel.as_str()) {
                Some((key, modifiers)) if gtk::accelerator_name(key, modifiers) == accel.as_str() => {}
                other => unparsed.push(format!("{accel} → {other:?}")),
            }
        }
    }
    h.check(
        "accels_installed",
        wrong.is_empty(),
        if wrong.is_empty() { format!("{count} accelerators") } else { wrong.join("; ") },
    );
    h.check(
        "accels_parse",
        unparsed.is_empty(),
        if unparsed.is_empty() { format!("{count} round trips") } else { unparsed.join("; ") },
    );
    let escape = app.actions_for_accel("Escape");
    let delete = app.actions_for_accel("Delete");
    h.check(
        "escape_and_delete_unregistered",
        escape.is_empty() && delete.is_empty(),
        format!("Escape → {escape:?}, Delete → {delete:?}"),
    );
}

/// Every `win.` command a document window can enable has a handler there (the integration gate's
/// rule, md.win's `WindowSurfaceTests`).
fn check_wiring(h: &Harness, window: &DocumentWindow) {
    let rich = ShellSnapshot {
        has_document: true,
        is_saved: true,
        is_dirty: true,
        has_book: true,
        zen_active: true,
        outline: vec![OutlineEntry::new(1, "A", "a", 0)],
        notes: vec![NoteEntry::new("n", 1)],
        diagrams: vec![DiagramRef::new(0, "Mermaid")],
        editor_visible: true,
        can_undo: true,
        can_redo: true,
        recent_entries: vec![RecentEntry::new("t", "n", "f")],
        window_titles: vec![WindowTitle::new(WindowId(1), "w", true)],
        has_selection: true,
        has_find_query: true,
        find_bar_open: true,
        ..ShellSnapshot::empty()
    };
    let plain = ShellSnapshot { zen_active: false, ..rich.clone() };
    let wired = window.wired();
    let missing: Vec<String> = CommandId::ALL
        .into_iter()
        .filter(|id| id.scope() == ActionScope::Window)
        .filter(|id| command_enablement::is_enabled(*id, &rich) || command_enablement::is_enabled(*id, &plain))
        .filter(|id| !wired.contains(id))
        .map(|id| format!("{id:?}"))
        .collect();
    h.check(
        "document_commands_wired",
        missing.is_empty(),
        if missing.is_empty() {
            format!("{} handlers", wired.len())
        } else {
            format!("no handler: {}", missing.join(", "))
        },
    );
}

/// Opens the File menu with a real click and photographs it.
async fn open_file_menu(h: &Harness, window: &DocumentWindow) {
    if !h.input() {
        return;
    }
    let toplevel = window.window();
    h.focus(&toplevel).await;
    let Some(bar) = find_menu_bar(toplevel.upcast_ref()) else { return };
    h.click(&toplevel, &bar, 20.0, 10.0).await;
    h.sleep(Duration::from_millis(400)).await;
    h.screenshot("file-menu").await;
    h.key("Escape").await;
}

fn find_menu_bar(widget: &gtk::Widget) -> Option<gtk::PopoverMenuBar> {
    if let Some(bar) = widget.downcast_ref::<gtk::PopoverMenuBar>() {
        return Some(bar.clone());
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        if let Some(bar) = find_menu_bar(&c) {
            return Some(bar);
        }
        child = c.next_sibling();
    }
    None
}

// ---- ticks --------------------------------------------------------------------------------------

fn state_of(window: &DocumentWindow, action: &str) -> Option<glib::Variant> {
    actions::action_map(window.adw_window()).get(action).and_then(|(_, state)| state.clone())
}

fn enabled_in(window: &DocumentWindow, action: &str) -> bool {
    actions::action_map(window.adw_window()).get(action).is_some_and(|(enabled, _)| *enabled)
}

fn activate(window: &DocumentWindow, action: &str, target: Option<glib::Variant>) {
    gio::prelude::ActionGroupExt::activate_action(window.adw_window(), action, target.as_ref());
}

fn activate_app(h: &Harness, action: &str, target: Option<glib::Variant>) {
    gio::prelude::ActionGroupExt::activate_action(&h.shell().app, action, target.as_ref());
}

async fn check_ticks(h: &Harness, a: &Rc<DocumentWindow>, b: &Rc<DocumentWindow>) {
    // View mode: per window; re-clicking the ticked row keeps it.
    let before_b = state_of(b, "win.view-mode");
    activate(a, "view-mode", Some("preview".to_variant()));
    h.sleep(BETWEEN).await;
    let a_preview = state_of(a, "win.view-mode").and_then(|v| v.str().map(str::to_string));
    activate(a, "view-mode", Some("preview".to_variant()));
    h.sleep(BETWEEN).await;
    let a_again = state_of(a, "win.view-mode").and_then(|v| v.str().map(str::to_string));
    h.check(
        "view_mode_ticks",
        a_preview.as_deref() == Some("preview")
            && a_again.as_deref() == Some("preview")
            && a.snapshot().displayed_mode == ViewMode::Preview
            && state_of(b, "win.view-mode") == before_b,
        format!(
            "A {a_preview:?} then {a_again:?}; B {:?} → {:?}",
            before_b.as_ref().and_then(|v| v.str().map(str::to_string)),
            state_of(b, "win.view-mode").and_then(|v| v.str().map(str::to_string))
        ),
    );
    h.check(
        "undo_off_without_an_editor",
        !enabled_in(a, "win.undo") && !a.snapshot().editor_visible,
        "Preview: no editor on screen, so Ctrl+Z goes to the focused widget",
    );
    h.screenshot("preview-mode").await;
    activate(a, "view-mode", Some("split".to_variant()));
    h.sleep(BETWEEN).await;

    // Zen: a check row; the chrome goes and comes back. Full screen itself is the window
    // manager's to grant, and Xvfb runs none (a real-desktop check).
    activate(a, "zen-mode", None);
    h.sleep(Duration::from_millis(500)).await;
    let on = state_of(a, "win.zen-mode").and_then(|v| v.get::<bool>());
    let chrome_hidden = !a.chrome_shown();
    let preview_off = !enabled_in(a, "win.find-next");
    h.screenshot("zen").await;
    activate(a, "zen-mode", None);
    h.sleep(Duration::from_millis(500)).await;
    let off = state_of(a, "win.zen-mode").and_then(|v| v.get::<bool>());
    h.check(
        "zen_ticks",
        on == Some(true) && off == Some(false) && chrome_hidden && a.chrome_shown() && preview_off,
        format!(
            "on {on:?}, chrome hidden {chrome_hidden}; off {off:?}, chrome back {}; full screen requested, not granted without a WM",
            a.chrome_shown()
        ),
    );

    // Typing: app-wide, ticked in every window from the setting.
    let settings = h.shell().settings.clone();
    activate_app(h, "capitalize-sentences", None);
    h.sleep(BETWEEN).await;
    let off_everywhere = !a.snapshot().capitalize_sentences
        && !b.snapshot().capitalize_sentences
        && state_of(a, "app.capitalize-sentences").and_then(|v| v.get::<bool>()) == Some(false);
    let stored = md_logic::settings::TypingSettings::read(settings.as_ref()).capitalize_sentences;
    activate_app(h, "capitalize-sentences", None);
    h.sleep(BETWEEN).await;
    let back = a.snapshot().capitalize_sentences && b.snapshot().capitalize_sentences;
    h.check(
        "typing_ticks_every_window",
        off_everywhere && !stored && back,
        format!("off in both {off_everywhere}, stored {stored}, back {back}"),
    );

    // PDF page size: a radio, app-wide.
    activate_app(h, "pdf-page-size", Some("6x9".to_variant()));
    h.sleep(BETWEEN).await;
    let state = state_of(a, "app.pdf-page-size").and_then(|v| v.str().map(str::to_string));
    let both = a.snapshot().pdf_page_size_id == "6x9" && b.snapshot().pdf_page_size_id == "6x9";
    activate_app(h, "pdf-page-size", Some("6x9".to_variant()));
    h.sleep(BETWEEN).await;
    let kept = state_of(a, "app.pdf-page-size").and_then(|v| v.str().map(str::to_string));
    activate_app(h, "pdf-page-size", Some("a4".to_variant()));
    h.sleep(BETWEEN).await;
    h.check(
        "pdf_page_size_ticks_every_window",
        state.as_deref() == Some("6x9")
            && both
            && kept.as_deref() == Some("6x9")
            && b.snapshot().pdf_page_size_id == "a4",
        format!("state {state:?}, both windows {both}, re-click {kept:?}"),
    );
}

async fn check_window_list(h: &Harness, a: &Rc<DocumentWindow>, b: &Rc<DocumentWindow>) {
    let a_state = state_of(a, "win.activate-window").and_then(|v| v.get::<u64>());
    let b_state = state_of(b, "win.activate-window").and_then(|v| v.get::<u64>());
    let rows = a.snapshot().window_titles;
    activate(a, "activate-window", Some(b.id().0.to_variant()));
    h.sleep(BETWEEN).await;
    activate(a, "activate-window", Some(a.id().0.to_variant()));
    h.sleep(BETWEEN).await;
    let kept = state_of(a, "win.activate-window").and_then(|v| v.get::<u64>());
    h.check(
        "window_list_ticks_itself",
        a_state == Some(a.id().0) && b_state == Some(b.id().0) && kept == Some(a.id().0) && rows.len() == 2,
        format!(
            "A ticks {a_state:?}, B ticks {b_state:?}, after clicks A {kept:?}; rows {:?}",
            rows.iter().map(|r| &r.title).collect::<Vec<_>>()
        ),
    );
}

// ---- dialogs and chords ---------------------------------------------------------------------------

async fn check_dialog_open(h: &Harness, a: &Rc<DocumentWindow>, b: &Rc<DocumentWindow>) {
    let entry = gtk::Entry::new();
    let dialog = adw::AlertDialog::new(Some("Self-test"), Some("The window behind is inert."));
    dialog.add_response("close", "Close");
    dialog.set_extra_child(Some(&entry));
    dialog.present(Some(a.adw_window()));
    h.sleep(Duration::from_millis(400)).await;
    let a_off = !enabled_in(a, "win.save") && !enabled_in(a, "win.view-mode") && a.snapshot().dialog_open;
    let app_on = enabled_in(a, "app.quit") && enabled_in(a, "app.new");
    let b_on = enabled_in(b, "win.save");
    h.screenshot("dialog-open").await;

    if h.input() && h.focus(&a.window()).await {
        // A window chord does nothing behind the dialog; an editing chord reaches the entry.
        let mode_before = a.snapshot().displayed_mode;
        entry.grab_focus();
        h.type_text("abc").await;
        h.key("ctrl+3").await;
        h.key("ctrl+z").await;
        h.sleep(Duration::from_millis(200)).await;
        let text = entry.text().to_string();
        h.check(
            "disabled_chord_reaches_the_focused_widget",
            a.snapshot().displayed_mode == mode_before && text.len() < 3,
            format!(
                "mode {:?} → {:?}; the entry after typing abc and Ctrl+Z: {text:?}",
                mode_before,
                a.snapshot().displayed_mode
            ),
        );
    }

    dialog.close();
    h.sleep(Duration::from_millis(400)).await;
    let a_back = enabled_in(a, "win.save") && !a.snapshot().dialog_open;
    h.check(
        "dialog_open_disables_window_actions",
        a_off && app_on && b_on && a_back,
        format!("A's win.* off {a_off}, app.* on {app_on}, B unaffected {b_on}, back after close {a_back}"),
    );
}

/// The View chords from the window, each firing once (facts §1, risk §16.4).
async fn check_chords(h: &Harness, a: &Rc<DocumentWindow>) {
    if !h.input() || !h.focus(&a.window()).await {
        skip("view_chords", "no input tier");
        return;
    }
    let mut seen = Vec::new();
    for (chord, mode) in [("ctrl+1", ViewMode::Edit), ("ctrl+3", ViewMode::Preview), ("ctrl+2", ViewMode::Split)] {
        h.key(chord).await;
        h.sleep(BETWEEN).await;
        seen.push((chord, a.snapshot().displayed_mode, mode));
    }
    h.check(
        "view_chords",
        seen.iter().all(|(_, got, want)| got == want),
        seen.iter().map(|(c, got, _)| format!("{c} → {got:?}")).collect::<Vec<_>>().join(", "),
    );
    let probe = a.editor();
    probe.set_text("probe");
    if probe.text() != "probe" {
        skip("ctrl_z_in_preview_and_find_field", "blocked by P3's EditorPane stub (no text round trip)");
    } else {
        probe.set_text("");
        // The editor is real: Ctrl+Z in Preview leaves its text alone.
        probe.set_text("one");
        activate(a, "view-mode", Some("preview".to_variant()));
        h.sleep(BETWEEN).await;
        h.key("ctrl+z").await;
        let kept = probe.text() == "one";
        activate(a, "view-mode", Some("split".to_variant()));
        h.sleep(BETWEEN).await;
        h.check("ctrl_z_in_preview_does_nothing", kept, format!("editor text {:?}", probe.text()));
        probe.set_text("");
    }
}

// ---- Quit, drafts, relaunch, hand-off ---------------------------------------------------------------

/// A saved document for the restore check — only when P2's file system is in.
async fn open_saved_document(h: &Harness) -> Option<PathBuf> {
    let shell = h.shell();
    let path = h.out().join("restore-me.md");
    if std::fs::write(&path, "# Restore me\n\nSaved, in Preview.\n").is_err() {
        return None;
    }
    let text = path.to_string_lossy().into_owned();
    let stamp: Option<FileStamp> = shell.files.stamp(&text);
    if !shell.files.file_exists(&text) || stamp.is_none() {
        skip("plain_launch_restores_saved_documents", "blocked by P2's StdFileSystem stub (no file is seen)");
        return None;
    }
    let id = shell.windows.open_path(&text)?;
    h.sleep(Duration::from_millis(500)).await;
    let window = shell.windows.documents().into_iter().find(|w| w.id() == id)?;
    activate(&window, "view-mode", Some("preview".to_variant()));
    h.sleep(BETWEEN).await;
    Some(path)
}

async fn check_quit_keeps_drafts(h: &Harness, saved: Option<&PathBuf>) -> Option<PathBuf> {
    let shell = h.shell();
    let id = shell.windows.open_untitled(Some(DRAFT_TEXT.to_string()), true);
    h.sleep(Duration::from_millis(400)).await;
    let window = shell.windows.documents().into_iter().find(|w| w.id() == id)?;
    let title = window.adw_window().title().map(|t| t.to_string()).unwrap_or_default();
    h.check("example_like_window_is_dirty", title.ends_with(strings::EDITED_SUFFIX), format!("title {title:?}"));

    // Quit's synchronous half — what `app.quit` and `query-end` run before any window goes.
    shell.windows.prepare_quit();
    let dialogs = shell.windows.documents().iter().filter(|w| w.adw_window().visible_dialog().is_some()).count();
    let draft = window.draft_path();
    let on_disk = draft.as_ref().and_then(|p| std::fs::read_to_string(p).ok());
    let private = draft.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode() & 0o777
    });
    let session = std::fs::read_to_string(shell.paths.session_file()).unwrap_or_default();
    let named = draft.as_ref().is_some_and(|p| session.contains(&*p.to_string_lossy()));
    let saved_named = saved.is_none_or(|p| session.contains(&*p.to_string_lossy()));
    let clean_listed = session.matches("\"path\"").count();
    h.check(
        "quit_keeps_drafts",
        dialogs == 0 && on_disk.as_deref() == Some(DRAFT_TEXT) && private == Some(0o600) && named && saved_named,
        format!(
            "{dialogs} dialog(s); draft {} ({}), text kept {}; session.json names it {named}, {clean_listed} row(s)",
            draft.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            private.map(|m| format!("{m:o}")).unwrap_or_default(),
            on_disk.as_deref() == Some(DRAFT_TEXT),
        ),
    );
    draft
}

/// A second `md` on the same profile, reporting through `MD_SELFTEST_PROBE`.
struct Child {
    process: std::process::Child,
    probe: PathBuf,
}

impl Child {
    fn spawn(h: &Harness, name: &str, args: &[&str]) -> Option<Child> {
        let probe = h.out().join(format!("probe-{name}.json"));
        let _ = std::fs::remove_file(&probe);
        let log = std::fs::File::create(h.out().join(format!("child-{name}.log"))).ok()?;
        let process = h
            .md_command()
            .args(args)
            .env("MD_SELFTEST_PROBE", &probe)
            .stdout(log.try_clone().ok()?)
            .stderr(log)
            .spawn()
            .ok()?;
        Some(Child { process, probe })
    }

    fn report(&self) -> String {
        std::fs::read_to_string(&self.probe).unwrap_or_default()
    }

    async fn wait_report(&self, h: &Harness, limit: Duration, wanted: impl Fn(&str) -> bool) -> String {
        h.wait_until(limit, || wanted(&self.report())).await;
        self.report()
    }

    async fn wait_exit(&mut self, h: &Harness, limit: Duration) -> Option<std::process::ExitStatus> {
        let start = Instant::now();
        loop {
            if let Ok(Some(status)) = self.process.try_wait() {
                return Some(status);
            }
            if start.elapsed() >= limit {
                return None;
            }
            h.sleep(Duration::from_millis(100)).await;
        }
    }

    fn kill(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// A quick second process that hands its arguments to the running primary and exits.
async fn hand_off(h: &Harness, name: &str, args: &[&str]) -> (Option<std::process::ExitStatus>, Duration) {
    let start = Instant::now();
    let Some(mut child) = Child::spawn(h, name, args) else { return (None, start.elapsed()) };
    let status = child.wait_exit(h, Duration::from_secs(10)).await;
    if status.is_none() {
        child.kill();
    }
    (status, start.elapsed())
}

async fn check_relaunch(h: &Harness, draft: &Path, saved: Option<&PathBuf>) {
    let Some(mut primary) = Child::spawn(h, "relaunch", &[]) else {
        h.check("relaunch_restores_drafts", false, "could not start a second md");
        return;
    };
    let needle = "Привет — 😀 kept across a relaunch.";
    let report = primary.wait_report(h, Duration::from_secs(30), |r| r.contains(needle)).await;
    let edited = report.contains(&format!("{}\"", strings::EDITED_SUFFIX));
    let same_draft = report.contains(&*draft.to_string_lossy());
    h.check(
        "relaunch_restores_drafts",
        report.contains(needle) && edited && same_draft,
        format!("dirty {edited}, same draft file {same_draft}; probe: {}", report.trim()),
    );
    h.screenshot("relaunch").await;
    if let Some(saved) = saved {
        let restored = report.contains(&*saved.to_string_lossy()) && report.contains("\"mode\": \"preview\"");
        h.check("plain_launch_restores_saved_documents", restored, "the saved document comes back in Preview");
    }

    // The second-launch hand-off over the session bus: a plain `md` presents the primary's window;
    // `md path` opens the path there (twice: the second time it focuses the window that has it).
    let (status, took) = hand_off(h, "second-launch", &[]).await;
    let report = primary.wait_report(h, Duration::from_secs(10), |r| r.contains("PresentMostRecent")).await;
    h.check(
        "second_launch_presents_the_window",
        status.is_some_and(|s| s.success()) && report.contains("PresentMostRecent"),
        format!("the second md exited {status:?} after {} ms", took.as_millis()),
    );
    // The `.desktop` file's New Window action: `md --new-window` asks the primary for `app.new`.
    let windows_before = report.matches("\"title\"").count();
    let (status, _) = hand_off(h, "new-window", &["--new-window"]).await;
    let report = primary.wait_report(h, Duration::from_secs(10), |r| r.contains("app.New")).await;
    h.check(
        "new_window_hands_over",
        status.is_some_and(|s| s.success())
            && report.contains("app.New")
            && report.matches("\"title\"").count() == windows_before + 1,
        format!(
            "exited {status:?}; the primary had {windows_before} window(s), now {}",
            report.matches("\"title\"").count()
        ),
    );

    let path = h.out().join("hand-off.md");
    let _ = std::fs::write(&path, "# Handed off\n");
    let path_text = path.to_string_lossy().into_owned();
    let (status, took) = hand_off(h, "open-path", &[&path_text]).await;
    let report = primary.wait_report(h, Duration::from_secs(10), |r| r.contains("OpenPath")).await;
    h.check(
        "second_launch_hands_over_its_file",
        status.is_some_and(|s| s.success()) && report.contains(&format!("OpenPath(\\\"{path_text}\\\")")),
        format!("exited {status:?} after {} ms", took.as_millis()),
    );
    if report.contains(&format!("\"path\": \"{path_text}\"")) {
        let (status, _) = hand_off(h, "open-path-again", &[&path_text]).await;
        let report = primary.wait_report(h, Duration::from_secs(10), |r| r.contains("Focus(")).await;
        h.check(
            "second_open_focuses_the_window",
            status.is_some_and(|s| s.success()) && report.contains("Focus("),
            "Focus",
        );
    } else {
        skip(
            "second_open_focuses_the_window",
            "blocked by P2's StdFileSystem stub (the handed-over file did not open)",
        );
    }

    // Quit in the primary: Ctrl+Q asks nothing, and the session still names the draft.
    let mut quit_ok = false;
    if h.input() {
        let pid = primary.process.id().to_string();
        let (_, ids) = h.xdotool(&["search", "--onlyvisible", "--pid", &pid]).await;
        if let Some(xid) = ids.lines().map(str::trim).rfind(|l| !l.is_empty()) {
            h.xdotool(&["windowfocus", "--sync", xid]).await;
            h.sleep(Duration::from_millis(200)).await;
            h.key("ctrl+q").await;
            let status = primary.wait_exit(h, Duration::from_secs(20)).await;
            let session = std::fs::read_to_string(h.shell().paths.session_file()).unwrap_or_default();
            quit_ok =
                status.is_some_and(|s| s.success()) && session.contains(&*draft.to_string_lossy()) && draft.is_file();
            h.check(
                "ctrl_q_keeps_the_draft",
                quit_ok,
                format!("exit {status:?}; session names the draft {}", session.contains(&*draft.to_string_lossy())),
            );
        }
    }
    if !quit_ok {
        primary.kill();
    }
}

async fn check_version(h: &Harness) {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("md"));
    let (ok, output) = h.run(&[&exe.to_string_lossy(), "--version"]).await;
    let wanted = format!("{} {}", strings::APP_NAME, env!("CARGO_PKG_VERSION"));
    h.check("version", ok && output.trim() == wanted, format!("{:?}", output.trim()));
}

async fn check_about(h: &Harness) {
    activate_app(h, "about", None);
    h.sleep(Duration::from_millis(600)).await;
    // About hangs on the focused md window (the most recently focused when none is).
    let dialog = h.shell().windows.documents().iter().find_map(|w| w.adw_window().visible_dialog());
    let about = dialog.as_ref().and_then(|d| d.downcast_ref::<adw::AboutDialog>().cloned());
    let fields = about.as_ref().map(|a| {
        (a.application_name().to_string(), a.version().to_string(), a.license_type(), a.application_icon().to_string())
    });
    h.screenshot("about").await;
    h.check(
        "about",
        fields.as_ref().is_some_and(|(name, version, license, icon)| {
            name == strings::APP_NAME
                && version == env!("CARGO_PKG_VERSION")
                && *license == gtk::License::MitX11
                && icon == crate::app::APP_ID
        }),
        format!("{fields:?}"),
    );
    if let Some(dialog) = dialog {
        dialog.close();
        h.sleep(Duration::from_millis(400)).await;
    }
}

// ---- closing ----------------------------------------------------------------------------------------

async fn check_close_policy(h: &Harness, clean: &Rc<DocumentWindow>, draft: Option<&PathBuf>) {
    let shell = h.shell();
    // An untitled window that was never edited closes without a word.
    let id = clean.id();
    clean.adw_window().close();
    let gone = h.wait_until(Duration::from_secs(5), || shell.windows.host(id).is_none()).await;
    let row = shell.registry.windows().iter().any(|(w, _)| *w == id);
    h.check("clean_untitled_closes_silently", gone && !row, format!("closed {gone}, still in the Window list {row}"));

    // A dirty untitled window asks; Cancel (the dialog's close response) keeps it and its draft.
    let Some(dirty) = shell.windows.documents().into_iter().find(|w| w.draft_path().is_some()) else { return };
    dirty.adw_window().close();
    h.sleep(Duration::from_millis(500)).await;
    let asked = dirty.adw_window().visible_dialog();
    if let Some(dialog) = &asked {
        h.screenshot("save-changes").await;
        dialog.close();
        h.sleep(Duration::from_millis(400)).await;
    }
    let still_open = shell.windows.host(dirty.id()).is_some();
    let kept = draft.is_none_or(|p| p.is_file());
    h.check(
        "dirty_untitled_close_cancelled_keeps_it",
        still_open && kept,
        format!(
            "asked {}; still open {still_open}, draft kept {kept}{}",
            asked.is_some(),
            if asked.is_none() { " (P2's AdwAlerts stub answers Cancel at once)" } else { "" }
        ),
    );
}
