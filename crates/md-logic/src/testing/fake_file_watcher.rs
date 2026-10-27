//! Port of md.win `FakeFileWatcher`.

use std::cell::{Cell, RefCell};
use std::fmt;

use crate::event::Event;
use crate::seams::FileWatcher;

/// [`FileWatcher`] a test drives by hand. Events raised while stopped or disposed are dropped, as
/// the real adapter's would be; a raise is synchronous — in a test the caller is the UI thread.
///
/// The seam has no `dispose` (the logic releases a watcher by dropping it; assert that with
/// `Rc::strong_count` on the fake). [`FakeFileWatcher::dispose`] is kept to model an adapter torn
/// down underneath the logic.
#[derive(Default)]
pub struct FakeFileWatcher {
    watched_path: RefCell<Option<String>>,
    is_watching: Cell<bool>,
    is_disposed: Cell<bool>,
    watch_history: RefCell<Vec<String>>,
    changed: Event<str>,
    renamed: Event<(String, String)>,
    deleted: Event<str>,
}

impl fmt::Debug for FakeFileWatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeFileWatcher")
            .field("watched_path", &self.watched_path.borrow())
            .field("is_watching", &self.is_watching.get())
            .field("is_disposed", &self.is_disposed.get())
            .finish()
    }
}

impl FakeFileWatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// The last path passed to [`FileWatcher::watch`] (kept after a stop).
    pub fn watched_path(&self) -> Option<String> {
        self.watched_path.borrow().clone()
    }

    pub fn is_watching(&self) -> bool {
        self.is_watching.get()
    }

    pub fn is_disposed(&self) -> bool {
        self.is_disposed.get()
    }

    /// Every path ever passed to [`FileWatcher::watch`], in order (re-targets included).
    pub fn watch_history(&self) -> Vec<String> {
        self.watch_history.borrow().clone()
    }

    /// Stops delivery for good; a later [`FileWatcher::watch`] panics (md.win throws
    /// `ObjectDisposedException`).
    pub fn dispose(&self) {
        self.is_watching.set(false);
        self.is_disposed.set(true);
    }

    /// The watched file changed (at `path`, default the watched path); delivered only while
    /// watching. Returns whether it was delivered.
    pub fn raise_changed(&self, path: Option<&str>) -> bool {
        let Some(path) = self.deliverable(path) else { return false };
        self.changed.emit(&path);
        true
    }

    /// The watched file (`old_path`, default the watched path) was renamed to `new_path`.
    pub fn raise_renamed(&self, new_path: &str, old_path: Option<&str>) -> bool {
        let Some(old_path) = self.deliverable(old_path) else { return false };
        self.renamed.emit(&(old_path, new_path.to_string()));
        true
    }

    /// The watched file (at `path`, default the watched path) was deleted.
    pub fn raise_deleted(&self, path: Option<&str>) -> bool {
        let Some(path) = self.deliverable(path) else { return false };
        self.deleted.emit(&path);
        true
    }

    fn deliverable(&self, path: Option<&str>) -> Option<String> {
        if !self.is_watching.get() || self.is_disposed.get() {
            return None;
        }
        Some(match path {
            Some(p) => p.to_string(),
            None => self.watched_path.borrow().clone().unwrap_or_default(),
        })
    }
}

impl FileWatcher for FakeFileWatcher {
    fn watch(&self, file_path: &str) {
        assert!(!self.is_disposed.get(), "FakeFileWatcher: watch() after dispose()");
        *self.watched_path.borrow_mut() = Some(file_path.to_string());
        self.is_watching.set(true);
        self.watch_history.borrow_mut().push(file_path.to_string());
    }

    fn stop(&self) {
        self.is_watching.set(false);
    }

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
