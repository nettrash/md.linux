//! The `editor` self-test group (shell-design §13.3): the keystroke tier — capitals, the override,
//! the wrap and their undo steps; Enter continuation; Shift+Enter; Tab; the format chords; find,
//! replace, Replace All; the placeholder; Split ↔ stacked at 640 px; Zen geometry and the capsule;
//! the footer.
//!
//! The group builds its own window — `ArticlePanes`, `Footer`, `DocumentBar`, `ZenCapsule` wired
//! the way the document window wires them (shell-design §5, §7): the chords as window-level
//! capture-phase shortcuts from md-logic's command table (GTK runs the application's accelerators
//! exactly there), the editing chords dispatched to the focused widget, the session's echo of every
//! edit back into `set_text`, the derived text through md-logic's `DerivedTextScheduler`. Every key
//! is a real XTEST key press from `xdotool` into the real `GtkTextView`; every expectation that is
//! a rule (a capital, an Enter edit, a Format edit) is asked of md-logic / md-core and compared with
//! what the buffer holds, so the checks prove the wiring, not a second copy of the rules.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use md_core::document::view_mode::ViewMode;
use md_core::text::utf16::utf16_len;
use md_logic::commands::{CommandId, KeyModifiers, command_table};
use md_logic::exec::{Spawner, ThreadOffThread};
use md_logic::preview::{EditorJump, JumpId};
use md_logic::seams::{OffThread, Scheduler, WordCounter};
use md_logic::settings::{SettingsStore, settings_keys};
use md_logic::text::{formatting_adapter, smart_typing_adapter};
use md_logic::view::{DerivedTextScheduler, DocumentWindowState, PaneLayout, ZenController};

use crate::editor::article_panes::{ArticlePanes, PanesRole};
use crate::editor::document_bar::DocumentBar;
use crate::editor::editor_pane::EditorPane;
use crate::editor::find_bar::FindBar;
use crate::editor::footer::Footer;
use crate::editor::zen::{self, ZenCapsule};
use crate::selftest::Harness;
use crate::web::preview_host::PreviewHost;

const TITLE: &str = "md editor self-test";
const WIDTH: i32 = 1100;
const HEIGHT: i32 = 760;

/// The test window: what a document window holds, wired as it wires it.
struct Rig {
    window: gtk::Window,
    panes: Rc<ArticlePanes>,
    editor: Rc<EditorPane>,
    find: Rc<FindBar>,
    footer: Rc<Footer>,
    bar: Rc<DocumentBar>,
    state: Rc<DocumentWindowState>,
    zen: Rc<ZenController>,
    capsule: Rc<ZenCapsule>,
    derived: Rc<DerivedTextScheduler>,
    /// What the session last received through `text_edited`.
    session: Rc<RefCell<String>>,
    edits: Rc<Cell<usize>>,
    /// `history_changed` notifications (can-undo / can-redo).
    history: Rc<Cell<usize>>,
    /// `FindBar::changed` notifications.
    find_changes: Rc<Cell<usize>>,
    layouts: Rc<RefCell<Vec<PaneLayout>>>,
    /// A focusable button outside the editor: where a menu click leaves the focus.
    probe: gtk::Button,
    preview: Rc<PreviewHost>,
}

pub async fn run(h: &Harness) {
    let shell = h.shell();
    let rig = build(h);
    rig.window.present();
    let mapped = h.wait_until(Duration::from_secs(10), || rig.window.is_mapped()).await;
    h.check("window", mapped, format!("{TITLE} mapped at {WIDTH}×{HEIGHT}"));
    h.next_frame(&rig.window).await;
    h.sleep(Duration::from_millis(300)).await;

    placeholder(h, &rig).await;
    footer(h, &rig).await;
    document_bar(h, &rig);
    jumps(h, &rig).await;
    scroll_sync(h, &rig).await;
    layout(h, &rig).await;
    book_article(h);

    if !h.input() {
        h.check("input_tier", true, "skipped (--no-input)");
    } else if h.focus(&rig.window).await {
        rig.editor.focus();
        h.sleep(Duration::from_millis(150)).await;
        let focused = rig.editor.text_view().has_focus();
        h.check("focus", focused, "the editor holds the keyboard focus");
        capitals(h, &rig).await;
        scenarios(h, &rig, &shell.settings).await;
        wrap(h, &rig, &shell.settings).await;
        enter_keys(h, &rig).await;
        format_chords(h, &rig).await;
        clipboard(h, &rig).await;
        find(h, &rig).await;
        composition(h, &rig).await;
        zen_mode(h, &rig).await;
    } else {
        h.check("focus", false, "xdotool could not focus the test window");
    }

    restore_typing_defaults(&shell.settings);
    rig.window.close();
}

// ── the rig ──────────────────────────────────────────────────────────────────────────────────

fn build(h: &Harness) -> Rig {
    let shell = h.shell();
    let panes = ArticlePanes::new(&shell, PanesRole::Document);
    let editor = panes.editor();
    let find = panes.find_bar();
    let preview = PreviewHost::new(&shell.web);
    panes.set_preview(&preview);
    let footer = Footer::new();
    let bar = DocumentBar::new();

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let probe = gtk::Button::builder().label("probe").halign(gtk::Align::Start).build();
    column.append(&probe);
    column.append(&panes.widget());
    column.append(&bar.widget());
    column.append(&footer.widget());
    let overlay = gtk::Overlay::builder().child(&column).build();
    let window =
        gtk::Window::builder().title(TITLE).default_width(WIDTH).default_height(HEIGHT).child(&overlay).build();

    let state = Rc::new(DocumentWindowState::new());
    let scheduler: Rc<dyn Scheduler> = shell.scheduler.clone();
    let zen = ZenController::new(Rc::clone(&state), Some(Rc::clone(&scheduler)));
    let capsule = ZenCapsule::new(&overlay, Rc::clone(&zen));
    let spawner: Rc<dyn Spawner> = shell.spawner.clone();
    let off_thread: Rc<dyn OffThread> = Rc::new(ThreadOffThread);
    let words: Arc<dyn WordCounter> = Arc::clone(&shell.words);
    let derived = DerivedTextScheduler::new(scheduler, spawner, off_thread, words);

    let rig = Rig {
        window,
        panes,
        editor,
        find,
        footer,
        bar,
        state,
        zen,
        capsule,
        derived,
        session: Rc::new(RefCell::new(String::new())),
        edits: Rc::new(Cell::new(0)),
        history: Rc::new(Cell::new(0)),
        find_changes: Rc::new(Cell::new(0)),
        layouts: Rc::new(RefCell::new(Vec::new())),
        probe,
        preview,
    };
    wire(&rig);
    rig
}

/// The document window's wiring, as far as these widgets need it (shell-design §5, §7).
fn wire(rig: &Rig) {
    // The session: every edit reaches it, and it echoes the text back (identical text: no-op).
    let (session, edits, derived) = (Rc::clone(&rig.session), Rc::clone(&rig.edits), Rc::clone(&rig.derived));
    let echo = Rc::downgrade(&rig.editor);
    rig.editor.text_edited().connect(move |text| {
        *session.borrow_mut() = text.to_string();
        edits.set(edits.get() + 1);
        derived.text_changed(text);
        if let Some(editor) = echo.upgrade() {
            editor.set_text(text);
        }
    });
    let footer = Rc::clone(&rig.footer);
    rig.derived.changed().connect(move |d| footer.update(d));
    let history = Rc::clone(&rig.history);
    rig.editor.history_changed().connect(move |_| history.set(history.get() + 1));
    let find_changes = Rc::clone(&rig.find_changes);
    rig.find.changed().connect(move |_| find_changes.set(find_changes.get() + 1));
    let layouts = Rc::clone(&rig.layouts);
    rig.panes.layout_changed().connect(move |l| layouts.borrow_mut().push(*l));

    // Zen: the controller asks for full screen; the window reports it back; the state lays out.
    let window = rig.window.clone();
    rig.zen.full_screen_requested().connect(move |full| {
        if *full {
            window.fullscreen();
        } else {
            window.unfullscreen();
        }
    });
    let zen = Rc::clone(&rig.zen);
    rig.window.connect_fullscreened_notify(move |w| zen.presenter_changed(w.is_fullscreen()));
    let (state, panes, capsule, footer) =
        (Rc::clone(&rig.state), Rc::clone(&rig.panes), Rc::clone(&rig.capsule), rig.footer.widget());
    let weak_state = Rc::downgrade(&state);
    rig.state.changed().connect(move |_| {
        let Some(state) = weak_state.upgrade() else { return };
        let zen = state.zen_active().then(|| state.zen_reading());
        panes.set_zen(zen);
        panes.set_mode(state.effective_mode());
        footer.set_visible(zen.is_none());
        capsule.refresh();
    });
    drop(state);

    // The chords: GTK runs an application's accelerators in the window's capture phase.
    let shortcuts = gtk::ShortcutController::new();
    shortcuts.set_propagation_phase(gtk::PropagationPhase::Capture);
    shortcuts.set_scope(gtk::ShortcutScope::Local);
    for &id in formatting_adapter::commands() {
        let editor = Rc::clone(&rig.editor);
        add(&shortcuts, id, move || editor.apply_format(id));
    }
    let find = Rc::clone(&rig.find);
    add(&shortcuts, CommandId::Find, move || find.show_find());
    let find = Rc::clone(&rig.find);
    add(&shortcuts, CommandId::Replace, move || find.show_replace());
    let find = Rc::clone(&rig.find);
    add(&shortcuts, CommandId::FindNext, move || find.find_next(true));
    let find = Rc::clone(&rig.find);
    add(&shortcuts, CommandId::FindPrevious, move || find.find_next(false));
    let find = Rc::clone(&rig.find);
    add(&shortcuts, CommandId::UseSelectionForFind, move || find.use_selection());
    let zen = Rc::clone(&rig.zen);
    add(&shortcuts, CommandId::ZenMode, move || zen.toggle());
    for (id, mode) in [
        (CommandId::ViewEdit, ViewMode::Edit),
        (CommandId::ViewSplit, ViewMode::Split),
        (CommandId::ViewPreview, ViewMode::Preview),
    ] {
        let state = Rc::clone(&rig.state);
        add(&shortcuts, id, move || state.set_stored_mode(mode));
    }
    // The editing chords act on whichever widget has the focus (shell-design §2.2).
    for (id, verb) in [
        (CommandId::Undo, "undo"),
        (CommandId::Redo, "redo"),
        (CommandId::Cut, "cut"),
        (CommandId::Copy, "copy"),
        (CommandId::Paste, "paste"),
        (CommandId::SelectAll, "select-all"),
    ] {
        let (window, editor) = (rig.window.clone(), Rc::clone(&rig.editor));
        add(&shortcuts, id, move || dispatch_to_focus(&window, &editor, verb));
    }
    let window = rig.window.clone();
    add(&shortcuts, CommandId::FullScreen, move || {
        if window.is_fullscreen() {
            window.unfullscreen();
        } else {
            window.fullscreen();
        }
    });
    rig.window.add_controller(shortcuts);

    // Escape is the window's own key, bubble phase: what nothing below took leaves Zen (§2.2).
    let keys = gtk::EventControllerKey::new();
    let zen = Rc::clone(&rig.zen);
    keys.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gtk::gdk::Key::Escape && zen.is_active() {
            zen.set_active(false);
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    rig.window.add_controller(keys);
}

/// Every accelerator md-logic's table gives `id` (first and second), all running `run`.
fn add(controller: &gtk::ShortcutController, id: CommandId, run: impl Fn() + 'static) {
    let run = Rc::new(run);
    for accelerator in command_table::spec(id).accelerators() {
        let Some(trigger) = gtk::ShortcutTrigger::parse_string(&accelerator) else { continue };
        let run = Rc::clone(&run);
        let action = gtk::CallbackAction::new(move |_, _| {
            run();
            glib::Propagation::Stop
        });
        controller.add_shortcut(gtk::Shortcut::new(Some(trigger), Some(action)));
    }
}

fn dispatch_to_focus(window: &gtk::Window, editor: &Rc<EditorPane>, verb: &str) {
    let focus = gtk::prelude::GtkWindowExt::focus(window);
    if focus.as_ref() == Some(editor.text_view().upcast_ref::<gtk::Widget>()) {
        match verb {
            "undo" => editor.undo(),
            "redo" => editor.redo(),
            "cut" => editor.cut(),
            "copy" => editor.copy(),
            "paste" => editor.paste(),
            _ => editor.select_all(),
        }
    } else if let Some(focus) = focus {
        let action = match verb {
            "undo" | "redo" => format!("text.{verb}"),
            "select-all" => "selection.select-all".to_string(),
            other => format!("clipboard.{other}"),
        };
        let _ = WidgetExt::activate_action(&focus, &action, None);
    }
}

fn restore_typing_defaults(settings: &Rc<crate::services::settings_store::KeyFileSettingsStore>) {
    settings.set_bool(settings_keys::CONTINUE_LISTS, settings_keys::CONTINUE_LISTS_DEFAULT);
    settings.set_bool(settings_keys::CAPITALIZE_SENTENCES, settings_keys::CAPITALIZE_SENTENCES_DEFAULT);
    settings.set_bool(settings_keys::WRAP_SELECTION, settings_keys::WRAP_SELECTION_DEFAULT);
}

// ── helpers ──────────────────────────────────────────────────────────────────────────────────

/// Fresh text, a fresh history, the selection given, the editor focused.
async fn reset(h: &Harness, rig: &Rig, text: &str, start: usize, length: usize) {
    rig.editor.set_text(text);
    rig.editor.reset_history();
    rig.editor.select_utf16(start, length);
    rig.editor.focus();
    rig.editor.typing_hooks().clear_trace();
    h.sleep(Duration::from_millis(60)).await;
}

fn state(editor: &EditorPane) -> (String, (usize, usize)) {
    (editor.text(), editor.selection_utf16())
}

/// Waits (≤ 2 s) for the buffer to hold `want`, then records the check.
async fn expect_text(h: &Harness, rig: &Rig, name: &str, want: &str, context: &str) -> bool {
    let editor = Rc::clone(&rig.editor);
    let want_owned = want.to_string();
    h.wait_until(Duration::from_secs(2), move || editor.text() == want_owned).await;
    let (text, selection) = state(&rig.editor);
    let ok = text == want;
    let mut detail = format!("{context}: {text:?}, selection {selection:?}");
    if !ok {
        detail.push_str(&format!(" — expected {want:?}; trace {:?}", rig.editor.typing_hooks().trace()));
    }
    h.check(name, ok, detail)
}

/// The xdotool spelling of a command's first chord: `ctrl+shift+l`, `ctrl+alt+2`.
fn xdotool_chord(id: CommandId) -> String {
    let chord = command_table::spec(id).chord.expect("a command with a chord");
    let mut keys = Vec::new();
    if chord.modifiers.contains(KeyModifiers::CTRL) {
        keys.push("ctrl");
    }
    if chord.modifiers.contains(KeyModifiers::SHIFT) {
        keys.push("shift");
    }
    if chord.modifiers.contains(KeyModifiers::ALT) {
        keys.push("alt");
    }
    keys.push(chord.key);
    keys.join("+")
}

fn focused_is(rig: &Rig, widget: &impl IsA<gtk::Widget>) -> bool {
    gtk::prelude::GtkWindowExt::focus(&rig.window)
        .is_some_and(|f| f == *widget.upcast_ref::<gtk::Widget>() || f.is_ancestor(widget))
}

// ── no keys ──────────────────────────────────────────────────────────────────────────────────

async fn placeholder(h: &Harness, rig: &Rig) {
    rig.editor.set_text("");
    let empty = rig.editor.placeholder().is_visible();
    let label = rig.editor.placeholder().label().to_string();
    rig.editor.set_text("x");
    let hidden = !rig.editor.placeholder().is_visible();
    rig.editor.set_text("");
    let back = rig.editor.placeholder().is_visible();
    h.check(
        "placeholder",
        empty && hidden && back && label == md_logic::strings::EDITOR_PLACEHOLDER,
        format!("{label:?} shown while empty ({empty}), hidden with text ({hidden}), back when emptied ({back})"),
    );
    h.screenshot("placeholder").await;
}

/// The five md.win word-count vectors, through md-logic's scheduler (off the UI thread) into the
/// footer. An external `set_text` is not an edit, so the window feeds the scheduler itself, as it
/// does when it appears.
async fn footer(h: &Harness, rig: &Rig) {
    const VECTORS: [(&str, usize); 5] =
        [("", 0), ("   \n\n", 0), ("Hello, world!", 2), ("it's \u{2014} done", 2), ("One\ntwo\n\nthree", 3)];
    let mut all = true;
    let mut details = Vec::new();
    for (text, words) in VECTORS {
        rig.editor.set_text(text);
        rig.derived.text_changed(&rig.editor.text());
        let want = md_logic::strings::footer(words, md_core::book::writing_stats::characters(text));
        let footer = Rc::clone(&rig.footer);
        let target = want.clone();
        let ok = h.wait_until(Duration::from_secs(3), move || footer.text() == target).await;
        all &= ok;
        details.push(format!("{text:?} → {:?}{}", rig.footer.text(), if ok { "" } else { " ✗" }));
    }
    h.check("footer_word_counts", all, details.join("; "));
    rig.footer.set_busy(true);
    let busy = rig.footer.is_busy();
    rig.footer.set_busy(false);
    h.check("footer_spinner", busy && !rig.footer.is_busy(), "set_busy shows and hides the spinner");
    rig.editor.set_text("");
}

fn document_bar(h: &Harness, rig: &Rig) {
    use md_logic::strings::{buttons, documents};
    let pressed = Rc::new(Cell::new(0));
    let (a, b) = (Rc::clone(&pressed), Rc::clone(&pressed));
    rig.bar.show_conflict(move || a.set(1), move || b.set(2));
    let conflict = rig.bar.texts();
    let shown = rig.bar.is_shown();
    rig.bar.press(true);
    let reload = pressed.get();
    rig.bar.press(false);
    let keep = pressed.get();
    let ok = shown
        && conflict
            == (
                documents::FILE_CHANGED_ON_DISK.to_string(),
                buttons::RELOAD_FROM_DISK.to_string(),
                buttons::KEEP_MY_VERSION.to_string(),
            )
        && reload == 1
        && keep == 2;
    h.check("bar_conflict", ok, format!("{conflict:?}, buttons → {reload}, {keep}"));

    let message = documents::could_not_save("disk full");
    let (a, b) = (Rc::clone(&pressed), Rc::clone(&pressed));
    rig.bar.show_save_error(&message, move || a.set(3), move || b.set(4));
    let save_error = rig.bar.texts();
    rig.bar.press(false);
    let save_as = pressed.get();
    rig.bar.hide();
    let ok = save_error == (message.clone(), buttons::RETRY.to_string(), buttons::SAVE_AS.to_string())
        && save_as == 4
        && !rig.bar.is_shown();
    h.check("bar_save_error", ok, format!("{save_error:?}, Save As → {save_as}, hidden after"));

    // A book article's save error offers Retry alone.
    let a = Rc::clone(&pressed);
    rig.bar.show_save_error_retry_only(&message, move || a.set(5));
    let retry_only = rig.bar.texts();
    rig.bar.press(true);
    let retried = pressed.get();
    rig.bar.show_conflict(|| {}, || {});
    let both_again = rig.bar.texts().2 == buttons::KEEP_MY_VERSION;
    rig.bar.hide();
    h.check(
        "bar_retry_only",
        retry_only == (message, buttons::RETRY.to_string(), String::new()) && retried == 5 && both_again,
        format!("{retry_only:?}, Retry → {retried}; the next bar shows its second button again: {both_again}"),
    );
}

async fn jumps(h: &Harness, rig: &Rig) {
    rig.editor.set_text("zero\none\ntwo 😀\nthree\n");
    rig.editor.select_utf16(0, 0);
    let jump = EditorJump::new(JumpId::next(), 3);
    rig.editor.apply_jump(jump.clone());
    let first = rig.editor.selection_utf16();
    rig.editor.select_utf16(0, 0);
    rig.editor.apply_jump(jump);
    let again = rig.editor.selection_utf16();
    // Line 3 starts after "zero\none\ntwo 😀\n": 5 + 4 + 7 (the emoji is two units).
    h.check(
        "jump_once_per_id",
        first == (16, 0) && again == (0, 0),
        format!("line 3 → {first:?}; the same id again → {again:?}"),
    );
    rig.editor.set_text("");
}

async fn scroll_sync(h: &Harness, rig: &Rig) {
    let long: String = (0..400).map(|i| format!("Line {i}\n")).collect();
    rig.editor.set_text(&long);
    h.sleep(Duration::from_millis(300)).await;
    let reported = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&reported);
    let id = rig.editor.scrolled().connect(move |f| sink.borrow_mut().push(*f));
    // The preview reports half way (its `mdScroll` message, as `PreviewHost::did_scroll`): through
    // the link `set_preview` made, the editor goes there and does not echo it back.
    rig.preview.did_scroll().emit(&0.5);
    h.sleep(Duration::from_millis(100)).await;
    let echoed = reported.borrow().len();
    let adjustment = rig.editor.text_view().vadjustment().expect("a scrollable text view");
    let applied = adjustment.value() / (adjustment.upper() - adjustment.page_size());
    // A scroll of the editor's own is reported as a fraction.
    adjustment.set_value((adjustment.upper() - adjustment.page_size()) * 0.25);
    h.sleep(Duration::from_millis(100)).await;
    let own = reported.borrow().last().copied();
    rig.editor.scrolled().disconnect(id);
    // Both halves are registered on the link the window shares with the preview.
    let link = format!("{:?}", rig.panes.scroll_sync());
    let both = link.contains("editor: true") && link.contains("preview: true");
    let ok = (applied - 0.5).abs() < 0.01 && echoed == 0 && own.is_some_and(|f| (f - 0.25).abs() < 0.01) && both;
    h.check(
        "scroll_sync",
        ok,
        format!(
            "preview 0.5 → editor at {applied:.3}, {echoed} echo(es); editor scrolled to 0.25 → reported {own:?}; {link}"
        ),
    );
    rig.editor.set_text("");
}

async fn layout(h: &Harness, rig: &Rig) {
    rig.editor.set_text("# Layout\n\nSome text.");
    let xid = h.xid(&rig.window).await;
    // xdotool sizes X windows in device pixels; GTK lays out in logical ones (GDK_SCALE=2 runs).
    let scale = rig.window.scale_factor().max(1);
    let resize = |w: i32| {
        let xid = xid.clone();
        async move {
            match xid {
                Some(xid) => {
                    let (width, height) = ((w * scale).to_string(), (HEIGHT * scale).to_string());
                    h.xdotool(&["windowsize", "--sync", &xid, &width, &height]).await.0
                }
                None => false,
            }
        }
    };
    let measure = |rig: &Rig| {
        let (column, preview) = rig.panes.columns();
        (column.width(), column.height(), preview.width(), preview.height())
    };
    rig.state.set_stored_mode(ViewMode::Split);
    let mut rows = Vec::new();
    let mut all = true;
    for (width, want) in [(1000, PaneLayout::SideBySide), (600, PaneLayout::Stacked), (700, PaneLayout::SideBySide)] {
        let resized = resize(width).await;
        let panes = Rc::clone(&rig.panes);
        let ok = h.wait_until(Duration::from_secs(3), move || panes.layout() == want).await;
        h.next_frame(&rig.window).await;
        let (cw, ch, pw, ph) = measure(rig);
        let halves = match want {
            PaneLayout::SideBySide => (cw - pw).abs() <= 2 && (ch - ph).abs() <= 2,
            _ => (ch - ph).abs() <= 2 && (cw - pw).abs() <= 2,
        };
        all &= resized && ok && halves;
        rows.push(format!(
            "{width} px → {:?} (editor {cw}×{ch}, preview {pw}×{ph}){}",
            rig.panes.layout(),
            if ok && halves { "" } else { " ✗" }
        ));
        if width == 600 {
            h.screenshot("split-stacked").await;
        } else if width == 1000 {
            h.screenshot("split-side-by-side").await;
        }
    }
    h.check("split_breakpoint_640", all, rows.join("; "));

    rig.state.set_stored_mode(ViewMode::Edit);
    let edit = (rig.panes.layout(), rig.panes.columns().1.is_visible());
    rig.state.set_stored_mode(ViewMode::Preview);
    let preview = (rig.panes.layout(), rig.panes.columns().0.is_visible());
    rig.state.set_stored_mode(ViewMode::Split);
    let split = rig.panes.layout();
    let reported = rig.layouts.borrow().contains(&PaneLayout::EditorOnly);
    h.check(
        "modes",
        edit == (PaneLayout::EditorOnly, false) && preview == (PaneLayout::PreviewOnly, false) && reported,
        format!(
            "Edit → {edit:?}; Preview → {preview:?}; Split → {split:?}; layout_changed saw {:?}",
            rig.layouts.borrow()
        ),
    );
    let _ = resize(WIDTH).await;
    rig.editor.set_text("");
}

/// The Book window's article panes carry the find bar too (the Mac's 1.5; md.win's had none).
fn book_article(h: &Harness) {
    let panes = ArticlePanes::new(&h.shell(), PanesRole::BookArticle);
    let has_find = panes.find_bar().widget().is_ancestor(&panes.widget());
    h.check(
        "book_article_panes",
        panes.role() == PanesRole::BookArticle && has_find,
        format!("a book article's panes ({:?}) hold the find bar: {has_find}", panes.role()),
    );
}

// ── keys: capitals and their undo steps ──────────────────────────────────────────────────────

async fn capitals(h: &Harness, rig: &Rig) {
    reset(h, rig, "", 0, 0).await;
    h.type_text("md").await;
    expect_text(h, rig, "capital_md", "Md", "typed `md` into an empty document").await;
    let session = rig.session.borrow().clone();
    h.check("capital_reaches_the_session", session == "Md", format!("text_edited last carried {session:?}"));

    h.key("BackSpace").await;
    h.key("BackSpace").await;
    h.type_text("md").await;
    expect_text(h, rig, "retype_stays_lowercase", "md", "Backspace ×2 over `Md`, then `md` again").await;

    // The capital is its own undo step (smart-typing.md §3.5; risk §16.3).
    reset(h, rig, "", 0, 0).await;
    h.type_text("m").await;
    expect_text(h, rig, "capital_m", "M", "typed `m`").await;
    h.key("ctrl+z").await;
    let one = expect_text(h, rig, "capital_one_undo", "m", "one Ctrl+Z after the capital").await;
    let caret = rig.editor.selection_utf16();
    h.check("capital_undo_caret", caret == (1, 0), format!("the caret after the restored letter: {caret:?}"));
    h.key("ctrl+z").await;
    expect_text(h, rig, "capital_second_undo", "", "a second Ctrl+Z removes the typed letter").await;
    h.key("shift+ctrl+z").await;
    h.key("shift+ctrl+z").await;
    expect_text(h, rig, "capital_redo", "M", "Shift+Ctrl+Z twice redoes the letter and its capital").await;
    let _ = one;

    // After Ctrl+Z the next letter is typed on, not over (the caret was collapsed).
    reset(h, rig, "", 0, 0).await;
    h.type_text("m").await;
    h.key("ctrl+z").await;
    h.type_text("d").await;
    expect_text(h, rig, "undo_then_type_on", "md", "`m`, Ctrl+Z, `d`").await;

    // A capital after a sentence and on a new line.
    reset(h, rig, "One. ", 5, 0).await;
    h.type_text("two").await;
    expect_text(h, rig, "capital_after_sentence", "One. Two", "`two` after \"One. \"").await;
    let undo_state = (rig.editor.can_undo(), rig.editor.can_redo());
    let notified = rig.history.get();
    h.check(
        "can_undo",
        undo_state == (true, false) && notified > 0,
        format!("can undo / redo after typing: {undo_state:?}; history_changed raised {notified} times"),
    );
}

/// md.win's `SelfTestTyping.Scenarios` (md-logic's `text_typing_hooks` test data), played on the
/// real view with the two switches set through the settings store, live.
async fn scenarios(h: &Harness, rig: &Rig, settings: &Rc<crate::services::settings_store::KeyFileSettingsStore>) {
    enum K {
        T(&'static str),
        Enter,
        Back,
        Undo,
    }
    use K::*;
    let table: [(&str, &str, usize, bool, bool, &[K], &str); 14] = [
        ("enter.continuesABullet", "", 0, true, false, &[T("- a"), Enter], "- a\n- "),
        ("enter.countsANumberedItemOn", "", 0, true, false, &[T("1. a"), Enter], "1. a\n2. "),
        ("enter.onAnEmptyItemEndsTheList", "", 0, true, false, &[T("- a"), Enter, Enter], "- a\n"),
        ("enter.switchOffIsAPlainNewline", "", 0, false, false, &[T("- a"), Enter], "- a\n"),
        ("capital.firstLetterOfTheDocument", "", 0, false, true, &[T("hello")], "Hello"),
        ("capital.firstLetterOfALine", "One\n", 4, false, true, &[T("two")], "One\nTwo"),
        ("capital.afterASentenceEnds", "One. ", 5, false, true, &[T("two")], "One. Two"),
        ("capital.overrideDeleteAndRetypeStaysLowercase", "", 0, false, true, &[T("m"), Back, T("md")], "md"),
        ("capital.ctrlZAfterACapitalRestoresTheLowercaseLetter", "", 0, false, true, &[T("m"), Undo], "m"),
        (
            "capital.ctrlZThenANewLineStillCapitalizes",
            "",
            0,
            false,
            true,
            &[T("m"), Undo, Enter, T("hello")],
            "m\nHello",
        ),
        ("capital.ctrlZThenTypingOnKeepsTheLetter", "", 0, false, true, &[T("m"), Undo, T("d")], "md"),
        ("capital.aCapitalTypedLastIsInTheText", "One. ", 5, false, true, &[T("t")], "One. T"),
        ("capital.switchOffLeavesTheLetterAlone", "", 0, false, false, &[T("hello")], "hello"),
        ("both.aListItemsFirstLetter", "", 0, true, true, &[T("- one"), Enter, T("two")], "- One\n- Two"),
    ];
    let mut failed = Vec::new();
    for (name, initial, caret, lists, capitals, keys, want) in table {
        settings.set_bool(settings_keys::CONTINUE_LISTS, lists);
        settings.set_bool(settings_keys::CAPITALIZE_SENTENCES, capitals);
        reset(h, rig, initial, caret, 0).await;
        for key in keys {
            match key {
                T(text) => {
                    h.type_text(text).await;
                }
                Enter => {
                    h.key("Return").await;
                }
                Back => {
                    h.key("BackSpace").await;
                }
                Undo => {
                    h.key("ctrl+z").await;
                }
            }
        }
        let editor = Rc::clone(&rig.editor);
        h.wait_until(Duration::from_secs(1), move || editor.text() == want).await;
        let got = rig.editor.text();
        if got != want {
            failed.push(format!("{name}: {got:?} ≠ {want:?}"));
        }
    }
    restore_typing_defaults(settings);
    let ok = failed.is_empty();
    h.check("scenarios", ok, if ok { format!("all {} md.win scenarios", table.len()) } else { failed.join("; ") });
}

// ── keys: the typed wrap ─────────────────────────────────────────────────────────────────────

async fn wrap(h: &Harness, rig: &Rig, settings: &Rc<crate::services::settings_store::KeyFileSettingsStore>) {
    reset(h, rig, "a word b", 2, 4).await;
    h.type_text("*").await;
    expect_text(h, rig, "wrap_star", "a *word* b", "`*` typed over the selected `word`").await;
    let selection = rig.editor.selection_utf16();
    let selected = (rig.editor.has_selection(), rig.editor.selected_text());
    h.check(
        "wrap_selects_the_word",
        selection == (3, 4) && selected == (true, "word".to_string()),
        format!("selection after the wrap: {selection:?} {selected:?}"),
    );

    // Undo, as on the Mac and md.win: the wrap is its own step, then the typed replacement is one
    // more (GTK 4.22 splits a typed replacement of more than one character into a deletion and an
    // insertion; the pane bridges them into one undo step).
    let mut steps = vec![rig.editor.text()];
    for _ in 0..2 {
        h.key("ctrl+z").await;
        h.sleep(Duration::from_millis(60)).await;
        steps.push(rig.editor.text());
    }
    let restored = rig.editor.selection_utf16();
    let ok = steps[1] == "a * b" && steps[2] == "a word b" && restored == (2, 4);
    h.check(
        "wrap_one_undo",
        ok,
        format!(
            "Ctrl+Z ×2: {steps:?}; the word comes back selected {restored:?} — the first Ctrl+Z undoes the wrap alone, \
             the second the typed `*`"
        ),
    );
    // Redo walks the same two steps forward.
    for _ in 0..2 {
        h.key("ctrl+shift+z").await;
        h.sleep(Duration::from_millis(60)).await;
    }
    let redone = rig.editor.text();
    h.check("wrap_redo", redone == "a *word* b", format!("Shift+Ctrl+Z ×2: {redone:?}"));

    for (mark, want) in [("_", "a _word_ b"), ("`", "a `word` b"), ("$", "a $word$ b")] {
        reset(h, rig, "a word b", 2, 4).await;
        h.type_text(mark).await;
        expect_text(h, rig, &format!("wrap_{}", mark_name(mark)), want, &format!("`{mark}` over `word`")).await;
    }

    settings.set_bool(settings_keys::WRAP_SELECTION, false);
    reset(h, rig, "a word b", 2, 4).await;
    h.type_text("*").await;
    expect_text(h, rig, "wrap_switched_off", "a * b", "md.wrapSelection off: `*` replaces the selection").await;
    settings.set_bool(settings_keys::WRAP_SELECTION, true);

    // The retype rule over a one-scalar selection of md's own capital.
    reset(h, rig, "", 0, 0).await;
    h.type_text("md").await;
    rig.editor.select_utf16(0, 1);
    h.type_text("m").await;
    expect_text(h, rig, "retype_over_selected_capital", "md", "`M` selected and `m` typed over it").await;
}

fn mark_name(mark: &str) -> &'static str {
    match mark {
        "_" => "underscore",
        "`" => "backtick",
        _ => "dollar",
    }
}

// ── keys: Enter, Shift+Enter, Ctrl+Enter, Tab ────────────────────────────────────────────────

async fn enter_keys(h: &Harness, rig: &Rig) {
    // A table row continues exactly as md-core's rule says.
    let row = "| a | b |";
    let want = smart_typing_adapter::enter(row, utf16_len(row) as isize, 0)
        .map(|e| {
            let units: Vec<u16> = row.encode_utf16().collect();
            let mut out: Vec<u16> = units[..e.location].to_vec();
            out.extend(e.replacement.encode_utf16());
            out.extend_from_slice(&units[e.location + e.length..]);
            String::from_utf16_lossy(&out)
        })
        .unwrap_or_else(|| format!("{row}\n"));
    reset(h, rig, row, utf16_len(row), 0).await;
    h.key("Return").await;
    expect_text(h, rig, "enter_table_row", &want, "Enter at the end of a table row").await;
    h.key("ctrl+z").await;
    expect_text(h, rig, "enter_one_undo", row, "one Ctrl+Z after the continuation").await;

    reset(h, rig, "- a", 3, 0).await;
    h.key("shift+Return").await;
    expect_text(h, rig, "shift_enter_plain", "- a\n", "Shift+Enter after a list item").await;
    reset(h, rig, "- a", 3, 0).await;
    h.key("ctrl+Return").await;
    expect_text(h, rig, "ctrl_enter_plain", "- a\n", "Ctrl+Enter after a list item").await;

    reset(h, rig, "x", 1, 0).await;
    h.key("Tab").await;
    expect_text(h, rig, "tab_inserts_a_tab", "x\t", "Tab after `x`").await;
    let focused = rig.editor.text_view().has_focus();
    h.check("tab_keeps_focus", focused, "the focus stays in the editor after Tab");
}

// ── keys: every Format command by its chord, one undo each ───────────────────────────────────

async fn format_chords(h: &Harness, rig: &Rig) {
    let mut failed = Vec::new();
    let mut done = 0;
    for &id in formatting_adapter::commands() {
        let (text, start, length) = match id {
            CommandId::Bold
            | CommandId::Italic
            | CommandId::InlineCode
            | CommandId::Strikethrough
            | CommandId::Link => ("one two three", 4, 3),
            CommandId::Outdent => ("\tone\ntwo", 2, 0),
            _ => ("one\ntwo", 5, 0),
        };
        let op = formatting_adapter::op_for(id).expect("a Format command");
        let edit = formatting_adapter::edit(text, start as isize, length as isize, op);
        let Some(edit) = edit else {
            failed.push(format!("{id:?}: md-core makes no edit of the sample"));
            continue;
        };
        let want = edit.apply_to(text);
        let want_selection = (edit.selection_start, edit.selection_end - edit.selection_start);
        reset(h, rig, text, start, length).await;
        let chord = xdotool_chord(id);
        h.key(&chord).await;
        let editor = Rc::clone(&rig.editor);
        let target = want.clone();
        h.wait_until(Duration::from_secs(1), move || editor.text() == target).await;
        let (got, selection) = state(&rig.editor);
        h.key("ctrl+z").await;
        let editor = Rc::clone(&rig.editor);
        h.wait_until(Duration::from_secs(1), move || editor.text() == text).await;
        let undone = rig.editor.text();
        if got != want || selection != want_selection || undone != text {
            failed.push(format!(
                "{id:?} ({chord}): {got:?} sel {selection:?} (want {want:?} sel {want_selection:?}), Ctrl+Z → {undone:?}"
            ));
        }
        done += 1;
    }
    let ok = failed.is_empty() && done == formatting_adapter::commands().len();
    h.check(
        "format_chords",
        ok,
        if ok {
            format!("{done} Format commands, each by its chord and undone by one Ctrl+Z")
        } else {
            failed.join("; ")
        },
    );

    // Bold at a caret right after typing: an insertion GtkTextHistory would chain onto the word.
    reset(h, rig, "", 0, 0).await;
    h.type_text("word").await;
    expect_text(h, rig, "bold_at_caret", "Word", "typed `word`").await;
    h.key("ctrl+b").await;
    expect_text(h, rig, "bold_at_caret_edit", "Word****", "Ctrl+B at the caret").await;
    let caret = rig.editor.selection_utf16();
    h.check("bold_at_caret_inside", caret == (6, 0), format!("the caret between the marks: {caret:?}"));
    h.key("ctrl+z").await;
    expect_text(h, rig, "bold_at_caret_one_undo", "Word", "one Ctrl+Z leaves the typed word").await;

    // A menu row: the focus comes back to the editor (a menu click leaves it elsewhere — here on a
    // plain button, which, like a menu, does not take the PRIMARY selection).
    reset(h, rig, "one two", 4, 3).await;
    rig.probe.grab_focus();
    h.sleep(Duration::from_millis(100)).await;
    let away = !rig.editor.text_view().has_focus();
    rig.editor.apply_format(CommandId::Italic);
    h.sleep(Duration::from_millis(100)).await;
    let back = rig.editor.text_view().has_focus();
    h.check(
        "format_menu_row_refocuses",
        away && back && rig.editor.text() == "one *two*",
        format!("focus away ({away}) → Format ▸ Italic → editor focused ({back}), {:?}", rig.editor.text()),
    );
    h.screenshot("format").await;
}

// ── keys: paste, cut, copy ───────────────────────────────────────────────────────────────────

async fn clipboard(h: &Harness, rig: &Rig) {
    let clipboard = rig.editor.text_view().clipboard();
    reset(h, rig, "", 0, 0).await;
    clipboard.set_text("hello\r\nworld");
    h.sleep(Duration::from_millis(100)).await;
    h.key("ctrl+v").await;
    expect_text(h, rig, "paste_crlf_as_lf_uncapitalized", "hello\nworld", "a CRLF clipboard pasted at the start").await;
    h.type_text(" now").await;
    expect_text(h, rig, "paste_then_typing", "hello\nworld now", "typing after the paste").await;

    reset(h, rig, "one two", 0, 0).await;
    rig.editor.select_all();
    rig.editor.copy();
    rig.editor.cut();
    let cut = rig.editor.text();
    h.sleep(Duration::from_millis(150)).await;
    rig.editor.paste();
    let editor = Rc::clone(&rig.editor);
    h.wait_until(Duration::from_secs(2), move || editor.text() == "one two").await;
    let pasted = rig.editor.text();
    rig.editor.select_utf16(0, 3);
    rig.editor.delete_selection();
    let deleted = rig.editor.text();
    h.check(
        "edit_rows",
        cut.is_empty() && pasted == "one two" && deleted == " two",
        format!("Select All + Copy + Cut → {cut:?}; Paste → {pasted:?}; Delete over `one` → {deleted:?}"),
    );
}

// ── keys: find and replace ───────────────────────────────────────────────────────────────────

async fn find(h: &Harness, rig: &Rig) {
    let text = "cat dog cat bird cat";
    reset(h, rig, text, 0, 0).await;
    h.key(&xdotool_chord(CommandId::Find)).await;
    let changes = rig.find_changes.get();
    let opened = rig.find.is_open() && focused_is(rig, &rig.find.query_entry());
    h.check("find_opens_focused", opened, "Ctrl+F: the bar is open with the query focused");
    h.type_text("cat").await;
    h.sleep(Duration::from_millis(100)).await;
    let as_you_type = (rig.editor.selection_utf16(), rig.find.tally_text());
    let announced = rig.find_changes.get() > changes && rig.find.has_query();
    h.check(
        "find_as_you_type",
        as_you_type == ((0, 3), "1 of 3".to_string()) && announced,
        format!("typing `cat` selects {:?}, tally {:?}, changed raised: {announced}", as_you_type.0, as_you_type.1),
    );
    h.screenshot("find").await;
    h.key("Return").await;
    let next = rig.editor.selection_utf16();
    let kept = focused_is(rig, &rig.find.query_entry());
    h.key("shift+Return").await;
    let previous = rig.editor.selection_utf16();
    h.check(
        "find_enter_next_previous",
        next == (8, 3) && previous == (0, 3) && kept,
        format!("Enter → {next:?} (focus kept: {kept}), Shift+Enter → {previous:?}"),
    );

    // Anything else taking the PRIMARY selection (a selection made in another widget or app)
    // collapses the editor's hit to its caret — `GtkTextBuffer`'s clipboard detach; the bar
    // still knows the hit it stands on (the tally, Find Previous).
    h.key("Return").await;
    rig.window.primary_clipboard().set_text("elsewhere");
    h.sleep(Duration::from_millis(120)).await;
    let collapsed = rig.editor.selection_utf16();
    let tally = rig.find.tally_text();
    h.key("shift+Return").await;
    let previous = rig.editor.selection_utf16();
    h.check(
        "find_recovers_its_hit",
        collapsed == (11, 0) && tally == "2 of 3" && previous == (0, 3),
        format!("hit (8, 3), PRIMARY taken → editor at {collapsed:?}, tally {tally:?}; Shift+Enter → {previous:?}"),
    );

    // Ctrl+G from the editor: the next hit, the focus stays in the editor.
    rig.editor.focus();
    h.sleep(Duration::from_millis(80)).await;
    h.key(&xdotool_chord(CommandId::FindNext)).await;
    let hit = rig.editor.selection_utf16();
    let in_editor = rig.editor.text_view().has_focus();
    h.key(&xdotool_chord(CommandId::FindPrevious)).await;
    let back = rig.editor.selection_utf16();
    h.check(
        "find_next_from_editor",
        hit == (8, 3) && in_editor && back == (0, 3),
        format!("Ctrl+G → {hit:?} (editor focused: {in_editor}), Shift+Ctrl+G → {back:?}"),
    );

    // Escape in the query closes the bar and gives the focus back.
    rig.find.show_find();
    h.sleep(Duration::from_millis(80)).await;
    h.key("Escape").await;
    let closed = !rig.find.is_open() && rig.editor.text_view().has_focus();
    h.check("find_escape_closes", closed, "Escape in the query: bar closed, editor focused");

    // Escape in the editor with the bar open closes the bar first.
    rig.find.show_find();
    rig.editor.focus();
    h.sleep(Duration::from_millis(80)).await;
    h.key("Escape").await;
    h.check("find_escape_from_editor", !rig.find.is_open(), "Escape in the editor closes an open bar");

    // Replace: the hit the editor stands on, then the next one; focus stays in the field.
    rig.editor.select_utf16(8, 3);
    h.key(&xdotool_chord(CommandId::Replace)).await;
    let replace_focused = focused_is(rig, &rig.find.replacement_entry());
    h.type_text("cow").await;
    h.key("Return").await;
    let editor = Rc::clone(&rig.editor);
    h.wait_until(Duration::from_secs(1), move || editor.text() == "cat dog cow bird cat").await;
    let after_one = (rig.editor.text(), rig.editor.selection_utf16(), focused_is(rig, &rig.find.replacement_entry()));
    h.check(
        "replace_once",
        replace_focused && after_one == ("cat dog cow bird cat".to_string(), (17, 3), true),
        format!("Ctrl+H focused the replacement ({replace_focused}); Enter → {after_one:?}"),
    );

    // Ctrl+H again selects the replacement field's text, which takes the PRIMARY selection and
    // collapses the editor's hit: Replace still replaces the hit the bar stands on.
    h.key(&xdotool_chord(CommandId::Replace)).await;
    let collapsed = rig.editor.selection_utf16();
    h.key("Return").await;
    let editor = Rc::clone(&rig.editor);
    h.wait_until(Duration::from_secs(1), move || editor.text() == "cat dog cow bird cow").await;
    let after_two = (rig.editor.text(), rig.editor.selection_utf16());
    h.check(
        "replace_after_refocus",
        after_two == ("cat dog cow bird cow".to_string(), (0, 3)),
        format!("Ctrl+H over a filled field left the editor at {collapsed:?}; Enter → {after_two:?}"),
    );

    // Each Replace is one undo step (in the editor; in the field Ctrl+Z would undo the field).
    rig.editor.focus();
    h.sleep(Duration::from_millis(80)).await;
    h.key("ctrl+z").await;
    expect_text(
        h,
        rig,
        "replace_undo_second",
        "cat dog cow bird cat",
        "Ctrl+Z in the editor undoes the second Replace",
    )
    .await;
    h.key("ctrl+z").await;
    expect_text(h, rig, "replace_undo_first", text, "a second Ctrl+Z undoes the first").await;

    // Replace All (Shift+Enter): one edit, one undo step, the focus back in the editor with the
    // rewritten span selected.
    rig.editor.select_utf16(0, 0);
    rig.find.replacement_entry().grab_focus();
    h.sleep(Duration::from_millis(80)).await;
    h.key("shift+Return").await;
    let editor = Rc::clone(&rig.editor);
    h.wait_until(Duration::from_secs(1), move || editor.text() == "cow dog cow bird cow").await;
    h.sleep(Duration::from_millis(50)).await;
    let all = (rig.editor.text(), rig.editor.text_view().has_focus(), rig.editor.selection_utf16());
    h.screenshot("replace-all").await;
    h.key("ctrl+z").await;
    let editor = Rc::clone(&rig.editor);
    h.wait_until(Duration::from_secs(1), move || editor.text() == text).await;
    let undone = rig.editor.text();
    h.check(
        "replace_all_one_undo",
        all == ("cow dog cow bird cow".to_string(), true, (0, 20)) && undone == text,
        format!("Shift+Enter → {all:?}; one Ctrl+Z → {undone:?}"),
    );

    // Use Selection for Find.
    rig.editor.select_utf16(4, 3);
    h.key(&xdotool_chord(CommandId::UseSelectionForFind)).await;
    let query = rig.find.query_entry().text().to_string();
    let tally = rig.find.tally_text();
    h.check(
        "use_selection_for_find",
        query == "dog" && tally == "1 of 1" && rig.editor.text_view().has_focus(),
        format!("Ctrl+E over `dog` → query {query:?}, tally {tally:?}, focus kept in the editor"),
    );
    rig.find.close();
}

/// A dead key through GTK's own input method (GtkIMContextSimple, X11): the preedit, then the
/// committed letter, judged like typing — a sentence start makes it a capital (the Mac's rule).
async fn composition(h: &Harness, rig: &Rig) {
    reset(h, rig, "", 0, 0).await;
    let composing = Rc::new(Cell::new(false));
    let seen = Rc::clone(&composing);
    let id = rig.editor.text_view().connect_preedit_changed(move |_, preedit| {
        if !preedit.is_empty() {
            seen.set(true);
        }
    });
    h.key("dead_acute").await;
    h.key("e").await;
    rig.editor.text_view().disconnect(id);
    let editor = Rc::clone(&rig.editor);
    h.wait_until(Duration::from_secs(1), move || !editor.text().is_empty()).await;
    let text = rig.editor.text();
    h.check(
        "dead_key_capital",
        text == "\u{C9}",
        format!("dead_acute, e at the document start → {text:?} (preedit seen: {})", composing.get()),
    );
}

// ── keys: Zen ────────────────────────────────────────────────────────────────────────────────

async fn zen_mode(h: &Harness, rig: &Rig) {
    reset(h, rig, "# Zen\n\nWriting in the column.", 0, 0).await;
    rig.state.set_stored_mode(ViewMode::Split);
    h.key(&xdotool_chord(CommandId::ZenMode)).await;
    let entered = rig.zen.is_active();
    let bin = rig.panes.zen_bin();
    let zen_bin = bin.clone();
    h.wait_until(Duration::from_secs(2), move || zen_bin.is_active()).await;
    h.next_frame(&rig.window).await;
    h.sleep(Duration::from_millis(200)).await;
    let full = rig.window.is_fullscreen();
    let (bw, bh) = (bin.width(), bin.height());
    let child = bin.child().expect("the panes");
    let bounds = child.compute_bounds(&bin);
    let (cw, ch) = (child.width(), child.height());
    let (want_x, want_w) = zen::centred(bw, zen::WIDTH_FRACTION, 0);
    let (want_y, want_h) = zen::centred(bh, zen::HEIGHT_FRACTION, 0);
    let x = bounds.map(|b| b.x().round() as i32).unwrap_or(-1);
    let y = bounds.map(|b| b.y().round() as i32).unwrap_or(-1);
    let geometry =
        (cw - want_w).abs() <= 1 && (ch - want_h).abs() <= 1 && (x - want_x).abs() <= 1 && (y - want_y).abs() <= 1;
    let writing = rig.panes.layout() == PaneLayout::EditorOnly;
    h.check(
        "zen_enter",
        entered && geometry && writing && !rig.footer.widget().is_visible(),
        format!(
            "Shift+Ctrl+Return: Zen on, column {cw}×{ch} at ({x}, {y}) of {bw}×{bh} (want {want_w}×{want_h} at \
             ({want_x}, {want_y})), editor only, footer hidden; window full screen: {full}"
        ),
    );
    let shown = rig.capsule.is_shown();
    let capsule = rig.capsule.widget();
    let place = capsule.compute_bounds(&rig.window).map(|b| (b.x() + b.width() / 2.0, b.y()));
    let centred = place
        .is_some_and(|(mid, top)| (f64::from(mid) - f64::from(rig.window.width()) / 2.0).abs() <= 2.0 && top <= 12.0);
    h.check(
        "zen_capsule_place",
        centred,
        format!("the capsule's top centre: {place:?} in a {}-wide window", rig.window.width()),
    );
    h.screenshot("zen").await;

    // The capsule fades 2.5 s after the last reveal; pointer movement brings it back.
    let capsule = Rc::clone(&rig.capsule);
    let faded = h.wait_until(Duration::from_millis(4000), move || !capsule.is_shown()).await;
    rig.zen.reveal();
    let revealed = rig.capsule.is_shown();
    h.check(
        "zen_capsule_fades",
        shown && faded && revealed,
        format!("shown on entry ({shown}), faded after 2.5 s ({faded}), revealed again ({revealed})"),
    );

    // The capsule's read switch shows the preview; write brings the editor back.
    rig.capsule.press(1);
    let reading = rig.panes.layout();
    rig.capsule.press(0);
    let writing_again = rig.panes.layout();
    h.check(
        "zen_switches",
        reading == PaneLayout::PreviewOnly && writing_again == PaneLayout::EditorOnly,
        format!("Read → {reading:?}, Write → {writing_again:?}"),
    );

    // Typing still works in the column, and Escape leaves Zen.
    rig.editor.focus();
    h.type_text("!").await;
    let typed = rig.editor.text().ends_with('!') || rig.editor.text().starts_with('!');
    h.key("Escape").await;
    let zen_bin = bin.clone();
    let left = h.wait_until(Duration::from_secs(2), move || !zen_bin.is_active()).await;
    let restored = rig.panes.layout();
    h.check(
        "zen_leave",
        typed && left && !rig.zen.is_active() && restored != PaneLayout::EditorOnly && rig.footer.widget().is_visible(),
        format!("typing in Zen ({typed}); Escape → Zen off ({left}), layout {restored:?}, footer back"),
    );

    // Leaving full screen by other means (F11) drops Zen — when the display grants full screen.
    h.key(&xdotool_chord(CommandId::ZenMode)).await;
    let window = rig.window.clone();
    let granted = h.wait_until(Duration::from_secs(2), move || window.is_fullscreen()).await;
    if granted {
        h.key("F11").await;
        let zen = Rc::clone(&rig.zen);
        let dropped = h.wait_until(Duration::from_secs(2), move || !zen.is_active()).await;
        h.check("zen_f11_leaves", dropped, "F11 while in Zen: full screen left and Zen with it");
    } else {
        // No window manager under Xvfb: nothing grants full screen, so `notify::fullscreened`
        // never fires. The window's report is driven directly to prove the controller's half.
        rig.zen.presenter_changed(false);
        let dropped = !rig.zen.is_active();
        rig.zen.set_active(false);
        h.check(
            "zen_f11_leaves",
            dropped,
            "no window manager grants full screen here; presenter_changed(false) drops Zen (real-desktop check)",
        );
    }
    rig.zen.set_active(false);
}
