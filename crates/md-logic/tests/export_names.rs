//! Port of md.win `tests/Md.App.Logic.Tests/Export/ExportNamesTests.cs`.
//!
//! What each save dialog opens with. The family rule and the Linux layer both live in md-core's
//! `export_file_names` and are pinned there; these are the shapes this port builds on top of them.

use md_logic::export::export_names;
use md_logic::strings::exports;
use md_logic::testing::FileTypeChoice;

#[test]
fn each_export_suggests_the_title_with_its_own_extension() {
    assert_eq!(export_names::pdf("Notes"), "Notes.pdf");
    assert_eq!(export_names::html("Notes"), "Notes.html");
    assert_eq!(export_names::epub("Notes"), "Notes.epub");
    assert_eq!(export_names::latex("Notes"), "Notes.tex");
    assert_eq!(export_names::source("Notes"), "Notes.md");
    assert_eq!(export_names::text_bundle_folder("Notes"), "Notes.textbundle");
}

#[test]
fn the_familys_sanitiser_runs_first_on_every_one_of_them() {
    // Split-join on the forbidden set: two offending characters make two dashes, as every port does.
    assert_eq!(export_names::pdf("a/b"), "a-b.pdf");
    assert_eq!(export_names::epub("a:?b"), "a--b.epub");
    assert_eq!(export_names::html("   "), "Document.html");
}

#[test]
fn a_diagram_is_numbered_from_one_for_the_reader() {
    assert_eq!(export_names::diagram_svg("Notes", 0), "Notes-1.svg");
    assert_eq!(export_names::diagram_svg("Notes", 3), "Notes-4.svg");
    assert_eq!(export_names::diagram_svg("", 0), "Document-1.svg");
}

/// LINUX: md.win escapes the device name on its own (`CON-.pdf`); `CON` is an ordinary name on
/// Linux and on the Mac, so it stays `CON.pdf`. The diagram half holds on every port: the suffix is
/// appended to the family stem before the platform layer runs, so no doubled dash can appear.
#[test]
fn a_device_name_keeps_its_ordinal_rather_than_collecting_two_dashes() {
    assert_eq!(export_names::diagram_svg("CON", 0), "CON-1.svg");
    assert_eq!(export_names::pdf("CON"), "CON.pdf");
}

#[test]
fn every_picker_offers_exactly_one_type_with_the_macs_label() {
    fn one(choices: Vec<FileTypeChoice>) -> (String, String) {
        assert_eq!(choices.len(), 1);
        let row = &choices[0];
        assert_eq!(row.extensions.len(), 1);
        (row.label.clone(), row.extensions[0].clone())
    }

    assert_eq!(one(export_names::pdf_choices()), (exports::PDF.to_string(), ".pdf".to_string()));
    assert_eq!(one(export_names::html_choices()), (exports::HTML.to_string(), ".html".to_string()));
    assert_eq!(one(export_names::epub_choices()), (exports::EPUB.to_string(), ".epub".to_string()));
    assert_eq!(one(export_names::latex_choices()), (exports::LATEX.to_string(), ".tex".to_string()));
    assert_eq!(one(export_names::svg_choices()), (exports::SVG.to_string(), ".svg".to_string()));
}

#[test]
fn linux_refuses_only_nul_and_the_two_dot_names() {
    assert_eq!(export_names::pdf("a\0b"), "a-b.pdf");
    assert_eq!(export_names::text_bundle_folder(".."), "...textbundle");
    assert_eq!(export_names::pdf("trailing. "), "trailing..pdf");
}
