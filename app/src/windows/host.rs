//! `WindowHost` — what every md window offers the rest of the shell (docs/shell-work-packages.md
//! §3.2, **frozen**: changed only by agreement of its implementers and every caller). Implemented
//! by the document window (P1) and the Book window (P5); used by the menus, the exports, the book
//! host and the self-test.

use std::rc::Rc;

use md_logic::commands::ShellSnapshot;
use md_logic::exec::LocalBoxFuture;
use md_logic::seams::{Alerts, Pickers, WindowId};

/// The two kinds of md window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WindowKind {
    /// One per document (and per untitled document).
    Document,
    /// The one app-wide Book window.
    Book,
}

/// One md window as the shell sees it.
#[allow(dead_code)] // S0 stub: removed at the integration gate (P1), once every method has its callers
pub trait WindowHost {
    /// md-logic's id for this window, unique for the life of the process.
    fn id(&self) -> WindowId;

    fn kind(&self) -> WindowKind;

    /// The toplevel (an `adw::ApplicationWindow`), for dialog parents and `present()`.
    fn window(&self) -> gtk::Window;

    /// The content overlay: export renderers and the Zen capsule live in it.
    fn overlay(&self) -> gtk::Overlay;

    /// This window's dialogs (one at a time).
    fn alerts(&self) -> Rc<dyn Alerts>;

    /// This window's file dialogs.
    fn pickers(&self) -> Rc<dyn Pickers>;

    /// An export or print in flight: the window's close waits for it, at most 10 s.
    fn track_output(&self, output: LocalBoxFuture<'static, ()>);

    /// What the window publishes for its menus, as of now.
    fn snapshot(&self) -> ShellSnapshot;

    /// Recomputes the snapshot and refreshes the menus.
    fn publish(&self);

    /// Whether the window currently shows the dark palette.
    fn is_dark(&self) -> bool;
}
