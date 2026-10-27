//! One `gio::SimpleAction` per `CommandId` (shell-design §2.2): kebab-case names in the `app.` or
//! `win.` scope, radio groups as stateful actions, parametrised rows, enablement and ticks from the
//! window's `ShellSnapshot`, the editing chords dispatched to the focused widget, and every
//! accelerator installed once in `startup`.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P1

use std::rc::Rc;

use md_logic::commands::CommandDispatcher;

use crate::app::Shell;
use crate::windows::host::WindowKind;

/// The `win.` actions of one window, each activating `dispatcher` with its `CommandId`.
pub fn install_window_actions(window: &adw::ApplicationWindow, kind: WindowKind, dispatcher: &Rc<CommandDispatcher>) {}

/// The `app.` actions and every accelerator (`startup`).
pub fn install_app_actions(shell: &Rc<Shell>) {}
