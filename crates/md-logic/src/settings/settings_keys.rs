//! Every settings key the app writes (shell-design §9), plus the other names under which md keeps
//! state on the machine — port of md.win `SettingsKeys`. PRIVACY.md must enumerate exactly this
//! list. Add a key here first, to the constants, to [`ALL`] and to this table; the tests pin all
//! three against each other.
//!
//! | Key | Type | Default | Written by | Read by | Notes |
//! | --- | --- | --- | --- | --- | --- |
//! | `md.viewModeMemory` | string (`v1` codec, md-core) | absent ⇒ `[]` | the view-mode controller / first-save migrate | `view_mode_memory::lookup` | ≤ 200 entries ≈ 4.6 KB; identity = `sha256("file:" + canonical path)[0..8]` hex |
//! | `md.pdfPageSize` | string (`PageSize` id) | `"a4"` | the two PDF Page Size pickers | Export PDF, Book PDF | unknown → A4 |
//! | `md.bookBookmark` | string (book folder path) | `""` = no book | the book library host (open / close) | Book menu enablement, Book window | the whole record of the open book on Linux |
//! | `md.bookOpensInSeparateWindows` | bool | `false` | Book menu toggle | the book navigator, the stepper | |
//! | `md.bookLastArticle` | string (`/`-separated, relative to the root) | `""` | the book navigator's selection | its reload | |
//! | `md.bookViewMode` | string (`edit`/`split`/`preview`) | `"split"` | Book pane | Book pane | app-wide, never per file |
//! | `md.linux.windowSize.document` **(Linux)** | string `"WxH"`, GTK logical px | absent ⇒ `900x640` | document window close | new document windows | last size |
//! | `md.linux.windowSize.book` **(Linux)** | string `"WxH"`, GTK logical px | absent ⇒ `1000x700` | Book window close | Book window | |
//! | `md.continueLists` | bool | `true` | Edit ▸ Typing ▸ Continue Lists and Tables | the editor (Enter) | the same key on every md port (smart-typing.md §3.1) |
//! | `md.capitalizeSentences` | bool | `true` | Edit ▸ Typing ▸ Capitalize Sentences | the editor (the letter) | the same key on every md port |
//! | `md.wrapSelection` | bool | `true` | Edit ▸ Typing ▸ Wrap Selection When Typing | the editor (formatting.md §4) | 1.6 |
//!
//! Not settings keys: `md.viewMode`, `md.zen`, `md.zenReading` and window placements →
//! [`SESSION_FILE_NAME`]; per-document encoding / newline / scroll positions → session memory
//! only.
//!
//! LINUX DECISIONS: md.win's two Windows-only keys `md.win.windowSize.document` /
//! `md.win.windowSize.book` are spelled `md.linux.windowSize.*` here, in GTK's logical pixels
//! instead of WinUI's effective pixels. md.win's `BookAccessToken` (`md.book`, the
//! `FutureAccessList` token holding the book folder grant) has no counterpart: an unsandboxed
//! `.deb` app opens any folder its user can, so [`BOOK_BOOKMARK`] is the whole record. md.win's
//! `WebView2UserDataFolderName` has none either: where WebKitGTK keeps website data (and the
//! recent-files list, and the store behind this trait) is the GTK app's decision, and the app adds
//! each such name here so PRIVACY.md can list it.

/// string — the `v1` per-file view-mode codec (md-core `view_mode_memory`). Absent ⇒ `[]`.
/// Written by the view-mode controller's set-mode and the first-save migrate branch; read by
/// `view_mode_memory::lookup`. ≤ 200 entries ≈ 4.6 KB; identity = sha256("file:" + canonical
/// path)[0..8] hex.
pub const VIEW_MODE_MEMORY: &str = "md.viewModeMemory";

/// string — a `PageSize` id. Default [`PDF_PAGE_SIZE_DEFAULT`]; an unknown id reads as A4.
/// Written by the two PDF Page Size pickers; read by Export PDF and Book PDF.
pub const PDF_PAGE_SIZE: &str = "md.pdfPageSize";

/// string — the open book's folder path; `""` (or absent) = no book. Written by the book library
/// host when a book is opened or closed; read by Book menu enablement and the Book window.
pub const BOOK_BOOKMARK: &str = "md.bookBookmark";

/// bool — default `false`. The Book menu's "Open Articles in Separate Windows" toggle; read by the
/// book navigator and the stepper.
pub const BOOK_OPENS_IN_SEPARATE_WINDOWS: &str = "md.bookOpensInSeparateWindows";

/// string — the last selected article, `/`-separated relative to the book root (the Mac's
/// spelling); default `""`. Written when the navigator's selection changes; read by its reload.
pub const BOOK_LAST_ARTICLE: &str = "md.bookLastArticle";

/// string — `edit` / `split` / `preview`; default [`BOOK_VIEW_MODE_DEFAULT`]. App-wide, never per
/// file; written and read by the Book pane.
pub const BOOK_VIEW_MODE: &str = "md.bookViewMode";

/// (Linux) string — `"WxH"` in GTK logical pixels, the last document window size; absent ⇒
/// [`DOCUMENT_WINDOW_SIZE_DEFAULT`]. Written when a document window closes; read when a new one
/// is placed. md.win: `md.win.windowSize.document`.
pub const DOCUMENT_WINDOW_SIZE: &str = "md.linux.windowSize.document";

/// (Linux) string — `"WxH"` in GTK logical pixels, the last Book window size; absent ⇒
/// [`BOOK_WINDOW_SIZE_DEFAULT`]. Written when the Book window closes; read by the Book window.
/// md.win: `md.win.windowSize.book`.
pub const BOOK_WINDOW_SIZE: &str = "md.linux.windowSize.book";

/// bool — default [`CONTINUE_LISTS_DEFAULT`] (true). Edit ▸ Continue Lists and Tables: whether
/// Enter in the editor continues a list item, a quote line or a table row (docs/smart-typing.md
/// §1, §3.1). The same key on every md port. Written by the Edit menu toggle; read by the editor
/// on every Enter.
pub const CONTINUE_LISTS: &str = "md.continueLists";

/// bool — default [`CAPITALIZE_SENTENCES_DEFAULT`] (true). Edit ▸ Capitalize Sentences: whether a
/// lowercase letter typed at a line start or after a sentence end is capitalized
/// (docs/smart-typing.md §2, §3.1). The same key on every md port. Written by the Edit menu
/// toggle; read by the editor on every insertion.
pub const CAPITALIZE_SENTENCES: &str = "md.capitalizeSentences";

/// bool — default [`WRAP_SELECTION_DEFAULT`] (true). Edit ▸ Typing ▸ Wrap Selection When Typing:
/// whether one of the four marks (`*`, `_`, the backtick, `$`) typed over a selection wraps it
/// instead of replacing it (docs/formatting.md §4). Written by the Edit menu toggle; read by the
/// editor.
pub const WRAP_SELECTION: &str = "md.wrapSelection";

/// The eleven keys, in §9 order.
pub const ALL: [&str; 11] = [
    VIEW_MODE_MEMORY,
    PDF_PAGE_SIZE,
    BOOK_BOOKMARK,
    BOOK_OPENS_IN_SEPARATE_WINDOWS,
    BOOK_LAST_ARTICLE,
    BOOK_VIEW_MODE,
    DOCUMENT_WINDOW_SIZE,
    BOOK_WINDOW_SIZE,
    CONTINUE_LISTS,
    CAPITALIZE_SENTENCES,
    WRAP_SELECTION,
];

pub const PDF_PAGE_SIZE_DEFAULT: &str = "a4";
pub const BOOK_VIEW_MODE_DEFAULT: &str = "split";
pub const DOCUMENT_WINDOW_SIZE_DEFAULT: &str = "900x640";
pub const BOOK_WINDOW_SIZE_DEFAULT: &str = "1000x700";
pub const CONTINUE_LISTS_DEFAULT: bool = true;
pub const CAPITALIZE_SENTENCES_DEFAULT: bool = true;
pub const WRAP_SELECTION_DEFAULT: bool = true;

// Not settings keys — the other names md stores state under (§9 "Not settings keys").

/// `session.json` — saved documents' mode / Zen / placement and the Book window frame (§1.6).
pub const SESSION_FILE_NAME: &str = "session.json";
