//! The `documents` self-test group (shell-design §13.3): every associated extension through `open`;
//! a second `open` focuses; TextPack / TextBundle imports; autosave in place (inode kept); external
//! edits, atomic replacements, deletes and vim's dance through the real `GioFileWatcher`; Revert;
//! Rename / Move To; the rescue copy; recents; alerts and pickers dismissed by Escape.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P2

use crate::selftest::Harness;

pub async fn run(h: &Harness) {}
