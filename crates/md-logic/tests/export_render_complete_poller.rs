//! Port of md.win `tests/Md.App.Logic.Tests/Export/RenderCompletePollerTests.cs`.
//!
//! The handshake every export waits on (§4.8). Two things are load-bearing and both are easy to get
//! wrong: the flag is compared against the *quoted* JSON string, and running out of attempts is a
//! success.

mod common;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use common::task::Task;
use md_logic::export::RenderCompletePoller;
use md_logic::preview::scripts;
use md_logic::testing::*;

fn make(answer: impl Fn(&str) -> String + 'static) -> (RenderCompletePoller, Rc<FakeScheduler>, FakeRenderSurface) {
    let scheduler = Rc::new(FakeScheduler::new());
    let surface = FakeRenderSurface::default();
    surface.set_eval_handler(answer);
    (RenderCompletePoller::new(scheduler.clone()), scheduler, surface)
}

#[test]
fn the_page_that_is_already_done_is_not_waited_for() {
    let (poller, scheduler, surface) = make(|_| scripts::RENDER_COMPLETE_RESULT.to_string());
    let cancel = CancelToken::new();

    let task = Task::start(poller.wait(&surface, &cancel));

    assert!(task.is_completed());
    assert_eq!(task.output(), Ok(()));
    assert_eq!(surface.evals(), [scripts::RENDER_COMPLETE]);
    assert_eq!(scheduler.pending_timers(), 0);
}

#[test]
fn it_asks_every_quarter_second_and_stops_as_soon_as_the_flag_is_up() {
    let asked = Rc::new(Cell::new(0));
    let counter = asked.clone();
    let (poller, scheduler, surface) = make(move |_| {
        counter.set(counter.get() + 1);
        if counter.get() >= 3 { scripts::RENDER_COMPLETE_RESULT.to_string() } else { "null".to_string() }
    });
    let cancel = CancelToken::new();

    let mut task = Task::start(poller.wait(&surface, &cancel));

    assert!(!task.is_completed());
    assert_eq!(scheduler.pending_delays(), [RenderCompletePoller::INTERVAL]);
    scheduler.advance(RenderCompletePoller::INTERVAL);
    assert!(!task.poll());
    scheduler.advance(RenderCompletePoller::INTERVAL);
    assert!(task.poll());
    assert_eq!(task.output(), Ok(()));
    assert_eq!(surface.evals().len(), 3);
    assert_eq!(asked.get(), 3);
}

#[test]
fn running_out_of_attempts_counts_as_success_because_the_dom_is_still_worth_exporting() {
    let (poller, scheduler, surface) = make(|_| "null".to_string());
    let cancel = CancelToken::new();

    let mut task = Task::start(poller.wait(&surface, &cancel));
    for _ in 0..RenderCompletePoller::MAX_ATTEMPTS {
        assert!(!task.is_completed());
        scheduler.advance(RenderCompletePoller::INTERVAL);
        task.poll();
    }

    assert!(task.is_completed());
    assert_eq!(task.output(), Ok(())); // never failed
    // The Mac's loop: one read, then 480 more after 480 waits.
    assert_eq!(surface.evals().len(), RenderCompletePoller::MAX_ATTEMPTS + 1);
    assert_eq!(scheduler.pending_timers(), 0);
}

#[test]
fn nothing_but_the_quoted_one_counts_as_the_flag() {
    for answer in [
        "1",     // the number, not the attribute
        "null",  // a dead page or a failed script
        "\"\"",  //
        "",      //
        "\"0\"", //
    ] {
        let (poller, scheduler, surface) = make(move |_| answer.to_string());
        let cancel = CancelToken::new();

        let task = Task::start(poller.wait(&surface, &cancel));

        assert!(!task.is_completed(), "{answer:?}");
        assert_eq!(scheduler.pending_timers(), 1, "{answer:?}");
        // Left waiting; the export would poll on.
    }
}

/// LINUX: md.win's token runs a callback on cancel that disposes the timer and cancels the task at
/// once. exec.rs's `CancelToken` is a flag that wakes nothing, so the wait notices at its next poll
/// (here: by hand; in the app, at the latest when the 250 ms timer fires) — and then it stops the
/// timer rather than letting it fire later, and never asks again.
#[test]
fn a_cancelled_wait_stops_the_timer_rather_than_firing_it_later() {
    let (poller, scheduler, surface) = make(|_| "null".to_string());
    let cancel = CancelToken::new();

    let mut task = Task::start(poller.wait(&surface, &cancel));
    assert_eq!(scheduler.pending_timers(), 1);

    cancel.cancel();

    assert!(task.poll());
    assert_eq!(task.output(), Err(RenderError::Cancelled));
    assert_eq!(scheduler.pending_timers(), 0);
    scheduler.advance(RenderCompletePoller::INTERVAL);
    assert_eq!(surface.evals().len(), 1); // it never asked again
}

#[test]
fn a_wait_cancelled_before_it_starts_asks_nothing() {
    let (poller, scheduler, surface) = make(|_| "null".to_string());
    let cancel = CancelToken::new();
    cancel.cancel();

    let task = Task::start(poller.wait(&surface, &cancel));

    assert!(task.is_completed());
    assert_eq!(task.output(), Err(RenderError::Cancelled));
    assert!(surface.evals().is_empty());
    assert_eq!(scheduler.pending_timers(), 0);
}

#[test]
fn the_numbers_are_the_familys_two_minutes() {
    assert_eq!(RenderCompletePoller::MAX_ATTEMPTS, 480);
    assert_eq!(RenderCompletePoller::INTERVAL, Duration::from_millis(250));
    assert_eq!(scripts::RENDER_COMPLETE_RESULT, "\"1\"");
}
