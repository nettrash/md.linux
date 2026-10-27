//! Port of md.win `tests/Md.App.Logic.Tests/Export/BusyRingTests.cs`.
//!
//! §7.1: "exports on one window are serialised; a progress ring appears in the footer after
//! 500 ms". Every rule the GTK half must not decide for itself is here — the delay, the two ways an
//! export can end before it, and the thread the notification arrives on.
//!
//! The ring records what it was told rather than drawing anything: `shown` is the exact sequence of
//! `show` calls, so "nothing happened" and "shown then hidden" are different assertions and a
//! spurious hide cannot hide inside a final `false`.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use md_logic::export::{BusyRing, ExportPipeline};
use md_logic::testing::*;

struct Fixture {
    scheduler: Rc<FakeScheduler>,
    ui: Rc<FakeUiThread>,
    shown: Rc<RefCell<Vec<bool>>>,
}

impl Fixture {
    fn new() -> Self {
        Fixture {
            scheduler: Rc::new(FakeScheduler::new()),
            ui: Rc::new(FakeUiThread::new()),
            shown: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn ring(&self) -> BusyRing {
        let shown = self.shown.clone();
        BusyRing::new(self.scheduler.clone(), self.ui.clone(), move |show| shown.borrow_mut().push(show))
    }

    fn shown(&self) -> Vec<bool> {
        self.shown.borrow().clone()
    }
}

const DELAY: Duration = ExportPipeline::BUSY_RING_DELAY;
const JUST_UNDER: Duration = Duration::from_millis(499);

#[test]
fn the_delay_is_the_one_the_design_puts_beside_the_event() {
    // The number lives in ExportPipeline, next to busy_changed; the ring reads it rather than
    // keeping a second copy — which is how a design's one number becomes two.
    assert_eq!(DELAY, Duration::from_millis(500));
    assert_eq!(JUST_UNDER, DELAY - Duration::from_millis(1));
}

#[test]
fn a_fast_export_never_flickers_a_ring() {
    let f = Fixture::new();
    let ring = f.ring();
    ring.busy_changed(true);
    f.scheduler.advance(JUST_UNDER);
    ring.busy_changed(false);
    // Well past the delay: the armed timer must have been dropped, not merely outrun.
    f.scheduler.advance(Duration::from_secs(5));

    assert!(f.shown().is_empty());
    assert!(!ring.is_shown());
    assert_eq!(f.scheduler.pending_timers(), 0);
}

#[test]
fn a_slow_export_shows_the_ring_on_the_delay_and_hides_it_when_it_ends() {
    let f = Fixture::new();
    let ring = f.ring();
    ring.busy_changed(true);

    f.scheduler.advance(JUST_UNDER);
    assert!(f.shown().is_empty()); // 499 ms is still "not yet"

    f.scheduler.advance(Duration::from_millis(1));
    assert_eq!(f.shown(), [true]);
    assert!(ring.is_shown());

    ring.busy_changed(false);
    assert_eq!(f.shown(), [true, false]);
    assert!(!ring.is_shown());
}

#[test]
fn a_ring_that_was_never_shown_is_never_hidden() {
    // The case a naive `show(busy)` gets wrong: a fast export would end with show(false) on a
    // widget that was already hidden — harmless to look at, and a lie to anything reading the
    // sequence.
    let f = Fixture::new();
    let ring = f.ring();
    ring.busy_changed(true);
    ring.busy_changed(false);
    f.scheduler.advance(Duration::from_secs(5));
    assert!(f.shown().is_empty());
}

#[test]
fn each_export_gets_its_own_five_hundred_milliseconds() {
    let f = Fixture::new();
    let ring = f.ring();

    // One slow export: shown, then hidden.
    ring.busy_changed(true);
    f.scheduler.advance(DELAY);
    ring.busy_changed(false);
    assert_eq!(f.shown(), [true, false]);

    // The next one starts the clock again from zero rather than showing at once.
    ring.busy_changed(true);
    f.scheduler.advance(JUST_UNDER);
    assert_eq!(f.shown(), [true, false]);
    f.scheduler.advance(Duration::from_millis(1));
    assert_eq!(f.shown(), [true, false, true]);
}

#[test]
fn a_second_busy_does_not_restart_the_clock() {
    // The pipeline serialises, so true/false alternate — but a ring that restarted its timer on a
    // repeated true would push the ring past the moment it is worth showing.
    let f = Fixture::new();
    let ring = f.ring();
    ring.busy_changed(true);
    f.scheduler.advance(Duration::from_millis(300));
    ring.busy_changed(true);
    f.scheduler.advance(Duration::from_millis(200));

    assert_eq!(f.shown(), [true]);
    // Both notifications were marshalled; the second simply had nothing to do.
    assert_eq!(f.ui.post_count(), 2);
    assert_eq!(f.scheduler.pending_timers(), 0);
}

#[test]
fn an_idle_false_does_nothing_at_all() {
    let f = Fixture::new();
    let ring = f.ring();
    ring.busy_changed(false);
    f.scheduler.advance(Duration::from_secs(5));
    assert!(f.shown().is_empty());
    assert_eq!(f.scheduler.pending_timers(), 0);
}

/// Every notification goes through the UI-thread seam, and nothing reaches the `show` callback
/// until that post runs.
#[test]
fn every_notification_is_marshalled_through_the_ui_thread() {
    let f = Fixture::new();
    f.ui.set_run_inline(false);
    let ring = f.ring();

    ring.busy_changed(true);
    assert_eq!(f.ui.post_count(), 1);
    // Not even the timer is armed yet: the post is what arms it, on the UI thread.
    assert_eq!(f.scheduler.pending_timers(), 0);
    f.scheduler.advance(Duration::from_secs(5));
    assert!(f.shown().is_empty());

    f.ui.run_posted();
    assert_eq!(f.scheduler.pending_timers(), 1);
    f.scheduler.advance(DELAY);
    assert_eq!(f.shown(), [true]);

    ring.busy_changed(false);
    assert_eq!(f.shown(), [true]); // still queued
    f.ui.run_posted();
    assert_eq!(f.shown(), [true, false]);
}

#[test]
fn posts_are_applied_in_the_order_the_pipeline_raised_them() {
    // One export ending while the next begins: posting BOTH keeps false-then-true from arriving
    // reversed and leaving a ring on screen with nothing running.
    let f = Fixture::new();
    f.ui.set_run_inline(false);
    let ring = f.ring();

    ring.busy_changed(true);
    f.ui.run_posted();
    f.scheduler.advance(DELAY);
    assert_eq!(f.shown(), [true]);

    ring.busy_changed(false);
    ring.busy_changed(true);
    f.ui.run_posted();
    assert_eq!(f.shown(), [true, false]); // hidden, and the next 500 ms is running
    f.scheduler.advance(DELAY);
    assert_eq!(f.shown(), [true, false, true]);
}

#[test]
fn cancel_drops_an_armed_ring_without_touching_the_control() {
    // The window is going. The footer goes with it, so the ring must neither fire nor be told to
    // hide.
    let f = Fixture::new();
    let ring = f.ring();
    ring.busy_changed(true);
    f.scheduler.advance(JUST_UNDER);

    ring.cancel();
    f.scheduler.advance(Duration::from_secs(5));

    assert!(f.shown().is_empty());
    assert_eq!(f.scheduler.pending_timers(), 0);
}

#[test]
fn cancel_while_the_ring_is_showing_does_not_hide_it() {
    let f = Fixture::new();
    let ring = f.ring();
    ring.busy_changed(true);
    f.scheduler.advance(DELAY);
    assert_eq!(f.shown(), [true]);

    ring.cancel();
    assert_eq!(f.shown(), [true]);
    assert!(!ring.is_shown());
}

#[test]
fn a_cancelled_ring_can_start_again() {
    // Not a case the app reaches — cancel is the close — but a latch that could never re-arm would
    // be a trap for the next caller.
    let f = Fixture::new();
    let ring = f.ring();
    ring.busy_changed(true);
    ring.cancel();

    ring.busy_changed(true);
    f.scheduler.advance(DELAY);
    assert_eq!(f.shown(), [true]);
}

/// md.win: a negative delay (clamped to zero) shows the ring on the next tick. A `Duration` cannot
/// be negative, so the boundary is pinned at zero: the timer still has to fire.
#[test]
fn a_negative_delay_shows_the_ring_on_the_next_tick() {
    let f = Fixture::new();
    let shown = f.shown.clone();
    let ring = BusyRing::with_delay(
        f.scheduler.clone(),
        f.ui.clone(),
        move |show| shown.borrow_mut().push(show),
        Duration::ZERO,
    );
    ring.busy_changed(true);
    assert!(f.shown().is_empty()); // the timer still has to fire
    f.scheduler.run_due();
    assert_eq!(f.shown(), [true]);
}

#[test]
#[ignore = "md.win pins ArgumentNullException for each dependency; Rust references cannot be null"]
fn the_three_dependencies_are_required() {}
