//! Port of md.win `BookAdversarialTests`: adversarial review of the book module against the source
//! of truth. Every expectation here was read off md.macOS — the Swift sources, or a Swift probe run
//! on macOS (2026-09-06: `BookLibrary.ordered` over 36,000 random pairs,
//! `enumerateSubstrings(.byWords)` over 490 lines, `URL.pathExtension` / `deletingPathExtension`
//! over 62 names, `fileExists(atPath:)` on a folder) — never off a port's own output. Invisible
//! code points are spelled as escapes so the vectors stay reviewable. (md.win shares a test
//! collection here because its flush gate is a static event; each session below is its own.)

mod book_support;

use std::cell::{Cell, RefCell};
use std::cmp::Ordering::{self, Greater, Less};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::rc::Rc;
use std::time::{Duration, UNIX_EPOCH};

use book_support::{TempDir, combine, read_text, temp_path};
use md_core::book::article_file_system::{ArticleFileSystem, FileStamp};
use md_core::book::book_article_session::{
    AutosaveScheduler, BookArticleSession, BookStage, BookStageKind, DocumentOwnership,
};
use md_core::book::book_model::BookArticle;
use md_core::book::{book_order, book_paths, writing_stats};

// MARK: Ordering — Foundation's localizedStandardCompare, not Kotlin's stand-in

/// Pairs from the 36,000-pair differential on which the previous comparator (the Kotlin
/// `naturalCompare` port: per-unit lowercase, ordinal punctuation) disagreed with Foundation. The
/// order is `BookLibrary.ordered`'s: Less when the first name lists first. Punctuation collates
/// TAB, space, `_ - , ; : ! ? . ' " ( ) …` — all before digits, digits before letters — so `10_x`
/// lists before `10-x`, `2) x` after `2_x`, and `~0` before `b`.
const FOUNDATION_PAIRS: &[(&str, Ordering, &str)] = &[
    ("2) aa.txt", Greater, "2_Tables.markdown"),
    ("9223372036854775807. notes", Less, "9223372036854775807)setup.MD"),
    (")Tables", Greater, "-ase"),
    ("0) ecole", Greater, "0-01.markdown"),
    ("9223372036854775807) X2.MD", Greater, "9223372036854775807-.ab"),
    ("007)The Storm.md", Greater, "007-.Intro.markdown"),
    ("007-ecole", Less, "007) 01.txt"),
    ("-)part02.md", Greater, "-.rd party"),
    ("2) ase.txt", Greater, "2-)Draft.txt"),
    ("2) aesir.md", Greater, "2-ase"),
    ("2_ase", Less, "2. Tables"),
    ("2-a-b", Greater, "2_Writer-Tools.markdown"),
    ("9223372036854775807) ab.md", Greater, "9223372036854775807. setup"),
    ("0-.Tables.MD", Greater, "0_10.MD"),
    ("007_writer.tools", Less, "007. Writer-Tools.MD"),
    (")aa.txt", Greater, "-Writer-Tools.markdown"),
    ("0) The Storm.txt", Greater, "0_X2.markdown"),
    ("007. Tables.markdown", Greater, "007_aa.MD"),
    ("10_Zebra.txt", Less, "10.appendix.markdown"),
    ("2.v1.2", Less, "2)Draft"),
    ("1_a-b", Less, "1-)ase.MD"),
    ("10) setup.markdown", Greater, "10. Draft (Copy)"),
    ("10_part02", Less, "10-part02"),
    ("0) Tables", Greater, "0-Tables"),
    ("1)", Greater, "0001_C"),
    ("~0", Less, "b"),
    ("_1", Less, "&+"),
    (")A", Greater, "_"),
    (",b0A9", Greater, "_'"),
    (",c1-&Y", Less, "'9#Z 2Y-"),
    ("( _.~", Greater, "-"),
    ("~9a&!#z ", Less, "C#0ca "),
    ("-C.A(.C ", Less, "+0!BC,'"),
    ("_z_(", Less, "&1(B"),
    (", 9", Less, "+Y-xX"),
    ("A..&c+", Greater, "~x9 0Y9"),
    ("&y.", Greater, "-1-1c"),
    ("(' .a", Less, "#.c1zy.-"),
    ("!x9", Greater, "- x0c)01"),
    ("-(_xC", Less, "#0!Y)z"),
    ("Aa", Greater, "~~b'+1A("),
    ("'a)bz1", Greater, "_BC9,"),
    ("x", Less, "X"),
    ("a1", Less, "A1"),
    ("d", Less, "D"),
    ("1-A", Less, "01-a"),
    ("a01", Less, "A1"),
];

#[test]
fn order_agrees_with_foundation_where_the_kotlin_stand_in_did_not() {
    assert_eq!(47, FOUNDATION_PAIRS.len());
    for &(a, sign, b) in FOUNDATION_PAIRS {
        assert_eq!(sign, book_order::compare(a, b), "{a:?} vs {b:?}");
        assert_eq!(sign.reverse(), book_order::compare(b, a), "{b:?} vs {a:?}");
        assert_eq!(sign == Less, book_order::ordered(a, b));
        assert_eq!(sign == Greater, book_order::ordered(b, a));
    }
}

// MARK: Word count — ICU's root rules, which both siblings run, not bare UAX #29

/// `enumerateSubstrings(.byWords)` counts on macOS. The colon family is not MidLetter under ICU's
/// root tailoring (`a:b` is two words; bare UAX #29 says one), wide digits are Numeric (fullwidth
/// 123 is one word), Roman numerals and circled letters are words while circled digits, fractions
/// and superscripts are not, Thai letters never join Latin or digits, and every control and line
/// separator breaks. (Foundation's Japanese dictionary path also reports a bare "_" or space
/// between Katakana runs as a word token — "カ_カ" counts 3 on macOS; that artifact is not
/// reproduced or pinned.)
const FOUNDATION_WORD_COUNTS: &[(&str, usize)] = &[
    ("a:b", 2),
    ("a：b", 2),
    ("a﹕b", 2),
    ("a·b", 1),
    ("a‧b", 1),
    ("a.b", 1),
    ("1:2", 2),
    ("1;2", 1),
    ("1,2", 1),
    ("1.2", 1),
    ("1٫2", 1),
    ("1٬2", 1),
    ("1⁄2", 1),
    ("１２３", 1),
    ("１２3", 1),
    ("1２", 1),
    ("１a", 1),
    ("１.２", 1),
    ("１．２", 1),
    ("١٢٣", 1),
    ("०१", 1),
    ("Ａ１", 1),
    ("Ⅻ", 1),
    ("ⅫⅢ", 1),
    ("ⅻ", 1),
    ("\u{1F130}", 1),
    ("ⓐ", 1),
    ("〇", 1),
    ("ᛮ", 1),
    ("\u{10140}", 1),
    ("①", 0),
    ("①②", 0),
    ("½", 0),
    ("2½", 1),
    ("x²", 1),
    ("₂", 0),
    ("กข", 1),
    ("สว\u{0E31}สด\u{0E35}", 1),
    ("ก1", 2),
    ("1ก", 2),
    ("กa", 2),
    ("ລາວ", 1),
    ("1\u{00A0}000", 2),
    ("1\u{202F}000", 1),
    ("1\u{2009}000", 2),
    ("1\u{2007}000", 2),
    ("a\u{202F}b", 1),
    ("a\u{200B}b", 2),
    ("a\u{200C}b", 1),
    ("a\u{2060}b", 1),
    ("a\u{FEFF}b", 1),
    ("a\u{00AD}b", 1),
    ("a\u{034F}b", 1),
    ("\u{0301}a", 1),
    ("a\u{0301}", 1),
    ("\u{200D}", 0),
    ("\u{FE0F}", 0),
    ("\u{0301}", 0),
    ("'tis", 1),
    ("dogs'", 1),
    ("'a'", 1),
    ("a''b", 2),
    ("rock 'n' roll", 3),
    ("rock ’n’ roll", 3),
    ("can't've", 1),
    ("O'Brien", 1),
    ("l’homme", 1),
    ("ג\"י", 1),
    ("גד\"", 1),
    ("\"גד", 1),
    ("a\"b", 2),
    ("ג׳י", 1),
    ("ג״י", 1),
    ("א_ב", 1),
    ("カー", 1),
    ("カｰ", 1),
    ("ｶﾀ", 1),
    ("カa", 2),
    ("カ1", 2),
    ("カあ", 2),
    ("가각", 1),
    ("가 각", 2),
    ("가", 1),
    ("1\u{FE0F}\u{20E3}", 1),
    ("#\u{FE0F}\u{20E3}", 0),
    ("\u{1F600}", 0),
    ("a\u{1F600}b", 2),
    ("\u{1F1FA}\u{1F1F8}", 0),
    ("©", 0),
    ("☃\u{FE0F}", 0),
    ("http://x.y/z", 3),
    ("x.y/z", 2),
    ("a(b)c", 3),
    ("a<b>c", 3),
    ("a&b", 2),
    ("a+b", 2),
    ("a=b", 2),
    ("a/b", 2),
    ("a\\b", 2),
    ("e-mail", 2),
    ("a‐b", 2),
    ("a\u{2011}b", 2),
    ("a–b", 2),
    ("a—b", 2),
    ("a−b", 2),
    ("a－b", 2),
    ("x-", 1),
    ("-x", 1),
    ("-", 0),
    ("-5", 1),
    ("a - b", 2),
    ("a_b", 1),
    ("a‿b", 1),
    ("a＿b", 1),
    ("1_2", 1),
    ("_a", 1),
    ("a_", 1),
    ("\u{1D400}\u{1D401}", 1),
    ("\u{1D7CE}1", 1),
    ("\u{10400}", 1),
    ("٣a", 1),
    ("a٣", 1),
    ("ß", 1),
    ("İ", 1),
    ("a\u{0085}b", 2),
    ("a\u{2028}b", 2),
    ("a\u{2029}b", 2),
    ("a\u{000B}b", 2),
    ("a\u{000C}b", 2),
    ("a\r\nb", 2),
    ("a\n\rb", 2),
    ("a\u{001C}b", 2),
    ("a\u{001F}b", 2),
    ("a\0b", 2),
    ("a\u{007F}b", 2),
    ("1a1", 1),
    ("a1a", 1),
    (" a ", 1),
    ("  ", 0),
    ("\t", 0),
    ("abc", 1),
    ("a  b", 2),
];

#[test]
fn word_count_follows_icu_root_tailorings_not_bare_uax29() {
    assert_eq!(142, FOUNDATION_WORD_COUNTS.len());
    for &(text, expected) in FOUNDATION_WORD_COUNTS {
        assert_eq!(expected, writing_stats::words(text), "{text:?}");
    }
}

// MARK: Rescue copy — Foundation's two extension splits, and folders occupying a name

/// A file system in memory whose stamp never moves: whatever is written, the file looks unchanged
/// to the flush.
#[derive(Default)]
struct RecordingFileSystem {
    files: RefCell<HashMap<String, Vec<u8>>>,
    writes: RefCell<Vec<String>>,
}

const CONSTANT_STAMP_SECONDS: u64 = 1_788_652_800; // 2026-09-06T00:00:00Z

impl ArticleFileSystem for RecordingFileSystem {
    fn file_exists(&self, path: &str) -> bool {
        self.files.borrow().contains_key(path)
    }

    fn read_all_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        self.files.borrow().get(path).cloned().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.to_string()))
    }

    fn write_all_bytes(&self, path: &str, data: &[u8]) -> io::Result<()> {
        self.writes.borrow_mut().push(path.to_string());
        self.files.borrow_mut().insert(path.to_string(), data.to_vec());
        Ok(())
    }

    fn stamp(&self, path: &str) -> Option<FileStamp> {
        self.files
            .borrow()
            .contains_key(path)
            .then(|| FileStamp { modified: UNIX_EPOCH + Duration::from_secs(CONSTANT_STAMP_SECONDS), size: 0 })
    }
}

fn memory_root() -> String {
    book_paths::standardize(&combine(&[&temp_path(), "md-adversarial-memory-book"]))
}

/// Swift builds the rescue name from `url.deletingPathExtension().lastPathComponent` and
/// `url.pathExtension` (empty → "md"). Probed on macOS: a stem of nothing but dots has no extension
/// ("..md" stays whole), a trailing dot is no extension, CFURL refuses an extension containing a
/// space — U+0020 only, a TAB is accepted — while `deletingPathExtension` still strips it, and the
/// extension keeps its case. `BookArticle::name` is the same `deletingPathExtension`.
#[test]
fn rescue_copy_names_and_article_names_follow_foundations_splits() {
    let cases: &[(&str, &str, &str)] = &[
        ("01-Scene.md", "01-Scene", "01-Scene (rescued).md"),
        ("01-Scene.MD", "01-Scene", "01-Scene (rescued).MD"),
        ("README", "README", "README (rescued).md"),
        ("notes.", "notes.", "notes. (rescued).md"),
        ("01.Scene", "01", "01 (rescued).Scene"),
        (".hidden.md", ".hidden", ".hidden (rescued).md"),
        ("..md", "..md", "..md (rescued).md"),
        ("...md", "...md", "...md (rescued).md"),
        ("a..md", "a.", "a. (rescued).md"),
        ("a.tar.gz", "a.tar", "a.tar (rescued).gz"),
        ("01-Scene.md.bak", "01-Scene.md", "01-Scene.md (rescued).bak"),
        ("a.b c", "a", "a (rescued).md"),
        ("a.md ", "a", "a (rescued).md"),
        ("a.b\tc", "a", "a (rescued).b\tc"),
        ("Draft (copy).txt", "Draft (copy)", "Draft (copy) (rescued).txt"),
    ];
    let root = memory_root();
    for &(file, article_name, rescue_name) in cases {
        let path = combine(&[&root, file]);
        assert_eq!(article_name, BookArticle::new(path.clone()).name(), "{file:?}");

        let fs = Rc::new(RecordingFileSystem::default());
        let mut session = BookArticleSession::new().with_file_system(fs.clone());
        session.open_book(&root);
        let rescued = session.write_rescue_copy(&path).expect("rescued");
        assert_eq!(rescue_name, book_paths::file_name(&rescued), "{file:?}");
        assert_eq!(Some(root.clone()), book_paths::directory_name(&rescued));
        assert_eq!(vec![rescued], *fs.writes.borrow());
    }
}

#[test]
fn rescue_copy_skips_a_folder_squatting_on_its_name() {
    // Swift guards with fileExists(atPath:), which is true for a folder (probed); a files-only
    // check would try to write into the folder's name and fail the rescue.
    let root = TempDir::new("md-adversarial-");
    let article = root.join(&["01-Scene.md"]);
    fs::write(&article, "# Scene\n").unwrap();
    fs::create_dir(root.join(&["01-Scene (rescued).md"])).unwrap();
    let mut session = BookArticleSession::new();
    session.open_book(&root.path);
    assert!(session.select(Some(&article)));
    session.edit("# Mine\n");

    let rescued = session.write_rescue_copy(&article).expect("rescued");
    assert_eq!("01-Scene (rescued 2).md", book_paths::file_name(&rescued));
    assert_eq!("# Mine\n", read_text(&rescued));
    assert!(std::path::Path::new(&root.join(&["01-Scene (rescued).md"])).is_dir());
    assert!(session.dirty());
    session.select(None);
}

// MARK: Session — promises read off the Swift state machine

#[derive(Default)]
struct FakeOwnership {
    owned: RefCell<HashSet<String>>,
}

impl DocumentOwnership for FakeOwnership {
    fn owns(&self, path: &str) -> bool {
        self.owned.borrow().iter().any(|o| book_paths::same(o, path))
    }

    fn show_owner(&self, _path: &str) {}
}

/// The debounced autosave, fired by hand: `fire` is what the app's timer callback does.
#[derive(Default)]
struct ManualAutosave {
    pending: Cell<bool>,
}

impl AutosaveScheduler for ManualAutosave {
    fn schedule(&self) {
        self.pending.set(true);
    }

    fn cancel(&self) {
        self.pending.set(false);
    }
}

impl ManualAutosave {
    fn fire(&self, session: &mut BookArticleSession) {
        if self.pending.replace(false) {
            session.flush_now();
        }
    }
}

#[test]
fn autosave_flush_hands_off_when_a_document_opened_the_article_mid_session() {
    // Swift flushNow: `let owner = documentOwning(url)` is read before the write and `if owner !=
    // nil { stage = .handoff(url) }` after it — the debounced autosave is the one flush that
    // reaches this without going through recheck_ownership, and it must still step aside so there
    // is never a second writer.
    let root = TempDir::new("md-adversarial-");
    let article = root.join(&["01-Scene.md"]);
    fs::write(&article, "# Scene\n").unwrap();
    let ownership = Rc::new(FakeOwnership::default());
    let autosave = Rc::new(ManualAutosave::default());
    let mut session = BookArticleSession::new().with_ownership(ownership.clone()).with_autosave(autosave.clone());
    session.open_book(&root.path);
    assert!(session.select(Some(&article)));
    session.edit("# Typed here, then opened in a window\n");
    assert!(autosave.pending.get());

    ownership.owned.borrow_mut().insert(article.clone());
    autosave.fire(&mut session);

    assert_eq!(&BookStage::Handoff(book_paths::standardize(&article)), session.stage());
    assert_eq!(None, session.editing_path());
    assert!(!session.dirty());
    assert_eq!("# Typed here, then opened in a window\n", read_text(&article));
    // And the reclaim on the way back is a fresh load, not a stale buffer.
    ownership.owned.borrow_mut().clear();
    fs::write(&article, "# Edited in the window\n").unwrap();
    session.recheck_ownership();
    assert_eq!(Some(book_paths::standardize(&article).as_str()), session.editing_path());
    assert_eq!("# Edited in the window\n", session.text());
    session.close_book();
}

#[test]
fn reload_from_disk_never_writes_the_buffer_even_when_the_flush_could_land() {
    // Swift resolveConflictReloading clears dirty BEFORE detaching: "writing the buffer is exactly
    // what must not happen here". The existing conflict tests cannot tell — their stamps have
    // moved, so the detach's flush would be refused anyway. Here a stray Deleted notice (the file
    // was recreated by the time the event arrived, stamp unchanged) conflicts the session while a
    // flush COULD land.
    let fs = Rc::new(RecordingFileSystem::default());
    let root = memory_root();
    let article = combine(&[&root, "01-Scene.md"]);
    fs.files.borrow_mut().insert(article.clone(), b"# Scene\n".to_vec());
    let mut session = BookArticleSession::new().with_file_system(fs.clone());
    session.open_book(&root);
    assert!(session.select(Some(&article)));
    session.edit("# Mine, unsaved\n");
    session.file_deleted();
    assert!(session.conflicted());
    assert_eq!(BookStageKind::Editing, session.stage().kind());

    session.resolve_conflict_reloading();

    assert!(fs.writes.borrow().is_empty());
    assert_eq!("# Scene\n", session.text());
    assert_eq!(b"# Scene\n".to_vec(), fs.files.borrow()[&article]);
    assert!(!session.dirty());
    assert!(!session.conflicted());
    assert_eq!(&BookStage::Editing(article.clone()), session.stage());
    // Keep My Version is the one path that writes without a stamp check.
    session.edit("# Mine again\n");
    session.file_deleted();
    session.resolve_conflict_keeping_mine();
    assert_eq!(vec![article.clone()], *fs.writes.borrow());
    assert_eq!(b"# Mine again\n".to_vec(), fs.files.borrow()[&article]);
    session.close_book();
    assert_eq!(vec![article], *fs.writes.borrow());
}

#[test]
fn select_is_idempotent_across_spellings_of_the_same_path() {
    // Swift compares `current == target` on standardizedFileURLs, so a selection restored as
    // "Book/./01-Scene.md" or "Book/../Book/01-Scene.md" is the article already open: no flush, no
    // reload, the undo stack and the unsaved text survive.
    let root = TempDir::new("md-adversarial-");
    let article = root.join(&["01-Scene.md"]);
    fs::write(&article, "# Scene\n").unwrap();
    let mut session = BookArticleSession::new();
    session.open_book(&root.path);
    assert!(session.select(Some(&article)));
    session.edit("# Still typing\n");
    let generation = session.undo_generation();

    assert!(session.select(Some(&root.join(&[".", "01-Scene.md"]))));
    assert!(session.select(Some(&root.join(&["..", book_paths::name(&root.path), "01-Scene.md"]))));
    assert!(session.select(Some(&format!("{article}/"))));

    assert_eq!(generation, session.undo_generation());
    assert!(session.dirty());
    assert_eq!("# Still typing\n", session.text());
    assert_eq!("# Scene\n", read_text(&article));
    assert_eq!(Some(book_paths::standardize(&article).as_str()), session.editing_path());
    session.close_book();
    assert_eq!("# Still typing\n", read_text(&article));
}
