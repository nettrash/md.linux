//! Port of md.win `Commands/CommandEnablement.cs`: the *Enabled* column of shell-design §2 as
//! code, plus the tick marks. Pure functions of one [`ShellSnapshot`]: the menu builder calls them
//! when the snapshot changes (GTK: `SimpleAction::set_enabled` / `set_state`), and
//! [`CommandDispatcher`](super::CommandDispatcher) calls [`is_enabled`] before every invocation —
//! so a disabled chord falls through to the focused widget instead of doing nothing loudly (§2.9;
//! GTK skips an accelerator whose action is disabled and lets the key propagate).
//!
//! The legend, from §2.1: `doc` = the window publishes an active document (document windows
//! always; the Book window only while its session is Editing); `viewMode` = the same set; `zen` =
//! document windows only; `saved` = the document has a path; `dirty` = differs from the last
//! explicit save; `book` = `md.bookBookmark` is non-empty; `stepper` = Book window only.

use md_core::document::view_mode::ViewMode;

use super::command_dispatcher::CommandArgument;
use super::command_id::CommandId;
use super::command_table::{self, MenuPath};
use super::shell_snapshot::ShellSnapshot;

/// §2.1 `doc`. A document window always publishes one; the Book window only while the detail
/// session is editing an article.
pub fn has_active_document(s: &ShellSnapshot) -> bool {
    if s.is_book_window { s.is_editing_article } else { s.has_document }
}

/// §2.1 `saved` — an active document that has a path.
fn saved(s: &ShellSnapshot) -> bool {
    has_active_document(s) && s.is_saved
}

/// Whether this window has a find bar at all. §8.1 lists the Book window's rows — the menu bar,
/// the split view, the footer and the info bar — and there is no find bar among them: Find is a
/// port's addition to the document window (§3.5), because the Mac gets Find from `NSTextView` and
/// its book pane publishes no find of its own.
///
/// So the whole find family is dead in the Book window, not merely unhandled there. Reading it off
/// `has_find_query` / `find_bar_open` alone would have made Find Next and Esc right by accident — a
/// Book window can never set either — while leaving Find…, Replace… and Use Selection for Find
/// enabled on `editor_visible` and `has_selection`, which an article does set.
fn has_find_bar(s: &ShellSnapshot) -> bool {
    !s.is_book_window
}

/// Whether the row for `id` is live in this window right now. Exhaustive: a new [`CommandId`]
/// without a rule does not compile.
pub fn is_enabled(id: CommandId, s: &ShellSnapshot) -> bool {
    use CommandId as C;
    match id {
        // File (§2.2)
        C::New | C::Open => true,
        C::OpenRecentEntry | C::ClearRecent => !s.recent_entries.is_empty(),
        C::OpenTextBundleFolder | C::Example | C::ExampleBook | C::Close => true,
        C::Save => has_active_document(s),
        // Never in the Book window (§8.5): an article IS its file inside the book folder, and the
        // folder is the book — the reading order, the chapter it belongs to and the name the
        // sidebar shows are all read back out of that path (§8.2, §8.3). "Save this article
        // somewhere else" either silently drops it out of the book or writes a second copy the book
        // never lists, and the Mac's book pane offers no Save As for the same reason: its
        // activeDocument publishes a nil fileURL. Rename… and Move To… (inside the book) are the
        // Book window's answer, and the sidebar's own menu is where a writer reaches them.
        C::SaveAs => has_active_document(s) && !s.is_book_window,
        C::Duplicate => has_active_document(s),
        C::Rename | C::MoveTo => saved(s),
        C::RevertToSaved => saved(s) && s.is_dirty,
        C::Print => has_active_document(s),
        C::ExportPdf | C::ExportHtml | C::ExportEpub | C::ExportLaTeX | C::ExportTextBundle => has_active_document(s),
        // The Export submenu itself is never disabled (the page-size picker inside it is a
        // setting), but its diagram rows exist only while the document has diagrams.
        C::ExportDiagramSvg => !s.diagrams.is_empty(),
        C::PdfPageSize | C::Quit => true,

        // Edit (§2.4) — the text view implements these; they live while an editor pane is on
        // screen.
        C::Undo => s.editor_visible && s.can_undo,
        C::Redo => s.editor_visible && s.can_redo,
        // Cut, Copy and Delete act on the selection and on nothing else: over an empty one each is
        // a row that lights up and does nothing (the Mac greys all three the same way).
        C::Cut | C::Copy | C::Delete => s.editor_visible && s.has_selection,
        C::Paste | C::SelectAll => s.editor_visible,
        // The find family, all five gated on the window HAVING a find bar (§8.1) as well as on
        // §2.4's own condition — and every one of them on an editor ON SCREEN, because each either
        // selects its hit in the editor or opens the bar the layout rule hides with it: lit with
        // the editor collapsed (Preview, Zen's reading half) they did nothing visible, which is the
        // macOS Preview-only Find bug. The three that open the bar are dead in Zen too, where the
        // window refuses to show any chrome (§5.4); Ctrl+G still walks the text there.
        C::Find => has_find_bar(s) && s.editor_visible && !s.zen_active,
        C::FindNext | C::FindPrevious => has_find_bar(s) && s.editor_visible && s.has_find_query,
        // Replace opens the same bar as Find… and edits through the same editor pane, so it asks
        // for exactly what Find… asks for.
        C::Replace => has_find_bar(s) && s.editor_visible && !s.zen_active,
        C::UseSelectionForFind => has_find_bar(s) && s.editor_visible && !s.zen_active && s.has_selection,
        // The typing toggles are settings, live in every window like the PDF page size: a writer
        // may turn them off from the Book window or with no editor on screen.
        C::ContinueLists | C::CapitalizeSentences | C::WrapSelection => true,

        // Format (1.6) — an edit of the editor's text, so an editor on screen with a document in
        // it.
        C::Bold
        | C::Italic
        | C::InlineCode
        | C::Strikethrough
        | C::Link
        | C::Heading1
        | C::Heading2
        | C::Heading3
        | C::Heading4
        | C::Heading5
        | C::Heading6
        | C::BulletList
        | C::TaskList
        | C::Quote
        | C::Indent
        | C::Outdent => s.editor_visible && has_active_document(s),

        // View (§2.5)
        C::ViewEdit | C::ViewSplit | C::ViewPreview => has_active_document(s),
        // Present but disabled in the Book window, which publishes no Zen command.
        C::ZenMode => !s.is_book_window && s.has_document,
        C::ShowSidebar => s.is_book_window,
        C::FullScreen => true,

        // Book (§2.6)
        C::NewBook | C::OpenBook | C::ShowBook => true,
        C::CloseBook | C::PrintBook | C::ExportBookPdf | C::ExportBookEpub | C::ExportBookLaTeX => s.has_book,

        // Go (§2.7) — the stepper is the Book window's.
        C::PreviousArticle => s.is_book_window && s.can_previous,
        C::NextArticle => s.is_book_window && s.can_next,
        C::Contents => !s.outline.is_empty(),
        C::Notes => !s.notes.is_empty(),

        // Window and Help (§2.8)
        C::Minimize | C::Zoom => true,
        C::ActivateWindow => !s.window_titles.is_empty(),
        C::Help | C::PrivacyPolicy | C::About => true,

        // §2.9: Esc belongs to the editor unless Zen or the find bar has a use for it.
        C::Escape => s.zen_active || s.find_bar_open,
    }
}

/// A submenu is live while anything inside it is (§2.2, §2.6): Export Book ▸ goes with the book,
/// Diagram as SVG ▸ with the diagrams, and Export ▸ is never disabled because PDF Page Size ▸
/// inside it never is.
pub fn is_submenu_enabled(path: &MenuPath, s: &ShellSnapshot) -> bool {
    command_table::all().iter().any(|spec| match spec.path {
        Some(p) => (p.is_under(path) || p == *path) && is_enabled(spec.id, s),
        None => false,
    })
}

/// The tick on a toggle row. False for every command that is not one.
pub fn is_checked(id: CommandId, s: &ShellSnapshot) -> bool {
    match id {
        // In Zen, Ctrl+1 and Ctrl+2 both mean "write" and Split is never ticked (§2.5).
        CommandId::ViewEdit => s.published_mode() == ViewMode::Edit,
        CommandId::ViewSplit => s.published_mode() == ViewMode::Split,
        CommandId::ViewPreview => s.published_mode() == ViewMode::Preview,
        CommandId::ZenMode => s.zen_active,
        CommandId::ShowSidebar => s.sidebar_open,
        // Ticked from the settings snapshot, never from the click (smart-typing.md §3.1).
        CommandId::ContinueLists => s.continue_lists,
        CommandId::CapitalizeSentences => s.capitalize_sentences,
        CommandId::WrapSelection => s.wrap_selection,
        _ => false,
    }
}

/// The tick on one row of a dynamic command: a PDF Page Size radio whose `PageSize::id` is the
/// stored one (compared exactly, like `PageSize::named`), or the Window row for this window.
/// `argument` is what the row would dispatch with.
pub fn is_row_checked(id: CommandId, argument: &CommandArgument, s: &ShellSnapshot) -> bool {
    match (id, argument) {
        (CommandId::PdfPageSize, CommandArgument::Text(page_size_id)) => *page_size_id == s.pdf_page_size_id,
        (CommandId::ActivateWindow, CommandArgument::Window(window)) => {
            s.window_titles.iter().any(|w| w.id == *window && w.is_this)
        }
        _ => false,
    }
}
