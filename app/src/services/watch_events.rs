//! Pure: a `gio::FileMonitor` event → `Changed | Renamed(old, new) | Deleted | Ignore` for every row
//! of shell-design §6.7, vim's rename-away-then-create included — `(event, file, other, our_path,
//! exists_after_250ms)` in, the report out, unit-tested without GTK.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P2
