//! Port of md.win `tests/Md.App.Logic.Tests/FileAssociationTests.cs`.
//!
//! §6.1: the packaging's file-type associations and `document_loader`'s picker lists are one
//! list — the family's canonical extension sets — pinned here as literals, so neither side can
//! drift without this file noticing. (They did once on Windows: the loader offered `.text` in the
//! Open dialog while the manifest registered only `.txt`.) Every extension is a literal on purpose:
//! a test that compares the packaging to `MARKDOWN_EXTENSIONS` alone passes for any list the two
//! happen to share.
//!
//! LINUX: md.win reads `Package.appxmanifest`. Linux associates by MIME type: a shared-mime-info
//! package ([`MIME_PACKAGE`]: one `<mime-type>` per kind, one `<glob pattern="*.ext"/>` per
//! extension) gives each extension its type, and the desktop entry ([`DESKTOP_ENTRY`]) claims the
//! types in its `MimeType=` key. Neither file exists yet, so every test that reads them is ignored
//! until the packaging is written; the halves that check md-logic's own lists run. The packaging
//! work may rename the two files or the private MIME types below — deliberately, here.
//!
//! An open question for that work, recorded so it is not lost: shared-mime-info's own
//! `text/vnd.graphviz` globs `*.dot` as well as `*.gv`, so a desktop entry that claims
//! `text/vnd.graphviz` makes md a handler for `.dot` — the Word template every md port leaves
//! unclaimed — unless the `.gv` association uses a type of md's own.

mod common;

use std::collections::BTreeSet;

use common::document_fixtures::repo_root;
use md_logic::documents::document_loader;
use md_logic::strings;

/// The shared-mime-info package, relative to the repository root.
const MIME_PACKAGE: &str = "packaging/me.nettrash.md.xml";

/// The desktop entry, relative to the repository root (named after the application id, as
/// `GApplication`'s D-Bus activation requires).
const DESKTOP_ENTRY: &str = "packaging/me.nettrash.md.desktop";

const PACKAGING: &str = "packaging not yet written";

// The canonical sets, per kind, in the order every md port declares them.
const MARKDOWN: [&str; 10] =
    [".md", ".markdown", ".mdown", ".markdn", ".mdtext", ".mdtxt", ".mkd", ".mkdn", ".mdwn", ".mkdown"];
const PLANT_UML: [&str; 4] = [".puml", ".plantuml", ".iuml", ".pu"];
const GRAPHVIZ: [&str; 1] = [".gv"];
const TEXT_PACK: [&str; 1] = [".textpack"];
const PLAIN_TEXT: [&str; 2] = [".txt", ".text"];

/// The MIME type each kind is declared under, in md.win's association order (markdown, plantuml,
/// graphviz, textpack, plaintext). md.win's `ContentType`s are text/markdown, text/plain,
/// text/vnd.graphviz, application/zip, text/plain; Linux needs one type per kind for the globs.
const KINDS: [(&str, &[&str]); 5] = [
    ("text/markdown", &MARKDOWN),
    ("text/x-plantuml", &PLANT_UML),
    ("text/vnd.graphviz", &GRAPHVIZ),
    ("application/x-textpack", &TEXT_PACK),
    ("text/plain", &PLAIN_TEXT),
];

fn read(relative: &str) -> String {
    let path = format!("{}/{relative}", repo_root());
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Each `<mime-type type="…">` of the package with its globs' extensions (`*.md` → `.md`), in
/// document order. A hand scan: the package is a flat, generated-by-hand file, and md-logic's
/// tests take no XML dependency.
fn mime_types() -> Vec<(String, Vec<String>)> {
    let xml = read(MIME_PACKAGE);
    let mut types = Vec::new();
    for block in xml.split("<mime-type").skip(1) {
        let block = block.split("</mime-type>").next().unwrap_or("");
        let name = attribute(block, "type").expect("a <mime-type> carries type=\"…\"");
        let globs = block
            .split("<glob")
            .skip(1)
            .filter_map(|glob| attribute(glob, "pattern"))
            .map(|pattern| pattern.trim_start_matches('*').to_string())
            .collect();
        types.push((name, globs));
    }
    types
}

fn attribute(element: &str, name: &str) -> Option<String> {
    let start = element.find(&format!("{name}=\""))? + name.len() + 2;
    let end = element[start..].find('"')? + start;
    Some(element[start..end].to_string())
}

/// The desktop entry's `MimeType=` list.
fn desktop_mime_types() -> Vec<String> {
    let entry = read(DESKTOP_ENTRY);
    let line = entry.lines().find(|l| l.starts_with("MimeType=")).expect("the entry has a MimeType= key");
    line["MimeType=".len()..].split(';').filter(|t| !t.is_empty()).map(str::to_string).collect()
}

fn packaging_extensions() -> Vec<String> {
    mime_types().into_iter().flat_map(|(_, globs)| globs).collect()
}

#[test]
#[ignore = "packaging not yet written"]
fn the_manifest_declares_exactly_the_canonical_sets_one_association_per_kind() {
    let types = mime_types();
    assert_eq!(types.len(), KINDS.len(), "{PACKAGING}");
    for ((name, globs), (expected_name, expected_globs)) in types.iter().zip(KINDS) {
        assert_eq!(name, expected_name);
        assert_eq!(globs, &expected_globs.iter().map(|e| e.to_string()).collect::<Vec<_>>(), "{name}");
    }
}

#[test]
#[ignore = "packaging not yet written"]
fn every_file_type_carries_its_kinds_content_type() {
    // The aliases must not be pasted in under the wrong type: a .mkd is text/markdown like a .md, a
    // .pu is PlantUML like a .puml. And the desktop entry claims exactly the five types.
    for (name, globs) in mime_types() {
        let (_, expected) =
            KINDS.iter().find(|(kind, _)| *kind == name).unwrap_or_else(|| panic!("unexpected type {name}"));
        for glob in globs {
            assert!(expected.contains(&glob.as_str()), "{glob} under {name}");
        }
    }
    assert_eq!(desktop_mime_types(), KINDS.map(|(name, _)| name.to_string()));
}

#[test]
#[ignore = "packaging not yet written"]
fn an_extension_is_declared_once_lowercase_and_with_its_dot() {
    // shared-mime-info matches globs case-sensitively first and then folded; one lowercase spelling
    // per extension, under one type, keeps the answer unambiguous.
    let all = packaging_extensions();
    assert_eq!(all.iter().collect::<BTreeSet<_>>().len(), all.len());
    for e in &all {
        assert!(e.starts_with('.'), "{e}");
        assert_eq!(e.to_ascii_lowercase(), *e);
        assert_eq!(e.matches('.').count(), 1, "{e}");
        assert!(!e.contains(' '), "{e}");
    }
}

#[test]
#[ignore = "packaging not yet written"]
fn the_open_picker_offers_every_associated_type_and_nothing_else() {
    // Both directions. A type the desktop hands us through a double-click that the Open dialog
    // cannot show, and a type the dialog shows that the file manager never sends us, are the same
    // drift.
    let mut packaging = packaging_extensions();
    packaging.sort();
    let mut open: Vec<String> = document_loader::OPEN_EXTENSIONS.iter().map(|e| e.to_string()).collect();
    open.sort();
    assert_eq!(packaging, open);
    assert!(!packaging.iter().any(|e| e == ".textbundle" || e == ".dot"));
}

/// The md-logic half of md.win's `TheOpenPickerOffersEveryAssociatedTypeAndNothingElse`:
/// `.textbundle` is a folder — opened through File ▸ Open TextBundle Folder… (§6.5), which a file
/// chooser in file mode cannot pick — and `.dot` stays unclaimed (Word's template); Graphviz is
/// `.gv` only.
#[test]
fn the_open_picker_never_offers_a_folder_or_a_word_template() {
    assert!(!document_loader::OPEN_EXTENSIONS.contains(&".textbundle"));
    assert!(!document_loader::OPEN_EXTENSIONS.contains(&".dot"));
    let associated: Vec<&str> = KINDS.iter().flat_map(|(_, e)| e.iter().copied()).collect();
    let mut sorted_associated = associated.clone();
    sorted_associated.sort();
    let mut sorted_open = document_loader::OPEN_EXTENSIONS.to_vec();
    sorted_open.sort();
    assert_eq!(sorted_open, sorted_associated);
}

#[test]
fn the_loader_lists_are_the_canonical_sets_in_the_manifests_order() {
    assert_eq!(document_loader::MARKDOWN_EXTENSIONS, MARKDOWN);
    assert_eq!(document_loader::PLANT_UML_EXTENSIONS, PLANT_UML);
    assert_eq!(document_loader::GRAPHVIZ_EXTENSIONS, GRAPHVIZ);
    assert_eq!(document_loader::PLAIN_TEXT_EXTENSIONS, PLAIN_TEXT);
}

#[test]
fn the_save_picker_writes_every_associated_type_except_the_pack() {
    // Save As offers each kind under its own label with the canonical extensions, so a .mkd or an
    // .iuml saves back in place under its own name; .textpack is import-only (§6.5).
    let by_label = |label: &str| -> Vec<&str> {
        document_loader::SAVE_CHOICES.iter().find(|(l, _)| *l == label).map(|(_, e)| e.to_vec()).unwrap()
    };
    assert_eq!(by_label(strings::exports::MARKDOWN_DOCUMENT), MARKDOWN);
    assert_eq!(by_label(strings::exports::PLAIN_TEXT), PLAIN_TEXT);
    assert_eq!(by_label(strings::exports::PLANT_UML_DIAGRAM), PLANT_UML);
    assert_eq!(by_label(strings::exports::GRAPHVIZ_DOT_GRAPH), GRAPHVIZ);
    assert_eq!(document_loader::SAVE_CHOICES.len(), 4);

    let mut writable: Vec<&str> = document_loader::SAVE_CHOICES.iter().flat_map(|(_, e)| e.iter().copied()).collect();
    writable.sort();
    let mut associated: Vec<&str> =
        KINDS.iter().flat_map(|(_, e)| e.iter().copied()).filter(|e| !TEXT_PACK.contains(e)).collect();
    associated.sort();
    assert_eq!(writable, associated);
}

/// The packaging half of md.win's `TheSavePickerWritesEveryAssociatedTypeExceptThePack`.
#[test]
#[ignore = "packaging not yet written"]
fn the_save_picker_writes_every_associated_type_except_the_pack_according_to_the_packaging() {
    let mut writable: Vec<String> =
        document_loader::SAVE_CHOICES.iter().flat_map(|(_, e)| e.iter().map(|x| x.to_string())).collect();
    writable.sort();
    let mut associated: Vec<String> =
        packaging_extensions().into_iter().filter(|e| !TEXT_PACK.contains(&e.as_str())).collect();
    associated.sort();
    assert_eq!(writable, associated);
}
