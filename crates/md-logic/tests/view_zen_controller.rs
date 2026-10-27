//! Port of md.win `tests/Md.App.Logic.Tests/ZenControllerTests.cs`: shell-design §5.4 — the
//! request/report pair (our own toggle vs. the user leaving full screen by any other means) and the
//! 2.5 s capsule fade. LINUX: the request is `gtk_window_fullscreen` / `unfullscreen`, the report
//! `notify::fullscreened`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use md_core::document::view_mode::ViewMode;
use md_logic::testing::*;
use md_logic::view::{DocumentWindowState, ZenController};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn recorder(zen: &ZenController) -> Rc<RefCell<Vec<bool>>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    zen.full_screen_requested().connect(move |full| s.borrow_mut().push(*full));
    seen
}

#[test]
fn toggling_asks_for_full_screen_and_back() {
    let state = Rc::new(DocumentWindowState::new());
    let zen = ZenController::new(state.clone(), None);
    let requests = recorder(&zen);

    zen.toggle();
    assert!(state.zen_active());
    zen.toggle();
    assert!(!state.zen_active());

    assert_eq!(*requests.borrow(), [true, false]);
}

#[test]
fn our_own_presenter_change_does_not_drop_zen() {
    // notify::fullscreened may arrive inside gtk_window_fullscreen; the toggling flag is what makes
    // that safe.
    let state = Rc::new(DocumentWindowState::new());
    let zen = ZenController::new(state.clone(), None);
    let toggling_seen = Rc::new(RefCell::new(Vec::new()));
    {
        let (weak, seen) = (Rc::downgrade(&zen), toggling_seen.clone());
        zen.full_screen_requested().connect(move |full| {
            let zen = weak.upgrade().unwrap();
            seen.borrow_mut().push(zen.is_toggling());
            zen.presenter_changed(*full);
        });
    }

    zen.toggle();

    assert!(state.zen_active());
    assert_eq!(*toggling_seen.borrow(), [true]);
    assert!(!zen.is_toggling());
}

#[test]
fn a_presenter_report_of_not_full_screen_arriving_inside_our_own_toggle_is_ignored() {
    // §5.4: "if Active && !toggling && !isFullScreen → Active = false". The !toggling clause is
    // what stops a report we provoked from undoing the toggle that provoked it — the report can
    // carry the OLD state, and the request can be refused outright (a window manager that does not
    // do full screen). Without the flag Zen would switch itself off in the same call that switched
    // it on.
    let state = Rc::new(DocumentWindowState::new());
    let zen = ZenController::new(state.clone(), None);
    let toggling_seen = Rc::new(RefCell::new(Vec::new()));
    {
        let (weak, seen) = (Rc::downgrade(&zen), toggling_seen.clone());
        zen.full_screen_requested().connect(move |_| {
            let zen = weak.upgrade().unwrap();
            seen.borrow_mut().push(zen.is_toggling());
            zen.presenter_changed(false);
        });
    }

    zen.toggle();

    assert!(state.zen_active());
    assert_eq!(*toggling_seen.borrow(), [true]);
    assert!(!zen.is_toggling());
}

#[test]
fn the_same_presenter_report_one_turn_later_still_drops_zen() {
    // The flag covers only our own call; a report that arrives after it returns is the user's.
    let state = Rc::new(DocumentWindowState::new());
    let zen = ZenController::new(state.clone(), None);
    zen.toggle();
    assert!(!zen.is_toggling());

    zen.presenter_changed(false);

    assert!(!state.zen_active());
}

#[test]
fn leaving_full_screen_by_any_other_means_drops_zen_and_asks_for_nothing() {
    let state = Rc::new(DocumentWindowState::new());
    let zen = ZenController::new(state.clone(), None);
    zen.toggle();
    let requests = recorder(&zen);

    zen.presenter_changed(false); // F11, Esc, the window manager

    assert!(!state.zen_active());
    assert!(requests.borrow().is_empty()); // the window is already a normal window
}

#[test]
fn full_screen_without_zen_is_left_alone() {
    let state = Rc::new(DocumentWindowState::new());
    let zen = ZenController::new(state.clone(), None);

    zen.presenter_changed(true);
    assert!(!state.zen_active());

    zen.presenter_changed(false);
    assert!(!state.zen_active());
}

#[test]
fn zen_never_touches_the_stored_mode() {
    let state = Rc::new(DocumentWindowState::new());
    state.set_stored_mode(ViewMode::Split);
    let zen = ZenController::new(state.clone(), None);

    zen.toggle();
    zen.set_reading(true);
    zen.set_reading(false);
    zen.presenter_changed(false);

    assert_eq!(state.stored_mode(), ViewMode::Split);
    assert!(!state.zen_reading());
}

// ── The fade ──────────────────────────────────────────────────────────────────────────────

#[test]
fn the_capsule_is_shown_on_entering_zen_and_fades_after_two_and_a_half_seconds() {
    let scheduler = Rc::new(FakeScheduler::new());
    let state = Rc::new(DocumentWindowState::new());
    let zen = ZenController::new(state, Some(scheduler.clone()));
    let shown = Rc::new(RefCell::new(Vec::new()));
    {
        let shown = shown.clone();
        zen.controls_shown_changed().connect(move |s| shown.borrow_mut().push(*s));
    }

    zen.toggle();
    assert!(zen.controls_shown());
    assert_eq!(scheduler.pending_timers(), 1);

    scheduler.advance(ms(2499));
    assert!(zen.controls_shown());

    scheduler.advance(ms(1));
    assert!(!zen.controls_shown());
    assert_eq!(*shown.borrow(), [false]);
}

#[test]
fn every_reveal_restarts_the_timer() {
    let scheduler = Rc::new(FakeScheduler::new());
    let zen = ZenController::new(Rc::new(DocumentWindowState::new()), Some(scheduler.clone()));
    zen.toggle();

    scheduler.advance(ms(2000));
    zen.reveal();
    scheduler.advance(ms(2000));
    assert!(zen.controls_shown());

    scheduler.advance(ms(500));
    assert!(!zen.controls_shown());

    zen.reveal();
    assert!(zen.controls_shown());
    assert_eq!(scheduler.pending_timers(), 1);
}

#[test]
fn leaving_zen_cancels_the_hide_and_leaves_the_capsule_ready_for_next_time() {
    let scheduler = Rc::new(FakeScheduler::new());
    let zen = ZenController::new(Rc::new(DocumentWindowState::new()), Some(scheduler.clone()));
    zen.toggle();
    scheduler.advance(ZenController::CHROME_HIDE_DELAY);
    assert!(!zen.controls_shown());

    zen.toggle();

    assert!(zen.controls_shown());
    assert_eq!(scheduler.pending_timers(), 0);
}

#[test]
fn reveal_outside_zen_does_nothing() {
    let scheduler = Rc::new(FakeScheduler::new());
    let zen = ZenController::new(Rc::new(DocumentWindowState::new()), Some(scheduler.clone()));

    zen.reveal();

    assert_eq!(scheduler.pending_timers(), 0);
}

#[test]
fn without_a_scheduler_the_capsule_simply_stays_shown() {
    let zen = ZenController::new(Rc::new(DocumentWindowState::new()), None);

    zen.toggle();
    zen.reveal();

    assert!(zen.controls_shown());
}
