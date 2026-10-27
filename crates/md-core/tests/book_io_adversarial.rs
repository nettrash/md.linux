//! Port of md.win `BookioAdversarialTests`: adversarial cover for the book-I/O module, written
//! against oracles rather than against the code under test: every expectation was produced by
//! compiling the real md.macOS functions with `swiftc` (BookNavigator.swift's `splitExtension` /
//! `splitPrefix` / `displayName` / `compile` and the compile reader, plus Foundation's
//! `trimmingCharacters(in: .whitespaces)` and `FileManager`) and reading the answer off the probe —
//! or, for the naming rule, from the Linux decision this port declares. Each test closes a hole a
//! mutant walked through: the case-only rename staging, the "which article could not be read" name
//! in three of its four branches, the order side of the selection compare, and the create prompts'
//! name rule.

mod book_support;

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use book_support::{TempDir, combine, latin1, names_in, temp_path, utf16le, write};
use md_core::book::article_file_system::{ArticleFileSystem, FileStamp};
use md_core::book::book_folder::{self, BookFolderFileSystem, RenameOutcome, read_article};
use md_core::book::book_model::{self, Book, BookArticle, BookChapter};
use md_core::book::renumber_plan::BookFileMover;
use md_core::book::{book_compiler, book_naming, book_paths};
use md_core::text::unicode;

fn fake_root() -> String {
    book_paths::standardize("/tmp/03-My Book")
}

type Articles<'a> = &'a [(&'a str, &'a str)];

fn make_book(articles: Articles<'_>, chapters: &[(&str, Articles<'_>)]) -> (Book, HashMap<String, String>) {
    let root = fake_root();
    let mut texts = HashMap::new();
    let roots = articles
        .iter()
        .map(|(file, text)| {
            let path = combine(&[&root, file]);
            texts.insert(path.clone(), text.to_string());
            BookArticle::new(path)
        })
        .collect();
    let sections = chapters
        .iter()
        .map(|(name, files)| {
            let folder = combine(&[&root, name]);
            let articles = files
                .iter()
                .map(|(file, text)| {
                    let path = combine(&[&folder, file]);
                    texts.insert(path.clone(), text.to_string());
                    BookArticle::new(path)
                })
                .collect();
            BookChapter::new(folder, articles)
        })
        .collect();
    (Book::new(root, roots, sections), texts)
}

/// A case-insensitive volume, the shape a vfat / exFAT stick or a casefolded ext4 folder has on
/// Linux (md.win: "Windows-shaped"): names match case-insensitively, and a move whose destination
/// differs from its source only by case is refused — what the local mover's no-replace check does
/// there, because the destination "exists" as the source itself.
#[derive(Default)]
struct CaseInsensitiveFolder {
    files: RefCell<HashMap<String, (String, Vec<u8>)>>,
    directories: RefCell<HashMap<String, String>>,
}

fn key(path: &str) -> String {
    unicode::upper_invariant(path)
}

impl CaseInsensitiveFolder {
    fn add_file(&self, path: &str, data: &[u8]) {
        self.files.borrow_mut().insert(key(path), (path.to_string(), data.to_vec()));
    }

    fn add_directory(&self, path: &str) {
        self.directories.borrow_mut().insert(key(path), path.to_string());
    }

    /// Everything the folder holds, the folder itself excluded, by stored spelling.
    fn names(&self, root: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .files
            .borrow()
            .values()
            .map(|(path, _)| path.clone())
            .chain(self.directories.borrow().values().cloned())
            .filter(|path| key(path) != key(root))
            .map(|path| book_paths::file_name(&path).to_string())
            .collect();
        names.sort();
        names
    }
}

impl ArticleFileSystem for CaseInsensitiveFolder {
    fn file_exists(&self, path: &str) -> bool {
        self.files.borrow().contains_key(&key(path)) || self.directories.borrow().contains_key(&key(path))
    }

    fn read_all_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        self.files
            .borrow()
            .get(&key(path))
            .map(|(_, data)| data.clone())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn write_all_bytes(&self, path: &str, data: &[u8]) -> io::Result<()> {
        self.add_file(path, data);
        Ok(())
    }

    fn stamp(&self, path: &str) -> Option<FileStamp> {
        self.files.borrow().get(&key(path)).map(|(_, data)| FileStamp { modified: UNIX_EPOCH, size: data.len() as u64 })
    }
}

impl BookFileMover for CaseInsensitiveFolder {
    fn move_item(&self, from: &str, to: &str) -> io::Result<()> {
        if from != to && key(from) == key(to) {
            return Err(io::Error::other("Source and destination path must be different."));
        }
        if self.file_exists(to) {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "File exists"));
        }
        let file = self.files.borrow_mut().remove(&key(from));
        if let Some((_, data)) = file {
            self.add_file(to, &data);
        } else if self.directories.borrow_mut().remove(&key(from)).is_some() {
            self.add_directory(to);
        } else {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        Ok(())
    }
}

impl BookFolderFileSystem for CaseInsensitiveFolder {
    fn directory_exists(&self, path: &str) -> bool {
        self.directories.borrow().contains_key(&key(path))
    }

    fn create_directory(&self, path: &str) -> io::Result<()> {
        self.add_directory(path);
        Ok(())
    }

    fn delete(&self, path: &str) -> io::Result<()> {
        self.files.borrow_mut().remove(&key(path));
        self.directories.borrow_mut().remove(&key(path));
        Ok(())
    }
}

/// The staging route is the whole point of the case-only branch, and neither a Mac nor an ext4
/// disk can show it: APFS takes "01-draft.md" to "01-Draft.md" as a direct move, ext4 holds two
/// different names. Against a volume that refuses the direct move — NTFS for md.win, a vfat stick
/// or a casefolded folder here — the rename must still land, in the exact case asked for, with no
/// hidden staging name left behind.
#[test]
fn case_only_rename_survives_a_volume_that_refuses_the_direct_move() {
    let root = book_paths::standardize(&combine(&[&temp_path(), "md-caseless"]));
    let files = CaseInsensitiveFolder::default();
    files.add_directory(&root);
    let article = combine(&[&root, "01-draft.md"]);
    files.add_file(&article, b"# Draft\n");
    files.add_directory(&combine(&[&root, "02-one"]));

    assert_eq!(RenameOutcome::Renamed, book_folder::rename_item_with(&article, "Draft", &files));
    assert_eq!(RenameOutcome::Renamed, book_folder::rename_item_with(&combine(&[&root, "02-one"]), "One", &files));

    assert_eq!(vec!["01-Draft.md", "02-One"], files.names(&root));
    // The case actually stored, not just a case-insensitive hit.
    assert!(files.files.borrow().values().any(|(path, _)| *path == combine(&[&root, "01-Draft.md"])));
    assert!(files.directories.borrow().values().any(|path| *path == combine(&[&root, "02-One"])));
    assert!(!files.names(&root).iter().any(|n| n.starts_with(".md-reorder-")));
    assert_eq!(Some(b"# Draft\n".to_vec()), files.read_all_bytes(&combine(&[&root, "01-Draft.md"])).ok());
}

/// Swift alerts with `article.name` — the sidebar name, ordering prefix kept, extension gone — from
/// the one `append` / `read` closure both loops call, so a root article and a chapter article are
/// named the same way by both readers. Four branches; the shipped tests exercised one each.
#[test]
fn every_unreadable_article_is_named_by_its_sidebar_name_in_both_loops_of_both_readers() {
    let (book, sources) =
        make_book(&[("01-Preface.md", "Front matter.")], &[("02-One", &[("01-a.markdown", "First.")])]);
    let sources = &sources;
    let refusing = |name: &'static str| {
        move |path: &str| if book_paths::file_name(path) == name { None } else { sources.get(path).cloned() }
    };

    assert_eq!(Err("01-Preface".to_string()), book_compiler::compile_book_source(&book, refusing("01-Preface.md")));
    assert_eq!(Err("01-a".to_string()), book_compiler::compile_book_source(&book, refusing("01-a.markdown")));

    assert_eq!(
        Some("01-Preface".to_string()),
        book_compiler::read_structured_book(&book, refusing("01-Preface.md")).err()
    );
    assert_eq!(Some("01-a".to_string()), book_compiler::read_structured_book(&book, refusing("01-a.markdown")).err());

    // ...and a readable run names nothing.
    assert!(book_compiler::compile_book_source(&book, |path| sources.get(path).cloned()).is_ok());
}

/// Swift's `orderIndex` puts *both* sides through `standardizedFileURL.path`; a compare that only
/// normalises the needle finds nothing when the haystack carries the odd spelling.
#[test]
fn order_index_standardizes_the_order_side_as_well_as_the_target() {
    let root = fake_root();
    let order = [
        BookArticle::new(combine(&[&root, "01-Preface.md"])),
        BookArticle::new(combine(&[&root, "02-One", ".", "01-a.md"])),
        BookArticle::new(combine(&[&root, "02-One", "sub", "..", "02-b.md"])),
    ];
    assert_eq!(Some(1), book_folder::order_index(&order, Some(&combine(&[&root, "02-One", "01-a.md"]))));
    assert_eq!(Some(2), book_folder::order_index(&order, Some(&combine(&[&root, "02-One", "02-b.md"]))));
    assert_eq!(Some(0), book_folder::order_index(&order, Some(&combine(&[&root, "01-Preface.md"]))));
}

/// The three prompts trim with Foundation's `.whitespaces`, whose frozen tables still hold U+200B
/// and have never held U+180E. Probed with swiftc: U+200B goes at both ends; U+180E, U+0085, U+000B
/// and U+FEFF all survive, and so does a newline. A Unicode `White_Space` trim gets four of these
/// the other way round.
#[test]
fn the_prompt_trim_is_foundations_whitespace_set_not_the_bcls() {
    // U+200B is in Foundation's frozen .whitespaces table (Unicode has moved it to Cf).
    assert_eq!("Draft", book_folder::trimmed_prompt_name("\u{200B}Draft\u{200B}"));
    // U+0020, U+3000 IDEOGRAPHIC SPACE, U+00A0, U+2003, U+1680, U+202F, U+205F, TAB.
    assert_eq!("Draft", book_folder::trimmed_prompt_name("\u{0020}Draft\u{3000}"));
    assert_eq!("D", book_folder::trimmed_prompt_name("\u{00A0}\u{2003}\u{1680}D\u{202F}\u{205F}\u{0009}"));
    // U+180E left the set in Unicode 6.3 and Foundation agrees: it stays.
    assert_eq!("\u{180E}Draft\u{180E}", book_folder::trimmed_prompt_name("\u{180E}Draft\u{180E}"));
    // Line terminators are .newlines, never .whitespaces: NEL, VT and LF all stay.
    assert_eq!("\u{0085}Draft\u{0085}", book_folder::trimmed_prompt_name("\u{0085}Draft\u{0085}"));
    assert_eq!("\u{000B}Draft\n", book_folder::trimmed_prompt_name("\u{000B}Draft\n"));
    // A BOM is content, not space.
    assert_eq!("\u{FEFF}Draft", book_folder::trimmed_prompt_name("\u{FEFF}Draft"));
    assert_eq!("", book_folder::trimmed_prompt_name("   "));
}

/// `compileBookSource` appends the chapter part before it walks that chapter's articles,
/// unconditionally — so a chapter with nothing in it still spends a page on its heading, and the
/// structured read still emits its section. The expected string came out of the real `compile` run
/// under swiftc with that part list.
#[test]
fn an_empty_chapter_still_spends_a_page_on_its_heading_and_still_gets_a_section() {
    let (book, sources) =
        make_book(&[("01-Preface.md", "Front matter.")], &[("02-Empty", &[]), ("03-Two", &[("01-c.md", "Third.")])]);

    assert_eq!(
        Ok("# My Book\n\n\\newpage\n\nFront matter.\n\n\\newpage\n\n# Empty\n\n\\newpage\n\n".to_string()
            + "# Two\n\n\\newpage\n\nThird."),
        book_compiler::compile_book_source(&book, |path| sources.get(path).cloned())
    );

    let structured = book_compiler::read_structured_book(&book, |path| sources.get(path).cloned()).expect("read");
    assert_eq!(vec!["Empty", "Two"], structured.sections.iter().map(|s| s.title.as_str()).collect::<Vec<_>>());
    assert!(structured.sections[0].units.is_empty());
}

/// `displayName` runs `splitExtension` before `splitPrefix` and does not care that the thing it
/// names is a folder, so a chapter folder called "02-Notes.md" is the chapter "Notes" in the
/// compile, the EPUB and the .tex — while the sidebar (`BookChapter.name`) still shows the folder's
/// whole name. Probed with swiftc: 02-Notes.md gives Notes, x.TXT gives x, "..md" gives one dot.
#[test]
fn a_chapter_folder_carrying_an_article_extension_loses_it_in_every_title() {
    let (book, sources) = make_book(&[], &[("02-Notes.md", &[("01-a.md", "A.")]), ("x.TXT", &[("01-b.md", "B.")])]);

    assert_eq!(
        Ok("# My Book\n\n\\newpage\n\n# Notes\n\n\\newpage\n\nA.\n\n\\newpage\n\n# x\n\n\\newpage\n\nB.".to_string()),
        book_compiler::compile_book_source(&book, |path| sources.get(path).cloned())
    );
    let structured = book_compiler::read_structured_book(&book, |path| sources.get(path).cloned()).expect("read");
    assert_eq!(vec!["Notes", "x"], structured.sections.iter().map(|s| s.title.as_str()).collect::<Vec<_>>());
    // The sidebar keeps the folder name whole; only the export titles are display names.
    assert_eq!(vec!["02-Notes.md", "x.TXT"], book.chapters.iter().map(BookChapter::name).collect::<Vec<_>>());
    assert_eq!(".", book_naming::display_name("..md"));
}

/// The compile reader, on the two BOMs. A UTF-8 BOM is dropped by both sides — Foundation's
/// `String(data:encoding:.utf8)` swallows EF BB BF (probed) and so does the plain-text codec. A
/// UTF-16 file is where the declared §7.2 divergence reaches past CP1251: the Mac's compile reader
/// has no UTF-16 step, so it falls to Latin-1 and the probe prints U+00FF U+00FE '#' U+0000 and so
/// on. This port reads it as the text it is.
#[test]
fn the_compile_reader_drops_a_utf8_bom_and_reads_the_utf16_file_the_mac_turns_into_mojibake() {
    let container = TempDir::new("md-bookio-");
    let root = container.join(&["03-My Book"]);
    let mut bom = vec![0xEF, 0xBB, 0xBF];
    bom.extend_from_slice(b"# Scene\n");
    write(&combine(&[&root, "01-Bom.md"]), &bom);
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend(utf16le("# S"));
    write(&combine(&[&root, "02-Wide.md"]), &utf16);

    assert_eq!(Some("# Scene\n".to_string()), read_article(&combine(&[&root, "01-Bom.md"])));
    assert_eq!(Some("# S".to_string()), read_article(&combine(&[&root, "02-Wide.md"])));
    // What the Mac's compile would have put in the PDF instead.
    assert_ne!(Some(latin1(&utf16)), read_article(&combine(&[&root, "02-Wide.md"])));

    let tree = book_model::load(&root).expect("the book lists");
    assert_eq!(
        Ok("# My Book\n\n\\newpage\n\n# Scene\n\n\n\\newpage\n\n# S".to_string()),
        book_compiler::compile_book_source(&tree, read_article)
    );
}

/// md.win: `TheCreatePromptsRefuseAWindowsDeviceNameAndOnlyThat` — "CON.md" is Win32's console
/// device in every folder, so md.win refuses it. LINUX DECISION: a device name is an ordinary name
/// on Linux (and on the Mac), so the prompts create it; what they refuse is what a Linux folder
/// cannot hold — a NUL, a separator, "." and "..". A trailing space is accepted for an article
/// exactly as md.win accepts it: ".md" follows it, and Foundation creates "Draft .md" (probed).
#[test]
fn the_create_prompts_accept_a_windows_device_name_and_refuse_only_what_linux_cannot_hold() {
    let container = TempDir::new("md-bookio-");
    let root = container.join(&["03-My Book"]);
    std::fs::create_dir(&root).unwrap();

    assert_eq!(Some(combine(&[&root, "CON.md"])), book_folder::create_article(&root, "CON"));
    assert_eq!(Some(combine(&[&root, "prn.md"])), book_folder::create_article(&root, "prn"));
    assert_eq!(Some(combine(&[&root, "LPT1.md"])), book_folder::create_article(&root, "LPT1"));
    assert!(book_folder::create_chapter(&root, "NUL"));
    assert!(Path::new(&combine(&[&root, "CON.md"])).is_file());

    // What Linux itself refuses never reaches the disk.
    assert_eq!(None, book_folder::create_article(&root, "a\0b"));
    assert_eq!(None, book_folder::create_article(&root, "a/b"));
    assert!(!book_folder::create_chapter(&root, "a\0b"));
    assert!(!book_folder::create_chapter(&root, "."));
    assert!(!book_folder::create_chapter(&root, ".."));
    assert!(!book_folder::create_chapter(&root, "a:b"));

    // Everything a book actually holds still goes through, trailing space included.
    assert!(book_folder::create_article(&root, "Draft ").is_some());
    assert!(book_folder::create_article(&root, "CONS").is_some());
    assert!(book_folder::create_article(&root, "Scene.md").is_some());
    assert!(book_folder::create_chapter(&root, "v1.2"));
    assert_eq!(
        vec!["CON.md", "CONS.md", "Draft .md", "LPT1.md", "NUL", "Scene.md.md", "prn.md", "v1.2"],
        names_in(&root)
    );
}
