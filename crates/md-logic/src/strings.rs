//! Every user-facing string the design fixes verbatim — port of md.win `Md.App.Logic.Strings`,
//! shared by the modules so no dialog spells its own. Copied from md.macOS byte for byte: U+2026
//! ellipsis, U+2014 em dash, U+00B7 middle dot, U+00A9 copyright sign, U+201C/U+201D curly quotes
//! and U+2019 where the Mac has them — written as escapes below so a font or an editor cannot
//! quietly swap them. Menu item titles are not here: they live in the command table.
//!
//! md.win's nested static classes are modules: `Strings.Buttons.DontSave` →
//! [`buttons::DONT_SAVE`], `Strings.Documents.CouldNotSave(e)` → [`documents::could_not_save`].
//!
//! LINUX DECISIONS (where md.win's wording is Windows'):
//! - [`documents::INVALID_NAME_MESSAGE`] is the Mac's sentence, because md-core's
//!   `book_folder::is_valid_name` refuses what the Mac refuses (`/` and `:`, plus NUL and the
//!   names `.` and `..`, which nobody types), not Win32's nine characters, trailing dot or space.
//! - [`help::SUPPORT_URL`] / [`help::PRIVACY_URL`] point at a `linux/` folder on nettrash.me, the
//!   site's one-folder-per-channel layout (`appstore/`, `msstore/`, `play/`); md.win's `msstore/`
//!   pages describe the Microsoft Store edition.
//! - The chords in the book tooltips ([`books::PREVIOUS_ARTICLE_TOOLTIP`],
//!   [`books::NEXT_ARTICLE_TOOLTIP`], [`books::EMPTY_MESSAGE`]) are Alt+Up / Alt+Down, the command
//!   table's Previous / Next Article, spelled as GTK labels them (`Chord::display_text`). The Mac's
//!   ⌃⌘↑ / ⌃⌘↓ became Ctrl+Alt+Up / Down on Windows, which GNOME binds to workspace switching; see
//!   `commands::command_table`. A test pins these three strings to the table's chords.

/// The app's name.
pub const APP_NAME: &str = "md";
pub const ELLIPSIS: &str = "\u{2026}";
pub const EM_DASH: &str = "\u{2014}";

/// Untitled windows are numbered per process: "Untitled", "Untitled 2", … (§6.7).
pub const UNTITLED: &str = "Untitled";

/// "Untitled" for the first (and for 0), "Untitled {n}" after it.
pub fn untitled_numbered(ordinal: usize) -> String {
    if ordinal <= 1 { UNTITLED.to_string() } else { format!("{UNTITLED} {ordinal}") }
}

/// Appended to the window title while the document differs from its last explicit save (§6.7).
pub const EDITED_SUFFIX: &str = " \u{2014} Edited";

/// File ▸ Duplicate's window title (§6.4).
pub fn duplicate_title(name: &str) -> String {
    format!("{name} copy")
}

/// The Book window's title with no book, and its Window-menu row.
pub const BOOK: &str = "Book";

/// The editor's placeholder, same face, size and inset as the text (§3.1).
pub const EDITOR_PLACEHOLDER: &str = "# Start writing\u{2026}";

/// The footer, U+00B7, no pluralisation (§5.5).
pub fn footer(words: usize, characters: usize) -> String {
    format!("{words} words \u{00B7} {characters} characters")
}

/// A note whose first non-empty line is missing (§5.6).
pub const EMPTY_NOTE: &str = "(empty note)";

/// Dialog and bar buttons.
pub mod buttons {
    pub const OK: &str = "OK";
    pub const SAVE: &str = "Save";
    pub const DONT_SAVE: &str = "Don't Save";
    pub const CANCEL: &str = "Cancel";
    pub const REPLACE: &str = "Replace";
    pub const CREATE: &str = "Create";
    pub const DELETE: &str = "Delete";
    pub const RETRY: &str = "Retry";
    pub const SAVE_AS: &str = "Save As\u{2026}";
    pub const RELOAD_FROM_DISK: &str = "Reload from Disk";
    pub const KEEP_MY_VERSION: &str = "Keep My Version";
    pub const DONE: &str = "Done";
    pub const SHOW_WINDOW: &str = "Show Window";
    pub const OPEN_BOOK: &str = "Open Book\u{2026}";
    pub const CLEAR_MENU: &str = "Clear Menu";
}

/// §6.3–§6.6: the document session's alerts, the conflict and save-error bars, the close dialog,
/// rescue copies.
pub mod documents {
    pub const COULD_NOT_OPEN: &str = "The document could not be opened.";
    pub const TEXT_PACK_UNREADABLE: &str = "The TextPack could not be read.";

    /// The conflict bar's title; its two buttons are [`super::buttons::RELOAD_FROM_DISK`] /
    /// [`super::buttons::KEEP_MY_VERSION`].
    pub const FILE_CHANGED_ON_DISK: &str = "The file changed on disk.";

    /// The save-error bar: "Couldn’t save — {error}" (U+2019, U+2014); Retry is the action button,
    /// Save As… sits in the content.
    pub fn could_not_save(error: &str) -> String {
        format!("Couldn\u{2019}t save \u{2014} {error}")
    }

    /// The close dialog for an untitled, dirty document; buttons Save / Don't Save / Cancel.
    pub fn save_changes_question(title: &str) -> String {
        format!("Do you want to save the changes made to the document \u{201C}{title}\u{201D}?")
    }

    /// "{stem} (rescued){ext}", then "(rescued 2)", … — 100 attempts before giving up (§6.3).
    /// `extension_with_dot` as `Path.GetExtension` returns it ("" or ".md").
    pub fn rescue_copy_name(stem: &str, extension_with_dot: &str, attempt: usize) -> String {
        if attempt <= 1 {
            format!("{stem} (rescued){extension_with_dot}")
        } else {
            format!("{stem} (rescued {attempt}){extension_with_dot}")
        }
    }

    pub fn rescue_failed_title(name: &str) -> String {
        format!("Could not save \u{201C}{name}\u{201D}")
    }

    pub fn rescue_kept_message(file: &str) -> String {
        format!("Your text was kept as \u{201C}{file}\u{201D} in the same folder.")
    }

    /// Open Recent: the recent-files entry no longer resolves (§6.6).
    pub fn file_not_found(name: &str) -> String {
        format!("The file \u{201C}{name}\u{201D} could not be found.")
    }

    /// The fallback for a save failure the file system gave no message of its own for. Two
    /// wordings because one session serves both surfaces (§6.3): a document window says
    /// "document", the book's detail pane says "article" — the Mac's own string.
    pub const COULD_NOT_WRITE_DOCUMENT: &str = "The document could not be written.";

    pub const COULD_NOT_WRITE_ARTICLE: &str = "The article could not be written.";

    pub const RENAME_TITLE: &str = "Rename";
    pub const RENAME_MESSAGE: &str = "Only the name changes \u{2014} the file extension is kept.";
    pub const COULD_NOT_RENAME: &str = "Could not rename";

    /// LINUX DECISION: the Mac's sentence (md.macOS `BookNavigator.renameItem`), the rule
    /// md-core's `book_folder::is_valid_name` enforces. md.win's is `A name cannot contain \ / : *
    /// ? " < > | or end with a dot or a space.` — Win32's rule, which Linux does not have.
    pub const INVALID_NAME_MESSAGE: &str = "A name cannot contain \"/\" or \":\".";
}

/// §7: alert titles and messages of the exports, the print overlay and the save dialogs' type
/// labels.
pub mod exports {
    pub const COULD_NOT_GENERATE_PDF: &str = "Could not generate PDF";
    pub const COULD_NOT_EXPORT_PDF: &str = "Could not export PDF";
    pub const COULD_NOT_EXPORT_HTML: &str = "Could not export HTML";
    pub const COULD_NOT_EXPORT_LATEX: &str = "Could not export LaTeX";
    pub const COULD_NOT_EXPORT_SVG: &str = "Could not export SVG";
    pub const COULD_NOT_EXPORT_TEXT_BUNDLE: &str = "Could not export TextBundle";
    pub const COULD_NOT_EXPORT_EPUB: &str = "Could not export EPUB";

    pub const PAGE_NOT_CAPTURED: &str = "The rendered page could not be captured.";
    pub const PDF_PAGES_NOT_PRODUCED: &str = "The PDF pages could not be produced.";
    pub const RICH_IMAGES_NOT_CAPTURED: &str = "The rich-content images could not be captured.";
    pub const DIAGRAM_NOT_CAPTURED: &str = "This diagram couldn't be captured \u{2014} it may have failed to render.";

    /// The folder dialog's title for Export ▸ TextBundle… (§7.7).
    pub const CHOOSE_TEXT_BUNDLE_FOLDER: &str = "Choose where to keep the TextBundle";

    pub fn folder_exists(name: &str) -> String {
        format!("A folder named \u{201C}{name}\u{201D} already exists.")
    }

    /// Print overlay: re-show the dialog / dismiss (§7.2).
    pub const PRINT_AGAIN: &str = "Print\u{2026}";

    // The save dialog's type labels, in the Mac's order (§6.1, §7).
    pub const MARKDOWN_DOCUMENT: &str = "Markdown Document";
    pub const PLAIN_TEXT: &str = "Plain Text";
    pub const PLANT_UML_DIAGRAM: &str = "PlantUML Diagram";
    pub const GRAPHVIZ_DOT_GRAPH: &str = "Graphviz DOT Graph";
    pub const PDF: &str = "PDF";
    pub const HTML: &str = "HTML";
    pub const EPUB: &str = "EPUB";
    pub const SVG: &str = "SVG";
    pub const LATEX: &str = "LaTeX";

    /// §7.1: the accessible name of the footer's progress indicator, which has no visible label.
    /// Deliberately not "Exporting" — the indicator covers every serialised output, not only the
    /// Export menu's (md.win's two Share rows among them; Linux has no share sheet).
    pub const WORKING: &str = "Working\u{2026}";
}

/// §8: the Book window's prompts, placeholders, tooltips and alerts.
pub mod books {
    pub const NO_BOOK_OPEN: &str = "No Book Open";
    pub const NO_BOOK_OPEN_MESSAGE: &str =
        "A book is a folder: its subfolders are chapters and its Markdown files are articles.";

    pub const NEW_BOOK_TITLE: &str = "New Book";
    pub const NEW_BOOK_DEFAULT_NAME: &str = "My Book";
    pub const NEW_BOOK_MESSAGE: &str =
        "Name your book and choose where to keep it. Chapters are folders inside it; articles are Markdown files.";
    pub const COULD_NOT_CREATE_BOOK: &str = "Could not create book";
    pub const CHOOSE_EXAMPLE_BOOK_FOLDER: &str = "Choose where to keep the example book. It is an ordinary book folder \u{2014} chapters inside, Markdown articles in each \u{2014} yours to edit.";
    pub const CHOOSE_BOOK_FOLDER: &str =
        "Choose a folder to open as a book. Its subfolders are chapters; its Markdown files are articles.";
    pub const COULD_NOT_UNPACK_EXAMPLE_BOOK: &str = "Could not unpack example book";
    pub const COULD_NOT_REORDER: &str = "Could not reorder";

    pub const NEW_CHAPTER_TITLE: &str = "New Chapter";
    pub const NEW_CHAPTER_MESSAGE: &str =
        "A chapter is a folder. Start the name with a number (\"02 \u{2026}\") to place it in the reading order.";
    pub const NEW_ARTICLE_TITLE: &str = "New Article";
    pub const NEW_ARTICLE_MESSAGE: &str = "A Markdown file is created, started with a matching heading.";
    pub const RENAME_MESSAGE: &str =
        "Only the name changes \u{2014} the ordering number and the file extension are kept.";

    pub fn delete_question(name: &str) -> String {
        format!("Delete \u{201C}{name}\u{201D}?")
    }

    pub const DELETE_CHAPTER_MESSAGE: &str = "The chapter folder and every article in it will be deleted.";
    pub const DELETE_ARTICLE_MESSAGE: &str = "The article file will be deleted.";

    // Sidebar rows and context menu (§8.3).
    pub const NEW_ARTICLE_ROW: &str = "New Article\u{2026}";
    pub const NEW_CHAPTER_BUTTON: &str = "New Chapter\u{2026}";
    pub const OPEN_IN_NEW_WINDOW: &str = "Open in New Window";
    pub const RENAME_ITEM: &str = "Rename\u{2026}";
    pub const MOVE_UP: &str = "Move Up";
    pub const MOVE_DOWN: &str = "Move Down";
    pub const DELETE_ITEM: &str = "Delete\u{2026}";

    // Tooltips (§8.3, §8.4). The two chords must match the command table's Previous / Next
    // Article bindings (see the module comment).
    pub const NEW_CHAPTER_TOOLTIP: &str = "Create a new chapter folder in the book";
    pub const VIEW_MODE_TOOLTIP: &str = "Switch between editing, split and preview";
    pub const PREVIOUS_ARTICLE_TOOLTIP: &str = "The previous article in reading order (Alt+Up)";
    pub const NEXT_ARTICLE_TOOLTIP: &str = "The next article in reading order (Alt+Down)";
    pub const CONTENTS_TOOLTIP: &str = "Jump to a heading";
    pub const NOTES_TOOLTIP: &str = "Jump to a private author note";
    pub const SHARE_TOOLTIP: &str = "Compile the whole book into a PDF, an EPUB, or print it";
    pub const OPEN_ARTICLES_IN_SEPARATE_WINDOWS: &str = "Open Articles in Separate Windows";

    // The detail toolbar's Share menu (§8.4). Deliberately NOT the Book menu's wording — the Mac
    // names the same five actions differently there ("Share Book as PDF", "PDF…"), and both
    // spellings ship as they are. `SHARE_AS_PDF` is the share-sheet row: kept for the record, but
    // Linux has no share sheet to hand the PDF to (see `seams`).
    pub const SHARE_AS_PDF: &str = "Share as PDF";
    pub const EXPORT_AS_PDF: &str = "Export as PDF\u{2026}";
    pub const PDF_PAGE_SIZE_LABEL: &str = "PDF Page Size";
    pub const EXPORT_AS_EPUB: &str = "Export as EPUB\u{2026}";
    pub const EXPORT_AS_LATEX: &str = "Export as LaTeX\u{2026}";
    pub const PRINT_ROW: &str = "Print\u{2026}";

    // Detail-pane stages (§8.5).
    pub const HANDOFF_TITLE: &str = "Open in Its Own Window";

    pub fn handoff_message(title: &str) -> String {
        format!(
            "\u{201C}{title}\u{201D} is open as a document window, and that window owns the file while it stays open. Close it to write here again."
        )
    }

    pub const UNREADABLE_TITLE: &str = "Could Not Read the Article";

    pub fn unreadable_message(title: &str) -> String {
        format!("\u{201C}{title}\u{201D} could not be read. It may have been moved or deleted outside the book.")
    }

    pub const EMPTY_TITLE: &str = "Select an Article";
    pub const EMPTY_MESSAGE: &str =
        "Choose an article in the sidebar to write here. Alt+Up and Alt+Down move through the book in reading order.";

    /// The delete alert's title when the file system refuses (book.md §6.2; the Mac's own "Could
    /// not delete" + the OS message).
    pub const COULD_NOT_DELETE: &str = "Could not delete";

    // Outputs (§8.7).
    pub const COULD_NOT_COMPILE_BOOK: &str = "Could not compile book";
    pub const NO_BOOK_ACCESSIBLE: &str = "No book is open, or the book folder is not accessible.";

    pub fn article_unreadable(name: &str) -> String {
        format!("The article \"{name}\" could not be read.")
    }
}

/// §5.4: the Zen capsule's tooltips.
pub mod zen {
    pub const WRITE: &str = "Write";
    pub const READ: &str = "Read";
    pub const EXIT: &str = "Exit Zen Mode";
}

/// §3.5: the find bar's buttons.
pub mod find {
    pub const NEXT: &str = "Next";
    pub const PREVIOUS: &str = "Previous";
    pub const DONE: &str = "Done";

    /// §3.5: replaces the hit the editor is standing on, then moves to the next one.
    pub const REPLACE: &str = "Replace";

    /// §3.5: every hit, in one undo step.
    pub const REPLACE_ALL: &str = "Replace All";

    /// The two boxes are the same shape, so the placeholders are what tells them apart.
    pub const QUERY_PLACEHOLDER: &str = "Find";

    pub const REPLACEMENT_PLACEHOLDER: &str = "Replace with";

    /// 2026-09-27, the find panel: the chevron that shows and hides the replace row.
    pub const TOGGLE_REPLACE: &str = "Toggle Replace";

    /// The count when the query is not in the document.
    pub const NO_RESULTS: &str = "No results";

    /// The count beside the query: "3 of 12" on a hit, "12 results" off one, [`NO_RESULTS`] for
    /// none, nothing for no query. (md.win formats with the invariant culture; integers have no
    /// separators either way.)
    pub fn tally(current: usize, total: usize, has_query: bool) -> String {
        if !has_query {
            String::new()
        } else if total == 0 {
            NO_RESULTS.to_string()
        } else if current > 0 {
            format!("{current} of {total}")
        } else if total == 1 {
            "1 result".to_string()
        } else {
            format!("{total} results")
        }
    }
}

/// §2.8: Help ▸ md Help / Privacy Policy / About md.
pub mod help {
    /// LINUX DECISION: md.win's is `https://nettrash.me/msstore/md/support.html`; the Linux
    /// edition's pages live beside the other channels' (see the module comment).
    pub const SUPPORT_URL: &str = "https://nettrash.me/linux/md/support.html";
    /// LINUX DECISION: md.win's is `https://nettrash.me/msstore/md/privacy.html`.
    pub const PRIVACY_URL: &str = "https://nettrash.me/linux/md/privacy.html";
    pub const COPYRIGHT: &str = "\u{00A9} 2026 nettrash. MIT licensed.";

    /// §2.8: the line above [`ENGINES`] in the About dialog. "Bundled" and "open source" are the
    /// family's wording; the app never claims it has no third-party dependencies, and never that
    /// the licence texts are published.
    pub const ENGINES_HEADING: &str = "Bundled open source engines:";

    /// The bundled engines (`rich/`, byte-identical to md.win's), as About lists them. Never the
    /// phrase "no third-party dependencies".
    pub const ENGINES: [&str; 5] = [
        "KaTeX 0.17.0 + mhchem",
        "Mermaid 11.16.0",
        "Graphviz 14.1.1 via Viz.js 3.24.0",
        "PlantUML 1.2026.4beta4",
        "highlight.js 11.11.1",
    ];
}
