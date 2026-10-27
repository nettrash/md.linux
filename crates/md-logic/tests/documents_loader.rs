//! Port of md.win `tests/Md.App.Logic.Tests/DocumentLoaderTests.cs`.
//!
//! §6.1, §6.3 and §6.5: what one path turns into, and which alert each failure earns.

mod common;

use common::document_fixtures::{cp1251, notes_pack};
use md_core::document::plain_text_codec::TextEncoding;
use md_core::export::text_bundle;
use md_logic::documents::{DocumentKind, LoadFailure, NewLine, document_loader, file_names};
use md_logic::strings;
use md_logic::testing::*;

#[test]
fn a_plain_file_is_edited_in_place_under_its_own_stem() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Docs/Chapter One.md", "# Title\r\nBody\r\n");

    let load = document_loader::load("/Docs/Chapter One.md", &fs);

    assert_eq!(load.failure, LoadFailure::None);
    assert_eq!(load.alert_title(), None);
    let document = load.document.unwrap();
    assert_eq!(document.kind, DocumentKind::PlainText);
    assert_eq!(document.text, "# Title\nBody\n");
    assert_eq!(document.dressing.new_line, NewLine::CrLf);
    assert_eq!(document.title, "Chapter One");
}

#[test]
fn a_legacy_encoded_file_keeps_its_encoding() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Docs/legacy.md", cp1251("Привет, мир!"));

    let document = document_loader::load("/Docs/legacy.md", &fs).document.unwrap();

    assert_eq!(document.text, "Привет, мир!");
    assert_eq!(document.dressing.encoding, TextEncoding::WindowsCp1251);
}

#[test]
fn a_missing_file_is_could_not_open_with_the_systems_own_message() {
    let load = document_loader::load("/Docs/gone.md", &FakeFileSystem::new());

    assert_eq!(load.document, None);
    assert_eq!(load.failure, LoadFailure::Unreadable);
    assert_eq!(load.alert_title(), Some(strings::documents::COULD_NOT_OPEN));
    assert_ne!(load.error_text, "");
}

#[test]
fn a_text_pack_imports_as_an_untitled_document_titled_after_the_bundle() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Downloads/Notes.textpack", notes_pack("# From a pack\n"));

    let load = document_loader::load("/Downloads/Notes.textpack", &fs);

    assert_eq!(load.failure, LoadFailure::None);
    let document = load.document.unwrap();
    assert_eq!(document.kind, DocumentKind::TextPack);
    assert_eq!(document.text, "# From a pack\n");
    assert_eq!(document.title, "Notes");
}

#[test]
fn zip_bytes_under_a_markdown_name_are_still_read_as_a_pack_not_as_text() {
    // md-core's sniff: the full PK\x03\x04 magic, never the two letters. Feeding zip bytes to the
    // text decoder is what Latin-1 would happily "succeed" at.
    let fs = FakeFileSystem::new();
    fs.add_file("/Downloads/mislabelled.md", notes_pack("# Inside\n"));

    assert_eq!(document_loader::load("/Downloads/mislabelled.md", &fs).document.unwrap().kind, DocumentKind::TextPack);
}

#[test]
fn prose_that_begins_with_the_letters_pk_is_ordinary_text() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Docs/pk.md", "PK is a person, not an archive.\n");

    assert_eq!(document_loader::load("/Docs/pk.md", &fs).document.unwrap().kind, DocumentKind::PlainText);
}

#[test]
fn a_corrupt_pack_earns_the_text_pack_alert_and_is_never_decoded_as_text() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Downloads/broken.textpack", [0x50, 0x4B, 0x03, 0x04, 0xFF, 0xFF, 0xFF]);

    let load = document_loader::load("/Downloads/broken.textpack", &fs);

    assert_eq!(load.document, None);
    assert_eq!(load.failure, LoadFailure::BundleUnreadable);
    assert_eq!(load.alert_title(), Some(strings::documents::TEXT_PACK_UNREADABLE));
}

#[test]
fn a_text_bundle_folder_imports_its_text_file() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Docs/Trip.textbundle/text.md", "# Trip\n");
    fs.add_file("/Docs/Trip.textbundle/info.json", "{}");
    fs.add_file("/Docs/Trip.textbundle/assets/photo.png", "PNG");

    let document = document_loader::load("/Docs/Trip.textbundle", &fs).document.unwrap();

    assert_eq!(document.kind, DocumentKind::TextBundleFolder);
    assert_eq!(document.text, "# Trip\n");
    assert_eq!(document.title, "Trip");
}

#[test]
fn a_bundle_folder_falls_back_from_text_md_to_text_markdown_to_any_text_file() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Docs/A.textbundle/text.markdown", "markdown wins over text.txt");
    fs.add_file("/Docs/A.textbundle/text.txt", "not this one");
    assert_eq!(document_loader::load("/Docs/A.textbundle", &fs).document.unwrap().text, "markdown wins over text.txt");

    let only = FakeFileSystem::new();
    only.add_file("/Docs/B.textbundle/text.txt", "the only one");
    assert_eq!(document_loader::load("/Docs/B.textbundle", &only).document.unwrap().text, "the only one");
}

#[test]
fn a_bundle_folder_with_no_text_file_earns_the_bundle_alert() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Docs/Empty.textbundle/info.json", "{}");

    let load = document_loader::load("/Docs/Empty.textbundle", &fs);

    assert_eq!(load.document, None);
    assert_eq!(load.failure, LoadFailure::BundleUnreadable);
}

#[test]
fn a_file_named_textbundle_that_is_not_a_folder_is_just_a_file() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Docs/odd.textbundle", "# Actually text\n");

    assert_eq!(document_loader::load("/Docs/odd.textbundle", &fs).document.unwrap().kind, DocumentKind::PlainText);
}

#[test]
fn a_bundle_folder_is_listed_for_its_text_file_only_never_for_its_assets() {
    // md.win wraps the fake in a counting file system; this fake records its reads itself.
    let fs = FakeFileSystem::new();
    fs.add_file("/Docs/Big.textbundle/text.md", "# Big\n");
    // Both shapes of neighbour a real bundle has: a top-level sibling of text.md — every bundle
    // carries info.json, and a cover image is common — and the assets folder. Only the second is
    // filtered out by "is it a file"; without the "text." prefix test the first is read into
    // memory, which is the whole reason the prefix is there.
    fs.add_file("/Docs/Big.textbundle/info.json", text_bundle::INFO_JSON);
    fs.add_file("/Docs/Big.textbundle/cover.png", "c".repeat(4096));
    fs.add_file("/Docs/Big.textbundle/assets/huge.bin", "x".repeat(4096));

    document_loader::load("/Docs/Big.textbundle", &fs);

    assert_eq!(fs.reads(), ["/Docs/Big.textbundle/text.md"]);
}

#[test]
fn an_alias_extension_is_an_ordinary_file_edited_in_place_under_its_own_stem() {
    // Every spelling the family associates is a plain file to the loader: read, decoded, edited in
    // place under its own name, never mistaken for a bundle. And the alias is in the Open filter,
    // so File ▸ Open… finds the same file a double-click delivers.
    let rows = [
        ("/Docs/Notes.mkd", "Notes"),
        ("/Docs/Notes.mdtxt", "Notes"),
        ("/Docs/Notes.mkdown", "Notes"),
        ("/Diagrams/Login Flow.iuml", "Login Flow"),
        ("/Diagrams/Login Flow.pu", "Login Flow"),
        ("/Docs/readme.text", "readme"),
    ];
    for (path, title) in rows {
        let fs = FakeFileSystem::new();
        fs.add_file(path, "@startuml\r\nA -> B\r\n@enduml\r\n");

        let load = document_loader::load(path, &fs);

        assert_eq!(load.failure, LoadFailure::None, "{path}");
        let document = load.document.unwrap();
        assert_eq!(document.kind, DocumentKind::PlainText);
        assert_eq!(document.path, path);
        assert_eq!(document.text, "@startuml\nA -> B\n@enduml\n");
        assert_eq!(document.dressing.new_line, NewLine::CrLf);
        assert_eq!(document.title, title);
        assert!(document_loader::OPEN_EXTENSIONS.contains(&file_names::extension_of(path).as_str()), "{path}");
    }
}

#[test]
fn the_picker_sets_are_the_macs_lists_in_the_macs_order() {
    assert_eq!(
        document_loader::MARKDOWN_EXTENSIONS,
        [".md", ".markdown", ".mdown", ".markdn", ".mdtext", ".mdtxt", ".mkd", ".mkdn", ".mdwn", ".mkdown"]
    );
    assert_eq!(document_loader::PLAIN_TEXT_EXTENSIONS, [".txt", ".text"]);
    assert_eq!(document_loader::PLANT_UML_EXTENSIONS, [".puml", ".plantuml", ".iuml", ".pu"]);
    assert_eq!(document_loader::GRAPHVIZ_EXTENSIONS, [".gv"]);
    assert_eq!(
        document_loader::OPEN_EXTENSIONS,
        [
            ".md",
            ".markdown",
            ".mdown",
            ".markdn",
            ".mdtext",
            ".mdtxt",
            ".mkd",
            ".mkdn",
            ".mdwn",
            ".mkdown",
            ".txt",
            ".text",
            ".puml",
            ".plantuml",
            ".iuml",
            ".pu",
            ".gv",
            ".textpack",
        ]
    );
    // The Open list is the four groups plus md-core's pack extension, nothing hand-copied.
    let concatenated: Vec<&str> = document_loader::MARKDOWN_EXTENSIONS
        .iter()
        .chain(&document_loader::PLAIN_TEXT_EXTENSIONS)
        .chain(&document_loader::PLANT_UML_EXTENSIONS)
        .chain(&document_loader::GRAPHVIZ_EXTENSIONS)
        .copied()
        .chain([text_bundle::PACK_EXTENSION])
        .collect();
    assert_eq!(document_loader::OPEN_EXTENSIONS.to_vec(), concatenated);
    assert_eq!(document_loader::open_extensions(), document_loader::OPEN_EXTENSIONS.map(String::from).to_vec());

    let labels: Vec<&str> = document_loader::SAVE_CHOICES.iter().map(|(label, _)| *label).collect();
    assert_eq!(
        labels,
        [
            strings::exports::MARKDOWN_DOCUMENT,
            strings::exports::PLAIN_TEXT,
            strings::exports::PLANT_UML_DIAGRAM,
            strings::exports::GRAPHVIZ_DOT_GRAPH
        ]
    );
    let writable: Vec<&str> = document_loader::SAVE_CHOICES.iter().flat_map(|(_, e)| e.iter().copied()).collect();
    assert_eq!(
        writable,
        [
            ".md",
            ".markdown",
            ".mdown",
            ".markdn",
            ".mdtext",
            ".mdtxt",
            ".mkd",
            ".mkdn",
            ".mdwn",
            ".mkdown",
            ".txt",
            ".text",
            ".puml",
            ".plantuml",
            ".iuml",
            ".pu",
            ".gv",
        ]
    );
    // Never a bundle type: saving one back would drop its assets.
    assert!(!writable.iter().any(|e| *e == ".textpack" || *e == ".textbundle"));
    // The seam's shape carries the same groups.
    let choices = document_loader::save_file_type_choices();
    assert_eq!(choices.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), labels);
    assert_eq!(choices.iter().flat_map(|c| c.extensions.iter().map(String::as_str)).collect::<Vec<_>>(), writable);
}

#[test]
fn the_save_picker_defaults_to_the_documents_own_extension() {
    let rows = [
        (Some("/Docs/a.md"), ".md"),
        (Some("/Docs/a.mkd"), ".mkd"),
        (Some("/Docs/a.puml"), ".puml"),
        (Some("/Docs/a.iuml"), ".iuml"),
        (Some("/Docs/a.text"), ".text"),
        (Some("/Docs/README"), ".md"),
        (None, ".md"),
    ];
    for (path, expected) in rows {
        assert_eq!(document_loader::default_extension_for(path), expected, "{path:?}");
    }
}
