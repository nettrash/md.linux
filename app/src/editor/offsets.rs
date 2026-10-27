//! Pure: `GtkTextIter` char offsets ↔ md-logic's UTF-16 offsets over md-core `text::utf16`
//! (shell-design §3.2) — ASCII, Cyrillic, combining marks, emoji, surrogate pairs, both ends, out of
//! range clamped; unit-tested.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P3
