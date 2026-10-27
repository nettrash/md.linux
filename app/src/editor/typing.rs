//! The typing glue (shell-design §3.3): md-logic's `TypingHooks` and `FormattingAdapter` fed at
//! the buffer's `begin-user-action` / `end-user-action`, deletion side from `delete-range`, Enter
//! in a capture-phase key controller, undo / redo / paste / preedit hooks, the capital and the wrap
//! applied on a `Priority::HIGH` idle as their own user actions — with the event sequencing
//! extracted as the pure, unit-tested `UserActionTracker`.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P3
