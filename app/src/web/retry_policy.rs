//! Pure: a port of the Mac's `PreviewRetryPolicy` (shell-design §4.9) with its Swift tests — the
//! first web-process death reloads, a second with no completed render between them gives up with
//! the quiet line, an edit buys one attempt, an `mdRender` completion resets the count.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P4
