//! Port of md.win `BookCodecSwapTests`: the two places md.win's shipped stand-in codec and the real
//! plain-text codec disagree, now that the session reads and writes through the real one. Both
//! differences are the codec's Foundation measurements reaching the book window: a UTF-8 BOM is not
//! text, and 0x98 is not Windows-1251.

mod book_support;

use std::fs;

use book_support::{TempDir, cp1251, latin1};
use md_core::book::book_article_session::BookArticleSession;

#[test]
fn a_bomed_utf8_article_loses_its_bom_on_read_and_on_save() {
    // Foundation's .utf8 decode drops a leading EF BB BF and its encode writes none back (measured).
    // The stand-in kept the U+FEFF in the buffer, so it reached the preview, the word count and the
    // next save.
    let scratch = TempDir::new("md-codec-");
    let article = scratch.join(&["01-Bom.md"]);
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"# Scene\n");
    fs::write(&article, bytes).unwrap();
    let mut session = BookArticleSession::new();
    session.open_book(&scratch.path);

    assert!(session.select(Some(&article)));
    assert_eq!("# Scene\n", session.text());
    assert_eq!(65001, session.code_page());

    session.edit("# Scene\n\nMore.\n");
    assert!(session.flush_now());
    assert_eq!(b"# Scene\n\nMore.\n".to_vec(), fs::read(&article).unwrap());
    session.close_book();
}

#[test]
fn the_one_byte_windows1251_leaves_undefined_opens_as_latin1() {
    // 0x98 is the single undefined byte in CP1251 and Foundation refuses it in both directions;
    // .NET's table maps it to U+0098 anyway, so md.win's codec refuses it by hand, and so does this
    // one. The stand-in did not, and read such a file as CP1251 — which would have saved it back as
    // CP1251 on the Mac's Latin-1 file.
    let scratch = TempDir::new("md-codec-");
    let mut bytes = cp1251("Привет");
    bytes.push(0x98);
    let article = scratch.join(&["01-Undefined.md"]);
    fs::write(&article, &bytes).unwrap();
    let mut session = BookArticleSession::new();
    session.open_book(&scratch.path);

    assert!(session.select(Some(&article)));
    assert_eq!(28591, session.code_page());
    assert_eq!(latin1(&bytes), session.text());

    // And it round-trips as Latin-1: every byte survives a save untouched.
    let edited = format!("{}!", session.text());
    session.edit(&edited);
    assert!(session.flush_now());
    let mut expected = bytes.clone();
    expected.push(b'!');
    assert_eq!(expected, fs::read(&article).unwrap());
    assert_eq!(28591, session.code_page());
    session.close_book();
}
