//! Port of md.win `tests/Md.App.Logic.Tests/CommandDispatcherTests.cs`: the dispatcher's three
//! jobs (§2.9) — run what is wired, refuse what is disabled so the chord falls through to the
//! focused widget, and swallow a double fire (md.win's WebView2 forwards accelerators twice ~100 ms
//! apart; GTK was measured firing once, and the guard is kept as a net).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use md_logic::commands::command_table;
use md_logic::commands::{CommandArgument, CommandDispatcher, CommandId, RecentEntry, ShellSnapshot};
use md_logic::testing::*;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

const NONE: &CommandArgument = &CommandArgument::None;

/// md.win's fixture: a scheduler (whose clock the dispatcher reads) and a snapshot the test can
/// swap under the dispatcher.
struct Fixture {
    scheduler: FakeScheduler,
    snapshot: Rc<RefCell<ShellSnapshot>>,
}

impl Fixture {
    fn new() -> Self {
        Fixture {
            scheduler: FakeScheduler::new(),
            snapshot: Rc::new(RefCell::new(ShellSnapshot { has_document: true, ..ShellSnapshot::empty() })),
        }
    }

    fn set(&self, snapshot: ShellSnapshot) {
        *self.snapshot.borrow_mut() = snapshot;
    }

    fn new_dispatcher(&self) -> CommandDispatcher {
        let snapshot = self.snapshot.clone();
        CommandDispatcher::new(self.scheduler.clock().clone(), move || snapshot.borrow().clone())
    }
}

fn counter() -> (Rc<Cell<usize>>, impl Fn() + 'static) {
    let runs = Rc::new(Cell::new(0));
    let r = runs.clone();
    (runs, move || r.set(r.get() + 1))
}

#[test]
fn runs_a_wired_enabled_command_and_reports_it_handled() {
    let f = Fixture::new();
    let dispatcher = f.new_dispatcher();
    let (runs, handler) = counter();
    dispatcher.register(CommandId::Save, handler);

    assert!(dispatcher.try_invoke(CommandId::Save, NONE));
    assert_eq!(runs.get(), 1);
}

#[test]
fn two_invocations_ten_milliseconds_apart_run_once() {
    let f = Fixture::new();
    let dispatcher = f.new_dispatcher();
    let (runs, handler) = counter();
    dispatcher.register(CommandId::Save, handler);

    assert!(dispatcher.try_invoke(CommandId::Save, NONE));
    f.scheduler.advance(ms(10));
    // Handled, so nothing else acts on the duplicate either — that is the whole point.
    assert!(dispatcher.try_invoke(CommandId::Save, NONE));

    assert_eq!(runs.get(), 1);
}

#[test]
fn two_invocations_two_hundred_milliseconds_apart_run_twice() {
    let f = Fixture::new();
    let dispatcher = f.new_dispatcher();
    let (runs, handler) = counter();
    dispatcher.register(CommandId::Save, handler);

    dispatcher.try_invoke(CommandId::Save, NONE);
    f.scheduler.advance(ms(200));
    dispatcher.try_invoke(CommandId::Save, NONE);

    assert_eq!(runs.get(), 2);
}

#[test]
fn the_window_is_exactly_one_hundred_and_fifty_milliseconds() {
    let f = Fixture::new();
    let dispatcher = f.new_dispatcher();
    let (runs, handler) = counter();
    dispatcher.register(CommandId::Save, handler);

    dispatcher.try_invoke(CommandId::Save, NONE);
    f.scheduler.advance(ms(149));
    dispatcher.try_invoke(CommandId::Save, NONE);
    assert_eq!(runs.get(), 1);

    f.scheduler.advance(ms(1));
    dispatcher.try_invoke(CommandId::Save, NONE);
    assert_eq!(runs.get(), 2);
    assert_eq!(CommandDispatcher::DOUBLE_FIRE_WINDOW, ms(150));
}

#[test]
fn the_window_is_per_command_not_global() {
    let f = Fixture::new();
    let dispatcher = f.new_dispatcher();
    let (saves, save) = counter();
    let (prints, print) = counter();
    dispatcher.register(CommandId::Save, save);
    dispatcher.register(CommandId::Print, print);

    dispatcher.try_invoke(CommandId::Save, NONE);
    f.scheduler.advance(ms(10));
    dispatcher.try_invoke(CommandId::Print, NONE);

    assert_eq!(saves.get(), 1);
    assert_eq!(prints.get(), 1);
}

#[test]
fn a_suppressed_duplicate_does_not_extend_the_window() {
    // Held-down keys repeat; the guard measures from the last invocation that ran, so a chord held
    // for a second still repeats rather than locking out.
    let f = Fixture::new();
    let dispatcher = f.new_dispatcher();
    let (runs, handler) = counter();
    dispatcher.register(CommandId::Save, handler);

    dispatcher.try_invoke(CommandId::Save, NONE); // t=0, runs
    f.scheduler.advance(ms(100));
    dispatcher.try_invoke(CommandId::Save, NONE); // t=100, swallowed
    f.scheduler.advance(ms(60));
    dispatcher.try_invoke(CommandId::Save, NONE); // t=160, 160 ms since the last run

    assert_eq!(runs.get(), 2);
}

#[test]
fn an_unwired_command_is_not_handled_so_the_key_reaches_the_control() {
    let f = Fixture::new();
    let dispatcher = f.new_dispatcher();
    assert!(!dispatcher.try_invoke(CommandId::Save, NONE));
    assert!(!dispatcher.can_invoke(CommandId::Save));
}

#[test]
fn a_disabled_command_is_not_handled_and_never_runs() {
    let f = Fixture::new();
    f.set(ShellSnapshot::empty()); // no document
    let dispatcher = f.new_dispatcher();
    let (runs, handler) = counter();
    dispatcher.register(CommandId::Save, handler);

    assert!(!dispatcher.can_invoke(CommandId::Save));
    assert!(!dispatcher.try_invoke(CommandId::Save, NONE));
    assert_eq!(runs.get(), 0);
}

#[test]
fn the_snapshot_is_read_on_every_invocation_never_cached() {
    let f = Fixture::new();
    f.set(ShellSnapshot::empty());
    let dispatcher = f.new_dispatcher();
    let (runs, handler) = counter();
    dispatcher.register(CommandId::Save, handler);
    assert!(!dispatcher.try_invoke(CommandId::Save, NONE));

    f.set(ShellSnapshot { has_document: true, ..ShellSnapshot::empty() });
    assert!(dispatcher.try_invoke(CommandId::Save, NONE));
    assert_eq!(runs.get(), 1);
}

#[test]
fn a_disabled_invocation_leaves_the_guard_alone_so_the_next_enabled_one_runs_immediately() {
    let f = Fixture::new();
    f.set(ShellSnapshot::empty());
    let dispatcher = f.new_dispatcher();
    let (runs, handler) = counter();
    dispatcher.register(CommandId::Save, handler);

    dispatcher.try_invoke(CommandId::Save, NONE); // refused
    f.set(ShellSnapshot { has_document: true, ..ShellSnapshot::empty() });
    assert!(dispatcher.try_invoke(CommandId::Save, NONE)); // same instant, but nothing ran before
    assert_eq!(runs.get(), 1);
}

#[test]
fn the_argument_reaches_the_handler_unchanged() {
    let f = Fixture::new();
    f.set(ShellSnapshot {
        recent_entries: vec![RecentEntry::new("tok", "notes.md", "folder")],
        ..ShellSnapshot::empty()
    });
    let dispatcher = f.new_dispatcher();
    let seen = Rc::new(RefCell::new(None));
    {
        let seen = seen.clone();
        dispatcher.register_with_argument(CommandId::OpenRecentEntry, move |argument| {
            *seen.borrow_mut() = Some(argument.clone());
        });
    }

    assert!(dispatcher.try_invoke(CommandId::OpenRecentEntry, &CommandArgument::Text("tok".into())));
    assert_eq!(*seen.borrow(), Some(CommandArgument::Text("tok".into())));
}

#[test]
fn registering_twice_replaces_the_handler() {
    let f = Fixture::new();
    let dispatcher = f.new_dispatcher();
    let (first, first_handler) = counter();
    let (second, second_handler) = counter();
    dispatcher.register(CommandId::Save, first_handler);
    dispatcher.register(CommandId::Save, second_handler);

    dispatcher.try_invoke(CommandId::Save, NONE);

    assert_eq!(first.get(), 0);
    assert_eq!(second.get(), 1);
}

#[test]
fn execute_is_try_invoke_for_a_menu_click_and_is_debounced_the_same_way() {
    let f = Fixture::new();
    let dispatcher = f.new_dispatcher();
    let (runs, handler) = counter();
    dispatcher.register(CommandId::Save, handler);

    dispatcher.execute(CommandId::Save, NONE);
    dispatcher.execute(CommandId::Save, NONE);
    assert_eq!(runs.get(), 1);

    dispatcher.reset_debounce();
    dispatcher.execute(CommandId::Save, NONE);
    assert_eq!(runs.get(), 2);
}

#[test]
fn the_guard_is_stamped_before_the_handler_runs_so_a_reentrant_fire_is_swallowed() {
    // A command that opens a modal dialog spins the main loop; the duplicate accelerator can arrive
    // while the handler is still on the stack.
    let f = Fixture::new();
    let dispatcher = Rc::new(f.new_dispatcher());
    let runs = Rc::new(Cell::new(0));
    {
        let runs = runs.clone();
        let weak = Rc::downgrade(&dispatcher);
        dispatcher.register(CommandId::Save, move || {
            runs.set(runs.get() + 1);
            if runs.get() < 5 {
                weak.upgrade().unwrap().try_invoke(CommandId::Save, NONE);
            }
        });
    }

    dispatcher.try_invoke(CommandId::Save, NONE);

    assert_eq!(runs.get(), 1);
}

#[test]
fn the_dispatcher_publishes_the_windows_snapshot_for_the_menu_builder() {
    let f = Fixture::new();
    let dispatcher = f.new_dispatcher();
    assert_eq!(dispatcher.snapshot(), *f.snapshot.borrow());
    let dirty = ShellSnapshot { is_dirty: true, ..f.snapshot.borrow().clone() };
    f.set(dirty.clone());
    assert_eq!(dispatcher.snapshot(), dirty);
}

#[test]
fn every_root_accelerator_can_be_wired_and_invoked() {
    // What the shell does with the window's accelerators, once: nothing in the table is missing a
    // handler slot. LINUX: the editing rows are registered too (their handlers act on the focused
    // widget), and Escape is the window's key controller's, not an accelerator (§2.2).
    let f = Fixture::new();
    f.set(ShellSnapshot {
        has_document: true,
        is_saved: true,
        is_dirty: true,
        editor_visible: true,
        can_undo: true,
        can_redo: true,
        has_book: true,
        has_selection: true,
        has_find_query: true,
        is_book_window: false,
        find_bar_open: true,
        ..ShellSnapshot::empty()
    });
    let dispatcher = f.new_dispatcher();
    let seen = Rc::new(RefCell::new(Vec::new()));
    for spec in command_table::root_accelerators() {
        let (id, seen) = (spec.id, seen.clone());
        dispatcher.register(id, move || seen.borrow_mut().push(id));
    }

    for spec in command_table::root_accelerators() {
        dispatcher.try_invoke(spec.id, NONE);
    }

    assert!(command_table::root_accelerators().iter().all(|s| !s.accelerators().is_empty()));
    assert!(seen.borrow().contains(&CommandId::Find));
    assert!(seen.borrow().contains(&CommandId::Copy));
    assert!(!seen.borrow().contains(&CommandId::Escape));

    // Escape, from the window's own controller, through the same dispatcher.
    let escaped = Rc::new(RefCell::new(0));
    {
        let escaped = escaped.clone();
        dispatcher.register(CommandId::Escape, move || *escaped.borrow_mut() += 1);
    }
    assert!(dispatcher.try_invoke(CommandId::Escape, NONE));
    assert_eq!(*escaped.borrow(), 1);
}
