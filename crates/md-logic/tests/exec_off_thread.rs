//! LINUX: the `OffThread` seam (shell-design §5.5, §15) — md.win's `RunOffThread` delegate as a
//! seam the app satisfies with `gio::spawn_blocking`: `Send` work in, its result back as a future
//! on the thread that polls it, `None` when the work never finished. `ThreadOffThread` (std only)
//! runs it on real threads; `FakeOffThread` holds it for the tests.

use std::cell::RefCell;
use std::rc::Rc;
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use md_logic::testing::*;

/// Polls `executor` until `done` says so, failing after ten seconds.
fn pump_until(executor: &LocalExecutor, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    executor.run_until_stalled();
    while !done() {
        assert!(Instant::now() < deadline, "the work never came back");
        thread::sleep(Duration::from_millis(1));
        executor.run_until_stalled();
    }
}

/// Spawns `run(work)` on `executor` and records what it resolves to.
fn spawn_run<T: Send + 'static>(
    executor: &LocalExecutor,
    off_thread: Rc<dyn OffThread>,
    work: impl FnOnce() -> T + Send + 'static,
) -> Rc<RefCell<Option<Option<T>>>> {
    let landed = Rc::new(RefCell::new(None));
    let slot = landed.clone();
    let pending = off_thread.run(work);
    executor.spawn_local(Box::pin(async move {
        *slot.borrow_mut() = Some(pending.await);
    }));
    landed
}

#[test]
fn run_hands_back_the_works_own_type() {
    let executor = LocalExecutor::new();
    let off_thread: Rc<dyn OffThread> = Rc::new(FakeOffThread::inline());
    let number = spawn_run(&executor, off_thread.clone(), || 41_u64 + 1);
    let text = spawn_run(&executor, off_thread, || "derived".to_string());
    executor.run_until_stalled();
    assert_eq!(*number.borrow(), Some(Some(42)));
    assert_eq!(*text.borrow(), Some(Some("derived".to_string())));
}

#[test]
fn thread_off_thread_runs_the_work_on_another_thread_and_resolves_where_it_is_polled() {
    let ui = thread::current().id();
    let executor = LocalExecutor::new();
    let landed = spawn_run(&executor, Rc::new(ThreadOffThread), || (thread::current().id(), "a".repeat(3)));
    pump_until(&executor, || landed.borrow().is_some());

    let (worked_on, value): (ThreadId, String) = landed.borrow_mut().take().unwrap().unwrap();
    assert_ne!(worked_on, ui);
    assert_eq!(value, "aaa");
}

#[test]
fn thread_off_thread_answers_none_for_work_that_panics() {
    // gio::spawn_blocking's JoinHandle resolves to Err for a panic; the seam's answer is None.
    let executor = LocalExecutor::new();
    let landed = spawn_run(&executor, Rc::new(ThreadOffThread), || -> u32 { panic!("the parse blew up") });
    pump_until(&executor, || landed.borrow().is_some());
    assert_eq!(*landed.borrow(), Some(None));

    // The runner is not poisoned by it.
    let next = spawn_run(&executor, Rc::new(ThreadOffThread), || 7_u32);
    pump_until(&executor, || next.borrow().is_some());
    assert_eq!(*next.borrow(), Some(Some(7)));
}

#[test]
fn a_runner_that_answers_with_another_type_reads_as_unfinished() {
    struct Rogue;
    impl OffThread for Rogue {
        fn spawn_blocking(&self, _work: OffThreadWork) -> LocalBoxFuture<'static, Option<OffThreadValue>> {
            ready(Some(Box::new("not a number")))
        }
    }
    let executor = LocalExecutor::new();
    let landed = spawn_run(&executor, Rc::new(Rogue), || 1_u32);
    executor.run_until_stalled();
    assert_eq!(*landed.borrow(), Some(None));
}

#[test]
fn the_fake_holds_work_until_the_test_runs_it_newest_or_oldest_first() {
    let executor = LocalExecutor::new();
    let fake = Rc::new(FakeOffThread::new());
    let first = spawn_run(&executor, fake.clone(), || 1_u32);
    let second = spawn_run(&executor, fake.clone(), || 2_u32);
    let third = spawn_run(&executor, fake.clone(), || 3_u32);
    executor.run_until_stalled();
    assert_eq!(fake.pending(), 3);
    assert_eq!(fake.runs(), 0);
    assert!(first.borrow().is_none() && second.borrow().is_none() && third.borrow().is_none());

    assert!(fake.run_newest());
    // Run, but nobody has polled the future yet: nothing has landed.
    assert!(third.borrow().is_none());
    executor.run_until_stalled();
    assert_eq!(*third.borrow(), Some(Some(3)));
    assert!(first.borrow().is_none());

    assert!(fake.drop_oldest());
    fake.run_all();
    executor.run_until_stalled();
    assert_eq!(*first.borrow(), Some(None)); // dropped: never finished
    assert_eq!(*second.borrow(), Some(Some(2)));
    assert_eq!(fake.pending(), 0);
    assert_eq!(fake.runs(), 2);
    assert!(!fake.run_oldest());
    assert!(!fake.run_newest());
    assert!(!fake.drop_oldest());
}

#[test]
fn the_inline_fake_runs_each_piece_of_work_as_it_is_handed_over() {
    let fake = Rc::new(FakeOffThread::inline());
    let executor = LocalExecutor::new();
    let landed = spawn_run(&executor, fake.clone(), || "now");
    assert_eq!(fake.runs(), 1);
    assert_eq!(fake.pending(), 0);
    executor.run_until_stalled();
    assert_eq!(*landed.borrow(), Some(Some("now")));
}
