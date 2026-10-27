//! The whole book as one Markdown source, and the structured read the EPUB and LaTeX exports take
//! (md.win `Book/BookCompiler.cs`).
//!
//! Port of `BookLibrary.compile` (md.macOS BookNavigator.swift; Kotlin `compileBook` builds the
//! same units): a title page, then every part on a page of its own — `\newpage` between them, the
//! marker the export pipeline's parser turns into a page break. Chapter headings get their own
//! page before their articles; article text is untouched (a `---` inside stays an ordinary rule,
//! an author's own `\newpage` passes through).

use super::book_model::Book;
use super::book_naming;
use super::book_paths;
use super::structured_book::{BookSection, BookUnit, StructuredBook};

/// One already-read piece of the book, in reading order (Swift `BookLibrary.Part`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BookPart {
    /// A chapter boundary — becomes a heading on its own page.
    Chapter(String),
    /// An article's Markdown source, verbatim.
    Article(String),
}

/// Exactly `"\n\n\\newpage\n\n"` in Swift source: LF LF backslash newpage LF LF.
pub const PAGE_SEPARATOR: &str = "\n\n\\newpage\n\n";

/// Pure — strings in, string out; reading the files is [`compile_book_source`]'s caller's job.
pub fn compile(book_name: &str, parts: &[BookPart]) -> String {
    let mut sections: Vec<String> = Vec::with_capacity(parts.len() + 1);
    sections.push(format!("# {book_name}"));
    for part in parts {
        sections.push(match part {
            BookPart::Chapter(name) => format!("# {name}"),
            BookPart::Article(text) => text.clone(),
        });
    }
    sections.join(PAGE_SEPARATOR)
}

// MARK: Reading the whole book (Swift's compileBookSource / readStructuredBook)

/// The book's compile title: the display name of the root folder, prefix stripped — not
/// [`Book::name`], which is the raw folder name the window shows. It also names the exported PDF
/// and EPUB.
pub fn title(book: &Book) -> String {
    book_naming::display_name(book.name()).to_string()
}

/// Read every article in reading order and compile the book (see [`compile`]). `read_article`
/// takes an article path and returns its Markdown, or `None` when it cannot be read — one
/// unreadable article aborts the whole compile, as on the Mac, and the `Err` then names it
/// ([`super::book_model::BookArticle::name`], ordering prefix kept) for the alert.
/// `book_folder::read_article` is the reader the app passes.
pub fn compile_book_source(
    book: &Book,
    mut read_article: impl FnMut(&str) -> Option<String>,
) -> Result<String, String> {
    let mut parts = Vec::new();
    for article in &book.articles {
        let text = read_article(&article.path).ok_or_else(|| article.name().to_string())?;
        parts.push(BookPart::Article(text));
    }
    for chapter in &book.chapters {
        parts.push(BookPart::Chapter(book_naming::display_name(chapter.name()).to_string()));
        for article in &chapter.articles {
            let text = read_article(&article.path).ok_or_else(|| article.name().to_string())?;
            parts.push(BookPart::Article(text));
        }
    }
    Ok(compile(&title(book), &parts))
}

/// Read the book in the same reading order as the compile, but with the chapter and article
/// boundaries kept rather than flattened: the EPUB gives each one its own file and the LaTeX turns
/// them into `\chapter` and `\section`. Same abort rule as [`compile_book_source`] — one unreadable
/// article and the export stops, named in the `Err`.
pub fn read_structured_book(
    book: &Book,
    mut read_article: impl FnMut(&str) -> Option<String>,
) -> Result<StructuredBook, String> {
    let mut unit = |path: &str, name: &str| -> Result<BookUnit, String> {
        let source = read_article(path).ok_or_else(|| name.to_string())?;
        Ok(BookUnit { title: book_naming::display_name(book_paths::name(path)).to_string(), source })
    };
    let mut front_units = Vec::new();
    for article in &book.articles {
        front_units.push(unit(&article.path, article.name())?);
    }
    let mut sections = Vec::new();
    for chapter in &book.chapters {
        let mut units = Vec::new();
        for article in &chapter.articles {
            units.push(unit(&article.path, article.name())?);
        }
        sections.push(BookSection { title: book_naming::display_name(chapter.name()).to_string(), units });
    }
    Ok(StructuredBook { title: title(book), front_units, sections })
}
