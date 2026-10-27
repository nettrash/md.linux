//! The process-wide services behind md-logic's seams (shell-design §1.7).
//!
//! | Module | What | Owner |
//! | --- | --- | --- |
//! | [`glib_seams`] | `GlibScheduler` (Scheduler + Clock), `GlibUiThread`, `GlibSpawner`, `SystemClock` | S0 |
//! | [`settings_store`] | `KeyFileSettingsStore` (§9.1) | S0 |
//! | [`paths`] | install and XDG folders (§9.3) | S0 |
//! | [`file_system`] | `StdFileSystem`, `CanonicalFileIdentity` (§6.2) | P2 |
//! | [`file_watcher`] | `GioFileWatcher` (§6.7) | P2 |
//! | [`watch_events`] | pure: monitor event → changed / renamed / deleted | P2 |
//! | [`pickers`] | `GtkPickers` (§6.1) | P2 |
//! | [`alerts`] | `AdwAlerts` (§7.8) | P2 |
//! | [`recent`] | `RecentFiles` (§6.6) | P2 |
//! | [`examples`] | `ExampleLibrary` (§2.3) | P2 |
//! | [`document_ops`] | rename / move, TextBundle / TextPack import helpers (§6.4, §6.5) | P2 |

pub mod alerts;
pub mod document_ops;
pub mod examples;
pub mod file_system;
pub mod file_watcher;
pub mod glib_seams;
pub mod paths;
pub mod pickers;
pub mod recent;
pub mod settings_store;
pub mod watch_events;
