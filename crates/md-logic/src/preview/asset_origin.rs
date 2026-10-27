//! Port of md.win `Preview/AssetOrigin.cs`: the origin the preview and every export renderer load.
//!
//! LINUX DECISION: md.win serves one virtual host, `https://md.assets/`, through WebView2's
//! `WebResourceRequested`. WebKitGTK has no virtual hosts; the GTK shell registers a URI scheme,
//! `md-asset:`, once on the one `WebContext` (docs/linux-facts.md §3: secure + CORS-enabled, never
//! `local`), and each surface — a document's live preview, an export renderer — gets its **own
//! host**, which is also its own origin: `md-asset://preview-1/index.html`,
//! `md-asset://export-2/index.html`. The handler serves `index.html` from that surface's HTML in
//! memory and `rich/…` from the install's engines folder (`/usr/share/md/rich` in the .deb); the
//! document and the engines share the origin, which is what lets the generated HTML keep its
//! relative `rich/…` URLs, md-init.js's `import('./plantuml.js')` and katex.min.css's
//! `url(fonts/…)` unchanged. There is no `web` folder: nothing maps a directory, so nothing but
//! `rich/` and `index.html` can be fetched.

/// The URI scheme the shell registers.
pub const SCHEME: &str = "md-asset";

/// The document every surface serves from memory; `reload()` re-requests it and picks up the new
/// HTML (the handler answers with `Cache-Control: no-store`).
pub const INDEX_PATH: &str = "index.html";

/// The engines' folder, under the origin and under the install; the only prefix besides
/// [`INDEX_PATH`] the page ever asks for.
pub const RICH_FOLDER_NAME: &str = "rich";

/// The `Content-Security-Policy` header the handler may put on `index.html` (docs/linux-facts.md
/// §3): no `'unsafe-inline'` for scripts, so a `javascript:` URL and an inline handler are refused
/// by the engine itself while every engine still renders. The link guard is the parity measure;
/// this is the belt to its braces.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self' data: blob:; script-src 'self' 'unsafe-eval' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https: http:; font-src 'self' data:";

/// The document URL of the surface whose host is `host`: `md-asset://{host}/index.html`.
pub fn index_url(host: &str) -> String {
    format!("{SCHEME}://{host}/{INDEX_PATH}")
}

/// The host of the `n`th live preview: `preview-{n}`.
pub fn preview_host(n: u64) -> String {
    format!("preview-{n}")
}

/// The host of the `n`th export renderer: `export-{n}`.
pub fn export_host(n: u64) -> String {
    format!("export-{n}")
}

/// `md-asset://host/a/b?q#f` → (`host`, `a/b`): the surface and the path the scheme handler
/// resolves, query and fragment dropped. `None` for another scheme or a URI with no path.
pub fn split_uri(uri: &str) -> Option<(&str, &str)> {
    let rest = uri.strip_prefix(SCHEME)?.strip_prefix("://")?;
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    let rest = &rest[..end];
    let slash = rest.find('/')?;
    Some((&rest[..slash], &rest[slash + 1..]))
}

/// A path relative to `rich/` stays inside it: no empty, `.` or `..` segment, no backslash, no
/// percent-escape (none of the engines' files needs one), no NUL. The handler serves nothing that
/// fails this.
pub fn is_contained(relative: &str) -> bool {
    !relative.is_empty()
        && !relative.contains(['\\', '%', '\0'])
        && relative.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}
