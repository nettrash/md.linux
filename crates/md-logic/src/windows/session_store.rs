//! Port of md.win `Windows/SessionStore.cs` (`SessionWindow`, `SessionBook`, `SessionState`,
//! `SessionStore`).
//!
//! `session.json` (shell-design.md §1.6) — where the Mac's
//! `@SceneStorage("md.viewMode" / "md.zen" / "md.zenReading")` and its AppKit-restored window
//! frames live. Written on every window close and on Quit, read on a plain first launch.
//!
//! Three rules, each a test: only *saved* documents are listed (an untitled draft is not
//! autosaved, so restoring it would restore nothing); a file that has since gone is skipped
//! silently; and per-window state never goes to the settings store, which is app-wide (§9).
//!
//! LINUX DECISIONS:
//! - The folder is the one the app passes in — its XDG data folder
//!   (`$XDG_DATA_HOME/me.nettrash.md`, i.e. `~/.local/share/me.nettrash.md`), md.win's
//!   `LocalFolder`. Unlike `LocalFolder` it may not exist on a first run: the app creates it
//!   (`LocalFileSystem::ensure_directory`) before the first [`save`], which otherwise just reports
//!   `false`.
//! - The JSON is written as UTF-8 with only the escapes JSON requires. md.win's
//!   `Utf8JsonWriter` also escapes every non-ASCII character and `"<>&'+` as `\uXXXX`; both
//!   spellings read back identically, and the file is per-platform anyway.
//! - The frame is in GTK logical pixels; GTK 4 cannot report a window's position, so `x`/`y` are
//!   what the app records (`0`) — see `window_placement`.

use md_core::document::view_mode::ViewMode;

use super::json::{self, Value};
use super::window_placement::WindowRect;
use crate::documents::file_names;
use crate::seams::FileSystem;
use crate::settings::settings_keys;

/// One restored document window: which file, how it was showing it, and where it was (§1.6).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionWindow {
    /// The saved document. Untitled drafts are never listed (§14 row 8).
    pub path: String,
    /// The window's RAW mode preference, not what a narrow window displayed.
    pub mode: ViewMode,
    /// Zen was on.
    pub zen: bool,
    /// Zen was showing the preview rather than the editor.
    pub zen_reading: bool,
    /// The frame in logical pixels; clamped into the current monitor on restore.
    pub placement: WindowRect,
    /// The window was maximised — the frame is what it would restore to.
    pub maximized: bool,
}

impl SessionWindow {
    /// md.win's positional record constructor.
    pub fn new(
        path: &str,
        mode: ViewMode,
        zen: bool,
        zen_reading: bool,
        placement: WindowRect,
        maximized: bool,
    ) -> Self {
        SessionWindow { path: path.to_string(), mode, zen, zen_reading, placement, maximized }
    }
}

/// The Book window's half of the session: whether it was open and where (§1.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionBook {
    pub open: bool,
    pub placement: WindowRect,
    pub maximized: bool,
}

impl SessionBook {
    pub const fn new(open: bool, placement: WindowRect, maximized: bool) -> Self {
        SessionBook { open, placement, maximized }
    }
}

/// What `session.json` holds. Equality is structural over the windows (md.win had to override a
/// record's reference comparison of the list to get that; a `Vec` compares by value).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SessionState {
    pub windows: Vec<SessionWindow>,
    pub book: Option<SessionBook>,
}

impl SessionState {
    /// No windows, no book — what anything unreadable decodes to.
    pub const EMPTY: SessionState = SessionState { windows: Vec::new(), book: None };

    pub fn new(windows: Vec<SessionWindow>, book: Option<SessionBook>) -> Self {
        SessionState { windows, book }
    }
}

/// The document's `"v"`. A file with any other version reads as empty rather than half-understood.
pub const VERSION: i32 = 1;

/// `session.json` — the same constant `settings_keys::SESSION_FILE_NAME` names for PRIVACY.md.
pub const FILE_NAME: &str = settings_keys::SESSION_FILE_NAME;

/// The full path inside the app's data folder.
pub fn path_in(folder: &str) -> String {
    file_names::combine(folder, FILE_NAME)
}

// ---- codec ----

/// The design's shape, keys in its order. A window with no path is dropped here rather than at the
/// call site: "never record an untitled document" is a property of the file, so the one place that
/// writes the file enforces it.
pub fn encode(state: &SessionState) -> String {
    let mut out = String::new();
    out.push_str(&format!("{{\"v\":{VERSION},\"windows\":["));
    let mut first = true;
    for window in state.windows.iter().filter(|w| !file_names::is_blank(&w.path)) {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str("{\"path\":");
        json::write_string(&mut out, &window.path);
        out.push_str(&format!(
            ",\"mode\":\"{}\",\"zen\":{},\"zenReading\":{},",
            window.mode.raw_value(),
            window.zen,
            window.zen_reading
        ));
        write_frame(&mut out, window.placement, window.maximized);
        out.push('}');
    }
    out.push(']');
    if let Some(book) = state.book {
        out.push_str(&format!(",\"book\":{{\"open\":{},", book.open));
        write_frame(&mut out, book.placement, book.maximized);
        out.push('}');
    }
    out.push('}');
    out
}

/// The inverse. Anything unreadable — absent, truncated, a different `"v"`, a row without a path —
/// is [`SessionState::EMPTY`] (or loses that row): a corrupt session file must cost the user their
/// window layout, never their launch. Fields this build does not know are ignored; missing ones
/// take their defaults.
pub fn decode(json: Option<&str>) -> SessionState {
    let Some(text) = json.filter(|j| !file_names::is_blank(j)) else { return SessionState::EMPTY };
    let Some(root) = json::parse(text) else { return SessionState::EMPTY };
    if !matches!(root, Value::Object(_)) || root.get("v").and_then(Value::as_i32) != Some(VERSION) {
        return SessionState::EMPTY;
    }

    let mut windows = Vec::new();
    if let Some(Value::Array(rows)) = root.get("windows") {
        for row in rows.iter().filter(|r| matches!(r, Value::Object(_))) {
            let Some(path) = string(row, "path").filter(|p| !file_names::is_blank(p)) else { continue };
            windows.push(SessionWindow {
                path: path.to_string(),
                mode: ViewMode::from_raw_value(string(row, "mode")).unwrap_or(ViewMode::WINDOW_DEFAULT),
                zen: boolean(row, "zen"),
                zen_reading: boolean(row, "zenReading"),
                placement: frame(row),
                maximized: boolean(row, "maximized"),
            });
        }
    }
    let book = root.get("book").filter(|b| matches!(b, Value::Object(_))).map(|row| SessionBook {
        open: boolean(row, "open"),
        placement: frame(row),
        maximized: boolean(row, "maximized"),
    });
    SessionState { windows, book }
}

// ---- disk ----

/// Read the session from the app's data folder. Missing or unreadable ⇒ [`SessionState::EMPTY`].
/// Invalid UTF-8 is decoded lossily first, as .NET's `Encoding.UTF8.GetString` does.
///
/// # Panics
///
/// For an empty folder (md.win throws `ArgumentException`).
pub fn load(file_system: &dyn FileSystem, folder: &str) -> SessionState {
    assert!(!folder.is_empty(), "session_store::load: empty folder");
    let path = path_in(folder);
    if !file_system.file_exists(&path) {
        return SessionState::EMPTY;
    }
    match file_system.read_all_bytes(&path) {
        Ok(bytes) => decode(Some(&String::from_utf8_lossy(&bytes))),
        Err(_) => SessionState::EMPTY,
    }
}

/// Write it. False when the write failed — a session that cannot be recorded is a lost layout,
/// never a blocked close, so the caller ignores it.
///
/// # Panics
///
/// For an empty folder.
pub fn save(file_system: &dyn FileSystem, folder: &str, state: &SessionState) -> bool {
    assert!(!folder.is_empty(), "session_store::save: empty folder");
    file_system.write_all_bytes_in_place(&path_in(folder), encode(state).as_bytes()).is_ok()
}

/// The rows whose files are still there (§1.6 "missing files are skipped silently"), and the book
/// only when it was open. This is what a launch restores and what
/// `ActivationDescription::has_restorable_session` is computed from — an empty result means the
/// plain launch opens one untitled window instead.
pub fn restorable(state: &SessionState, file_system: &dyn FileSystem) -> SessionState {
    let windows = state
        .windows
        .iter()
        .filter(|w| !file_names::is_blank(&w.path) && file_system.file_exists(&w.path))
        .cloned()
        .collect();
    SessionState { windows, book: state.book.filter(|b| b.open) }
}

// ---- helpers ----

fn write_frame(out: &mut String, frame: WindowRect, maximized: bool) {
    out.push_str(&format!(
        "\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"maximized\":{}",
        frame.x, frame.y, frame.width, frame.height, maximized
    ));
}

fn frame(row: &Value) -> WindowRect {
    WindowRect::new(int(row, "x"), int(row, "y"), int(row, "w"), int(row, "h"))
}

fn string<'a>(row: &'a Value, name: &str) -> Option<&'a str> {
    match row.get(name) {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

fn boolean(row: &Value, name: &str) -> bool {
    matches!(row.get(name), Some(Value::Bool(true)))
}

fn int(row: &Value, name: &str) -> i32 {
    row.get(name).and_then(Value::as_i32).unwrap_or(0)
}
