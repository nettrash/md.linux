//! `GioFileWatcher` — md-logic's `FileWatcher` over `gio::File::monitor_file(WATCH_MOVES)`
//! (shell-design §6.7); the event table itself is the pure [`super::watch_events`].
#![allow(dead_code, unused_variables)] // S0 stub: removed by P2

use std::rc::Rc;

use md_logic::event::Event;
use md_logic::seams::FileWatcher;

/// One watched file per session (a document window's, the book article's).
pub struct GioFileWatcher {
    changed: Event<str>,
    renamed: Event<(String, String)>,
    deleted: Event<str>,
}

impl GioFileWatcher {
    pub fn new() -> Rc<GioFileWatcher> {
        Rc::new(GioFileWatcher { changed: Event::new(), renamed: Event::new(), deleted: Event::new() })
    }
}

impl FileWatcher for GioFileWatcher {
    fn watch(&self, file_path: &str) {}

    fn stop(&self) {}

    fn changed(&self) -> &Event<str> {
        &self.changed
    }

    fn renamed(&self) -> &Event<(String, String)> {
        &self.renamed
    }

    fn deleted(&self) -> &Event<str> {
        &self.deleted
    }
}
