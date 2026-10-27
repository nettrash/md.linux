//! `ArticlePanes` — the editor column (find bar + editor) beside or above the preview in a
//! homogeneous `gtk::Box` inside an `adw::BreakpointBin` (stacked below 640 px of pane width), the
//! 1 px divider as a CSS border, all inside a `ZenBin` (shell-design §5.2, §5.3). Shared by the
//! document window and the Book window's article.
//!
//! md-logic's `SplitLayout` decides, this applies: Edit hides the preview slot and Preview the
//! editor column (`set_visible(false)` — the `WebView` stays in the tree, facts §2), Split shows
//! both as two equal halves with no splitter (the Mac has none), side by side from 640 px of the
//! panes' own width (the Book window's content is the window minus the sidebar) and stacked below
//! it. The breakpoint's condition is exactly `SplitLayout`'s threshold for whole pixels, so the
//! answer `layout_changed` reports and the orientation the box takes cannot disagree.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use md_core::document::view_mode::ViewMode;
use md_logic::event::{Event, HandlerId};
use md_logic::settings::SettingsStore;
use md_logic::view::{PaneLayout, ScrollSync, SplitLayout};

use crate::app::Shell;
use crate::editor::editor_pane::EditorPane;
use crate::editor::find_bar::FindBar;
use crate::editor::zen::ZenBin;
use crate::web::preview_host::PreviewHost;

/// Whose panes these are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PanesRole {
    Document,
    BookArticle,
}

/// The smallest the panes ask for: `adw::BreakpointBin` needs a size request of its own, and the
/// Book window's detail pane is the window minus a sidebar of up to 320 px.
const MINIMUM_WIDTH: i32 = 240;
const MINIMUM_HEIGHT: i32 = 160;

/// The editor and preview of one window.
pub struct ArticlePanes {
    /// Whose panes these are — reported to the self-test; the panes behave the same for both.
    #[cfg(feature = "selftest")]
    role: PanesRole,
    zen_bin: ZenBin,
    panes: gtk::Box,
    column: gtk::Box,
    preview_slot: gtk::Box,
    editor: Rc<EditorPane>,
    find_bar: Rc<FindBar>,
    scroll_sync: Rc<ScrollSync>,
    preview: RefCell<Option<(Rc<PreviewHost>, HandlerId)>>,
    mode: Cell<ViewMode>,
    zen: Cell<Option<bool>>,
    narrow: Cell<bool>,
    layout: Cell<Option<PaneLayout>>,
    layout_changed: Event<PaneLayout>,
}

impl ArticlePanes {
    pub fn new(shell: &Rc<Shell>, role: PanesRole) -> Rc<ArticlePanes> {
        let editor = EditorPane::new();
        let settings: Rc<dyn SettingsStore> = shell.settings.clone();
        editor.use_settings(Some(settings));
        let find_bar = FindBar::new(&editor);

        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.append(&find_bar.widget());
        column.append(&editor.widget());
        let preview_slot =
            gtk::Box::builder().orientation(gtk::Orientation::Vertical).hexpand(true).vexpand(true).build();
        let panes = gtk::Box::builder().orientation(gtk::Orientation::Horizontal).homogeneous(true).build();
        panes.append(&column);
        panes.append(&preview_slot);

        let bin = adw::BreakpointBin::builder()
            .child(&panes)
            .width_request(MINIMUM_WIDTH)
            .height_request(MINIMUM_HEIGHT)
            .hexpand(true)
            .vexpand(true)
            .build();
        let narrow = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            SplitLayout::SIDE_BY_SIDE_MINIMUM_WIDTH - 1.0,
            adw::LengthUnit::Px,
        ));
        let zen_bin = ZenBin::new(&bin);

        let scroll_sync = Rc::new(ScrollSync::new());
        editor.attach_scroll_sync(&scroll_sync);

        #[cfg(not(feature = "selftest"))]
        let _ = role;
        let this = Rc::new(ArticlePanes {
            #[cfg(feature = "selftest")]
            role,
            zen_bin,
            panes,
            column,
            preview_slot,
            editor,
            find_bar,
            scroll_sync,
            preview: RefCell::new(None),
            mode: Cell::new(ViewMode::Split),
            zen: Cell::new(None),
            narrow: Cell::new(false),
            layout: Cell::new(None),
            layout_changed: Event::new(),
        });

        let me = Rc::downgrade(&this);
        narrow.connect_apply(move |_| {
            if let Some(panes) = me.upgrade() {
                panes.narrow.set(true);
                panes.apply();
            }
        });
        let me = Rc::downgrade(&this);
        narrow.connect_unapply(move |_| {
            if let Some(panes) = me.upgrade() {
                panes.narrow.set(false);
                panes.apply();
            }
        });
        bin.add_breakpoint(narrow);

        // Escape in the editor column closes an open find bar first (md.win's OnEscape); with the
        // bar closed the key goes on to the window, whose controller leaves Zen.
        let keys = gtk::EventControllerKey::new();
        let me = Rc::downgrade(&this);
        keys.connect_key_pressed(move |_, keyval, _, state| {
            let plain = !state.intersects(
                gtk::gdk::ModifierType::SHIFT_MASK
                    | gtk::gdk::ModifierType::CONTROL_MASK
                    | gtk::gdk::ModifierType::ALT_MASK,
            );
            match me.upgrade() {
                Some(panes) if plain && keyval == gtk::gdk::Key::Escape && panes.find_bar.is_open() => {
                    panes.find_bar.close();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        this.column.add_controller(keys);

        this.apply();
        this
    }

    /// The `ZenBin` around the panes.
    pub fn widget(&self) -> gtk::Widget {
        self.zen_bin.clone().upcast()
    }

    pub fn editor(&self) -> Rc<EditorPane> {
        Rc::clone(&self.editor)
    }

    pub fn find_bar(&self) -> Rc<FindBar> {
        Rc::clone(&self.find_bar)
    }

    #[cfg(feature = "selftest")]
    pub fn role(&self) -> PanesRole {
        self.role
    }

    /// The preview goes in its slot, its scroll half joins the link (the editor's is already
    /// there), and it is told whether it is shown. A second call replaces the first.
    pub fn set_preview(&self, preview: &Rc<PreviewHost>) {
        if let Some((old, id)) = self.preview.borrow_mut().take() {
            old.did_scroll().disconnect(id);
        }
        while let Some(child) = self.preview_slot.first_child() {
            self.preview_slot.remove(&child);
        }
        let widget = preview.widget();
        widget.set_hexpand(true);
        widget.set_vexpand(true);
        self.preview_slot.append(&widget);

        let target = Rc::downgrade(preview);
        self.scroll_sync.set_scroll_preview(Some(Box::new(move |fraction| {
            if let Some(preview) = target.upgrade() {
                preview.apply_scroll_fraction(fraction);
            }
        })));
        let sync = Rc::clone(&self.scroll_sync);
        let id = preview.did_scroll().connect(move |fraction| sync.preview_did_scroll(*fraction));
        *self.preview.borrow_mut() = Some((Rc::clone(preview), id));
        preview.set_shown(self.layout.get().is_some_and(SplitLayout::shows_preview));
    }

    /// The effective mode from `DocumentWindowState`.
    pub fn set_mode(&self, mode: ViewMode) {
        if self.mode.replace(mode) != mode {
            self.apply();
        }
    }

    /// `None` = not in Zen; `Some(reading)` = Zen, showing the preview when `reading`.
    pub fn set_zen(&self, zen: Option<bool>) {
        if self.zen.replace(zen) != zen {
            self.apply();
        }
    }

    #[cfg(feature = "selftest")]
    pub fn scroll_sync(&self) -> Rc<ScrollSync> {
        Rc::clone(&self.scroll_sync)
    }

    /// Args: the new arrangement (side by side, stacked, one pane).
    pub fn layout_changed(&self) -> &Event<PaneLayout> {
        &self.layout_changed
    }

    /// The arrangement applied now.
    #[cfg(feature = "selftest")]
    pub fn layout(&self) -> PaneLayout {
        self.layout.get().unwrap_or(PaneLayout::SideBySide)
    }

    /// The editor column (find bar + editor) and the preview slot, for the self-test's geometry.
    #[cfg(feature = "selftest")]
    pub fn columns(&self) -> (gtk::Widget, gtk::Widget) {
        (self.column.clone().upcast(), self.preview_slot.clone().upcast())
    }

    /// The Zen layout bin.
    #[cfg(feature = "selftest")]
    pub fn zen_bin(&self) -> ZenBin {
        self.zen_bin.clone()
    }

    fn apply(&self) {
        let layout = match self.zen.get() {
            Some(true) => PaneLayout::PreviewOnly,
            Some(false) => PaneLayout::EditorOnly,
            None => {
                // The breakpoint is `width <= 639` px, which is `SplitLayout`'s `< 640` for the
                // whole pixels GTK allocates.
                let width = SplitLayout::SIDE_BY_SIDE_MINIMUM_WIDTH - if self.narrow.get() { 1.0 } else { 0.0 };
                SplitLayout::arrange(self.mode.get(), width)
            }
        };
        self.zen_bin.set_active(self.zen.get().is_some());
        self.column.set_visible(SplitLayout::shows_editor(layout));
        self.preview_slot.set_visible(SplitLayout::shows_preview(layout));
        self.panes.set_orientation(if layout == PaneLayout::Stacked {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        });
        self.column.remove_css_class("md-divider-right");
        self.column.remove_css_class("md-divider-bottom");
        match layout {
            PaneLayout::SideBySide => self.column.add_css_class("md-divider-right"),
            PaneLayout::Stacked => self.column.add_css_class("md-divider-bottom"),
            PaneLayout::EditorOnly | PaneLayout::PreviewOnly => {}
        }
        if let Some((preview, _)) = self.preview.borrow().as_ref() {
            preview.set_shown(SplitLayout::shows_preview(layout));
        }
        if self.layout.replace(Some(layout)) != Some(layout) {
            self.layout_changed.emit(&layout);
        }
    }
}
