//! `GioFileWatcher` — md-logic's `FileWatcher` over `gio::File::monitor_file(WATCH_MOVES)`
//! (shell-design §6.7). GLib's inotify monitor watches the file's folder, so it keeps working when
//! another editor replaces the file (facts §8); the event table itself is the pure
//! [`super::watch_events`], and this file adds the timing around it:
//!
//! - `changed` is coalesced — the first change arms a [`COALESCE`] timer and everything until it
//!   fires is the same report (an append says `Changed` then `ChangesDoneHint`);
//! - a file that left its path is judged after [`SETTLE`] (vim's backup dance and its relatives),
//!   by whether the path exists again;
//! - `renamed` / `deleted` drop a `changed` still waiting: the session learns the file's fate from
//!   the later report, and a stale change after a delete would turn a clean document conflicted.
//!
//! The monitor emits on the main context it was created on — the GTK main thread, where the
//! session lives — and every report is raised with no borrow held, so a handler may call
//! [`FileWatcher::watch`] again (the session retargets from inside `renamed`). A `watch` or `stop`
//! starts a new generation: timers armed for the previous target fire into nothing.
#![allow(dead_code)] // S0 stub: removed at the integration gate (P1) — this file's callers are P1's and P5's stubs until then

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use gio::prelude::*;
use md_logic::event::Event;
use md_logic::seams::FileWatcher;

use super::watch_events::{self, COALESCE, SETTLE, WatchReport};

/// One watched file per session (a document window's, the book article's).
pub struct GioFileWatcher {
    this: Weak<GioFileWatcher>,
    state: RefCell<Watch>,
    /// Bumped by every `watch` / `stop`; timers and monitor callbacks carry the value they were
    /// armed under and do nothing when it moved on.
    generation: Cell<u64>,
    changed: Event<str>,
    renamed: Event<(String, String)>,
    deleted: Event<str>,
}

#[derive(Default)]
struct Watch {
    path: Option<String>,
    monitor: Option<gio::FileMonitor>,
    /// The coalescing timer of a `changed` not yet raised.
    pending_changed: Option<glib::SourceId>,
}

impl GioFileWatcher {
    pub fn new() -> Rc<GioFileWatcher> {
        Rc::new_cyclic(|this| GioFileWatcher {
            this: this.clone(),
            state: RefCell::new(Watch::default()),
            generation: Cell::new(0),
            changed: Event::new(),
            renamed: Event::new(),
            deleted: Event::new(),
        })
    }

    /// The watched path, if any.
    pub fn path(&self) -> Option<String> {
        self.state.borrow().path.clone()
    }

    /// Tears the current target down and starts a new generation.
    fn reset(&self) -> u64 {
        let (monitor, pending) = {
            let mut state = self.state.borrow_mut();
            state.path = None;
            (state.monitor.take(), state.pending_changed.take())
        };
        if let Some(monitor) = monitor {
            monitor.cancel();
        }
        if let Some(timer) = pending {
            timer.remove();
        }
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        generation
    }

    fn on_event(&self, generation: u64, file: &gio::File, other: Option<&gio::File>, event: gio::FileMonitorEvent) {
        if generation != self.generation.get() {
            return;
        }
        let Some(ours) = self.path() else { return };
        // A file with no local path cannot be ours (the monitor was made from a path).
        let Some(file) = local_path(file) else { return };
        let other = other.and_then(local_path);
        if !watch_events::needs_settle(event, &file, other.as_deref(), &ours) {
            let report = watch_events::interpret(event, &file, other.as_deref(), &ours, false);
            self.deliver(generation, report);
            return;
        }
        let this = self.this.clone();
        glib::timeout_add_local_once(SETTLE, move || {
            let Some(watcher) = this.upgrade() else { return };
            if generation != watcher.generation.get() {
                return;
            }
            // Anything at the path counts, a dangling link included: something was written there.
            let exists_again = std::fs::symlink_metadata(&ours).is_ok();
            let report = watch_events::interpret(event, &file, other.as_deref(), &ours, exists_again);
            watcher.deliver(generation, report);
        });
    }

    fn deliver(&self, generation: u64, report: WatchReport) {
        let Some(ours) = self.path() else { return };
        match report {
            WatchReport::Ignore => {}
            WatchReport::Changed => {
                if self.state.borrow().pending_changed.is_some() {
                    return;
                }
                let this = self.this.clone();
                let timer = glib::timeout_add_local_once(COALESCE, move || {
                    let Some(watcher) = this.upgrade() else { return };
                    // The source is finished once this returns: forget it before anything else.
                    watcher.state.borrow_mut().pending_changed = None;
                    if generation != watcher.generation.get() {
                        return;
                    }
                    if let Some(path) = watcher.path() {
                        watcher.changed.emit(&path);
                    }
                });
                self.state.borrow_mut().pending_changed = Some(timer);
            }
            WatchReport::Renamed { old, new } => {
                self.drop_pending_changed();
                self.renamed.emit(&(old, new));
            }
            WatchReport::Deleted => {
                self.drop_pending_changed();
                self.deleted.emit(&ours);
            }
        }
    }

    fn drop_pending_changed(&self) {
        let pending = self.state.borrow_mut().pending_changed.take();
        if let Some(timer) = pending {
            timer.remove();
        }
    }
}

impl FileWatcher for GioFileWatcher {
    /// Watches `file_path` from now on; a path that cannot be monitored (its folder is gone, a
    /// file system without inotify support) is left unwatched — the stamp still guards every
    /// write.
    fn watch(&self, file_path: &str) {
        let generation = self.reset();
        if file_path.is_empty() {
            return;
        }
        let monitor = match gio::File::for_path(file_path)
            .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        {
            Ok(monitor) => monitor,
            Err(err) => {
                eprintln!("md: {file_path} is not watched for outside changes: {err}");
                return;
            }
        };
        let this = self.this.clone();
        monitor.connect_changed(move |_, file, other, event| {
            if let Some(watcher) = this.upgrade() {
                watcher.on_event(generation, file, other, event);
            }
        });
        let mut state = self.state.borrow_mut();
        state.path = Some(file_path.to_string());
        state.monitor = Some(monitor);
    }

    fn stop(&self) {
        self.reset();
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

impl Drop for GioFileWatcher {
    fn drop(&mut self) {
        let state = self.state.get_mut();
        if let Some(monitor) = state.monitor.take() {
            monitor.cancel();
        }
        if let Some(timer) = state.pending_changed.take() {
            timer.remove();
        }
    }
}

/// A `gio::File`'s local path as md-logic's UTF-8 string; `None` for a non-local file or a name
/// that is not UTF-8 (md-logic's paths are strings).
fn local_path(file: &gio::File) -> Option<String> {
    file.path().and_then(|path| path.into_os_string().into_string().ok())
}
