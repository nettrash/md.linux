//! The book folder's file operations (md.win `Book/BookFolder.cs`) — port of the `BookLibrary`
//! half of md.macOS BookNavigator.swift that touches disk (create, rename, delete) plus the pure
//! selection helpers the navigator computes around them. Listing is `book_model::load`,
//! reordering is `renumber_plan`, the in-place editor is `book_article_session`; this is
//! everything else.
//!
//! Failures are returned, never presented: md-core holds no alert text (md-logic does), so each
//! operation reports what happened and the window decides what to say. Swift's security scope has
//! no Linux analogue — the app hands over a folder path it already has rights to.

use std::fs;
use std::io;

use super::article_file_system::{ArticleFileSystem, FileStamp, LocalArticleFileSystem, local_stamp, with_path};
use super::book_model::{Book, BookArticle, reading_order};
use super::book_naming;
use super::book_paths;
use super::renumber_plan::{self, BookFileMover};
use crate::document::plain_text_codec;
use crate::text::{unicode, whitespace};

/// The folder operations [`BookFolder`](self) performs, behind a seam so the rules are testable
/// against a fake and the app can route through whatever storage API granted the book folder.
/// Extends the two seams that already exist: reads and writes come from [`ArticleFileSystem`],
/// moves from [`BookFileMover`] (which is also what the reorder executor uses). Every method fails
/// with an `io::Error`; the caller turns its text into the alert.
pub trait BookFolderFileSystem: ArticleFileSystem + BookFileMover {
    /// Whether `path` is an existing directory — [`ArticleFileSystem::file_exists`] answers true
    /// for both kinds.
    fn directory_exists(&self, path: &str) -> bool;

    /// Create one directory. The parent must exist: Swift passes
    /// `withIntermediateDirectories: false`.
    fn create_directory(&self, path: &str) -> io::Result<()>;

    /// Delete a file, or a folder with everything in it. Permanent — there is no Trash here, as
    /// there is no Trash in Swift's `removeItem`. A path that is not there fails, as Foundation's
    /// does, rather than reporting a delete that never happened.
    fn delete(&self, path: &str) -> io::Result<()>;
}

/// [`BookFolderFileSystem`] over `std::fs`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalBookFolderFileSystem;

impl ArticleFileSystem for LocalBookFolderFileSystem {
    /// Swift's `fileExists(atPath:)`: true for a folder too, so nothing is ever written over one.
    fn file_exists(&self, path: &str) -> bool {
        LocalArticleFileSystem.file_exists(path)
    }

    fn read_all_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        LocalArticleFileSystem.read_all_bytes(path)
    }

    fn write_all_bytes(&self, path: &str, data: &[u8]) -> io::Result<()> {
        LocalArticleFileSystem.write_all_bytes(path, data)
    }

    fn stamp(&self, path: &str) -> Option<FileStamp> {
        local_stamp(path)
    }
}

impl BookFileMover for LocalBookFolderFileSystem {
    /// Refuses an existing destination, as Foundation's `moveItem` does (see
    /// `renumber_plan::LocalBookFileMover`).
    fn move_item(&self, from: &str, to: &str) -> io::Result<()> {
        renumber_plan::move_no_replace(from, to)
    }
}

impl BookFolderFileSystem for LocalBookFolderFileSystem {
    fn directory_exists(&self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }

    /// `mkdir`, one level: exactly Foundation's `withIntermediateDirectories: false` contract, and
    /// an existing folder fails too.
    fn create_directory(&self, path: &str) -> io::Result<()> {
        fs::create_dir(path).map_err(|e| with_path(e, path))
    }

    /// The item itself, never what a link points at: a real folder goes with everything in it, a
    /// file or a link (dangling or not) is unlinked — `removeItem(at:)` on a link removes the link.
    /// A missing path is `ENOENT` from `lstat`, the OS's own answer.
    fn delete(&self, path: &str) -> io::Result<()> {
        let metadata = fs::symlink_metadata(path).map_err(|e| with_path(e, path))?;
        let result = if metadata.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
        result.map_err(|e| with_path(e, path))
    }
}

/// How a rename ended — the four answers Swift's `Bool` flattens into two.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RenameOutcome {
    /// The item was renamed.
    Renamed,
    /// The name on disk was already the requested one; nothing was touched (Swift returns true
    /// here).
    Unchanged,
    /// The name is empty or unusable here — show the "A name cannot contain …" message.
    InvalidName,
    /// The move failed, with the error text for the alert.
    Failed(String),
}

/// One sibling group as the management menu needs it (Swift's `articleContext`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookSiblings {
    /// The row's name — `BookArticle::name` or `BookChapter::name`, ordering prefix kept.
    pub name: String,
    /// The folder the renames happen in: the book root for a root article or a chapter, the
    /// chapter folder for its articles.
    pub folder: String,
    /// Every sibling's on-disk name in displayed order — the input `renumber_plan::plan` takes.
    pub names: Vec<String>,
    /// This item's position in `names`.
    pub index: usize,
}

// MARK: Names

/// Whether `name` can be a file or folder name here.
///
/// LINUX DECISION (md.win vets the nine Win32 characters, control characters, a trailing dot or
/// space and the device names): a Linux file system refuses exactly two characters in a name, `/`
/// (the separator) and NUL, and two whole names, `.` and `..` (they already exist in every
/// folder). The Mac's own rule is kept on top of those: Swift's rename refuses `:` as well,
/// because Finder shows a `:` in a name as `/`, and a book moved between the two machines should
/// not grow names one of them displays differently. Everything else is legal on Linux — a trailing
/// dot or space, a tab, `CON`, `a?b` — and is accepted, as the Mac creates them too. A leading dot
/// is *not* rejected: it is legal everywhere, and the item then disappears from the listing on
/// every port, which is the Mac's behaviour to the byte. The three prompts (new chapter, new
/// article, rename) all vet with this, as md.win's do.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\0', ':']) && name != "." && name != ".."
}

/// What the three prompts do with what was typed: Swift's `trimmingCharacters(in: .whitespaces)` —
/// Foundation's WS set ([`whitespace::trim_ws`]), so a no-break space goes too, and a newline stays.
pub fn trimmed_prompt_name(typed: &str) -> &str {
    whitespace::trim_ws(typed)
}

// MARK: Creation

/// Create a chapter folder in the book root through `std::fs` (see [`create_chapter_with`]).
pub fn create_chapter(root: &str, name: &str) -> bool {
    create_chapter_with(root, name, &LocalBookFolderFileSystem)
}

/// Create a chapter folder in the book root. Quiet on failure, as Swift is — the folder may
/// already exist or the name may be unusable, and the file manager is the recovery tool. One level
/// only: an intermediate folder is never created, so a name carrying a separator fails instead of
/// quietly nesting.
pub fn create_chapter_with(root: &str, name: &str, files: &dyn BookFolderFileSystem) -> bool {
    // The folder the OS will hold is spelled exactly `name`, so that is what is vetted.
    if !is_valid_name(name) {
        return false;
    }
    let path = book_paths::combine(root, name);
    let standardized = book_paths::standardize(&path);
    // Swift appends a path component, which cannot escape; a joined name that is rooted or walks
    // up would land outside the folder we hold rights to. Refuse anything not inside (the name
    // rule already refuses both on Linux; this is the second wall md.win keeps).
    if !book_paths::is_inside(&standardized, &book_paths::standardize(root)) {
        return false;
    }
    let Some(parent) = book_paths::directory_name(&standardized) else {
        return false;
    };
    // withIntermediateDirectories: false, and Foundation fails on an existing folder too.
    if !files.directory_exists(&parent) || files.file_exists(&path) {
        return false;
    }
    files.create_directory(&path).is_ok()
}

/// Create an article through `std::fs` (see [`create_article_with`]).
pub fn create_article(folder: &str, name: &str) -> Option<String> {
    create_article_with(folder, name, &LocalBookFolderFileSystem)
}

/// Create "`<name>.md`" in `folder` (the root or a chapter), seeded with the matching level-1
/// heading so it renders sensibly the moment it opens. Never overwrites: an existing name — a
/// folder's included — returns `None`. Quiet on failure, as Swift is. The returned path is the one
/// that was written, not a standardized spelling, so the caller can select it as it selects
/// listing paths. Every new article starts as UTF-8 without a BOM, like Swift's
/// `Data("# \(name)\n".utf8)`.
pub fn create_article_with(folder: &str, name: &str, files: &dyn BookFolderFileSystem) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    // appendingPathExtension("md") is unconditional in Swift: "Scene.md" typed into the prompt
    // becomes "Scene.md.md", which is what the Mac creates. The FILE NAME is what is vetted.
    let file_name = format!("{name}.md");
    if !is_valid_name(&file_name) {
        return None;
    }
    let path = book_paths::combine(folder, &file_name);
    // As in create_chapter: a name must not walk out of the book.
    if !book_paths::is_inside(&book_paths::standardize(&path), &book_paths::standardize(folder)) {
        return None;
    }
    if files.file_exists(&path) {
        return None;
    }
    files.write_all_bytes(&path, format!("# {name}\n").as_bytes()).ok()?;
    Some(path)
}

// MARK: Rename and delete

/// Rename through `std::fs` (see [`rename_item_with`]).
pub fn rename_item(path: &str, stem: &str) -> RenameOutcome {
    rename_item_with(path, stem, &LocalBookFolderFileSystem)
}

/// Rename the article or chapter at `path` to the display name `stem`: the ordering prefix and
/// the extension survive (`book_naming::renamed_name`), so only the readable half changes. A name
/// that is already what was asked for is [`RenameOutcome::Unchanged`] and touches nothing; a
/// collision comes back as [`RenameOutcome::Failed`] with the OS message.
pub fn rename_item_with(path: &str, stem: &str, files: &dyn BookFolderFileSystem) -> RenameOutcome {
    if !is_valid_name(stem) {
        return RenameOutcome::InvalidName;
    }
    let name = book_paths::name(path);
    let renamed = book_naming::renamed_name(name, stem);
    if renamed == name {
        return RenameOutcome::Unchanged;
    }
    // Only the root has no parent, and the root is not an item in a book.
    let Some(folder) = book_paths::directory_name(book_paths::trim_ending_separator(path)) else {
        return RenameOutcome::Failed(format!("{path}: not an item in a book"));
    };
    let target = book_paths::combine(&folder, &renamed);
    let result = if unicode::upper_invariant(&renamed) == unicode::upper_invariant(name) {
        // A case-only rename. md.win stages it because NTFS resolves the destination to the
        // source; on Linux the same happens on a case-insensitive volume (vfat, exFAT, NTFS via
        // ntfs3, ext4 with casefold) — and the local mover refuses an existing destination, which
        // there is the source itself. Going through the reorder executor's hidden staging name
        // lands the rename the same way on every volume; on a case-sensitive one it is merely two
        // moves instead of one.
        let temp = book_paths::combine(&folder, &renumber_plan::temp_name());
        files.move_item(path, &temp).and_then(|()| {
            files.move_item(&temp, &target).inspect_err(|_| {
                // Nothing may be left under a temporary name; the alert names the first failure.
                let _ = files.move_item(&temp, path);
            })
        })
    } else {
        files.move_item(path, &target)
    };
    match result {
        Ok(()) => RenameOutcome::Renamed,
        Err(error) => RenameOutcome::Failed(error.to_string()),
    }
}

/// Delete through `std::fs` (see [`delete_item_with`]).
pub fn delete_item(path: &str) -> Result<(), String> {
    delete_item_with(path, &LocalBookFolderFileSystem)
}

/// Delete the article (a file) or chapter (a folder, with everything in it) at `path`. Permanent —
/// Swift's `removeItem` does not go through the Trash and neither does this. Asking first is the
/// caller's job. A path that is already gone fails, as Foundation's does: md.win checks first
/// because .NET's `File.Delete` is a no-op for a missing file; `unlink` and `lstat` already refuse
/// one with the OS's own wording, so here the file system answers (the seam's contract says so).
pub fn delete_item_with(path: &str, files: &dyn BookFolderFileSystem) -> Result<(), String> {
    files.delete(path).map_err(|error| error.to_string())
}

// MARK: Reading

/// Read an article through `std::fs` (see [`read_article_with`]).
pub fn read_article(path: &str) -> Option<String> {
    read_article_with(path, &LocalBookFolderFileSystem)
}

/// An article's text for a compile or a structured export, or `None` when it cannot be read.
/// DIVERGENCE, deliberate (md.win's): Swift decodes UTF-8 then Latin-1 here — a looser, separate
/// decoder from the editor's `plain_text_codec` — so a Windows-1251 article the book editor opens
/// as Cyrillic compiles into the PDF/EPUB/LaTeX as Latin-1 mojibake on the Mac. This port reads
/// both sides through the codec, so a legacy article compiles as what it says. Latin-1 is still the
/// last resort inside the codec, so nothing that compiled before fails now.
pub fn read_article_with(path: &str, files: &dyn BookFolderFileSystem) -> Option<String> {
    let data = files.read_all_bytes(path).ok()?;
    plain_text_codec::decode(&data).map(|decoded| decoded.text)
}

// MARK: Selection (pure)

/// Where `path` sits in a reading order, compared on standardized paths: Previous / Next step
/// through this, and the reload's selection fallback re-points the selection with it. Swift
/// compares `standardizedFileURL.path` for the same reason a raw string compare will not do — the
/// selection and the listing can spell one article two ways. `None` when it is not in the order,
/// or nothing is selected.
pub fn order_index(order: &[BookArticle], path: Option<&str>) -> Option<usize> {
    let target = book_paths::standardize(path?);
    order.iter().position(|article| book_paths::standardize(&article.path) == target)
}

/// The article the selection should fall to once everything at or under `path` is deleted: the
/// first survivor after the deleted block in reading order, else the last one before it. Computed
/// *before* the delete, while the book snapshot still has the row. `None` when nothing in the book
/// dies with it (an empty chapter, or a path from another book).
pub fn deletion_neighbor(book: &Book, path: &str) -> Option<String> {
    let deleted = book_paths::standardize(path);
    let order: Vec<String> = reading_order(book).iter().map(|article| book_paths::standardize(&article.path)).collect();
    let dies = |candidate: &str| candidate == deleted || book_paths::is_inside(candidate, &deleted);
    let first = order.iter().position(|candidate| dies(candidate))?;
    if let Some(survivor) = order[first..].iter().find(|candidate| !dies(candidate)) {
        return Some(survivor.clone());
    }
    if first > 0 { Some(order[first - 1].clone()) } else { None }
}

/// `path` relative to the book root — the durable form of "where I was writing", remembered across
/// launches. Empty when the path is not inside the book (Swift returns "" for that, and for having
/// no book open). [`path_in`] reads back what this wrote.
pub fn relative_path(root: &str, path: &str) -> String {
    let root_path = book_paths::standardize(root);
    let full = book_paths::standardize(path);
    if !book_paths::is_inside(&full, &root_path) {
        return String::new();
    }
    let start = if root_path.ends_with(book_paths::SEPARATOR) { root_path.len() } else { root_path.len() + 1 };
    full[start..].to_string()
}

/// The inverse: the absolute path of a remembered relative path, or `None` when it is empty or
/// does not land inside the book. The guard matters — a stored value from another book, or an
/// absolute path, would otherwise walk straight out of the folder the app has rights to (a joined
/// rooted path is returned whole). A NUL names nothing on Linux (and .NET's `GetFullPath` refuses
/// it, which md.win maps to `None`).
pub fn path_in(root: &str, relative_path: &str) -> Option<String> {
    if relative_path.is_empty() || relative_path.contains('\0') {
        return None;
    }
    let root_path = book_paths::standardize(root);
    let full = book_paths::standardize(&book_paths::combine(&root_path, relative_path));
    book_paths::is_inside(&full, &root_path).then_some(full)
}

/// The sibling group of the article or chapter at `path` — its row name, the folder its renames
/// happen in, every sibling's on-disk name in displayed order and its own index: everything
/// Rename…, Move Up and Move Down need, recovered from the book snapshot (Swift's
/// `articleContext`, plus the chapter group the sidebar header's menu passes by hand). `None` when
/// the path is not in the book.
pub fn siblings_of(book: &Book, path: &str) -> Option<BookSiblings> {
    let target = book_paths::standardize(path);

    let among = |articles: &[BookArticle], folder: &str| -> Option<BookSiblings> {
        let index = articles.iter().position(|article| book_paths::standardize(&article.path) == target)?;
        Some(BookSiblings {
            name: articles[index].name().to_string(),
            folder: folder.to_string(),
            names: articles.iter().map(|article| book_paths::name(&article.path).to_string()).collect(),
            index,
        })
    };

    if let Some(root) = among(&book.articles, &book.root) {
        return Some(root);
    }
    for chapter in &book.chapters {
        if let Some(hit) = among(&chapter.articles, &chapter.path) {
            return Some(hit);
        }
    }
    // A chapter's siblings are the book's chapter list, renumbered inside the root.
    let index = book.chapters.iter().position(|chapter| book_paths::standardize(&chapter.path) == target)?;
    Some(BookSiblings {
        name: book.chapters[index].name().to_string(),
        folder: book.root.clone(),
        names: book.chapters.iter().map(|chapter| chapter.name().to_string()).collect(),
        index,
    })
}
