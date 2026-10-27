//! Port of md.win `tests/Md.App.Logic.Tests/AssetReaderTests.cs`.
//!
//! §7.7's containment rule. Export ▸ TextBundle… is the one feature that reads files the writer
//! never picked, so these are the tests that decide whether an `![](../secrets.txt)` in a
//! downloaded document copies something out of the user's machine.

use std::io;
use std::rc::Rc;

use md_logic::documents::asset_reader;
use md_logic::testing::*;

fn world() -> (Rc<FakeFileSystem>, Rc<FakeFileIdentity>) {
    let fs = Rc::new(FakeFileSystem::new());
    fs.add_file("/Docs/note.md", "# Note");
    fs.add_file("/Docs/photo.png", "PNG");
    fs.add_file("/Docs/images/logo.png", "LOGO");
    fs.add_file("/Secrets/passwords.txt", "SECRET");
    (fs, Rc::new(FakeFileIdentity::new()))
}

fn read(
    fs: &Rc<FakeFileSystem>,
    id: &Rc<FakeFileIdentity>,
    reference: &str,
    document_path: Option<&str>,
) -> Option<String> {
    let resolver = asset_reader::beside(document_path, fs.clone(), id.clone());
    resolver(reference).map(|bytes| String::from_utf8(bytes).unwrap())
}

fn read_beside_note(fs: &Rc<FakeFileSystem>, id: &Rc<FakeFileIdentity>, reference: &str) -> Option<String> {
    read(fs, id, reference, Some("/Docs/note.md"))
}

#[test]
fn an_image_beside_the_document_is_read() {
    let rows = [
        ("photo.png", "PNG"),
        ("./photo.png", "PNG"),
        ("images/logo.png", "LOGO"),
        ("images//logo.png", "LOGO"),
        ("images/../photo.png", "PNG"),
    ];
    for (reference, expected) in rows {
        let (fs, id) = world();
        assert_eq!(read_beside_note(&fs, &id, reference).as_deref(), Some(expected), "{reference:?}");
    }
    // LINUX: md.win also reads `images\logo.png` as the folder's images/logo.png. On Linux a
    // backslash is part of a name — that reference names a file called `images\logo.png`, which
    // is not there.
    let (fs, id) = world();
    assert_eq!(read_beside_note(&fs, &id, "images\\logo.png"), None);
    fs.add_file("/Docs/images\\logo.png", "ONE NAME");
    assert_eq!(read_beside_note(&fs, &id, "images\\logo.png").as_deref(), Some("ONE NAME"));
}

#[test]
fn nothing_outside_the_documents_own_folder_is_ever_read() {
    let references = [
        "..\\Secrets\\passwords.txt",
        "../Secrets/passwords.txt",
        "C:\\Secrets\\passwords.txt",
        "\\Secrets\\passwords.txt",
        "/Secrets/passwords.txt",
        "\\\\server\\share\\x.png",
        "../../../../etc/passwd",
        "",
    ];
    for reference in references {
        let (fs, id) = world();
        assert_eq!(read_beside_note(&fs, &id, reference), None, "{reference:?}");
    }
}

#[test]
fn a_missing_file_is_simply_not_found() {
    let (fs, id) = world();
    assert_eq!(read_beside_note(&fs, &id, "absent.png"), None);
}

#[test]
fn an_unsaved_document_resolves_nothing_so_it_exports_with_empty_assets() {
    let (fs, id) = world();
    assert_eq!(read(&fs, &id, "photo.png", None), None);
    assert_eq!(read(&fs, &id, "photo.png", Some("")), None);
    assert!(id.calls().is_empty()); // and nothing is even canonicalised
}

#[test]
fn a_link_pointing_out_of_the_folder_is_refused_even_though_the_name_is_inside() {
    let (fs, id) = world();
    // A symlink beside the document whose real target is elsewhere: the name passes the lexical
    // test, the canonical path does not.
    fs.add_file("/Docs/link.png", "VIA LINK");
    id.alias("/Docs/link.png", "/Secrets/real.png");

    assert_eq!(read_beside_note(&fs, &id, "link.png"), None);
}

#[test]
fn a_folder_whose_name_merely_starts_with_the_documents_folder_is_outside() {
    // "/Docs2" starts with "/Docs" as a string; it is not inside it.
    assert!(!asset_reader::is_strictly_inside("/Docs2/x.png", "/Docs"));
    assert!(asset_reader::is_strictly_inside("/Docs/x.png", "/Docs"));
    assert!(asset_reader::is_strictly_inside("/Docs//sub/x.png", "/Docs/")); // separators are separators
    assert!(!asset_reader::is_strictly_inside("/Docs", "/Docs")); // strictly inside
    // LINUX: md.win's "Windows' case rule" row — "/DOCS/x.png" inside "/docs" — is another folder
    // on ext4.
    assert!(!asset_reader::is_strictly_inside("/DOCS/x.png", "/docs"));
}

/// A file system whose reads always fail — md.win's `ThrowingFileSystem`.
struct ThrowingFileSystem(Rc<FakeFileSystem>);

impl FileSystem for ThrowingFileSystem {
    fn file_exists(&self, path: &str) -> bool {
        self.0.file_exists(path)
    }
    fn directory_exists(&self, path: &str) -> bool {
        self.0.directory_exists(path)
    }
    fn read_all_bytes(&self, _path: &str) -> io::Result<Vec<u8>> {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "locked"))
    }
    fn write_all_bytes_in_place(&self, path: &str, bytes: &[u8]) -> io::Result<()> {
        self.0.write_all_bytes_in_place(path, bytes)
    }
    fn stamp(&self, path: &str) -> Option<FileStamp> {
        self.0.stamp(path)
    }
    fn move_file(&self, from: &str, to: &str) -> io::Result<()> {
        self.0.move_file(from, to)
    }
    fn delete(&self, path: &str) -> io::Result<()> {
        self.0.delete(path)
    }
    fn enumerate_entries(&self, directory: &str) -> io::Result<Vec<String>> {
        self.0.enumerate_entries(directory)
    }
}

#[test]
fn an_unreadable_file_is_treated_as_not_found_so_the_reference_stays_as_written() {
    let (fs, id) = world();
    let reader = asset_reader::beside(Some("/Docs/note.md"), Rc::new(ThrowingFileSystem(fs)), id);
    assert_eq!(reader("photo.png"), None);
}

// ---- the cap (md.win's `maxBytes` overload; no test of its own there) ----

#[test]
fn a_file_over_the_cap_is_not_found_and_never_read() {
    let (fs, id) = world();
    let reader = asset_reader::beside_capped(Some("/Docs/note.md"), fs.clone(), id, 2);
    assert_eq!(reader("photo.png"), None); // 3 bytes
    assert!(fs.reads().is_empty());
}
