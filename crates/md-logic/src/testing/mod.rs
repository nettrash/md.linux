//! The fakes every md-logic test drives its logic through — port of md.win's
//! `tests/Md.App.Logic.Tests/Fakes`. Public (not `#[cfg(test)]`) so the integration tests in
//! `tests/` can `use md_logic::testing::*;`, which also brings in the seams, the settings store
//! trait and the executor, so a test needs no other import for the plumbing.
//!
//! | Fake | Seam | md.win source |
//! | --- | --- | --- |
//! | [`FakeClock`] | [`Clock`] | `FakeClock.cs` |
//! | [`FakeScheduler`] | [`Scheduler`] | `FakeScheduler.cs` |
//! | [`FakeUiThread`] | [`UiThread`] | `FakeUiThread.cs` |
//! | [`FakeFileSystem`] | [`FileSystem`] | `FakeFileSystem.cs` |
//! | [`FakeFileWatcher`] | [`FileWatcher`] | `FakeFileWatcher.cs` |
//! | [`FakeDocumentRegistry`] | [`DocumentRegistry`] | `FakeDocumentRegistry.cs` |
//! | [`FakeFileIdentity`] | [`FileIdentity`] | `FakeFileIdentity.cs` |
//! | [`FakeAlerts`] | [`Alerts`] | `FakeAlerts.cs` |
//! | [`FakePickers`] | [`Pickers`] | `FakePickers.cs` |
//! | [`FakeWordCounter`] | [`WordCounter`] | `FakeWordCounter.cs` |
//! | [`FakePreviewSurface`] | [`PreviewSurface`] | `FakePreviewSurface.cs` |
//! | [`FakeRenderSurface`], [`FakeRenderSurfaceFactory`] | [`RenderSurface`], [`RenderSurfaceFactory`] | `FakeRenderSurface.cs` |
//! | [`FakeSettingsStore`] | [`SettingsStore`] | `FakeSettingsStore.cs` |
//! | [`FakeTextBox`] | the editor the [`TypingHooks`](crate::text::TypingHooks) are fed from | `FakeTextBox.cs` |
//!
//! md.win's `FakeShare` has no counterpart (Linux has no share sheet).
//!
//! How the fakes are shaped, once for all of them:
//! - Every method takes `&self` (state in cells), so a test keeps an `Rc<FakeX>` for scripting and
//!   inspection and hands the logic a clone as `Rc<dyn Seam>` — the coercion is implicit.
//! - Recordings are methods returning an owned snapshot (`fs.writes()`, `alerts.warnings()`), so
//!   no borrow outlives the call. Scripts are `queue_*` methods; a fake answers from its queue,
//!   then with the "nothing happens" answer md.win's fakes give (cancel / don't delete / null).
//! - An asynchronous answer can be held open: `hold_*` queues an [`exec::Completion`] in the
//!   answer's place and returns it; the logic's future stays pending until the test completes it
//!   (C#'s `TaskCompletionSource`). Drive the logic on a [`LocalExecutor`] and pump with
//!   `run_until_stalled` after answering.
//! - C# exceptions are `io::Error`s (file system) or [`RenderError`]s (render surface); a misuse
//!   md.win's fakes throw for (a missing file in a set-up call, `set_title` on an unknown window,
//!   `watch` after `dispose`) panics, as a test bug should.
//! - Dropping a [`TimerHandle`] cancels the timer — unlike ignoring C#'s `IDisposable`. A test
//!   that wants a timer to fire either keeps the handle or calls `detach()`.
//!
//! [`exec::Completion`]: crate::exec::Completion

mod answer_queue;
mod fake_alerts;
mod fake_clock;
mod fake_document_registry;
mod fake_file_identity;
mod fake_file_system;
mod fake_file_watcher;
mod fake_pickers;
mod fake_preview_surface;
mod fake_render_surface;
mod fake_scheduler;
mod fake_settings_store;
mod fake_text_box;
mod fake_ui_thread;
mod fake_word_counter;

pub use answer_queue::{AnswerQueue, Scripted};
pub use fake_alerts::{Confirmation, FakeAlerts, NamePrompt, Warning};
pub use fake_clock::FakeClock;
pub use fake_document_registry::FakeDocumentRegistry;
pub use fake_file_identity::FakeFileIdentity;
pub use fake_file_system::FakeFileSystem;
pub use fake_file_watcher::FakeFileWatcher;
pub use fake_pickers::{FakePickers, SaveCall};
pub use fake_preview_surface::FakePreviewSurface;
pub use fake_render_surface::{FakeRenderSurface, FakeRenderSurfaceFactory};
pub use fake_scheduler::FakeScheduler;
pub use fake_settings_store::FakeSettingsStore;
pub use fake_text_box::FakeTextBox;
pub use fake_ui_thread::FakeUiThread;
pub use fake_word_counter::FakeWordCounter;

pub use crate::event::{Event, HandlerId};
pub use crate::exec::{CancelToken, Cancelled, Completion, LocalBoxFuture, LocalExecutor, Spawner, ready};
pub use crate::seams::*;
pub use crate::settings::{InMemorySettingsStore, SettingValue, SettingsStore};

use std::future::Future;

/// Runs `future` to completion on a fresh [`LocalExecutor`] — for an answer a fake gives at once
/// (`block_on(alerts.ask_save_changes("x".into()))`).
///
/// # Panics
///
/// If the future stalls: a held answer nobody completed.
pub fn block_on<T: 'static>(future: impl Future<Output = T> + 'static) -> T {
    LocalExecutor::new().block_on(future)
}
