//! The `shell` self-test group (shell-design §13.3): app starts; one window; eight menus in order;
//! every `CommandTable` row once with its title; accels as §2.9; ticks follow the snapshot; Quit
//! keeps drafts and the relaunch restores them; a disabled action's chord reaches the editor; the
//! second-launch hand-off; `--version`.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P1

use crate::selftest::Harness;

pub async fn run(h: &Harness) {}
