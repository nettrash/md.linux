//! Port of md.win `tests/Md.App.Logic.Tests/Preview/ScriptsTests.cs`: the injected and evaluated
//! scripts (§4.6, Appendix B). The scroll-sync script is shared with md.macOS and is quoted here as
//! the Mac has it.
//!
//! LINUX: md.win's licence — swapping the Mac's `postMessage` channel for WebView2's — is not
//! taken: WebKitGTK provides `window.webkit.messageHandlers`, so the script is the Mac's byte for
//! byte, and the substitution tests become identity tests.

use md_logic::preview::scripts::{
    self, CANVAS_RASTERISE_RESULT, LINK_GUARD, POST_MESSAGE_CALL, RENDER_COMPLETE, RENDER_COMPLETE_RESULT,
    RENDER_MESSAGE_HANDLER, RICH_SELECTOR, SCROLL_HEIGHT, SCROLL_MESSAGE_HANDLER, SCROLL_SYNC, SCROLL_Y,
    invariant_number,
};

/// md.macOS's `WKUserScript` source, verbatim (understand/rich.md §5.2). Do not tidy: the point of
/// the copy is that it is not tidied.
const MAC_SCROLL_SYNC: &str = r#"(function () {
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

fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn scroll_sync_is_the_mac_script_with_exactly_one_substitution() {
    // LINUX: with none — the Mac's bytes exactly.
    assert_eq!(SCROLL_SYNC, MAC_SCROLL_SYNC);
}

#[test]
fn the_substitution_happens_exactly_once() {
    // LINUX: the one channel the script posts on is the Mac's mdScroll handler, used once.
    assert_eq!(occurrences(MAC_SCROLL_SYNC, POST_MESSAGE_CALL), 1);
    assert_eq!(occurrences(SCROLL_SYNC, POST_MESSAGE_CALL), 1);
    assert_eq!(occurrences(SCROLL_SYNC, "messageHandlers"), 1);
    assert!(POST_MESSAGE_CALL.contains(&format!("messageHandlers?.{SCROLL_MESSAGE_HANDLER}?.")));
    assert!(!SCROLL_SYNC.contains("chrome.webview"));
}

#[test]
fn scroll_sync_only_defines_functions_and_listeners() {
    // Injected at document *start* (the Mac injected at document end), so it must not read or
    // write the DOM at injection time.
    assert!(!SCROLL_SYNC.contains("document.body"));
    assert!(!SCROLL_SYNC.contains("querySelector"));
    assert!(SCROLL_SYNC.contains("window.addEventListener('scroll'"));
    assert!(SCROLL_SYNC.contains("{ passive: true }"));
}

#[test]
fn scroll_sync_keeps_the_three_host_entry_points_and_the_300_ms_echo_window() {
    assert!(SCROLL_SYNC.contains("window.__mdMarkProgrammatic = function ()"));
    assert!(SCROLL_SYNC.contains("window.__mdSyncScrollTo = function (fraction)"));
    assert!(SCROLL_SYNC.contains("window.__mdScrollTo = function (y)"));
    assert!(SCROLL_SYNC.contains("(Date.now() - lastProgrammatic) < 300"));
}

#[test]
fn scripts_are_lf_only() {
    // Both go into a webkit::UserScript as a single string; the bytes are pinned against another
    // port and must not follow the checkout's line endings.
    assert!(!SCROLL_SYNC.contains('\r'));
    assert!(!LINK_GUARD.contains('\r'));
}

#[test]
fn link_guard_covers_click_and_auxclick_in_the_capture_phase() {
    assert!(LINK_GUARD.contains("document.addEventListener('click', guard, true);"));
    assert!(LINK_GUARD.contains("document.addEventListener('auxclick', guard, true);"));
}

#[test]
fn link_guard_lets_through_only_fragments_and_http() {
    assert!(LINK_GUARD.contains("if (href.charAt(0) === '#') return;"));
    assert!(LINK_GUARD.contains("href.indexOf('http://') === 0 || href.indexOf('https://') === 0"));
    assert!(LINK_GUARD.contains("e.preventDefault(); e.stopImmediatePropagation();"));
    // Lower-cased before the comparisons, so `JavaScript:` is caught too.
    assert!(LINK_GUARD.contains(".trim().toLowerCase()"));
}

#[test]
fn the_render_complete_probe_and_its_answer_are_the_family_contract() {
    assert_eq!(RENDER_COMPLETE, "document.documentElement.getAttribute('data-md-render-complete')");
    // Three characters: Android compares against "\"1\"" for the same reason.
    assert_eq!(RENDER_COMPLETE_RESULT, "\"1\"");
    assert_eq!(RENDER_COMPLETE_RESULT.len(), 3);
}

#[test]
fn the_measurement_scripts_are_the_macs() {
    assert_eq!(SCROLL_Y, "window.scrollY");
    assert_eq!(SCROLL_HEIGHT, "document.documentElement.scrollHeight");
}

#[test]
fn numbers_go_into_scripts_in_the_invariant_culture() {
    // LINUX: Rust has no current culture to switch to German; the .NET spelling is pinned instead.
    assert_eq!(scripts::scroll_to(1234.5), "window.__mdScrollTo?.(1234.5)");
    assert_eq!(scripts::sync_scroll_to(0.5), "window.__mdSyncScrollTo(0.5)");
}

#[test]
fn the_heading_jump_marks_itself_programmatic_first() {
    assert_eq!(
        scripts::jump_to_slug("a-heading"),
        "window.__mdMarkProgrammatic?.(); document.getElementById('a-heading')?.scrollIntoView(true)"
    );
}

#[test]
#[ignore = "LINUX: KeyForward is not ported — every GTK accelerator fires once from the WebKitWebView too (docs/linux-facts.md §1), so the page reports no chords"]
fn key_forward_claims_the_root_chords_and_leaves_the_editing_keys_to_the_page() {
    // md.win: the keydown relay lists [114,0,0,0] (F3) and [70,1,0,0] (Ctrl+F) but not Ctrl+C /
    // Ctrl+A, claims with preventDefault, posts through chrome.webview, skips IME composition and
    // AltGraph, and is LF only.
}

// ── LINUX: the names the GTK shell registers, and the export scripts md.win pins elsewhere ────

#[test]
fn the_message_handlers_are_the_ones_the_shared_scripts_post_to() {
    assert_eq!(SCROLL_MESSAGE_HANDLER, "mdScroll");
    assert_eq!(RENDER_MESSAGE_HANDLER, "mdRender");
    // rich/md-init.js is byte-identical across the ports; it posts render-complete to mdRender.
    let md_init = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../rich/md-init.js"))
        .expect("rich/md-init.js");
    assert!(md_init.contains(&format!("window.webkit?.messageHandlers?.{RENDER_MESSAGE_HANDLER}?.postMessage(")));
}

#[test]
fn the_rich_elements_measurement_is_md_wins_expression() {
    // The C# concatenation, spelled out.
    let expected = "Array.from(document.querySelectorAll('.md-mathi, .md-mathd, .mermaid, .plantuml, .graphviz'))\
                    .map(e => { const r = e.getBoundingClientRect(); return [r.left + window.scrollX, r.top + window.scrollY, r.width, r.height, \
                    (e.classList.contains('md-mathi') || e.classList.contains('md-mathd')) ? 1 : 0]; })";
    assert_eq!(scripts::rich_elements(), expected);
    assert_eq!(RICH_SELECTOR, ".md-mathi, .md-mathd, .mermaid, .plantuml, .graphviz");
}

#[test]
fn the_diagram_script_reads_the_nth_diagram_over_md_cores_selector() {
    let script = scripts::diagram_svg(3);
    assert!(script.contains(&format!("document.querySelectorAll('{}')", md_core::export::diagram_svg::DOM_SELECTOR)));
    assert!(script.contains("var el = nodes[3];"));
    assert!(script.contains("return svg ? svg.outerHTML : null;"));
    assert!(!script.contains('\r'));
}

#[test]
fn the_canvas_contingency_writes_its_scale_invariantly_and_parks_its_answer() {
    let script = scripts::canvas_rasterise(2, 2.0);
    assert!(script.contains(&format!("document.querySelectorAll('{RICH_SELECTOR}')[2];")));
    assert!(script.contains("Math.round(rect.width * 2)"));
    assert!(scripts::canvas_rasterise(0, 1.5).contains("Math.round(rect.height * 1.5)"));
    assert!(script.contains("window.__mdRaster.done = true;"));
    assert_eq!(CANVAS_RASTERISE_RESULT, "(window.__mdRaster && window.__mdRaster.done) ? window.__mdRaster.png : null");
}

#[test]
fn numbers_are_written_as_dotnet_writes_a_double() {
    assert_eq!(invariant_number(1234.5), "1234.5");
    assert_eq!(invariant_number(0.42), "0.42");
    assert_eq!(invariant_number(1.0), "1");
    assert_eq!(invariant_number(1e14), "100000000000000");
    assert_eq!(invariant_number(1e15), "1E+15");
    assert_eq!(invariant_number(1e21), "1E+21");
    assert_eq!(invariant_number(0.0001), "0.0001");
    assert_eq!(invariant_number(0.00001), "1E-05");
    assert_eq!(invariant_number(1e-7), "1E-07");
    assert_eq!(invariant_number(0.1 + 0.2), "0.30000000000000004");
    assert_eq!(invariant_number(-0.0), "-0");
    assert_eq!(invariant_number(f64::INFINITY), "Infinity");
    assert_eq!(invariant_number(f64::NEG_INFINITY), "-Infinity");
    assert_eq!(invariant_number(f64::NAN), "NaN");
}
