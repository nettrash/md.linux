//! The file associations are one list kept in three places (docs/packaging.md §2–§3, shell-design
//! §6.1): md-logic's `DocumentLoader` extensions, the MIME types `me.nettrash.md.desktop` lists, and
//! the globs that map an extension to one of those types — Ubuntu 26.04's shared-mime-info for the
//! spellings it already knows, `packaging/me.nettrash.md.xml` for the rest. They are pinned to each
//! other here in both directions, as md.win's `FileAssociationTests` pins its manifest: a type the
//! desktop hands md that the Open dialog cannot show, and a type the dialog shows that a
//! double-click never sends to md, are the same drift.
//!
//! Every extension and type is a literal on purpose: a test that only compared the files with
//! `DocumentLoader::OPEN_EXTENSIONS` would pass for any list the two happened to share.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use md_logic::documents::document_loader;

// The canonical sets, per kind, in the order every md port declares them.
const MARKDOWN: [&str; 10] =
    [".md", ".markdown", ".mdown", ".markdn", ".mdtext", ".mdtxt", ".mkd", ".mkdn", ".mdwn", ".mkdown"];
const PLAIN_TEXT: [&str; 2] = [".txt", ".text"];
const PLANT_UML: [&str; 4] = [".puml", ".plantuml", ".iuml", ".pu"];
const GRAPHVIZ: [&str; 1] = [".gv"];
const TEXT_PACK: [&str; 1] = [".textpack"];

/// Each kind and the MIME type a double-click on it reaches md through.
const KINDS: [(&[&str], &str); 5] = [
    (&MARKDOWN, "text/markdown"),
    (&PLAIN_TEXT, "text/plain"),
    (&PLANT_UML, "text/x-plantuml"),
    (&GRAPHVIZ, "text/vnd.graphviz"),
    (&TEXT_PACK, "application/x-textpack"),
];

/// The `.desktop` file's `MimeType=`, in order: md's own type first.
const DESKTOP_TYPES: [&str; 5] =
    ["text/markdown", "text/plain", "text/x-plantuml", "text/vnd.graphviz", "application/x-textpack"];

/// What the AppStream file says md provides: every type but plain text, for which md is one
/// alternate among many.
const METAINFO_TYPES: [&str; 4] = ["text/markdown", "text/x-plantuml", "text/vnd.graphviz", "application/x-textpack"];

/// The globs Ubuntu 26.04's shared-mime-info 2.4 already has for md's extensions and for `.dot`
/// (read from `/usr/share/mime/packages/freedesktop.org.xml` in the image, docs/packaging.md §3).
/// `.dot` is both Graphviz and a Word template there; md's loader never claims it, but because the
/// `.desktop` file lists `text/vnd.graphviz` (for `.gv`), a `.dot` that sniffs as Graphviz lists
/// md under Open With — the documented consequence.
const SYSTEM_GLOBS: [(&str, &str); 7] = [
    ("*.md", "text/markdown"),
    ("*.mkd", "text/markdown"),
    ("*.markdown", "text/markdown"),
    ("*.txt", "text/plain"),
    ("*.gv", "text/vnd.graphviz"),
    ("*.dot", "text/vnd.graphviz"),
    ("*.dot", "application/msword-template"),
];

fn packaging(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("packaging").join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The `MimeType=` list of the `[Desktop Entry]` group.
fn desktop_types() -> Vec<String> {
    let desktop = packaging("me.nettrash.md.desktop");
    let mut in_entry = false;
    let mut found = Vec::new();
    for line in desktop.lines() {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some(list) = line.strip_prefix("MimeType=") {
            assert!(list.ends_with(';'), "MimeType= must end with ';': {list}");
            found.push(list.split(';').filter(|t| !t.is_empty()).map(str::to_string).collect::<Vec<_>>());
        }
    }
    assert_eq!(found.len(), 1, "exactly one MimeType= line in [Desktop Entry]");
    found.remove(0)
}

/// One `<mime-type>` element of a shared-mime-info package file.
#[derive(Debug, Default)]
struct MimeType {
    globs: Vec<String>,
    parents: Vec<String>,
    has_comment: bool,
}

/// The value of `name="…"` inside one tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let len = tag[start..].find('"')?;
    Some(tag[start..start + len].to_string())
}

/// Every `<mime-type type="…">` with its globs, parents and whether it has a `<comment>`, keyed by
/// type, in a shared-mime-info XML file. Enough of XML for these files: no CDATA, no entities in
/// the attributes read.
fn mime_types(xml: &str) -> BTreeMap<String, MimeType> {
    let mut types: BTreeMap<String, MimeType> = BTreeMap::new();
    let mut rest = xml;
    while let Some(open) = rest.find("<mime-type ") {
        let tag_end = open + rest[open..].find('>').expect("an unterminated <mime-type");
        let name = attribute(&rest[open..tag_end], "type").expect("a <mime-type> without type=");
        let close = tag_end + rest[tag_end..].find("</mime-type>").expect("a <mime-type> without its end tag");
        let body = &rest[tag_end + 1..close];
        let entry = types.entry(name).or_default();
        let mut tags = body;
        while let Some(lt) = tags.find('<') {
            let gt = lt + tags[lt..].find('>').expect("an unterminated tag");
            let tag = &tags[lt..gt];
            if tag.starts_with("<glob ") {
                entry.globs.push(attribute(tag, "pattern").expect("a <glob> without pattern="));
            } else if tag.starts_with("<sub-class-of ") {
                entry.parents.push(attribute(tag, "type").expect("a <sub-class-of> without type="));
            } else if tag == "<comment" || tag.starts_with("<comment ") {
                entry.has_comment = true;
            }
            tags = &tags[gt + 1..];
        }
        rest = &rest[close..];
    }
    types
}

fn shipped_types() -> BTreeMap<String, MimeType> {
    mime_types(&packaging("me.nettrash.md.xml"))
}

/// Every (glob, type) pair the shipped XML declares.
fn shipped_globs() -> Vec<(String, String)> {
    shipped_types().into_iter().flat_map(|(t, m)| m.globs.into_iter().map(move |g| (g, t.clone()))).collect()
}

/// The types an extension's glob reaches once md is installed: the system's and md's together.
fn types_for(extension: &str) -> BTreeSet<String> {
    let glob = format!("*{extension}");
    let system = SYSTEM_GLOBS.iter().filter(|(g, _)| *g == glob).map(|(_, t)| t.to_string());
    let shipped = shipped_globs().into_iter().filter(|(g, _)| *g == glob).map(|(_, t)| t);
    system.chain(shipped).collect()
}

#[test]
fn the_loader_lists_are_the_canonical_sets() {
    assert_eq!(document_loader::MARKDOWN_EXTENSIONS, MARKDOWN);
    assert_eq!(document_loader::PLAIN_TEXT_EXTENSIONS, PLAIN_TEXT);
    assert_eq!(document_loader::PLANT_UML_EXTENSIONS, PLANT_UML);
    assert_eq!(document_loader::GRAPHVIZ_EXTENSIONS, GRAPHVIZ);
    let every: Vec<&str> = KINDS.iter().flat_map(|(extensions, _)| extensions.iter().copied()).collect();
    assert_eq!(document_loader::OPEN_EXTENSIONS.to_vec(), every, "the Open dialog lists every kind, in order");
}

#[test]
fn the_desktop_entry_lists_exactly_the_associated_types() {
    assert_eq!(desktop_types(), DESKTOP_TYPES);
    let kinds: Vec<&str> = KINDS.iter().map(|(_, t)| *t).collect();
    assert_eq!(kinds, DESKTOP_TYPES, "one type per kind and no other");
}

#[test]
fn every_open_extension_reaches_its_kinds_type_and_only_that() {
    // Desktop direction: a double-click on any extension the loader opens finds md, because its
    // glob — the system's or ours — names a type the .desktop file lists, unambiguously.
    let listed = desktop_types();
    for (extensions, mime) in KINDS {
        for extension in extensions {
            let reached = types_for(extension);
            assert_eq!(reached, BTreeSet::from([mime.to_string()]), "{extension}");
            assert!(listed.iter().any(|t| t == mime), "{extension}: {mime} is not in MimeType=");
        }
    }
}

#[test]
fn every_listed_type_is_reached_by_an_open_extension() {
    // The other direction: no type in MimeType= that no extension md opens maps to.
    for listed in desktop_types() {
        let reached = document_loader::OPEN_EXTENSIONS.iter().any(|extension| types_for(extension).contains(&listed));
        assert!(reached, "{listed} is listed but no open extension reaches it");
    }
}

#[test]
fn the_shipped_globs_are_exactly_the_open_extensions_the_system_lacks() {
    let shipped = shipped_globs();
    let shipped_set: BTreeSet<&str> = shipped.iter().map(|(g, _)| g.as_str()).collect();
    assert_eq!(shipped_set.len(), shipped.len(), "a glob declared twice: {shipped:?}");
    for (glob, _) in &shipped {
        let extension = glob.strip_prefix('*').unwrap_or_else(|| panic!("{glob} is not *.ext"));
        assert!(extension.starts_with('.') && extension.matches('.').count() == 1, "{glob}");
        assert_eq!(*glob, glob.to_ascii_lowercase(), "{glob}");
        assert!(document_loader::OPEN_EXTENSIONS.contains(&extension), "{glob} is not an extension md opens");
        assert!(!SYSTEM_GLOBS.iter().any(|(g, _)| g == glob), "{glob} is already shared-mime-info's");
    }
    for extension in document_loader::OPEN_EXTENSIONS {
        let glob = format!("*{extension}");
        let known = SYSTEM_GLOBS.iter().any(|(g, _)| *g == glob) || shipped_set.contains(glob.as_str());
        assert!(known, "{extension} maps to no type");
    }
}

#[test]
fn textbundle_and_dot_are_never_claimed() {
    // .textbundle is a folder, which neither a file chooser in file mode nor a glob can pick;
    // .dot stays unclaimed as on macOS and Windows (a Word template there).
    for never in [".textbundle", ".dot"] {
        assert!(!document_loader::OPEN_EXTENSIONS.contains(&never), "{never}");
        assert!(!shipped_globs().iter().any(|(g, _)| *g == format!("*{never}")), "{never}");
    }
}

#[test]
fn the_new_types_are_declared_with_a_parent_and_a_comment() {
    let types = shipped_types();
    assert_eq!(
        types.keys().map(String::as_str).collect::<Vec<_>>(),
        ["application/x-textpack", "text/markdown", "text/plain", "text/x-plantuml"]
    );
    let plantuml = &types["text/x-plantuml"];
    assert_eq!(plantuml.parents, ["text/plain"]);
    assert!(plantuml.has_comment);
    let textpack = &types["application/x-textpack"];
    assert_eq!(textpack.parents, ["application/zip"]);
    assert!(textpack.has_comment);
    // The two freedesktop types only gain globs: nothing of their definition is restated.
    for existing in ["text/markdown", "text/plain"] {
        assert!(types[existing].parents.is_empty() && !types[existing].has_comment, "{existing}");
    }
}

#[test]
fn the_metainfo_provides_the_listed_types_but_plain_text() {
    let metainfo = packaging("me.nettrash.md.metainfo.xml");
    let provided: Vec<&str> = metainfo
        .split("<mediatype>")
        .skip(1)
        .map(|rest| &rest[..rest.find("</mediatype>").expect("an unterminated <mediatype>")])
        .collect();
    assert_eq!(provided, METAINFO_TYPES);
    for mime in provided {
        assert!(DESKTOP_TYPES.contains(&mime), "{mime}");
    }
}

#[test]
fn the_system_globs_are_this_images_shared_mime_info() {
    // The app crate builds only where GTK's development files are — the ubuntu:26.04 image or an
    // Ubuntu 26.04 machine — and shared-mime-info is there with it. A newer one that learns one
    // of md's spellings makes this fail, which is the moment to revisit the shipped XML.
    let path = "/usr/share/mime/packages/freedesktop.org.xml";
    let xml = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{path}: {e} (run in the ubuntu:26.04 image: scripts/dev.sh test)"));
    let system = mime_types(&xml);
    let mut wanted: Vec<String> = document_loader::OPEN_EXTENSIONS.iter().map(|e| format!("*{e}")).collect();
    wanted.extend(["*.dot".to_string(), "*.textbundle".to_string()]);
    for glob in wanted {
        let found: BTreeSet<&str> =
            system.iter().filter(|(_, m)| m.globs.contains(&glob)).map(|(t, _)| t.as_str()).collect();
        let pinned: BTreeSet<&str> = SYSTEM_GLOBS.iter().filter(|(g, _)| *g == glob).map(|(_, t)| *t).collect();
        assert_eq!(found, pinned, "{glob}");
    }
}
