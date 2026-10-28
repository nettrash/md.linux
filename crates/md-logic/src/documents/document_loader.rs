//! Port of md.win `Documents/DocumentLoader.cs`.
//!
//! Opening one path (§6.1, §6.3, §6.5): decide what it is, read it, decode it. Pure over the
//! [`FileSystem`] seam so every branch — a legacy-encoded file, a pack whose zip is corrupt, a
//! `.textbundle` folder with only `text.markdown` in it — is testable without a disk.

use std::collections::HashMap;

use md_core::document::plain_text_codec::DecodedText;
use md_core::export::text_bundle;

use super::file_names;
use super::line_endings;
use super::rescue_copy::failure_message;
use super::text_file_dressing::TextFileDressing;
use crate::seams::{FileSystem, FileTypeChoice};
use crate::strings;

/// What was opened. The two bundle kinds import read-only (§6.5); a plain file is edited in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocumentKind {
    PlainText,
    TextPack,
    TextBundleFolder,
}

/// Why an open produced nothing — each maps to one alert (§6.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LoadFailure {
    #[default]
    None,
    /// Unreadable or undecodable: "The document could not be opened." plus the system's message.
    Unreadable,
    /// Not a readable pack/bundle: "The TextPack could not be read." Zip bytes are never fed to
    /// the text decoder.
    BundleUnreadable,
}

/// A file read and decoded, ready for the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedDocument {
    /// Plain text edits in place; a bundle opens untitled, titled after the bundle.
    pub kind: DocumentKind,
    /// The file or bundle folder that was read.
    pub path: String,
    /// LF, as the model wants it.
    pub text: String,
    /// Encoding, BOM and newline to write back with (a bundle is never written back; its dressing
    /// seeds the untitled document's).
    pub dressing: TextFileDressing,
    /// The window title: a file's stem, a bundle's stem.
    pub title: String,
}

/// The outcome of one open: a document, or a failure with the text to show beside its title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentLoad {
    pub document: Option<LoadedDocument>,
    pub failure: LoadFailure,
    pub error_text: String,
}

impl DocumentLoad {
    /// The alert's title, or `None` when nothing failed.
    pub fn alert_title(&self) -> Option<&'static str> {
        match self.failure {
            LoadFailure::None => None,
            LoadFailure::BundleUnreadable => Some(strings::documents::TEXT_PACK_UNREADABLE),
            LoadFailure::Unreadable => Some(strings::documents::COULD_NOT_OPEN),
        }
    }

    fn failed(failure: LoadFailure, error_text: String) -> Self {
        DocumentLoad { document: None, failure, error_text }
    }

    fn loaded(document: LoadedDocument) -> Self {
        DocumentLoad { document: Some(document), failure: LoadFailure::None, error_text: String::new() }
    }
}

/// The extensions md owns: the family's canonical Markdown set, in the order every port declares
/// it. The Linux packaging's `text/markdown` association lists exactly these, and the
/// file-association tests pin the two to each other.
pub const MARKDOWN_EXTENSIONS: [&str; 10] =
    [".md", ".markdown", ".mdown", ".markdn", ".mdtext", ".mdtxt", ".mkd", ".mkdn", ".mdwn", ".mkdown"];

/// Plain-text alternates.
pub const PLAIN_TEXT_EXTENSIONS: [&str; 2] = [".txt", ".text"];

/// PlantUML alternates.
pub const PLANT_UML_EXTENSIONS: [&str; 4] = [".puml", ".plantuml", ".iuml", ".pu"];

/// Graphviz alternates. Only `.gv`: like macOS and Windows, md never claims `.dot`, which is also
/// a Word template.
pub const GRAPHVIZ_EXTENSIONS: [&str; 1] = [".gv"];

/// What the Open dialog filters on (§6.1): every writable type, grouped in the Mac's order, plus
/// `.textpack` (md-core's `text_bundle::PACK_EXTENSION`) — every extension the packaging
/// associates and nothing else. Never `.textbundle`: that is a folder, which a file chooser in
/// file mode cannot pick (§6.5).
pub const OPEN_EXTENSIONS: [&str; 18] = [
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
];

/// The Save dialog's labelled type groups, in the Mac's order (§6.1). Never a bundle type: saving
/// a bundle back would drop its `assets/`.
pub const SAVE_CHOICES: [(&str, &[&str]); 4] = [
    (strings::exports::MARKDOWN_DOCUMENT, &MARKDOWN_EXTENSIONS),
    (strings::exports::PLAIN_TEXT, &PLAIN_TEXT_EXTENSIONS),
    (strings::exports::PLANT_UML_DIAGRAM, &PLANT_UML_EXTENSIONS),
    (strings::exports::GRAPHVIZ_DOT_GRAPH, &GRAPHVIZ_EXTENSIONS),
];

/// [`OPEN_EXTENSIONS`] in the shape [`crate::seams::Pickers::open_files`] takes.
pub fn open_extensions() -> Vec<String> {
    OPEN_EXTENSIONS.iter().map(|e| e.to_string()).collect()
}

/// [`SAVE_CHOICES`] in the shape [`crate::seams::Pickers::save_file`] takes.
pub fn save_file_type_choices() -> Vec<FileTypeChoice> {
    SAVE_CHOICES
        .iter()
        .map(|(label, extensions)| FileTypeChoice {
            label: label.to_string(),
            extensions: extensions.iter().map(|e| e.to_string()).collect(),
        })
        .collect()
}

/// The extension a Save As should default to for `path` — its own, or `.md` for an untitled or
/// extension-less document.
pub fn default_extension_for(path: Option<&str>) -> String {
    let ext = path.map(file_names::extension_of).unwrap_or_default();
    if ext.is_empty() { ".md".to_string() } else { ext }
}

/// Read and decode. A directory named `*.textbundle` imports as a bundle; bytes that
/// `text_bundle::looks_like_pack` claims import as a pack — never through the text decoder, which
/// ISO-8859-1 would let "succeed" on zip bytes and bake mojibake into the next save.
pub fn load(path: &str, file_system: &dyn FileSystem) -> DocumentLoad {
    let name = file_names::name_of(path);
    if ends_with_ignoring_ascii_case(name, text_bundle::BUNDLE_EXTENSION) && file_system.directory_exists(path) {
        return load_bundle_folder(path, file_system);
    }
    let bytes = match file_system.read_all_bytes(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return DocumentLoad::failed(
                LoadFailure::Unreadable,
                failure_message(&e, strings::documents::COULD_NOT_OPEN),
            );
        }
    };
    if text_bundle::looks_like_pack(name, &bytes) {
        return load_pack(path, &bytes);
    }
    let Some((text, dressing)) = TextFileDressing::undress(&bytes) else {
        return DocumentLoad::failed(LoadFailure::Unreadable, strings::documents::COULD_NOT_OPEN.to_string());
    };
    DocumentLoad::loaded(LoadedDocument {
        kind: DocumentKind::PlainText,
        path: path.to_string(),
        text,
        dressing,
        title: file_names::stem_of(path).to_string(),
    })
}

fn load_pack(path: &str, bytes: &[u8]) -> DocumentLoad {
    match text_bundle::text_from_pack(bytes) {
        Some(decoded) => imported(DocumentKind::TextPack, path, decoded),
        None => DocumentLoad::failed(LoadFailure::BundleUnreadable, String::new()),
    }
}

/// The map overload, not md-core's `text_from_bundle_directory`: the folder is listed through the
/// seam so the branch is testable off a disk. Only `text.*` is read — the assets are neither shown
/// (§6.5) nor small, and a bundle may declare a lot of them.
fn load_bundle_folder(path: &str, file_system: &dyn FileSystem) -> DocumentLoad {
    let listing = match file_system.enumerate_entries(path) {
        Ok(listing) => listing,
        Err(e) => {
            return DocumentLoad::failed(
                LoadFailure::Unreadable,
                failure_message(&e, strings::documents::COULD_NOT_OPEN),
            );
        }
    };
    let mut children: HashMap<String, Vec<u8>> = HashMap::new();
    for entry in listing {
        let child = file_names::name_of(&entry);
        if !child.starts_with("text.") || !file_system.file_exists(&entry) || children.contains_key(child) {
            continue;
        }
        match file_system.read_all_bytes(&entry) {
            Ok(bytes) => {
                children.insert(child.to_string(), bytes);
            }
            Err(e) => {
                return DocumentLoad::failed(
                    LoadFailure::Unreadable,
                    failure_message(&e, strings::documents::COULD_NOT_OPEN),
                );
            }
        }
    }
    match text_bundle::text_from_bundle(&children) {
        Some(decoded) => imported(DocumentKind::TextBundleFolder, path, decoded),
        None => DocumentLoad::failed(LoadFailure::BundleUnreadable, String::new()),
    }
}

fn imported(kind: DocumentKind, path: &str, decoded: DecodedText) -> DocumentLoad {
    // The bundle's own newline convention seeds the untitled document, so Save As writes the file
    // the writer's other tools produced; the BOM does not travel (the bytes we would write are a
    // new .md, and md-core's own bundle writer emits none).
    let dressing = TextFileDressing::new(decoded.encoding, false, line_endings::detect(&decoded.text));
    DocumentLoad::loaded(LoadedDocument {
        kind,
        path: path.to_string(),
        text: line_endings::normalize(&decoded.text),
        dressing,
        title: file_names::stem_of(path).to_string(),
    })
}

/// md.win's `EndsWith(suffix, OrdinalIgnoreCase)` for an ASCII suffix. Folding ASCII only is exact
/// here: no other character's simple upper case is a letter of `.textbundle` or `.textpack`
/// (md-core's `looks_like_pack` relies on the same fact).
pub(crate) fn ends_with_ignoring_ascii_case(name: &str, suffix: &str) -> bool {
    let (name, suffix) = (name.as_bytes(), suffix.as_bytes());
    name.len() >= suffix.len() && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}
