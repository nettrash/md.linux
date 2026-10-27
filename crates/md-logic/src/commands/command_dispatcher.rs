//! Port of md.win `Commands/CommandDispatcher.cs`: one per window — the handlers behind the ids,
//! the enablement gate, and the double-fire guard.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use md_core::markdown::block::{NoteEntry, OutlineEntry};

use super::command_enablement;
use super::command_id::CommandId;
use super::shell_snapshot::ShellSnapshot;
use crate::seams::{Clock, WindowId};

/// Which row of a dynamic command an invocation is for — md.win's `object? argument`, typed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum CommandArgument {
    /// The commands that take none (every accelerator).
    #[default]
    None,
    /// An Open Recent token, an example file name, a `PageSize` id.
    Text(String),
    /// A Go ▸ Contents row.
    Outline(OutlineEntry),
    /// A Go ▸ Notes row.
    Note(NoteEntry),
    /// A Diagram as SVG row: the diagram's ordinal.
    Diagram(usize),
    /// A Window menu row.
    Window(WindowId),
}

type Handler = Rc<dyn Fn(&CommandArgument)>;

/// One per window: the handlers behind the ids, the enablement gate, and the double-fire guard.
///
/// The guard is md.win's: WinUI's `WebView2` forwards accelerator keys to XAML twice, roughly
/// 100 ms apart, while the preview has focus (microsoft-ui-xaml #6231), which would open two
/// pickers or print two copies. A second invocation of the same command inside
/// [`CommandDispatcher::DOUBLE_FIRE_WINDOW`] is swallowed and reported as *handled*. On Linux the
/// spike measured every accelerator firing exactly once from either `GtkTextView` or
/// `WebKitWebView` (docs/linux-facts.md §1), so the guard has nothing to catch; it is kept because
/// 150 ms is far below a human repeat and a GTK or WebKit build that did double-fire would be
/// caught here rather than in a second print job.
///
/// No timer: the window is measured against the last invocation that was actually let through, so
/// a held-down chord repeats at 150 ms rather than being locked out for as long as it is held.
pub struct CommandDispatcher {
    clock: Rc<dyn Clock>,
    snapshot: Box<dyn Fn() -> ShellSnapshot>,
    handlers: RefCell<HashMap<CommandId, Handler>>,
    last_invoked: RefCell<HashMap<CommandId, Instant>>,
}

impl fmt::Debug for CommandDispatcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandDispatcher").field("handlers", &self.handlers.borrow().len()).finish()
    }
}

impl CommandDispatcher {
    /// The per-command debounce of §2.9.
    pub const DOUBLE_FIRE_WINDOW: Duration = Duration::from_millis(150);

    /// `clock` is the wall clock (the app's monotonic clock, tests a `FakeClock`); `snapshot` reads
    /// the window's current [`ShellSnapshot`] — called on every invocation, never cached.
    pub fn new(clock: Rc<dyn Clock>, snapshot: impl Fn() -> ShellSnapshot + 'static) -> Self {
        CommandDispatcher {
            clock,
            snapshot: Box::new(snapshot),
            handlers: RefCell::new(HashMap::new()),
            last_invoked: RefCell::new(HashMap::new()),
        }
    }

    /// The window's snapshot as of now.
    pub fn snapshot(&self) -> ShellSnapshot {
        (self.snapshot)()
    }

    /// Wires a command that takes no argument. Registering the same id twice replaces the handler
    /// rather than panicking — a window that rebuilds its wiring must not have to unwire first.
    pub fn register(&self, id: CommandId, handler: impl Fn() + 'static) {
        self.register_with_argument(id, move |_| handler());
    }

    /// Wires a command whose handler reads the row's [`CommandArgument`].
    pub fn register_with_argument(&self, id: CommandId, handler: impl Fn(&CommandArgument) + 'static) {
        self.handlers.borrow_mut().insert(id, Rc::new(handler));
    }

    /// True when the command is both wired and enabled — what a menu row's sensitivity reads.
    pub fn can_invoke(&self, id: CommandId) -> bool {
        self.handlers.borrow().contains_key(&id) && command_enablement::is_enabled(id, &self.snapshot())
    }

    /// Runs the command if it is wired and enabled. Returns what the accelerator's activation
    /// should report: false when nothing was wired or the command is disabled (the key falls
    /// through to the focused widget), true when it ran **and** when it was swallowed as a double
    /// fire.
    pub fn try_invoke(&self, id: CommandId, argument: &CommandArgument) -> bool {
        // The Rc is cloned out so no borrow is held while the handler runs: a handler may invoke
        // (or re-register) commands itself.
        let Some(handler) = self.handlers.borrow().get(&id).cloned() else { return false };
        if !command_enablement::is_enabled(id, &self.snapshot()) {
            return false;
        }

        let now = self.clock.now();
        // `checked_duration_since` is `None` when the clock went back: that is no double fire.
        let last = self.last_invoked.borrow().get(&id).copied();
        if let Some(since) = last.and_then(|last| now.checked_duration_since(last))
            && since < Self::DOUBLE_FIRE_WINDOW
        {
            return true;
        }

        // Stamped before the handler runs: a command that opens a modal dialog must not let the
        // duplicate through while it is awaited.
        self.last_invoked.borrow_mut().insert(id, now);
        handler(argument);
        true
    }

    /// A menu row's activation: the same path as a chord, minus the caller's interest in the
    /// answer.
    pub fn execute(&self, id: CommandId, argument: &CommandArgument) {
        self.try_invoke(id, argument);
    }

    /// Forgets the debounce history — for tests, and for a window that has been idle behind a
    /// modal.
    pub fn reset_debounce(&self) {
        self.last_invoked.borrow_mut().clear();
    }
}
