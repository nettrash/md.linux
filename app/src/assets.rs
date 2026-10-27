//! The `md-asset:` origin: `md-asset://<surface>/index.html` is served from memory and
//! `md-asset://<surface>/rich/…` from the bundled engines, so the generated HTML's relative
//! `rich/…` references, `import('./plantuml.js')` inside md-init.js and `url(fonts/…)` inside
//! katex.min.css all resolve unchanged — the md.win `https://md.assets/` design on WebKitGTK.
//!
//! Each surface (a document's preview, an export renderer) gets its own host name, which is
//! also its own origin; the handler looks the page up by host.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const SCHEME: &str = "md-asset";

/// The Content-Security-Policy the spike tried on every response (see docs/linux-facts.md §3):
/// no `'unsafe-inline'`, so `javascript:` URLs and inline handlers are blocked by the engine.
pub const CSP: &str = "default-src 'self' data: blob:; script-src 'self' 'unsafe-eval' 'wasm-unsafe-eval'; \
     style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https: http:; font-src 'self' data:";

/// How the scheme is registered with the context's `WebKitSecurityManager`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SchemeSecurity {
    pub secure: bool,
    pub cors: bool,
    pub local: bool,
    pub csp: bool,
}

impl SchemeSecurity {
    /// What the real shell should use (measured: docs/linux-facts.md §3).
    pub const SHELL: SchemeSecurity = SchemeSecurity { secure: true, cors: true, local: false, csp: false };
}

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
    csp: std::cell::Cell<bool>,
}

impl AssetHost {
    pub fn new(rich_dir: PathBuf) -> Rc<AssetHost> {
        Rc::new(AssetHost {
            rich_dir,
            pages: RefCell::new(HashMap::new()),
            log: RefCell::new(Vec::new()),
            csp: std::cell::Cell::new(false),
        })
    }

    /// `/usr/share/md/rich` in the .deb; `MD_RICH_DIR` overrides; the source tree's `rich/` in
    /// a development build.
    pub fn default_rich_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("MD_RICH_DIR") {
            return PathBuf::from(dir);
        }
        let installed = Path::new("/usr/share/md/rich");
        if installed.is_dir() {
            return installed.to_path_buf();
        }
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../rich")
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

    pub fn set_csp(&self, on: bool) {
        self.csp.set(on);
    }

    /// Registers the scheme on `ctx` (once per context) and its security flags.
    pub fn attach(self: &Rc<Self>, ctx: &webkit::WebContext, security: SchemeSecurity) {
        let host = Rc::clone(self);
        ctx.register_uri_scheme(SCHEME, move |req| host.serve(req));
        if let Some(sm) = ctx.security_manager() {
            if security.secure {
                sm.register_uri_scheme_as_secure(SCHEME);
            }
            if security.cors {
                sm.register_uri_scheme_as_cors_enabled(SCHEME);
            }
            if security.local {
                sm.register_uri_scheme_as_local(SCHEME);
            }
        }
        self.csp.set(security.csp);
    }

    fn serve(&self, req: &webkit::URISchemeRequest) {
        let uri = req.uri().map(|u| u.to_string()).unwrap_or_default();
        let (status, mime, body) = match split_uri(&uri) {
            Some((host, path)) => self.resolve(host, path),
            None => (404, "text/plain", Vec::new()),
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
        if self.csp.get() {
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
        if !is_contained(rest) {
            return (404, "text/plain", Vec::new());
        }
        match std::fs::read(self.rich_dir.join(rest)) {
            Ok(bytes) => (200, mime_for(rest), bytes),
            Err(_) => (404, "text/plain", Vec::new()),
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
/// percent-escape (none of the engines' files needs one).
pub fn is_contained(rel: &str) -> bool {
    !rel.is_empty()
        && !rel.contains(['\\', '%', '\0'])
        && rel.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

/// The Mac's MIME table (md.win `AssetMime`): WebKit sniffs nothing for a custom scheme, and
/// a module script with the wrong type is refused.
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
