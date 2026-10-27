//! `FindBar` — md's find & replace bar at the top of the editor column (shell-design §3.6): a
//! `gtk::Revealer` with the query `SearchEntry`, the tally, Previous / Next, Done, and the replace
//! row; every decision md-logic's `TextSearch` over UTF-16 offsets.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P3

use std::rc::Rc;

/// The bar of one editor column.
pub struct FindBar {
    revealer: gtk::Revealer,
}

impl FindBar {
    pub fn new() -> Rc<FindBar> {
        Rc::new(FindBar { revealer: gtk::Revealer::new() })
    }

    /// Edit ▸ Find ▸ Find… (Ctrl+F): the bar without the replace row, the query focused.
    pub fn show_find(&self) {}

    /// Find and Replace… (Ctrl+H): the replace row shown, the replacement focused.
    pub fn show_replace(&self) {}

    /// Find Next (`forward`) / Find Previous: the hit selected in the editor.
    pub fn find_next(&self, forward: bool) {}

    /// Use Selection for Find (Ctrl+E).
    pub fn use_selection(&self) {}

    pub fn is_open(&self) -> bool {
        false
    }

    pub fn has_query(&self) -> bool {
        false
    }

    /// Done / Escape: closes and returns focus to the editor.
    pub fn close(&self) {}
}
