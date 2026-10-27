//! The `md-asset:` origin (shell-design §4.2): `md-asset://<host>/index.html` is served from memory
//! and `md-asset://<host>/rich/…` from the bundled engines, so the generated HTML's relative `rich/…`
//! references, `import('./plantuml.js')` inside md-init.js and `url(fonts/…)` inside katex.min.css
//! all resolve unchanged — md.win's `https://md.assets/` design on WebKitGTK.
//!
//! Each surface (a document's preview, an export renderer) gets its own host name, which is also
//! its own origin; the handler looks the page up by host. `rich/` is the only directory a page can
//! reach and `index.html` never touches the disk. The URL shape, the containment rule, the MIME
//! table and the Content-Security-Policy are md-logic's (`preview::asset_origin`,
//! `preview::asset_mime`); this file is the WebKit plumbing around them and the one decision they
//! leave to the handler: what each request is answered with ([`AssetHost::answer`], pure and
//! unit-tested).
//!
//! The spike's handler (docs/linux-facts.md §3), kept: registered once on the one context, secure +
//! CORS-enabled, never `local` (which breaks the module scripts); every response
//! `Cache-Control: no-store`, so `reload()` re-requests `index.html` and picks up the new page; the
//! CSP on `index.html` only.
// P4 → P1 / P5: until the windows construct previews and exports, only the self-test reaches this;
// removed at the integration gate.
#![cfg_attr(not(feature = "selftest"), allow(dead_code))]

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use md_logic::preview::asset_mime;
use md_logic::preview::asset_origin::{self, CONTENT_SECURITY_POLICY, INDEX_PATH, RICH_FOLDER_NAME, SCHEME};

/// What one request is answered with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub status: u32,
    pub mime: &'static str,
    pub body: Vec<u8>,
    /// Whether the response carries [`CONTENT_SECURITY_POLICY`] (`index.html` only).
    pub csp: bool,
}

impl Answer {
    fn not_found() -> Answer {
        Answer { status: 404, mime: "text/plain", body: Vec::new(), csp: false }
    }
}

/// One answered request, for the self-test.
#[cfg(feature = "selftest")]
#[derive(Clone, Debug)]
pub struct Served {
    pub uri: String,
    pub status: u32,
    pub csp: bool,
}

/// The pages of every live surface and the engines folder they share.
pub struct AssetHost {
    rich_dir: PathBuf,
    pages: RefCell<HashMap<String, String>>,
    #[cfg(feature = "selftest")]
    log: RefCell<Vec<Served>>,
}

impl AssetHost {
    pub fn new(rich_dir: PathBuf) -> Rc<AssetHost> {
        Rc::new(AssetHost {
            rich_dir,
            pages: RefCell::new(HashMap::new()),
            #[cfg(feature = "selftest")]
            log: RefCell::new(Vec::new()),
        })
    }

    /// What `md-asset://<host>/index.html` serves from now on.
    pub fn set_page(&self, host: &str, html: String) {
        self.pages.borrow_mut().insert(host.to_owned(), html);
    }

    /// The page `host` serves, if any.
    pub fn page(&self, host: &str) -> Option<String> {
        self.pages.borrow().get(host).cloned()
    }

    /// The surface is gone: its host serves nothing any more.
    pub fn remove_page(&self, host: &str) {
        self.pages.borrow_mut().remove(host);
    }

    /// Every request answered since the last call, oldest first.
    #[cfg(feature = "selftest")]
    pub fn take_log(&self) -> Vec<Served> {
        std::mem::take(&mut *self.log.borrow_mut())
    }

    /// Registers the scheme on `context` — once per process — as secure and CORS-enabled, never as
    /// local (facts §3: registrations leak within a process, and `local` breaks module scripts).
    pub fn attach(self: &Rc<Self>, context: &webkit::WebContext) {
        let host = Rc::clone(self);
        context.register_uri_scheme(SCHEME, move |request| host.serve(request));
        if let Some(security) = context.security_manager() {
            security.register_uri_scheme_as_secure(SCHEME);
            security.register_uri_scheme_as_cors_enabled(SCHEME);
        }
    }

    /// A file under `rich/` by its path relative to that folder, contained; `None` when it is
    /// outside, missing or unreadable.
    pub fn read_rich(&self, relative: &str) -> Option<Vec<u8>> {
        if !asset_origin::is_contained(relative) {
            return None;
        }
        std::fs::read(self.rich_dir.join(relative)).ok()
    }

    /// The answer to `uri`: a surface's `index.html` from memory with the CSP, a contained `rich/`
    /// file with md-logic's MIME type, and an empty 404 for everything else.
    pub fn answer(&self, uri: &str) -> Answer {
        let Some((host, path)) = asset_origin::split_uri(uri) else { return Answer::not_found() };
        if path == INDEX_PATH {
            return match self.pages.borrow().get(host) {
                Some(html) => Answer { status: 200, mime: "text/html", body: html.clone().into_bytes(), csp: true },
                None => Answer::not_found(),
            };
        }
        let Some(relative) = path.strip_prefix(RICH_FOLDER_NAME).and_then(|rest| rest.strip_prefix('/')) else {
            return Answer::not_found();
        };
        match self.read_rich(relative) {
            Some(body) => Answer { status: 200, mime: asset_mime::mime_for(relative), body, csp: false },
            None => Answer::not_found(),
        }
    }

    fn serve(&self, request: &webkit::URISchemeRequest) {
        let uri = request.uri().map(|u| u.to_string()).unwrap_or_default();
        let answer = self.answer(&uri);
        #[cfg(feature = "selftest")]
        self.log.borrow_mut().push(Served { uri: uri.clone(), status: answer.status, csp: answer.csp });

        let bytes = glib::Bytes::from_owned(answer.body);
        let length = bytes.len() as i64;
        let stream = gio::MemoryInputStream::from_bytes(&bytes);
        let response = webkit::URISchemeResponse::new(&stream, length);
        response.set_status(answer.status, Some(if answer.status == 200 { "OK" } else { "Not Found" }));
        response.set_content_type(answer.mime);
        let headers = webkit::soup::MessageHeaders::new(webkit::soup::MessageHeadersType::Response);
        headers.append("Cache-Control", "no-store");
        if answer.csp {
            headers.append("Content-Security-Policy", CONTENT_SECURITY_POLICY);
        }
        response.set_http_headers(headers);
        request.finish_with_response(&response);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway `rich/` with two files, removed when dropped.
    struct RichDir(PathBuf);

    impl RichDir {
        fn new(name: &str) -> RichDir {
            let dir = std::env::temp_dir().join(format!("md-asset-host-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("fonts")).unwrap();
            std::fs::write(dir.join("md-init.js"), b"export {};").unwrap();
            std::fs::write(dir.join("fonts/KaTeX_Main-Regular.woff2"), b"wOF2").unwrap();
            RichDir(dir)
        }
    }

    impl Drop for RichDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_csp_header_is_the_one_the_design_measured() {
        assert_eq!(
            CONTENT_SECURITY_POLICY,
            "default-src 'self' data: blob:; script-src 'self' 'unsafe-eval' 'wasm-unsafe-eval'; \
             style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https: http:; font-src 'self' data:"
        );
        assert!(!CONTENT_SECURITY_POLICY.contains("script-src 'self' 'unsafe-inline'"));
    }

    #[test]
    fn each_host_serves_its_own_index_with_the_csp() {
        let rich = RichDir::new("index");
        let host = AssetHost::new(rich.0.clone());
        host.set_page("preview-1", "<p>one</p>".into());
        host.set_page("export-2", "<p>two</p>".into());
        let one = host.answer("md-asset://preview-1/index.html");
        assert_eq!((one.status, one.mime, one.csp), (200, "text/html", true));
        assert_eq!(one.body, b"<p>one</p>");
        assert_eq!(host.answer("md-asset://export-2/index.html#slug").body, b"<p>two</p>");
        assert_eq!(host.answer("md-asset://preview-9/index.html").status, 404);
        host.remove_page("preview-1");
        assert_eq!(host.answer("md-asset://preview-1/index.html").status, 404);
        assert_eq!(host.page("export-2").as_deref(), Some("<p>two</p>"));
    }

    #[test]
    fn rich_files_are_served_contained_with_the_macs_types_and_no_csp() {
        let rich = RichDir::new("rich");
        let host = AssetHost::new(rich.0.clone());
        let js = host.answer("md-asset://preview-1/rich/md-init.js");
        assert_eq!((js.status, js.mime, js.csp), (200, "text/javascript", false));
        assert_eq!(js.body, b"export {};");
        let font = host.answer("md-asset://p/rich/fonts/KaTeX_Main-Regular.woff2?v=1");
        assert_eq!((font.status, font.mime), (200, "font/woff2"));
        for escape in [
            "md-asset://p/rich/../Cargo.toml",
            "md-asset://p/rich/fonts/../../x",
            "md-asset://p/rich/%2e%2e/x",
            "md-asset://p/rich//md-init.js",
            "md-asset://p/rich/",
            "md-asset://p/richmd-init.js",
            "md-asset://p/other.md",
            "md-asset://p/md-init.js",
            "https://p/rich/md-init.js",
            "md-asset://hostonly",
        ] {
            let answer = host.answer(escape);
            assert_eq!((answer.status, answer.body.len(), answer.csp), (404, 0, false), "{escape}");
        }
        assert_eq!(host.read_rich("md-init.js").as_deref(), Some(&b"export {};"[..]));
        assert_eq!(host.read_rich("../md-init.js"), None);
    }

    #[test]
    fn origin_helpers_are_md_logics() {
        assert_eq!(asset_origin::index_url("preview-1"), "md-asset://preview-1/index.html");
        assert_eq!(asset_origin::split_uri("md-asset://preview-1/index.html#intro"), Some(("preview-1", "index.html")));
        assert_eq!(asset_mime::mime_for("katex.min.css"), "text/css");
        assert_eq!(asset_mime::mime_for("README"), "application/octet-stream");
    }
}
