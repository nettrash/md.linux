//! `MenuBar` — a window's eight menus (`File · Edit · Format · View · Book · Go · Window · Help`)
//! as a `gtk::PopoverMenuBar` over a `gio::Menu` built from md-logic's `CommandTable::menus()`;
//! `refresh` rebuilds only the dynamic submenus whose rows changed (shell-design §2.1, §2.3).
//!
//! The tree is md-logic's: a row → a `gio::MenuItem` on its action ([`actions::detailed_name`]),
//! a separator → a new section, a submenu → a submenu row. GTK 4 draws a submenu row sensitive
//! whatever its `submenu-action` says, so a submenu with nothing live inside
//! (`command_enablement::is_submenu_enabled`: Open Recent ▸ with no entries, Contents ▸ with no
//! headings, Export Book ▸ with no book) is swapped in place for a greyed row on the
//! always-disabled [`actions::UNAVAILABLE`] action, and back when something inside comes alive. Every dynamic command (Open Recent, Examples, Diagram as SVG, PDF
//! Page Size, Contents, Notes, the Window list) owns one section of its own, refilled when its rows
//! change; View's full-screen row is replaced in place when its title flips. Titles are the
//! table's, verbatim — GTK reads a menu label's `_` as a mnemonic, so labels are escaped
//! ([`escape_mnemonic`]) and the eight menu titles alone carry one.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use md_core::document::page_size::PageSize;
use md_logic::commands::{
    CommandId, CommandKind, CommandSpec, MenuNode, MenuPath, ShellSnapshot, command_enablement, command_table,
};
use md_logic::text::note_preview;

use crate::app::Shell;
use crate::menus::actions;
use crate::windows::document_window::display_folder;
use crate::windows::host::WindowKind;

/// The eight menu titles with their mnemonics (the spike's spelling, §2.1).
fn menu_label(title: &str) -> String {
    match title {
        "File" => "_File",
        "Edit" => "_Edit",
        "Format" => "F_ormat",
        "View" => "_View",
        "Book" => "_Book",
        "Go" => "_Go",
        "Window" => "_Window",
        "Help" => "_Help",
        other => return escape_mnemonic(other),
    }
    .to_string()
}

/// A label GTK shows verbatim: every `_` doubled (GTK reads a single one as a mnemonic and drops
/// it — "Wrap Selection When Typing * _ ` $" would lose its underscore).
pub fn escape_mnemonic(text: &str) -> String {
    text.replace('_', "__")
}

/// The inverse of [`escape_mnemonic`] — what the user sees for a model label.
#[cfg(any(test, feature = "selftest"))]
pub fn displayed(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                chars.next();
                out.push('_');
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// One window's menu bar.
pub struct MenuBar {
    widget: gtk::PopoverMenuBar,
    /// The section each dynamic command fills.
    dynamic: HashMap<CommandId, gio::Menu>,
    /// View's full-screen row: its section and position.
    full_screen: Option<(gio::Menu, i32)>,
    submenus: Vec<Submenu>,
    last: RefCell<Option<ShellSnapshot>>,
}

/// A submenu row, and whether it is currently the live submenu or the greyed stand-in.
struct Submenu {
    path: MenuPath,
    title: &'static str,
    model: gio::Menu,
    section: gio::Menu,
    position: i32,
    live: Cell<bool>,
}

impl Submenu {
    fn set_live(&self, live: bool) {
        if self.live.replace(live) == live {
            return;
        }
        let label = escape_mnemonic(self.title);
        let item = if live {
            gio::MenuItem::new_submenu(Some(&label), &self.model)
        } else {
            gio::MenuItem::new(Some(&label), Some(&format!("win.{}", actions::UNAVAILABLE)))
        };
        self.section.remove(self.position);
        self.section.insert_item(self.position, &item);
    }
}

impl MenuBar {
    pub fn new(shell: &Rc<Shell>, kind: WindowKind) -> MenuBar {
        let _ = kind; // every window carries the same eight menus (§2.1); enablement differs
        let model = gio::Menu::new();
        let mut builder = Builder { dynamic: HashMap::new(), full_screen: None, submenus: Vec::new() };
        for root in command_table::menus() {
            let menu = gio::Menu::new();
            builder.fill(&root.children, &menu);
            model.append_submenu(Some(&menu_label(root.title)), &menu);
        }
        let bar = MenuBar {
            widget: gtk::PopoverMenuBar::from_model(Some(&model)),
            dynamic: builder.dynamic,
            full_screen: builder.full_screen,
            submenus: builder.submenus,
            last: RefCell::new(None),
        };
        // The two lists no snapshot carries: the bundled examples and the page sizes.
        if let Some(section) = bar.dynamic.get(&CommandId::Example) {
            for example in shell.examples.examples() {
                section.append_item(&row(&example.name, CommandId::Example, Some(example.file_name.to_variant())));
            }
        }
        if let Some(section) = bar.dynamic.get(&CommandId::PdfPageSize) {
            for size in PageSize::all() {
                section.append_item(&row(size.label, CommandId::PdfPageSize, Some(size.id.to_variant())));
            }
        }
        bar
    }

    pub fn widget(&self) -> gtk::PopoverMenuBar {
        self.widget.clone()
    }

    /// The whole model (the self-test walks it).
    #[cfg(feature = "selftest")]
    pub fn model(&self) -> Option<gio::MenuModel> {
        self.widget.menu_model()
    }

    /// Cheap when the snapshot equals the last one.
    pub fn refresh(&self, snapshot: &ShellSnapshot) {
        let last = self.last.borrow().clone();
        if last.as_ref() == Some(snapshot) {
            return;
        }
        let changed = |pick: fn(&ShellSnapshot) -> u64| last.as_ref().is_none_or(|l| pick(l) != pick(snapshot));
        if changed(|s| hash(&s.recent_entries)) {
            self.fill(CommandId::OpenRecentEntry, recent_rows(snapshot));
        }
        if changed(|s| hash(&s.diagrams)) {
            let rows = snapshot.diagrams.iter().map(|d| (d.menu_title.clone(), (d.ordinal as u32).to_variant()));
            self.fill(CommandId::ExportDiagramSvg, rows.collect());
        }
        if changed(|s| hash(&s.outline)) {
            let rows = snapshot.outline.iter().enumerate();
            self.fill(
                CommandId::Contents,
                rows.map(|(i, e)| (command_table::contents_row_title(e), (i as u32).to_variant())).collect(),
            );
        }
        if changed(|s| hash(&s.notes)) {
            let rows = snapshot.notes.iter().enumerate();
            self.fill(
                CommandId::Notes,
                rows.map(|(i, n)| (note_preview::of(&n.text), (i as u32).to_variant())).collect(),
            );
        }
        if changed(|s| hash(&s.window_titles.iter().map(|w| (w.id, &w.title)).collect::<Vec<_>>())) {
            let rows = snapshot.window_titles.iter().map(|w| (w.title.clone(), w.id.0.to_variant()));
            self.fill(CommandId::ActivateWindow, rows.collect());
        }
        if changed(|s| u64::from(s.is_full_screen))
            && let Some((section, position)) = &self.full_screen
        {
            let spec = command_table::spec(CommandId::FullScreen);
            section.remove(*position);
            section.insert_item(*position, &static_row(spec, command_table::display_title(spec.id, snapshot)));
        }
        for submenu in &self.submenus {
            submenu.set_live(command_enablement::is_submenu_enabled(&submenu.path, snapshot));
        }
        if let Some(window) = self.widget.root().and_downcast::<adw::ApplicationWindow>() {
            actions::refresh(&window, snapshot);
        }
        *self.last.borrow_mut() = Some(snapshot.clone());
    }

    /// Replaces a dynamic section's rows.
    fn fill(&self, id: CommandId, rows: Vec<(String, glib::Variant)>) {
        let Some(section) = self.dynamic.get(&id) else { return };
        section.remove_all();
        for (label, target) in rows {
            section.append_item(&row(&label, id, Some(target)));
        }
    }
}

/// A structural fingerprint of a list, to tell whether a dynamic section must be rebuilt.
fn hash(value: &impl std::hash::Hash) -> u64 {
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
    BuildHasherDefault::<DefaultHasher>::default().hash_one(value)
}

/// Open Recent's rows: the file name, and its folder after an em dash when two rows share a name
/// (a `GMenu` row has no tooltip, §6.6).
fn recent_rows(snapshot: &ShellSnapshot) -> Vec<(String, glib::Variant)> {
    let entries = &snapshot.recent_entries;
    entries
        .iter()
        .map(|entry| {
            let shared = entries.iter().filter(|other| other.name == entry.name).count() > 1;
            let label = if shared && !entry.folder.is_empty() {
                format!("{} {} {}", entry.name, md_logic::strings::EM_DASH, display_folder(&entry.folder))
            } else {
                entry.name.clone()
            };
            (label, entry.token.to_variant())
        })
        .collect()
}

/// A dynamic row: `label` on `id`'s action with `target`.
fn row(label: &str, id: CommandId, target: Option<glib::Variant>) -> gio::MenuItem {
    let shape = actions::shape(id).expect("a dynamic command has an action");
    let action = format!("{}.{}", shape.scope.prefix(), shape.name);
    let item = gio::MenuItem::new(Some(&escape_mnemonic(label)), None);
    item.set_action_and_target_value(Some(&action), target.as_ref());
    item
}

/// A static row of the table on its detailed action; GTK draws the chord from the application's
/// accelerators by itself.
fn static_row(spec: &CommandSpec, title: &str) -> gio::MenuItem {
    let detailed = actions::detailed_name(spec.id);
    gio::MenuItem::new(Some(&escape_mnemonic(title)), detailed.as_deref())
}

struct Builder {
    dynamic: HashMap<CommandId, gio::Menu>,
    full_screen: Option<(gio::Menu, i32)>,
    submenus: Vec<Submenu>,
}

impl Builder {
    /// `nodes` into `menu`: a section per separator group, each dynamic command in a section of its
    /// own, submenus recursively.
    fn fill(&mut self, nodes: &[MenuNode], menu: &gio::Menu) {
        let mut section = gio::Menu::new();
        let flush = |section: &mut gio::Menu| {
            if section.n_items() > 0 {
                menu.append_section(None, section);
                *section = gio::Menu::new();
            }
        };
        for node in nodes {
            if node.separator_before {
                flush(&mut section);
            }
            match node.command {
                Some(spec) => match spec.kind {
                    CommandKind::Item | CommandKind::Toggle => {
                        if spec.id == CommandId::FullScreen {
                            self.full_screen = Some((section.clone(), section.n_items()));
                        }
                        section.append_item(&static_row(spec, spec.title));
                    }
                    CommandKind::DynamicItems | CommandKind::DynamicToggles | CommandKind::DynamicRadios => {
                        flush(&mut section);
                        let rows = gio::Menu::new();
                        menu.append_section(None, &rows);
                        self.dynamic.insert(spec.id, rows);
                    }
                    CommandKind::WindowKey => {}
                },
                None => {
                    let submenu = gio::Menu::new();
                    self.fill(&node.children, &submenu);
                    let item = gio::MenuItem::new_submenu(Some(&escape_mnemonic(node.title)), &submenu);
                    self.submenus.push(Submenu {
                        path: node.path,
                        title: node.title,
                        model: submenu,
                        section: section.clone(),
                        position: section.n_items(),
                        live: Cell::new(true),
                    });
                    section.append_item(&item);
                }
            }
        }
        flush(&mut section);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_escape_underscores_and_read_back() {
        let title = "Wrap Selection When Typing * _ ` $";
        assert_eq!(escape_mnemonic(title), "Wrap Selection When Typing * __ ` $");
        assert_eq!(displayed(&escape_mnemonic(title)), title);
        assert_eq!(displayed("_File"), "File");
        assert_eq!(displayed("F_ormat"), "Format");
        assert_eq!(displayed(&escape_mnemonic("a__b_c")), "a__b_c");
    }

    #[test]
    fn the_eight_menus_carry_their_mnemonics() {
        let labels: Vec<String> = command_table::menus().iter().map(|m| menu_label(m.title)).collect();
        assert_eq!(labels, ["_File", "_Edit", "F_ormat", "_View", "_Book", "_Go", "_Window", "_Help"]);
    }
}
