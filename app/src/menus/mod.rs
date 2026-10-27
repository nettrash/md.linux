//! The command surface (shell-design §2): one menu bar per window built from md-logic's
//! `CommandTable`, one `gio::SimpleAction` per `CommandId` in the `app.` or `win.` scope, the
//! accelerators, and About.
//!
//! | Module | What |
//! | --- | --- |
//! | [`menu_bar`] | `MenuBar`: `CommandTable` → `gio::Menu`, `refresh(&ShellSnapshot)` |
//! | [`actions`] | the actions, their scopes, enablement and state |
//! | [`accels`] | pure: `Chord` → GTK accelerator strings (+ tests) |
//! | [`about`] | `adw::AboutDialog` |
//!
//! Owner: P1.

pub mod about;
pub mod accels;
pub mod actions;
pub mod menu_bar;
