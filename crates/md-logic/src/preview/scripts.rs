//! Port of md.win `Preview/Scripts.cs`: every script the host injects or evaluates (Appendix B).
//!
//! Nothing in `rich/` or `md-init.js` is edited — those bytes are shared with the other ports — so
//! anything host-specific lives here and is either registered as a document-start
//! `webkit::UserScript` (`UserContentManager::add_script`; registered scripts are not part of the
//! DOM, so an export never carries them) or evaluated with `evaluate_javascript_future`.
//!
//! Both injected scripts run at *document start* (the Mac's `WKUserScript` ran at document end), so
//! neither may touch the DOM at injection time; both only register listeners and define functions.
//!
//! Every number interpolated into a script is written as .NET's invariant `double.ToString` writes
//! it ([`invariant_number`]) — the culture problem md.win guards against cannot arise in Rust, but
//! the *spelling* (shortest round trip, `E` notation past 1e15 and below 1e-4) is pinned so the
//! scripts read the same on every port.

use md_core::export::diagram_svg;

/// The Mac's scroll-sync user script, byte for byte.
///
/// LINUX DECISION: md.win substitutes `window.chrome.webview.postMessage(` for the Mac's
/// `window.webkit?.messageHandlers?.mdScroll?.postMessage(`. WebKitGTK provides
/// `window.webkit.messageHandlers` exactly as WKWebView does — md-init.js's `mdRender` message
/// already arrives through it unchanged (docs/linux-facts.md §4) — so on Linux the script is the
/// Mac's with **no** substitution, and the shell registers a [`SCROLL_MESSAGE_HANDLER`] handler
/// (`UserContentManager::register_script_message_handler`) whose value, as JSON, is
/// [`json_script::scroll_message`](super::json_script::scroll_message)'s input. Everything — the
/// 300 ms echo window, the rAF throttle, the passive listener, the three `window.__md*` entry
/// points — is the Mac's.
pub const SCROLL_SYNC: &str = r#"(function () {
    let lastProgrammatic = 0;
    function maxScroll() {
        return Math.max(0, document.documentElement.scrollHeight - window.innerHeight);
    }
    window.__mdMarkProgrammatic = function () { lastProgrammatic = Date.now(); };
    window.__mdSyncScrollTo = function (fraction) {
        lastProgrammatic = Date.now();
        window.scrollTo(0, Math.max(0, Math.min(1, fraction)) * maxScroll());
    };
    window.__mdScrollTo = function (y) {
        lastProgrammatic = Date.now();
        window.scrollTo(0, y);
    };
    let pending = false;
    window.addEventListener('scroll', function () {
        if (pending) { return; }
        pending = true;
        requestAnimationFrame(function () {
            pending = false;
            const max = maxScroll();
            window.webkit?.messageHandlers?.mdScroll?.postMessage({
                fraction: max > 0 ? window.scrollY / max : 0,
                echo: (Date.now() - lastProgrammatic) < 300
            });
        });
    }, { passive: true });
})();"#;

/// The call [`SCROLL_SYNC`] posts its message with — the Mac's, unchanged on Linux.
pub const POST_MESSAGE_CALL: &str = "window.webkit?.messageHandlers?.mdScroll?.postMessage(";

/// The script message handler [`SCROLL_SYNC`] posts to; the shell registers it on each preview's
/// `UserContentManager`.
pub const SCROLL_MESSAGE_HANDLER: &str = "mdScroll";

/// The script message handler `rich/md-init.js` posts `'complete'` to once math, highlighting and
/// every diagram engine are done — the render-complete event the shell may listen for instead of
/// polling [`RENDER_COMPLETE`] (docs/linux-facts.md §4).
pub const RENDER_MESSAGE_HANDLER: &str = "mdRender";

/// What WebKit gave the Mac for free and gives WebKitGTK too, measured: a clicked `javascript:`
/// href runs in-page with no navigation, so `decide-policy` never fires and
/// [`decide`](super::link_policy::decide) never gets a say (docs/linux-facts.md §3). Capture phase
/// on `document`, `click` and `auxclick` (a middle click is not a `click`), letting through only
/// same-document fragments and http(s) — which `decide-policy` then decides. The href is compared
/// lower-cased, so `JavaScript:` is caught too. Proven on WebKitGTK as a document-start user script
/// (top frame); it does not stop an inline `onclick`, which
/// [`CONTENT_SECURITY_POLICY`](super::asset_origin::CONTENT_SECURITY_POLICY) does.
pub const LINK_GUARD: &str = r#"(function () {
  function guard(e) {
    var a = e.target && e.target.closest ? e.target.closest('a[href]') : null;
    if (!a) return;
    var href = (a.getAttribute('href') || '').trim().toLowerCase();
    if (href.charAt(0) === '#') return;                                          // same-document
    if (href.indexOf('http://') === 0 || href.indexOf('https://') === 0) return;  // NavigationStarting decides
    e.preventDefault(); e.stopImmediatePropagation();                            // javascript:, data:, file:, mailto:, relative
  }
  document.addEventListener('click', guard, true);
  document.addEventListener('auxclick', guard, true);
})();"#;

/// The render-complete probe (§4.8). Exports poll it; the live preview never waits. The JSON result
/// is the three characters `"1"` — see [`RENDER_COMPLETE_RESULT`].
pub const RENDER_COMPLETE: &str = "document.documentElement.getAttribute('data-md-render-complete')";

/// What [`RENDER_COMPLETE`] returns once `md-init.js` has raised the flag: JSON, quotes included.
pub const RENDER_COMPLETE_RESULT: &str = "\"1\"";

/// Read before a reload so the reader keeps their place (§4.5). Absolute pixels, as the Mac saves
/// them.
pub const SCROLL_Y: &str = "window.scrollY";

/// The grow-to-content measurement the EPUB snapshotter takes.
pub const SCROLL_HEIGHT: &str = "document.documentElement.scrollHeight";

/// Restore the pre-reload position through the injected helper; optional chaining makes a page
/// without it a no-op.
pub fn scroll_to(y: f64) -> String {
    format!("window.__mdScrollTo?.({})", invariant_number(y))
}

/// Editor → preview: a fraction of the scrollable range, clamped in the page.
pub fn sync_scroll_to(fraction: f64) -> String {
    format!("window.__mdSyncScrollTo({})", invariant_number(fraction))
}

/// Jump to a heading (§4.5). The slug is interpolated raw, exactly as the Mac does: the parser
/// guarantees slugs are letters, digits, `-` and `_` only. Marked programmatic first so the
/// proportional echo does not drag a Split editor that is jumping by its own exact line.
pub fn jump_to_slug(slug: &str) -> String {
    format!("window.__mdMarkProgrammatic?.(); document.getElementById('{slug}')?.scrollIntoView(true)")
}

// ─────────────────────── the export scripts (Appendix B) ───────────────────────
// Only exports evaluate these; the live preview never does. Every one is a single expression whose
// value the evaluation hands back JSON-encoded, so every result goes through `json_script` — never
// straight to a file. The self-contained-HTML capture is not here: md-core owns those bytes.

/// md.win's `Md.Core.Export.EpubExport.RichSelector` — math and the three diagram engines, no
/// plot. Re-exported from md-core so the DOM query and the EPUB writer's marker scan can never
/// drift.
pub const RICH_SELECTOR: &str = md_core::export::epub_export::RICH_SELECTOR;

/// The EPUB snapshotter's measurement, the Mac's one-liner over [`RICH_SELECTOR`]. Per element, in
/// DOM order: `[left + scrollX, top + scrollY, width, height, isMath ? 1 : 0]` — page coordinates.
pub fn rich_elements() -> String {
    format!(
        "Array.from(document.querySelectorAll('{RICH_SELECTOR}'))\
         .map(e => {{ const r = e.getBoundingClientRect(); return [r.left + window.scrollX, r.top + window.scrollY, r.width, r.height, \
         (e.classList.contains('md-mathi') || e.classList.contains('md-mathd')) ? 1 : 0]; }})"
    )
}

/// The rendered `<svg>` of the `ordinal`-th diagram, over md-core's
/// [`diagram_svg::DOM_SELECTOR`] (the other selector: it has `div.plot` and no math, and the two
/// must never be merged). JSON `null` means the engine threw or timed out and left its source
/// text — the export then refuses rather than writing an empty file.
pub fn diagram_svg(ordinal: usize) -> String {
    [
        "(function () {".to_string(),
        format!("  var nodes = document.querySelectorAll('{}');", diagram_svg::DOM_SELECTOR),
        format!("  var el = nodes[{ordinal}];"),
        "  if (!el) return null;".to_string(),
        "  var svg = el.querySelector('svg');".to_string(),
        "  return svg ? svg.outerHTML : null;".to_string(),
        "})()".to_string(),
    ]
    .join("\n")
}

/// md.vscode's canvas rasteriser, md.win's written-and-off contingency for a renderer whose
/// snapshot fails: it rasterises the `ordinal`-th rich element's own `<svg>` (so the three diagram
/// engines, never a KaTeX formula) at `scale`, and parks the answer on `window.__mdRaster`, which
/// [`CANVAS_RASTERISE_RESULT`] then polls (an evaluation hands back a promise as `{}`). `btoa` takes
/// Latin-1, so the UTF-8 bytes are spelled out one at a time. On Linux the snapshot path is proven
/// (docs/linux-facts.md §6); this stays the contingency.
pub fn canvas_rasterise(ordinal: usize, scale: f64) -> String {
    let scale = invariant_number(scale);
    [
        "(function () {".to_string(),
        "  window.__mdRaster = { done: false, png: null };".to_string(),
        format!("  var el = document.querySelectorAll('{RICH_SELECTOR}')[{ordinal}];"),
        "  var svg = el ? el.querySelector('svg') : null;".to_string(),
        "  if (!svg) { window.__mdRaster.done = true; return false; }".to_string(),
        "  var rect = el.getBoundingClientRect();".to_string(),
        "  var clone = svg.cloneNode(true);".to_string(),
        "  clone.setAttribute('width', String(rect.width));".to_string(),
        "  clone.setAttribute('height', String(rect.height));".to_string(),
        "  if (!clone.getAttribute('xmlns')) { clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg'); }"
            .to_string(),
        "  var text = new XMLSerializer().serializeToString(clone);".to_string(),
        "  var image = new Image();".to_string(),
        "  image.onload = function () {".to_string(),
        "    var canvas = document.createElement('canvas');".to_string(),
        format!("    canvas.width = Math.max(1, Math.round(rect.width * {scale}));"),
        format!("    canvas.height = Math.max(1, Math.round(rect.height * {scale}));"),
        "    var context = canvas.getContext('2d');".to_string(),
        "    if (context) {".to_string(),
        "      context.drawImage(image, 0, 0, canvas.width, canvas.height);".to_string(),
        "      var url = canvas.toDataURL('image/png');".to_string(),
        "      window.__mdRaster.png = url.slice(url.indexOf(',') + 1);".to_string(),
        "    }".to_string(),
        "    window.__mdRaster.done = true;".to_string(),
        "  };".to_string(),
        "  image.onerror = function () { window.__mdRaster.done = true; };".to_string(),
        "  var bytes = new TextEncoder().encode(text);".to_string(),
        "  var latin1 = '';".to_string(),
        "  for (var b = 0; b < bytes.length; b++) latin1 += String.fromCharCode(bytes[b]);".to_string(),
        "  image.src = 'data:image/svg+xml;base64,' + btoa(latin1);".to_string(),
        "  return true;".to_string(),
        "})()".to_string(),
    ]
    .join("\n")
}

/// Polls [`canvas_rasterise`]: JSON `null` while it is still drawing (or when it failed), the
/// base64 PNG once it is done.
pub const CANVAS_RASTERISE_RESULT: &str =
    "(window.__mdRaster && window.__mdRaster.done) ? window.__mdRaster.png : null";

/// `double.ToString(CultureInfo.InvariantCulture)` (and `"R"`, the same string since .NET Core
/// 3.0): the shortest string that round-trips, in fixed notation while the decimal exponent is at
/// least −5 and below 15 (`0.0001`, `100000000000000`), and in `E` notation with a sign and at
/// least two exponent digits past either bound (`1E-05`, `1E+15`). Non-finite values are .NET's
/// words (`Infinity`, `-Infinity`, `NaN`) — which is why callers refuse them before they reach a
/// script. The same rule as md-core's private `pdf_export::invariant_number`.
pub fn invariant_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((scientific.as_str(), "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if value != 0.0 && !(-4..15).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}E{sign}{:02}", exponent.unsigned_abs());
    }
    format!("{value}")
}
