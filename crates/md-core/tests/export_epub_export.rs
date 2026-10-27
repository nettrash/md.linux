//! The EPUB 3 package: the container shape, the package document, the navigation, the XHTML fixer,
//! the rich-element swap, the title-derived identifier and the stored archive. Port of md.win
//! `EpubExportTests.cs` — itself the macOS `EPUBExport` tests in `mdTests.swift` plus the
//! invariants only md.win states: the snapshot count check, and every XML file in a finished
//! archive really parsing (here through `tests/common/xml.rs`, the stand-in for `XmlReader`).

#[path = "common/epub.rs"]
mod epub;
#[path = "common/xml.rs"]
mod xml;

use std::collections::{HashMap, HashSet};

use epub::*;
use md_core::export::epub_export::{self, EpubError, NavEntry, RichSnapshot};
use md_core::export::{ZipEntry, ZipWriter};
use md_core::markdown::{MetadataField, html, parser};

const MODIFIED: &str = "2026-07-24T00:00:00Z";

/// The single-document entries for `source`, assembled exactly as the export does for a document
/// with no rich blocks: the body is `xhtml_body(body_html(the export HTML))` — the same form a book
/// article is wrapped in — and the outline is read from the same source, so nav slugs and content
/// ids come from the real code paths rather than from fixtures. (Swift's test-private
/// `documentEpubEntries`.)
fn document_epub_entries(source: &str, title: &str) -> Vec<ZipEntry> {
    let html = html::document(source, title, false, true);
    let body = epub_export::xhtml_body(epub_export::body_html(&html));
    epub_export::document_entries(title, &body, &[], &parser::outline(source), MODIFIED, None, None, None)
}

fn build_document(source: &str, title: &str, snapshots: Option<&[RichSnapshot]>) -> Result<Vec<u8>, EpubError> {
    epub_export::build_document(source, title, snapshots, Some(MODIFIED), None, None)
}

// MARK: - The XHTML fixer

#[test]
fn xhtml_fixer_makes_well_formed_markup() {
    let fixed_up = epub_export::xhtml_body(
        &[
            "<p>a<br>b</p>",
            "<hr>",
            "<img src=\"x.png\" alt=\"pic\">",
            "<div class=\"md-list\"><span class=\"md-marker\">&bull;</span></div>",
            "<script type=\"module\" src=\"rich/md-init.js\"></script>",
        ]
        .join("\n"),
    );
    assert!(fixed_up.contains("<br/>"));
    assert!(fixed_up.contains("<hr/>"));
    assert!(fixed_up.contains("<img src=\"x.png\" alt=\"pic\"/>"));
    // Named entities become numeric — XML predefines only amp/lt/gt/quot/apos.
    assert!(fixed_up.contains("&#8226;"));
    assert!(!fixed_up.contains("&bull;"));
    // Engine references are stripped.
    assert!(!fixed_up.contains("<script"));
}

#[test]
fn xhtml_fixer_is_idempotent_and_keeps_angle_brackets_inside_attributes() {
    // Already self-closed voids survive a second pass unchanged.
    assert_eq!("<br/>", epub_export::xhtml_body("<br/>"));
    assert_eq!("<img src=\"a.png\"/>", epub_export::xhtml_body("<img src=\"a.png\"/>"));
    assert_eq!("<hr/>", epub_export::xhtml_body(&epub_export::xhtml_body("<hr>")));
    // Group 2 walks quoted values as units, so a `>` inside an attribute cannot end the tag
    // early — the whole alt text stays on the one element.
    assert_eq!("<img src=\"x.png\" alt=\"a > b\"/>", epub_export::xhtml_body("<img src=\"x.png\" alt=\"a > b\">"));
    // Trailing whitespace before the slash is swallowed, as in the Swift.
    assert_eq!("<br/>", epub_export::xhtml_body("<br  >"));
}

#[test]
fn xhtml_fixer_strips_whole_script_elements_including_their_body() {
    assert_eq!(
        "<p>a</p>\n<p>b</p>",
        epub_export::xhtml_body("<p>a</p>\n<script>var x = 1 < 2;\nvar y = 3;</script><p>b</p>")
    );
}

// MARK: - Rich elements

#[test]
fn rich_elements_become_images() {
    let html = "<p>x <span class=\"md-mathi\">a^2</span> y</p>\n<pre class=\"mermaid\">graph TD</pre>";
    let output = epub_export::replacing_rich_elements(
        html,
        &["<img src=\"images/m0.png\" alt=\"formula\"/>", "<img src=\"images/m1.png\" alt=\"diagram\"/>"],
    );
    assert_eq!(
        "<p>x <img src=\"images/m0.png\" alt=\"formula\"/> y</p>\n<img src=\"images/m1.png\" alt=\"diagram\"/>",
        output
    );
}

#[test]
fn rich_element_scan_reports_kinds_and_spans_in_document_order() {
    let html = "<p>x <span class=\"md-mathi\">a^2</span> y</p>\n<pre class=\"mermaid\">graph TD</pre>";
    let elements = epub_export::rich_elements(html);
    assert_eq!(2, elements.len());
    assert!(elements[0].is_math);
    assert_eq!("formula", elements[0].alt());
    assert_eq!("<span class=\"md-mathi\">a^2</span>", &html[elements[0].start..elements[0].end()]);
    assert!(!elements[1].is_math);
    assert_eq!("diagram", elements[1].alt());
    assert_eq!("<pre class=\"mermaid\">graph TD</pre>", &html[elements[1].start..elements[1].end()]);
}

#[test]
fn rich_element_scan_covers_all_six_containers_and_never_the_plot() {
    let html = [
        "<span class=\"md-mathi\">i</span>",
        "<span class=\"md-mathd\">d</span>",
        "<div class=\"md-mathd\">D</div>",
        "<pre class=\"mermaid\">m</pre>",
        "<div class=\"plantuml\">p</div>",
        "<div class=\"graphviz\" data-engine=\"neato\">g</div>",
        "<div class=\"plot\"><svg viewBox=\"0 0 1 1\"></svg></div>",
    ]
    .join("\n");
    let elements = epub_export::rich_elements(&html);
    assert_eq!(6, elements.len());
    assert_eq!(vec![true, true, true, false, false, false], elements.iter().map(|e| e.is_math).collect::<Vec<_>>());
    // The plot is deliberately not a rich container: it is already an <svg>, so it travels into
    // the book as vector rather than as a photograph of itself.
    let spans: String = elements.iter().map(|e| &html[e.start..e.end()]).collect();
    assert!(!spans.contains("plot"));
    assert!(!epub_export::contains_rich_content("<div class=\"plot\"><svg/></div>"));
}

#[test]
fn empty_tag_list_leaves_the_markup_alone_and_a_short_list_pairs_up_to_the_shorter() {
    let html = "<p><span class=\"md-mathi\">a</span><span class=\"md-mathi\">b</span></p>";
    assert_eq!(html, epub_export::replacing_rich_elements(html, &[]));
    // Swift pairs with `zip`: a missing snapshot leaves the trailing element as source. md.win
    // keeps that behaviour here and refuses the mismatch one level up, in `entries`.
    assert_eq!("<p>TAG<span class=\"md-mathi\">b</span></p>", epub_export::replacing_rich_elements(html, &["TAG"]));
}

#[test]
fn graphviz_containers_are_snapshotted_whatever_their_engine() {
    // A ```dot block in a book article must reach the EPUB as a PNG, not as its DOT source. The
    // container names its layout program in the tag, so both the gate that decides whether the
    // renderer runs at all and the replacement scan have to tolerate that extra attribute.
    let source = "```dot\ndigraph { a -> b }\n```\n\n```twopi\ngraph { a -- b }\n```";
    let document = html::document(source, "t", false, true);
    let body = epub_export::body_html(&document);
    assert!(body.contains("data-engine=\"twopi\""));
    assert!(epub_export::contains_rich_content(body), "a DOT diagram must trigger the snapshot pass");

    let output = epub_export::replacing_rich_elements(
        body,
        &["<img src=\"images/g0.png\" alt=\"diagram\"/>", "<img src=\"images/g1.png\" alt=\"diagram\"/>"],
    );
    assert!(output.contains("images/g0.png"));
    assert!(output.contains("images/g1.png"));
    // Both containers are gone — no leftover open tag, no raw DOT.
    assert!(!output.contains("class=\"graphviz\""));
    assert!(!output.contains("digraph"));
    assert!(!output.contains("data-engine"));
}

#[test]
fn every_graphviz_alias_reaches_the_same_replacement() {
    // The eight layout programs the writer names, through all ten fence aliases.
    for (alias, _) in html::GRAPHVIZ_ENGINES {
        let document = html::document(&format!("```{alias}\ndigraph {{ a -> b }}\n```"), "t", false, true);
        let body = epub_export::body_html(&document);
        assert!(epub_export::contains_rich_content(body), "{alias}");
        assert_eq!(1, epub_export::rich_elements(body).len(), "{alias}");
        let replaced = epub_export::replacing_rich_elements(body, &["IMG"]);
        assert!(replaced.starts_with("IMG"), "{alias}");
        assert!(!replaced.contains("digraph"), "{alias}");
        assert!(!replaced.contains("data-engine"), "{alias}");
    }
}

#[test]
fn contains_rich_content_names_the_five_classes_and_stops_at_the_closing_quote() {
    assert!(epub_export::contains_rich_content("<span class=\"md-mathi\">x</span>"));
    assert!(epub_export::contains_rich_content("<div class=\"md-mathd\">x</div>"));
    assert!(epub_export::contains_rich_content("<pre class=\"mermaid\">x</pre>"));
    assert!(epub_export::contains_rich_content("<div class=\"plantuml\">x</div>"));
    assert!(epub_export::contains_rich_content("<div class=\"graphviz\" data-engine=\"osage\">x</div>"));
    assert!(!epub_export::contains_rich_content("<p>plain</p>"));
}

/// md.win's `DiagramSvg.DomSelector`, copied from `Md.Core/Export/DiagramSvg.cs`: the diagram-SVG
/// module is ported separately (its stub is not this suite's), so the value is spelled out here.
const DIAGRAM_SVG_DOM_SELECTOR: &str = "pre.mermaid, div.plantuml, div.graphviz, div.plot";

#[test]
fn the_dom_selector_and_the_marker_table_name_the_same_containers() {
    // The app queries the live DOM with this exact string; the marker scan walks the same
    // markup. One kind counted here and not there shifts every later image onto the wrong
    // element, which is why the Swift states the invariant three separate times.
    assert_eq!(".md-mathi, .md-mathd, .mermaid, .plantuml, .graphviz", epub_export::RICH_SELECTOR);
    assert_eq!("application/epub+zip", epub_export::MIMETYPE);
    assert_eq!("content.xhtml", epub_export::CONTENT_FILE_NAME);
    // Not the diagram-SVG set: that one has div.plot and no formulas. Do not derive one from
    // the other.
    assert_ne!(DIAGRAM_SVG_DOM_SELECTOR, epub_export::RICH_SELECTOR);
}

#[test]
fn the_marker_scan_takes_only_the_six_shapes_the_writer_emits() {
    // DECIDED DIVERGENCE, pinned here: macOS scans with the regex
    // `<(span|div|pre) class="(?:md-mathi|md-mathd|mermaid|plantuml|graphviz)"[^>]*>…</\1>`,
    // which would also match combinations the writer never emits — a `<div class="md-mathi">`,
    // a `<span class="mermaid">`. md.win follows Kotlin and TypeScript and lists the six real
    // shapes instead, so the two agree byte-for-byte on this writer's output and differ only on
    // markup no port can produce. `contains_rich_content` is the wider test on all four ports, so
    // such markup would still open a renderer — and then fail the snapshot count check rather
    // than ship a shifted book.
    let unreal = "<div class=\"md-mathi\">x</div>";
    assert!(epub_export::rich_elements(unreal).is_empty());
    assert_eq!(unreal, epub_export::replacing_rich_elements(unreal, &["IMG"]));
    assert!(epub_export::contains_rich_content(unreal));
}

#[test]
fn image_tag_and_file_names_follow_the_macos_spelling() {
    assert_eq!("images/unit-000-rich-0.png", epub_export::image_file_name(0, 0));
    assert_eq!("images/unit-012-rich-3.png", epub_export::image_file_name(12, 3));
    assert_eq!("unit-000.xhtml", epub_export::unit_file_name(0));
    assert_eq!("unit-007.xhtml", epub_export::unit_file_name(7));
    assert_eq!(
        "<img src=\"images/unit-000-rich-0.png\" alt=\"formula\" width=\"84\"/>",
        epub_export::image_tag("images/unit-000-rich-0.png", true, 84)
    );
    assert_eq!(
        "<img src=\"images/unit-001-rich-2.png\" alt=\"diagram\" width=\"420\"/>",
        epub_export::image_tag("images/unit-001-rich-2.png", false, 420)
    );
}

#[test]
fn snapshot_width_rounds_half_away_from_zero_not_to_even() {
    // Swift's `Int(rect.width.rounded())`. .NET's Math.Round defaults to banker's rounding,
    // which would make 84.5 → 84 and 85.5 → 86 — two different rules in one column.
    assert_eq!(85, RichSnapshot::new(png(1), 84.5).display_width());
    assert_eq!(86, RichSnapshot::new(png(1), 85.5).display_width());
    assert_eq!(84, RichSnapshot::new(png(1), 84.4).display_width());
    assert_eq!(1, RichSnapshot::new(png(1), 0.5).display_width());
}

// MARK: - The package document

#[test]
fn opf_carries_metadata_manifest_and_spine() {
    let opf = epub_export::opf(
        "A & B",
        "urn:uuid:TEST",
        "2026-07-10T00:00:00Z",
        &["unit-000.xhtml", "unit-001.xhtml"],
        &["images/unit-001-rich-0.png"],
        &[],
        None,
        None,
    );
    assert!(opf.contains("<dc:title>A &amp; B</dc:title>"));
    assert!(opf.contains("<dc:language>en</dc:language>"));
    assert!(opf.contains("<dc:identifier id=\"bookid\">urn:uuid:TEST</dc:identifier>"));
    assert!(opf.contains("<meta property=\"dcterms:modified\">2026-07-10T00:00:00Z</meta>"));
    assert!(opf.contains("properties=\"nav\""));
    assert!(opf.contains("<item id=\"u1\" href=\"unit-001.xhtml\" media-type=\"application/xhtml+xml\"/>"));
    assert!(opf.contains("<item id=\"i0\" href=\"images/unit-001-rich-0.png\" media-type=\"image/png\"/>"));
    // The spine follows the unit order — the reading order.
    let first = opf.find("<itemref idref=\"u0\"/>").expect("u0 in the spine");
    let second = opf.find("<itemref idref=\"u1\"/>").expect("u1 in the spine");
    assert!(first < second);
}

#[test]
fn opf_flags_only_the_units_that_carry_an_svg() {
    // EPUB 3 requires the reserved property on every content document holding an <svg>;
    // EPUBCheck reports OPF-014 without it. Today that means a ```plot fence.
    let opf = epub_export::opf(
        "T",
        "urn:uuid:TEST",
        MODIFIED,
        &["unit-000.xhtml", "unit-001.xhtml", "unit-002.xhtml"],
        &[],
        &[1],
        None,
        None,
    );
    assert!(opf.contains("<item id=\"u0\" href=\"unit-000.xhtml\" media-type=\"application/xhtml+xml\"/>"));
    assert!(
        opf.contains(
            "<item id=\"u1\" href=\"unit-001.xhtml\" media-type=\"application/xhtml+xml\" properties=\"svg\"/>"
        )
    );
    assert!(opf.contains("<item id=\"u2\" href=\"unit-002.xhtml\" media-type=\"application/xhtml+xml\"/>"));
}

#[test]
fn opf_with_no_units_still_parses() {
    // The shape the identifier test drives: an empty manifest tail and an empty spine.
    let opf = epub_export::opf("Empty", "urn:uuid:TEST", MODIFIED, &[], &[], &[], None, None);
    assert!(opf.contains("<manifest>\n<item id=\"nav\""));
    assert!(opf.contains("media-type=\"text/css\"/>\n</manifest>"));
    assert!(opf.contains("<spine>\n\n</spine>"));
}

// MARK: - The navigation document

#[test]
fn nav_lists_root_articles_then_nested_chapters() {
    let nav = epub_export::nav(
        "Book",
        &[
            NavEntry::new("Intro", "unit-001.xhtml"),
            NavEntry::with_children("Chapter One", "unit-002.xhtml", vec![NavEntry::new("First", "unit-003.xhtml")]),
        ],
    );
    assert!(nav.contains("epub:type=\"toc\""));
    assert!(nav.contains("<a href=\"unit-001.xhtml\">Intro</a>"));
    // The chapter's articles nest inside the chapter's own item.
    assert!(nav.contains("<li><a href=\"unit-002.xhtml\">Chapter One</a>\n<ol>"));
    assert!(nav.contains("<a href=\"unit-003.xhtml\">First</a>"));
}

#[test]
fn nav_title_is_the_book_title_and_the_head_is_macos_spelled() {
    // macOS puts the book's own title in the <title> and the <h1>; Android writes the literal
    // "Contents". macOS wins, and the charset meta is macOS-only too.
    let nav = epub_export::nav("A & B", &[NavEntry::new("x", "unit-001.xhtml")]);
    assert!(nav.contains("<meta charset=\"utf-8\"/>"));
    assert!(nav.contains("<title>A &amp; B</title>"));
    assert!(nav.contains("<h1>A &amp; B</h1>"));
    assert!(!nav.contains("Contents"));
}

#[test]
fn xhtml_document_carries_the_charset_meta_and_the_shared_stylesheet() {
    let page = epub_export::xhtml_document("A \"quoted\" & <angled> title", "<p>body</p>");
    assert!(page.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n"));
    assert!(page.contains("<html xmlns=\"http://www.w3.org/1999/xhtml\">"));
    assert!(page.contains("<meta charset=\"utf-8\"/>"));
    assert!(page.contains("<title>A &quot;quoted&quot; &amp; &lt;angled&gt; title</title>"));
    assert!(page.contains("<link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/>"));
    assert!(page.ends_with("<body>\n<p>body</p>\n</body>\n</html>"));
}

// MARK: - Body extraction

#[test]
fn body_extraction_trims_and_keeps_the_script_for_the_fixer_to_remove() {
    let document = html::document("# Alpha\n\nText.\n", "Alpha", false, true);
    let body = epub_export::body_html(&document);
    assert!(body.starts_with("<h1 id=\"alpha\">Alpha</h1>"));
    assert!(body.ends_with("<script type=\"module\" src=\"rich/md-init.js\"></script>"));
    // Stripping the script leaves the newline that preceded it — this is where the blank line
    // before </body> in every content file comes from. Android cuts at the script and has none.
    assert!(epub_export::xhtml_body(body).ends_with("<p>Text.</p>\n"));
    let page = epub_export::xhtml_document("Alpha", &epub_export::xhtml_body(body));
    assert!(page.contains("<p>Text.</p>\n\n</body>"));
}

#[test]
fn body_extraction_hands_back_the_input_when_the_markers_are_missing_or_reversed() {
    assert_eq!("no body here", epub_export::body_html("no body here"));
    assert_eq!("<body", epub_export::body_html("<body"));
    // A close tag before the open one: the Swift traps on the range, md.vscode guards it.
    assert_eq!("</body>x<body >", epub_export::body_html("</body>x<body >"));
    // The LAST close tag wins, so a document that somehow held two yields the whole body.
    assert_eq!("a</body>b", epub_export::body_html("<body>a</body>b</body>"));
}

// MARK: - The identifier

#[test]
fn epub_identifier_is_stable_for_the_same_book() {
    // A fresh random UUID per export made every export a *different* publication: re-exporting
    // after fixing a typo stacked up beside the old file in a reader's library.
    let first = epub_export::stable_identifier("My Book");
    let second = epub_export::stable_identifier("My Book");
    assert_eq!(first, second);
    assert_ne!(first, epub_export::stable_identifier("Other Book"));

    let uuid = first.strip_prefix("urn:uuid:").expect("a urn:uuid");
    let groups: Vec<&str> = uuid.split('-').collect();
    assert_eq!(vec![8, 4, 4, 4, 12], groups.iter().map(|group| group.len()).collect::<Vec<_>>());
    assert!(uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    assert_eq!(Some('5'), groups[2].chars().next()); // version 5 — name-based, not random
    assert!("89ab".contains(groups[3].chars().next().unwrap())); // RFC 4122 variant

    // And it is the value that actually reaches the package document.
    let opf = epub_export::opf("My Book", &first, MODIFIED, &[], &[], &[], None, None);
    assert!(opf.contains(&format!("<dc:identifier id=\"bookid\">{first}</dc:identifier>")));
}

#[test]
fn stable_identifier_matches_the_rfc4122_oracle() {
    // Independently computed with Python's uuid.uuid5(uuid.NAMESPACE_URL, name) — the same RFC 4122
    // v5 construction the four ports share, so this pins the namespace, the SHA-1 and the hex layout
    // against something outside this family. Never "upgrade" the hash: it would orphan every book
    // already in a reader's library.
    for (title, expected) in [
        ("My Book", "urn:uuid:9905e6b1-7ffd-5c7f-9cef-1d878db12e9d"),
        ("Other Book", "urn:uuid:07addc7e-aaf2-52ce-8dff-3367db92aca3"),
        ("The Book", "urn:uuid:8f60e4b5-88ee-5555-a8df-3fb31302834d"),
        ("Round Trip", "urn:uuid:b662c790-6033-5d05-a67f-244c745d5c0c"),
        ("", "urn:uuid:1b4db7eb-4057-5ddf-91e0-36dec72071f5"),
    ] {
        assert_eq!(expected, epub_export::stable_identifier(title), "{title:?}");
    }
}

#[test]
fn stable_identifier_hashes_the_title_as_utf8() {
    // The encoding RFC 4122 §4.3 names, and the only one that gives a non-ASCII title the same
    // UUID here as on Apple and Android. Built from code points rather than typed, so the
    // assertion cannot quietly depend on how this file happens to be encoded.
    let cyrillic: String =
        [0x041F, 0x0440, 0x0438, 0x0432, 0x0435, 0x0442].into_iter().filter_map(char::from_u32).collect();
    assert_eq!("urn:uuid:b9be3511-b3be-5e1d-b703-42c05088d0fa", epub_export::stable_identifier(&cyrillic));
    // UTF-16 would hash different bytes entirely; this is the value Python's
    // uuid.uuid5(uuid.NAMESPACE_URL, name) gives for the same name.
    assert_ne!(epub_export::stable_identifier(&cyrillic), epub_export::stable_identifier("Privet"));
}

// MARK: - Single document

#[test]
fn document_epub_title_prefers_front_matter_then_file_name() {
    // The front-matter `title:` wins when present…
    assert_eq!(
        "My Essay",
        epub_export::document_title_from_front_matter(
            &parser::front_matter("---\ntitle: My Essay\n---\n\n# H"),
            "notes"
        )
    );
    // …the key is matched case-insensitively, and the value is trimmed…
    assert_eq!(
        "Spaced",
        epub_export::document_title_from_front_matter(&[MetadataField::new("Title", "  Spaced  ")], "notes")
    );
    // …an empty value falls through to the file name…
    assert_eq!("notes", epub_export::document_title_from_front_matter(&[MetadataField::new("title", "  ")], "notes"));
    // …and with no front matter at all the file name is the title.
    assert_eq!(
        "My File",
        epub_export::document_title_from_front_matter(&parser::front_matter("# Just a heading"), "My File")
    );
    // The source overload is the same rule with the parse folded in.
    assert_eq!("My Essay", epub_export::document_title("---\ntitle: My Essay\n---\n\n# H", "notes"));
    assert_eq!("My File", epub_export::document_title("# Just a heading", "My File"));
}

#[test]
fn document_title_trims_foundation_whitespace_not_dotnet_whitespace() {
    // Foundation's .whitespacesAndNewlines has U+200B in it (the frozen tables still call it Zs)
    // and .NET's char.IsWhiteSpace does not — so string.Trim() would keep a title of nothing but
    // zero-width spaces and hand a reader a blank one.
    let zero_width = "\u{200B}".repeat(3);
    assert_eq!(
        "notes",
        epub_export::document_title_from_front_matter(&[MetadataField::new("title", &zero_width)], "notes")
    );
    assert_eq!(
        "T",
        epub_export::document_title_from_front_matter(
            &[MetadataField::new("title", format!("{zero_width}T{zero_width}"))],
            "notes"
        )
    );
    // U+0085 NEL is in the set on both sides, so it trims either way.
    assert_eq!(
        "notes",
        epub_export::document_title_from_front_matter(&[MetadataField::new("title", "\u{0085}")], "notes")
    );
}

#[test]
fn document_epub_is_a_valid_stored_zip_with_mimetype_first() {
    let entries = document_epub_entries("# Alpha\n\nText.", "Alpha");
    // The pure entries list already puts `mimetype` first…
    assert_eq!("mimetype", entries[0].name);
    assert_eq!("application/epub+zip", text(&entries[0].data));

    // …and once archived (through the very zip writer the book export uses) it is a valid stored
    // zip whose sniffable magic is exactly what a reader checks before unzipping.
    let archive = ZipWriter::archive(&entries).expect("the archive fits");
    assert_eq!([0x50, 0x4B, 0x03, 0x04], archive[..4]);
    assert_eq!(0, archive[8]); // stored method, not deflated
    assert_eq!(0, archive[9]);
    assert_eq!("mimetype", text(&archive[30..38]));
    assert_eq!("application/epub+zip", text(&archive[38..58]));
}

#[test]
fn document_epub_opf_references_resolve_with_one_real_unit_and_no_title_page() {
    let entries = document_epub_entries("# Alpha\n\nText.\n\n## Beta\n\nMore.", "Alpha");
    let names: HashSet<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    let opf = entry_text(&entries, "OEBPS/content.opf");

    // Every href the manifest names must resolve to a packed file — nav, stylesheet and the one
    // content unit alike, no dangling reference.
    let hrefs = hrefs(&opf);
    assert!(hrefs.iter().any(|href| href == "content.xhtml"));
    for href in &hrefs {
        assert!(names.contains(format!("OEBPS/{href}").as_str()), "{href} is not packed");
    }

    // Exactly one content unit — content.xhtml — and no phantom title page: the book path makes
    // unit-000 an <h1>title</h1> page and starts its nav past it, and that unit must not exist here.
    let content_units: Vec<&str> =
        entries.iter().map(|e| e.name.as_str()).filter(|n| n.ends_with(".xhtml") && *n != "OEBPS/nav.xhtml").collect();
    assert_eq!(vec!["OEBPS/content.xhtml"], content_units);

    // The spine is that one unit, once.
    assert_eq!(1, opf.matches("<itemref").count());
    assert!(opf.contains("<itemref idref=\"u0\"/>"));
}

#[test]
fn document_epub_nav_lists_headings_with_slugs_matching_the_content_anchors() {
    // Two headings that would collide on slug, so the dedup ("beta", "beta-1") is exercised on
    // both sides at once.
    let source = "# Alpha\n\nText.\n\n## Beta\n\nMore.\n\n## Beta\n\nEnd.";
    let entries = document_epub_entries(source, "Alpha");
    let outline = parser::outline(source);
    assert_eq!(vec!["alpha", "beta", "beta-1"], outline.iter().map(|e| e.slug.as_str()).collect::<Vec<_>>());

    let nav = entry_text(&entries, "OEBPS/nav.xhtml");
    let content = entry_text(&entries, "OEBPS/content.xhtml");
    assert!(nav.contains("epub:type=\"toc\""));

    // Every heading is a nav entry pointing at its anchor inside the one content file, and that
    // anchor really exists — the nav slug and the id are the same rule, so a tap lands on the
    // section rather than on nothing.
    for slug in outline.iter().map(|e| &e.slug) {
        assert!(nav.contains(&format!("href=\"content.xhtml#{slug}\"")));
        assert!(content.contains(&format!("id=\"{slug}\"")));
    }
    assert_eq!(3, nav.matches("content.xhtml#").count());
}

#[test]
fn document_epub_of_a_headingless_document_has_a_valid_nav() {
    // A document with no headings has an empty outline, and a toc <nav> whose <ol> holds no <li>
    // is not valid EPUB 3. The fallback is a single entry — the whole document under its title,
    // linking to the content file itself.
    let entries = document_epub_entries("Just a paragraph, no headings.", "Untitled");
    let nav = entry_text(&entries, "OEBPS/nav.xhtml");
    assert!(!nav.contains("<ol>\n\n</ol>"));
    assert!(!nav.replace([' ', '\n'], "").contains("<ol></ol>"));
    assert!(nav.contains("<li>"));
    assert!(nav.contains("href=\"content.xhtml\""));
    assert!(nav.contains(">Untitled</a>"));
}

#[test]
fn document_epub_identifier_is_stable_across_two_exports() {
    // Two exports of the same document reach the same dc:identifier, so re-exporting after an
    // edit replaces the file in a reader's library instead of stacking a second copy beside it.
    fn identifier(entries: &[ZipEntry]) -> String {
        let opf = entry_text(entries, "OEBPS/content.opf");
        let open = "<dc:identifier id=\"bookid\">";
        let start = opf.find(open).expect("an identifier") + open.len();
        let end = start + opf[start..].find("</dc:identifier>").expect("closed");
        opf[start..end].to_string()
    }

    let first = document_epub_entries("# Title\n\nBody v1.", "The Same Doc");
    let second = document_epub_entries("# Title\n\nBody v2, edited.", "The Same Doc");
    assert_eq!(identifier(&first), identifier(&second));
    assert_eq!(epub_export::stable_identifier("The Same Doc"), identifier(&first));
    assert_ne!(identifier(&first), identifier(&document_epub_entries("# X", "Another Doc")));
}

// MARK: - The stylesheet

#[test]
fn stylesheet_is_the_export_css_plus_the_padding_override() {
    let css = epub_export::stylesheet();
    // The whole light export sheet, verbatim — quirks and all (the body padding and 11pt size
    // are intentional-by-omission on macOS; Android and md.vscode each diverge differently).
    assert!(css.contains("padding: 48px 56px;"));
    assert!(css.contains("font-size: 11pt;"));
    assert!(css.contains("html, body { background: #FFFFFF; }"));
    assert!(css.contains(".md-pagebreak { height: 0; margin: 0; break-after: page; }"));
    assert!(!css.contains("<style>"));
    assert!(!css.contains("</style>"));
    // …then the one override the reader needs.
    assert!(css.ends_with("\n/* EPUB: the reader owns pages and margins. */\nbody { padding: 0.5em 5%; }\n"));
    assert_eq!(
        html::css(false, true) + "\n/* EPUB: the reader owns pages and margins. */\nbody { padding: 0.5em 5%; }\n",
        css
    );
}

// MARK: - Timestamps and the container

#[test]
fn modified_is_utc_second_precision_with_no_fraction() {
    // epubcheck rejects fractional seconds in dcterms:modified.
    assert_eq!("2026-07-24T00:00:00Z", epub_export::modified(instant(2026, 7, 24, 0, 0, 0, 0, 0)));
    // A non-UTC input is converted, not relabelled.
    assert_eq!("2026-07-23T22:30:45Z", epub_export::modified(instant(2026, 7, 24, 0, 30, 45, 500, 120)));
    let now = epub_export::modified_now();
    assert_eq!(20, now.len());
    assert!(now.ends_with('Z'));
    assert!(!now.contains('.'));
}

#[test]
fn container_xml_points_at_the_package_document() {
    assert_eq!(
        epub_export::CONTAINER_XML,
        [
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
            "<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">",
            "<rootfiles>",
            "<rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/>",
            "</rootfiles>",
            "</container>",
        ]
        .join("\n")
    );
    assert!(!epub_export::CONTAINER_XML.contains("\n\n"));
}

// MARK: - Planning and the snapshot contract

#[test]
fn plan_document_describes_the_one_page_the_app_must_render() {
    let plan = epub_export::plan_document("$x^2$ and\n\n```mermaid\ngraph TD\n```\n", "Doc");
    assert_eq!(1, plan.units.len());
    let unit = &plan.units[0];
    assert_eq!(0, unit.index);
    assert_eq!("content.xhtml", unit.file);
    assert!(!unit.is_heading_page);
    // The page the app loads is the very HTML the writer produces for a paper export.
    assert_eq!(html::document("$x^2$ and\n\n```mermaid\ngraph TD\n```\n", "Doc", false, true), unit.document);
    assert_eq!(vec![true, false], unit.rich_elements.iter().map(|e| e.is_math).collect::<Vec<_>>());
    assert_eq!(epub_export::stable_identifier("Doc"), plan.identifier);
    assert_eq!(1, plan.rich_units().len());
}

#[test]
fn plan_document_of_a_plain_document_asks_for_no_rendering() {
    let plan = epub_export::plan_document("# Alpha\n\nText.\n", "Alpha");
    assert_eq!(1, plan.units.len());
    assert!(plan.units[0].rich_elements.is_empty());
    assert!(plan.rich_units().is_empty());
}

#[test]
fn a_mismatched_snapshot_count_is_refused_by_name() {
    let plan = epub_export::plan_document("$a$ and $b$\n", "Doc");
    assert_eq!(2, plan.units[0].rich_elements.len());

    // One short: Swift's `zip` would silently leave the second formula as TeX source.
    let Err(EpubError::SnapshotCount(thrown)) =
        build_document("$a$ and $b$\n", "Doc", Some(&[RichSnapshot::new(png(1), 40.0)]))
    else {
        panic!("a short snapshot list must be refused");
    };
    assert_eq!(0, thrown.unit_index);
    assert_eq!("Doc", thrown.unit_title);
    assert_eq!(2, thrown.expected);
    assert_eq!(1, thrown.actual);
    assert!(thrown.to_string().contains("2 rich element(s)"));

    // None at all, for a document that needs two.
    assert!(matches!(build_document("$a$ and $b$\n", "Doc", None), Err(EpubError::SnapshotCount(_))));

    // And one too many, for a document that needs none.
    let Err(EpubError::SnapshotCount(extra)) =
        build_document("Plain.\n", "Doc", Some(&[RichSnapshot::new(png(1), 40.0)]))
    else {
        panic!("an extra snapshot must be refused");
    };
    assert_eq!(0, extra.expected);
    assert_eq!(1, extra.actual);
}

#[test]
fn snapshots_become_images_in_the_archive_and_in_the_manifest() {
    let source = "Before $a^2$ after.\n\n```mermaid\ngraph TD\n```\n";
    let bytes =
        build_document(source, "Doc", Some(&[RichSnapshot::new(png(1), 84.5), RichSnapshot::new(png(2), 300.0)]))
            .expect("the counts match");
    let entries = read(&bytes);

    let content = entry_text(&entries, "OEBPS/content.xhtml");
    assert!(content.contains("<img src=\"images/unit-000-rich-0.png\" alt=\"formula\" width=\"85\"/>"));
    assert!(content.contains("<img src=\"images/unit-000-rich-1.png\" alt=\"diagram\" width=\"300\"/>"));
    assert!(!content.contains("md-mathi"));
    assert!(!content.contains("class=\"mermaid\""));

    assert_eq!(Some(png(1).as_slice()), entry(&entries, "OEBPS/images/unit-000-rich-0.png"));
    assert_eq!(Some(png(2).as_slice()), entry(&entries, "OEBPS/images/unit-000-rich-1.png"));
    let opf = entry_text(&entries, "OEBPS/content.opf");
    assert!(opf.contains("<item id=\"i0\" href=\"images/unit-000-rich-0.png\" media-type=\"image/png\"/>"));
    assert!(opf.contains("<item id=\"i1\" href=\"images/unit-000-rich-1.png\" media-type=\"image/png\"/>"));
}

#[test]
fn a_plot_stays_vector_and_flags_its_content_document() {
    // The one rich block that is not photographed: it is already an <svg>, so it goes in as
    // markup — and the content document that holds it must carry properties="svg" (OPF-014).
    let bytes = build_document("```plot\nsin(x)\n```\n", "Plotted", None).expect("no snapshots needed");
    let entries = read(&bytes);
    let content = entry_text(&entries, "OEBPS/content.xhtml");
    assert!(content.contains("<div class=\"plot\">"));
    assert!(content.contains("<svg"));
    assert!(!content.contains("<img"));
    let opf = entry_text(&entries, "OEBPS/content.opf");
    assert!(
        opf.contains(
            "<item id=\"u0\" href=\"content.xhtml\" media-type=\"application/xhtml+xml\" properties=\"svg\"/>"
        )
    );
    // A plot-only document never has to open a renderer at all.
    assert!(epub_export::plan_document("```plot\nsin(x)\n```\n", "Plotted").rich_units().is_empty());
}

// MARK: - Whole book

fn sample_book() -> md_core::book::structured_book::StructuredBook {
    book(
        "The Book",
        vec![unit("Preface", "Opening words.")],
        vec![
            section("One", vec![unit("First", "# Inner\n\nA."), unit("Second", "B.")]),
            section("Two", vec![unit("Third", "C.")]),
        ],
    )
}

#[test]
fn book_units_are_the_title_page_then_root_articles_then_chapters() {
    let plan = epub_export::plan_book(&sample_book());
    assert_eq!(
        vec![
            "unit-000.xhtml",
            "unit-001.xhtml",
            "unit-002.xhtml",
            "unit-003.xhtml",
            "unit-004.xhtml",
            "unit-005.xhtml",
            "unit-006.xhtml"
        ],
        plan.units.iter().map(|u| u.file.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(
        vec!["The Book", "Preface", "One", "First", "Second", "Two", "Third"],
        plan.units.iter().map(|u| u.title.as_str()).collect::<Vec<_>>()
    );
    // The title page and the two chapter openers are heading pages — nothing to render.
    assert_eq!(
        vec![true, false, true, false, false, true, false],
        plan.units.iter().map(|u| u.is_heading_page).collect::<Vec<_>>()
    );
    assert_eq!("<h1>The Book</h1>", plan.units[0].body);
    assert!(plan.rich_units().is_empty());
}

#[test]
fn book_nav_lists_root_articles_then_chapters_with_their_articles_nested() {
    let bytes = epub_export::build(&sample_book(), None, Some(MODIFIED), None).expect("built");
    let entries = read(&bytes);
    let nav = entry_text(&entries, "OEBPS/nav.xhtml");
    assert!(nav.contains("<h1>The Book</h1>"));
    assert!(nav.contains("<li><a href=\"unit-001.xhtml\">Preface</a></li>"));
    assert!(nav.contains(
        "<li><a href=\"unit-002.xhtml\">One</a>\n<ol>\n<li><a href=\"unit-003.xhtml\">First</a></li>\n<li><a href=\"unit-004.xhtml\">Second</a></li>\n</ol>\n</li>"
    ));
    assert!(nav.contains(
        "<li><a href=\"unit-005.xhtml\">Two</a>\n<ol>\n<li><a href=\"unit-006.xhtml\">Third</a></li>\n</ol>\n</li>"
    ));
    // The title page is a spine entry, never a nav row.
    assert!(!nav.contains("unit-000.xhtml"));

    let opf = entry_text(&entries, "OEBPS/content.opf");
    assert!(opf.contains("<dc:title>The Book</dc:title>"));
    assert!(opf.contains(&format!(
        "<dc:identifier id=\"bookid\">{}</dc:identifier>",
        epub_export::stable_identifier("The Book")
    )));
    assert_eq!(7, opf.matches("<itemref").count());
    assert!(opf.contains("<itemref idref=\"u0\"/>\n<itemref idref=\"u1\"/>"));
}

#[test]
fn book_archive_holds_every_unit_and_every_manifest_href_resolves() {
    let bytes = epub_export::build(&sample_book(), None, Some(MODIFIED), None).expect("built");
    let entries = read(&bytes);
    assert_eq!(
        vec![
            "mimetype",
            "META-INF/container.xml",
            "OEBPS/content.opf",
            "OEBPS/nav.xhtml",
            "OEBPS/style.css",
            "OEBPS/unit-000.xhtml",
            "OEBPS/unit-001.xhtml",
            "OEBPS/unit-002.xhtml",
            "OEBPS/unit-003.xhtml",
            "OEBPS/unit-004.xhtml",
            "OEBPS/unit-005.xhtml",
            "OEBPS/unit-006.xhtml",
        ],
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>()
    );
    let names: HashSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let opf = entry_text(&entries, "OEBPS/content.opf");
    for href in hrefs(&opf) {
        assert!(names.contains(format!("OEBPS/{href}").as_str()), "{href} is not packed");
    }
    // The title page really is a level-1 heading page under the book's name.
    assert!(entry_text(&entries, "OEBPS/unit-000.xhtml").contains("<body>\n<h1>The Book</h1>\n</body>"));
}

#[test]
fn book_snapshots_are_keyed_by_unit_index() {
    let rich_book = book(
        "Rich Book",
        vec![unit("Plain", "Nothing here.")],
        vec![section("Ch", vec![unit("Mathy", "Text $a^2$ text.\n")])],
    );
    let plan = epub_export::plan_book(&rich_book);
    // unit-000 title page, unit-001 Plain, unit-002 the chapter opener, unit-003 Mathy.
    let rich = plan.rich_units();
    assert_eq!(1, rich.len());
    assert_eq!(3, rich[0].index);

    let snapshots = HashMap::from([(3, vec![RichSnapshot::new(png(7), 40.0)])]);
    let bytes = epub_export::build(&rich_book, Some(&snapshots), Some(MODIFIED), None).expect("built");
    let entries = read(&bytes);
    assert_eq!(Some(png(7).as_slice()), entry(&entries, "OEBPS/images/unit-003-rich-0.png"));
    assert!(
        entry_text(&entries, "OEBPS/unit-003.xhtml")
            .contains("<img src=\"images/unit-003-rich-0.png\" alt=\"formula\" width=\"40\"/>")
    );
    // Keyed by unit index, so the photograph lands on the article that needs it and nowhere else.
    assert!(!entry_text(&entries, "OEBPS/unit-001.xhtml").contains("<img"));
}

#[test]
fn a_book_with_nothing_but_a_title_page_gets_the_swifts_empty_nav() {
    // KNOWN, deliberate: the macOS book nav has no fallback for a book with neither articles nor
    // chapters, so its <nav> has no <ol> at all — which is not valid EPUB 3. Android adds the
    // title page as the single entry there; the single-document path (above) has the fallback on
    // every port. Byte parity with macOS wins here, and this test is the flag on it.
    let bytes = epub_export::build(&book("Bare", vec![], vec![]), None, Some(MODIFIED), None).expect("built");
    let nav = entry_text(&read(&bytes), "OEBPS/nav.xhtml");
    assert!(nav.contains("<nav epub:type=\"toc\">\n<h1>Bare</h1></nav>"));
    assert!(!nav.contains("<ol>"));
}

// MARK: - The finished archive

#[test]
fn epub_round_trips_through_a_zip_reader() {
    // The macOS PDFExportTests shell out to /usr/bin/unzip and md.win opens the bytes with
    // System.IO.Compression; the stand-in here is an independent central-directory walk
    // (tests/common/epub.rs), then the app's own reader.
    let round_trip = book(
        "Round Trip",
        vec![unit("Intro", "# Intro\n\nHello *there*.")],
        vec![section("One", vec![unit("First", "Body text.")])],
    );
    let bytes = epub_export::build(&round_trip, None, Some(MODIFIED), None).expect("built");

    let archive = central_directory(&bytes);
    assert_eq!("mimetype", archive[0].name);
    assert_eq!("application/epub+zip", text(&archive[0].data));
    assert!(archive.iter().any(|entry| entry.name == "OEBPS/content.opf"));
    // Every member is stored, so every entry's compressed length equals its real length.
    for entry in &archive {
        assert_eq!(0, entry.method, "{}", entry.name);
        assert_eq!(entry.uncompressed, entry.compressed, "{}", entry.name);
    }

    // …and the app's own reader agrees, byte for byte.
    let entries = read(&bytes);
    assert_eq!(b"application/epub+zip".as_slice(), entries[0].data.as_slice());
    assert_eq!(archive.len(), entries.len());
    for (walked, read) in archive.iter().zip(&entries) {
        assert_eq!(walked.name, read.name);
        assert_eq!(walked.data, read.data);
    }
}

#[test]
fn every_xml_file_in_a_document_archive_is_well_formed() {
    // The whole point of the XHTML fixer: no browser here, but a conforming XML parser is the
    // same gate EPUBCheck applies first. Named entities, unclosed <br>/<img> or a stray <script>
    // would all fail this.
    for source in [
        "# Alpha\n\nText.\n\n- one\n- two\n\n---\n\n![pic](x.png)\n\n| a | b |\n| - | - |\n| 1 | 2 |\n",
        "```plot\nsin(x)\n```\n\n> A quote with `code` & \"quotes\" <angles>.\n",
        "Just a paragraph, no headings.",
    ] {
        assert_archive_xml_is_well_formed(&build_document(source, "Doc", None).expect("built"));
    }
}

#[test]
fn every_xml_file_in_a_book_archive_is_well_formed_including_snapshotted_units() {
    let tricky = book(
        "A & B \"Book\"",
        vec![unit("Pre<face>", "Words & more.\n\n![pic](x.png)\n")],
        vec![section("Ch & Co", vec![unit("Rich", "Text $a^2$ text.\n\n```mermaid\ngraph TD\n```\n")])],
    );
    let plan = epub_export::plan_book(&tricky);
    let rich = plan.rich_units();
    assert_eq!(1, rich.len());
    let snapshots =
        HashMap::from([(rich[0].index, vec![RichSnapshot::new(png(1), 40.0), RichSnapshot::new(png(2), 200.0)])]);
    let bytes = epub_export::build(&tricky, Some(&snapshots), Some(MODIFIED), None).expect("built");
    assert_archive_xml_is_well_formed(&bytes);
}

fn assert_archive_xml_is_well_formed(archive: &[u8]) {
    let entries = read(archive);
    let mut checked_any = false;
    for entry in &entries {
        if !entry.name.ends_with(".xhtml") && !entry.name.ends_with(".xml") && !entry.name.ends_with(".opf") {
            continue;
        }
        checked_any = true;
        // <!DOCTYPE html> with no subset is skipped rather than resolved, as DtdProcessing.Ignore does.
        if let Err(why) = xml::parse(&text(&entry.data)) {
            panic!("{} is not well-formed XML: {why}", entry.name);
        }
    }
    assert!(checked_any);
}

// MARK: - Port-specific

#[test]
fn assemble_is_exactly_the_entries_zipped() {
    // md.win's Assemble is `ZipWriter.Archive(Entries(…))`. This port writes the borrowed payloads
    // straight into the archive instead (a book's photographs are copied once, not three times), so
    // the two paths are pinned to the same bytes — with snapshots, a cover and the metadata lines.
    let source = "---\nauthor: A\nlanguage: de\n---\n\nBefore $a^2$ after.\n\n```mermaid\ngraph TD\n```\n";
    let plan = epub_export::plan_document(source, "Doc");
    let snapshots = HashMap::from([(0, vec![RichSnapshot::new(png(1), 84.5), RichSnapshot::new(png(2), 300.0)])]);
    let cover = md_core::export::epub_metadata::recognise(Some(&png(9)));
    let assembled =
        epub_export::assemble(&plan, Some(&snapshots), Some(MODIFIED), None, cover.as_ref()).expect("assembled");
    let entries = epub_export::entries(&plan, Some(&snapshots), Some(MODIFIED), None, cover.as_ref()).expect("entries");
    assert_eq!(ZipWriter::archive(&entries).expect("zipped"), assembled);
    assert_eq!(read(&assembled), entries);

    // The book path too, and the identifier seam.
    let plan = epub_export::plan_book(&sample_book());
    let assembled = epub_export::assemble(&plan, None, Some(MODIFIED), Some("urn:uuid:x"), None).expect("assembled");
    let entries = epub_export::entries(&plan, None, Some(MODIFIED), Some("urn:uuid:x"), None).expect("entries");
    assert_eq!(ZipWriter::archive(&entries).expect("zipped"), assembled);
    assert!(
        entry_text(&entries, "OEBPS/content.opf").contains("<dc:identifier id=\"bookid\">urn:uuid:x</dc:identifier>")
    );

    // `entries` refuses a mismatch by name, as `assemble` does.
    let plan = epub_export::plan_document("$a$\n", "Doc");
    let refused = epub_export::entries(&plan, None, Some(MODIFIED), None, None).expect_err("one formula, no snapshot");
    assert_eq!((0, 1, 0), (refused.unit_index, refused.expected, refused.actual));
}

#[test]
fn an_empty_snapshot_list_is_the_same_as_none() {
    // md.win's `Single`: a null or empty list both mean "no photographs".
    let none = build_document("# Plain\n", "Doc", None).expect("built");
    assert_eq!(none, build_document("# Plain\n", "Doc", Some(&[])).expect("built"));
}
