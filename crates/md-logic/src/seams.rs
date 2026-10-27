//! The seams — the shapes every md-logic module codes against, and the only things the GTK app
//! implements for it. Port of md.win `Md.App.Logic.Seams` (frozen there in shell-final.md
//! §13.2); the tests implement them with fakes.
//!
//! Translation rules from the C#:
//! - `Task<T>` → [`LocalBoxFuture<'static, T>`]; arguments are owned so the future can outlive
//!   the call. The app drives the futures on GLib's main context.
//! - `event Action<T>` → an [`Event<T>`] the implementation owns and exposes by reference.
//! - `IDisposable` timer → [`TimerHandle`]: cancelling on drop, `#[must_use]`.
//! - `Guid` window ids → [`WindowId`].
//! - Paths are UTF-8 strings, as in md-core.
//! - Exceptions from file operations → `std::io::Result`.
//!
//! md.win's `IShare` has no counterpart: Linux has no system share sheet (see the deviations in
//! docs/shell-design.md).

use std::fmt;
use std::io;
use std::time::{Duration, Instant};

pub use md_core::book::article_file_system::FileStamp;

pub use crate::event::Event;
use crate::exec::{CancelToken, Cancelled, LocalBoxFuture};

/// A window's identity — md.win's `Guid`. Unique for the life of the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub u64);

/// Wall clock for the debounces (the 150 ms command double-fire guard, the 250 ms derived-text
/// tick). Monotonic: only differences are ever compared.
pub trait Clock {
    fn now(&self) -> Instant;
}

/// A one-shot timer from [`Scheduler::after`]. Dropping it before it fires cancels it (C#
/// disposing the handle); [`TimerHandle::detach`] lets it fire with no handle kept.
#[must_use = "dropping a TimerHandle cancels the timer; call detach() to let it fire"]
pub struct TimerHandle {
    cancel: Option<Box<dyn FnOnce()>>,
}

impl TimerHandle {
    /// A handle whose drop runs `cancel` (the implementation makes it a no-op once fired).
    pub fn new(cancel: impl FnOnce() + 'static) -> Self {
        TimerHandle { cancel: Some(Box::new(cancel)) }
    }

    /// Cancels now (same as dropping).
    pub fn cancel(mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel();
        }
    }

    /// Lets the timer fire; nothing can cancel it any more.
    pub fn detach(mut self) {
        self.cancel = None;
    }
}

impl Drop for TimerHandle {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel();
        }
    }
}

impl fmt::Debug for TimerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimerHandle").field("armed", &self.cancel.is_some()).finish()
    }
}

/// UI-thread timers. App: `glib::timeout_add_local_once`; tests: a fake with manual advance.
/// Everything runs on the UI thread, so callers (autosave, re-render debounce, derived text)
/// never lock.
pub trait Scheduler {
    /// One-shot timer.
    fn after(&self, delay: Duration, action: Box<dyn FnOnce()>) -> TimerHandle;
    /// Runs `action` on the next main-loop turn.
    fn post(&self, action: Box<dyn FnOnce()>);
}

/// The UI thread as a destination. In GTK every callback already arrives on it; kept so the
/// ported logic reads like md.win's.
pub trait UiThread {
    fn post(&self, action: Box<dyn FnOnce()>);
    fn is_current(&self) -> bool;
}

/// The file operations the document session and the book model perform.
pub trait FileSystem {
    fn file_exists(&self, path: &str) -> bool;
    fn directory_exists(&self, path: &str) -> bool;
    fn read_all_bytes(&self, path: &str) -> io::Result<Vec<u8>>;
    /// Overwrite the existing file in place (open with truncate, write) — never
    /// write-then-rename, so the file keeps its inode, permissions, hard links and owner, and the
    /// monitor sees one change.
    fn write_all_bytes_in_place(&self, path: &str, bytes: &[u8]) -> io::Result<()>;
    /// `None` when the file does not exist (a vanished file counts as a stale stamp).
    fn stamp(&self, path: &str) -> Option<FileStamp>;
    fn move_file(&self, from: &str, to: &str) -> io::Result<()>;
    fn delete(&self, path: &str) -> io::Result<()>;
    fn enumerate_entries(&self, directory: &str) -> io::Result<Vec<String>>;
}

/// One file watched for external edits. App: a `gio::FileMonitor`; tests: a fake. Events are
/// hints only: the stamp check before every write is the actual guard.
pub trait FileWatcher {
    /// Watch this file; replaces any previous target.
    fn watch(&self, file_path: &str);
    fn stop(&self);
    /// Args: the path.
    fn changed(&self) -> &Event<str>;
    /// Args: (old path, new path).
    fn renamed(&self) -> &Event<(String, String)>;
    /// Args: the path.
    fn deleted(&self) -> &Event<str>;
}

/// Which window owns which file (canonical path, compared exactly — Linux paths are
/// case-sensitive), for the "already open → activate that window" rule and the book's hand-off.
pub trait DocumentRegistry {
    fn owning(&self, canonical_path: &str) -> Option<WindowId>;
    fn register(&self, window: WindowId, canonical_path: &str);
    fn unregister(&self, window: WindowId);
    /// Every open window, for the Window menu — document titles plus "Book".
    fn windows(&self) -> Vec<(WindowId, String)>;
    fn changed(&self) -> &Event<()>;
}

/// The canonical spelling of a path (symlinks and `..` resolved), so two spellings of one file
/// share one view-mode identity and one registry key.
pub trait FileIdentity {
    fn canonical(&self, path: &str) -> String;
}

/// The answer to "Do you want to save the changes…?".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseChoice {
    Save,
    DontSave,
    Cancel,
}

/// The dialogs the logic needs answered. App: `adw::AlertDialog`s, one at a time per window;
/// tests: scripted fakes. Titles and messages come from `strings`.
pub trait Alerts {
    /// Title + message + OK.
    fn warn(&self, title: String, message: String) -> LocalBoxFuture<'static, ()>;
    /// A name prompt (Rename, New Chapter, New Article, New Book); `None` when cancelled.
    fn prompt_name(
        &self,
        title: String,
        message: String,
        initial: String,
        accept_label: String,
    ) -> LocalBoxFuture<'static, Option<String>>;
    /// Delete / Cancel with Cancel as the default.
    fn confirm_delete(&self, title: String, message: String) -> LocalBoxFuture<'static, bool>;
    /// Save / Don't Save / Cancel.
    fn ask_save_changes(&self, title: String) -> LocalBoxFuture<'static, CloseChoice>;
    /// Replace / Cancel (the TextBundle folder export).
    fn confirm_replace(&self, message: String) -> LocalBoxFuture<'static, bool>;
}

/// A labelled group of extensions for a save dialog ("Markdown Document" → `.md`, …).
/// Extensions carry the dot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTypeChoice {
    pub label: String,
    pub extensions: Vec<String>,
}

/// The system file dialogs reduced to paths. App: `gtk::FileDialog`; tests: a fake.
/// Extensions carry the dot (".md").
pub trait Pickers {
    /// Multi-select open; empty when cancelled.
    fn open_files(&self, extensions: Vec<String>) -> LocalBoxFuture<'static, Vec<String>>;
    /// Save dialog; `None` when cancelled. Unlike WinRT's, GTK's dialog does not create the file.
    fn save_file(
        &self,
        suggested_name: String,
        choices: Vec<FileTypeChoice>,
        default_extension: String,
    ) -> LocalBoxFuture<'static, Option<String>>;
    /// Folder dialog; `None` when cancelled.
    fn pick_folder(&self, title: Option<String>) -> LocalBoxFuture<'static, Option<String>>;
}

/// The footer's word count.
pub trait WordCounter {
    fn count(&self, text: &str) -> usize;
}

/// The live preview as the coordinator sees it: the HTML the asset scheme serves as
/// `index.html`, whether the pane is shown, navigate / reload / evaluate, and the outcome of each
/// navigation.
pub trait PreviewSurface {
    /// What `index.html` returns on the next request.
    fn html(&self) -> String;
    fn set_html(&self, html: String);
    /// False while the pane is collapsed (Edit mode): the coordinator only records "stale".
    fn is_shown(&self) -> bool;
    fn navigate(&self, url: &str);
    fn reload(&self);
    /// The script's result serialised as JSON (`"1"` with quotes; `null` as the four letters).
    fn eval(&self, script: String) -> LocalBoxFuture<'static, String>;
    /// Args: whether the navigation succeeded.
    fn navigation_completed(&self) -> &Event<bool>;
}

/// A CSS-pixel rectangle (the rich-element rects the EPUB snapshotter reads).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RectD {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The page a PDF is laid out on, in inches: width and height = pt / 72, portrait, and 0.5 in
/// margins all round (the design's decision; the Mac inherits Page Setup, Android uses 0.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrintGeometry {
    pub page_width_in: f64,
    pub page_height_in: f64,
    pub margin_in: f64,
}

/// Which HTML a renderer serves: `Screen` — the live preview, Core HTML plus the app-appended
/// typewriter style; `Paper` — Print and PDF, Core HTML with `export: true` plus the same style;
/// `Export` — HTML / EPUB / SVG, pure Core HTML so nothing Linux-specific reaches a file another
/// port pins byte for byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RenderKind {
    Screen,
    Paper,
    Export,
}

/// A failed render-surface operation (the message is for the log, not the user).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderError {
    Cancelled,
    Failed(String),
}

impl From<Cancelled> for RenderError {
    fn from(_: Cancelled) -> Self {
        RenderError::Cancelled
    }
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RenderError::Cancelled => f.write_str("cancelled"),
            RenderError::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for RenderError {}

/// One offscreen renderer for one export: load + render-complete wait, eval, region snapshot,
/// height for grow-to-content, PDF. App: a fresh `webkit6::WebView` per export; tests: a fake.
/// Closed after the export.
pub trait RenderSurface {
    /// Load the served HTML and wait for `data-md-render-complete` (a timeout is success).
    fn load(&self, html: String, cancel: CancelToken) -> LocalBoxFuture<'static, Result<(), RenderError>>;
    /// The script's result serialised as JSON.
    fn eval(&self, script: String) -> LocalBoxFuture<'static, Result<String, RenderError>>;
    /// PNG of `css_rect` at `scale`.
    fn capture_region_png(&self, css_rect: RectD, scale: f64) -> LocalBoxFuture<'static, Result<Vec<u8>, RenderError>>;
    fn set_height(&self, css_px: f64) -> LocalBoxFuture<'static, Result<(), RenderError>>;
    /// The PDF bytes.
    fn pdf(&self, geometry: PrintGeometry) -> LocalBoxFuture<'static, Result<Vec<u8>, RenderError>>;
    /// Releases the web view (C#'s `DisposeAsync`).
    fn close(&self) -> LocalBoxFuture<'static, ()>;
}

/// Makes one [`RenderSurface`] per export; the kind decides which HTML entry point it serves.
pub trait RenderSurfaceFactory {
    fn create(&self, kind: RenderKind) -> LocalBoxFuture<'static, Result<Box<dyn RenderSurface>, RenderError>>;
}
