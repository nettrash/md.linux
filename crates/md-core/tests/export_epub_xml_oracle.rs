//! The XML reader the EPUB suites lean on (`tests/common/xml.rs`, the stand-in for md.win's
//! `XmlReader` / `XDocument`) is itself pinned here, so a well-formedness test built on it can never
//! pass because the checker accepts everything. Each refusal is one an XML 1.0 + Namespaces parser
//! must make and an EPUB writer could plausibly trip; then the fixer's whole reason to exist is
//! shown from the other side — the same body *without* `xhtml_body` is refused.

#[path = "common/xml.rs"]
mod xml;

use md_core::export::epub_export;
use md_core::markdown::html;

#[test]
fn the_checker_refuses_what_xml_refuses() {
    for (why, text) in [
        ("an undeclared entity", "<p>&bull;</p>"),
        ("an unclosed void element", "<p>a<br>b</p>"),
        ("an unclosed root", "<html><body>"),
        ("a mismatched end tag", "<a></b>"),
        ("'<' in an attribute value", "<a b=\"<\"/>"),
        ("a bare ampersand", "<a>&</a>"),
        ("an unquoted attribute", "<a b=c/>"),
        ("a duplicate attribute", "<a x=\"1\" x=\"2\"/>"),
        ("a duplicate expanded name", "<a xmlns:p=\"u\" xmlns:q=\"u\" p:x=\"1\" q:x=\"2\"/>"),
        ("an undeclared prefix", "<p:a/>"),
        ("an undeclared attribute prefix", "<a epub:type=\"toc\"/>"),
        ("two roots", "<a/><b/>"),
        ("no root", "<?xml version=\"1.0\"?>"),
        ("text after the root", "<a/>x"),
        ("']]>' in text", "<a>]]></a>"),
        ("'--' in a comment", "<!-- a -- b --><a/>"),
        ("a vertical tab", "<a>\u{0B}</a>"),
        ("a form feed in an attribute", "<a b=\"\u{0C}\"/>"),
        ("U+FFFE", "<a>\u{FFFE}</a>"),
        ("a NUL character reference", "<a>&#0;</a>"),
        ("a surrogate character reference", "<a>&#xD800;</a>"),
        ("a late XML declaration", "<a/><?xml version=\"1.0\"?>"),
        ("attributes run together", "<a b=\"1\"c=\"2\"/>"),
        ("a DOCTYPE after the root", "<a/><!DOCTYPE a>"),
    ] {
        assert!(xml::parse(text).is_err(), "{why}: {text:?} was accepted");
    }
}

#[test]
fn the_checker_accepts_a_content_document_and_resolves_its_names() {
    let text = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n\
                <html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\
                <!-- a comment --><head><title>A &amp; B</title></head>\
                <body><nav epub:type=\"toc\" id='n'><p>&lt;&#8226;&#x41;<![CDATA[<raw>]]></p><br/></nav></body></html>";
    let doc = xml::parse(text).expect("well-formed");
    assert!(doc.root.is("http://www.w3.org/1999/xhtml", "html"));
    let nav = doc.descendants().into_iter().find(|e| e.local == "nav").expect("a nav");
    // The default namespace reaches the element, never an unprefixed attribute.
    assert!(nav.is("http://www.w3.org/1999/xhtml", "nav"));
    assert_eq!(Some("toc"), nav.attribute_ns("http://www.idpf.org/2007/ops", "type"));
    assert_eq!(Some("n"), nav.attribute("id"));
    assert_eq!("<\u{2022}A<raw>", nav.value());
    let title = doc.descendants().into_iter().find(|e| e.local == "title").expect("a title");
    assert_eq!("A & B", title.value());
}

#[test]
fn an_unfixed_body_is_refused_and_the_fixed_one_accepted() {
    // A list (its marker is `&bull;`), a rule (`<hr>`), a hard break (`<br>`), an image (`<img>`)
    // and the md-init script: everything `xhtml_body` exists to repair.
    let document = html::document("- one\n- two\n\n---\n\nline  \nbreak\n\n![pic](x.png)\n", "T", false, true);
    let body = epub_export::body_html(&document);
    assert!(xml::parse(&epub_export::xhtml_document("T", body)).is_err(), "the raw HTML body must not pass as XML");
    let fixed = epub_export::xhtml_document("T", &epub_export::xhtml_body(body));
    if let Err(why) = xml::parse(&fixed) {
        panic!("the fixed body must be XML: {why}");
    }
}
