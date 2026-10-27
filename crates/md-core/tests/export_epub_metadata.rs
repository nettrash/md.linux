//! EPUB metadata and cover from front matter (1.6, #2). These are the family's vectors — md.vscode's
//! `test/epub-metadata.test.ts`, and the same ones md for iPhone, Mac and Android carry — so the
//! five package documents agree on which keys count, which values are accepted, and the exact lines
//! they become. Port of md.win `EpubMetadataTests.cs`.

#[path = "common/epub.rs"]
mod epub;

use epub::*;
use md_core::export::epub_export;
use md_core::export::epub_metadata::{self, CoverReference, EpubCover, EpubMetadata, MAX_COVER_BYTES};
use md_core::markdown::{MetadataField, parser};

const PNG: [u8; 12] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
const JPEG: [u8; 6] = [0xFF, 0xD8, 0xFF, 0xE0, 0, 16];
const GIF: &[u8] = b"GIF89a\x01\x00";
const WEBP: &[u8] = b"RIFF\x24\x00\x00\x00WEBPVP8 ";

fn fields(matter: &str) -> Vec<MetadataField> {
    parser::front_matter(&format!("---\n{matter}\n---\n\n# Body\n"))
}

fn lines(matter: &str, cover: Option<&EpubCover>) -> Vec<String> {
    epub_metadata::lines(&epub_metadata::from_front_matter(&fields(matter)), cover)
}

// MARK: - Metadata lines

#[test]
fn no_front_matter_is_the_one_language_line() {
    let expected = vec!["<dc:language>en</dc:language>".to_string()];
    assert_eq!(expected, epub_metadata::lines(&epub_metadata::from_front_matter(&[]), None));
    assert_eq!(expected, epub_metadata::lines(&EpubMetadata::default(), None));
    assert_eq!(expected, lines("title: Only a title", None));
}

#[test]
fn writes_every_key_in_the_family_order() {
    let lines = lines(
        &[
            "title: The Book",
            "author: Ada Lovelace",
            "language: ru",
            "date: 2026-10-10",
            "description: Notes on the \"Engine\" & more",
            "series: Analytical",
            "series_index: 2",
        ]
        .join("\n"),
        epub_metadata::recognise(Some(&PNG)).as_ref(),
    );
    assert_eq!(
        vec![
            "<dc:creator>Ada Lovelace</dc:creator>",
            "<dc:language>ru</dc:language>",
            "<dc:date>2026-10-10</dc:date>",
            "<dc:description>Notes on the &quot;Engine&quot; &amp; more</dc:description>",
            "<meta property=\"belongs-to-collection\" id=\"series\">Analytical</meta>",
            "<meta refines=\"#series\" property=\"collection-type\">series</meta>",
            "<meta refines=\"#series\" property=\"group-position\">2</meta>",
            "<meta name=\"cover\" content=\"cover-image\"/>",
        ],
        lines
    );
}

#[test]
fn matches_keys_case_insensitively_first_non_empty_wins_quotes_stripped() {
    assert_eq!(
        vec!["<dc:creator>Grace Hopper</dc:creator>", "<dc:language>en</dc:language>"],
        lines("Author:\nAUTHOR: \"Grace Hopper\"\nauthor: Someone Else", None)
    );
}

#[test]
fn takes_a_language_tag_else_en() {
    for (value, expected) in [
        ("en", "en"),
        ("pt-BR", "pt-BR"),
        ("zh-Hant-TW", "zh-Hant-TW"),
        ("ast", "ast"),
        ("English", "en"),
        ("e", "en"),
        ("en_US", "en"),
        ("en-", "en"),
        ("ру", "en"),
        ("en-toolongsubtag", "en"),
    ] {
        assert_eq!(
            expected,
            epub_metadata::from_front_matter(&fields(&format!("language: {value}"))).language,
            "{value:?}"
        );
    }
}

#[test]
fn keeps_a_w3c_date() {
    for value in [
        "2026",
        "2026-10",
        "2026-10-10",
        "2026-10-10T09:30Z",
        "2026-10-10T09:30:15+03:00",
        "2024-02-29",
        "2000-02-29",
        "2026-12-31T23:59:59-11:30",
    ] {
        assert_eq!(
            Some(value),
            epub_metadata::from_front_matter(&fields(&format!("date: {value}"))).date.as_deref(),
            "{value:?}"
        );
    }
}

#[test]
fn leaves_anything_else_out() {
    for value in [
        "10/10/2026",
        "October 2026",
        "2026-10-10 09:30",
        "2026-10-10T09:30",
        "26-10-10",
        "\u{FF12}\u{FF10}\u{FF12}\u{FF16}",
        "\u{0662}\u{0660}\u{0662}\u{0666}-10-10", // Arabic-Indic digits: \d would take them
        // Ranges, after the shape matches.
        "2026-13",
        "2026-00",
        "2026-10-00",
        "2026-02-29",
        "1900-02-29",
        "2026-04-31",
        "2026-10-10T24:00Z",
        "2026-10-10T23:60Z",
        "2026-10-10T23:59:60Z",
        "2026-10-10T10:00+24:00",
        "2026-10-10T10:00+05:60",
    ] {
        assert_eq!(None, epub_metadata::from_front_matter(&fields(&format!("date: {value}"))).date, "{value:?}");
    }
}

#[test]
fn date_regex_does_not_match_before_a_trailing_newline() {
    // .NET's $ matches before a final \n; the pattern ends in \z so the ports agree.
    assert!(!epub_metadata::is_w3c_date("2026-10-10\n"));
}

#[test]
fn gives_a_series_its_position_only_for_a_number_and_ignores_an_index_without_a_series() {
    assert!(
        lines("series: S\nseries_index: 2.5", None)
            .contains(&"<meta refines=\"#series\" property=\"group-position\">2.5</meta>".to_string())
    );
    assert_eq!(3, lines("series: S\nseries_index: two", None).len());
    assert_eq!(vec!["<dc:language>en</dc:language>"], lines("series_index: 3", None));
}

#[test]
fn escapes_as_the_rest_of_the_package_document_apostrophe_left_alone() {
    assert_eq!(
        "<dc:creator>O'Brien &lt;ob@example.com&gt;</dc:creator>",
        lines("author: O'Brien <ob@example.com>", None)[0]
    );
}

// MARK: - The cover reference

#[test]
fn decodes_a_base64_data_image_and_refuses_any_other_data() {
    let b64 = base64(&PNG);
    assert_eq!(
        Some(CoverReference::Inline(PNG.to_vec())),
        epub_metadata::reference(Some(&format!("data:image/png;base64,{b64}")))
    );
    assert_eq!(
        Some(CoverReference::Inline(PNG.to_vec())),
        epub_metadata::reference(Some(&format!("DATA:image/png;BASE64,{b64}")))
    );
    assert_eq!(None, epub_metadata::reference(Some("data:image/svg+xml,<svg/>")));
    assert_eq!(None, epub_metadata::reference(Some("data:image/png;base64,")));
    assert_eq!(None, epub_metadata::reference(Some("data:image/png;base64,abc")));
    assert_eq!(None, epub_metadata::reference(Some("data:image/png;base64,ab!d")));
}

#[test]
fn refuses_a_data_image_past_the_cap_before_decoding_it() {
    let mut image = vec![0u8; MAX_COVER_BYTES];
    image[..PNG.len()].copy_from_slice(&PNG);
    let at_the_cap = epub_metadata::reference(Some(&format!("data:image/png;base64,{}", base64(&image))));
    let Some(CoverReference::Inline(data)) = at_the_cap else {
        panic!("a cover at the cap is an inline image");
    };
    assert!(data == image, "the decoded cover is the image");
    let mut past = vec![0u8; MAX_COVER_BYTES + 3];
    past[..PNG.len()].copy_from_slice(&PNG);
    assert_eq!(None, epub_metadata::reference(Some(&format!("data:image/png;base64,{}", base64(&past)))));
}

#[test]
fn fetches_https_only() {
    assert_eq!(
        Some(CoverReference::Https("https://example.com/c.png".to_string())),
        epub_metadata::reference(Some("https://example.com/c.png"))
    );
    assert_eq!(
        Some(CoverReference::Https("HTTPS://example.com/c.png".to_string())),
        epub_metadata::reference(Some("HTTPS://example.com/c.png"))
    );
    assert_eq!(None, epub_metadata::reference(Some("http://example.com/c.png")));
    assert_eq!(None, epub_metadata::reference(Some("ftp://example.com/c.png")));
    assert_eq!(None, epub_metadata::reference(Some("file:///tmp/c.png")));
}

#[test]
fn reads_a_relative_path_from_beside_the_document_and_nothing_absolute() {
    assert_eq!(Some(CoverReference::Local("cover.png".to_string())), epub_metadata::reference(Some("cover.png")));
    assert_eq!(
        Some(CoverReference::Local("images/cover.jpg".to_string())),
        epub_metadata::reference(Some("images/cover.jpg"))
    );
    assert_eq!(Some(CoverReference::Local("../cover.png".to_string())), epub_metadata::reference(Some("../cover.png")));
    for value in ["/tmp/cover.png", "\\\\server\\c.png", "C:\\c.png", "c:/c.png", "#cover", "mailto:x", ""] {
        assert_eq!(None, epub_metadata::reference(Some(value)), "{value:?}");
    }
    assert_eq!(None, epub_metadata::reference(None));
}

// MARK: - Recognising a cover by its bytes

#[test]
fn knows_png_jpeg_gif_and_webp() {
    fn kind(cover: Option<EpubCover>) -> (String, String) {
        let cover = cover.expect("recognised");
        (cover.extension, cover.media_type)
    }
    assert_eq!(("png".into(), "image/png".into()), kind(epub_metadata::recognise(Some(&PNG))));
    assert_eq!(("jpg".into(), "image/jpeg".into()), kind(epub_metadata::recognise(Some(&JPEG))));
    assert_eq!(("gif".into(), "image/gif".into()), kind(epub_metadata::recognise(Some(GIF))));
    assert_eq!(("webp".into(), "image/webp".into()), kind(epub_metadata::recognise(Some(WEBP))));
}

#[test]
fn skips_anything_else_an_empty_file_and_an_oversized_one() {
    assert_eq!(None, epub_metadata::recognise(Some(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>")));
    assert_eq!(None, epub_metadata::recognise(Some(b"RIFF\x24\x00\x00\x00WAVE")));
    assert_eq!(None, epub_metadata::recognise(Some(&[0x89, 0x50, 0x4E, 0x47])));
    assert_eq!(None, epub_metadata::recognise(Some(&[])));
    assert_eq!(None, epub_metadata::recognise(None));
    let mut big = vec![0u8; MAX_COVER_BYTES + 1];
    big[..PNG.len()].copy_from_slice(&PNG);
    assert_eq!(None, epub_metadata::recognise(Some(&big)));
}

// MARK: - The package document

const UNITS: [&str; 1] = ["content.xhtml"];
const IMAGES: [&str; 1] = ["images/unit-000-rich-0.png"];

#[test]
fn is_byte_identical_to_15_without_front_matter() {
    let expected = [
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
        "<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\">",
        "<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">",
        "<dc:identifier id=\"bookid\">urn:uuid:x</dc:identifier>",
        "<dc:title>T</dc:title>",
        "<dc:language>en</dc:language>",
        "<meta property=\"dcterms:modified\">2026-10-10T00:00:00Z</meta>",
        "</metadata>",
        "<manifest>",
        "<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>",
        "<item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>",
        "<item id=\"u0\" href=\"content.xhtml\" media-type=\"application/xhtml+xml\"/>",
        "<item id=\"i0\" href=\"images/unit-000-rich-0.png\" media-type=\"image/png\"/>",
        "</manifest>",
        "<spine>",
        "<itemref idref=\"u0\"/>",
        "</spine>",
        "</package>",
    ]
    .join("\n");
    assert_eq!(expected, epub_export::opf("T", "urn:uuid:x", "2026-10-10T00:00:00Z", &UNITS, &IMAGES, &[], None, None));
    assert_eq!(
        expected,
        epub_export::opf(
            "T",
            "urn:uuid:x",
            "2026-10-10T00:00:00Z",
            &UNITS,
            &IMAGES,
            &[],
            Some(&EpubMetadata::default()),
            None
        )
    );
}

#[test]
fn lists_the_cover_last_in_the_manifest_and_keeps_it_out_of_the_spine() {
    let opf = epub_export::opf(
        "T",
        "urn:uuid:x",
        "2026-10-10T00:00:00Z",
        &UNITS,
        &IMAGES,
        &[],
        Some(&EpubMetadata::default()),
        epub_metadata::recognise(Some(&JPEG)).as_ref(),
    );
    assert!(opf.contains(
        "<item id=\"i0\" href=\"images/unit-000-rich-0.png\" media-type=\"image/png\"/>\n<item id=\"cover-image\" href=\"images/cover.jpg\" media-type=\"image/jpeg\" properties=\"cover-image\"/>\n</manifest>"
    ));
    assert!(opf.contains(
        "<dc:language>en</dc:language>\n<meta name=\"cover\" content=\"cover-image\"/>\n<meta property=\"dcterms:modified\">"
    ));
    assert!(!opf.contains("<itemref idref=\"cover-image\"/>"));
}

#[test]
fn plan_document_reads_the_front_matter_and_a_book_keeps_the_defaults() {
    let source = "---\nauthor: A\nlanguage: de\ncover: images/c.png\n---\n\n# Body\n";
    let plan = epub_export::plan_document(source, "T");
    let metadata = plan.metadata.as_ref().expect("a document's plan carries its metadata");
    assert_eq!(Some("A"), metadata.creator.as_deref());
    assert_eq!("de", metadata.language);
    assert_eq!(Some(CoverReference::Local("images/c.png".to_string())), plan.cover);

    let plain = epub_export::plan_document("# Body\n", "T");
    assert_eq!(Some(EpubMetadata::default()), plain.metadata);
    assert_eq!(None, plain.cover);
}

#[test]
fn packs_the_cover_after_the_snapshots_with_the_metadata_in_the_opf() {
    let source = "---\nauthor: A\nlanguage: de\n---\n\n# Body\n";
    let epub = epub_export::build_document(
        source,
        "T",
        None,
        Some("2026-10-10T00:00:00Z"),
        None,
        epub_metadata::recognise(Some(&PNG)).as_ref(),
    )
    .expect("built");
    let entries = read(&epub);
    let last = entries.last().expect("entries");
    assert_eq!("OEBPS/images/cover.png", last.name);
    assert_eq!(PNG.as_slice(), last.data.as_slice());
    let opf = entry_text(&entries, "OEBPS/content.opf");
    assert!(opf.contains(
        "<dc:creator>A</dc:creator>\n<dc:language>de</dc:language>\n<meta name=\"cover\" content=\"cover-image\"/>"
    ));
}

#[test]
fn no_cover_writes_no_cover_entry() {
    let epub =
        epub_export::build_document("# Body\n", "T", None, Some("2026-10-10T00:00:00Z"), None, None).expect("built");
    assert!(!read(&epub).iter().any(|entry| entry.name.contains("cover")));
}
