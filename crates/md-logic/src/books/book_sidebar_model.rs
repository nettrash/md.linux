//! Port of md.win `Books/BookSidebarModel.cs`: the sidebar as a list of rows (§8.3, book.md §13.2).
//! Which rows exist, in which order, what each is called and which of Move Up / Move Down is live —
//! all decided here, so the GTK list builder has nothing left to decide.

use md_core::book::book_folder;
use md_core::book::book_model::Book;

use crate::strings::books;

/// What one sidebar row is. Only the two article kinds are selectable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BookRowKind {
    /// An article in the book root — the front matter, before every chapter.
    RootArticle,
    /// A chapter's header row: the full folder name, not selectable, carrying the chapter's own
    /// management menu.
    ChapterHeader,
    /// An article inside a chapter.
    ChapterArticle,
    /// The "New Article…" row that closes the root section and every chapter section.
    NewArticle,
}

/// One row of the sidebar.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookRow {
    pub kind: BookRowKind,
    /// The text shown. Articles use `BookArticle::name` and chapters `BookChapter::name` — the
    /// ordering prefix stays visible, the Mac's choice (the reading order *is* the name, so hiding
    /// it would hide the thing being reordered).
    pub title: String,
    /// The article file or the chapter folder; `None` on a "New Article…" row.
    pub path: Option<String>,
    /// The folder this row belongs to: the book root for a root article, a chapter header and the
    /// root's own "New Article…" row; the chapter folder for a chapter's articles and its "New
    /// Article…" row.
    pub folder: String,
}

impl BookRow {
    pub fn is_article(&self) -> bool {
        matches!(self.kind, BookRowKind::RootArticle | BookRowKind::ChapterArticle)
    }

    /// Only articles carry the list's selection; a header or an action row that could be selected
    /// would fight it.
    pub fn is_selectable(&self) -> bool {
        self.is_article()
    }
}

/// What the management menu on one row needs: the sibling group it renumbers inside and where this
/// item sits in it (Swift's `articleContext`, plus the chapter group the section header's menu
/// passes by hand).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookRowCommands {
    pub name: String,
    pub folder: String,
    pub siblings: Vec<String>,
    pub index: usize,
    pub is_chapter: bool,
}

impl BookRowCommands {
    /// Move Up is dead at the top of the group (Swift `.disabled(index == 0)`).
    pub fn can_move_up(&self) -> bool {
        self.index > 0
    }

    /// Move Down is dead at the bottom (`.disabled(index == siblings.count - 1)`).
    pub fn can_move_down(&self) -> bool {
        self.index + 1 < self.siblings.len()
    }

    /// The Delete… alert's message — a chapter takes its articles with it, and says so.
    pub fn delete_message(&self) -> &'static str {
        if self.is_chapter { books::DELETE_CHAPTER_MESSAGE } else { books::DELETE_ARTICLE_MESSAGE }
    }
}

/// Every row in display order; empty when there is no book. Section 1 is the root articles and its
/// "New Article…" row (no header — the front matter has no title on the Mac either); then one
/// section per chapter: its header, its articles, its "New Article…" row. A chapter with no
/// articles still gets its section, so there is somewhere to put the first one.
pub fn rows(book: Option<&Book>) -> Vec<BookRow> {
    let Some(book) = book else { return Vec::new() };
    let new_article = |folder: &str| BookRow {
        kind: BookRowKind::NewArticle,
        title: books::NEW_ARTICLE_ROW.to_string(),
        path: None,
        folder: folder.to_string(),
    };
    let mut rows = Vec::new();
    for article in &book.articles {
        rows.push(BookRow {
            kind: BookRowKind::RootArticle,
            title: article.name().to_string(),
            path: Some(article.path.clone()),
            folder: book.root.clone(),
        });
    }
    rows.push(new_article(&book.root));
    for chapter in &book.chapters {
        rows.push(BookRow {
            kind: BookRowKind::ChapterHeader,
            title: chapter.name().to_string(),
            path: Some(chapter.path.clone()),
            folder: book.root.clone(),
        });
        for article in &chapter.articles {
            rows.push(BookRow {
                kind: BookRowKind::ChapterArticle,
                title: article.name().to_string(),
                path: Some(article.path.clone()),
                folder: chapter.path.clone(),
            });
        }
        rows.push(new_article(&chapter.path));
    }
    rows
}

/// The management menu's state for the article or chapter at `path`, or `None` when the path is
/// not in this book. A chapter's siblings are the chapter list (renumbered inside the root); an
/// article's are its own folder's articles.
pub fn commands_for(book: Option<&Book>, path: Option<&str>) -> Option<BookRowCommands> {
    let (book, path) = (book?, path?);
    let siblings = book_folder::siblings_of(book, path)?;
    // Byte for byte, as md.win compares with BookPaths.Comparison (ordinal on Linux).
    let is_chapter = book.chapters.iter().any(|chapter| chapter.path == path);
    Some(BookRowCommands {
        name: siblings.name,
        folder: siblings.folder,
        siblings: siblings.names,
        index: siblings.index,
        is_chapter,
    })
}

/// The row index of an article path, or `None` (md.win's -1) — what the list's selection is set
/// from. A chapter folder is a row, but never a selected one.
pub fn index_of_article(rows: &[BookRow], path: Option<&str>) -> Option<usize> {
    let path = path?;
    rows.iter().position(|row| row.is_article() && row.path.as_deref() == Some(path))
}
