//! Port of md.win `View/ScrollSync.cs`.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

type Scroll = Rc<dyn Fn(f64)>;

/// The link between the two panes (macOS §10.3): each side reports how far down it is as a
/// fraction of its own scrollable height, and the other side is told to go to the same fraction.
/// Proportional, not line-mapped — the Mac's choice, and the reason a 2000-line document scrolls
/// smoothly without the preview being re-measured.
///
/// Deliberately not observable: a scroll must never re-render or re-lay out anything, or the two
/// panes chase each other. Echo suppression is each side's own job — the preview's injected script
/// carries a 300 ms programmatic window, the editor uses
/// [`ScrollSyncGuard`](super::ScrollSyncGuard).
#[derive(Default)]
pub struct ScrollSync {
    scroll_editor: RefCell<Option<Scroll>>,
    scroll_preview: RefCell<Option<Scroll>>,
}

impl fmt::Debug for ScrollSync {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScrollSync")
            .field("editor", &self.scroll_editor.borrow().is_some())
            .field("preview", &self.scroll_preview.borrow().is_some())
            .finish()
    }
}

impl ScrollSync {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set by the editor pane: scroll the editor to this fraction (its `GtkAdjustment`).
    /// Re-handed on every pane rebuild; `None` unregisters.
    pub fn set_scroll_editor(&self, scroll: Option<Box<dyn Fn(f64)>>) {
        *self.scroll_editor.borrow_mut() = scroll.map(Rc::from);
    }

    /// Set by the preview host: `window.__mdSyncScrollTo(fraction)`.
    pub fn set_scroll_preview(&self, scroll: Option<Box<dyn Fn(f64)>>) {
        *self.scroll_preview.borrow_mut() = scroll.map(Rc::from);
    }

    pub fn editor_did_scroll(&self, fraction: f64) {
        let scroll = self.scroll_preview.borrow().clone();
        if let Some(scroll) = scroll {
            scroll(Self::clamp(fraction));
        }
    }

    pub fn preview_did_scroll(&self, fraction: f64) {
        let scroll = self.scroll_editor.borrow().clone();
        if let Some(scroll) = scroll {
            scroll(Self::clamp(fraction));
        }
    }

    /// [0, 1]. NaN becomes 0: the fraction is a division by a scrollable height that can be zero on
    /// either side, and a NaN reaching `GtkAdjustment::set_value` or `window.scrollTo` is a jump to
    /// the top on one side and nothing at all on the other.
    pub fn clamp(fraction: f64) -> f64 {
        if fraction.is_nan() { 0.0 } else { fraction.clamp(0.0, 1.0) }
    }
}
