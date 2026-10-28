//! Port of md.win `Windows/WindowRegistry.cs` — the app's [`DocumentRegistry`] implementation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;

use crate::event::Event;
use crate::seams::{DocumentRegistry, FileIdentity, WindowId};

#[derive(Default)]
struct State {
    by_path: HashMap<String, WindowId>,
    order: Vec<WindowId>,
    titles: HashMap<WindowId, String>,
}

impl State {
    /// Drop every path this window owns except `keep`; true when anything went.
    fn drop_ownership(&mut self, window: WindowId, keep: Option<&str>) -> bool {
        let before = self.by_path.len();
        self.by_path.retain(|path, owner| *owner != window || keep == Some(path.as_str()));
        self.by_path.len() != before
    }
}

/// Who owns which file, and what the Window menu lists (shell-design.md §8.6). Two maps, one
/// object: canonical path → window id (at most one path per window), and the ordered rows of every
/// live window with its title.
///
/// The ownership half is the [`DocumentRegistry`] seam, which `TextFileSession` drives: a document
/// window's session registers the file it edits and unregisters when it lets go, and the book's
/// detail pane asks [`DocumentRegistry::owning`] before it writes a byte. The rows half is the
/// window manager's: a window joins with [`WindowRegistry::add`] when it opens and leaves with
/// [`WindowRegistry::remove`] when it closes.
///
/// Those two halves are deliberately separate, and that is the one place this differs from the
/// test fake: [`DocumentRegistry::unregister`] drops the *ownership* only. A session detaching from
/// its file is not a window closing — an untitled window has never owned a path and must still
/// appear in the Window menu — so a row disappears when, and only when, its window does.
///
/// LINUX DECISION: paths compare exactly (md.win's dictionary is `OrdinalIgnoreCase`, as NTFS
/// is). On ext4 `Notes.md` and `NOTES.MD` are two files, so a case-only rename moves the ownership
/// like any other rename, and md.win's known limitation — `PathOf` answering with the pre-rename
/// spelling — does not exist here. Spellings that do name one file (`.`/`..`, doubled `/`,
/// symlinks) are folded by the [`FileIdentity`] before they reach the registry.
#[derive(Default)]
pub struct WindowRegistry {
    state: RefCell<State>,
    changed: Event<()>,
}

impl fmt::Debug for WindowRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowRegistry").field("windows", &self.windows()).finish()
    }
}

impl WindowRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// The same question as [`DocumentRegistry::owning`] from a path that has not been
    /// canonicalised yet — the activation router's "is this file already open?" (§1.2).
    pub fn find_by_path(&self, path: &str, identity: &dyn FileIdentity) -> Option<WindowId> {
        if path.is_empty() { None } else { self.owning(&identity.canonical(path)) }
    }

    /// The path a window owns, or `None`. For the window manager's own bookkeeping and for tests.
    pub fn path_of(&self, window: WindowId) -> Option<String> {
        self.state.borrow().by_path.iter().find(|(_, owner)| **owner == window).map(|(path, _)| path.clone())
    }

    /// A window opened: it joins the Window menu at once, before it has any file.
    pub fn add(&self, window: WindowId, title: &str) {
        let changed = {
            let mut s = self.state.borrow_mut();
            let mut changed = false;
            if !s.titles.contains_key(&window) {
                s.order.push(window);
                changed = true;
            }
            if s.titles.get(&window).map(String::as_str) != Some(title) {
                s.titles.insert(window, title.to_string());
                changed = true;
            }
            changed
        };
        if changed {
            self.changed.emit(&());
        }
    }

    /// The window's title moved (a save, a rename, an edit that added "— Edited"). Ignored for a
    /// window that is not open.
    pub fn set_title(&self, window: WindowId, title: &str) {
        let changed = {
            let mut s = self.state.borrow_mut();
            match s.titles.get_mut(&window) {
                Some(old) if old != title => {
                    *old = title.to_string();
                    true
                }
                _ => false,
            }
        };
        if changed {
            self.changed.emit(&());
        }
    }

    /// The window closed: its row and any ownership go together.
    pub fn remove(&self, window: WindowId) {
        let changed = {
            let mut s = self.state.borrow_mut();
            let mut changed = s.drop_ownership(window, None);
            if s.titles.remove(&window).is_some() {
                s.order.retain(|w| *w != window);
                changed = true;
            }
            changed
        };
        if changed {
            self.changed.emit(&());
        }
    }
}

impl DocumentRegistry for WindowRegistry {
    /// The window editing this canonical path, or `None`. Compared exactly (see the type comment).
    fn owning(&self, canonical_path: &str) -> Option<WindowId> {
        self.state.borrow().by_path.get(canonical_path).copied()
    }

    /// This window now edits this file. One path per window: a Save As moves the ownership rather
    /// than adding a second entry. A window that registers before the manager added its row gets
    /// one (titled `""`), so the ordering is still creation order.
    ///
    /// # Panics
    ///
    /// For an empty path (md.win throws `ArgumentException`).
    fn register(&self, window: WindowId, canonical_path: &str) {
        assert!(!canonical_path.is_empty(), "WindowRegistry::register: empty path");
        let changed = {
            let mut s = self.state.borrow_mut();
            let mut changed = false;
            if !s.titles.contains_key(&window) {
                s.order.push(window);
                s.titles.insert(window, String::new());
                changed = true;
            }
            changed |= s.drop_ownership(window, Some(canonical_path));
            if s.by_path.get(canonical_path) != Some(&window) {
                s.by_path.insert(canonical_path.to_string(), window);
                changed = true;
            }
            changed
        };
        if changed {
            self.changed.emit(&());
        }
    }

    /// This window no longer edits a file — a detach, a hand-off, a close. The window's row stays:
    /// closing is [`WindowRegistry::remove`].
    fn unregister(&self, window: WindowId) {
        let changed = self.state.borrow_mut().drop_ownership(window, None);
        if changed {
            self.changed.emit(&());
        }
    }

    /// Every live window in creation order, for the Window menu (§2.8).
    fn windows(&self) -> Vec<(WindowId, String)> {
        let s = self.state.borrow();
        s.order.iter().map(|w| (*w, s.titles.get(w).cloned().unwrap_or_default())).collect()
    }

    /// Raised when the rows or the ownership actually changed — never for a write of the value
    /// already there, so a menu rebuild cannot loop.
    fn changed(&self) -> &Event<()> {
        &self.changed
    }
}
