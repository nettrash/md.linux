//! Port of md.win `Commands/CommandTable.cs`: every command declared once — the menus are built
//! from this table, the accelerators are installed from it, and
//! [`command_enablement`](super::command_enablement) answers for the same ids, so a command cannot
//! exist in one of the three and not the others.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::OnceLock;

use md_core::markdown::block::OutlineEntry;

use super::chord::{Chord, KeyModifiers, keys};
use super::command_id::{CommandId, CommandKind};
use super::shell_snapshot::ShellSnapshot;

/// Where a command sits in the bar: the top-level menu, then up to two nested submenus
/// ("File ▸ Export ▸ PDF Page Size"). Submenu *containers* are not table rows — they exist because
/// commands name them here, which keeps every title in one place and every id unique.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MenuPath {
    pub menu: &'static str,
    pub submenu: Option<&'static str>,
    pub nested: Option<&'static str>,
}

impl MenuPath {
    /// A top-level menu.
    pub const fn menu(menu: &'static str) -> MenuPath {
        MenuPath { menu, submenu: None, nested: None }
    }

    /// A submenu of a top-level menu.
    pub const fn submenu(menu: &'static str, submenu: &'static str) -> MenuPath {
        MenuPath { menu, submenu: Some(submenu), nested: None }
    }

    /// A submenu inside a submenu.
    pub const fn nested(menu: &'static str, submenu: &'static str, nested: &'static str) -> MenuPath {
        MenuPath { menu, submenu: Some(submenu), nested: Some(nested) }
    }

    /// 1, 2 or 3.
    pub fn depth(&self) -> usize {
        if self.nested.is_some() {
            3
        } else if self.submenu.is_some() {
            2
        } else {
            1
        }
    }

    /// The last segment — a submenu's own title, or the menu's.
    pub fn leaf(&self) -> &'static str {
        self.nested.or(self.submenu).unwrap_or(self.menu)
    }

    pub fn segments(&self) -> Vec<&'static str> {
        let mut segments = vec![self.menu];
        segments.extend(self.submenu);
        segments.extend(self.nested);
        segments
    }

    /// True when this path is strictly deeper than `parent` and starts with it.
    pub fn is_under(&self, parent: &MenuPath) -> bool {
        if self.depth() <= parent.depth() || self.menu != parent.menu {
            return false;
        }
        parent.depth() == 1 || self.submenu == parent.submenu
    }

    /// This path cut to `depth` segments — how a child's path names the submenu it goes into.
    pub fn truncate(&self, depth: usize) -> MenuPath {
        match depth {
            1 => MenuPath::menu(self.menu),
            2 => MenuPath { menu: self.menu, submenu: self.submenu, nested: None },
            _ => *self,
        }
    }
}

impl fmt::Display for MenuPath {
    /// The segments joined with " ▸ " (U+25B8).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.segments().join(" \u{25B8} "))
    }
}

/// One row of the tables in shell-design §2. `title` is the Mac's own wording, byte for byte
/// (U+2026 ellipsis); for the dynamic kinds it is the *submenu's* title and the rows inside get
/// their titles from the snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CommandSpec {
    /// The command; unique across the table.
    pub id: CommandId,
    /// The menu row's text, verbatim.
    pub title: &'static str,
    /// The shortcut shown next to the row; `None` for the many rows that have none.
    pub chord: Option<Chord>,
    /// Where the row goes; `None` only for [`CommandKind::Accelerator`], which has no row.
    pub path: Option<MenuPath>,
    pub kind: CommandKind,
    /// A command md.macOS does not define as its own row — md.win's `WindowsOnly`, the design's
    /// "(Win)" mark, kept for the same rows.
    pub port_only: bool,
    /// Rows sharing a group sit together; a change of group between siblings is a separator.
    /// Numbers are per parent menu, in ascending order.
    pub group: u8,
    /// False for the chords §2.4 deliberately leaves with the focused widget (Ctrl+Z, Shift+Ctrl+Z,
    /// Ctrl+X/C/V/A and Delete): they are *shown* in the Edit menu (the menu item's `accel`
    /// attribute) but never registered with `set_accels_for_action`, so `GtkTextView` and the
    /// `WebKitWebView` keep their own editing keys.
    pub root_accelerator: bool,
}

/// The menu bar as a tree, derived from [`all`]: eight roots in bar order, each holding its rows
/// and submenus in table order with the separators already worked out. The GTK builder walks this
/// and does nothing but create `GMenu` sections and items (§2.9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuNode {
    /// The row's or submenu's text.
    pub title: &'static str,
    /// The path of the command, or of the submenu itself.
    pub path: MenuPath,
    /// The command this row runs; `None` for a submenu container and for the eight roots.
    pub command: Option<&'static CommandSpec>,
    /// A submenu's contents, in order; empty for a command row.
    pub children: Vec<MenuNode>,
    /// A separator goes in front of this row (in a `GMenu`: the row starts a new section).
    pub separator_before: bool,
}

/// The eight menus, in bar order. The Mac's bar without its app menu (§2.1): About moved to Help,
/// Quit to the end of File. Format (1.6) sits after Edit.
pub const MENU_TITLES: [&str; 8] = ["File", "Edit", "Format", "View", "Book", "Go", "Window", "Help"];

/// View's full-screen row while the window is full screen (§2.5); [`CommandId::FullScreen`]'s
/// table title is the other half.
pub const EXIT_FULL_SCREEN_TITLE: &str = "Exit Full Screen";

// Paths, named once so a typo cannot split a submenu in two.
const FILE: MenuPath = MenuPath::menu("File");
const FILE_OPEN_RECENT: MenuPath = MenuPath::submenu("File", "Open Recent");
const FILE_EXAMPLES: MenuPath = MenuPath::submenu("File", "Examples");
const FILE_EXPORT: MenuPath = MenuPath::submenu("File", "Export");
const FILE_EXPORT_DIAGRAMS: MenuPath = MenuPath::nested("File", "Export", "Diagram as SVG");
const FILE_EXPORT_PAGE_SIZE: MenuPath = MenuPath::nested("File", "Export", "PDF Page Size");
const EDIT: MenuPath = MenuPath::menu("Edit");
const EDIT_TYPING: MenuPath = MenuPath::submenu("Edit", "Typing");
const FORMAT: MenuPath = MenuPath::menu("Format");
const FORMAT_HEADING: MenuPath = MenuPath::submenu("Format", "Heading");
const VIEW: MenuPath = MenuPath::menu("View");
const BOOK: MenuPath = MenuPath::menu("Book");
const BOOK_EXPORT: MenuPath = MenuPath::submenu("Book", "Export Book");
const GO: MenuPath = MenuPath::menu("Go");
const GO_CONTENTS: MenuPath = MenuPath::submenu("Go", "Contents");
const GO_NOTES: MenuPath = MenuPath::submenu("Go", "Notes");
const WINDOW: MenuPath = MenuPath::menu("Window");
const HELP: MenuPath = MenuPath::menu("Help");

const CTRL: KeyModifiers = KeyModifiers::CTRL;
const CTRL_SHIFT: KeyModifiers = KeyModifiers::CTRL.with(KeyModifiers::SHIFT);
const CTRL_ALT: KeyModifiers = KeyModifiers::CTRL.with(KeyModifiers::ALT);
const ALT: KeyModifiers = KeyModifiers::ALT;

const fn chord(key: &'static str, modifiers: KeyModifiers) -> Option<Chord> {
    Some(Chord::new(key, modifiers))
}

/// A row in a menu, registered on the window (the common case).
const fn row(
    id: CommandId,
    title: &'static str,
    chord: Option<Chord>,
    path: MenuPath,
    kind: CommandKind,
    port_only: bool,
    group: u8,
) -> CommandSpec {
    CommandSpec { id, title, chord, path: Some(path), kind, port_only, group, root_accelerator: true }
}

/// An Edit row whose chord belongs to the focused widget: shown, never registered.
const fn widget_owned(id: CommandId, title: &'static str, chord: Option<Chord>, group: u8) -> CommandSpec {
    CommandSpec {
        id,
        title,
        chord,
        path: Some(EDIT),
        kind: CommandKind::Item,
        port_only: false,
        group,
        root_accelerator: false,
    }
}

use CommandId as C;
use CommandKind as K;

/// The table. Row order is menu order; `group` jumps mark the dividers §2 draws. Titles carry the
/// Mac's U+2026 ellipsis, written as an escape so no editor can quietly swap it for three dots.
///
/// LINUX DECISIONS (each chord checked against GtkTextView 4.22's own bindings and the GNOME 50 /
/// Ubuntu 26.04 defaults — mutter, gnome-shell, gnome-settings-daemon, IBus, the Ubuntu dock):
/// - Previous / Next Article are **Alt+Up / Alt+Down**. The Mac's ⌃⌘↑/↓ became Ctrl+Alt+Up/Down
///   on Windows, which GNOME binds to `switch-to-workspace-up/down`; Alt+Up/Down is md.win's own
///   documented fallback, and nothing on the desktop or in GtkTextView claims it (GtkTextView binds
///   Alt+arrows only on macOS builds).
/// - Find Next / Find Previous are **Ctrl+G / Shift+Ctrl+G**, the Mac's ⌘G / ⇧⌘G and GNOME's own
///   (GtkSearchEntry's next-match / previous-match), instead of Windows' F3 / Shift+F3.
/// - Redo shows **Shift+Ctrl+Z**, GNOME's spelling (GtkTextView binds it and Ctrl+Y alike).
/// - **Quit md** (the Mac's own row) with GNOME's **Ctrl+Q** replaces Windows' chordless Exit.
/// - Zen Mode is Shift+Ctrl+Return (GTK's name for Enter), Full Screen F11, Help F1 — free in GNOME.
/// - The Share rows are gone (no share sheet on Linux).
static ALL: [CommandSpec; 79] = [
    // ── File (§2.2) ───────────────────────────────────────────────────────────────────────
    row(C::New, "New", chord("n", CTRL), FILE, K::Item, false, 0),
    row(C::Open, "Open\u{2026}", chord("o", CTRL), FILE, K::Item, false, 0),
    row(C::OpenRecentEntry, "Open Recent", None, FILE_OPEN_RECENT, K::DynamicItems, false, 0),
    row(C::ClearRecent, "Clear Menu", None, FILE_OPEN_RECENT, K::Item, false, 1),
    row(C::OpenTextBundleFolder, "Open TextBundle Folder\u{2026}", None, FILE, K::Item, true, 0),
    row(C::Example, "Examples", None, FILE_EXAMPLES, K::DynamicItems, false, 0),
    row(C::ExampleBook, "Example Book\u{2026}", None, FILE_EXAMPLES, K::Item, false, 1),
    row(C::Close, "Close", chord("w", CTRL), FILE, K::Item, false, 1),
    row(C::Save, "Save", chord("s", CTRL), FILE, K::Item, false, 1),
    row(C::SaveAs, "Save As\u{2026}", chord("s", CTRL_SHIFT), FILE, K::Item, false, 1),
    row(C::Duplicate, "Duplicate", None, FILE, K::Item, false, 1),
    row(C::Rename, "Rename\u{2026}", None, FILE, K::Item, false, 1),
    row(C::MoveTo, "Move To\u{2026}", None, FILE, K::Item, false, 1),
    row(C::RevertToSaved, "Revert to Saved", None, FILE, K::Item, false, 1),
    row(C::Print, "Print\u{2026}", chord("p", CTRL), FILE, K::Item, false, 2),
    row(C::ExportPdf, "PDF\u{2026}", None, FILE_EXPORT, K::Item, false, 0),
    row(C::ExportHtml, "HTML\u{2026}", None, FILE_EXPORT, K::Item, false, 0),
    row(C::ExportEpub, "EPUB\u{2026}", None, FILE_EXPORT, K::Item, false, 0),
    row(C::ExportLaTeX, "LaTeX\u{2026}", None, FILE_EXPORT, K::Item, false, 0),
    row(C::ExportTextBundle, "TextBundle\u{2026}", None, FILE_EXPORT, K::Item, false, 0),
    row(C::ExportDiagramSvg, "Diagram as SVG", None, FILE_EXPORT_DIAGRAMS, K::DynamicItems, false, 0),
    row(C::PdfPageSize, "PDF Page Size", None, FILE_EXPORT_PAGE_SIZE, K::DynamicRadios, false, 0),
    row(C::Quit, "Quit md", chord("q", CTRL), FILE, K::Item, true, 4),
    // ── Edit (§2.4) ───────────────────────────────────────────────────────────────────────
    // Ctrl+Z, Shift+Ctrl+Z, Ctrl+X/C/V/A and Delete are shown but never registered: the focused
    // widget owns them.
    widget_owned(C::Undo, "Undo", chord("z", CTRL), 0),
    widget_owned(C::Redo, "Redo", chord("z", CTRL_SHIFT), 0),
    widget_owned(C::Cut, "Cut", chord("x", CTRL), 1),
    widget_owned(C::Copy, "Copy", chord("c", CTRL), 1),
    widget_owned(C::Paste, "Paste", chord("v", CTRL), 1),
    widget_owned(C::Delete, "Delete", Some(Chord::key(keys::DELETE)), 1),
    widget_owned(C::SelectAll, "Select All", chord("a", CTRL), 1),
    row(C::Find, "Find\u{2026}", chord("f", CTRL), EDIT, K::Item, true, 2),
    row(C::FindNext, "Find Next", chord("g", CTRL), EDIT, K::Item, true, 2),
    row(C::FindPrevious, "Find Previous", chord("g", CTRL_SHIFT), EDIT, K::Item, true, 2),
    row(C::Replace, "Replace\u{2026}", chord("h", CTRL), EDIT, K::Item, true, 2),
    row(C::UseSelectionForFind, "Use Selection for Find", chord("e", CTRL), EDIT, K::Item, true, 2),
    // Typing (smart-typing.md §3.1): settings, ticked from the snapshot, under the same "Typing"
    // heading and worded identically on every md port — a static submenu of toggles.
    row(C::ContinueLists, "Continue Lists and Tables", None, EDIT_TYPING, K::Toggle, false, 0),
    row(C::CapitalizeSentences, "Capitalize Sentences", None, EDIT_TYPING, K::Toggle, false, 0),
    // formatting.md §4: typing * _ ` $ over a selection wraps it — a setting like the two above.
    row(C::WrapSelection, "Wrap Selection When Typing * _ ` $", None, EDIT_TYPING, K::Toggle, false, 0),
    // ── Format (1.6, formatting.md §5) ────────────────────────────────────────────────────
    // Every row is one undoable edit of the editor's text; the chords are the family's.
    row(C::Bold, "Bold", chord("b", CTRL), FORMAT, K::Item, false, 0),
    row(C::Italic, "Italic", chord("i", CTRL), FORMAT, K::Item, false, 0),
    row(C::InlineCode, "Code", chord("c", CTRL_SHIFT), FORMAT, K::Item, false, 0),
    row(C::Strikethrough, "Strikethrough", chord("x", CTRL_SHIFT), FORMAT, K::Item, false, 0),
    row(C::Link, "Link", chord("k", CTRL), FORMAT, K::Item, false, 0),
    row(C::Heading1, "Heading 1", chord("1", CTRL_ALT), FORMAT_HEADING, K::Item, false, 0),
    row(C::Heading2, "Heading 2", chord("2", CTRL_ALT), FORMAT_HEADING, K::Item, false, 0),
    row(C::Heading3, "Heading 3", chord("3", CTRL_ALT), FORMAT_HEADING, K::Item, false, 0),
    row(C::Heading4, "Heading 4", chord("4", CTRL_ALT), FORMAT_HEADING, K::Item, false, 0),
    row(C::Heading5, "Heading 5", chord("5", CTRL_ALT), FORMAT_HEADING, K::Item, false, 0),
    row(C::Heading6, "Heading 6", chord("6", CTRL_ALT), FORMAT_HEADING, K::Item, false, 0),
    row(C::BulletList, "Bullet List", chord("l", CTRL_SHIFT), FORMAT, K::Item, false, 2),
    row(C::TaskList, "Task List", chord("t", CTRL_SHIFT), FORMAT, K::Item, false, 2),
    row(C::Quote, "Quote", chord("q", CTRL_SHIFT), FORMAT, K::Item, false, 2),
    row(C::Indent, "Indent", chord(keys::BRACKET_RIGHT, CTRL), FORMAT, K::Item, false, 3),
    row(C::Outdent, "Outdent", chord(keys::BRACKET_LEFT, CTRL), FORMAT, K::Item, false, 3),
    // ── View (§2.5) ───────────────────────────────────────────────────────────────────────
    row(C::ViewEdit, "Edit", chord("1", CTRL), VIEW, K::Toggle, false, 0),
    row(C::ViewSplit, "Split", chord("2", CTRL), VIEW, K::Toggle, false, 0),
    row(C::ViewPreview, "Preview", chord("3", CTRL), VIEW, K::Toggle, false, 0),
    row(C::ZenMode, "Zen Mode", chord(keys::RETURN, CTRL_SHIFT), VIEW, K::Toggle, false, 0),
    row(C::ShowSidebar, "Show Sidebar", None, VIEW, K::Toggle, false, 1),
    row(C::FullScreen, "Enter Full Screen", Some(Chord::key(keys::F11)), VIEW, K::Item, false, 1),
    // ── Book (§2.6) ───────────────────────────────────────────────────────────────────────
    row(C::NewBook, "New Book\u{2026}", None, BOOK, K::Item, false, 0),
    row(C::OpenBook, "Open Book\u{2026}", None, BOOK, K::Item, false, 0),
    row(C::ShowBook, "Show Book", chord("b", CTRL_SHIFT), BOOK, K::Item, false, 0),
    row(C::CloseBook, "Close Book", None, BOOK, K::Item, false, 0),
    row(C::PrintBook, "Print Book\u{2026}", None, BOOK, K::Item, false, 1),
    row(C::ExportBookPdf, "PDF\u{2026}", None, BOOK_EXPORT, K::Item, false, 0),
    row(C::ExportBookEpub, "EPUB\u{2026}", None, BOOK_EXPORT, K::Item, false, 0),
    row(C::ExportBookLaTeX, "LaTeX\u{2026}", None, BOOK_EXPORT, K::Item, false, 0),
    // ── Go (§2.7) ─────────────────────────────────────────────────────────────────────────
    row(C::PreviousArticle, "Previous Article", chord(keys::UP, ALT), GO, K::Item, false, 0),
    row(C::NextArticle, "Next Article", chord(keys::DOWN, ALT), GO, K::Item, false, 0),
    row(C::Contents, "Contents", None, GO_CONTENTS, K::DynamicItems, false, 0),
    row(C::Notes, "Notes", None, GO_NOTES, K::DynamicItems, false, 0),
    // ── Window (§2.8) ─────────────────────────────────────────────────────────────────────
    row(C::Minimize, "Minimize", None, WINDOW, K::Item, true, 0),
    row(C::Zoom, "Zoom", None, WINDOW, K::Item, true, 0),
    // Title never shown: the rows carry the windows' own titles.
    row(C::ActivateWindow, "Window List", None, WINDOW, K::DynamicToggles, true, 1),
    // ── Help (§2.8) ───────────────────────────────────────────────────────────────────────
    row(C::Help, "md Help", Some(Chord::key(keys::F1)), HELP, K::Item, false, 0),
    row(C::PrivacyPolicy, "Privacy Policy", None, HELP, K::Item, false, 0),
    row(C::About, "About md", None, HELP, K::Item, false, 1),
    // ── No menu row (§2.9) ────────────────────────────────────────────────────────────────
    CommandSpec {
        id: C::Escape,
        title: "Escape",
        chord: Some(Chord::key(keys::ESCAPE)),
        path: None,
        kind: K::Accelerator,
        port_only: true,
        group: 0,
        root_accelerator: true,
    },
];

/// The table, in menu order.
pub fn all() -> &'static [CommandSpec] {
    &ALL
}

/// The rows the shell registers as window accelerators — a chord, and not one §2.4 leaves to the
/// focused widget.
pub fn root_accelerators() -> &'static [&'static CommandSpec] {
    static ROOT: OnceLock<Vec<&'static CommandSpec>> = OnceLock::new();
    ROOT.get_or_init(|| ALL.iter().filter(|s| s.chord.is_some() && s.root_accelerator).collect())
}

/// The root command a chord runs, or `None`: an editing key §2.4 leaves to the focused widget, or
/// no command's chord at all.
pub fn root_accelerator_for(chord: Chord) -> Option<CommandId> {
    static BY_CHORD: OnceLock<HashMap<Chord, CommandId>> = OnceLock::new();
    BY_CHORD
        .get_or_init(|| root_accelerators().iter().filter_map(|s| s.chord.map(|c| (c, s.id))).collect())
        .get(&chord)
        .copied()
}

/// The row for `id` (md.win's `CommandTable.For`).
///
/// # Panics
///
/// When the table and the enum disagree — a build-time bug, pinned by a test.
pub fn spec(id: CommandId) -> &'static CommandSpec {
    ALL.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("No CommandSpec for {id:?}"))
}

/// The eight menus in bar order, each a tree of rows, submenus and separators.
pub fn menus() -> &'static [MenuNode] {
    static MENUS: OnceLock<Vec<MenuNode>> = OnceLock::new();
    MENUS.get_or_init(build_menus)
}

/// The row's text for this snapshot: the table title, except View's full-screen row, which the
/// Mac's system item spells both ways (§1.3, §2.5).
pub fn display_title(id: CommandId, snapshot: &ShellSnapshot) -> &'static str {
    if id == CommandId::FullScreen && snapshot.is_full_screen { EXIT_FULL_SCREEN_TITLE } else { spec(id).title }
}

/// A Go ▸ Contents row: two spaces of indent per heading level below 1, then the heading text —
/// the Mac's `String(repeating: "  ", count: max(0, entry.level - 1)) + entry.text`. (md-core's
/// level is unsigned, so md.win's negative-level case cannot arise; level 0 indents nothing.)
pub fn contents_row_title(entry: &OutlineEntry) -> String {
    format!("{}{}", "  ".repeat(entry.level.saturating_sub(1)), entry.text)
}

/// Where a submenu sits among its siblings: its group, in the parent's numbering. Its *position*
/// is where its first command appears in [`all`]; only the divider needs saying here.
fn submenu_group(path: &MenuPath) -> u8 {
    match *path {
        p if p == FILE_OPEN_RECENT || p == FILE_EXAMPLES => 0,
        p if p == FILE_EXPORT => 3,
        p if p == FILE_EXPORT_DIAGRAMS => 1,
        p if p == FILE_EXPORT_PAGE_SIZE => 2,
        p if p == EDIT_TYPING => 3,
        p if p == FORMAT_HEADING || p == BOOK_EXPORT || p == GO_CONTENTS || p == GO_NOTES => 1,
        _ => 0,
    }
}

fn build_menus() -> Vec<MenuNode> {
    MENU_TITLES
        .iter()
        .map(|&title| {
            let path = MenuPath::menu(title);
            MenuNode { title, path, command: None, children: children(&path), separator_before: false }
        })
        .collect()
}

fn children(parent: &MenuPath) -> Vec<MenuNode> {
    // One pass over the table keeps every sibling in declaration order; a submenu takes the place
    // of its first command.
    let mut slots: Vec<(MenuPath, Option<&'static CommandSpec>, u8)> = Vec::new();
    let mut opened: HashSet<MenuPath> = HashSet::new();
    for spec in ALL.iter() {
        let Some(path) = spec.path else { continue };
        if path == *parent {
            slots.push((*parent, Some(spec), spec.group));
        } else if path.is_under(parent) {
            let child = path.truncate(parent.depth() + 1);
            if opened.insert(child) {
                slots.push((child, None, submenu_group(&child)));
            }
        }
    }

    let mut nodes: Vec<MenuNode> = Vec::with_capacity(slots.len());
    let mut previous_group = 0;
    for (path, spec, group) in slots {
        let separator_before = !nodes.is_empty() && group != previous_group;
        nodes.push(match spec {
            Some(spec) => {
                MenuNode { title: spec.title, path, command: Some(spec), children: Vec::new(), separator_before }
            }
            None => MenuNode { title: path.leaf(), path, command: None, children: children(&path), separator_before },
        });
        previous_group = group;
    }
    nodes
}
