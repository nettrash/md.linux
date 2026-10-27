//! `MenuBar` — a window's eight menus (`File · Edit · Format · View · Book · Go · Window · Help`)
//! as a `gtk::PopoverMenuBar` over a `gio::Menu` built from md-logic's `CommandTable::menus()`;
//! `refresh` rebuilds only the dynamic submenus whose rows changed (shell-design §2.1, §2.3).
#![allow(dead_code, unused_variables)] // S0 stub: removed by P1

use std::rc::Rc;

use md_logic::commands::ShellSnapshot;

use crate::app::Shell;
use crate::windows::host::WindowKind;

/// One window's menu bar.
pub struct MenuBar {
    widget: gtk::PopoverMenuBar,
}

impl MenuBar {
    pub fn new(shell: &Rc<Shell>, kind: WindowKind) -> MenuBar {
        MenuBar { widget: gtk::PopoverMenuBar::from_model(None::<&gio::MenuModel>) }
    }

    pub fn widget(&self) -> gtk::PopoverMenuBar {
        self.widget.clone()
    }

    /// Cheap when the snapshot equals the last one.
    pub fn refresh(&self, snapshot: &ShellSnapshot) {}
}
