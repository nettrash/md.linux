//! Port of md.win `View/ZenController.cs`.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::time::Duration;

use super::document_window_state::DocumentWindowState;
use crate::event::Event;
use crate::seams::{Scheduler, TimerHandle};

/// Zen mode (shell-design §5.4; macOS §7): the window goes full screen, the whole chrome collapses
/// to one centred column, and a floating capsule fades out after 2.5 s. Nothing here touches
/// `md.viewMode` or `md.viewModeMemory` — Zen's write/read switch is view state and lives only in
/// [`DocumentWindowState`] (and, for a saved document, session.json).
///
/// The window is the shell's: [`ZenController::full_screen_requested`] is
/// `gtk_window_fullscreen` / `unfullscreen`, and [`ZenController::presenter_changed`] is
/// `notify::fullscreened`. The `toggling` flag is why the pair does not loop: our own request may
/// report back synchronously or a main-loop turn later (the flag covers the synchronous case, and a
/// later report says what we asked for), and only a full-screen change we did not ask for drops
/// Zen. Leaving full screen by any means — F11, Esc, the window manager — therefore leaves Zen too,
/// which is the Mac's behaviour.
pub struct ZenController {
    me: Weak<ZenController>,
    state: Rc<DocumentWindowState>,
    scheduler: Option<Rc<dyn Scheduler>>,
    hide: RefCell<Option<TimerHandle>>,
    toggling: Cell<bool>,
    controls_shown: Cell<bool>,
    full_screen_requested: Event<bool>,
    controls_shown_changed: Event<bool>,
}

impl fmt::Debug for ZenController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZenController")
            .field("active", &self.state.zen_active())
            .field("controls_shown", &self.controls_shown.get())
            .finish()
    }
}

impl ZenController {
    /// The Mac's 2.5 s: every reveal restarts it, and the capsule fades when it fires.
    pub const CHROME_HIDE_DELAY: Duration = Duration::from_millis(2500);

    /// `scheduler` is the UI-thread timer behind the fade. `None` (a window built without one, and
    /// every test that does not care) keeps the capsule permanently shown — no timer, no fade.
    pub fn new(state: Rc<DocumentWindowState>, scheduler: Option<Rc<dyn Scheduler>>) -> Rc<ZenController> {
        Rc::new_cyclic(|me| ZenController {
            me: me.clone(),
            state,
            scheduler,
            hide: RefCell::new(None),
            toggling: Cell::new(false),
            controls_shown: Cell::new(true),
            full_screen_requested: Event::new(),
            controls_shown_changed: Event::new(),
        })
    }

    /// Args: true = full screen, false = back to a normal window.
    pub fn full_screen_requested(&self) -> &Event<bool> {
        &self.full_screen_requested
    }

    /// The capsule's opacity target: shown = opacity 1 and `can_target`, hidden = 0 and not.
    pub fn controls_shown_changed(&self) -> &Event<bool> {
        &self.controls_shown_changed
    }

    pub fn is_active(&self) -> bool {
        self.state.zen_active()
    }

    /// False = writing (the editor is the centre pane), true = reading (the preview is).
    pub fn is_reading(&self) -> bool {
        self.state.zen_reading()
    }

    pub fn controls_shown(&self) -> bool {
        self.controls_shown.get()
    }

    /// True only for the instant our own full-screen request is in flight (§5.4).
    pub fn is_toggling(&self) -> bool {
        self.toggling.get()
    }

    /// View ▸ Zen Mode (Shift+Ctrl+Return), and the capsule's exit button.
    pub fn toggle(&self) {
        self.set_active(!self.state.zen_active());
    }

    pub fn set_active(&self, active: bool) {
        if self.state.zen_active() == active {
            return;
        }
        self.state.set_zen_active(active);
        self.toggling.set(true);
        self.full_screen_requested.emit(&active);
        self.toggling.set(false);
        if active {
            self.reveal();
        } else {
            self.cancel_hide();
        }
    }

    /// The window's full-screen state actually changed. Our own change is ignored; anything else
    /// that leaves full screen drops Zen — and drops it without asking for a change back, because
    /// the window is already where it needs to be.
    pub fn presenter_changed(&self, is_full_screen: bool) {
        if self.toggling.get() || is_full_screen || !self.state.zen_active() {
            return;
        }
        self.state.set_zen_active(false);
        self.cancel_hide();
    }

    /// The capsule's write / read switches, and Ctrl+1 / Ctrl+2 / Ctrl+3 through
    /// [`ViewModeController::select`](super::ViewModeController::select) while Zen is on. Stores
    /// nothing.
    pub fn set_reading(&self, reading: bool) {
        self.state.set_zen_reading(reading);
    }

    /// Pointer movement over the Zen column, and entering Zen. Shows the capsule and restarts the
    /// hide timer; a no-op outside Zen, where there is no capsule to reveal.
    pub fn reveal(&self) {
        if !self.state.zen_active() {
            return;
        }
        self.show_controls(true);
        let previous = self.hide.borrow_mut().take();
        drop(previous); // cancels the running timer
        if let Some(scheduler) = &self.scheduler {
            let me = self.me.clone();
            let timer = scheduler.after(
                Self::CHROME_HIDE_DELAY,
                Box::new(move || {
                    if let Some(zen) = me.upgrade() {
                        zen.show_controls(false);
                    }
                }),
            );
            *self.hide.borrow_mut() = Some(timer);
        }
    }

    fn cancel_hide(&self) {
        let previous = self.hide.borrow_mut().take();
        drop(previous);
        // Leaving Zen leaves the capsule "shown", so re-entering does not start behind a fade-in.
        self.show_controls(true);
    }

    fn show_controls(&self, shown: bool) {
        if self.controls_shown.get() == shown {
            return;
        }
        self.controls_shown.set(shown);
        self.controls_shown_changed.emit(&shown);
    }
}
