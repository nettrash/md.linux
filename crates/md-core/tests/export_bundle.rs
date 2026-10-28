//! The pure pieces of TextBundle / TextPack (macOS mdTests "TextBundle / TextPack" plus the
//! Kotlin-only cases): the info.json shape, reading text.md out of a folder and out of a pack,
//! the export image-ref rewrite, the bundle assembly, and the on-disk containment of asset reads.
//! The dialog plumbing is not here. Port of md.win `TextBundleTests.cs`.

mod export_zip_fixtures;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;

use export_zip_fixtures::*;
use md_core::document::plain_text_codec::{self, TextEncoding};
use md_core::export::text_bundle::{self, Asset, INFO_JSON};
use md_core::export::zip_reader;

fn bundle(files: &[(&str, &[u8])]) -> HashMap<String, Vec<u8>> {
    files.iter().map(|&(name, data)| (name.to_string(), data.to_vec())).collect()
}

fn text_of(decoded: Option<plain_text_codec::DecodedText>) -> Option<String> {
    decoded.map(|d| d.text)
}

fn names(assets: &[Asset]) -> Vec<&str> {
    assets.iter().map(|asset| asset.name.as_str()).collect()
}

fn one(_: &str) -> Option<Vec<u8>> {
    Some(vec![1])
}

// MARK: info.json

#[test]
fn info_json_has_the_text_bundle_shape() {
    let root: serde_json::Value = serde_json::from_str(INFO_JSON).expect("valid JSON");
    assert_eq!(root["version"].as_i64(), Some(2));
    assert_eq!(root["type"].as_str(), Some("net.daringfireball.markdown"));
    assert_eq!(root["transient"].as_bool(), Some(false));
    // Canonical bytes: stable key order, two-space indent, no trailing newline.
    assert_eq!(
        INFO_JSON,
        "{\n  \"version\": 2,\n  \"type\": \"net.daringfireball.markdown\",\n  \"transient\": false\n}"
    );
    assert_eq!(INFO_JSON.len(), 81);
}

// MARK: Import — a .textbundle folder

#[test]
fn import_reads_text_markdown_with_encoding() {
    // A `text.markdown` (the spec's other name) in a legacy encoding must decode with the right
    // encoding and round-trip byte for byte.
    let cyrillic = cp1251("Привет");
    let result = text_bundle::text_from_bundle(&bundle(&[("text.markdown", &cyrillic)])).expect("a text file");
    assert_eq!(result.text, "Привет");
    assert_eq!(result.encoding, TextEncoding::WindowsCp1251);
    assert_eq!(plain_text_codec::encode(&result.text, result.encoding).data, cyrillic);
}

#[test]
fn import_prefers_text_md() {
    let files = bundle(&[("text.markdown", b"markdown\n"), ("text.md", b"md\n")]);
    assert_eq!(text_of(text_bundle::text_from_bundle(&files)), Some("md\n".to_string()));
}

#[test]
fn import_nil_without_a_text_file() {
    assert_eq!(text_bundle::text_from_bundle(&bundle(&[("info.json", b"{}")])), None);
    assert_eq!(text_bundle::text_from_bundle(&HashMap::new()), None);
}

#[test]
fn import_falls_back_to_any_text_dot_file_deterministically() {
    // The spec names the file `text` with an extension matching the declared type; Swift's "any
    // text.*" pick is dictionary order, made ordinal-smallest here.
    let mut files = bundle(&[("text.txt", b"txt\n"), ("info.json", b"{}"), ("text.mdown", b"mdown\n")]);
    assert_eq!(text_of(text_bundle::text_from_bundle(&files)), Some("mdown\n".to_string()));
    // `text.markdown` still beats an arbitrary `text.*`.
    files.insert("text.markdown".to_string(), utf8("markdown\n"));
    assert_eq!(text_of(text_bundle::text_from_bundle(&files)), Some("markdown\n".to_string()));
}

#[test]
fn import_of_an_empty_text_file_is_empty_not_null() {
    // FileWrapper hands back empty Data for an empty file, and the codec decodes it.
    let result = text_bundle::text_from_bundle(&bundle(&[("text.md", b"")])).expect("decodes");
    assert_eq!(result.text, "");
    assert_eq!(result.encoding, TextEncoding::Utf8);
}

#[test]
fn import_reads_a_folder_on_disk() {
    let root = TempFolder::new("md-textbundle-tests");
    let notes = root.path().join("Notes.textbundle");
    fs::create_dir_all(notes.join("assets")).unwrap();
    fs::write(notes.join("text.md"), "# From disk\n").unwrap();
    fs::write(notes.join("info.json"), INFO_JSON).unwrap();
    let result = text_bundle::text_from_bundle_directory(&notes);
    assert_eq!(result.as_ref().map(|d| d.text.as_str()), Some("# From disk\n"));
    assert_eq!(result.map(|d| d.encoding), Some(TextEncoding::Utf8));
    // A missing folder, or one without a text file, is None — not an error.
    assert_eq!(text_bundle::text_from_bundle_directory(&root.path().join("Missing.textbundle")), None);
    fs::create_dir(root.path().join("Empty.textbundle")).unwrap();
    assert_eq!(text_bundle::text_from_bundle_directory(&root.path().join("Empty.textbundle")), None);
}

#[test]
fn import_from_disk_skips_folders_named_like_text_files() {
    // Beyond md.win's suite, its Directory.GetFiles rule made explicit: only files count — a
    // folder called text.md is not the text, a plain file text.txt is.
    let root = TempFolder::new("md-textbundle-tests");
    let notes = root.path().join("Odd.textbundle");
    fs::create_dir_all(notes.join("text.md")).unwrap();
    fs::write(notes.join("text.txt"), "txt\n").unwrap();
    assert_eq!(text_of(text_bundle::text_from_bundle_directory(&notes)), Some("txt\n".to_string()));
    // A bundle path that is a file, not a folder, is None.
    assert_eq!(text_bundle::text_from_bundle_directory(&notes.join("text.txt")), None);
}

// MARK: Import — a .textpack

#[test]
fn pack_import_reads_stored_zip() {
    // A pack wraps a `.textbundle` folder, so text.md is nested under it.
    let zip = stored(&[("Doc.textbundle/info.json", INFO_JSON.as_bytes()), ("Doc.textbundle/text.md", b"# Packed\n")]);
    let result = text_bundle::text_from_pack(&zip);
    assert_eq!(result.as_ref().map(|d| d.text.as_str()), Some("# Packed\n"));
    assert_eq!(result.map(|d| d.encoding), Some(TextEncoding::Utf8));
}

#[test]
fn pack_import_reads_deflated_zip() {
    let payload = "# Deflated\n\nSome longer body text to compress well.\n";
    let archive = deflated("Doc.textbundle/text.md", payload.as_bytes(), None, 8);
    assert_eq!(text_of(text_bundle::text_from_pack(&archive)), Some(payload.to_string()));
}

#[test]
fn pack_import_rejects_non_zip() {
    assert_eq!(text_bundle::text_from_pack(b"plain text, not a pack"), None);
    assert_eq!(text_bundle::text_from_pack(&[]), None);
}

#[test]
fn pack_import_prefers_text_md() {
    // text.md wins even when text.markdown is the earlier entry.
    let zip = stored(&[("B.textbundle/text.markdown", b"markdown\n"), ("B.textbundle/text.md", b"md\n")]);
    assert_eq!(text_of(text_bundle::text_from_pack(&zip)), Some("md\n".to_string()));
}

#[test]
fn pack_import_decodes_legacy_encoding() {
    // The pack-path counterpart of the folder-import encoding test.
    let result = text_bundle::text_from_pack(&stored(&[("B.textbundle/text.md", &cp1251("Привет"))]));
    assert_eq!(result.as_ref().map(|d| d.text.as_str()), Some("Привет"));
    assert_eq!(result.map(|d| d.encoding), Some(TextEncoding::WindowsCp1251));
}

#[test]
fn pack_import_nil_without_a_text_file_or_with_an_empty_one() {
    assert_eq!(text_bundle::text_from_pack(&stored(&[("B.textbundle/info.json", b"{}")])), None);
    // Swift's `guard let data = entry?.data, !data.isEmpty` — an empty text entry is None on the
    // pack path (the folder path decodes it to ""); Kotlin returns "" here.
    assert_eq!(text_bundle::text_from_pack(&stored(&[("B.textbundle/text.md", b"")])), None);
}

#[test]
fn pack_import_matches_the_text_file_by_its_last_component_only() {
    // Any nesting depth, and a `text.*` that is not the whole name does not count.
    assert_eq!(text_of(text_bundle::text_from_pack(&stored(&[("a/b/c/text.md", b"deep\n")]))), Some("deep\n".into()));
    assert_eq!(text_bundle::text_from_pack(&stored(&[("Doc.textbundle/context.md", b"no\n")])), None);
}

#[test]
fn looks_like_pack_by_name_or_zip_magic() {
    let zip = stored(&[("Doc.textbundle/text.md", b"hi\n")]);
    // The 4-byte local-file-header magic identifies an unnamed pack…
    assert!(text_bundle::looks_like_pack("whatever", &zip));
    // …and so does the extension, whatever the bytes.
    assert!(text_bundle::looks_like_pack("Notes.textpack", b"not a zip"));
    assert!(text_bundle::looks_like_pack("Notes.TEXTPACK", b"not a zip"));
    // A plain Markdown file — even one whose text starts with the two letters "PK" — is NOT
    // mistaken for a pack (only PK + 03 04 is).
    assert!(!text_bundle::looks_like_pack("notes.md", b"# Notes\n"));
    assert!(!text_bundle::looks_like_pack("pk.md", b"PKZIP is a program\n"));
    assert!(!text_bundle::looks_like_pack("pk.md", &[0x50, 0x4B]));
}

// MARK: Export — the image-ref rewrite

#[test]
fn export_copies_found_image_and_leaves_the_rest_untouched() {
    // A findable local ref is copied to assets/ and rewritten; an unfindable local ref and a
    // remote URL are left exactly as written.
    let png = vec![0x89, 0x50, 0x4E, 0x47];
    let source = "![cat](photo.png) ![dog](missing.png) ![web](https://e/x.png)";
    let result = text_bundle::export_rewriting(source, |path| (path == "photo.png").then(|| png.clone()));
    assert!(result.text.contains("![cat](assets/photo.png)"));
    assert!(result.text.contains("![dog](missing.png)"));
    assert!(result.text.contains("![web](https://e/x.png)"));
    assert_eq!(result.assets, [Asset::new("photo.png", png)]);
}

#[test]
fn export_keeps_image_title_when_rewriting() {
    // Only the URL is rewritten; a following "title" is preserved — either quote.
    let result = text_bundle::export_rewriting("![alt](pic.png \"A cat\")", one);
    assert!(result.text.contains("![alt](assets/pic.png \"A cat\")"));
    let single = text_bundle::export_rewriting("![alt](pic.png 'A cat')", one);
    assert_eq!(single.text, "![alt](assets/pic.png 'A cat')");
    // The separator is any ICU whitespace run, a tab included.
    let tabbed = text_bundle::export_rewriting("![alt](pic.png\t\"A cat\")", one);
    assert_eq!(tabbed.text, "![alt](assets/pic.png\t\"A cat\")");
}

#[test]
fn export_dedupes_asset_names_but_reuses_one_path_once() {
    // Two distinct paths sharing a file name get distinct assets; the same path used again reuses
    // its asset (copied once, both refs rewritten).
    let source = "![a](a/logo.png) ![b](b/logo.png) ![c](a/logo.png)";
    let result = text_bundle::export_rewriting(source, |_| Some(vec![7]));
    assert_eq!(names(&result.assets), ["logo.png", "logo-2.png"]);
    assert!(result.text.contains("![a](assets/logo.png)"));
    assert!(result.text.contains("![b](assets/logo-2.png)"));
    assert!(result.text.contains("![c](assets/logo.png)"));
    // The loader is consulted once per distinct path.
    let calls = RefCell::new(Vec::new());
    text_bundle::export_rewriting(source, |path| {
        calls.borrow_mut().push(path.to_string());
        Some(vec![7])
    });
    assert_eq!(calls.into_inner(), ["a/logo.png", "b/logo.png"]);
}

#[test]
fn export_disambiguates_dotfiles_and_extensionless_names() {
    // The extension split is past the FIRST character, so a dotfile keeps its whole name as the
    // stem (`.keep` → `.keep-2`, not `-2.keep`), and a name with no dot gets the counter
    // appended. A third collision counts on.
    let dotfiles = text_bundle::export_rewriting("![a](x/.keep) ![b](y/.keep)", one);
    assert_eq!(names(&dotfiles.assets), [".keep", ".keep-2"]);
    let plain = text_bundle::export_rewriting("![a](x/README) ![b](y/README) ![c](z/README)", one);
    assert_eq!(names(&plain.assets), ["README", "README-2", "README-3"]);
    let dotted = text_bundle::export_rewriting("![a](x/a.tar.gz) ![b](y/a.tar.gz)", one);
    assert_eq!(names(&dotted.assets), ["a.tar.gz", "a.tar-2.gz"]);
}

#[test]
fn export_dedup_is_case_insensitive_and_locale_independent() {
    // `Logo.PNG` and `logo.png` collide (the fold is case-insensitive), and the fold is
    // locale-independent — md.win runs this under a Turkish culture; Rust has no ambient culture,
    // and the fold is .NET's invariant one (`text::unicode::lower_invariant`).
    let result =
        text_bundle::export_rewriting("![a](x/Logo.PNG) ![b](y/logo.png) ![c](z/ICON.PNG) ![d](w/icon.png)", one);
    assert_eq!(names(&result.assets), ["Logo.PNG", "logo-2.png", "ICON.PNG", "icon-2.png"]);
}

#[test]
fn export_destination_stops_at_every_icu_whitespace() {
    // ICU's \s (measured on macOS): a destination may not contain NBSP, VT or NEL any more than a
    // space, so such a ref never matches and stays exactly as written.
    for gap in ['\u{00A0}', '\u{000B}', '\u{0085}', '\u{2003}', '\u{2028}', ' '] {
        let source = format!("![a](pic{gap}x.png)");
        let result = text_bundle::export_rewriting(&source, one);
        assert_eq!(result.text, source);
        assert!(result.assets.is_empty());
    }
    // U+200B and U+FEFF are not whitespace to ICU: the ref matches and is rewritten.
    for not_gap in ['\u{200B}', '\u{FEFF}'] {
        let result = text_bundle::export_rewriting(&format!("![a](pic{not_gap}x.png)"), one);
        assert_eq!(result.text, format!("![a](assets/pic{not_gap}x.png)"));
    }
}

#[test]
fn export_leaves_the_source_alone_when_nothing_resolves() {
    let source = "# Title\n\n![a](x.png) and `![b](y.png)`\n";
    let result = text_bundle::export_rewriting(source, |_| None);
    assert_eq!(result.text, source);
    assert!(result.assets.is_empty());
    // A ref whose last component is empty (a folder) is skipped even when the loader answers —
    // there is no file name to copy it under.
    let folder = text_bundle::export_rewriting("![a](dir/)", one);
    assert_eq!(folder.text, "![a](dir/)");
    assert!(folder.assets.is_empty());
}

#[test]
fn export_rewrites_every_occurrence_in_document_order() {
    // Several edits in one document: the splice must keep the untouched text between them intact
    // (the Swift applies its UTF-16 ranges back to front).
    let source = "Начало ![a](a.png) середина ![b](sub/b.png \"t\") конец ![a](a.png).";
    let result = text_bundle::export_rewriting(source, |path| Some(utf8(path)));
    assert_eq!(result.text, "Начало ![a](assets/a.png) середина ![b](assets/b.png \"t\") конец ![a](assets/a.png).");
    assert_eq!(result.assets, [Asset::new("a.png", utf8("a.png")), Asset::new("b.png", utf8("sub/b.png"))]);
}

#[test]
fn local_relative_reference_classification() {
    assert!(text_bundle::is_local_relative_reference("photo.png"));
    assert!(text_bundle::is_local_relative_reference("images/photo.png"));
    assert!(!text_bundle::is_local_relative_reference("https://x/y.png"));
    assert!(!text_bundle::is_local_relative_reference("http://x/y.png"));
    assert!(!text_bundle::is_local_relative_reference("data:image/png;base64,AAAA"));
    assert!(!text_bundle::is_local_relative_reference("/abs/photo.png"));
    assert!(!text_bundle::is_local_relative_reference("#anchor"));
    assert!(!text_bundle::is_local_relative_reference(""));
    assert!(!text_bundle::is_local_relative_reference("mailto:me@example.com"));
    assert!(!text_bundle::is_local_relative_reference("file:///x/y.png"));
}

#[test]
fn reference_classification_is_code_unit_exact_around_marks() {
    // A relative path carrying a combining mark is still local — the mark is a scalar of its own
    // and never manufactures a "://".
    assert!(text_bundle::is_local_relative_reference("cafe\u{0301}/photo.png"));
    assert!(text_bundle::is_local_relative_reference("café/photo.png"));
    // A real scheme is still caught when a mark sits elsewhere in the URL.
    assert!(!text_bundle::is_local_relative_reference("https://ho\u{0301}st/x.png"));
    // A ':' wearing a combining mark is NOT the "://" delimiter.
    assert!(text_bundle::is_local_relative_reference("weird:\u{0301}//name.png"));
}

#[test]
fn reference_classification_folds_schemes_ascii_only() {
    // md.win runs this under tr-TR; there is no ambient culture to set in Rust.
    assert!(!text_bundle::is_local_relative_reference("DATA:image/png;base64,AAAA"));
    assert!(!text_bundle::is_local_relative_reference("MailTo:me@example.com"));
    // Swift's and Kotlin's full case mapping lower İ to "i" + U+0307, so "MAİLTO:" is not the
    // mailto scheme on either sibling; .NET's ToLowerInvariant would have said otherwise.
    assert!(text_bundle::is_local_relative_reference("MA\u{0130}LTO:x.png"));
    assert!(text_bundle::is_local_relative_reference("dat\u{0130}:x.png"));
}

// MARK: Export — the bundle assembly

#[test]
fn bundle_wrapper_holds_text_info_and_assets() {
    let assets = vec![Asset::new("p.png", [1u8, 2])];
    let wrapper = text_bundle::bundle_wrapper("# Hi\n", assets.clone());
    assert_eq!(wrapper.text_bytes(), b"# Hi\n");
    assert_eq!(wrapper.info_json_bytes(), INFO_JSON.as_bytes());
    assert_eq!(wrapper.assets(), assets);
    // UTF-8 without a BOM, whatever the document's own encoding was.
    assert_eq!(text_bundle::bundle_wrapper("П", vec![]).text_bytes(), [0xD0, 0x9F]);
}

#[test]
fn bundle_wrapper_with_no_images_still_carries_empty_assets() {
    let wrapper = text_bundle::bundle_wrapper("plain\n", vec![]);
    assert!(wrapper.assets().is_empty());
    let root = TempFolder::new("md-textbundle-tests");
    let target = root.path().join("Plain.textbundle");
    wrapper.write(&target).unwrap();
    assert!(target.join("assets").is_dir());
    assert_eq!(fs::read_dir(target.join("assets")).unwrap().count(), 0);
    assert_eq!(fs::read(target.join("text.md")).unwrap(), b"plain\n");
    assert_eq!(utf8_text(&fs::read(target.join("info.json")).unwrap()), INFO_JSON);
}

#[test]
fn write_replaces_an_existing_bundle_whole() {
    // FileWrapper's atomic write swaps the whole package: a stale asset from a previous export
    // must not linger beside the new text.
    let root = TempFolder::new("md-textbundle-tests");
    let target = root.path().join("Doc.textbundle");
    text_bundle::bundle_wrapper("v1\n", vec![Asset::new("old.png", [1u8])]).write(&target).unwrap();
    assert!(target.join("assets").join("old.png").is_file());
    text_bundle::bundle_wrapper("v2\n", vec![Asset::new("new.png", [2u8])]).write(&target).unwrap();
    assert!(!target.join("assets").join("old.png").exists());
    assert_eq!(fs::read(target.join("assets").join("new.png")).unwrap(), [2]);
    assert_eq!(utf8_text(&fs::read(target.join("text.md")).unwrap()), "v2\n");
    // The folder is readable back as a bundle, and nothing was left in staging.
    assert_eq!(text_of(text_bundle::text_from_bundle_directory(&target)), Some("v2\n".to_string()));
    let left: Vec<String> =
        fs::read_dir(root.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(left, ["Doc.textbundle"]);
}

#[cfg(unix)]
#[test]
fn write_replaces_a_link_at_the_destination_and_never_follows_it() {
    // Beyond md.win's suite: a symlink where the bundle goes is replaced by the bundle; the
    // folder it pointed at keeps its contents.
    let root = TempFolder::new("md-textbundle-tests");
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(elsewhere.join("keep.txt"), "keep").unwrap();
    let target = root.path().join("Doc.textbundle");
    std::os::unix::fs::symlink(&elsewhere, &target).unwrap();
    text_bundle::bundle_wrapper("v\n", vec![]).write(&target).unwrap();
    assert!(!fs::symlink_metadata(&target).unwrap().file_type().is_symlink());
    assert!(target.join("text.md").is_file());
    assert_eq!(fs::read_to_string(elsewhere.join("keep.txt")).unwrap(), "keep");
}

#[test]
fn write_refuses_an_asset_name_that_is_not_a_file_name() {
    // Beyond md.win: export only ever names an asset by a last path component, but a hand-built
    // wrapper must not write outside its assets/ folder. Nothing is created on refusal.
    let root = TempFolder::new("md-textbundle-tests");
    let target = root.path().join("Doc.textbundle");
    for name in ["../escape.png", "a/b.png", "..", ".", "", "nul\0.png"] {
        let error = text_bundle::bundle_wrapper("x\n", vec![Asset::new(name, [1u8])]).write(&target).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{name:?}");
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn pack_entries_nest_under_the_bundle_folder() {
    // Android's bundleEntries: info.json, text.md, then the assets — no stray directory marker
    // when there are real assets…
    let assets = vec![Asset::new("p.png", [1u8, 2])];
    let entries: HashMap<String, Vec<u8>> = text_bundle::bundle_wrapper("# Hi\n", assets)
        .pack_entries("Doc")
        .into_iter()
        .map(|entry| (entry.name, entry.data))
        .collect();
    assert_eq!(utf8_text(&entries["Doc.textbundle/text.md"]), "# Hi\n");
    assert!(entries.contains_key("Doc.textbundle/info.json"));
    assert_eq!(entries["Doc.textbundle/assets/p.png"], [1, 2]);
    assert!(!entries.contains_key("Doc.textbundle/assets/"));
    // …and an explicit empty assets/ entry when there are none.
    let empty: Vec<String> =
        text_bundle::bundle_wrapper("plain\n", vec![]).pack_entries("Doc").into_iter().map(|e| e.name).collect();
    assert_eq!(empty, ["Doc.textbundle/info.json", "Doc.textbundle/text.md", "Doc.textbundle/assets/"]);
}

#[test]
fn pack_round_trips_text_and_assets() {
    // The whole export→import loop through the app's own container: text via the importer, the
    // copied asset byte-exact via the reader.
    let png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A];
    let rewrite = text_bundle::export_rewriting("![c](photo.png)", |_| Some(png.clone()));
    assert_eq!(rewrite.text, "![c](assets/photo.png)");
    let pack = text_bundle::bundle_wrapper(&rewrite.text, rewrite.assets.clone()).to_text_pack("Doc").unwrap();
    assert!(text_bundle::looks_like_pack("anything", &pack));
    assert_eq!(text_of(text_bundle::text_from_pack(&pack)), Some(rewrite.text.clone()));
    let entries = zip_reader::entries(&pack, None);
    assert_eq!(named(entries.as_deref(), "Doc.textbundle/assets/photo.png").map(|e| &e.data), Some(&png));
    // A pack with no assets still lists the empty folder for a foreign reader.
    let bare = text_bundle::bundle_wrapper("plain\n", vec![]).to_text_pack("Doc").unwrap();
    assert_foreign_readers_accept(
        &bare,
        &["Doc.textbundle/info.json", "Doc.textbundle/text.md", "Doc.textbundle/assets/"],
    );
}

#[test]
fn asset_equality_is_by_name_and_bytes() {
    assert_eq!(Asset::new("p.png", [1u8]), Asset::new("p.png", [1u8]));
    assert_ne!(Asset::new("p.png", [1u8]), Asset::new("p.png", [2u8]));
    assert_ne!(Asset::new("p.png", [1u8]), Asset::new("P.png", [1u8]));
}

// MARK: Export — reading a referenced image with containment

#[test]
fn read_asset_finds_a_file_beside_the_document_and_nothing_else() {
    let root = TempFolder::new("md-textbundle-tests");
    let docs = root.path().join("docs");
    fs::create_dir_all(docs.join("images")).unwrap();
    let document = docs.join("notes.md");
    fs::write(&document, "# n\n").unwrap();
    fs::write(docs.join("photo.png"), [1, 2, 3]).unwrap();
    fs::write(docs.join("images").join("deep.png"), [4]).unwrap();
    fs::write(root.path().join("outside.png"), [9]).unwrap();
    let read = |relative: &str| text_bundle::read_asset(relative, Some(&document));

    assert_eq!(read("photo.png"), Some(vec![1, 2, 3]));
    assert_eq!(read("images/deep.png"), Some(vec![4]));
    assert_eq!(read("images/../photo.png"), Some(vec![1, 2, 3]));
    assert_eq!(read("./photo.png"), Some(vec![1, 2, 3]));
    assert_eq!(read("missing.png"), None);
    // A folder is not an image, and neither is the folder itself.
    assert_eq!(read("images"), None);
    assert_eq!(read(""), None);
    assert_eq!(read("."), None);
    // Climbing out of the document's folder is "not found", never a read.
    assert_eq!(read("../outside.png"), None);
    assert_eq!(read("images/../../outside.png"), None);
    // A rooted path (md.win: the Windows drive-letter case the classifier lets through).
    assert_eq!(read(root.path().join("outside.png").to_str().unwrap()), None);
    // An empty document path names no folder (.NET's GetFullPath refuses it outright).
    assert_eq!(text_bundle::read_asset("photo.png", Some(std::path::Path::new(""))), None);
    // An unsaved document has no folder to look beside.
    assert_eq!(text_bundle::read_asset("photo.png", None), None);
}

#[cfg(unix)]
#[test]
fn read_asset_resolves_symlinks_before_the_containment_check() {
    // A symlink beside the document, named like an image, could otherwise point anywhere on
    // disk and pass the prefix test — both a file link and a folder link. (md.win skips this on
    // a Windows host without the symlink privilege; Linux always has it.)
    use std::os::unix::fs::symlink;
    let root = TempFolder::new("md-textbundle-tests");
    let docs = root.path().join("docs");
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir(&docs).unwrap();
    fs::create_dir(&elsewhere).unwrap();
    let document = docs.join("notes.md");
    fs::write(&document, "# n\n").unwrap();
    fs::write(elsewhere.join("secret.png"), [0x53]).unwrap();
    fs::write(docs.join("real.png"), [0x52]).unwrap();
    symlink(elsewhere.join("secret.png"), docs.join("link.png")).unwrap();
    symlink(&elsewhere, docs.join("imgs")).unwrap();
    symlink(docs.join("real.png"), docs.join("same.png")).unwrap();
    // Relative links resolve against the link's own folder.
    symlink("../elsewhere/secret.png", docs.join("relative.png")).unwrap();
    symlink("real.png", docs.join("near.png")).unwrap();
    let read = |relative: &str| text_bundle::read_asset(relative, Some(&document));

    assert_eq!(read("link.png"), None);
    assert_eq!(read("imgs/secret.png"), None);
    assert_eq!(read("relative.png"), None);
    // A link that stays inside the folder is fine, and is read through.
    assert_eq!(read("same.png"), Some(vec![0x52]));
    assert_eq!(read("near.png"), Some(vec![0x52]));
    // `imgs/..` folds lexically (standardizedFileURL), so it names docs/real.png — inside.
    assert_eq!(read("imgs/../real.png"), Some(vec![0x52]));
    // A loop of links is "not found", never a hang.
    symlink(docs.join("loop-b"), docs.join("loop-a")).unwrap();
    symlink(docs.join("loop-a"), docs.join("loop-b")).unwrap();
    assert_eq!(read("loop-a"), None);
    // The document's own folder may be reached through a link: both sides resolve.
    let linked_docs = root.path().join("linked-docs");
    symlink(&docs, &linked_docs).unwrap();
    assert_eq!(text_bundle::read_asset("real.png", Some(&linked_docs.join("notes.md"))), Some(vec![0x52]));
    assert_eq!(text_bundle::read_asset("link.png", Some(&linked_docs.join("notes.md"))), None);
}
