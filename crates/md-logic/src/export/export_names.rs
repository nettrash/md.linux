//! Port of md.win `Export/ExportNames.cs`: the name each save dialog is opened with, and the
//! dialog's own type rows (§7, §6.1).
//!
//! Every stem goes through md-core's [`export_file_names`]: the family's split-join sanitiser plus
//! this port's Linux layer (NUL, and the names `.` and `..`).
//!
//! LINUX DECISION: md.win's Windows layer — control characters, trailing dots and spaces, the
//! reserved device names — is not a rule here, so `CON` stays `CON.pdf` (md.win: `CON-.pdf`), as it
//! does on the Mac. The diagram name keeps md.win's order all the same (family stem, then the
//! suffix, then the platform layer), which is what keeps `CON-1.svg` free of a doubled dash on a
//! port that does escape device names.

use md_core::export::{export_file_names, text_bundle};

use crate::seams::FileTypeChoice;
use crate::strings::exports;

pub const PDF_EXTENSION: &str = ".pdf";
pub const HTML_EXTENSION: &str = ".html";
pub const EPUB_EXTENSION: &str = ".epub";
pub const LATEX_EXTENSION: &str = ".tex";
pub const SVG_EXTENSION: &str = ".svg";
pub const MARKDOWN_EXTENSION: &str = ".md";

/// One labelled type row — every export dialog offers exactly one.
pub fn choices(label: &str, extension: &str) -> Vec<FileTypeChoice> {
    vec![FileTypeChoice { label: label.to_string(), extensions: vec![extension.to_string()] }]
}

pub fn pdf_choices() -> Vec<FileTypeChoice> {
    choices(exports::PDF, PDF_EXTENSION)
}

pub fn html_choices() -> Vec<FileTypeChoice> {
    choices(exports::HTML, HTML_EXTENSION)
}

pub fn epub_choices() -> Vec<FileTypeChoice> {
    choices(exports::EPUB, EPUB_EXTENSION)
}

pub fn latex_choices() -> Vec<FileTypeChoice> {
    choices(exports::LATEX, LATEX_EXTENSION)
}

pub fn svg_choices() -> Vec<FileTypeChoice> {
    choices(exports::SVG, SVG_EXTENSION)
}

pub fn pdf(title: &str) -> String {
    export_file_names::sanitized(title, PDF_EXTENSION)
}

pub fn html(title: &str) -> String {
    export_file_names::sanitized(title, HTML_EXTENSION)
}

pub fn epub(title: &str) -> String {
    export_file_names::sanitized(title, EPUB_EXTENSION)
}

pub fn latex(title: &str) -> String {
    export_file_names::sanitized(title, LATEX_EXTENSION)
}

/// The Markdown name of a document's source — md.win's temp copy for Share ▸ Source…, which Linux
/// does not have; kept as the family's name for a `.md` copy of a title.
pub fn source(title: &str) -> String {
    export_file_names::sanitized(title, MARKDOWN_EXTENSION)
}

/// One diagram: `{sanitised title}-{ordinal + 1}.svg`, the ordinal made 1-based for the reader, as
/// the Mac writes it. The family stem comes first, the suffix next, and the platform layer last.
pub fn diagram_svg(title: &str, ordinal: usize) -> String {
    export_file_names::sanitized(&format!("{}-{}", export_file_names::portable_stem(title), ordinal + 1), SVG_EXTENSION)
}

/// The TextBundle is a folder, so this is a folder name, not a file name (§7.7).
pub fn text_bundle_folder(title: &str) -> String {
    export_file_names::sanitized(title, text_bundle::BUNDLE_EXTENSION)
}
