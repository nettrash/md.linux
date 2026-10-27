//! `WebServices` — the process's web plumbing (shell-design §4.1): the global `webkit::WebContext`
//! with the `md-asset:` scheme registered once (secure + CORS, never local), one ephemeral
//! `webkit::NetworkSession` every `WebView` shares (cookies, caches and HSTS stay in memory, so md
//! writes nothing under `~/.local/share/md/webkitgtk` or `~/.cache/md/webkitgtk`), the
//! `webkit::Settings` factory, the surface host names, and the bubblewrap preflight before the
//! first view.
//!
//! Created in `startup` but lazy: WebKit is only started by the first [`WebServices::new_view`], so
//! a process that never shows a preview never starts a web process.
//!
//! **The sandbox preflight.** WebKit's web processes start inside bubblewrap; when bwrap cannot get
//! a user namespace (Ubuntu 26.04 restricts them to AppArmor profiles that allow it), WebKit
//! `g_error`s and the whole of md aborts without a word (docs/linux-facts.md §9). So before the
//! first view exists md runs `bwrap --unshare-user --ro-bind / / true` once itself (≈ 10 ms). If it
//! fails, no page is ever loaded (a load is what starts the web process) and md shows one alert
//! naming `/etc/apparmor.d/md` (docs/packaging.md §7), then quits. md never sets
//! `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS`; when someone else has (the Docker dev image), WebKit
//! starts no sandbox and there is nothing to probe. `MD_SKIP_SANDBOX_PROBE=1` skips the probe in
//! debug and self-test builds only — a release build ignores it.
// P4 → P1 / P5: until the windows construct previews and exports, only the self-test reaches this;
// removed at the integration gate.
#![cfg_attr(not(feature = "selftest"), allow(dead_code))]

use std::cell::{Cell, OnceCell};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use md_logic::preview::asset_origin;
use md_logic::strings::preview;

use crate::services::paths::Paths;
use crate::web::asset_host::AssetHost;

/// The environment variable WebKit reads to start its web processes without bubblewrap.
const WEBKIT_NO_SANDBOX: &str = "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS";
/// The dev image's switch past the probe (debug and self-test builds only).
const SKIP_PROBE: &str = "MD_SKIP_SANDBOX_PROBE";

/// What the sandbox preflight found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preflight {
    /// Not run, and why.
    Skipped(&'static str),
    /// bwrap made its user namespace: WebKit's will start too.
    Passed,
    /// bwrap could not; its own words.
    Failed(String),
}

/// The web context, the session, the asset host and the host-name counters, one per process.
pub struct WebServices {
    asset_host: Rc<AssetHost>,
    /// `$XDG_RUNTIME_DIR/md`: the PDF temp files (shell-design §7.3).
    temp_dir: PathBuf,
    engine: OnceCell<(webkit::WebContext, webkit::NetworkSession)>,
    sandbox: OnceCell<Preflight>,
    refused: Cell<bool>,
    previews: Cell<u64>,
    exports: Cell<u64>,
}

impl WebServices {
    pub fn new(paths: &Paths) -> Rc<WebServices> {
        Rc::new(WebServices {
            asset_host: AssetHost::new(paths.rich_dir()),
            temp_dir: paths.runtime_dir().to_path_buf(),
            engine: OnceCell::new(),
            sandbox: OnceCell::new(),
            refused: Cell::new(false),
            previews: Cell::new(0),
            exports: Cell::new(0),
        })
    }

    /// The sandbox preflight, run once per process (the answer is kept).
    pub fn preflight(&self) -> Preflight {
        self.sandbox
            .get_or_init(|| match skip_reason() {
                Some(reason) => Preflight::Skipped(reason),
                None => match probe_sandbox() {
                    Ok(()) => Preflight::Passed,
                    Err(message) => Preflight::Failed(message),
                },
            })
            .clone()
    }

    /// False once the preflight failed: nothing may be loaded, because a load starts the web
    /// process and a web process that cannot be sandboxed aborts md.
    pub fn can_load(&self) -> bool {
        !matches!(self.preflight(), Preflight::Failed(_))
    }

    /// A `WebView` on the shared context and session with md's settings and `user_content`. The
    /// first call runs the sandbox preflight; a failed one schedules the alert and the quit.
    pub fn new_view(&self, user_content: &webkit::UserContentManager) -> webkit::WebView {
        if let Preflight::Failed(message) = self.preflight()
            && !self.refused.replace(true)
        {
            eprintln!("md: the WebKit sandbox cannot start: {message}");
            glib::idle_add_local_once(refuse_to_start);
        }
        let (context, session) = self.engine.get_or_init(|| {
            // The inherent `default()` is the global context (an Option); registrations leak within a
            // process, so the scheme is registered exactly once (facts §3).
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

    /// A bundled engine file, through the asset host's containment — the HTML export's KaTeX
    /// inlining. `rel` is relative to `rich/`; md-logic's slash-separated key with its `rich/`
    /// prefix (`rich/katex.min.css`) is accepted too.
    pub fn read_rich_asset(&self, rel: &str) -> Option<Vec<u8>> {
        let rel = rel.strip_prefix(asset_origin::RICH_FOLDER_NAME).and_then(|r| r.strip_prefix('/')).unwrap_or(rel);
        self.asset_host.read_rich(rel)
    }

    /// The next live preview's host: `preview-1`, `preview-2`, …
    pub fn next_preview_host(&self) -> String {
        self.previews.set(self.previews.get() + 1);
        asset_origin::preview_host(self.previews.get())
    }

    /// The next export renderer's host: `export-1`, `export-2`, …
    pub fn next_export_host(&self) -> String {
        self.exports.set(self.exports.get() + 1);
        asset_origin::export_host(self.exports.get())
    }

    /// Where a PDF is printed before its bytes are read back (and the file deleted):
    /// `$XDG_RUNTIME_DIR/md`, created `0700` on first use; the system temp folder when that fails.
    pub fn temp_dir(&self) -> PathBuf {
        use std::os::unix::fs::DirBuilderExt;
        let made = std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&self.temp_dir);
        match made {
            Ok(()) => self.temp_dir.clone(),
            Err(err) => {
                eprintln!("md: {}: {err}; printing through the temp folder", self.temp_dir.display());
                std::env::temp_dir()
            }
        }
    }
}

/// md's `webkit::Settings` (shell-design §4.1): JavaScript on, every storage and device API off,
/// backgrounds printed, the inspector and console echo only in debug builds.
pub fn settings() -> webkit::Settings {
    let settings = webkit::Settings::new();
    settings.set_enable_javascript(true);
    settings.set_javascript_can_open_windows_automatically(false);
    settings.set_javascript_can_access_clipboard(false);
    settings.set_enable_back_forward_navigation_gestures(false);
    settings.set_enable_html5_local_storage(false);
    settings.set_enable_html5_database(false);
    settings.set_enable_webaudio(false);
    settings.set_enable_webrtc(false);
    settings.set_enable_media_stream(false);
    settings.set_enable_fullscreen(false);
    settings.set_print_backgrounds(true);
    settings.set_default_charset("utf-8");
    settings.set_enable_developer_extras(cfg!(debug_assertions));
    settings.set_enable_write_console_messages_to_stdout(cfg!(debug_assertions));
    settings
}

/// Why the probe is not run, if it is not.
fn skip_reason() -> Option<&'static str> {
    if std::env::var_os(WEBKIT_NO_SANDBOX).is_some_and(|v| !v.is_empty()) {
        return Some("WebKit's sandbox is disabled by WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS");
    }
    if (cfg!(debug_assertions) || cfg!(feature = "selftest")) && std::env::var_os(SKIP_PROBE).is_some_and(|v| v == "1")
    {
        return Some("MD_SKIP_SANDBOX_PROBE=1 (development builds only)");
    }
    None
}

/// `bwrap --unshare-user --ro-bind / / true`, as WebKit's web processes start: `Ok` when bubblewrap
/// made its user namespace, else its own words (or why it could not be run at all).
pub fn probe_sandbox() -> Result<(), String> {
    let output = std::process::Command::new("bwrap")
        .args(["--unshare-user", "--ro-bind", "/", "/", "true"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|err| format!("bwrap: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let words = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if words.is_empty() { format!("bwrap: {}", output.status) } else { words })
}

/// The preflight failed: one alert, then quit (shell-design §4.1).
fn refuse_to_start() {
    let Some(app) = gio::Application::default().and_downcast::<adw::Application>() else {
        std::process::exit(1);
    };
    let parent = app.active_window().or_else(|| app.windows().into_iter().next());
    let Some(parent) = parent else {
        eprintln!("md: {}: {}", preview::SANDBOX_ALERT_TITLE, preview::SANDBOX_ALERT_MESSAGE);
        app.quit();
        return;
    };
    let dialog = adw::AlertDialog::new(Some(preview::SANDBOX_ALERT_TITLE), Some(preview::SANDBOX_ALERT_MESSAGE));
    dialog.add_response("ok", md_logic::strings::buttons::OK);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("ok");
    let quit = app.clone();
    dialog.connect_response(None, move |_, _| quit.quit());
    dialog.present(Some(&parent));
}
