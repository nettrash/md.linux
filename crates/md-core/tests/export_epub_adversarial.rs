//! Adversarial cover for the EPUB writer: the structural invariants of the OCF container and the
//! EPUB 3 package that `export_epub_export.rs` asserts on by substring but never links up, plus the
//! two places the marker scan deliberately parts company with macOS's `NSRegularExpression`. Port
//! of md.win `EpubAdversarialTests.cs`.
//!
//! Every expectation here comes from the EPUB 3 / OCF specification or from a run of the real Swift
//! `EPUBExport` pure half (compiled out of `md.macOS/md/DocumentExport.swift` and driven over the
//! same vectors), never from what this port happens to print. The Swift values quoted in the
//! comments are that run's output.
//!
//! Each of the first four tests was written to kill a surviving mutant: the OPF's
//! `unique-identifier` pointing at nothing, `nav.xhtml` leaving the XHTML namespace, the
//! navigation document's stylesheet link dangling, and the nav manifest item claiming `text/html`.
//! All four produce a file every reader rejects and every earlier assertion still passed. The tree
//! comes from `tests/common/xml.rs`, the stand-in for md.win's `XDocument`.

#[path = "common/epub.rs"]
mod epub;
#[path = "common/xml.rs"]
mod xml;

use std::collections::{HashMap, HashSet};

use epub::*;
use md_core::book::structured_book::StructuredBook;
use md_core::export::ZipEntry;
use md_core::export::epub_export::{self, NavEntry, RichSnapshot};
use md_core::markdown::html;

const MODIFIED: &str = "2026-07-24T00:00:00Z";
const XHTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";
const OPF_NAMESPACE: &str = "http://www.idpf.org/2007/opf";
const DC_NAMESPACE: &str = "http://purl.org/dc/elements/1.1/";
const OPS_NAMESPACE: &str = "http://www.idpf.org/2007/ops";

fn sample_book() -> StructuredBook {
    book(
        "A & B \"Book\"",
        vec![unit("Preface", "Opening words.\n")],
        vec![
            section(
                "One",
                vec![
                    unit("First", "# Inner\n\nText $a^2$ text.\n\n```mermaid\ngraph TD\n```\n"),
                    unit("Second", "B.\n"),
                ],
            ),
            section("Two", vec![unit("Third", "```plot\nsin(x)\n```\n")]),
        ],
    )
}

/// The sample book, photographed exactly as the app must photograph it.
fn sample_book_archive() -> Vec<u8> {
    let plan = epub_export::plan_book(&sample_book());
    let snapshots: HashMap<usize, Vec<RichSnapshot>> = plan
        .rich_units()
        .iter()
        .map(|unit| {
            let shots = (0..unit.rich_elements.len())
                .map(|index| RichSnapshot::new(png(index as u8), 120.0 + index as f64))
                .collect();
            (unit.index, shots)
        })
        .collect();
    epub_export::assemble(&plan, Some(&snapshots), Some(MODIFIED), None, None).expect("assembled")
}

fn document_archive(source: &str, title: &str) -> Vec<u8> {
    epub_export::build_document(source, title, None, Some(MODIFIED), None, None).expect("built")
}

fn parse(entries: &[ZipEntry], name: &str) -> xml::Document {
    let data = entry(entries, name).unwrap_or_else(|| panic!("no entry {name}"));
    // The <!DOCTYPE html> has no internal subset, and nothing is fetched.
    xml::parse(&text(data)).unwrap_or_else(|why| panic!("{name}: {why}"))
}

// MARK: - The package document really is a package

#[test]
fn the_package_unique_identifier_names_a_dc_identifier_that_is_really_there() {
    // EPUB 3 §3.4.3: package/@unique-identifier MUST be the id of a dc:identifier in the
    // metadata. A package whose attribute names nothing is rejected outright (EPUBCheck
    // RSC-005), and no substring assertion notices — both halves can be spelled correctly and
    // still not refer to each other. Swift writes `unique-identifier="bookid"` on the package
    // and `<dc:identifier id="bookid">` in the metadata; this pins the LINK, not the spelling.
    for archive in [sample_book_archive(), document_archive("# Alpha\n\nText.\n", "Doc")] {
        let entries = read(&archive);
        let opf = parse(&entries, "OEBPS/content.opf");
        let package = &opf.root;
        assert!(package.is(OPF_NAMESPACE, "package"));
        assert_eq!(Some("3.0"), package.attribute("version"));

        let unique = package.attribute("unique-identifier").unwrap_or_default();
        assert!(!unique.is_empty());
        let identifiers: Vec<_> = package
            .elements(OPF_NAMESPACE, "metadata")
            .flat_map(|metadata| metadata.elements(DC_NAMESPACE, "identifier"))
            .filter(|element| element.attribute("id") == Some(unique))
            .collect();
        assert_eq!(1, identifiers.len());
        let identifier = identifiers[0].value();
        // …and the value it resolves to is the title-derived one, not a fresh UUID.
        assert!(identifier.starts_with("urn:uuid:"));
        let titles: Vec<_> = package
            .elements(OPF_NAMESPACE, "metadata")
            .flat_map(|metadata| metadata.elements(DC_NAMESPACE, "title"))
            .collect();
        assert_eq!(1, titles.len());
        assert_eq!(epub_export::stable_identifier(&titles[0].value()), identifier);
    }
}

#[test]
fn every_manifest_item_is_referenced_by_the_spine_or_is_a_reserved_resource() {
    // Two directions at once: every spine itemref resolves to a manifest item (a dangling
    // idref is EPUBCheck RSC-005), and every content document in the manifest carries the
    // media type a reader dispatches on. The nav document in particular must be
    // application/xhtml+xml with properties="nav" — calling it text/html leaves the package
    // parseable, the hrefs resolvable, and the book unopenable.
    let entries = read(&sample_book_archive());
    let opf = parse(&entries, "OEBPS/content.opf");
    let manifest = opf.root.element(OPF_NAMESPACE, "manifest").expect("a manifest");
    let items: Vec<_> = manifest.elements(OPF_NAMESPACE, "item").collect();
    let mut by_id = HashMap::new();
    for item in &items {
        let id = item.attribute("id").expect("every item has an id");
        assert!(by_id.insert(id, *item).is_none(), "duplicate manifest id {id}");
    }

    let nav_items: Vec<_> = items
        .iter()
        .filter(|item| item.attribute("properties").unwrap_or_default().split(' ').any(|p| p == "nav"))
        .collect();
    assert_eq!(1, nav_items.len());
    let nav = nav_items[0];
    assert_eq!(Some("nav.xhtml"), nav.attribute("href"));
    assert_eq!(Some("application/xhtml+xml"), nav.attribute("media-type"));

    for item in &items {
        let href = item.attribute("href").expect("every item has an href");
        let expected = if href.ends_with(".xhtml") {
            Some("application/xhtml+xml")
        } else if href.ends_with(".css") {
            Some("text/css")
        } else if href.ends_with(".png") {
            Some("image/png")
        } else {
            None
        };
        assert_eq!(expected, item.attribute("media-type"), "{href}");
    }

    let spine = opf.root.element(OPF_NAMESPACE, "spine").expect("a spine");
    let refs: Vec<&str> =
        spine.elements(OPF_NAMESPACE, "itemref").map(|itemref| itemref.attribute("idref").expect("an idref")).collect();
    assert!(!refs.is_empty());
    for idref in &refs {
        let item = by_id.get(idref).unwrap_or_else(|| panic!("spine idref {idref} has no manifest item"));
        assert_eq!(Some("application/xhtml+xml"), item.attribute("media-type"));
    }
    // The nav document is the one manifest item that is deliberately NOT in the spine on macOS.
    assert!(!refs.contains(&nav.attribute("id").expect("an id")));
    // Every other content document is, in reading order, exactly once.
    let distinct: HashSet<&&str> = refs.iter().collect();
    assert_eq!(refs.len(), distinct.len());
}

// MARK: - Every packed XHTML is a content document a reader can open

#[test]
fn every_packed_xhtml_sits_in_the_xhtml_namespace_and_the_nav_declares_the_ops_prefix() {
    // Swift's literals, checked against the compiled Swift:
    //   xhtmlDocument: <html xmlns="http://www.w3.org/1999/xhtml">
    //   nav:           <html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
    // An XHTML file in the wrong namespace still parses as XML — the suite's well-formedness
    // gate cannot see it — but it is not an EPUB content document.
    for archive in [sample_book_archive(), document_archive("# Alpha\n\nText.\n", "Doc")] {
        let entries = read(&archive);
        let pages: Vec<&ZipEntry> = entries.iter().filter(|entry| entry.name.ends_with(".xhtml")).collect();
        assert!(!pages.is_empty());
        for page in pages {
            let doc = xml::parse(&text(&page.data)).unwrap_or_else(|why| panic!("{}: {why}", page.name));
            assert!(doc.root.is(XHTML_NAMESPACE, "html"), "{}", page.name);
            assert_eq!(1, doc.root.elements(XHTML_NAMESPACE, "head").count(), "{}", page.name);
            assert_eq!(1, doc.root.elements(XHTML_NAMESPACE, "body").count(), "{}", page.name);
        }
        // The toc lives in the OPS namespace, on a <nav> that is really a nav element.
        let nav_doc = parse(&entries, "OEBPS/nav.xhtml");
        let tocs: Vec<_> = nav_doc.descendants().into_iter().filter(|e| e.is(XHTML_NAMESPACE, "nav")).collect();
        assert_eq!(1, tocs.len());
        assert_eq!(Some("toc"), tocs[0].attribute_ns(OPS_NAMESPACE, "type"));
    }
}

#[test]
fn every_local_reference_in_every_packed_page_resolves_to_a_packed_member() {
    // The stylesheet link, the nav's own links and the snapshot images all point at files this
    // export writes; a typo in any of them is EPUBCheck RSC-007 and a reader showing an
    // unstyled page or a broken image. Author-supplied image sources are deliberately out of
    // scope — macOS packs only the snapshot PNGs too, so `![pic](local.png)` dangles on every
    // port — hence a corpus that references nothing of its own.
    let archives = [
        sample_book_archive(),
        document_archive("# Alpha\n\nText.\n\n## Beta\n\nMore.\n", "Doc"),
        document_archive("Just a paragraph, no headings.", "Untitled"),
    ];
    for archive in archives {
        let entries = read(&archive);
        let names: HashSet<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        let mut seen = 0;
        for page in entries.iter().filter(|entry| entry.name.ends_with(".xhtml")) {
            let doc = xml::parse(&text(&page.data)).unwrap_or_else(|why| panic!("{}: {why}", page.name));
            for element in doc.descendants() {
                for attribute in element.attributes.iter().filter(|a| a.local == "href" || a.local == "src") {
                    let value = attribute.value.as_str();
                    if value.is_empty() || value.starts_with('#') || value.contains("://") {
                        continue;
                    }
                    seen += 1;
                    let target = format!("OEBPS/{}", value.split('#').next().unwrap_or_default());
                    assert!(names.contains(target.as_str()), "{} references {value}, which is not packed", page.name);
                }
            }
        }
        // Every page links the stylesheet, so this can never be a vacuous pass.
        assert!(seen >= entries.iter().filter(|entry| entry.name.ends_with(".xhtml")).count());
        // …and the anchors the nav points into really exist in the file it names.
        let nav_doc = parse(&entries, "OEBPS/nav.xhtml");
        for anchor in nav_doc.descendants().into_iter().filter(|e| e.is(XHTML_NAMESPACE, "a")) {
            let href = anchor.attribute("href").expect("an href");
            if !href.contains('#') {
                continue;
            }
            let parts: Vec<&str> = href.split('#').collect();
            let page = parse(&entries, &format!("OEBPS/{}", parts[0]));
            assert!(
                page.descendants().iter().any(|element| element.attribute("id") == Some(parts[1])),
                "no id {} in {}",
                parts[1],
                parts[0]
            );
        }
    }
}

// MARK: - The two decided partings from the macOS regex

#[test]
fn an_unclosed_rich_container_stops_the_scan_where_the_swift_regex_would_skip_past_it() {
    // DECIDED DIVERGENCE, from Kotlin's and TypeScript's marker scan. Measured on the compiled
    // Swift pure half over these exact vectors:
    //   in  "<span class=\"md-mathi\">unclosed <pre class=\"mermaid\">graph</pre>"
    //   Swift richElements -> one match at 32 length 32; replacingRichElements(["ONLY"])
    //         -> "<span class=\"md-mathi\">unclosed ONLY"
    //   md.win           -> no elements; the markup is handed back untouched.
    // The writer closes every container it opens, so this shape cannot be produced — and if it
    // ever were, `contains_rich_content` (the wider test on all four ports) still says yes, the
    // app still photographs what the DOM reports, and the count check refuses the export rather
    // than shipping a book whose pictures have slid onto the wrong elements.
    let unclosed_first = "<span class=\"md-mathi\">unclosed <pre class=\"mermaid\">graph</pre>";
    assert!(epub_export::rich_elements(unclosed_first).is_empty());
    assert_eq!(unclosed_first, epub_export::replacing_rich_elements(unclosed_first, &["ONLY"]));
    assert!(epub_export::contains_rich_content(unclosed_first));

    let unclosed_pre = "<pre class=\"mermaid\">unclosed <span class=\"md-mathi\">a</span>";
    assert!(epub_export::rich_elements(unclosed_pre).is_empty());
    assert_eq!(unclosed_pre, epub_export::replacing_rich_elements(unclosed_pre, &["ONLY"]));

    // A closed container after a closed container is unaffected: the halt is the unclosed case
    // alone, not a one-element cap.
    let two = "<pre class=\"mermaid\">a</pre><span class=\"md-mathi\">b</span>";
    assert_eq!(2, epub_export::rich_elements(two).len());
}

#[test]
fn the_writer_still_emits_the_bare_containers_the_marker_table_is_spelled_for() {
    // Only the Graphviz marker is a PREFIX (`<div class="graphviz"`, no `>`), because the
    // writer names the layout program in the same tag. The other five markers are whole tags,
    // so an attribute added to any of them would make it invisible to the scan while macOS's
    // `[^>]*>` would still match it — measured on the compiled Swift:
    //   "<span class=\"md-mathi\" data-x=\"a>b\">m</span>" -> Swift one match at 0 length 44,
    //   md.win none. That is the exact failure the Swift comment warns about for Graphviz
    //   ("leaving every DOT diagram in the book as raw source, and shifting the snapshots onto
    //   the wrong elements"), so this test guards the assumption instead of the symptom: the
    // day the writer adds an attribute to a math, Mermaid or PlantUML container, it fires here
    // and the marker for that container has to become a prefix too.
    let samples = [
        ("Text $a^2$ text.\n", "<span class=\"md-mathi\">"),
        ("Text $$a^2$$ text.\n", "<span class=\"md-mathd\">"),
        ("```latex\na^2\n```\n", "<div class=\"md-mathd\">"),
        ("```mermaid\ngraph TD\n```\n", "<pre class=\"mermaid\">"),
        ("```plantuml\n@startuml\n@enduml\n```\n", "<div class=\"plantuml\">"),
    ];
    for (source, opener) in samples {
        let document = html::document(source, "t", false, true);
        let body = epub_export::body_html(&document);
        assert!(body.contains(opener), "{source:?}");
        let elements = epub_export::rich_elements(body);
        assert_eq!(1, elements.len(), "{source:?}");
        assert!(body[elements[0].start..elements[0].end()].starts_with(opener), "{source:?}");
    }
    // Graphviz is the one that does carry an extra attribute, and its marker is the prefix.
    let document = html::document("```dot\ndigraph { a -> b }\n```\n", "t", false, true);
    let dot = epub_export::body_html(&document);
    assert!(dot.contains("<div class=\"graphviz\" data-engine=\"dot\">"));
    assert_eq!(1, epub_export::rich_elements(dot).len());
}

#[test]
fn titles_are_escaped_for_the_four_characters_macos_escapes_and_the_apostrophe_is_left_alone() {
    // export.md §6.8 states the rule ("& → &amp;  < → &lt;  > → &gt;  \" → &quot;   (never ')")
    // and no test pinned the negative half. It is reachable on the first apostrophe in a book
    // title, and it is not cosmetic: XML predefines &apos;, so "fixing" it would still parse
    // everywhere and silently break byte parity with the two Apple editions and with md.vscode.
    // Measured on the compiled Swift over the same title:
    //   xhtmlDocument(title: "a'b", …) -> "<title>a'b</title>"
    //   nav(title: "a'b", …)           -> "<title>a'b</title>" and "<h1>a'b</h1>"
    let tricky = "Alice's & Bob's \"<Book>\"";
    let escaped = "Alice's &amp; Bob's &quot;&lt;Book&gt;&quot;";

    let page = epub_export::xhtml_document(tricky, "<p>x</p>");
    assert!(page.contains(&format!("<title>{escaped}</title>")));
    assert!(!page.contains("&apos;"));
    assert!(!page.contains("&#39;"));

    let nav = epub_export::nav(tricky, &[NavEntry::new(tricky, "unit-001.xhtml")]);
    assert!(nav.contains(&format!("<h1>{escaped}</h1>")));
    assert!(nav.contains(&format!(">{escaped}</a>")));
    assert!(!nav.contains("&apos;"));

    // The escaper runs & first, so an already-escaped entity is escaped again rather than
    // passed through — a title that literally reads "&amp;" must survive a round trip.
    assert!(
        epub_export::opf("&amp;", "urn:uuid:TEST", MODIFIED, &[], &[], &[], None, None)
            .contains("<dc:title>&amp;amp;</dc:title>")
    );

    // And it holds through a real archive, where the title reaches four separate files.
    let entries = read(&document_archive("# H\n", tricky));
    for name in ["OEBPS/content.opf", "OEBPS/nav.xhtml", "OEBPS/content.xhtml"] {
        let text = entry_text(&entries, name);
        assert!(text.contains(escaped), "{name}");
        assert!(!text.contains("&apos;"), "{name}");
    }
}

// MARK: - The container's byte layout, read back

#[test]
fn the_archive_is_the_stored_ocf_layout_from_its_first_byte_to_its_end_record() {
    // The OCF rule a reader sniffs before it unzips anything: `mimetype` is the first member,
    // stored, no extra field, so its payload lands at a fixed offset. Then every other member
    // is stored too (macOS never deflates; Android deflates all but the mimetype) and every
    // local header carries the fixed 1980-01-01 date that makes the archive a pure function of
    // its payloads.
    let archive = sample_book_archive();
    let entries = read(&archive);

    assert_eq!("mimetype", entries[0].name);
    assert_eq!([0x50, 0x4B, 0x03, 0x04], archive[..4]);
    assert_eq!(0, u16_at(&archive, 6)); // flags: bit 11 (UTF-8 names) clear
    assert_eq!(0, u16_at(&archive, 8)); // method: stored
    assert_eq!("mimetype", text(&archive[30..38]));
    assert_eq!("application/epub+zip", text(&archive[38..58]));
    assert_eq!(epub_export::MIMETYPE, text(&entries[0].data));

    // Walk every local header the central directory names and check the same three fields.
    let end = archive.len() - 22;
    assert_eq!([0x50, 0x4B, 0x05, 0x06], archive[end..end + 4]);
    let count = usize::from(u16_at(&archive, end + 10));
    assert_eq!(entries.len(), count);
    let directory = u32_at(&archive, end + 16) as usize;
    let mut cursor = directory;
    for (index, entry) in entries.iter().enumerate() {
        assert_eq!([0x50, 0x4B, 0x01, 0x02], archive[cursor..cursor + 4], "record {index}");
        assert_eq!(0, u16_at(&archive, cursor + 10)); // method: stored
        assert_eq!(0, u16_at(&archive, cursor + 12)); // DOS time
        assert_eq!(0x0021, u16_at(&archive, cursor + 14)); // DOS date 1980-01-01
        let compressed = u32_at(&archive, cursor + 20);
        let uncompressed = u32_at(&archive, cursor + 24);
        assert_eq!(uncompressed, compressed); // stored: same size
        let name_length = usize::from(u16_at(&archive, cursor + 28));
        let local = u32_at(&archive, cursor + 42) as usize;
        let name = text(&archive[cursor + 46..cursor + 46 + name_length]);
        assert_eq!(entry.name, name);
        assert_eq!(entry.data.len(), uncompressed as usize);
        // The central record's offset really points at that member's local header.
        assert_eq!([0x50, 0x4B, 0x03, 0x04], archive[local..local + 4]);
        assert_eq!(name, text(&archive[local + 30..local + 30 + name_length]));
        cursor += 46
            + name_length
            + usize::from(u16_at(&archive, cursor + 30)) // extra
            + usize::from(u16_at(&archive, cursor + 32)); // comment
    }
    assert_eq!(directory + u32_at(&archive, end + 12) as usize, cursor);

    // Two builds of the same book are byte-identical: nothing in here is a wall clock.
    assert_eq!(archive, sample_book_archive());
}
