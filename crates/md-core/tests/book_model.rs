//! Port of md.win `BookModelTests`: the tree (mdTests: testReadingOrderIsRootArticlesThenChapters,
//! testReadingOrderOfEmptyBookIsEmpty) and the listing that builds it — through an in-memory
//! `BookListing`, and once through `std::fs` — plus the whole-book compile (mdTests:
//! testCompileBeginsWithTitlePage, testCompileOrdersPartsAndPagesEveryOne,
//! testCompileOfEmptyBookIsJustTheTitlePage; Kotlin BookOrderTest's compileBook vectors).

mod book_support;

use std::collections::HashMap;
use std::fs;

use book_support::{TempDir, combine};
use md_core::book::book_compiler::{self, BookPart};
use md_core::book::book_model::{self, Book, BookArticle, BookChapter, BookEntry, BookListing};
use md_core::book::book_paths;

fn root() -> String {
    book_paths::standardize("/tmp/Book")
}

/// A book snapshot straight from paths — the listing is not under test.
fn make_book(root: &str, articles: &[&str], chapters: &[(&str, &[&str])]) -> Book {
    Book::new(
        root,
        articles.iter().map(|a| BookArticle::new(combine(&[root, a]))).collect(),
        chapters
            .iter()
            .map(|(name, files)| {
                let folder = combine(&[root, name]);
                let articles = files.iter().map(|f| BookArticle::new(combine(&[&folder, f]))).collect();
                BookChapter::new(folder, articles)
            })
            .collect(),
    )
}

fn names(articles: &[BookArticle]) -> Vec<&str> {
    articles.iter().map(BookArticle::name).collect()
}

#[test]
fn reading_order_is_root_articles_then_chapters() {
    let book = make_book(&root(), &["01-Preface.md"], &[("02-One", &["01-a.md", "02-b.md"]), ("03-Two", &["01-c.md"])]);
    assert_eq!(vec!["01-Preface", "01-a", "02-b", "01-c"], names(&book_model::reading_order(&book)));
}

#[test]
fn reading_order_of_empty_book_is_empty() {
    let book = make_book(&root(), &[], &[("02-One", &[])]);
    assert!(book_model::reading_order(&book).is_empty());
    assert!(book_model::reading_order(&Book::empty(&root())).is_empty());
}

#[test]
fn names_follow_the_swift_model() {
    let root = root();
    // Article: last extension off, prefix kept. Chapter: full folder name. Book: raw root name.
    assert_eq!("01-Preface", BookArticle::new(combine(&[&root, "01-Preface.md"])).name());
    assert_eq!("chapter.one", BookArticle::new(combine(&[&root, "chapter.one.md"])).name());
    assert_eq!("README", BookArticle::new(combine(&[&root, "README"])).name());
    assert_eq!("v1.2", BookChapter::new(combine(&[&root, "v1.2"]), Vec::new()).name());
    assert_eq!("02-One", BookChapter::new(format!("{}/", combine(&[&root, "02-One"])), Vec::new()).name());
    assert_eq!("Book", Book::empty(&root).name());
    assert_eq!("Book", Book::empty(&format!("{root}/")).name());
}

/// Folders keyed by their standardized path — byte-exact, as Linux compares names.
#[derive(Default)]
struct MemoryListing {
    folders: HashMap<String, Vec<BookEntry>>,
}

impl BookListing for MemoryListing {
    fn list(&self, folder: &str) -> Option<Vec<BookEntry>> {
        self.folders.get(&book_paths::standardize(folder)).cloned()
    }
}

fn sample_listing() -> MemoryListing {
    let root = root();
    let mut listing = MemoryListing::default();
    listing.folders.insert(
        root.clone(),
        vec![
            BookEntry::new("10-deploy.md", false),
            BookEntry::new("01-intro.md", false),
            BookEntry::new("2. setup.markdown", false),
            BookEntry::new("notes.TXT", false),
            BookEntry::new("cover.png", false),
            BookEntry::new(".DS_Store", false),
            BookEntry::hidden("secret.md", false),
            BookEntry::new("03-Two", true),
            BookEntry::new("02-One", true),
            BookEntry::new("Empty", true),
            BookEntry::new(".git", true),
        ],
    );
    listing.folders.insert(
        combine(&[&root, "02-One"]),
        vec![
            BookEntry::new("02-b.md", false),
            BookEntry::new("01-a.md", false),
            BookEntry::new("Foo.md", true),
            BookEntry::new("img.png", false),
        ],
    );
    listing.folders.insert(combine(&[&root, "03-Two"]), vec![BookEntry::new("01-c.md", false)]);
    listing.folders.insert(combine(&[&root, "Empty"]), Vec::new());
    listing
}

#[test]
fn load_separates_articles_from_chapters_and_orders_both() {
    let root = root();
    let book = book_model::load_with(&root, &sample_listing()).expect("the root lists");
    assert_eq!(root, book.root);
    assert_eq!(vec!["01-intro", "2. setup", "10-deploy", "notes"], names(&book.articles));
    assert_eq!(combine(&[&root, "notes.TXT"]), book.articles[3].path);
    // Every subfolder is a chapter, the empty one included; hidden ones are not.
    assert_eq!(vec!["02-One", "03-Two", "Empty"], book.chapters.iter().map(BookChapter::name).collect::<Vec<_>>());
    // Inside a chapter: sorted, files only — a sub-subfolder named "Foo.md" is not an article.
    assert_eq!(vec!["01-a", "02-b"], names(&book.chapters[0].articles));
    assert_eq!(combine(&[&root, "02-One", "01-a.md"]), book.chapters[0].articles[0].path);
    assert!(book.chapters[2].articles.is_empty());
    assert_eq!(
        vec!["01-intro", "2. setup", "10-deploy", "notes", "01-a", "02-b", "01-c"],
        names(&book_model::reading_order(&book))
    );
}

#[test]
fn load_returns_null_for_an_unlistable_root_and_keeps_an_unlistable_chapter() {
    let root = root();
    assert_eq!(None, book_model::load_with(&combine(&[&root, "Nowhere"]), &MemoryListing::default()));
    let mut listing = sample_listing();
    listing.folders.remove(&combine(&[&root, "03-Two"]));
    let book = book_model::load_with(&root, &listing).expect("the root lists");
    assert_eq!(vec!["02-One", "03-Two", "Empty"], book.chapters.iter().map(BookChapter::name).collect::<Vec<_>>());
    assert!(book.chapters[1].articles.is_empty());
}

#[test]
fn load_from_disk_matches_the_listing_contract() {
    let root = TempDir::new("md-book-");
    fs::write(root.join(&["01-Preface.md"]), "# Preface\n").unwrap();
    fs::write(root.join(&["cover.png"]), "").unwrap();
    fs::write(root.join(&[".md-reorder-LEFTOVER"]), "").unwrap();
    fs::create_dir(root.join(&["02-One"])).unwrap();
    fs::write(root.join(&["02-One", "02-b.md"]), "").unwrap();
    fs::write(root.join(&["02-One", "01-a.md"]), "").unwrap();
    fs::create_dir(root.join(&["02-One", "Nested.md"])).unwrap();
    fs::create_dir(root.join(&["Drafts"])).unwrap();

    let book = book_model::load(&root.path).expect("the root lists");
    assert_eq!(vec!["01-Preface"], names(&book.articles));
    assert_eq!(vec!["02-One", "Drafts"], book.chapters.iter().map(BookChapter::name).collect::<Vec<_>>());
    assert_eq!(vec!["01-a", "02-b"], names(&book.chapters[0].articles));
    assert_eq!(None, book_model::load(&root.join(&["missing"])));
}

/// LINUX DECISIONS, pinned on disk: a dot-name is the whole hidden convention (Linux has no hidden
/// attribute — md.win also skips the Windows Hidden/System flags); a symbolic link is classified by
/// itself, as the Mac's `.isDirectoryKey` does, so a linked folder is not a chapter while a link to
/// an article file is an article; and a name that is not UTF-8 is not listed.
#[test]
fn load_from_disk_follows_the_linux_listing_decisions() {
    use std::os::unix::ffi::OsStrExt;

    let outside = TempDir::new("md-book-outside-");
    fs::create_dir(outside.join(&["Linked Chapter"])).unwrap();
    fs::write(outside.join(&["Linked Chapter", "01-x.md"]), "# x\n").unwrap();
    fs::write(outside.join(&["shared.md"]), "# Shared\n").unwrap();

    let root = TempDir::new("md-book-");
    fs::write(root.join(&[".hidden.md"]), "").unwrap();
    fs::create_dir(root.join(&[".Hidden Chapter"])).unwrap();
    std::os::unix::fs::symlink(outside.join(&["Linked Chapter"]), root.join(&["02-Linked"])).unwrap();
    std::os::unix::fs::symlink(outside.join(&["shared.md"]), root.join(&["01-Shared.md"])).unwrap();
    let not_utf8 = std::path::Path::new(&root.path).join(std::ffi::OsStr::from_bytes(b"03-\xFF.md"));
    let wrote_non_utf8 = fs::write(&not_utf8, "").is_ok();

    let book = book_model::load(&root.path).expect("the root lists");
    assert_eq!(vec!["01-Shared"], names(&book.articles));
    assert!(book.chapters.is_empty());
    // The linked article reads through the link.
    assert_eq!(Some("# Shared\n".to_string()), md_core::book::book_folder::read_article(&book.articles[0].path));
    // (APFS refuses a non-UTF-8 name outright, so on a Mac there is nothing to skip.)
    if wrote_non_utf8 {
        assert_eq!(1, book.articles.len());
    }
}

// MARK: Compilation (the whole book as one Markdown source)

#[test]
fn compile_begins_with_title_page() {
    let compiled = book_compiler::compile("My Book", &[BookPart::Article("Hello.".into())]);
    // The title page comes first, on a page of its own.
    assert!(compiled.starts_with("# My Book\n\n\\newpage\n\n"));
    assert!(compiled.ends_with("Hello."));
}

#[test]
fn compile_orders_parts_and_pages_every_one() {
    // Reading order in, one page per part out: chapter headings on their own page, \newpage between
    // all parts, text verbatim.
    let compiled = book_compiler::compile(
        "B",
        &[
            BookPart::Article("Front matter.".into()),
            BookPart::Chapter("One".into()),
            BookPart::Article("First.\n\n---\n\nStill first.".into()),
            BookPart::Article("Second.".into()),
        ],
    );
    assert_eq!(
        "# B\n\n\\newpage\n\nFront matter.\n\n\\newpage\n\n# One\n\n\\newpage\n\nFirst.\n\n---\n\nStill first.\n\n\\newpage\n\nSecond.",
        compiled
    );
    // The article's own `---` stays an ordinary rule — only the compiler's four `\newpage` markers
    // cut pages. (That the parser turns exactly these four into page breaks, and not the rule, is
    // the parser module's test.)
    assert_eq!(4, compiled.matches("\n\n\\newpage\n\n").count());
    assert_eq!(1, compiled.matches("\n\n---\n\n").count());
}

#[test]
fn compile_of_empty_book_is_just_the_title_page() {
    assert_eq!("# Empty", book_compiler::compile("Empty", &[]));
}

#[test]
fn compile_kotlin_vectors_agree() {
    // Kotlin compileBook(bookName, rootArticles, chapters) builds the same units.
    assert_eq!(
        "# Book\n\n\\newpage\n\nroot article\n\n\\newpage\n\n# Chapter One\n\n\\newpage\n\nfirst\n\n\\newpage\n\nsecond",
        book_compiler::compile(
            "Book",
            &[
                BookPart::Article("root article".into()),
                BookPart::Chapter("Chapter One".into()),
                BookPart::Article("first".into()),
                BookPart::Article("second".into()),
            ]
        )
    );
    // A page break on both sides of a chapter heading: nothing shares its page.
    assert!(
        book_compiler::compile("B", &[BookPart::Chapter("Setup".into()), BookPart::Article("body".into())])
            .contains("\\newpage\n\n# Setup\n\n\\newpage")
    );
    // Article content is verbatim: notes and the author's own page breaks pass through.
    let article = "line one\n\n---\n\n<!-- note: private -->\n\n\\newpage\n\nline two";
    assert!(book_compiler::compile("B", &[BookPart::Article(article.into())]).contains(article));
    // The separator is exactly LF LF backslash newpage LF LF — one backslash.
    assert_eq!(
        &[0x0A, 0x0A, 0x5C, 0x6E, 0x65, 0x77, 0x70, 0x61, 0x67, 0x65, 0x0A, 0x0A],
        book_compiler::PAGE_SEPARATOR.as_bytes()
    );
}
