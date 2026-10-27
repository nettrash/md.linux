//! Port of md.win `tests/Md.App.Logic.Tests/Preview/AssetMimeTests.cs`: the Mac scheme handler's
//! table, row for row (§4.2), plus what `rich/` actually holds.

use std::path::{Path, PathBuf};

use md_logic::preview::asset_mime::{FALLBACK, mime_for};
use md_logic::preview::asset_origin::RICH_FOLDER_NAME;

const ROWS: [(&str, &str); 9] = [
    ("js", "text/javascript"),
    ("mjs", "text/javascript"),
    ("css", "text/css"),
    ("html", "text/html"),
    ("json", "application/json"),
    ("svg", "image/svg+xml"),
    ("woff2", "font/woff2"),
    ("woff", "font/woff"),
    ("ttf", "font/ttf"),
];

#[test]
fn every_row_of_the_mac_table() {
    for (extension, mime) in ROWS {
        assert_eq!(mime_for(extension), mime);
        assert_eq!(mime_for(&format!(".{extension}")), mime);
        assert_eq!(mime_for(&format!("rich/thing.{extension}")), mime);
        // md.win's Windows install path, kept: the rule only reads the extension.
        assert_eq!(mime_for(&format!("C:\\Program Files\\md\\web\\rich\\thing.{extension}")), mime);
        // LINUX: and where the .deb puts the engines.
        assert_eq!(mime_for(&format!("/usr/share/md/rich/thing.{extension}")), mime);
    }
}

#[test]
fn every_kind_of_file_rich_actually_holds() {
    for (path, mime) in [
        ("rich/md-init.js", "text/javascript"),
        ("rich/plantuml.js", "text/javascript"),
        ("rich/katex.min.css", "text/css"),
        ("rich/fonts/KaTeX_Main-Regular.woff2", "font/woff2"),
        ("rich/fonts/KaTeX_Main-Regular.woff", "font/woff"),
        ("rich/fonts/KaTeX_Main-Regular.ttf", "font/ttf"),
    ] {
        assert_eq!(mime_for(path), mime, "{path}");
    }
}

#[test]
fn modules_must_not_fall_to_octet_stream() {
    // A module fetched as application/octet-stream is rejected outright, which would take
    // md-init.js's dynamic import('./plantuml.js') with it.
    assert_eq!(mime_for("rich/md-init.js"), "text/javascript");
}

#[test]
fn extensions_are_matched_case_insensitively() {
    for path in ["JS", ".Woff2", "rich/KATEX.MIN.CSS"] {
        assert_ne!(mime_for(path), FALLBACK, "{path}");
    }
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("readable rich/") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            files(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn every_file_the_app_actually_ships_in_rich_gets_a_real_type() {
    // The rows above are the Mac's table; this one is the package. If someone vendors an engine
    // with an extension the table has no row for, the handler would serve it as
    // application/octet-stream — and a module or a font served that way is rejected outright.
    // LINUX: this repo's own `rich/` (byte-identical to md.win's `src/Md.App/rich`), which the .deb
    // installs and the `md-asset:` handler serves.
    let rich = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(RICH_FOLDER_NAME);
    assert!(rich.is_dir(), "{} is missing: the engines are the preview", rich.display());

    let mut all = Vec::new();
    files(&rich, &mut all);
    let mut unmapped: Vec<String> = all
        .iter()
        .filter(|f| mime_for(&f.to_string_lossy()) == FALLBACK)
        .map(|f| f.strip_prefix(&rich).unwrap_or(f).to_string_lossy().into_owned())
        .collect();
    unmapped.sort();
    assert!(unmapped.is_empty(), "served as {FALLBACK}: {unmapped:?}");
    assert!(!all.is_empty());
}

#[test]
fn everything_else_is_octet_stream_exactly_as_on_the_mac() {
    // md.win's sixth row is `null`: a Rust `&str` cannot be one, and "" is the same answer.
    for path in ["rich/logo.png", "rich/engine.wasm", "rich/data.bin", "rich/LICENSE", ""] {
        assert_eq!(mime_for(path), "application/octet-stream", "{path}");
    }
}
