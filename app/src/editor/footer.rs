//! `Footer` — the bar under the panes (shell-design §5.4): "N words · M characters" on the right,
//! the export spinner on the left (md-logic's `BusyRing`); hidden in Zen.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P3

use std::rc::Rc;

use md_logic::view::DerivedText;

/// One window's footer.
pub struct Footer {
    root: gtk::Box,
}

impl Footer {
    pub fn new() -> Rc<Footer> {
        Rc::new(Footer { root: gtk::Box::new(gtk::Orientation::Horizontal, 0) })
    }

    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().into()
    }

    pub fn update(&self, derived: &DerivedText) {}

    pub fn set_busy(&self, busy: bool) {}
}
