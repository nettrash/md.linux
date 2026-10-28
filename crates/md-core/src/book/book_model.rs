//! The book tree from a listing (md.win `Book/BookModel.cs`): articles, chapters, the book
//! snapshot, the listing seam and the reading order.

use std::fs;

use super::{book_naming, book_order, book_paths};

/// One article — a Markdown file in the book. Identity is the file path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookArticle {
    pub path: String,
}

impl BookArticle {
    pub fn new(path: impl Into<String>) -> Self {
        BookArticle { path: path.into() }
    }

    /// The sidebar name: file name without its last extension, ordering prefix kept
    /// ("01-Preface.md" → "01-Preface", "chapter.one.md" → "chapter.one"). Swift's
    /// `deletingPathExtension`; a leading or trailing dot is not an extension.
    pub fn name(&self) -> &str {
        book_paths::deleting_path_extension(book_paths::name(&self.path))
    }
}

/// One chapter — a direct subfolder of the book — with its articles already in reading order.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookChapter {
    pub path: String,
    pub articles: Vec<BookArticle>,
}

impl BookChapter {
    pub fn new(path: impl Into<String>, articles: Vec<BookArticle>) -> Self {
        BookChapter { path: path.into(), articles }
    }

    /// Folders show their full name: a dot in a folder name is part of the name, not a format to
    /// hide.
    pub fn name(&self) -> &str {
        book_paths::name(&self.path)
    }
}

/// A snapshot of the book as listed from disk, rebuilt wholesale after every change. Root articles
/// are the front matter and come before every chapter.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Book {
    pub root: String,
    pub articles: Vec<BookArticle>,
    pub chapters: Vec<BookChapter>,
}

impl Book {
    pub fn new(root: impl Into<String>, articles: Vec<BookArticle>, chapters: Vec<BookChapter>) -> Self {
        Book { root: root.into(), articles, chapters }
    }

    /// The window title: the raw root folder name. The compile title is
    /// `book_naming::display_name` of it instead.
    pub fn name(&self) -> &str {
        book_paths::name(&self.root)
    }

    /// A book with nothing in it — what an empty folder lists as.
    pub fn empty(root: &str) -> Book {
        Book::new(book_paths::standardize(root), Vec::new(), Vec::new())
    }
}

/// One directory entry as the listing saw it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookEntry {
    pub name: String,
    pub is_directory: bool,
    /// A hidden flag the listing reports beside the name. Dot-names are hidden regardless.
    /// LINUX DECISION: Linux has no hidden attribute (md.win reads the Windows Hidden and System
    /// attributes; the Mac also honours the BSD `UF_HIDDEN` flag), so [`DirectoryBookListing`]
    /// never sets it — a dot-name is the whole convention, which is exactly what the reorder
    /// executor's `.md-reorder-…` staging names rely on. Kept for listings that have such a flag.
    pub is_hidden: bool,
}

impl BookEntry {
    pub fn new(name: impl Into<String>, is_directory: bool) -> Self {
        BookEntry { name: name.into(), is_directory, is_hidden: false }
    }

    pub fn hidden(name: impl Into<String>, is_directory: bool) -> Self {
        BookEntry { name: name.into(), is_directory, is_hidden: true }
    }
}

/// The directory-listing seam [`load_with`] reads through, so the tree logic is testable against
/// an in-memory folder and the app can route through whatever storage API granted the folder.
pub trait BookListing {
    /// The entries of `folder`, or `None` when it cannot be listed.
    fn list(&self, folder: &str) -> Option<Vec<BookEntry>>;
}

/// [`BookListing`] over `std::fs`.
#[derive(Clone, Copy, Debug, Default)]
pub struct DirectoryBookListing;

impl BookListing for DirectoryBookListing {
    fn list(&self, folder: &str) -> Option<Vec<BookEntry>> {
        let reader = fs::read_dir(folder).ok()?;
        let mut entries = Vec::new();
        for entry in reader {
            let Ok(entry) = entry else {
                // An entry that vanished mid-listing is simply not there.
                continue;
            };
            // LINUX DECISION: a name that is not UTF-8 is skipped. APFS cannot hold one, so the
            // Mac never lists one; here it could not be displayed, renamed or written back
            // faithfully, and the book model's paths are strings.
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            // LINUX DECISION, after the Mac: the entry's own type, never its target's — Foundation's
            // `.isDirectoryKey` is false for a symbolic link to a folder, so a linked folder is not
            // a chapter there either (md.win follows Windows junctions). A link to a file with an
            // article extension is an article, read through the link.
            let is_directory = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            entries.push(BookEntry::new(name, is_directory));
        }
        Some(entries)
    }
}

/// List the folder at `root` through `std::fs`. `None` when the root cannot be listed.
pub fn load(root: &str) -> Option<Book> {
    load_with(root, &DirectoryBookListing)
}

/// The book tree from a listing. Port of `BookLibrary.loadBook / readingOrder`: every direct
/// subfolder is a chapter (empty or not — it still gets a section and a "New Article…" row), every
/// article-extension file is an article, one level deep, hidden items skipped. Swift lists a
/// chapter's entries without checking that they are files, so a sub-subfolder named "Foo.md" shows
/// up as an article there; Kotlin filters on `isFile`. This port filters, as md.win does — a folder
/// is not an article. `None` when the root cannot be listed.
pub fn load_with(root: &str, listing: &dyn BookListing) -> Option<Book> {
    let root_path = book_paths::standardize(root);
    let top = listing.list(&root_path)?;
    let mut articles = Vec::new();
    let mut chapters = Vec::new();
    for entry in top {
        if is_skipped(&entry) {
            continue;
        }
        let path = book_paths::combine(&root_path, &entry.name);
        if entry.is_directory {
            // An unlistable chapter is still a chapter, just an empty one — the section stays
            // visible instead of vanishing from the sidebar.
            let inner = listing.list(&path).unwrap_or_default();
            let mut chapter_articles: Vec<BookArticle> = inner
                .iter()
                .filter(|e| !is_skipped(e) && !e.is_directory && book_naming::is_article_name(&e.name))
                .map(|e| BookArticle::new(book_paths::combine(&path, &e.name)))
                .collect();
            chapter_articles.sort_by(|a, b| book_order::compare(a.name(), b.name()));
            chapters.push(BookChapter::new(path, chapter_articles));
        } else if book_naming::is_article_name(&entry.name) {
            articles.push(BookArticle::new(path));
        }
    }
    // `sort_by` is stable, like Swift's `sorted` and LINQ's `OrderBy`.
    articles.sort_by(|a, b| book_order::compare(a.name(), b.name()));
    chapters.sort_by(|a, b| book_order::compare(a.name(), b.name()));
    Some(Book::new(root_path, articles, chapters))
}

/// The whole book flattened: root articles (front matter), then each chapter's articles, chapters
/// already in order. What Previous / Next step through and the order the compile, EPUB and LaTeX
/// walk.
pub fn reading_order(book: &Book) -> Vec<BookArticle> {
    book.articles.iter().chain(book.chapters.iter().flat_map(|chapter| chapter.articles.iter())).cloned().collect()
}

/// Swift's `.skipsHiddenFiles`: dot-names and flagged-hidden items. The reorder executor's staging
/// names (".md-reorder-…") are dot-names on purpose.
fn is_skipped(entry: &BookEntry) -> bool {
    entry.is_hidden || entry.name.starts_with('.')
}
