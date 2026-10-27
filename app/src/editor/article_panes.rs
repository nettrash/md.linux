//! `ArticlePanes` — the editor column (find bar + editor) beside or above the preview in a
//! homogeneous `gtk::Box` inside an `adw::BreakpointBin` (stacked below 640 px of pane width), the
//! 1 px divider as a CSS border, all inside a `ZenBin` (shell-design §5.2, §5.3). Shared by the
//! document window and the Book window's article.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P3

use std::rc::Rc;

use md_core::document::view_mode::ViewMode;
use md_logic::event::Event;
use md_logic::view::{PaneLayout, ScrollSync};

use crate::app::Shell;
use crate::editor::editor_pane::EditorPane;
use crate::editor::find_bar::FindBar;
use crate::web::preview_host::PreviewHost;

/// Whose panes these are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PanesRole {
    Document,
    BookArticle,
}

/// The editor and preview of one window.
pub struct ArticlePanes {
    root: gtk::Box,
    editor: Rc<EditorPane>,
    find_bar: Rc<FindBar>,
    scroll_sync: Rc<ScrollSync>,
    layout_changed: Event<PaneLayout>,
}

impl ArticlePanes {
    pub fn new(shell: &Rc<Shell>, role: PanesRole) -> Rc<ArticlePanes> {
        Rc::new(ArticlePanes {
            root: gtk::Box::new(gtk::Orientation::Horizontal, 0),
            editor: EditorPane::new(),
            find_bar: FindBar::new(),
            scroll_sync: Rc::new(ScrollSync::new()),
            layout_changed: Event::new(),
        })
    }

    /// The `ZenBin` around the panes.
    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().into()
    }

    pub fn editor(&self) -> Rc<EditorPane> {
        Rc::clone(&self.editor)
    }

    pub fn find_bar(&self) -> Rc<FindBar> {
        Rc::clone(&self.find_bar)
    }

    pub fn set_preview(&self, preview: &Rc<PreviewHost>) {}

    /// The effective mode from `DocumentWindowState`.
    pub fn set_mode(&self, mode: ViewMode) {}

    /// `None` = not in Zen; `Some(reading)` = Zen, showing the preview when `reading`.
    pub fn set_zen(&self, zen: Option<bool>) {}

    pub fn scroll_sync(&self) -> Rc<ScrollSync> {
        Rc::clone(&self.scroll_sync)
    }

    /// Args: the new arrangement (side by side, stacked, one pane).
    pub fn layout_changed(&self) -> &Event<PaneLayout> {
        &self.layout_changed
    }
}
