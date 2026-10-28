//! Port of md.win `Md.App.Logic.Commands`: the declarative command surface (shell-design §2) —
//! every command once ([`CommandId`], [`command_table`]), when it is live and ticked
//! ([`command_enablement`] over a window's [`ShellSnapshot`]), how a chord is spelled for GTK
//! ([`Chord`]), and the per-window [`CommandDispatcher`] with the double-fire guard.
//!
//! | Module | md.win source |
//! | --- | --- |
//! | [`command_id`] | `CommandId.cs` (`CommandId`, `CommandKind`) |
//! | [`chord`] | `Chord.cs` (`KeyModifiers`, `VirtualKeys` → [`keys`], `Chord`) |
//! | [`command_table`] | `CommandTable.cs` (`MenuPath`, `CommandSpec`, `MenuNode`, `CommandTable`) |
//! | [`shell_snapshot`] | `ShellSnapshot.cs` (`RecentEntry`, `DiagramRef`, `ShellSnapshot`) |
//! | [`command_enablement`] | `CommandEnablement.cs` |
//! | [`command_dispatcher`] | `CommandDispatcher.cs` |
//!
//! How the GTK shell uses it: one `gio::SimpleAction` per [`CommandId`] (named
//! [`CommandId::action_name`], in the `app.` or `win.` group [`CommandId::scope`] names; stateful
//! for the toggles and radios), activated through [`CommandDispatcher::execute`]; `set_enabled`
//! from [`command_enablement::is_enabled`] and the state from [`command_enablement::is_checked`] /
//! [`command_enablement::is_row_checked`] on every snapshot change (a disabled action's accelerator
//! lets the key through, which is the fall-through md.win's `TryInvoke → false` gives); the menu
//! model from [`command_table::menus`], a separator starting a new `GMenu` section;
//! `set_accels_for_action` with [`CommandSpec::accelerators`] (first chord shown, a second one
//! registered after it) for every row of [`command_table::root_accelerators`] — the editing rows
//! included, whose handlers act on the focused widget ([`CommandSpec::dispatches_to_focus`]) — and
//! Escape from the window's own key controller ([`CommandKind::WindowKey`]).

pub mod chord;
pub mod command_dispatcher;
pub mod command_enablement;
pub mod command_id;
pub mod command_table;
pub mod shell_snapshot;

pub use chord::{Chord, KeyModifiers, keys};
pub use command_dispatcher::{CommandArgument, CommandDispatcher};
pub use command_id::{ActionScope, CommandId, CommandKind};
pub use command_table::{CommandSpec, MenuNode, MenuPath};
pub use shell_snapshot::{DiagramRef, RecentEntry, ShellSnapshot, WindowTitle};
