//! Port of md.win `tests/Md.App.Logic.Tests/FakesTests.cs` (minus `FakeShareTests`: Linux has no
//! share sheet).
//!
//! The fakes in `md_logic::testing` are what every module's tests drive their logic through, so
//! their behaviour is pinned here: a fake that quietly changed its ordering or stamp semantics
//! would make another module's green tests mean nothing.
//!
//! md.win's paths are Windows paths on a case-insensitive fake; these are Linux paths on a
//! case-sensitive one (see `FakeFileSystem`), and the tests that pinned the Windows rules pin the
//! Linux ones instead.

use std::cell::{Cell, RefCell};
use std::io;
use std::rc::Rc;
use std::time::Duration;

use md_logic::testing::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

type Log = Rc<RefCell<Vec<&'static str>>>;

fn log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

fn push(log: &Log, entry: &'static str) -> Box<dyn FnOnce()> {
    let log = log.clone();
    Box::new(move || log.borrow_mut().push(entry))
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------------------------
// FakeScheduler

#[test]
fn timers_fire_earliest_first_and_registration_order_breaks_ties() {
    let s = FakeScheduler::new();
    let log = log();
    s.after(ms(300), push(&log, "c")).detach();
    s.after(ms(100), push(&log, "a1")).detach();
    s.after(ms(100), push(&log, "a2")).detach();
    s.after(ms(200), push(&log, "b")).detach();

    s.advance(ms(250));
    assert_eq!(*log.borrow(), ["a1", "a2", "b"]);
    assert_eq!(s.pending_timers(), 1);

    s.advance(ms(50));
    assert_eq!(*log.borrow(), ["a1", "a2", "b", "c"]);
    assert_eq!(s.pending_timers(), 0);
}

#[test]
fn the_clock_reads_the_due_time_inside_a_callback_and_the_target_afterwards() {
    let s = Rc::new(FakeScheduler::new());
    let start = s.now();
    let seen = Rc::new(Cell::new(None));
    {
        let (s2, seen) = (s.clone(), seen.clone());
        s.after(ms(100), Box::new(move || seen.set(Some(s2.now())))).detach();
    }

    s.advance(Duration::from_secs(1));
    assert_eq!(seen.get(), Some(start + ms(100)));
    assert_eq!(s.now(), start + Duration::from_secs(1));
}

#[test]
fn a_timer_scheduled_by_a_callback_fires_in_the_same_advance_when_it_falls_due() {
    let s = Rc::new(FakeScheduler::new());
    let log = log();
    {
        let (s2, log2) = (s.clone(), log.clone());
        s.after(
            ms(100),
            Box::new(move || {
                log2.borrow_mut().push("first");
                s2.after(ms(50), push(&log2, "chained")).detach(); // due at 150: inside the window
                s2.after(ms(500), push(&log2, "later")).detach(); // due at 600: outside
            }),
        )
        .detach();
    }

    s.advance(ms(200));
    assert_eq!(*log.borrow(), ["first", "chained"]);
    assert_eq!(s.pending_timers(), 1);
    assert_eq!(s.pending_delays(), [ms(400)]);
}

#[test]
fn dropping_the_handle_cancels_the_timer() {
    // md.win: Disposing_the_handle_cancels_the_timer.
    let s = FakeScheduler::new();
    let fired = Rc::new(Cell::new(false));
    let handle = {
        let fired = fired.clone();
        s.after(ms(100), Box::new(move || fired.set(true)))
    };
    assert_eq!(s.pending_timers(), 1);

    drop(handle);
    assert_eq!(s.pending_timers(), 0);
    s.advance(Duration::from_secs(1));
    assert!(!fired.get());
}

#[test]
fn cancel_cancels_and_detach_lets_the_timer_fire_with_no_handle_kept() {
    // Rust only: C# lets an ignored IDisposable fire; here an ignored handle cancels.
    let s = FakeScheduler::new();
    let log = log();
    s.after(ms(10), push(&log, "cancelled")).cancel();
    s.after(ms(10), push(&log, "detached")).detach();
    let _ = s.after(ms(10), push(&log, "dropped at once"));

    s.advance(ms(10));
    assert_eq!(*log.borrow(), ["detached"]);
}

#[test]
fn cancelling_from_inside_a_callback_stops_a_later_timer() {
    let s = FakeScheduler::new();
    let fired = Rc::new(Cell::new(false));
    let second: Rc<RefCell<Option<TimerHandle>>> = Rc::new(RefCell::new(None));
    {
        let second = second.clone();
        s.after(
            ms(100),
            Box::new(move || {
                let handle = second.borrow_mut().take();
                if let Some(handle) = handle {
                    handle.cancel();
                }
            }),
        )
        .detach();
    }
    {
        let fired = fired.clone();
        *second.borrow_mut() = Some(s.after(ms(200), Box::new(move || fired.set(true))));
    }

    s.advance(Duration::from_secs(1));
    assert!(!fired.get());
    assert_eq!(s.pending_timers(), 0);
}

#[test]
fn restarting_a_debounce_shows_in_pending_delays() {
    let s = FakeScheduler::new();
    let handle = s.after(ms(350), Box::new(|| {}));
    s.advance(ms(200));
    assert_eq!(s.pending_delays(), [ms(150)]);

    drop(handle);
    let _restarted = s.after(ms(350), Box::new(|| {}));
    assert_eq!(s.pending_delays(), [ms(350)]);
}

#[test]
fn a_zero_delay_is_due_now_and_run_due_fires_it_without_moving_the_clock() {
    // md.win: A_negative_delay_is_due_now_… — its −5 ms is clamped to zero; a Duration cannot be
    // negative, so zero is the whole case here.
    let s = FakeScheduler::new();
    let fired = Rc::new(Cell::new(false));
    let start = s.now();
    {
        let fired = fired.clone();
        s.after(Duration::ZERO, Box::new(move || fired.set(true))).detach();
    }
    assert!(!fired.get());

    s.run_due();
    assert!(fired.get());
    assert_eq!(s.now(), start);
}

#[test]
fn posted_actions_run_only_on_run_posted_fifo_including_reposts() {
    let s = Rc::new(FakeScheduler::new());
    let log = log();
    {
        let (s2, log2) = (s.clone(), log.clone());
        s.post(Box::new(move || {
            log2.borrow_mut().push("a");
            s2.post(push(&log2, "c"));
        }));
    }
    s.post(push(&log, "b"));

    s.advance(Duration::from_secs(1)); // time does not drain posts
    assert!(log.borrow().is_empty());
    assert_eq!(s.pending_posts(), 2);

    assert_eq!(s.run_posted(), 3);
    assert_eq!(*log.borrow(), ["a", "b", "c"]);
    assert_eq!(s.pending_posts(), 0);
}

fn repost_forever(s: &Rc<FakeScheduler>) {
    let again = s.clone();
    s.post(Box::new(move || repost_forever(&again)));
}

#[test]
#[should_panic(expected = "Post loop: an action keeps re-posting itself")]
fn a_self_reposting_action_is_reported_not_spun() {
    let s = Rc::new(FakeScheduler::new());
    repost_forever(&s);
    s.run_posted();
}

// md.win's Advancing_backwards_throws and Null_actions_are_rejected cannot arise: a Duration is
// never negative and a Box<dyn FnOnce()> is never null.

#[test]
fn a_shared_clock_moves_with_the_scheduler_and_the_scheduler_sees_the_clock() {
    let clock = Rc::new(FakeClock::new());
    let s = FakeScheduler::with_clock(clock.clone());
    assert_eq!(s.now(), clock.start());
    assert_eq!(clock.wall_now(), FakeClock::epoch());

    s.advance(Duration::from_secs(60));
    assert_eq!(clock.wall_now(), FakeClock::epoch() + Duration::from_secs(60));

    clock.advance(Duration::from_secs(60));
    assert_eq!(s.now(), clock.start() + Duration::from_secs(120));
    assert!(Rc::ptr_eq(s.clock(), &clock));
}

// ---------------------------------------------------------------------------------------------
// FakeFileSystem

const A: &str = "/home/u/Docs/a.md";

#[test]
fn stamp_is_none_for_a_missing_file_and_mtime_plus_size_for_an_existing_one() {
    let fs = FakeFileSystem::new();
    assert_eq!(fs.stamp(A), None);
    assert!(!fs.file_exists(A));

    fs.add_file(A, "hello");
    let stamp = fs.stamp(A).expect("a stamp");
    assert_eq!(stamp.size, 5);
    assert_eq!(stamp.modified, FakeClock::epoch());
    assert!(fs.file_exists(A));
    assert!(fs.directory_exists("/home/u/Docs"));
    assert_eq!(fs.read_all_bytes(A).unwrap(), b"hello");
}

#[test]
fn every_write_gets_a_strictly_later_stamp_even_when_the_clock_stands_still() {
    let fs = FakeFileSystem::new();
    fs.add_file(A, "one");
    let s1 = fs.stamp(A).unwrap();

    fs.write_all_bytes_in_place(A, b"one").unwrap(); // same bytes, same size, same clock time
    let s2 = fs.stamp(A).unwrap();
    assert!(s2.modified > s1.modified);
    assert_eq!(s1.size, s2.size);
    assert_ne!(s1, s2);
}

#[test]
fn our_write_is_logged_and_an_external_write_is_not_but_both_move_the_stamp() {
    let fs = FakeFileSystem::new();
    fs.add_file(A, "one");
    let s0 = fs.stamp(A).unwrap();

    fs.write_all_bytes_in_place(A, b"two!").unwrap();
    let s1 = fs.stamp(A).unwrap();
    fs.write_externally(A, "three");
    let s2 = fs.stamp(A).unwrap();

    assert_eq!(fs.writes(), [A]);
    assert_ne!(s0, s1);
    assert_ne!(s1, s2);
    assert!(s2.modified > s1.modified);
    assert_eq!(s1.size, 4);
    assert_eq!(s2.size, 5);
    assert_eq!(fs.text(A).as_deref(), Some("three"));
}

#[test]
fn touch_changes_the_mtime_and_keeps_the_bytes() {
    let fs = FakeFileSystem::new();
    fs.add_file(A, "same");
    let before = fs.stamp(A).unwrap();

    fs.touch(A);
    let after = fs.stamp(A).unwrap();
    assert!(after.modified > before.modified);
    assert_eq!(before.size, after.size);
    assert_eq!(fs.text(A).as_deref(), Some("same"));
    assert!(fs.writes().is_empty());
}

#[test]
fn the_clock_drives_the_stamps() {
    let clock = Rc::new(FakeClock::new());
    let fs = FakeFileSystem::with_clock(clock.clone());
    fs.add_file(A, "x");
    assert!(Rc::ptr_eq(fs.clock(), &clock));

    clock.advance(Duration::from_secs(3600));
    fs.write_all_bytes_in_place(A, b"y").unwrap();
    assert_eq!(fs.stamp(A).unwrap().modified, FakeClock::epoch() + Duration::from_secs(3600));
}

#[test]
fn an_explicit_mtime_is_kept() {
    let fs = FakeFileSystem::new();
    // 2020-05-06 07:08:09 UTC
    let when = std::time::UNIX_EPOCH + Duration::from_secs(1_588_748_889);
    fs.add_file_stamped(A, b"x", when);
    assert_eq!(fs.stamp(A), Some(FileStamp { modified: when, size: 1 }));
}

#[test]
fn move_keeps_the_stamp_and_refuses_to_overwrite() {
    let fs = FakeFileSystem::new();
    fs.add_file(A, "x");
    fs.add_file("/home/u/Docs/b.md", "y");
    let stamp = fs.stamp(A);

    fs.move_file(A, "/home/u/Docs/c.md").unwrap();
    assert!(!fs.file_exists(A));
    assert_eq!(fs.stamp("/home/u/Docs/c.md"), stamp);
    let kind = |r: io::Result<()>| r.unwrap_err().kind();
    assert_eq!(kind(fs.move_file("/home/u/Docs/c.md", "/home/u/Docs/b.md")), io::ErrorKind::AlreadyExists);
    assert_eq!(kind(fs.move_file("/home/u/Docs/nope.md", "/home/u/Docs/d.md")), io::ErrorKind::NotFound);
    assert_eq!(kind(fs.move_file("/home/u/Docs/c.md", "/home/u/Elsewhere/c.md")), io::ErrorKind::NotFound);
    assert!(fs.file_exists("/home/u/Docs/c.md"));
}

#[test]
fn move_renames_a_folder_with_everything_under_it() {
    let fs = FakeFileSystem::new();
    fs.add_file("/home/u/Book/01 Intro/a.md", "x");
    fs.add_file("/home/u/Book/01 Intro/Deeper/b.md", "y");
    fs.add_file("/home/u/Book/01 Introduction/c.md", "z"); // a sibling sharing the prefix stays

    fs.move_file("/home/u/Book/01 Intro", "/home/u/Book/01 Start").unwrap();
    assert!(!fs.directory_exists("/home/u/Book/01 Intro"));
    assert!(fs.directory_exists("/home/u/Book/01 Start"));
    assert!(fs.directory_exists("/home/u/Book/01 Start/Deeper"));
    assert_eq!(fs.text("/home/u/Book/01 Start/a.md").as_deref(), Some("x"));
    assert_eq!(fs.text("/home/u/Book/01 Start/Deeper/b.md").as_deref(), Some("y"));
    assert_eq!(fs.text("/home/u/Book/01 Introduction/c.md").as_deref(), Some("z"));
}

#[test]
fn paths_are_case_sensitive_slash_separated_and_ignore_a_trailing_slash() {
    // md.win: Paths_are_case_insensitive_and_accept_either_separator — NTFS's rules; these are
    // ext4's.
    let fs = FakeFileSystem::new();
    fs.add_file("/home/u/Docs/Notes/a.md", "x");
    assert!(fs.file_exists("/home/u/Docs/Notes/a.md"));
    assert!(!fs.file_exists("/home/u/docs/notes/A.MD"));
    assert!(!fs.directory_exists("/home/u/DOCS/notes"));
    assert!(fs.directory_exists("/home/u/Docs/Notes/"));
    assert!(fs.directory_exists("/"));
    assert_eq!(fs.stamp("/home/u/docs/notes/A.MD"), None);

    // A backslash is an ordinary character in a Linux name.
    fs.add_file("/home/u/Docs/a\\b.md", "y");
    assert!(fs.file_exists("/home/u/Docs/a\\b.md"));
    assert!(!fs.directory_exists("/home/u/Docs/a"));
    assert_eq!(fs.enumerate_entries("/home/u/Docs").unwrap(), strings(&["/home/u/Docs/a\\b.md", "/home/u/Docs/Notes"]));
}

#[test]
fn a_read_only_file_refuses_the_in_place_write_and_keeps_its_bytes() {
    let fs = FakeFileSystem::new();
    fs.add_file(A, "keep");
    fs.set_read_only(A, true);

    let error = fs.write_all_bytes_in_place(A, b"new").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), format!("{A}: Permission denied (os error 13)"));
    assert_eq!(fs.text(A).as_deref(), Some("keep"));
    assert_eq!(fs.writes(), [A]); // the attempt is logged: the save-error bar path can be asserted on

    fs.set_read_only(A, false);
    fs.write_all_bytes_in_place(A, b"new").unwrap();
    assert_eq!(fs.text(A).as_deref(), Some("new"));
}

#[test]
fn next_write_failure_is_returned_once() {
    let fs = FakeFileSystem::new();
    fs.add_file(A, "x");
    fs.set_next_write_failure(Some(io::Error::other("disk full")));

    let error = fs.write_all_bytes_in_place(A, b"y").unwrap_err();
    assert_eq!(error.to_string(), "disk full");
    assert_eq!(fs.text(A).as_deref(), Some("x"));
    assert!(!fs.has_next_write_failure());

    fs.write_all_bytes_in_place(A, b"y").unwrap();
    assert_eq!(fs.text(A).as_deref(), Some("y"));
}

#[test]
fn writing_into_a_missing_folder_fails_like_std_fs_and_into_an_existing_one_creates_the_file() {
    let fs = FakeFileSystem::new();
    let error = fs.write_all_bytes_in_place("/home/u/Nope/a.md", b"x").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);

    fs.add_directory("/home/u/Docs");
    fs.write_all_bytes_in_place("/home/u/Docs/new.md", b"x").unwrap();
    assert!(fs.file_exists("/home/u/Docs/new.md"));
    assert!(fs.stamp("/home/u/Docs/new.md").is_some());
}

#[test]
fn reading_a_missing_file_fails() {
    let fs = FakeFileSystem::new();
    assert_eq!(fs.read_all_bytes(A).unwrap_err().kind(), io::ErrorKind::NotFound);
    assert_eq!(fs.reads(), [A]);
    assert_eq!(fs.bytes(A), None);
    assert_eq!(fs.text(A), None);
}

#[test]
fn delete_removes_files_and_whole_folders_and_ignores_a_missing_file() {
    let fs = FakeFileSystem::new();
    fs.add_file("/home/u/Book/01 Intro/a.md", "x");
    fs.add_file("/home/u/Book/02 Body/b.md", "y");

    fs.delete("/home/u/Book/nothing.md").unwrap(); // deleting a missing file is a no-op, as in md.win
    fs.delete("/home/u/Book/01 Intro").unwrap();
    assert!(!fs.directory_exists("/home/u/Book/01 Intro"));
    assert!(!fs.file_exists("/home/u/Book/01 Intro/a.md"));
    assert!(fs.file_exists("/home/u/Book/02 Body/b.md"));

    fs.delete("/home/u/Book/02 Body/b.md").unwrap();
    assert!(!fs.file_exists("/home/u/Book/02 Body/b.md"));
    assert!(fs.directory_exists("/home/u/Book/02 Body"));
}

#[test]
fn enumerate_entries_lists_direct_children_sorted_case_insensitively_and_fails_for_a_missing_folder() {
    let fs = FakeFileSystem::new();
    fs.add_file("/home/u/Book/02 Body/b.md", "y");
    fs.add_file("/home/u/Book/01 Intro/a.md", "x");
    fs.add_file("/home/u/Book/README.md", "r");
    fs.add_file("/home/u/Book/about.md", "r");

    assert_eq!(
        fs.enumerate_entries("/home/u/Book").unwrap(),
        strings(&["/home/u/Book/01 Intro", "/home/u/Book/02 Body", "/home/u/Book/about.md", "/home/u/Book/README.md"])
    );
    assert_eq!(fs.enumerate_entries("/home/u/Nope").unwrap_err().kind(), io::ErrorKind::NotFound);
}

#[test]
fn keys_drop_a_trailing_slash_and_nothing_else() {
    // md.win: Keys_use_forward_slashes_without_a_trailing_one — its `C:\` and UNC rows have no
    // Linux meaning; a backslash stays a backslash.
    for (path, key) in [
        ("/home/u/Docs/a.md", "/home/u/Docs/a.md"),
        ("/home/u/Docs/", "/home/u/Docs"),
        ("/home/u/Docs//", "/home/u/Docs"),
        ("/tmp/a.md", "/tmp/a.md"),
        ("/", "/"),
        ("//", "/"),
        ("a\\b.md", "a\\b.md"),
        ("relative/a.md", "relative/a.md"),
    ] {
        assert_eq!(FakeFileSystem::key(path), key, "{path}");
    }
}

// ---------------------------------------------------------------------------------------------
// FakePreviewSurface

#[test]
fn records_html_navigations_reloads_and_scripts_in_order() {
    let s = FakePreviewSurface::new();
    s.set_html("<p>1</p>".into());
    s.set_html("<p>2</p>".into());
    s.navigate("md-asset://preview-1/index.html");
    s.reload();
    s.reload();
    block_on(s.eval("window.scrollY".into()));

    assert_eq!(s.html_history(), ["<p>1</p>", "<p>2</p>"]);
    assert_eq!(s.html(), "<p>2</p>");
    assert_eq!(s.navigations(), ["md-asset://preview-1/index.html"]);
    assert_eq!(s.reload_count(), 2);
    assert_eq!(s.evals(), ["window.scrollY"]);
}

#[test]
fn eval_results_come_from_the_handler_then_the_queue_then_json_null() {
    let s = FakePreviewSurface::new();
    assert_eq!(block_on(s.eval("a".into())), "null");

    s.queue_eval_result("\"1\"");
    s.queue_eval_result("120.5");
    assert_eq!(block_on(s.eval("b".into())), "\"1\"");
    assert_eq!(block_on(s.eval("c".into())), "120.5");
    assert_eq!(block_on(s.eval("d".into())), "null");

    s.set_eval_handler(|script| if script == "x" { "42".into() } else { "0".into() });
    s.queue_eval_result("ignored while a handler is set");
    assert_eq!(block_on(s.eval("x".into())), "42");
    assert_eq!(block_on(s.eval("y".into())), "0");
    assert_eq!(s.evals(), ["a", "b", "c", "d", "x", "y"]);
}

#[test]
fn a_held_eval_stays_pending_until_answered() {
    let s = Rc::new(FakePreviewSurface::new());
    let executor = LocalExecutor::new();
    let held = s.hold_eval();
    let result = Rc::new(RefCell::new(None));
    {
        let (future, result) = (s.eval("window.scrollY".into()), result.clone());
        executor.spawn_local(Box::pin(async move { *result.borrow_mut() = Some(future.await) }));
    }
    executor.run_until_stalled();
    assert_eq!(*result.borrow(), None);

    held.complete("1200".into());
    executor.run_until_stalled();
    assert_eq!(result.borrow().as_deref(), Some("1200"));
}

#[test]
fn navigation_completion_is_observable_and_is_shown_is_settable() {
    let s = FakePreviewSurface::new();
    let results = Rc::new(RefCell::new(Vec::new()));
    {
        let results = results.clone();
        s.navigation_completed().connect(move |ok| results.borrow_mut().push(*ok));
    }
    assert!(s.is_shown());

    s.set_is_shown(false);
    s.complete_navigation(true);
    s.complete_navigation(false);
    assert_eq!(*results.borrow(), [true, false]);
    assert!(!s.is_shown());
}

// ---------------------------------------------------------------------------------------------
// FakeRenderSurface

#[test]
fn records_every_call_in_order_and_disposal_is_observable() {
    let s = FakeRenderSurface::new(RenderKind::Paper);
    let a4 = PrintGeometry { page_width_in: 8.27, page_height_in: 11.69, margin_in: 0.5 };
    let rect = RectD { x: 1.0, y: 2.0, width: 300.0, height: 40.0 };
    block_on(s.load("<html/>".into(), CancelToken::new())).unwrap();
    block_on(s.eval("document.title".into())).unwrap();
    let png = block_on(s.capture_region_png(rect, 2.0)).unwrap();
    block_on(s.set_height(842.5)).unwrap();
    let pdf = block_on(s.pdf(a4)).unwrap();

    assert_eq!(s.kind(), RenderKind::Paper);
    assert_eq!(s.loaded_html(), ["<html/>"]);
    assert_eq!(s.evals(), ["document.title"]);
    assert_eq!(s.captures(), [(rect, 2.0)]);
    assert_eq!(s.heights(), [842.5]);
    assert_eq!(s.pdf_requests(), [a4]);
    // md.win also pins `MarginIn == 0.5` as the record's default; the Rust seam's PrintGeometry
    // has no constructor, so the design's margin is whatever the export code passes.
    assert_eq!(png, s.png_bytes()); // a copy each time: a caller mutating its bytes cannot change the script
    assert_eq!(png, FakeRenderSurface::DEFAULT_PNG);
    assert!(pdf.starts_with(b"%PDF-"));

    assert!(!s.is_closed());
    block_on(s.close());
    assert!(s.is_closed());
}

#[test]
fn scripted_results_and_failures_surface_as_the_pipeline_would_see_them() {
    let s = FakeRenderSurface::default();
    s.set_load_failure(Some(RenderError::Failed("render".into())));
    s.set_pdf_failure(Some(RenderError::Failed("pdf".into())));
    let letter = PrintGeometry { page_width_in: 8.5, page_height_in: 11.0, margin_in: 0.5 };
    assert_eq!(block_on(s.load("x".into(), CancelToken::new())), Err(RenderError::Failed("render".into())));
    assert_eq!(block_on(s.pdf(letter)), Err(RenderError::Failed("pdf".into())));
    assert_eq!(s.loaded_html(), ["x"]); // a failed load was still attempted (and recorded)
    assert_eq!(s.pdf_requests().len(), 1);

    s.queue_eval_result("\"1\"");
    assert_eq!(block_on(s.eval("a".into())), Ok("\"1\"".to_string()));
    assert_eq!(block_on(s.eval("b".into())), Ok("null".to_string()));
    s.set_eval_handler(|_| "7".into());
    assert_eq!(block_on(s.eval("c".into())), Ok("7".to_string()));

    s.set_png_bytes(&[1, 2, 3]);
    let unit = RectD { x: 0.0, y: 0.0, width: 1.0, height: 1.0 };
    assert_eq!(block_on(s.capture_region_png(unit, 1.0)), Ok(vec![1, 2, 3]));
}

#[test]
fn a_cancelled_token_stops_the_load_before_it_is_recorded() {
    let s = FakeRenderSurface::default();
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(block_on(s.load("y".into(), token)), Err(RenderError::Cancelled));
    assert!(s.loaded_html().is_empty());
}

#[test]
fn a_held_load_stays_pending_until_the_test_finishes_the_render() {
    let s = Rc::new(FakeRenderSurface::default());
    let executor = LocalExecutor::new();
    let render = s.hold_load();
    let outcome = Rc::new(RefCell::new(None));
    {
        let (future, outcome) = (s.load("<p/>".into(), CancelToken::new()), outcome.clone());
        executor.spawn_local(Box::pin(async move { *outcome.borrow_mut() = Some(future.await) }));
    }
    executor.run_until_stalled();
    assert_eq!(*outcome.borrow(), None);
    assert_eq!(s.loaded_html(), ["<p/>"]);

    render.complete(Err(RenderError::Cancelled));
    executor.run_until_stalled();
    assert_eq!(*outcome.borrow(), Some(Err(RenderError::Cancelled)));
}

#[test]
fn the_factory_records_kinds_and_hands_out_fresh_surfaces_unless_told_otherwise() {
    let f = FakeRenderSurfaceFactory::new();
    let a = block_on(f.create(RenderKind::Export)).unwrap();
    let b = block_on(f.create(RenderKind::Paper)).unwrap();

    assert_eq!(f.kinds(), [RenderKind::Export, RenderKind::Paper]);
    let created = f.created();
    assert_eq!(created.len(), 2);
    assert!(!Rc::ptr_eq(&created[0], &created[1]));
    // The boxes are the very surfaces the factory recorded: what the logic does to `a` shows on
    // created[0] and not on created[1].
    block_on(a.eval("a".into())).unwrap();
    block_on(b.eval("b".into())).unwrap();
    assert_eq!(created[0].evals(), ["a"]);
    assert_eq!(created[1].evals(), ["b"]);
    assert_eq!(created[0].kind(), RenderKind::Export);
    assert_eq!(created[1].kind(), RenderKind::Paper);

    let shared = Rc::new(FakeRenderSurface::new(RenderKind::Screen));
    {
        let shared = shared.clone();
        f.set_create(move |_| shared.clone());
    }
    let c = block_on(f.create(RenderKind::Screen)).unwrap();
    block_on(c.set_height(10.0)).unwrap();
    assert_eq!(shared.heights(), [10.0]);
    assert!(Rc::ptr_eq(&f.last(), &shared));
    assert_eq!(f.created().len(), 3);
}

#[test]
fn a_failing_factory_records_the_kind_and_hands_out_nothing() {
    let f = FakeRenderSurfaceFactory::new();
    f.set_failure(Some(RenderError::Failed("no renderer".into())));
    assert_eq!(block_on(f.create(RenderKind::Paper)).err(), Some(RenderError::Failed("no renderer".into())));
    assert_eq!(f.kinds(), [RenderKind::Paper]);
    assert!(f.created().is_empty());
}

// ---------------------------------------------------------------------------------------------
// FakeAlerts

#[test]
fn unscripted_questions_get_the_answer_that_changes_nothing_and_every_call_is_recorded() {
    let a = FakeAlerts::new();
    block_on(a.warn("Could not rename".into(), "msg".into()));
    assert_eq!(
        block_on(a.prompt_name("Rename".into(), "Only the name changes".into(), "old".into(), "Rename".into())),
        None
    );
    assert!(!block_on(
        a.confirm_delete("Delete \u{201C}x\u{201D}?".into(), "The article file will be deleted.".into())
    ));
    assert_eq!(block_on(a.ask_save_changes("Untitled".into())), CloseChoice::Cancel);
    assert!(!block_on(a.confirm_replace("A folder named \u{201C}x\u{201D} already exists.".into())));

    assert_eq!(a.warnings(), [Warning::new("Could not rename", "msg")]);
    assert_eq!(a.name_prompts(), [NamePrompt::new("Rename", "Only the name changes", "old", "Rename")]);
    assert_eq!(
        a.delete_confirmations(),
        [Confirmation::new("Delete \u{201C}x\u{201D}?", "The article file will be deleted.")]
    );
    assert_eq!(a.save_changes_questions(), ["Untitled"]);
    assert_eq!(a.replace_confirmations(), ["A folder named \u{201C}x\u{201D} already exists."]);
}

#[test]
fn scripted_answers_are_consumed_in_order_then_the_safe_answer_returns() {
    let a = FakeAlerts::new();
    a.queue_name_answer(Some("New"));
    a.queue_name_answer(None);
    a.queue_delete_answer(true);
    a.queue_save_changes_answer(CloseChoice::Save);
    a.queue_save_changes_answer(CloseChoice::DontSave);
    a.queue_replace_answer(true);

    let prompt = || a.prompt_name("t".into(), "m".into(), "i".into(), "ok".into());
    assert_eq!(block_on(prompt()).as_deref(), Some("New"));
    assert_eq!(block_on(prompt()), None);
    assert_eq!(block_on(prompt()), None);
    assert!(block_on(a.confirm_delete("t".into(), "m".into())));
    assert!(!block_on(a.confirm_delete("t".into(), "m".into())));
    assert_eq!(block_on(a.ask_save_changes("x".into())), CloseChoice::Save);
    assert_eq!(block_on(a.ask_save_changes("x".into())), CloseChoice::DontSave);
    assert_eq!(block_on(a.ask_save_changes("x".into())), CloseChoice::Cancel);
    assert!(block_on(a.confirm_replace("m".into())));
    assert!(!block_on(a.confirm_replace("m".into())));
    assert_eq!(a.name_prompts().len(), 3);
}

#[test]
fn a_held_dialog_stays_open_until_the_test_answers_it_and_queued_answers_keep_their_order() {
    let a = Rc::new(FakeAlerts::new());
    let executor = LocalExecutor::new();
    a.queue_save_changes_answer(CloseChoice::DontSave);
    let second = a.hold_save_changes();
    let warning = a.hold_warn();
    let answers = Rc::new(RefCell::new(Vec::new()));
    let warned = Rc::new(Cell::new(false));
    {
        let (alerts, answers, warned) = (a.clone(), answers.clone(), warned.clone());
        executor.spawn_local(Box::pin(async move {
            // Await first: a RefMut held across an await would still be borrowed when the test looks.
            let first = alerts.ask_save_changes("one".into()).await;
            answers.borrow_mut().push(first);
            let second = alerts.ask_save_changes("two".into()).await;
            answers.borrow_mut().push(second);
            alerts.warn("t".into(), "m".into()).await;
            warned.set(true);
        }));
    }
    executor.run_until_stalled();
    assert_eq!(*answers.borrow(), [CloseChoice::DontSave]); // the first was immediate
    assert_eq!(a.save_changes_questions(), ["one", "two"]); // the second is up, unanswered
    assert_eq!(executor.pending(), 1);

    second.complete(CloseChoice::Save);
    executor.run_until_stalled();
    assert_eq!(*answers.borrow(), [CloseChoice::DontSave, CloseChoice::Save]);
    assert_eq!(a.warnings(), [Warning::new("t", "m")]);
    assert!(!warned.get());

    warning.complete(());
    executor.run_until_stalled();
    assert!(warned.get());
    assert_eq!(executor.pending(), 0);
}

// ---------------------------------------------------------------------------------------------
// FakePickers

#[test]
fn unscripted_pickers_are_cancelled_and_calls_are_recorded_with_their_arguments() {
    let p = FakePickers::new();
    let choices = vec![
        FileTypeChoice { label: "Markdown Document".into(), extensions: strings(&[".md", ".markdown"]) },
        FileTypeChoice { label: "Plain Text".into(), extensions: strings(&[".txt"]) },
    ];

    assert!(block_on(p.open_files(strings(&[".md", ".txt"]))).is_empty());
    assert_eq!(block_on(p.save_file("Untitled".into(), choices.clone(), ".md".into())), None);
    assert_eq!(block_on(p.pick_folder(Some("Choose where to keep the TextBundle".into()))), None);
    assert_eq!(block_on(p.pick_folder(None)), None);

    assert_eq!(p.open_calls(), [strings(&[".md", ".txt"])]);
    let saves = p.save_calls();
    assert_eq!(saves.len(), 1);
    assert_eq!(saves[0].suggested_name, "Untitled");
    assert_eq!(saves[0].default_extension, ".md");
    assert_eq!(saves[0].labels(), ["Markdown Document", "Plain Text"]);
    assert_eq!(saves[0].choices, choices);
    assert_eq!(p.folder_calls(), [Some("Choose where to keep the TextBundle".to_string()), None]);
}

#[test]
fn scripted_picker_answers_are_consumed_in_order() {
    // md.win: FakePickersTests.Scripted_answers_are_consumed_in_order.
    let p = FakePickers::new();
    p.queue_open_answer(&["/home/u/Docs/a.md", "/home/u/Docs/b.md"]);
    p.queue_save_answer(Some("/home/u/Docs/out.pdf"));
    p.queue_folder_answer(Some("/home/u/Docs"));

    assert_eq!(block_on(p.open_files(strings(&[".md"]))), ["/home/u/Docs/a.md", "/home/u/Docs/b.md"]);
    assert!(block_on(p.open_files(strings(&[".md"]))).is_empty());
    assert_eq!(block_on(p.save_file("a".into(), vec![], ".pdf".into())).as_deref(), Some("/home/u/Docs/out.pdf"));
    assert_eq!(block_on(p.save_file("a".into(), vec![], ".pdf".into())), None);
    assert_eq!(block_on(p.pick_folder(None)).as_deref(), Some("/home/u/Docs"));
    assert_eq!(block_on(p.pick_folder(None)), None);
}

#[test]
fn a_held_save_dialog_stays_open_until_answered() {
    let p = Rc::new(FakePickers::new());
    let executor = LocalExecutor::new();
    let dialog = p.hold_save();
    let chosen = Rc::new(RefCell::new(None));
    {
        let (future, chosen) = (p.save_file("Doc".into(), vec![], ".pdf".into()), chosen.clone());
        executor.spawn_local(Box::pin(async move { *chosen.borrow_mut() = Some(future.await) }));
    }
    executor.run_until_stalled();
    assert_eq!(*chosen.borrow(), None);
    assert_eq!(p.save_calls().len(), 1);

    dialog.complete(Some("/home/u/Doc.pdf".into()));
    executor.run_until_stalled();
    assert_eq!(*chosen.borrow(), Some(Some("/home/u/Doc.pdf".to_string())));
}

// ---------------------------------------------------------------------------------------------
// FakeFileWatcher

#[test]
fn events_are_delivered_only_while_watching_and_default_to_the_watched_path() {
    let w = FakeFileWatcher::new();
    let changed = Rc::new(RefCell::new(Vec::<String>::new()));
    let renamed = Rc::new(RefCell::new(Vec::<(String, String)>::new()));
    let deleted = Rc::new(RefCell::new(Vec::<String>::new()));
    {
        let changed = changed.clone();
        w.changed().connect(move |p| changed.borrow_mut().push(p.to_string()));
        let renamed = renamed.clone();
        w.renamed().connect(move |pair| renamed.borrow_mut().push(pair.clone()));
        let deleted = deleted.clone();
        w.deleted().connect(move |p| deleted.borrow_mut().push(p.to_string()));
    }

    assert!(!w.is_watching());
    assert!(!w.raise_changed(Some("x"))); // not watching: dropped

    w.watch("/home/u/Docs/a.md");
    assert!(w.is_watching());
    assert_eq!(w.watched_path().as_deref(), Some("/home/u/Docs/a.md"));
    assert!(w.raise_changed(None));
    assert!(w.raise_renamed("/home/u/Docs/b.md", None));
    assert!(w.raise_deleted(None));

    w.stop();
    assert!(!w.raise_changed(None));
    w.watch("/home/u/Docs/c.md"); // re-target
    assert!(w.raise_changed(None));

    assert_eq!(*changed.borrow(), ["/home/u/Docs/a.md", "/home/u/Docs/c.md"]);
    assert_eq!(*renamed.borrow(), [("/home/u/Docs/a.md".to_string(), "/home/u/Docs/b.md".to_string())]);
    assert_eq!(*deleted.borrow(), ["/home/u/Docs/a.md"]);
    assert_eq!(w.watch_history(), ["/home/u/Docs/a.md", "/home/u/Docs/c.md"]);
}

#[test]
fn dispose_stops_delivery() {
    // md.win: Dispose_stops_delivery_and_refuses_a_new_target (split: the refusal is a panic here,
    // pinned by the next test).
    let w = FakeFileWatcher::new();
    let raised = Rc::new(Cell::new(0));
    {
        let raised = raised.clone();
        w.changed().connect(move |_| raised.set(raised.get() + 1));
    }
    w.watch("/home/u/Docs/a.md");

    w.dispose();
    assert!(w.is_disposed());
    assert!(!w.is_watching());
    assert!(!w.raise_changed(None));
    assert_eq!(raised.get(), 0);
}

#[test]
#[should_panic(expected = "watch() after dispose()")]
fn dispose_refuses_a_new_target() {
    let w = FakeFileWatcher::new();
    w.watch("/home/u/Docs/a.md");
    w.dispose();
    w.watch("/home/u/Docs/b.md");
}

// ---------------------------------------------------------------------------------------------
// FakeDocumentRegistry

#[test]
fn owning_is_exact_one_path_per_window_and_every_mutation_raises_changed() {
    // md.win: Owning_is_case_insensitive_… — the seam compares exactly on Linux.
    let r = FakeDocumentRegistry::new();
    let (w1, w2) = (WindowId(1), WindowId(2));
    let changes = Rc::new(Cell::new(0));
    {
        let changes = changes.clone();
        r.changed().connect(move |_| changes.set(changes.get() + 1));
    }

    r.add_window(w1, "Untitled");
    r.register(w1, "/home/u/Docs/a.md");
    r.register(w2, "/home/u/Docs/b.md");
    assert_eq!(r.owning("/home/u/Docs/a.md"), Some(w1));
    assert_eq!(r.owning("/home/u/docs/A.MD"), None); // another file on Linux
    assert_eq!(r.owning("/home/u/Docs/b.md"), Some(w2));
    assert_eq!(r.owning("/home/u/Docs/c.md"), None);

    r.register(w1, "/home/u/Docs/renamed.md"); // Save As / Rename re-keys the window
    assert_eq!(r.owning("/home/u/Docs/a.md"), None);
    assert_eq!(r.owning("/home/u/Docs/renamed.md"), Some(w1));
    assert_eq!(r.path_of(w1).as_deref(), Some("/home/u/Docs/renamed.md"));
    // register keeps an existing title, defaults a new one to the stem
    assert_eq!(r.windows(), [(w1, "Untitled".to_string()), (w2, "b".to_string())]);

    r.set_title(w1, "renamed \u{2014} Edited");
    r.unregister(w2);
    assert_eq!(r.windows(), [(w1, "renamed \u{2014} Edited".to_string())]);
    assert_eq!(r.path_of(w2), None);
    assert_eq!(r.owning("/home/u/Docs/b.md"), None);

    assert_eq!(changes.get(), 6);
    assert_eq!(r.changed_count(), 6);
}

#[test]
#[should_panic(expected = "is not registered")]
fn set_title_of_an_unknown_window_panics() {
    // The last assertion of md.win's registry test (KeyNotFoundException).
    let r = FakeDocumentRegistry::new();
    r.register(WindowId(1), "/home/u/Docs/a.md");
    r.unregister(WindowId(1));
    r.set_title(WindowId(1), "x");
}

#[test]
fn the_default_title_is_the_stem_of_the_last_segment() {
    let r = FakeDocumentRegistry::new();
    r.register(WindowId(1), "/home/u/Docs/notes.v2.md");
    r.register(WindowId(2), "/home/u/Docs/README");
    r.register(WindowId(3), "/home/u/.hidden");
    r.register(WindowId(4), "/home/u/a\\b.md");
    let titles: Vec<String> = r.windows().into_iter().map(|(_, t)| t).collect();
    assert_eq!(titles, ["notes.v2", "README", "", "a\\b"]);
}

// ---------------------------------------------------------------------------------------------
// FakeUiThread

#[test]
fn runs_inline_by_default_or_queues_for_run_posted() {
    let ui = FakeUiThread::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    let post = |n: i32| {
        let log = log.clone();
        Box::new(move || log.borrow_mut().push(n)) as Box<dyn FnOnce()>
    };
    ui.post(post(1));
    assert_eq!(*log.borrow(), [1]);
    assert!(ui.is_current());

    ui.set_run_inline(false);
    ui.set_is_current(false);
    ui.post(post(2));
    ui.post(post(3));
    assert_eq!(*log.borrow(), [1]);
    assert_eq!(ui.pending_posts(), 2);
    assert!(!ui.is_current());
    assert_eq!(ui.run_posted(), 2);
    assert_eq!(*log.borrow(), [1, 2, 3]);
    assert_eq!(ui.post_count(), 3);
    // md.win's null-action assertion cannot arise: a Box<dyn FnOnce()> is never null.
}

// ---------------------------------------------------------------------------------------------
// FakeSettingsStore

#[test]
fn logs_writes_and_removals_and_forwards_changed() {
    use md_logic::settings::settings_keys;

    let s = FakeSettingsStore::new();
    let changed = Rc::new(RefCell::new(Vec::<String>::new()));
    {
        let changed = changed.clone();
        s.changed().connect(move |key| changed.borrow_mut().push(key.to_string()));
    }

    s.set_string(settings_keys::PDF_PAGE_SIZE, "letter");
    s.set_string(settings_keys::PDF_PAGE_SIZE, "letter"); // logged, but not a change
    s.set_bool(settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS, true);
    s.remove(settings_keys::PDF_PAGE_SIZE);

    assert_eq!(
        s.writes(),
        [
            (settings_keys::PDF_PAGE_SIZE.to_string(), SettingValue::from("letter")),
            (settings_keys::PDF_PAGE_SIZE.to_string(), SettingValue::from("letter")),
            (settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS.to_string(), SettingValue::from(true)),
        ]
    );
    assert_eq!(s.removals(), [settings_keys::PDF_PAGE_SIZE]);
    assert_eq!(
        *changed.borrow(),
        [settings_keys::PDF_PAGE_SIZE, settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS, settings_keys::PDF_PAGE_SIZE]
    );
    assert_eq!(s.get_string(settings_keys::PDF_PAGE_SIZE), None);
    assert!(s.get_bool(settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS, false));
    assert_eq!(s.keys(), [settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS]);
    assert!(!s.was_written(settings_keys::VIEW_MODE_MEMORY)); // the "Zen never stores" shape of assertion
}

#[test]
fn a_changed_handler_may_read_and_write_the_store() {
    // Rust only: the store must not hold a borrow while it raises `changed`.
    let s = Rc::new(FakeSettingsStore::new());
    let seen = Rc::new(RefCell::new(Vec::new()));
    {
        let (store, seen) = (s.clone(), seen.clone());
        s.changed().connect(move |key| {
            seen.borrow_mut().push((key.to_string(), store.get_string(key)));
            if key == "a" {
                store.set_string("b", "from the handler");
            }
        });
    }
    s.set_string("a", "1");
    assert_eq!(
        *seen.borrow(),
        [("a".to_string(), Some("1".to_string())), ("b".to_string(), Some("from the handler".to_string()))]
    );
}

// ---------------------------------------------------------------------------------------------
// FakeWordCounter

#[test]
fn counts_runs_with_a_letter_or_digit_unless_overridden_and_records_calls() {
    let c = FakeWordCounter::new();
    assert_eq!(c.count("Hello, world \u{2014} 2 words"), 4); // the lone em dash is not a word
    assert_eq!(c.count("   "), 0);
    assert_eq!(c.count(""), 0);
    assert_eq!(c.count("\u{00A0}Привет\u{2003}мир\n"), 2); // .NET's whitespace: NBSP and EM SPACE split

    c.set_override(|_| 42);
    assert_eq!(c.count("anything"), 42);
    assert_eq!(c.calls(), ["Hello, world \u{2014} 2 words", "   ", "", "\u{00A0}Привет\u{2003}мир\n", "anything"]);
}

// ---------------------------------------------------------------------------------------------
// FakeFileIdentity

#[test]
fn canonical_tidies_the_spelling_and_follows_aliases_exactly() {
    // md.win: Canonical_normalises_separators_and_follows_aliases_case_insensitively — Windows'
    // spelling rules; these are Linux's (see FakeFileIdentity).
    let id = FakeFileIdentity::new();
    assert_eq!(id.canonical("/home/u/Docs/a.md"), "/home/u/Docs/a.md");
    assert_eq!(id.canonical("/home/u//Docs/./x/../a.md"), "/home/u/Docs/a.md");
    assert_eq!(id.canonical(" /home/u/Docs/a.md"), " /home/u/Docs/a.md"); // a space is part of a name

    id.alias("/home/u/link/a.md", "/home/u/Docs/a.md");
    assert_eq!(id.canonical("/home/u/link/a.md"), "/home/u/Docs/a.md");
    assert_eq!(id.canonical("/home/u/link//a.md"), "/home/u/Docs/a.md");
    assert_eq!(id.canonical("/home/u/LINK/a.md"), "/home/u/LINK/a.md"); // another file on Linux
    assert_eq!(id.calls().len(), 6);
}

#[test]
fn normalise_keeps_relative_paths_relative_and_never_climbs_above_the_root() {
    for (path, normal) in [
        ("/", "/"),
        ("/..", "/"),
        ("/a/../../b", "/b"),
        ("a/./b/", "a/b"),
        ("../a", "../a"),
        ("a/../..", ".."),
        ("", "."),
        ("a\\b", "a\\b"),
    ] {
        assert_eq!(FakeFileIdentity::normalise(path), normal, "{path:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// FakeClock

#[test]
fn starts_at_the_epoch_and_moves_only_when_told() {
    let clock = FakeClock::new();
    // 2026-01-01 12:00:00 UTC
    assert_eq!(clock.wall_now(), std::time::UNIX_EPOCH + Duration::from_secs(1_767_268_800));
    assert_eq!(clock.wall_now(), FakeClock::epoch());
    assert_eq!(clock.now(), clock.start());

    clock.advance(ms(150));
    assert_eq!(clock.wall_now(), FakeClock::epoch() + ms(150));
    assert_eq!(clock.now(), clock.start() + ms(150));
    assert_eq!(clock.elapsed(), ms(150));

    clock.set_elapsed(Duration::ZERO);
    assert_eq!(clock.wall_now(), FakeClock::epoch());
}

// ---------------------------------------------------------------------------------------------
// AnswerQueue

#[test]
fn an_answer_queue_gives_ready_then_held_then_the_fallback() {
    let q: AnswerQueue<i32> = AnswerQueue::new();
    q.push(1);
    let held = q.hold();
    assert_eq!(q.len(), 2);

    assert_eq!(block_on(q.next_or(|| 0)), 1);
    let second = q.next_or(|| 0);
    held.complete(2);
    assert_eq!(block_on(second), 2);
    assert!(q.is_empty());
    assert_eq!(block_on(q.next_or(|| 0)), 0);
}
