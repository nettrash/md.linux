//! `WebServices` — the process's web plumbing (shell-design §4.1): the global `webkit::WebContext`
//! with the `md-asset:` scheme registered once (secure + CORS, never local), one ephemeral
//! `webkit::NetworkSession` every `WebView` shares, the `webkit::Settings` factory, and the
//! bubblewrap preflight before the first view.
//!
//! The S0 stub is the spike's proven setup (docs/linux-facts.md §0, §3), created lazily on the first
//! view so a window without a preview never starts WebKit.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P4

use std::cell::OnceCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::services::paths::Paths;
use crate::web::asset_host::AssetHost;

/// The web context, the session and the asset host, one per process.
pub struct WebServices {
    asset_host: Rc<AssetHost>,
    /// `$XDG_RUNTIME_DIR/md`: the PDF temp files (shell-design §7.3).
    temp_dir: PathBuf,
    engine: OnceCell<(webkit::WebContext, webkit::NetworkSession)>,
}

impl WebServices {
    pub fn new(paths: &Paths) -> Rc<WebServices> {
        Rc::new(WebServices {
            asset_host: AssetHost::new(paths.rich_dir()),
            temp_dir: paths.runtime_dir().to_path_buf(),
            engine: OnceCell::new(),
        })
    }

    /// A `WebView` on the shared context and session with md's settings and `user_content`.
    pub fn new_view(&self, user_content: &webkit::UserContentManager) -> webkit::WebView {
        let (context, session) = self.engine.get_or_init(|| {
            // The inherent `default()` is the global context (an Option); registrations leak
            // within a process, so the scheme is registered exactly once (facts §3).
            let context = webkit::WebContext::default().unwrap_or_default();
            self.asset_host.attach(&context);
            (context, webkit::NetworkSession::new_ephemeral())
        });
        webkit::WebView::builder()
            .web_context(context)
            .network_session(session)
            .user_content_manager(user_content)
            .settings(&settings())
            .build()
    }

    pub fn asset_host(&self) -> Rc<AssetHost> {
        Rc::clone(&self.asset_host)
    }

    /// A file under `rich/`, through the asset host's containment (HTML export's KaTeX inlining).
    pub fn read_rich_asset(&self, rel: &str) -> Option<Vec<u8>> {
        self.asset_host.read_rich(rel)
    }
}

/// md's `webkit::Settings` (the spike's; shell-design §4.1 lists the full set).
pub fn settings() -> webkit::Settings {
    let settings = webkit::Settings::new();
    settings.set_enable_developer_extras(cfg!(debug_assertions));
    settings.set_enable_write_console_messages_to_stdout(cfg!(debug_assertions));
    settings.set_javascript_can_open_windows_automatically(false);
    settings.set_enable_back_forward_navigation_gestures(false);
    settings.set_enable_webaudio(false);
    settings.set_enable_webrtc(false);
    settings.set_enable_media_stream(false);
    settings.set_print_backgrounds(true);
    settings
}
