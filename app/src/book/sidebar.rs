//! The Book sidebar (shell-design §8.3): a `gtk::ListBox` of md-logic `BookSidebarModel` rows —
//! root articles, "New Article…", chapter headers and their articles — the "New Chapter…" button,
//! and the per-row context menu over the window's `book.` action group.
//!
//! What the rows ARE (which exist, in which order, what each is called, which of Move Up / Move
//! Down is live) is md-logic's answer; every operation ends in `BookNavigatorModel`, which renames,
//! renumbers and deletes through md-core's `book_folder` / `renumber_plan` and re-lists. This file
//! only turns rows into widgets and gestures into those calls.
//!
//! The Mac's lesson, kept: no per-row click handler that races the list's own selection. A
//! primary click is the `ListBox`'s (selection → the navigator); the secondary click and a long
//! press only open the menu; the one row with a handler of its own is "New Article…", a button that
//! is not selectable at all.
//!
//! The actions, all on the window's `book` group (`book.<name>`, target the item's path):
//!
//! | Action | Does |
//! | --- | --- |
//! | `open-in-window(s)` | the article in its own document window (also double-click / Enter) |
//! | `rename(s)` | prompt, then `perform_rename` (the ordering prefix and extension are kept) |
//! | `move-up(s)`, `move-down(s)` | renumber the sibling group (`move_by` −1 / +1) |
//! | `delete(s)` | confirm, then `perform_delete` (the selection falls to the neighbour) |
//! | `new-article(s)` | prompt, then `create_article` in that folder (target: the folder) |
//! | `new-chapter` | prompt, then `create_chapter` in the book root |

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use md_core::book::book_model::Book;
use md_core::book::{book_naming, book_paths};
use md_logic::books::book_sidebar_model;
use md_logic::books::{BookNavigatorModel, BookRow, BookRowKind};
use md_logic::seams::Alerts;
use md_logic::strings::{books, buttons, documents};

/// The `book.` actions this file installs (see the module table).
pub const OPEN_IN_WINDOW: &str = "open-in-window";
pub const RENAME: &str = "rename";
pub const MOVE_UP: &str = "move-up";
pub const MOVE_DOWN: &str = "move-down";
pub const DELETE: &str = "delete";
pub const NEW_ARTICLE: &str = "new-article";
pub const NEW_CHAPTER: &str = "new-chapter";

/// The sidebar's width: the Mac's `navigationSplitViewColumnWidth(min: 200, ideal: 240, max: 320)`.
pub const WIDTH_MIN: i32 = 200;
pub const WIDTH_IDEAL: i32 = 240;
pub const WIDTH_MAX: i32 = 320;

/// One built row: what it is and the widget that shows it.
struct Built {
    row: BookRow,
    widget: gtk::ListBoxRow,
}

/// The Book window's sidebar.
pub struct BookSidebar {
    me: Weak<BookSidebar>,
    navigator: Rc<BookNavigatorModel>,
    alerts: Rc<dyn Alerts>,
    root: gtk::Box,
    scrolled: gtk::ScrolledWindow,
    list: gtk::ListBox,
    built: RefCell<Vec<Built>>,
    rendered: RefCell<Option<Option<Book>>>,
    /// Set while the list is changed from code, so `row-selected` does not echo to the navigator.
    syncing: Cell<bool>,
    move_up: gio::SimpleAction,
    move_down: gio::SimpleAction,
}

impl BookSidebar {
    /// The sidebar over `navigator`; its actions go into `actions` (the window's `book` group) and
    /// its prompts through `alerts` (the window's).
    pub fn new(
        navigator: Rc<BookNavigatorModel>,
        alerts: Rc<dyn Alerts>,
        actions: &gio::SimpleActionGroup,
    ) -> Rc<Self> {
        let list =
            gtk::ListBox::builder().selection_mode(gtk::SelectionMode::Single).activate_on_single_click(false).build();
        list.add_css_class("navigation-sidebar");
        let scrolled =
            gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).vexpand(true).child(&list).build();

        let new_chapter = gtk::Button::builder()
            .child(&icon_label("folder-new-symbolic", books::NEW_CHAPTER_BUTTON, "md-sidebar-row").0)
            .tooltip_text(books::NEW_CHAPTER_TOOLTIP)
            .action_name(format!("book.{NEW_CHAPTER}"))
            .halign(gtk::Align::Start)
            .margin_start(6)
            .margin_end(6)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        new_chapter.add_css_class("flat");

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("sidebar-pane");
        root.set_size_request(WIDTH_MIN, -1);
        root.append(&new_chapter);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        root.append(&scrolled);

        let path_action = |name: &str| gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        let move_up = path_action(MOVE_UP);
        let move_down = path_action(MOVE_DOWN);

        let sidebar = Rc::new_cyclic(|me: &Weak<BookSidebar>| BookSidebar {
            me: me.clone(),
            navigator,
            alerts,
            root,
            scrolled,
            list,
            built: RefCell::new(Vec::new()),
            rendered: RefCell::new(None),
            syncing: Cell::new(false),
            move_up: move_up.clone(),
            move_down: move_down.clone(),
        });

        // ---- the actions ----
        let with_path = |action: &gio::SimpleAction, run: fn(&BookSidebar, String)| {
            let weak = Rc::downgrade(&sidebar);
            action.connect_activate(move |_, target| {
                if let (Some(sidebar), Some(path)) = (weak.upgrade(), target.and_then(|t| t.get::<String>())) {
                    run(&sidebar, path);
                }
            });
            actions.add_action(action);
        };
        with_path(&path_action(OPEN_IN_WINDOW), |s, path| {
            s.navigator.open_in_window(&path);
        });
        with_path(&path_action(RENAME), |s, path| s.prompt_rename(path));
        with_path(&move_up, |s, path| s.move_item(&path, -1));
        with_path(&move_down, |s, path| s.move_item(&path, 1));
        with_path(&path_action(DELETE), |s, path| s.prompt_delete(path));
        with_path(&path_action(NEW_ARTICLE), |s, folder| s.prompt_new_article(folder));
        let new_chapter_action = gio::SimpleAction::new(NEW_CHAPTER, None);
        let weak = Rc::downgrade(&sidebar);
        new_chapter_action.connect_activate(move |_, _| {
            if let Some(sidebar) = weak.upgrade() {
                sidebar.prompt_new_chapter();
            }
        });
        actions.add_action(&new_chapter_action);

        // ---- the list ----
        let weak = Rc::downgrade(&sidebar);
        sidebar.list.connect_row_selected(move |_, row| {
            if let Some(sidebar) = weak.upgrade() {
                sidebar.on_row_selected(row);
            }
        });
        let weak = Rc::downgrade(&sidebar);
        sidebar.list.connect_row_activated(move |_, row| {
            let Some(sidebar) = weak.upgrade() else { return };
            // Double-click / Enter on an article: Open in New Window (§8.3).
            let path = sidebar.row_for(row).filter(BookRow::is_article).and_then(|r| r.path);
            if let Some(path) = path {
                sidebar.navigator.open_in_window(&path);
            }
        });
        sidebar
    }

    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// Rebuild the rows when the listing changed, then put the highlight where the navigator says.
    pub fn render(&self, book: Option<&Book>, selection: Option<&str>) {
        let stale = self.rendered.borrow().as_ref().is_none_or(|rendered| rendered.as_ref() != book);
        if stale {
            self.rebuild(book);
            *self.rendered.borrow_mut() = Some(book.cloned());
        }
        self.sync_selection(selection);
    }

    fn rebuild(&self, book: Option<&Book>) {
        self.syncing.set(true);
        self.list.remove_all();
        let mut built = Vec::new();
        for row in book_sidebar_model::rows(book) {
            let widget = match row.kind {
                BookRowKind::RootArticle | BookRowKind::ChapterArticle => self.article_row(&row),
                BookRowKind::ChapterHeader => self.header_row(&row),
                BookRowKind::NewArticle => new_article_row(&row),
            };
            self.list.append(&widget);
            built.push(Built { row, widget });
        }
        *self.built.borrow_mut() = built;
        self.syncing.set(false);
    }

    fn article_row(&self, row: &BookRow) -> gtk::ListBoxRow {
        let (content, _) = icon_label("text-x-generic-symbolic", &row.title, "md-sidebar-row");
        let widget = gtk::ListBoxRow::builder().child(&content).selectable(true).activatable(true).build();
        self.attach_menu(&widget, row.path.clone().unwrap_or_default(), true);
        widget
    }

    fn header_row(&self, row: &BookRow) -> gtk::ListBoxRow {
        let label = gtk::Label::builder()
            .label(&row.title)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .margin_top(10)
            .margin_start(6)
            .build();
        label.add_css_class("md-chapter-header");
        label.add_css_class("heading");
        // A chapter header is a label and a menu, not a destination (§8.3).
        let widget = gtk::ListBoxRow::builder().child(&label).selectable(false).activatable(false).build();
        self.attach_menu(&widget, row.path.clone().unwrap_or_default(), false);
        widget
    }

    /// The secondary click and a long press open the row's menu. Neither touches the selection.
    fn attach_menu(&self, widget: &gtk::ListBoxRow, path: String, is_article: bool) {
        let click = gtk::GestureClick::builder().button(gtk::gdk::BUTTON_SECONDARY).build();
        let (weak, row, target) = (self.me.clone(), widget.clone(), path.clone());
        click.connect_pressed(move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(sidebar) = weak.upgrade() {
                sidebar.popup_menu(&row, &target, is_article, x, y);
            }
        });
        widget.add_controller(click);

        let press = gtk::GestureLongPress::new();
        let (weak, row) = (self.me.clone(), widget.clone());
        press.connect_pressed(move |gesture, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(sidebar) = weak.upgrade() {
                sidebar.popup_menu(&row, &path, is_article, x, y);
            }
        });
        widget.add_controller(press);
    }

    /// The management menu (md.win §8.3): Open in New Window (articles only) · Rename…, Move Up,
    /// Move Down · Delete…. Move Up / Move Down are dead at the ends of the sibling group.
    fn popup_menu(&self, row: &gtk::ListBoxRow, path: &str, is_article: bool, x: f64, y: f64) {
        let commands = book_sidebar_model::commands_for(self.navigator.book().as_ref(), Some(path));
        self.move_up.set_enabled(commands.as_ref().is_some_and(|c| c.can_move_up()));
        self.move_down.set_enabled(commands.as_ref().is_some_and(|c| c.can_move_down()));

        let popover = gtk::PopoverMenu::from_model(Some(&menu_model(path, is_article)));
        popover.set_parent(row);
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        let (up, down) = (self.move_up.clone(), self.move_down.clone());
        popover.connect_closed(move |popover| {
            // After the item's action has run: unparenting inside `closed` can drop it. The two
            // actions are shared by every row, so they go back to live for the next caller; a
            // move that cannot happen is refused by `move_item` itself.
            let (popover, up, down) = (popover.clone(), up.clone(), down.clone());
            glib::idle_add_local_once(move || {
                popover.unparent();
                up.set_enabled(true);
                down.set_enabled(true);
            });
        });
        popover.popup();
    }

    /// Move Up (`delta` −1) / Move Down (+1), when the item is not already at that end of its
    /// sibling group.
    fn move_item(&self, path: &str, delta: isize) {
        let commands = book_sidebar_model::commands_for(self.navigator.book().as_ref(), Some(path));
        let possible = commands.is_some_and(|c| if delta < 0 { c.can_move_up() } else { c.can_move_down() });
        if possible {
            self.navigator.move_by(path, delta);
        }
    }

    /// Put the list's highlight on `selection` (or nowhere) without telling the navigator.
    fn sync_selection(&self, selection: Option<&str>) {
        let rows: Vec<BookRow> = self.built.borrow().iter().map(|b| b.row.clone()).collect();
        let index = book_sidebar_model::index_of_article(&rows, selection);
        let current = self.list.selected_row().map(|row| row.index());
        let wanted = index.map(|i| i as i32);
        if current == wanted {
            return;
        }
        self.syncing.set(true);
        match index.and_then(|i| self.built.borrow().get(i).map(|b| b.widget.clone())) {
            Some(widget) => {
                self.list.select_row(Some(&widget));
                self.scroll_into_view(&widget);
            }
            None => self.list.unselect_all(),
        }
        self.syncing.set(false);
    }

    /// Scroll just enough to show `widget` once it is laid out (an article picked by a step may be
    /// below the fold).
    fn scroll_into_view(&self, widget: &gtk::ListBoxRow) {
        let (scrolled, list, widget) = (self.scrolled.clone(), self.list.clone(), widget.clone());
        glib::idle_add_local_once(move || {
            let Some(bounds) = widget.compute_bounds(&list) else { return };
            let adjustment = scrolled.vadjustment();
            let (top, bottom) = (f64::from(bounds.y()), f64::from(bounds.y() + bounds.height()));
            let (value, page) = (adjustment.value(), adjustment.page_size());
            if top < value {
                adjustment.set_value(top);
            } else if bottom > value + page {
                adjustment.set_value(bottom - page);
            }
        });
    }

    fn on_row_selected(&self, row: Option<&gtk::ListBoxRow>) {
        if self.syncing.get() {
            return;
        }
        let path = row.and_then(|row| self.row_for(row)).filter(BookRow::is_article).and_then(|r| r.path);
        match path {
            Some(path) => self.navigator.select(Some(&path)),
            // Only articles carry the selection; anything else puts the highlight straight back.
            None => self.sync_selection(self.navigator.selection().as_deref()),
        }
    }

    fn row_for(&self, widget: &gtk::ListBoxRow) -> Option<BookRow> {
        self.built.borrow().iter().find(|b| b.widget == *widget).map(|b| b.row.clone())
    }

    // ---- the prompts (md.win §8.3, verbatim) ----

    fn prompt_new_chapter(&self) {
        let (alerts, navigator) = (Rc::clone(&self.alerts), Rc::clone(&self.navigator));
        glib::spawn_future_local(async move {
            let typed = alerts
                .prompt_name(
                    books::NEW_CHAPTER_TITLE.to_string(),
                    books::NEW_CHAPTER_MESSAGE.to_string(),
                    String::new(),
                    buttons::CREATE.to_string(),
                )
                .await;
            if let Some(name) = typed {
                navigator.create_chapter(&name);
            }
        });
    }

    fn prompt_new_article(&self, folder: String) {
        let (alerts, navigator) = (Rc::clone(&self.alerts), Rc::clone(&self.navigator));
        glib::spawn_future_local(async move {
            let typed = alerts
                .prompt_name(
                    books::NEW_ARTICLE_TITLE.to_string(),
                    books::NEW_ARTICLE_MESSAGE.to_string(),
                    String::new(),
                    buttons::CREATE.to_string(),
                )
                .await;
            if let Some(name) = typed {
                navigator.create_article(&name, &folder);
            }
        });
    }

    fn prompt_rename(&self, path: String) {
        let (alerts, navigator) = (Rc::clone(&self.alerts), Rc::clone(&self.navigator));
        // Pre-filled with the readable half: the ordering prefix and the extension are kept anyway.
        let current = book_naming::display_name(book_paths::name(&path)).to_string();
        glib::spawn_future_local(async move {
            let typed = alerts
                .prompt_name(
                    documents::RENAME_TITLE.to_string(),
                    books::RENAME_MESSAGE.to_string(),
                    current,
                    documents::RENAME_TITLE.to_string(),
                )
                .await;
            if let Some(stem) = typed {
                navigator.perform_rename(&path, &stem);
            }
        });
    }

    fn prompt_delete(&self, path: String) {
        let Some(commands) = book_sidebar_model::commands_for(self.navigator.book().as_ref(), Some(&path)) else {
            return;
        };
        let (alerts, navigator) = (Rc::clone(&self.alerts), Rc::clone(&self.navigator));
        glib::spawn_future_local(async move {
            let confirmed = alerts
                .confirm_delete(books::delete_question(&commands.name), commands.delete_message().to_string())
                .await;
            if confirmed {
                navigator.perform_delete(&path);
            }
        });
    }
}

/// The self-test reads the sidebar back from its widgets.
#[cfg(feature = "selftest")]
impl BookSidebar {
    /// Every row as the list shows it: (kind, the text of the first label inside the row widget,
    /// selectable).
    pub fn shown_rows(&self) -> Vec<(BookRowKind, String, bool)> {
        let built = self.built.borrow();
        let mut out = Vec::new();
        let mut index = 0;
        while let Some(widget) = self.list.row_at_index(index) {
            index += 1;
            if let Some(b) = built.iter().find(|b| b.widget == widget) {
                let text = first_label(widget.upcast_ref()).map(|l| l.text().to_string()).unwrap_or_default();
                out.push((b.row.kind, text, widget.is_selectable()));
            }
        }
        out
    }

    /// The path of the highlighted row.
    pub fn selected_path(&self) -> Option<String> {
        let selected = self.list.selected_row()?;
        self.row_for(&selected).and_then(|row| row.path)
    }

    /// The row widget showing `path`.
    pub fn row_widget(&self, path: &str) -> Option<gtk::ListBoxRow> {
        self.built.borrow().iter().find(|b| b.row.path.as_deref() == Some(path)).map(|b| b.widget.clone())
    }

    /// Whether Move Up / Move Down are live (as of the last menu).
    pub fn move_enabled(&self) -> (bool, bool) {
        (self.move_up.is_enabled(), self.move_down.is_enabled())
    }
}

/// The first label in `widget`'s tree, depth first.
#[cfg(feature = "selftest")]
fn first_label(widget: &gtk::Widget) -> Option<gtk::Label> {
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        return Some(label.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(label) = first_label(&current) {
            return Some(label);
        }
        child = current.next_sibling();
    }
    None
}

/// The "New Article…" row: a flat button (the row itself is neither selectable nor activatable).
fn new_article_row(row: &BookRow) -> gtk::ListBoxRow {
    let (content, label) = icon_label("list-add-symbolic", &row.title, "md-new-article");
    label.add_css_class("dim-label");
    let button = gtk::Button::builder()
        .child(&content)
        .action_name(format!("book.{NEW_ARTICLE}"))
        .action_target(&row.folder.to_variant())
        .halign(gtk::Align::Start)
        .build();
    button.add_css_class("flat");
    gtk::ListBoxRow::builder().child(&button).selectable(false).activatable(false).build()
}

/// An icon and a label side by side; the label carries `class`.
fn icon_label(icon: &str, text: &str, class: &str) -> (gtk::Box, gtk::Label) {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    content.append(&gtk::Image::from_icon_name(icon));
    let label = gtk::Label::builder().label(text).xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build();
    label.add_css_class(class);
    content.append(&label);
    (content, label)
}

/// The context menu's model, every item targeting `path`.
fn menu_model(path: &str, is_article: bool) -> gio::Menu {
    let item = |title: &str, action: &str| {
        let item = gio::MenuItem::new(Some(title), None);
        item.set_action_and_target_value(Some(&format!("book.{action}")), Some(&path.to_variant()));
        item
    };
    let menu = gio::Menu::new();
    if is_article {
        let open = gio::Menu::new();
        open.append_item(&item(books::OPEN_IN_NEW_WINDOW, OPEN_IN_WINDOW));
        menu.append_section(None, &open);
    }
    let manage = gio::Menu::new();
    manage.append_item(&item(books::RENAME_ITEM, RENAME));
    manage.append_item(&item(books::MOVE_UP, MOVE_UP));
    manage.append_item(&item(books::MOVE_DOWN, MOVE_DOWN));
    menu.append_section(None, &manage);
    let delete = gio::Menu::new();
    delete.append_item(&item(books::DELETE_ITEM, DELETE));
    menu.append_section(None, &delete);
    menu
}

/// The sidebar width a drag may leave: the Mac's 200–320.
pub fn clamp_width(position: i32) -> i32 {
    position.clamp(WIDTH_MIN, WIDTH_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_width_stays_between_200_and_320() {
        assert_eq!(clamp_width(0), 200);
        assert_eq!(clamp_width(199), 200);
        assert_eq!(clamp_width(WIDTH_IDEAL), 240);
        assert_eq!(clamp_width(320), 320);
        assert_eq!(clamp_width(900), 320);
    }
}
