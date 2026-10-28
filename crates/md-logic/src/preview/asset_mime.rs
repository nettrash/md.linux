//! Port of md.win `Preview/AssetMime.cs`: the Mac scheme handler's MIME table, to the row (§4.2,
//! rich.md §4.1).

/// What an extension this table does not know is served as.
pub const FALLBACK: &str = "application/octet-stream";

/// The type for `path_or_extension` — a file name, a path, or a bare extension with or without its
/// dot. Case-insensitive.
///
/// LINUX DECISION: on Windows this table is a written-and-off contingency (Chromium infers the type
/// of a folder-mapped file itself). WebKitGTK sniffs nothing for a custom scheme: the `md-asset:`
/// handler answers every request with exactly the type it is given, and a module script served as
/// anything but JavaScript is refused — so on Linux every `rich/…` response goes through this
/// table. Deliberately the Mac's nine rows and no more: a row this table does not have (a `.png`, a
/// `.wasm`) falls to [`FALLBACK`] exactly as on macOS and iOS. Add a row here and in the Mac
/// handler together, or the ports drift.
pub fn mime_for(path_or_extension: &str) -> &'static str {
    match extension(path_or_extension).as_str() {
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "html" => "text/html",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        _ => FALLBACK,
    }
}

/// The last dot-separated segment of the last path segment, lower-cased; empty when there is none.
/// Both `/` and `\` separate segments, as in md.win (a Linux path has no `\`, and only the
/// extension is ever read). md.win lower-cases with `ToLowerInvariant`; ASCII lower-casing gives the
/// same answer for this table, because the only non-ASCII letters whose simple lower case is ASCII
/// are U+0130 (→ `i`) and U+212A KELVIN SIGN (→ `k`), and no row has an `i` or a `k`.
fn extension(path_or_extension: &str) -> String {
    let name = match path_or_extension.rfind(['/', '\\']) {
        Some(slash) => &path_or_extension[slash + 1..],
        None => path_or_extension,
    };
    let extension = match name.rfind('.') {
        Some(dot) => &name[dot + 1..],
        None => name,
    };
    extension.to_ascii_lowercase()
}
