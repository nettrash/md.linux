//! Port of md.win `FakeDocumentRegistry`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;

use crate::event::Event;
use crate::seams::{DocumentRegistry, WindowId};

#[derive(Default)]
struct State {
    by_path: HashMap<String, WindowId>,
    order: Vec<WindowId>,
    titles: HashMap<WindowId, String>,
}

/// [`DocumentRegistry`] over maps: canonical path → window (compared exactly, as the seam
/// requires — Linux paths are case-sensitive; md.win's fake ignores case), one path per window,
/// plus the title list the Window menu shows. Untitled windows are added with
/// [`FakeDocumentRegistry::add_window`]; [`DocumentRegistry::register`] gives a window its path
/// (and a default title, the file's stem) — [`FakeDocumentRegistry::set_title`] changes it.
/// [`DocumentRegistry::changed`] fires on every mutation; [`FakeDocumentRegistry::changed_count`]
/// counts them.
#[derive(Default)]
pub struct FakeDocumentRegistry {
    state: RefCell<State>,
    changed: Event<()>,
    changed_count: Cell<usize>,
}

impl fmt::Debug for FakeDocumentRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeDocumentRegistry").field("windows", &self.windows()).finish()
    }
}

impl FakeDocumentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn changed_count(&self) -> usize {
        self.changed_count.get()
    }

    /// The path a window currently owns, or `None`.
    pub fn path_of(&self, window: WindowId) -> Option<String> {
        self.state.borrow().by_path.iter().find(|(_, w)| **w == window).map(|(p, _)| p.clone())
    }

    /// An untitled window (or a new title for a known one).
    pub fn add_window(&self, window: WindowId, title: &str) {
        {
            let mut state = self.state.borrow_mut();
            if !state.order.contains(&window) {
                state.order.push(window);
            }
            state.titles.insert(window, title.to_string());
        }
        self.raise();
    }

    /// # Panics
    ///
    /// If the window is not registered (md.win throws `KeyNotFoundException`).
    pub fn set_title(&self, window: WindowId, title: &str) {
        {
            let mut state = self.state.borrow_mut();
            let slot = state.titles.get_mut(&window).unwrap_or_else(|| panic!("window {window:?} is not registered"));
            *slot = title.to_string();
        }
        self.raise();
    }

    /// `Path.GetFileNameWithoutExtension` of the last `/` segment: everything before the last dot
    /// (".hidden" → "", "a.b.md" → "a.b").
    fn stem(path: &str) -> String {
        let name = match path.rfind('/') {
            Some(cut) => &path[cut + 1..],
            None => path,
        };
        match name.rfind('.') {
            Some(dot) => name[..dot].to_string(),
            None => name.to_string(),
        }
    }

    fn raise(&self) {
        self.changed_count.set(self.changed_count.get() + 1);
        self.changed.emit(&());
    }
}

impl DocumentRegistry for FakeDocumentRegistry {
    fn owning(&self, canonical_path: &str) -> Option<WindowId> {
        self.state.borrow().by_path.get(canonical_path).copied()
    }

    fn register(&self, window: WindowId, canonical_path: &str) {
        {
            let mut state = self.state.borrow_mut();
            state.by_path.retain(|_, w| *w != window);
            state.by_path.insert(canonical_path.to_string(), window);
            if !state.order.contains(&window) {
                state.order.push(window);
            }
            state.titles.entry(window).or_insert_with(|| Self::stem(canonical_path));
        }
        self.raise();
    }

    fn unregister(&self, window: WindowId) {
        {
            let mut state = self.state.borrow_mut();
            state.by_path.retain(|_, w| *w != window);
            state.order.retain(|w| *w != window);
            state.titles.remove(&window);
        }
        self.raise();
    }

    fn windows(&self) -> Vec<(WindowId, String)> {
        let state = self.state.borrow();
        state.order.iter().map(|w| (*w, state.titles.get(w).cloned().unwrap_or_default())).collect()
    }

    fn changed(&self) -> &Event<()> {
        &self.changed
    }
}
