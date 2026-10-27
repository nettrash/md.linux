//! The `md-asset:` origin (shell-design §4.2): `md-asset://<host>/index.html` is served from memory
//! and `md-asset://<host>/rich/…` from the bundled engines, so the generated HTML's relative `rich/…`
//! references, `import('./plantuml.js')` inside md-init.js and `url(fonts/…)` inside katex.min.css
//! all resolve unchanged — md.win's `https://md.assets/` design on WebKitGTK.
//!
//! Each surface (a document's preview, an export renderer) gets its own host name, which is also
//! its own origin; the handler looks the page up by host.
//!
//! S0 moved the spike's `assets.rs` here as the starting point (docs/linux-facts.md §3): the probe's
//! registration switches are gone (secure + CORS is what was measured to work); the CSP header is
//! sent on `index.html` as the design says.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P4

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

pub const SCHEME: &str = "md-asset";

/// The Content-Security-Policy on every `index.html` (facts §3): no `'unsafe-inline'` for scripts,
/// so `javascript:` URLs and inline handlers are blocked by the engine.
pub const CSP: &str = "default-src 'self' data: blob:; script-src 'self' 'unsafe-eval' 'wasm-unsafe-eval'; \
     style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https: http:; font-src 'self' data:";

/// One answered request, for the self-test.
#[derive(Debug)]
pub struct Served {
    pub path: String,
    pub status: u32,
    pub mime: &'static str,
}

pub struct AssetHost {
    rich_dir: PathBuf,
    pages: RefCell<HashMap<String, String>>,
    log: RefCell<Vec<Served>>,
}

impl AssetHost {
    pub fn new(rich_dir: PathBuf) -> Rc<AssetHost> {
        Rc::new(AssetHost { rich_dir, pages: RefCell::new(HashMap::new()), log: RefCell::new(Vec::new()) })
    }

    pub fn index_url(host: &str) -> String {
        format!("{SCHEME}://{host}/index.html")
    }

    pub fn set_page(&self, host: &str, html: String) {
        self.pages.borrow_mut().insert(host.to_owned(), html);
    }

    pub fn take_log(&self) -> Vec<Served> {
        std::mem::take(&mut *self.log.borrow_mut())
    }

    /// Registers the scheme on `ctx` (once per process) as secure and CORS-enabled — never as
    /// local, which breaks the module scripts (facts §3).
    pub fn attach(self: &Rc<Self>, ctx: &webkit::WebContext) {
        let host = Rc::clone(self);
        ctx.register_uri_scheme(SCHEME, move |req| host.serve(req));
        if let Some(sm) = ctx.security_manager() {
            sm.register_uri_scheme_as_secure(SCHEME);
            sm.register_uri_scheme_as_cors_enabled(SCHEME);
        }
    }

    /// A file under `rich/`, contained.
    pub fn read_rich(&self, rel: &str) -> Option<Vec<u8>> {
        if !is_contained(rel) {
            return None;
        }
        std::fs::read(self.rich_dir.join(rel)).ok()
    }

    fn serve(&self, req: &webkit::URISchemeRequest) {
        let uri = req.uri().map(|u| u.to_string()).unwrap_or_default();
        let (status, mime, body, is_index) = match split_uri(&uri) {
            Some((host, path)) => {
                let (status, mime, body) = self.resolve(host, path);
                (status, mime, body, path == "index.html")
            }
            None => (404, "text/plain", Vec::new(), false),
        };
        self.log.borrow_mut().push(Served { path: uri, status, mime });
        let bytes = glib::Bytes::from_owned(body);
        let len = bytes.len() as i64;
        let stream = gio::MemoryInputStream::from_bytes(&bytes);
        let response = webkit::URISchemeResponse::new(&stream, len);
        response.set_status(status, Some(if status == 200 { "OK" } else { "Not Found" }));
        response.set_content_type(mime);
        let headers = webkit::soup::MessageHeaders::new(webkit::soup::MessageHeadersType::Response);
        headers.append("Cache-Control", "no-store");
        if is_index {
            headers.append("Content-Security-Policy", CSP);
        }
        response.set_http_headers(headers);
        req.finish_with_response(&response);
    }

    fn resolve(&self, host: &str, path: &str) -> (u32, &'static str, Vec<u8>) {
        if path == "index.html" {
            return match self.pages.borrow().get(host) {
                Some(html) => (200, "text/html", html.clone().into_bytes()),
                None => (404, "text/plain", Vec::new()),
            };
        }
        let Some(rest) = path.strip_prefix("rich/") else {
            return (404, "text/plain", Vec::new());
        };
        match self.read_rich(rest) {
            Some(bytes) => (200, mime_for(rest), bytes),
            None => (404, "text/plain", Vec::new()),
        }
    }
}

/// `md-asset://host/a/b?q#f` → (`host`, `a/b`).
pub fn split_uri(uri: &str) -> Option<(&str, &str)> {
    let rest = uri.strip_prefix(SCHEME)?.strip_prefix("://")?;
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    let rest = &rest[..end];
    let slash = rest.find('/')?;
    Some((&rest[..slash], &rest[slash + 1..]))
}

/// A relative path stays inside `rich/`: no empty, `.` or `..` segment, no backslash, no
/// percent-escape (none of the engines' files needs one), no NUL.
pub fn is_contained(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.contains(['\\', '%', '\0'])
        && rel.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

/// The Mac's MIME table (md.win `AssetMime`; md-logic `preview::asset_mime` is the shell's source):
/// WebKit sniffs nothing for a custom scheme, and a module script with the wrong type is refused.
pub fn mime_for(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "html" => "text/html",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_uri_strips_query_and_fragment() {
        assert_eq!(split_uri("md-asset://preview-1/index.html#intro"), Some(("preview-1", "index.html")));
        assert_eq!(split_uri("md-asset://p/rich/fonts/K.woff2?x=1"), Some(("p", "rich/fonts/K.woff2")));
        assert_eq!(split_uri("https://p/index.html"), None);
        assert_eq!(split_uri("md-asset://hostonly"), None);
    }

    #[test]
    fn containment_rejects_escapes() {
        assert!(is_contained("katex.min.js"));
        assert!(is_contained("fonts/KaTeX_Main-Regular.woff2"));
        assert!(!is_contained("../Cargo.toml"));
        assert!(!is_contained("fonts/../../x"));
        assert!(!is_contained("fonts//x"));
        assert!(!is_contained("%2e%2e/x"));
        assert!(!is_contained("a\0b"));
        assert!(!is_contained(""));
    }

    #[test]
    fn mime_table_is_the_macs() {
        assert_eq!(mime_for("md-init.js"), "text/javascript");
        assert_eq!(mime_for("x.MJS"), "text/javascript");
        assert_eq!(mime_for("katex.min.css"), "text/css");
        assert_eq!(mime_for("fonts/a.woff2"), "font/woff2");
        assert_eq!(mime_for("fonts/a.woff"), "font/woff");
        assert_eq!(mime_for("fonts/a.ttf"), "font/ttf");
        assert_eq!(mime_for("a.svg"), "image/svg+xml");
        assert_eq!(mime_for("README"), "application/octet-stream");
    }
}
