//! The `privacy` self-test group (shell-design §13.3): after every other group, `OUT/home` holds
//! only `settings.conf`, `session.json`, the drafts and `recently-used.xbel` — nothing under
//! `.cache/md` or `.local/share/md/webkitgtk`.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P1

use crate::selftest::Harness;

pub async fn run(h: &Harness) {}
