//! `Footer` — the bar under the panes (shell-design §5.4): "N words · M characters" on the right,
//! the export spinner on the left (shown by md-logic's `BusyRing` through `set_busy`); the window
//! hides it in Zen. Typewriter 11 pt, tabular figures, ink 60 % on paper-secondary (theme
//! `md-footer`).

use std::rc::Rc;

use gtk::prelude::*;
use md_logic::view::DerivedText;

/// One window's footer.
pub struct Footer {
    root: gtk::Box,
    spinner: adw::Spinner,
    label: gtk::Label,
}

impl Footer {
    pub fn new() -> Rc<Footer> {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("md-footer");
        let spinner = adw::Spinner::new();
        spinner.set_visible(false);
        let label = gtk::Label::builder()
            .label(md_logic::strings::footer(0, 0))
            .hexpand(true)
            .halign(gtk::Align::End)
            .xalign(1.0)
            .build();
        root.append(&spinner);
        root.append(&label);
        Rc::new(Footer { root, spinner, label })
    }

    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// "N words · M characters" from the derived text (`strings::footer`).
    pub fn update(&self, derived: &DerivedText) {
        self.label.set_text(&md_logic::strings::footer(derived.words, derived.characters));
    }

    /// The export spinner (md-logic's `BusyRing` decides when: 500 ms into an export).
    pub fn set_busy(&self, busy: bool) {
        self.spinner.set_visible(busy);
    }

    /// The label's text, as shown.
    #[cfg(feature = "selftest")]
    pub fn text(&self) -> String {
        self.label.text().to_string()
    }

    #[cfg(feature = "selftest")]
    pub fn is_busy(&self) -> bool {
        self.spinner.is_visible()
    }
}
