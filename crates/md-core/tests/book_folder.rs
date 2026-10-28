//! Port of md.win `BookFolderTests`: the book folder's file operations against real scratch
//! folders — creation, rename (including the case-only rename a case-insensitive volume makes
//! interesting), deletion and the selection helpers around them — plus the two compile readers and
//! the decoder they use. macOS has no unit tests for this half (BookLibrary's I/O is only exercised
//! through the UI), so these are md.win's own, written off the Swift line by line.
//!
//! Where md.win pins a Windows naming rule, the case pins the LINUX decision instead (see
//! `book_folder::is_valid_name`), and says so.

mod book_support;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use book_support::{TempDir, combine, cp1251, latin1, read_text, temp_path, unique, write};
use md_core::book::article_file_system::{ArticleFileSystem, FileStamp};
use md_core::book::book_folder::{self, BookFolderFileSystem, RenameOutcome};
use md_core::book::book_model::{self, Book, BookArticle, BookChapter};
use md_core::book::book_paths;
use md_core::book::renumber_plan::BookFileMover;
use md_core::book::{book_compiler, book_folder::read_article};

/// A scratch container with the book root inside it, so the compile title is a fixed name and not
/// a unique one.
struct TempBook {
    _container: TempDir,
    root: String,
}

impl TempBook {
    fn new() -> Self {
        let container = TempDir::new("md-folder-");
        let root = container.join(&["03-My Book"]);
        fs::create_dir(&root).unwrap();
        TempBook { _container: container, root }
    }

    fn write(&self, relative: &str, text: &str) -> String {
        self.write_bytes(relative, text.as_bytes())
    }

    fn write_bytes(&self, relative: &str, data: &[u8]) -> String {
        let path = combine(&[&self.root, relative]);
        write(&path, data);
        path
    }

    fn names(&self, relative: &str) -> Vec<String> {
        book_support::names_in(&combine(&[&self.root, relative]))
    }
}

/// A book root that never touches disk — the pure helpers only compare paths.
fn fake_root() -> String {
    book_paths::standardize("/tmp/Book")
}

/// A book snapshot straight from paths, as the model tests build one — the listing is not under
/// test here.
fn make_book(articles: &[&str], chapters: &[(&str, &[&str])]) -> Book {
    let root = fake_root();
    Book::new(
        root.clone(),
        articles.iter().map(|a| BookArticle::new(combine(&[&root, a]))).collect(),
        chapters
            .iter()
            .map(|(name, files)| {
                let folder = combine(&[&root, name]);
                let articles = files.iter().map(|f| BookArticle::new(combine(&[&folder, f]))).collect();
                BookChapter::new(folder, articles)
            })
            .collect(),
    )
}

// MARK: Names

/// md.win: `IsValidNameFollowsTheWindowsRuleNotJustSwiftsTwoCharacters`. Ported to the LINUX rule:
/// the same names, with the answers Linux gives — `/` and NUL are the only characters a Linux file
/// system refuses, `.` and `..` the only names, and the Mac's `:` stays refused. Every Windows-only
/// refusal (`\ * ? " < > |`, a trailing dot or space, a control character, a device name) is a
/// legal Linux name and is accepted, as the Mac accepts it.
#[test]
fn is_valid_name_follows_the_linux_rule_plus_the_macs_colon() {
    let cases: &[(&str, bool)] = &[
        // Ordinary names, the ones the prompts are for.
        ("Preface", true),
        ("01-Preface", true),
        ("v1.2", true),
        ("Ideas (draft)", true),
        (" leading space", true),
        (".hidden", true),
        ("CONS", true),
        ("COM10", true),
        // The separator and NUL; the Mac's own refusal of ":".
        ("", false),
        ("a/b", false),
        ("a\0b", false),
        ("a:b", false),
        // The Win32 characters are ordinary characters on Linux.
        ("a\\b", true),
        ("a*b", true),
        ("a?b", true),
        ("a\"b", true),
        ("a<b", true),
        ("a>b", true),
        ("a|b", true),
        // A trailing dot or space, a control character, a Windows device name: all legal here.
        ("trailing.", true),
        ("trailing ", true),
        ("tab\there", true),
        ("CON", true),
        ("con.md", true),
        ("NUL", true),
        ("lpt1.txt", true),
        // The two names every Linux folder already holds.
        (".", false),
        ("..", false),
        ("...", true),
    ];
    for &(name, valid) in cases {
        assert_eq!(valid, book_folder::is_valid_name(name), "{name:?}");
    }
}

#[test]
fn prompt_names_are_trimmed_with_foundations_whitespace_set() {
    assert_eq!("Draft", book_folder::trimmed_prompt_name("  Draft\t"));
    // .whitespaces is Zs plus tab: a no-break space goes, a newline stays (it is not in the set).
    assert_eq!("Draft", book_folder::trimmed_prompt_name("\u{00A0}Draft\u{2003}"));
    assert_eq!("Draft\n", book_folder::trimmed_prompt_name(" Draft\n"));
    assert_eq!("", book_folder::trimmed_prompt_name("   "));
}

// MARK: Creation

#[test]
fn create_chapter_makes_one_folder_and_refuses_to_replace_anything() {
    let book = TempBook::new();
    assert!(book_folder::create_chapter(&book.root, "02-One"));
    assert!(Path::new(&combine(&[&book.root, "02-One"])).is_dir());

    // Foundation's withIntermediateDirectories: false fails on an existing folder…
    assert!(!book_folder::create_chapter(&book.root, "02-One"));
    // …on a name a file already holds…
    book.write("notes.md", "# Notes\n");
    assert!(!book_folder::create_chapter(&book.root, "notes.md"));
    // …and on a missing intermediate, where a create-all would have built the chain.
    assert!(!book_folder::create_chapter(&book.root, &combine(&["Missing", "Deep"])));
    assert!(!Path::new(&combine(&[&book.root, "Missing"])).exists());
    assert!(!book_folder::create_chapter(&book.root, ""));
    // A rooted name joined on would be returned whole, where appendingPathComponent cannot escape:
    // the book folder is the boundary for both prompts.
    let outside = combine(&[&temp_path(), &unique("md-escape-")]);
    assert!(!book_folder::create_chapter(&book.root, &outside));
    assert!(!Path::new(&outside).exists());
    assert_eq!(None, book_folder::create_article(&book.root, &outside));
    assert!(!Path::new(&format!("{outside}.md")).exists());
    assert_eq!(vec!["02-One", "notes.md"], book.names(""));
}

#[test]
fn create_article_seeds_a_matching_heading_and_never_overwrites() {
    let book = TempBook::new();
    let created = book_folder::create_article(&book.root, "01-Scene").expect("created");
    assert_eq!(combine(&[&book.root, "01-Scene.md"]), created);
    assert_eq!(b"# 01-Scene\n".to_vec(), fs::read(&created).unwrap());

    // Swift appends the extension unconditionally, so a typed ".md" doubles it.
    assert_eq!(Some(combine(&[&book.root, "Scene.md.md"])), book_folder::create_article(&book.root, "Scene.md"));
    // Never twice, and never over a folder squatting on the name.
    assert_eq!(None, book_folder::create_article(&book.root, "01-Scene"));
    assert_eq!(b"# 01-Scene\n".to_vec(), fs::read(&created).unwrap());
    fs::create_dir(combine(&[&book.root, "Folder.md"])).unwrap();
    assert_eq!(None, book_folder::create_article(&book.root, "Folder"));
    assert_eq!(None, book_folder::create_article(&book.root, ""));
    // Into a chapter, which is the other folder the prompt offers.
    assert!(book_folder::create_chapter(&book.root, "02-One"));
    let in_chapter = book_folder::create_article(&combine(&[&book.root, "02-One"]), "01-a");
    assert_eq!(Some(combine(&[&book.root, "02-One", "01-a.md"])), in_chapter);
}

// MARK: Rename

#[test]
fn rename_keeps_the_ordering_prefix_and_the_extension() {
    let book = TempBook::new();
    let article = book.write("01-Draft.md", "# Draft\n");
    assert_eq!(RenameOutcome::Renamed, book_folder::rename_item(&article, "Final"));
    assert_eq!(vec!["01-Final.md"], book.names(""));
    assert_eq!("# Draft\n", read_text(&combine(&[&book.root, "01-Final.md"])));

    // A chapter is a folder: no extension logic, prefix kept.
    fs::create_dir(combine(&[&book.root, "02-Getting Started"])).unwrap();
    assert_eq!(
        RenameOutcome::Renamed,
        book_folder::rename_item(&combine(&[&book.root, "02-Getting Started"]), "Basics")
    );
    assert!(Path::new(&combine(&[&book.root, "02-Basics"])).is_dir());
}

#[test]
fn rename_to_the_name_it_already_has_is_a_no_op() {
    let book = TempBook::new();
    let article = book.write("01-Draft.md", "# Draft\n");
    let stamp = fs::metadata(&article).unwrap().modified().unwrap();
    // Swift returns true here without touching the disk.
    assert_eq!(RenameOutcome::Unchanged, book_folder::rename_item(&article, "Draft"));
    assert_eq!(stamp, fs::metadata(&article).unwrap().modified().unwrap());
    assert_eq!(vec!["01-Draft.md"], book.names(""));
}

/// md.win pins "a?b" as a refused name ("Windows-only members of the set, rejected here so the port
/// behaves the same on both"). LINUX DECISION: "a?b" is a legal name here and on the Mac, so it is
/// accepted; NUL — which no Linux file system can hold — is the refused stand-in.
#[test]
fn rename_refuses_an_unusable_name_and_reports_a_collision() {
    let book = TempBook::new();
    let article = book.write("01-Draft.md", "# Draft\n");
    book.write("01-Final.md", "# Final\n");

    assert_eq!(RenameOutcome::InvalidName, book_folder::rename_item(&article, ""));
    assert_eq!(RenameOutcome::InvalidName, book_folder::rename_item(&article, "a/b"));
    assert_eq!(RenameOutcome::InvalidName, book_folder::rename_item(&article, "a:b"));
    assert_eq!(RenameOutcome::InvalidName, book_folder::rename_item(&article, "a\0b"));
    assert!(book_folder::is_valid_name("a?b"));

    // A collision is the OS's answer, not a validation failure — Swift surfaces it the same way.
    match book_folder::rename_item(&article, "Final") {
        RenameOutcome::Failed(collision) => assert!(!collision.is_empty()),
        other => panic!("expected a failure, got {other:?}"),
    }
    assert_eq!(vec!["01-Draft.md", "01-Final.md"], book.names(""));
    assert_eq!("# Final\n", read_text(&combine(&[&book.root, "01-Final.md"])));
}

/// On a case-insensitive volume (APFS where this was written, vfat / exFAT / casefolded ext4 on
/// Linux) the destination "already exists" because it IS the source, and the local mover refuses
/// it on that check alone. The two-phase route is what makes "01-draft.md" -> "01-Draft.md" work
/// there, and it must leave nothing staged behind. On a case-sensitive volume it is two plain
/// moves, and the outcome is the same.
#[test]
fn case_only_rename_goes_through_a_staging_name_on_a_case_insensitive_volume() {
    let book = TempBook::new();
    let article = book.write("01-draft.md", "# Draft\n");
    assert_eq!(RenameOutcome::Renamed, book_folder::rename_item(&article, "Draft"));
    assert_eq!(vec!["01-Draft.md"], book.names(""));
    assert_eq!("# Draft\n", read_text(&combine(&[&book.root, "01-Draft.md"])));

    // A chapter folder too.
    fs::create_dir(combine(&[&book.root, "02-one"])).unwrap();
    assert_eq!(RenameOutcome::Renamed, book_folder::rename_item(&combine(&[&book.root, "02-one"]), "One"));
    assert_eq!(vec!["01-Draft.md", "02-One"], book.names(""));
    assert!(!book.names("").iter().any(|name| name.starts_with(".md-reorder-")));
}

// MARK: Delete

#[test]
fn delete_takes_an_article_or_a_whole_chapter_and_fails_on_what_is_already_gone() {
    let book = TempBook::new();
    let article = book.write("01-Preface.md", "# Preface\n");
    book.write(&combine(&["02-One", "01-a.md"]), "a");
    book.write(&combine(&["02-One", "02-b.md"]), "b");

    assert_eq!(Ok(()), book_folder::delete_item(&article));
    assert!(!Path::new(&article).exists());

    // A chapter goes with everything in it — permanent, no Trash.
    assert_eq!(Ok(()), book_folder::delete_item(&combine(&[&book.root, "02-One"])));
    assert!(book.names("").is_empty());

    // Foundation throws for a missing path, and so does this.
    let missing = book_folder::delete_item(&article).unwrap_err();
    assert!(!missing.is_empty());
}

// MARK: Selection helpers (pure)

#[test]
fn deletion_neighbor_is_the_first_survivor_after_the_block_else_the_last_before() {
    let book = make_book(&["01-Preface.md"], &[("02-One", &["01-a.md", "02-b.md"]), ("03-Two", &["01-c.md"])]);
    let root = fake_root();
    let within = |parts: &[&str]| {
        let mut all = vec![root.as_str()];
        all.extend_from_slice(parts);
        combine(&all)
    };

    // Reading order is Preface, a, b, c.
    assert_eq!(
        Some(within(&["02-One", "01-a.md"])),
        book_folder::deletion_neighbor(&book, &within(&["01-Preface.md"]))
    );
    assert_eq!(
        Some(within(&["02-One", "02-b.md"])),
        book_folder::deletion_neighbor(&book, &within(&["02-One", "01-a.md"]))
    );
    // A chapter takes its whole block with it; the survivor after it wins.
    assert_eq!(Some(within(&["03-Two", "01-c.md"])), book_folder::deletion_neighbor(&book, &within(&["02-One"])));
    // Nothing after the last block: fall back to the last article before it.
    assert_eq!(Some(within(&["02-One", "02-b.md"])), book_folder::deletion_neighbor(&book, &within(&["03-Two"])));
    assert_eq!(
        Some(within(&["02-One", "02-b.md"])),
        book_folder::deletion_neighbor(&book, &within(&["03-Two", "01-c.md"]))
    );
    // Nothing in the book dies with it.
    assert_eq!(None, book_folder::deletion_neighbor(&book, &within(&["04-Empty"])));
    let parent = book_paths::directory_name(&root).unwrap();
    assert_eq!(None, book_folder::deletion_neighbor(&book, &combine(&[&parent, "Elsewhere", "01-a.md"])));
    // The whole book: every candidate dies, and there is no survivor either side.
    assert_eq!(None, book_folder::deletion_neighbor(&book, &root));
}

#[test]
fn order_index_compares_standardized_paths_not_spellings() {
    let book = make_book(&["01-Preface.md"], &[("02-One", &["01-a.md", "02-b.md"])]);
    let order = book_model::reading_order(&book);
    let root = fake_root();
    assert_eq!(Some(0), book_folder::order_index(&order, Some(&combine(&[&root, "01-Preface.md"]))));
    // The same article spelled with a dot segment is the same article.
    assert_eq!(Some(2), book_folder::order_index(&order, Some(&combine(&[&root, "02-One", ".", "02-b.md"]))));
    assert_eq!(None, book_folder::order_index(&order, Some(&combine(&[&root, "nothing.md"]))));
    assert_eq!(None, book_folder::order_index(&order, None));
}

#[test]
fn relative_path_and_path_in_round_trip_and_refuse_to_leave_the_book() {
    let root = fake_root();
    let parent = book_paths::directory_name(&root).unwrap();
    let article = combine(&[&root, "02-One", "01-a.md"]);
    let relative = book_folder::relative_path(&root, &article);
    assert_eq!(combine(&["02-One", "01-a.md"]), relative);
    assert_eq!(Some(article), book_folder::path_in(&root, &relative));
    assert_eq!("01-Preface.md", book_folder::relative_path(&root, &combine(&[&root, "01-Preface.md"])));
    // Outside, or the root itself, is "" — Swift's "no book / not inside" answer.
    assert_eq!("", book_folder::relative_path(&root, &combine(&[&parent, "Elsewhere", "a.md"])));
    assert_eq!("", book_folder::relative_path(&root, &root));
    // A remembered value from another book must not walk out of the folder we hold rights to.
    assert_eq!(None, book_folder::path_in(&root, ""));
    assert_eq!(None, book_folder::path_in(&root, ".."));
    assert_eq!(None, book_folder::path_in(&root, &combine(&["..", "Elsewhere", "a.md"])));
    assert_eq!(None, book_folder::path_in(&root, &combine(&[&parent, "Elsewhere", "a.md"])));
    // A NUL names nothing (md.win: GetFullPath throws, and the guard answers None).
    assert_eq!(None, book_folder::path_in(&root, "a\0b.md"));
}

#[test]
fn siblings_of_finds_the_group_a_rename_or_a_move_acts_on() {
    let book =
        make_book(&["01-Preface.md", "02-Notes.md"], &[("02-One", &["01-a.md", "02-b.md"]), ("03-Two", &["01-c.md"])]);
    let root = fake_root();

    let in_root = book_folder::siblings_of(&book, &combine(&[&root, "02-Notes.md"])).expect("a root article");
    assert_eq!("02-Notes", in_root.name);
    assert_eq!(root, in_root.folder);
    assert_eq!(vec!["01-Preface.md", "02-Notes.md"], in_root.names);
    assert_eq!(1, in_root.index);

    let in_chapter =
        book_folder::siblings_of(&book, &combine(&[&root, "02-One", "01-a.md"])).expect("a chapter article");
    assert_eq!(combine(&[&root, "02-One"]), in_chapter.folder);
    assert_eq!(vec!["01-a.md", "02-b.md"], in_chapter.names);
    assert_eq!(0, in_chapter.index);

    // A chapter's siblings are the chapter list, renumbered inside the root.
    let chapter = book_folder::siblings_of(&book, &combine(&[&root, "03-Two"])).expect("a chapter");
    assert_eq!("03-Two", chapter.name);
    assert_eq!(root, chapter.folder);
    assert_eq!(vec!["02-One", "03-Two"], chapter.names);
    assert_eq!(1, chapter.index);

    assert_eq!(None, book_folder::siblings_of(&book, &combine(&[&root, "nothing.md"])));
}

// MARK: The seam

/// Everything book_folder asks a file system for, in memory — Swift reached FileManager directly.
#[derive(Default)]
struct RecordingFolder {
    files: RefCell<HashMap<String, Vec<u8>>>,
    directories: RefCell<HashSet<String>>,
    log: RefCell<Vec<String>>,
}

impl ArticleFileSystem for RecordingFolder {
    fn file_exists(&self, path: &str) -> bool {
        self.files.borrow().contains_key(path) || self.directories.borrow().contains(path)
    }

    fn read_all_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        self.files.borrow().get(path).cloned().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.to_string()))
    }

    fn write_all_bytes(&self, path: &str, data: &[u8]) -> io::Result<()> {
        self.log.borrow_mut().push(format!("write {path}"));
        self.files.borrow_mut().insert(path.to_string(), data.to_vec());
        Ok(())
    }

    fn stamp(&self, path: &str) -> Option<FileStamp> {
        self.files.borrow().get(path).map(|data| FileStamp { modified: UNIX_EPOCH, size: data.len() as u64 })
    }
}

impl BookFileMover for RecordingFolder {
    fn move_item(&self, from: &str, to: &str) -> io::Result<()> {
        self.log.borrow_mut().push(format!("move {} -> {}", book_paths::file_name(from), book_paths::file_name(to)));
        let data = self.files.borrow_mut().remove(from);
        if let Some(data) = data {
            self.files.borrow_mut().insert(to.to_string(), data);
        } else if self.directories.borrow_mut().remove(from) {
            self.directories.borrow_mut().insert(to.to_string());
        } else {
            return Err(io::Error::new(io::ErrorKind::NotFound, from.to_string()));
        }
        Ok(())
    }
}

impl BookFolderFileSystem for RecordingFolder {
    fn directory_exists(&self, path: &str) -> bool {
        self.directories.borrow().contains(path)
    }

    fn create_directory(&self, path: &str) -> io::Result<()> {
        self.log.borrow_mut().push(format!("mkdir {path}"));
        self.directories.borrow_mut().insert(path.to_string());
        Ok(())
    }

    fn delete(&self, path: &str) -> io::Result<()> {
        self.log.borrow_mut().push(format!("delete {path}"));
        let removed_file = self.files.borrow_mut().remove(path).is_some();
        let removed_folder = self.directories.borrow_mut().remove(path);
        if removed_file || removed_folder {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, path.to_string()))
        }
    }
}

#[test]
fn every_operation_goes_through_the_injected_file_system() {
    let root = book_paths::standardize(&combine(&[&temp_path(), &unique("md-seam-book-")]));
    let files = RecordingFolder::default();
    files.directories.borrow_mut().insert(root.clone());

    assert!(book_folder::create_chapter_with(&root, "02-One", &files));
    let article = book_folder::create_article_with(&root, "01-Scene", &files).expect("created");
    assert_eq!(combine(&[&root, "01-Scene.md"]), article);
    assert_eq!(b"# 01-Scene\n".to_vec(), files.files.borrow()[&article]);

    assert_eq!(RenameOutcome::Renamed, book_folder::rename_item_with(&article, "Opening", &files));
    assert!(files.files.borrow().contains_key(&combine(&[&root, "01-Opening.md"])));
    assert_eq!(
        Some("# 01-Scene\n".to_string()),
        book_folder::read_article_with(&combine(&[&root, "01-Opening.md"]), &files)
    );
    assert_eq!(Ok(()), book_folder::delete_item_with(&combine(&[&root, "01-Opening.md"]), &files));
    assert_eq!(None, book_folder::read_article_with(&combine(&[&root, "01-Opening.md"]), &files));

    assert_eq!(
        vec![
            format!("mkdir {}", combine(&[&root, "02-One"])),
            format!("write {}", combine(&[&root, "01-Scene.md"])),
            "move 01-Scene.md -> 01-Opening.md".to_string(),
            format!("delete {}", combine(&[&root, "01-Opening.md"])),
        ],
        *files.log.borrow()
    );
    // Nothing reached the disk.
    assert!(!Path::new(&root).exists());
}

// MARK: The compile readers

fn sample_book() -> TempBook {
    let book = TempBook::new();
    book.write("01-Preface.md", "Front matter.");
    book.write(&combine(&["02-One", "01-a.md"]), "First.\n\n---\n\nStill first.");
    book.write(&combine(&["02-One", "02-b.md"]), "Second.");
    book.write(&combine(&["03-Two", "01-c.md"]), "Third.");
    book
}

#[test]
fn compile_book_source_reads_the_whole_book_in_reading_order() {
    let book = sample_book();
    let tree = book_model::load(&book.root).expect("the book lists");
    let compiled = book_compiler::compile_book_source(&tree, read_article);
    // Title page, then every part on a page of its own; the article's "---" is not a page break.
    assert_eq!(
        Ok("# My Book\n\n\\newpage\n\nFront matter.\n\n\\newpage\n\n# One\n\n\\newpage\n\n".to_string()
            + "First.\n\n---\n\nStill first.\n\n\\newpage\n\nSecond.\n\n\\newpage\n\n# Two\n\n\\newpage\n\nThird."),
        compiled
    );
    assert_eq!("My Book", book_compiler::title(&tree));
}

#[test]
fn compile_book_source_aborts_and_names_the_article_it_could_not_read() {
    let book = sample_book();
    let tree = book_model::load(&book.root).expect("the book lists");
    let refused = combine(&[&book.root, "02-One", "02-b.md"]);
    let compiled =
        book_compiler::compile_book_source(&tree, |path| if path == refused { None } else { read_article(path) });
    // The alert names the sidebar name, ordering prefix kept.
    assert_eq!(Err("02-b".to_string()), compiled);
    assert_eq!(None, read_article(&combine(&[&book.root, "missing.md"])));
}

#[test]
fn compile_decodes_a_legacy_article_as_cyrillic_not_mojibake() {
    // DIVERGENCE, deliberate (md.win report §7.2): Swift's compile reader is UTF-8 then Latin-1, so
    // this article compiles into the Mac's PDF as mojibake while the same file edits correctly in
    // the book window. The port reads both through the plain-text codec, so the compile says what
    // the file says.
    let cyrillic = "Привет, мир!";
    let book = TempBook::new();
    let bytes = cp1251(cyrillic);
    book.write_bytes("01-Legacy.md", &bytes);

    let tree = book_model::load(&book.root).expect("the book lists");
    let compiled = book_compiler::compile_book_source(&tree, read_article).expect("compiled");
    assert_eq!(format!("# My Book\n\n\\newpage\n\n{cyrillic}"), compiled);
    assert!(!compiled.contains(&latin1(&bytes)));
    assert_eq!(Some(cyrillic.to_string()), read_article(&combine(&[&book.root, "01-Legacy.md"])));
}

#[test]
fn read_structured_book_keeps_the_chapter_boundaries_the_compile_flattens() {
    let book = sample_book();
    let tree = book_model::load(&book.root).expect("the book lists");
    let structured = book_compiler::read_structured_book(&tree, read_article).expect("read");
    // Every title is a display name; the sources are verbatim.
    assert_eq!("My Book", structured.title);
    assert_eq!(vec!["Preface"], structured.front_units.iter().map(|u| u.title.as_str()).collect::<Vec<_>>());
    assert_eq!("Front matter.", structured.front_units[0].source);
    assert_eq!(vec!["One", "Two"], structured.sections.iter().map(|s| s.title.as_str()).collect::<Vec<_>>());
    assert_eq!(vec!["a", "b"], structured.sections[0].units.iter().map(|u| u.title.as_str()).collect::<Vec<_>>());
    assert_eq!("First.\n\n---\n\nStill first.", structured.sections[0].units[0].source);
    assert_eq!(vec!["c"], structured.sections[1].units.iter().map(|u| u.title.as_str()).collect::<Vec<_>>());
}

#[test]
fn read_structured_book_aborts_and_names_the_article_it_could_not_read() {
    let book = sample_book();
    let tree = book_model::load(&book.root).expect("the book lists");
    let refused = combine(&[&book.root, "01-Preface.md"]);
    let structured =
        book_compiler::read_structured_book(&tree, |path| if path == refused { None } else { read_article(path) });
    assert_eq!(Err("01-Preface".to_string()), structured);
}

#[test]
fn an_empty_book_still_compiles_to_its_title_page() {
    let book = TempBook::new();
    let tree = book_model::load(&book.root).expect("the book lists");
    assert_eq!(Ok("# My Book".to_string()), book_compiler::compile_book_source(&tree, read_article));
    let structured = book_compiler::read_structured_book(&tree, read_article).expect("read");
    assert!(structured.front_units.is_empty());
    assert!(structured.sections.is_empty());
}
