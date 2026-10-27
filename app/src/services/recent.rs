//! `RecentFiles` — File ▸ Open Recent over `gtk::RecentManager` (shell-design §6.6): the desktop's
//! shared `recently-used.xbel`, md's ten newest items newer than `md.linux.recentClearedAt`, Clear
//! Menu hides rather than deletes. The filter is pure and unit-tested over plain records.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P2

use std::rc::Rc;

use md_logic::commands::RecentEntry;
use md_logic::event::Event;
use md_logic::settings::SettingsStore;

/// md's view of the desktop's recent list.
pub struct RecentFiles {
    /// For `md.linux.recentClearedAt` (md-logic pending: `settings_keys::RECENT_CLEARED_AT`).
    settings: Rc<dyn SettingsStore>,
    changed: Event<()>,
}

impl RecentFiles {
    pub fn new(settings: Rc<dyn SettingsStore>) -> Rc<RecentFiles> {
        Rc::new(RecentFiles { settings, changed: Event::new() })
    }

    /// At most ten, newest first.
    pub fn entries(&self) -> Vec<RecentEntry> {
        Vec::new()
    }

    pub fn add(&self, path: &str) {}

    pub fn clear(&self) {}

    /// The path behind a row's token; `None` → the file is gone: forgotten here, alerted by the
    /// caller.
    pub fn resolve(&self, token: &str) -> Option<String> {
        None
    }

    pub fn changed(&self) -> &Event<()> {
        &self.changed
    }
}
