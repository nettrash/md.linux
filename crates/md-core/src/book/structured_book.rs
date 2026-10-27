//! A book read with its structure kept (md.win `Book/StructuredBook.cs`, Swift `EPUBBook`) — the
//! input the EPUB and LaTeX exports share, as opposed to the one flat Markdown source the PDF
//! compile stitches.

/// One article read for a whole-book export: its display name and its Markdown. Swift's
/// `EPUBBook.Article { name, markdown }`, Kotlin's `ArticleContent(title, source)`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookUnit {
    pub title: String,
    pub source: String,
}

/// One chapter with its articles in reading order. Swift's `EPUBBook.Chapter { name, articles }`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookSection {
    pub title: String,
    pub units: Vec<BookUnit>,
}

/// Reading order is [`StructuredBook::front_units`] (the root articles, the front matter before any
/// chapter) first, then each [`BookSection`] in order with its units in order — the same order the
/// PDF compile and the EPUB spine use. Titles are display names: the book library has already
/// stripped the `01-` ordering prefixes and the `.md` extension.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StructuredBook {
    pub title: String,
    pub front_units: Vec<BookUnit>,
    pub sections: Vec<BookSection>,
}
