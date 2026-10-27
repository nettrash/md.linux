//! The `preview` self-test group (shell-design §13.3): the engines under the CSP; `javascript:` and
//! `onclick` inert; `other.md` cancelled; `#slug` scrolls; one reload per debounce; scroll kept;
//! the dark re-render; scroll sync both ways; a killed web process reloads once, twice gives the
//! notice.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P4

use crate::selftest::Harness;

pub async fn run(h: &Harness) {}
