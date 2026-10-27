//! Port of md.win `tests/Md.App.Logic.Tests/CommandTableTests.cs`: the invariants of the
//! declarative command surface (shell-design §2) — one row per command, one owner per chord, the
//! Mac's wording to the character, and the eight menus with the dividers §2 draws. Everything the
//! GTK menu builder does is a walk over what is asserted here.
//!
//! LINUX: the chords are GTK's (see `commands::chord`); the Share rows are gone; Exit is "Quit md"
//! with Ctrl+Q. The tests that pinned those rows pin the Linux ones, and the Linux-only tests at
//! the end pin the chord decisions against GNOME's and GtkTextView's own bindings.

use std::collections::{HashMap, HashSet};

use md_core::markdown::block::OutlineEntry;
use md_logic::commands::command_table::{self, EXIT_FULL_SCREEN_TITLE, MENU_TITLES};
use md_logic::commands::{Chord, CommandId, CommandKind, KeyModifiers, MenuNode, MenuPath, ShellSnapshot, keys};
use md_logic::strings::books;

const CTRL: KeyModifiers = KeyModifiers::CTRL;
const SHIFT: KeyModifiers = KeyModifiers::SHIFT;
const ALT: KeyModifiers = KeyModifiers::ALT;

// ── The table itself ──────────────────────────────────────────────────────────────────────

#[test]
fn every_command_id_appears_exactly_once() {
    let mut ids: Vec<CommandId> = command_table::all().iter().map(|s| s.id).collect();
    let distinct: HashSet<CommandId> = ids.iter().copied().collect();
    assert_eq!(ids.len(), distinct.len());
    ids.sort();
    let mut every = CommandId::ALL.to_vec();
    every.sort();
    assert_eq!(every, ids);
}

#[test]
fn no_chord_is_claimed_twice() {
    let mut seen: HashMap<Chord, usize> = HashMap::new();
    for chord in command_table::all().iter().filter_map(|s| s.chord) {
        *seen.entry(chord).or_default() += 1;
    }
    let duplicates: Vec<String> = seen.iter().filter(|(_, n)| **n > 1).map(|(c, _)| c.display_text()).collect();
    assert!(duplicates.is_empty(), "{duplicates:?}");
}

#[test]
fn every_row_has_a_path_except_the_accelerator_only_one() {
    for spec in command_table::all() {
        if spec.kind == CommandKind::Accelerator {
            assert!(spec.path.is_none());
        } else {
            assert!(spec.path.is_some(), "{:?}", spec.id);
        }
    }
    let accelerators: Vec<CommandId> =
        command_table::all().iter().filter(|s| s.kind == CommandKind::Accelerator).map(|s| s.id).collect();
    assert_eq!(accelerators, vec![CommandId::Escape]);
}

#[test]
fn every_row_sits_in_one_of_the_eight_menus() {
    for spec in command_table::all() {
        if let Some(path) = spec.path {
            assert!(MENU_TITLES.contains(&path.menu), "{:?}", spec.id);
        }
    }
}

#[test]
fn for_panics_for_nothing_because_the_table_is_complete() {
    for id in CommandId::ALL {
        assert_eq!(command_table::spec(id).id, id);
    }
}

// ── Titles, verbatim ──────────────────────────────────────────────────────────────────────

/// Every title, transcribed from §2 independently of the table. The ellipsis is U+2026 (written as
/// an escape here too): three ASCII dots in either file would fail this.
const TITLES: [(CommandId, &str); 62] = [
    (CommandId::New, "New"),
    (CommandId::Open, "Open\u{2026}"),
    (CommandId::OpenRecentEntry, "Open Recent"),
    (CommandId::ClearRecent, "Clear Menu"),
    (CommandId::OpenTextBundleFolder, "Open TextBundle Folder\u{2026}"),
    (CommandId::Example, "Examples"),
    (CommandId::ExampleBook, "Example Book\u{2026}"),
    (CommandId::Close, "Close"),
    (CommandId::Save, "Save"),
    (CommandId::SaveAs, "Save As\u{2026}"),
    (CommandId::Duplicate, "Duplicate"),
    (CommandId::Rename, "Rename\u{2026}"),
    (CommandId::MoveTo, "Move To\u{2026}"),
    (CommandId::RevertToSaved, "Revert to Saved"),
    (CommandId::Print, "Print\u{2026}"),
    // LINUX: md.win's Share ▸ Source… / Rendered PDF… rows do not exist (no share sheet).
    (CommandId::ExportPdf, "PDF\u{2026}"),
    (CommandId::ExportHtml, "HTML\u{2026}"),
    (CommandId::ExportEpub, "EPUB\u{2026}"),
    (CommandId::ExportLaTeX, "LaTeX\u{2026}"),
    (CommandId::ExportTextBundle, "TextBundle\u{2026}"),
    (CommandId::ExportDiagramSvg, "Diagram as SVG"),
    (CommandId::PdfPageSize, "PDF Page Size"),
    // LINUX: the Mac's own app-menu row, moved to File where md.win put "Exit".
    (CommandId::Quit, "Quit md"),
    (CommandId::Undo, "Undo"),
    (CommandId::Redo, "Redo"),
    (CommandId::Cut, "Cut"),
    (CommandId::Copy, "Copy"),
    (CommandId::Paste, "Paste"),
    (CommandId::Delete, "Delete"),
    (CommandId::SelectAll, "Select All"),
    (CommandId::Find, "Find\u{2026}"),
    (CommandId::FindNext, "Find Next"),
    (CommandId::FindPrevious, "Find Previous"),
    (CommandId::Replace, "Replace\u{2026}"),
    (CommandId::UseSelectionForFind, "Use Selection for Find"),
    (CommandId::ContinueLists, "Continue Lists and Tables"),
    (CommandId::CapitalizeSentences, "Capitalize Sentences"),
    (CommandId::ViewEdit, "Edit"),
    (CommandId::ViewSplit, "Split"),
    (CommandId::ViewPreview, "Preview"),
    (CommandId::ZenMode, "Zen Mode"),
    (CommandId::ShowSidebar, "Show Sidebar"),
    (CommandId::FullScreen, "Enter Full Screen"),
    (CommandId::NewBook, "New Book\u{2026}"),
    (CommandId::OpenBook, "Open Book\u{2026}"),
    (CommandId::ShowBook, "Show Book"),
    (CommandId::CloseBook, "Close Book"),
    // LINUX: md.win's "Share Book as PDF" row does not exist.
    (CommandId::PrintBook, "Print Book\u{2026}"),
    (CommandId::ExportBookPdf, "PDF\u{2026}"),
    (CommandId::ExportBookEpub, "EPUB\u{2026}"),
    (CommandId::ExportBookLaTeX, "LaTeX\u{2026}"),
    (CommandId::PreviousArticle, "Previous Article"),
    (CommandId::NextArticle, "Next Article"),
    (CommandId::Contents, "Contents"),
    (CommandId::Notes, "Notes"),
    (CommandId::Minimize, "Minimize"),
    (CommandId::Zoom, "Zoom"),
    (CommandId::ActivateWindow, "Window List"),
    (CommandId::Help, "md Help"),
    (CommandId::PrivacyPolicy, "Privacy Policy"),
    (CommandId::About, "About md"),
    (CommandId::Escape, "Escape"),
];

#[test]
fn title_is_the_macs_word_verbatim() {
    for (id, expected) in TITLES {
        assert_eq!(command_table::spec(id).title, expected, "{id:?}");
    }
}

/// md.win reads its own docs/shell-design.md §2 and asserts every ellipsis title is spelled there.
/// LINUX: this repo carries no shell design document, so the same trap — an ASCII "..." creeping
/// into a title, which no eye catches in a menu — is caught on the table itself: every title that
/// ends in an ellipsis ends in U+2026, and no title holds three dots.
#[test]
fn every_title_with_an_ellipsis_is_spelled_in_section_2_of_the_design() {
    for spec in command_table::all() {
        assert!(!spec.title.contains("..."), "{:?} spells its ellipsis with three dots", spec.id);
    }
    let with_ellipsis = command_table::all().iter().filter(|s| s.title.contains('\u{2026}')).count();
    assert_eq!(with_ellipsis, 20); // md.win's 22, less the two Share rows
    for spec in command_table::all().iter().filter(|s| s.title.contains('\u{2026}')) {
        assert!(spec.title.ends_with('\u{2026}'), "{:?}", spec.id);
    }
}

#[test]
fn exit_full_screen_is_the_other_half_of_the_full_screen_row() {
    let windowed = ShellSnapshot::empty();
    assert_eq!(command_table::display_title(CommandId::FullScreen, &windowed), "Enter Full Screen");
    let full = ShellSnapshot { is_full_screen: true, ..ShellSnapshot::empty() };
    assert_eq!(command_table::display_title(CommandId::FullScreen, &full), "Exit Full Screen");
    assert_eq!(EXIT_FULL_SCREEN_TITLE, "Exit Full Screen");
    // Everything else ignores the snapshot.
    assert_eq!(command_table::display_title(CommandId::Save, &full), "Save");
}

// ── Chords ────────────────────────────────────────────────────────────────────────────────

/// Every chord as GTK labels it (`gtk_accelerator_get_label`, untranslated — measured in the
/// Ubuntu 26.04 image with GTK 4.22.4) and as `gtk_accelerator_name` spells it (the string the
/// shell hands `set_accels_for_action`; each parsed, `gtk_accelerator_valid`, and round-tripped
/// unchanged).
const CHORDS: [(CommandId, &str, &str); 45] = [
    (CommandId::New, "Ctrl+N", "<Control>n"),
    (CommandId::Open, "Ctrl+O", "<Control>o"),
    (CommandId::Close, "Ctrl+W", "<Control>w"),
    (CommandId::Save, "Ctrl+S", "<Control>s"),
    (CommandId::SaveAs, "Shift+Ctrl+S", "<Shift><Control>s"),
    (CommandId::Print, "Ctrl+P", "<Control>p"),
    // LINUX: GNOME's Quit; md.win's Exit had no chord.
    (CommandId::Quit, "Ctrl+Q", "<Control>q"),
    (CommandId::Undo, "Ctrl+Z", "<Control>z"),
    // LINUX: GNOME's Redo (GtkTextView binds it and Ctrl+Y alike); md.win shows Ctrl+Y.
    (CommandId::Redo, "Shift+Ctrl+Z", "<Shift><Control>z"),
    (CommandId::Cut, "Ctrl+X", "<Control>x"),
    (CommandId::Copy, "Ctrl+C", "<Control>c"),
    (CommandId::Paste, "Ctrl+V", "<Control>v"),
    (CommandId::Delete, "Delete", "Delete"),
    (CommandId::SelectAll, "Ctrl+A", "<Control>a"),
    (CommandId::Find, "Ctrl+F", "<Control>f"),
    // LINUX: the Mac's ⌘G / ⇧⌘G and GNOME's own; md.win's F3 / Shift+F3.
    (CommandId::FindNext, "Ctrl+G", "<Control>g"),
    (CommandId::FindPrevious, "Shift+Ctrl+G", "<Shift><Control>g"),
    (CommandId::Replace, "Ctrl+H", "<Control>h"),
    (CommandId::UseSelectionForFind, "Ctrl+E", "<Control>e"),
    (CommandId::ViewEdit, "Ctrl+1", "<Control>1"),
    (CommandId::ViewSplit, "Ctrl+2", "<Control>2"),
    (CommandId::ViewPreview, "Ctrl+3", "<Control>3"),
    (CommandId::ZenMode, "Shift+Ctrl+Return", "<Shift><Control>Return"),
    (CommandId::FullScreen, "F11", "F11"),
    (CommandId::ShowBook, "Shift+Ctrl+B", "<Shift><Control>b"),
    // LINUX: GNOME binds Ctrl+Alt+Up/Down to switch-to-workspace-up/down.
    (CommandId::PreviousArticle, "Alt+Up", "<Alt>Up"),
    (CommandId::NextArticle, "Alt+Down", "<Alt>Down"),
    (CommandId::Help, "F1", "F1"),
    (CommandId::Escape, "Escape", "Escape"),
    // Format (1.6, formatting.md §5) — the family's chords, nettrash's 2026-10-10 decision.
    (CommandId::Bold, "Ctrl+B", "<Control>b"),
    (CommandId::Italic, "Ctrl+I", "<Control>i"),
    (CommandId::InlineCode, "Shift+Ctrl+C", "<Shift><Control>c"),
    (CommandId::Strikethrough, "Shift+Ctrl+X", "<Shift><Control>x"),
    (CommandId::Link, "Ctrl+K", "<Control>k"),
    (CommandId::Heading1, "Ctrl+Alt+1", "<Control><Alt>1"),
    (CommandId::Heading2, "Ctrl+Alt+2", "<Control><Alt>2"),
    (CommandId::Heading3, "Ctrl+Alt+3", "<Control><Alt>3"),
    (CommandId::Heading4, "Ctrl+Alt+4", "<Control><Alt>4"),
    (CommandId::Heading5, "Ctrl+Alt+5", "<Control><Alt>5"),
    (CommandId::Heading6, "Ctrl+Alt+6", "<Control><Alt>6"),
    (CommandId::BulletList, "Shift+Ctrl+L", "<Shift><Control>l"),
    (CommandId::TaskList, "Shift+Ctrl+T", "<Shift><Control>t"),
    (CommandId::Quote, "Shift+Ctrl+Q", "<Shift><Control>q"),
    (CommandId::Indent, "Ctrl+]", "<Control>bracketright"),
    (CommandId::Outdent, "Ctrl+[", "<Control>bracketleft"),
];

#[test]
fn chord_reads_the_gtk_way() {
    for (id, label, _) in CHORDS {
        let chord = command_table::spec(id).chord.unwrap_or_else(|| panic!("{id:?} has no chord"));
        assert_eq!(chord.display_text(), label, "{id:?}");
        assert_eq!(chord.to_string(), label, "{id:?}");
    }
}

#[test]
fn chord_is_the_accelerator_gtk_parses() {
    // LINUX: the second half of md.win's chord — what the shell registers.
    for (id, _, accelerator) in CHORDS {
        assert_eq!(command_table::spec(id).chord.map(|c| c.accelerator()).as_deref(), Some(accelerator), "{id:?}");
    }
}

#[test]
fn only_these_forty_five_rows_carry_a_chord() {
    // LINUX: md.win's forty-four, plus Quit md.
    let mut with_chords: Vec<CommandId> =
        command_table::all().iter().filter(|s| s.chord.is_some()).map(|s| s.id).collect();
    let mut expected: Vec<CommandId> = CHORDS.iter().map(|(id, _, _)| *id).collect();
    with_chords.sort();
    expected.sort();
    assert_eq!(expected, with_chords);
}

#[test]
#[ignore = "reads md.win's docs/app-api.md; the Linux repo has no app-api document to keep in step"]
fn the_app_api_doc_carries_the_tables_own_counts_and_its_whole_command_id_listing() {
    // md.win: the row count, "N of the M chords" and the CommandId listing in docs/app-api.md
    // match the table.
}

#[test]
fn the_editors_own_keys_are_shown_but_never_registered_on_the_root() {
    // §2.4: Ctrl+Z, Shift+Ctrl+Z, Ctrl+X/C/V/A and Delete stay with the focused widget — the web
    // view must keep Ctrl+C on a preview selection, and Delete must delete a character, not a
    // selection's worth.
    let widget_owned = [
        CommandId::Undo,
        CommandId::Redo,
        CommandId::Cut,
        CommandId::Copy,
        CommandId::Paste,
        CommandId::Delete,
        CommandId::SelectAll,
    ];
    for id in widget_owned {
        let spec = command_table::spec(id);
        assert!(spec.chord.is_some());
        assert!(!spec.root_accelerator);
    }
    let mut not_root: Vec<CommandId> =
        command_table::all().iter().filter(|s| !s.root_accelerator).map(|s| s.id).collect();
    not_root.sort();
    let mut expected = widget_owned.to_vec();
    expected.sort();
    assert_eq!(expected, not_root);
    assert!(command_table::root_accelerators().iter().all(|s| !widget_owned.contains(&s.id)));
    // LINUX: md.win's 37, plus Quit md's Ctrl+Q.
    assert_eq!(command_table::root_accelerators().len(), 38);
}

#[test]
fn key_names_are_the_menu_spellings() {
    // LINUX: GTK's labels, by keyval name (md.win: "Esc", "Del", "Enter").
    for (key, expected) in [
        (keys::ESCAPE, "Escape"),
        (keys::DELETE, "Delete"),
        (keys::RETURN, "Return"),
        (keys::UP, "Up"),
        (keys::DOWN, "Down"),
        (keys::F1, "F1"),
        (keys::F11, "F11"),
        ("1", "1"),
        ("z", "Z"),
    ] {
        assert_eq!(Chord::key_name(key), expected, "{key}");
    }
}

#[test]
fn modifiers_read_shift_then_ctrl_then_alt() {
    // LINUX: GTK's order (md.win: Ctrl, Alt, Shift — "Ctrl+Alt+Shift+F3").
    let chord = Chord::new("F3", CTRL | ALT | SHIFT);
    assert_eq!(chord.display_text(), "Shift+Ctrl+Alt+F3");
    assert_eq!(chord.accelerator(), "<Shift><Control><Alt>F3");
}

// ── The bar ───────────────────────────────────────────────────────────────────────────────

#[test]
fn the_bar_is_eight_menus_in_the_macs_order_without_the_app_menu() {
    assert_eq!(MENU_TITLES, ["File", "Edit", "Format", "View", "Book", "Go", "Window", "Help"]);
    let titles: Vec<&str> = command_table::menus().iter().map(|m| m.title).collect();
    assert_eq!(titles, MENU_TITLES);
    assert!(command_table::menus().iter().all(|m| m.command.is_none()));
}

fn menu(title: &str) -> &'static MenuNode {
    let found: Vec<&MenuNode> = command_table::menus().iter().filter(|m| m.title == title).collect();
    assert_eq!(found.len(), 1, "{title}");
    found[0]
}

fn child<'a>(parent: &'a MenuNode, title: &str) -> &'a MenuNode {
    let found: Vec<&MenuNode> = parent.children.iter().filter(|c| c.title == title).collect();
    assert_eq!(found.len(), 1, "{title}");
    found[0]
}

/// Rows in order, a lone "—" standing in for each separator §2 draws.
fn rows(menu: &MenuNode) -> Vec<&'static str> {
    let mut rows = Vec::new();
    for c in &menu.children {
        if c.separator_before {
            rows.push("—");
        }
        rows.push(c.title);
    }
    rows
}

#[test]
fn file_reads_as_section_2_2_does_dividers_included() {
    // LINUX: no Share ▸ beside Export ▸, and Quit md where md.win has Exit.
    assert_eq!(
        rows(menu("File")),
        [
            "New",
            "Open\u{2026}",
            "Open Recent",
            "Open TextBundle Folder\u{2026}",
            "Examples",
            "—",
            "Close",
            "Save",
            "Save As\u{2026}",
            "Duplicate",
            "Rename\u{2026}",
            "Move To\u{2026}",
            "Revert to Saved",
            "—",
            "Print\u{2026}",
            "—",
            "Export",
            "—",
            "Quit md",
        ]
    );
}

#[test]
fn files_submenus_hold_what_section_2_2_says() {
    assert_eq!(rows(child(menu("File"), "Open Recent")), ["Open Recent", "—", "Clear Menu"]);
    assert_eq!(rows(child(menu("File"), "Examples")), ["Examples", "—", "Example Book\u{2026}"]);
    // LINUX: md.win's Share ▸ (Source…, Rendered PDF…) does not exist.
    assert!(menu("File").children.iter().all(|c| c.title != "Share"));
    assert_eq!(
        rows(child(menu("File"), "Export")),
        [
            "PDF\u{2026}",
            "HTML\u{2026}",
            "EPUB\u{2026}",
            "LaTeX\u{2026}",
            "TextBundle\u{2026}",
            "—",
            "Diagram as SVG",
            "—",
            "PDF Page Size",
        ]
    );
}

#[test]
fn edit_reads_as_section_2_4_does() {
    assert_eq!(
        rows(menu("Edit")),
        [
            "Undo",
            "Redo",
            "—",
            "Cut",
            "Copy",
            "Paste",
            "Delete",
            "Select All",
            "—",
            "Find\u{2026}",
            "Find Next",
            "Find Previous",
            "Replace\u{2026}",
            "Use Selection for Find",
            "—",
            "Typing",
        ]
    );
}

#[test]
fn the_typing_rows_are_toggles_without_chords_under_edit_typing_worded_as_every_port_words_them() {
    // smart-typing.md §3.1: the same two titles on every md port, under the same "Typing" heading
    // (a submenu here, as on the Mac), no shortcut, ticked from the settings. 1.6 adds the third,
    // the wrap of formatting.md §4, worded as that document words it.
    for id in [CommandId::ContinueLists, CommandId::CapitalizeSentences, CommandId::WrapSelection] {
        let spec = command_table::spec(id);
        assert_eq!(spec.kind, CommandKind::Toggle);
        assert!(spec.chord.is_none());
        assert_eq!(spec.path, Some(MenuPath::submenu("Edit", "Typing")));
        assert!(!spec.port_only);
    }
    let typing = child(menu("Edit"), "Typing");
    assert!(typing.command.is_none()); // a container, not a row
    assert!(typing.separator_before);
    assert_eq!(
        rows(typing),
        ["Continue Lists and Tables", "Capitalize Sentences", "Wrap Selection When Typing * _ ` $"]
    );
    assert!(typing.children.iter().all(|c| c.command.map(|s| s.kind) == Some(CommandKind::Toggle)));
}

fn all_nodes(nodes: &'static [MenuNode]) -> Vec<&'static MenuNode> {
    let mut out = Vec::new();
    for node in nodes {
        out.push(node);
        out.extend(all_nodes(&node.children));
    }
    out
}

#[test]
fn the_static_submenus_are_the_ones_the_four_ports_share() {
    // Every submenu container in the bar; a new one lands here on purpose, not by accident.
    // LINUX: no "File ▸ Share".
    let submenus: Vec<String> = all_nodes(command_table::menus())
        .into_iter()
        .filter(|n| n.command.is_none() && n.path.depth() > 1)
        .map(|n| n.path.to_string())
        .collect();
    assert_eq!(
        submenus,
        [
            "File ▸ Open Recent",
            "File ▸ Examples",
            "File ▸ Export",
            "File ▸ Export ▸ Diagram as SVG",
            "File ▸ Export ▸ PDF Page Size",
            "Edit ▸ Typing",
            "Format ▸ Heading",
            "Book ▸ Export Book",
            "Go ▸ Contents",
            "Go ▸ Notes",
        ]
    );
}

#[test]
fn format_reads_as_the_formatting_spec_does() {
    // formatting.md §5: the inline marks, the headings, the line blocks, the indent pair.
    assert_eq!(
        rows(menu("Format")),
        [
            "Bold",
            "Italic",
            "Code",
            "Strikethrough",
            "Link",
            "—",
            "Heading",
            "—",
            "Bullet List",
            "Task List",
            "Quote",
            "—",
            "Indent",
            "Outdent",
        ]
    );
    assert_eq!(
        rows(child(menu("Format"), "Heading")),
        ["Heading 1", "Heading 2", "Heading 3", "Heading 4", "Heading 5", "Heading 6"]
    );
    let commands = md_logic::text::formatting_adapter::commands();
    for id in commands.iter().copied() {
        let spec = command_table::spec(id);
        assert_eq!(spec.kind, CommandKind::Item);
        assert!(spec.root_accelerator);
        assert!(!spec.port_only);
    }
    assert_eq!(commands.len(), 16);
}

#[test]
fn no_two_rows_share_a_chord() {
    let chords: Vec<Chord> = command_table::all().iter().filter_map(|s| s.chord).collect();
    let distinct: HashSet<Chord> = chords.iter().copied().collect();
    assert_eq!(chords.len(), distinct.len());
}

#[test]
fn the_bracket_keys_read_as_their_characters() {
    assert_eq!(Chord::key_name(keys::BRACKET_LEFT), "[");
    assert_eq!(Chord::key_name(keys::BRACKET_RIGHT), "]");
    assert_eq!(Chord::key_name("6"), "6");
}

#[test]
fn view_reads_as_section_2_5_does() {
    assert_eq!(rows(menu("View")), ["Edit", "Split", "Preview", "Zen Mode", "—", "Show Sidebar", "Enter Full Screen"]);
}

#[test]
fn book_reads_as_section_2_6_does() {
    // LINUX: no "Share Book as PDF".
    assert_eq!(
        rows(menu("Book")),
        ["New Book\u{2026}", "Open Book\u{2026}", "Show Book", "Close Book", "—", "Print Book\u{2026}", "Export Book"]
    );
    assert_eq!(rows(child(menu("Book"), "Export Book")), ["PDF\u{2026}", "EPUB\u{2026}", "LaTeX\u{2026}"]);
}

#[test]
fn go_reads_as_section_2_7_does() {
    assert_eq!(rows(menu("Go")), ["Previous Article", "Next Article", "—", "Contents", "Notes"]);
    let contents = child(menu("Go"), "Contents");
    assert_eq!(contents.children.len(), 1);
    assert_eq!(contents.children[0].command.map(|s| s.id), Some(CommandId::Contents));
    let notes = child(menu("Go"), "Notes");
    assert_eq!(notes.children.len(), 1);
    assert_eq!(notes.children[0].command.map(|s| s.id), Some(CommandId::Notes));
}

#[test]
fn window_and_help_read_as_section_2_8_does() {
    assert_eq!(rows(menu("Window")), ["Minimize", "Zoom", "—", "Window List"]);
    assert_eq!(rows(menu("Help")), ["md Help", "Privacy Policy", "—", "About md"]);
}

#[test]
fn escape_is_in_no_menu() {
    assert!(all_nodes(command_table::menus()).iter().all(|n| n.command.map(|s| s.id) != Some(CommandId::Escape)));
}

#[test]
fn every_row_of_the_table_reaches_the_bar_exactly_once() {
    let placed: Vec<CommandId> =
        all_nodes(command_table::menus()).iter().filter_map(|n| n.command.map(|s| s.id)).collect();
    let distinct: HashSet<CommandId> = placed.iter().copied().collect();
    assert_eq!(placed.len(), distinct.len());
    assert_eq!(placed.len(), command_table::all().len() - 1); // every row but Escape
}

#[test]
fn menu_path_knows_its_depth_leaf_and_ancestry() {
    let nested = MenuPath::nested("File", "Export", "PDF Page Size");
    assert_eq!(nested.depth(), 3);
    assert_eq!(nested.leaf(), "PDF Page Size");
    assert!(nested.is_under(&MenuPath::menu("File")));
    assert!(nested.is_under(&MenuPath::submenu("File", "Export")));
    assert!(!nested.is_under(&MenuPath::submenu("File", "Open Recent")));
    assert!(!nested.is_under(&nested));
    assert_eq!(nested.truncate(2), MenuPath::submenu("File", "Export"));
    assert_eq!(nested.to_string(), "File ▸ Export ▸ PDF Page Size");
}

// ── The row titles the builder cannot invent ──────────────────────────────────────────────

#[test]
fn contents_rows_indent_two_spaces_per_heading_level_below_one() {
    // md.win's (-1, "Title") case cannot arise: md-core's heading level is unsigned.
    for (level, expected) in [(1, "Title"), (2, "  Title"), (3, "    Title"), (0, "Title")] {
        assert_eq!(command_table::contents_row_title(&OutlineEntry::new(level, "Title", "title", 0)), expected);
    }
}

#[test]
fn the_page_size_radios_share_one_group() {
    // LINUX: WinUI groups radios by a GroupName string (md.win pins "PdfPageSize"); a GTK radio row
    // is a target of ONE stateful string action, so what the seven rows share is the action.
    let spec = command_table::spec(CommandId::PdfPageSize);
    assert_eq!(spec.kind, CommandKind::DynamicRadios);
    assert_eq!(CommandId::PdfPageSize.action_name(), "pdf-page-size");
}

#[test]
fn only_the_designs_port_only_rows_are_marked_so() {
    // LINUX: md.win's "(Win)" set, with Quit md in Exit's place.
    let mut expected = vec![
        CommandId::OpenTextBundleFolder,
        CommandId::Quit,
        CommandId::Find,
        CommandId::FindNext,
        CommandId::FindPrevious,
        CommandId::Replace,
        CommandId::UseSelectionForFind,
        CommandId::Minimize,
        CommandId::Zoom,
        CommandId::ActivateWindow,
        CommandId::Escape,
    ];
    expected.sort();
    let mut marked: Vec<CommandId> = command_table::all().iter().filter(|s| s.port_only).map(|s| s.id).collect();
    marked.sort();
    assert_eq!(expected, marked);
}

#[test]
fn root_accelerator_for_answers_the_root_chords_and_not_the_editing_keys() {
    let find_next = command_table::spec(CommandId::FindNext).chord.unwrap();
    assert_eq!(command_table::root_accelerator_for(find_next), Some(CommandId::FindNext));
    assert_eq!(command_table::root_accelerator_for(Chord::new("f", CTRL)), Some(CommandId::Find));
    let copy = command_table::spec(CommandId::Copy).chord.unwrap();
    assert_eq!(command_table::root_accelerator_for(copy), None);
    assert_eq!(command_table::root_accelerator_for(Chord::new("g", ALT)), None);
    for spec in command_table::root_accelerators() {
        assert_eq!(command_table::root_accelerator_for(spec.chord.unwrap()), Some(spec.id));
    }
}

// ── LINUX: the chord decisions, pinned against what the desktop and the widget already own ──

/// The Control / Alt bindings GNOME 50 on Ubuntu 26.04 ships (no Super), read with `gsettings
/// list-recursively` after installing mutter-common, gnome-shell-common,
/// gnome-settings-daemon-common, ubuntu-settings (its overrides), ibus and the Ubuntu dock in the
/// 26.04 image: org.gnome.desktop.wm.keybindings, org.gnome.mutter(.wayland).keybindings,
/// org.gnome.shell.keybindings, org.gnome.settings-daemon.plugins.media-keys,
/// org.freedesktop.ibus.panel.emoji. In `gtk_accelerator_name` spelling.
const GNOME_BINDINGS: [&str; 26] = [
    "<Alt>space",
    "<Alt>F7",
    "<Alt>F8",
    "<Alt>F4",
    "<Alt>F6",
    "<Shift><Alt>F6",
    "<Control><Alt>Escape",
    "<Shift><Control><Alt>Escape",
    "<Alt>Escape",
    "<Shift><Alt>Escape",
    "<Shift><Control><Alt>Down",
    "<Shift><Control><Alt>Left",
    "<Shift><Control><Alt>Right",
    "<Shift><Control><Alt>Up",
    "<Alt>F2",
    "<Alt>Tab",
    "<Shift><Alt>Tab",
    "<Control><Alt>Tab",
    "<Shift><Control><Alt>Tab",
    "<Control><Alt>Down",
    "<Control><Alt>Left",
    "<Control><Alt>Right",
    "<Control><Alt>Up",
    "<Alt>F10",
    "<Control><Alt>Delete",
    "<Shift><Control>u",
];

/// The other desktop bindings in the same dump that a root chord must not take: Alt+F5
/// (unmaximize), Ctrl+Alt+T (terminal), Ctrl+Alt+F1…F12 (Wayland session switch), Alt+Print.
const MORE_GNOME_BINDINGS: [&str; 5] =
    ["<Alt>F5", "<Control><Alt>t", "<Control><Alt>F1", "<Control><Alt>F12", "<Alt>Print"];

/// GtkTextView 4.22.4's own key bindings that use Control or Alt (gtktextview.c, class_init), in
/// `gtk_accelerator_name` spelling — the keys a window accelerator would take from the editor.
/// Ctrl+Z / Ctrl+Y / Shift+Ctrl+Z, Ctrl+X/C/V and Ctrl+A are here on purpose: the widget-owned Edit
/// rows show them and never register them.
const TEXT_VIEW_BINDINGS: [&str; 24] = [
    "<Control>Right",
    "<Control>Left",
    "<Control>Up",
    "<Control>Down",
    "<Control>Home",
    "<Control>End",
    "<Control>Page_Up",
    "<Control>Page_Down",
    "<Control>a",
    "<Control>slash",
    "<Control>backslash",
    "<Shift><Control>a",
    "<Control>Delete",
    "<Control>BackSpace",
    "<Shift><Control>Delete",
    "<Shift><Control>BackSpace",
    "<Control>x",
    "<Control>c",
    "<Control>v",
    "<Control>Insert",
    "<Control>z",
    "<Control>y",
    "<Shift><Control>z",
    "<Control>period",
];

#[test]
fn no_root_chord_is_one_the_desktop_or_the_editor_already_owns() {
    let taken: HashSet<&str> = GNOME_BINDINGS
        .iter()
        .chain(MORE_GNOME_BINDINGS.iter())
        .chain(TEXT_VIEW_BINDINGS.iter())
        .chain(["<Control>semicolon", "<Alt>Delete", "<Alt>BackSpace", "<Control>Tab", "<Shift><Control>Tab"].iter())
        .copied()
        .collect();
    for spec in command_table::root_accelerators() {
        let accelerator = spec.chord.unwrap().accelerator();
        assert!(!taken.contains(accelerator.as_str()), "{:?} takes {accelerator}", spec.id);
    }
    // The clash that moved Previous / Next Article off md.win's chords.
    assert!(taken.contains("<Control><Alt>Up") && taken.contains("<Control><Alt>Down"));
}

#[test]
fn the_widget_owned_rows_show_what_the_text_view_binds() {
    for id in
        [CommandId::Undo, CommandId::Redo, CommandId::Cut, CommandId::Copy, CommandId::Paste, CommandId::SelectAll]
    {
        let accelerator = command_table::spec(id).chord.unwrap().accelerator();
        assert!(TEXT_VIEW_BINDINGS.contains(&accelerator.as_str()), "{id:?}: {accelerator}");
    }
}

#[test]
fn the_article_tooltips_name_the_tables_chords() {
    let previous = command_table::spec(CommandId::PreviousArticle).chord.unwrap().display_text();
    let next = command_table::spec(CommandId::NextArticle).chord.unwrap().display_text();
    assert_eq!(books::PREVIOUS_ARTICLE_TOOLTIP, format!("The previous article in reading order ({previous})"));
    assert_eq!(books::NEXT_ARTICLE_TOOLTIP, format!("The next article in reading order ({next})"));
    assert!(books::EMPTY_MESSAGE.contains(&format!("{previous} and {next} move through the book")));
    assert!(!books::EMPTY_MESSAGE.contains("Ctrl+Alt"));
}

#[test]
fn every_command_has_a_unique_valid_action_name() {
    let mut seen = HashSet::new();
    for id in CommandId::ALL {
        let name = id.action_name();
        assert!(!name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
        assert!(!name.starts_with('-') && !name.ends_with('-') && !name.contains("--"), "{name}");
        assert!(seen.insert(name.clone()), "{name} twice");
    }
    assert_eq!(CommandId::SaveAs.action_name(), "save-as");
    assert_eq!(CommandId::ExportLaTeX.action_name(), "export-latex");
    assert_eq!(CommandId::ExportBookLaTeX.action_name(), "export-book-latex");
    assert_eq!(CommandId::Heading1.action_name(), "heading1");
    assert_eq!(CommandId::ViewEdit.action_name(), "view-edit");
}
