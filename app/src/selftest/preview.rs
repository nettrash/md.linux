//! The `preview` self-test group (shell-design §13.3): the bundled examples render offline under
//! the CSP — math (with `\ce{}`), code, Mermaid (light and dark), Graphviz, PlantUML and plots; the
//! link guard, the CSP and the policy together — `javascript:` and `onclick` inert, `other.md`
//! cancelled, http(s) handed out, `#slug` scrolls, no new window; one reload per 350 ms debounce
//! with the scroll kept; the dark switch re-renders with `data-md-dark`; scroll sync both ways with
//! echoes dropped; the pinned zoom and the Copy-only context menu; a killed web process reloads
//! once, twice in a row shows the notice, and an edit brings the document back.
//!
//! The group drives its own window holding one `PreviewHost` and md-logic's `PreviewCoordinator`
//! over it — the document window is another package's, and this proves the host as any window will
//! use it.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use md_logic::preview::json_script::{self, Json};
use md_logic::preview::{LinkDecision, PreviewCoordinator, asset_origin, link_policy, scripts};
use md_logic::strings;
use webkit::prelude::*;

use crate::selftest::Harness;
use crate::web::context::{self, Preflight};
use crate::web::preview_host::PreviewHost;
use crate::web::retry_policy::RetryResponse;

const TITLE: &str = "md self-test: preview";

/// How long a page may take to report render-complete (PlantUML's first import is the slow one).
const RENDER_LIMIT: Duration = Duration::from_secs(90);

struct Preview<'a> {
    h: &'a Harness,
    window: adw::ApplicationWindow,
    preview: Rc<PreviewHost>,
    coordinator: Rc<PreviewCoordinator>,
    documents: Cell<u32>,
    /// The token of the document on screen (a new one is a fresh page at the top).
    token: RefCell<String>,
}

pub async fn run(h: &Harness) {
    let shell = h.shell();
    sandbox(h, &shell);

    let preview = PreviewHost::new(&shell.web);
    let window = adw::ApplicationWindow::builder()
        .application(&shell.app)
        .title(TITLE)
        .default_width(1000)
        .default_height(760)
        .content(&preview.widget())
        .build();
    window.present();
    let coordinator = preview.coordinator(&shell);
    let t = Preview { h, window, preview, coordinator, documents: Cell::new(0), token: RefCell::new(String::new()) };
    h.next_frame(&t.window).await;

    t.engines().await;
    t.dark().await;
    t.links().await;
    t.debounce().await;
    t.scroll_sync().await;
    t.chrome().await;
    t.crash_recovery().await;

    t.preview.close();
    t.window.close();
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::Default);
}

/// The preflight answers without starting WebKit, and the probe itself sees what WebKit would.
fn sandbox(h: &Harness, shell: &crate::app::Shell) {
    let docker = std::env::var_os("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS").is_some_and(|v| !v.is_empty());
    let answer = shell.web.preflight();
    h.check(
        "sandbox_preflight_answers",
        if docker { matches!(answer, Preflight::Skipped(_)) } else { answer == Preflight::Passed },
        format!("{answer:?}"),
    );
    let probe = context::probe_sandbox();
    h.check(
        "sandbox_probe_reports_bwrap",
        if docker { probe.is_err() } else { probe.is_ok() },
        format!("bwrap --unshare-user: {probe:?}"),
    );
}

impl Preview<'_> {
    async fn eval(&self, script: &str) -> String {
        self.preview.surface().eval(script.to_string()).await
    }

    async fn json(&self, script: &str) -> Json {
        json_script::parse(&self.eval(script).await).unwrap_or(Json::Null)
    }

    async fn number(&self, script: &str) -> f64 {
        json_script::number(&self.eval(script).await).unwrap_or(f64::NAN)
    }

    /// Waits for the next completed navigation of the preview.
    async fn wait_loaded(&self, finished_before: u32) -> bool {
        let probe = self.preview.probe();
        self.h.wait_until(Duration::from_secs(30), || probe.loads_finished.get() > finished_before).await
    }

    /// `data-md-render-complete` on the page (md-init.js's flag), polled.
    async fn wait_rendered(&self) -> Option<Duration> {
        let start = Instant::now();
        while start.elapsed() < RENDER_LIMIT {
            if self.eval(scripts::RENDER_COMPLETE).await == scripts::RENDER_COMPLETE_RESULT {
                return Some(start.elapsed());
            }
            self.h.sleep(Duration::from_millis(100)).await;
        }
        None
    }

    /// `text` as a new document (a fresh page at the top), rendered.
    async fn show(&self, text: &str, title: &str, dark: bool) -> Option<Duration> {
        self.documents.set(self.documents.get() + 1);
        let token = format!("selftest-{}", self.documents.get());
        *self.token.borrow_mut() = token.clone();
        let before = self.preview.probe().loads_finished.get();
        self.preview.apply_theme(dark);
        self.coordinator.update(text, title, dark, Some(&token));
        if !self.wait_loaded(before).await {
            return None;
        }
        self.wait_rendered().await
    }

    /// The same document with new text: the debounced, scroll-keeping reload.
    fn edit(&self, text: &str, title: &str, dark: bool) {
        let token = self.token.borrow().clone();
        self.coordinator.update(text, title, dark, Some(&token));
    }

    fn example(&self, name: &str) -> String {
        let path = self.h.shell().paths.examples_dir().join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| format!("# {name}\n\n{} is unreadable: {err}", path.display()))
    }

    async fn scroll_into_view(&self, selector: &str) {
        let script = format!(
            "document.querySelector({})?.scrollIntoView({{block: 'start'}}); window.scrollBy(0, -24); window.scrollY",
            json_script_string(selector)
        );
        let _ = self.eval(&script).await;
    }

    // ── the engines ────────────────────────────────────────────────────────────────────────────

    async fn counts(&self) -> Json {
        self.json(
            "({katex: document.querySelectorAll('.katex').length, \
              katexErrors: document.querySelectorAll('.katex-error').length, \
              mhchem: Array.from(document.querySelectorAll('.katex annotation')).filter(a => a.textContent.includes('\\\\ce')).length, \
              hljs: document.querySelectorAll('code.hljs').length, \
              mermaid: document.querySelectorAll('pre.mermaid svg').length, \
              graphviz: document.querySelectorAll('div.graphviz svg').length, \
              plantuml: document.querySelectorAll('div.plantuml svg').length, \
              plots: document.querySelectorAll('div.plot').length, \
              plotSvgs: document.querySelectorAll('div.plot svg').length, \
              polylines: document.querySelectorAll('div.plot svg polyline').length, \
              scripts: Array.from(document.scripts).map(s => s.getAttribute('src') || 'inline'), \
              dark: document.body.dataset.mdDark || '', \
              fonts: document.getElementById('md-linux-fonts') ? 1 : 0, \
              origin: location.origin})",
        )
        .await
    }

    async fn engines(&self) {
        let h = self.h;
        let host = self.preview.host_name().to_string();
        let _ = h.shell().web.asset_host().take_log();

        // Math, \ce{} included.
        let rendered = self.show(&self.example("06-Math.md"), "06-Math", false).await;
        let c = self.counts().await;
        h.check(
            "math_renders",
            rendered.is_some() && num(&c, "katex") >= 5.0 && num(&c, "katexErrors") == 0.0 && num(&c, "mhchem") >= 1.0,
            format!("in {rendered:?}: {}", describe(&c, &["katex", "katexErrors", "mhchem", "fonts", "origin"])),
        );
        self.scroll_into_view(".katex-display").await;
        h.screenshot("math").await;

        // The CSP went out with the page, and every engine file the page asked for was served.
        let served = h.shell().web.asset_host().take_log();
        let index = served.iter().filter(|s| s.uri == asset_origin::index_url(&host)).collect::<Vec<_>>();
        let rich = served.iter().filter(|s| s.uri.contains("/rich/")).collect::<Vec<_>>();
        h.check(
            "index_carries_the_csp",
            !index.is_empty() && index.iter().all(|s| s.status == 200 && s.csp) && rich.iter().all(|s| !s.csp),
            format!("{} index.html responses, {} rich/ responses", index.len(), rich.len()),
        );
        let missing: Vec<&str> = rich.iter().filter(|s| s.status != 200).map(|s| s.uri.as_str()).collect();
        h.check(
            "engine_files_are_served",
            rich.len() >= 4 && missing.is_empty(),
            format!("{} rich/ files, missing {missing:?}", rich.len()),
        );

        // Code.
        let rendered = self.show(&self.example("04-Code.md"), "04-Code", false).await;
        let c = self.counts().await;
        h.check(
            "code_highlights",
            rendered.is_some() && num(&c, "hljs") >= 1.0,
            format!("in {rendered:?}: {}", describe(&c, &["hljs"])),
        );
        self.scroll_into_view("code.hljs").await;
        h.screenshot("code").await;

        // Mermaid, Graphviz, PlantUML.
        let rendered = self.show(&self.example("07-Diagrams.md"), "07-Diagrams", false).await;
        let c = self.counts().await;
        h.check(
            "diagrams_render",
            rendered.is_some()
                && num(&c, "mermaid") == 3.0
                && num(&c, "graphviz") == 2.0
                && num(&c, "plantuml") == 2.0
                && text(&c, "dark") == "0",
            format!("in {rendered:?}: {}", describe(&c, &["mermaid", "graphviz", "plantuml", "dark"])),
        );
        for (name, selector) in [("mermaid", "pre.mermaid"), ("graphviz", "div.graphviz"), ("plantuml", "div.plantuml")]
        {
            self.scroll_into_view(selector).await;
            h.screenshot(name).await;
        }

        // Plots: drawn by md-core, no engine loaded for them.
        let rendered = self.show(&self.example("08-Plots.md"), "08-Plots", false).await;
        let c = self.counts().await;
        let scripts = text_list(&c, "scripts");
        h.check(
            "plots_render_without_an_engine",
            rendered.is_some()
                && num(&c, "plots") >= 6.0
                && num(&c, "plotSvgs") == num(&c, "plots")
                && num(&c, "polylines") >= num(&c, "plots")
                && scripts == ["rich/md-init.js"],
            format!("in {rendered:?}: {} scripts {scripts:?}", describe(&c, &["plots", "plotSvgs", "polylines"])),
        );
        h.screenshot("plots").await;

        // The design's own plot vector: one fence, two functions → one svg, two polylines.
        let rendered = self.show("```plot\nsin(x)\ncos(x)\n```\n", "plot", false).await;
        let c = self.counts().await;
        h.check(
            "plot_two_functions",
            rendered.is_some() && num(&c, "plotSvgs") == 1.0 && num(&c, "polylines") == 2.0,
            describe(&c, &["plotSvgs", "polylines"]),
        );
    }

    // ── light / dark ──────────────────────────────────────────────────────────────────────────

    async fn dark(&self) {
        let h = self.h;
        let style = adw::StyleManager::default();
        let source = self.example("07-Diagrams.md");
        if self.show(&source, "07-Diagrams", false).await.is_none() {
            h.check("dark_rerenders", false, "the light page never rendered");
            return;
        }
        let _ = self.eval("window.scrollTo(0, 400); window.scrollY").await;
        h.sleep(Duration::from_millis(300)).await;

        // What a window does on `dark-notify`: the background first, then the coordinator.
        style.set_color_scheme(adw::ColorScheme::ForceDark);
        let dark = style.is_dark();
        let before = self.preview.probe().loads_finished.get();
        self.preview.apply_theme(dark);
        self.edit(&source, "07-Diagrams", dark);
        let reloaded = self.wait_loaded(before).await;
        let rendered = self.wait_rendered().await;
        h.sleep(Duration::from_millis(300)).await;
        let c = self.counts().await;
        let y = self.number(scripts::SCROLL_Y).await;
        let background = self.preview.view().background_color();
        h.check(
            "dark_rerenders",
            dark && reloaded
                && rendered.is_some()
                && text(&c, "dark") == "1"
                && num(&c, "mermaid") == 3.0
                && num(&c, "plantuml") == 2.0
                && num(&c, "graphviz") == 2.0
                && (y - 400.0).abs() <= 2.0
                && background.red() < 0.5,
            format!(
                "dark {dark}, reloaded {reloaded}, {rendered:?}: {}, scrollY {y}, background {background}",
                describe(&c, &["dark", "mermaid", "graphviz", "plantuml"])
            ),
        );
        self.scroll_into_view("pre.mermaid").await;
        h.screenshot("mermaid-dark").await;
        self.scroll_into_view("div.plantuml").await;
        h.screenshot("plantuml-dark").await;

        style.set_color_scheme(adw::ColorScheme::ForceLight);
        self.preview.apply_theme(false);
        let before = self.preview.probe().loads_finished.get();
        self.edit(&source, "07-Diagrams", false);
        let back = self.wait_loaded(before).await && self.wait_rendered().await.is_some();
        let c = self.counts().await;
        h.check("light_again", back && text(&c, "dark") == "0", describe(&c, &["dark"]));
    }

    // ── links ─────────────────────────────────────────────────────────────────────────────────

    /// A page of every kind of link, served as this preview's index.html — with the CSP, the guard
    /// and the message handlers all in place.
    async fn load_link_page(&self) -> bool {
        let page = "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>links</title></head>\
            <body style=\"font: 20px sans-serif; padding: 20px\">\
            <p><a id=\"js\" href=\"javascript:document.documentElement.dataset.js='ran';void 0\">javascript: link</a></p>\
            <p><a id=\"ext\" href=\"https://example.com/x\">https link</a></p>\
            <p><a id=\"blank\" href=\"https://example.org/y\" target=\"_blank\">target=_blank link</a></p>\
            <p><a id=\"rel\" href=\"other.md\">relative other.md</a></p>\
            <p><a id=\"file\" href=\"file:///etc/hostname\">file: link</a></p>\
            <p><a id=\"frag\" href=\"#target\">#fragment</a></p>\
            <p><a id=\"inline\" href=\"#\" onclick=\"document.documentElement.dataset.inline='ran'\">inline onclick</a></p>\
            <div style=\"height: 2000px\"></div><h2 id=\"target\">target</h2></body></html>";
        let surface = self.preview.surface();
        surface.set_html(page.to_string());
        let before = self.preview.probe().loads_finished.get();
        surface.navigate(self.preview.index_url());
        if !self.wait_loaded(before).await {
            return false;
        }
        // The coordinator restores its last saved position on every completed load; this page
        // starts at the top.
        self.h.sleep(Duration::from_millis(200)).await;
        self.number("window.scrollTo(0, 0); window.scrollY").await == 0.0
    }

    async fn link_state(&self) -> Json {
        self.json(
            "({js: document.documentElement.dataset.js || '', inline: document.documentElement.dataset.inline || '', \
              href: location.href, y: window.scrollY})",
        )
        .await
    }

    async fn links(&self) {
        let h = self.h;
        let index = self.preview.index_url().to_string();
        let windows_before = h.shell().app.windows().len();
        let mut rows: Vec<String> = Vec::new();
        let mut ok = true;
        let kinds: Vec<(&str, bool)> = {
            let mut kinds = vec![
                ("js", false),
                ("ext", false),
                ("blank", false),
                ("rel", false),
                ("file", false),
                ("frag", false),
                ("inline", false),
            ];
            if h.input() {
                kinds.extend([("js", true), ("ext", true), ("rel", true), ("inline", true)]);
            }
            kinds
        };
        for (id, real) in kinds {
            if !self.load_link_page().await {
                h.check("links_page_loads", false, "the link page never finished loading");
                return;
            }
            let decided_before = self.preview.probe().decisions.borrow().len();
            if real {
                let rect = self
                    .json(&format!(
                        "(() => {{ const r = document.getElementById('{id}').getBoundingClientRect(); return [r.left + 6, r.top + r.height / 2]; }})()"
                    ))
                    .await;
                let (x, y) = match &rect {
                    Json::Array(v) => (as_num(v.first()), as_num(v.get(1))),
                    _ => (f64::NAN, f64::NAN),
                };
                h.focus(self.window.upcast_ref()).await;
                if let Some((sx, sy)) = self.screen_point(x, y).await {
                    h.xdotool(&["mousemove", "--sync", &sx.to_string(), &sy.to_string(), "click", "1"]).await;
                    h.sleep(Duration::from_millis(300)).await;
                }
            } else {
                let _ = self.eval(&format!("document.getElementById('{id}').click(); 0")).await;
            }
            h.sleep(Duration::from_millis(700)).await;
            let state = self.link_state().await;
            let decisions: Vec<_> = self.preview.probe().decisions.borrow()[decided_before..].to_vec();
            let href = text(&state, "href");
            let stayed = href.split('#').next() == Some(index.as_str());
            let consistent =
                decisions.iter().all(|d| d.decision == link_policy::decide(&d.uri, d.user_gesture, &index));
            let passed = consistent
                && match id {
                    "js" => text(&state, "js").is_empty() && stayed && href == index,
                    "inline" => text(&state, "inline").is_empty() && stayed,
                    "ext" => {
                        stayed
                            && decisions.iter().any(|d| d.uri == "https://example.com/x")
                            && (!real
                                || decisions
                                    .iter()
                                    .any(|d| d.decision == LinkDecision::OpenExternally && d.user_gesture))
                    }
                    "blank" => {
                        stayed
                            && decisions.iter().any(|d| d.new_window && d.uri.starts_with("https://example.org/"))
                            && h.shell().app.windows().len() == windows_before
                    }
                    "rel" | "file" => {
                        stayed && href == index && decisions.iter().all(|d| d.decision != LinkDecision::Allow)
                    }
                    "frag" => {
                        href == format!("{index}#target")
                            && num(&state, "y") > 1000.0
                            && decisions.iter().any(|d| d.decision == LinkDecision::Allow)
                    }
                    _ => false,
                };
            ok &= passed;
            let decided: Vec<String> = decisions
                .iter()
                .map(|d| {
                    format!(
                        "{} {:?}{}{}",
                        d.uri,
                        d.decision,
                        if d.user_gesture { " gesture" } else { "" },
                        if d.new_window { " new-window" } else { "" }
                    )
                })
                .collect();
            rows.push(format!(
                "{id}{}: {} — {} y={} js={:?} inline={:?} [{}]",
                if real { " (xdotool)" } else { "" },
                if passed { "ok" } else { "WRONG" },
                href,
                num(&state, "y"),
                text(&state, "js"),
                text(&state, "inline"),
                decided.join("; ")
            ));
        }
        h.check("links_follow_the_policy", ok, rows.join(" | "));
        h.screenshot("links").await;
    }

    // ── the debounce and the scroll ───────────────────────────────────────────────────────────

    async fn debounce(&self) {
        let h = self.h;
        let mut text: String = (1..=120)
            .map(|n| {
                format!(
                    "## Section {n}\n\nParagraph {n} of a long document, long enough to scroll well past 1200 px.\n\n"
                )
            })
            .collect();
        if self.show(&text, "Long", false).await.is_none() {
            h.check("debounce_one_reload", false, "the long page never rendered");
            return;
        }
        let y0 = self.number("window.scrollTo(0, 1200); window.scrollY").await;
        h.sleep(Duration::from_millis(300)).await;

        let probe = self.preview.probe();
        let started_before = probe.loads_started.get();
        let finished_before = probe.loads_finished.get();
        let typed = "Typed under Xvfb.";
        let mut last_edit = Instant::now();
        for c in typed.chars() {
            text.push(c);
            self.edit(&text, "Long", false);
            last_edit = Instant::now();
            h.sleep(Duration::from_millis(40)).await;
        }
        // The reload's start, to the 10 ms.
        let mut reload_after = None;
        let watch = Instant::now();
        while watch.elapsed() < Duration::from_secs(3) {
            if probe.loads_started.get() > started_before {
                reload_after = Some(last_edit.elapsed());
                break;
            }
            h.sleep(Duration::from_millis(10)).await;
        }
        let finished = h.wait_until(Duration::from_secs(10), || probe.loads_finished.get() > finished_before).await;
        h.sleep(Duration::from_millis(1000)).await;
        let reloads = probe.loads_started.get() - started_before;
        let y = self.number(scripts::SCROLL_Y).await;
        let in_page = self.eval(&format!("document.body.innerText.includes({})", json_script_string(typed))).await;
        let gap_ok =
            reload_after.is_some_and(|gap| gap >= Duration::from_millis(330) && gap < Duration::from_millis(1500));
        h.check(
            "debounce_one_reload",
            finished && reloads == 1 && gap_ok && in_page == "true",
            format!(
                "{} edits 40 ms apart → {reloads} reload(s), started {reload_after:?} after the last; text in page: {in_page}",
                typed.len()
            ),
        );
        h.check(
            "reload_keeps_the_scroll",
            y0 == 1200.0 && (y - 1200.0).abs() <= 2.0,
            format!("scrollY {y0} before, {y} after the reload"),
        );
        h.screenshot("debounced-reload").await;

        // A collapsed pane only records that it is stale, and loads once when shown again.
        self.preview.set_shown(false);
        let started = probe.loads_started.get();
        text.push_str("\n\nWritten while the preview was hidden.\n");
        self.edit(&text, "Long", false);
        h.sleep(Duration::from_millis(800)).await;
        let while_hidden = probe.loads_started.get() - started;
        let stale = self.coordinator.is_stale();
        self.preview.set_shown(true);
        let finished_before = probe.loads_finished.get();
        self.coordinator.show();
        let shown = h.wait_until(Duration::from_secs(10), || probe.loads_finished.get() > finished_before).await;
        h.sleep(Duration::from_millis(500)).await;
        let in_page = self.eval("document.body.innerText.includes('Written while the preview was hidden.')").await;
        h.check(
            "hidden_pane_loads_once_when_shown",
            while_hidden == 0 && stale && shown && probe.loads_started.get() - started == 1 && in_page == "true",
            format!(
                "{while_hidden} loads while hidden (stale {stale}), {} after show; text in page: {in_page}",
                probe.loads_started.get() - started
            ),
        );
    }

    async fn scroll_sync(&self) {
        let h = self.h;
        let fractions: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&fractions);
        let handler = self.preview.did_scroll().connect(move |&f| sink.borrow_mut().push(f));

        // Preview → editor: the reader scrolls the page.
        let max = self.number("Math.max(0, document.documentElement.scrollHeight - window.innerHeight)").await;
        let _ = self.eval("window.scrollTo(0, 600); 0").await;
        let reported = h.wait_until(Duration::from_secs(3), || !fractions.borrow().is_empty()).await;
        h.sleep(Duration::from_millis(400)).await;
        let last = fractions.borrow().last().copied().unwrap_or(f64::NAN);
        h.check(
            "scroll_sync_preview_to_editor",
            reported && max > 600.0 && (last - 600.0 / max).abs() < 0.01,
            format!("reported {:?}, expected {:.4}", fractions.borrow(), 600.0 / max),
        );

        // Editor → preview: the sync sets it, and the echo does not come back.
        h.sleep(Duration::from_millis(400)).await;
        let seen = fractions.borrow().len();
        self.preview.apply_scroll_fraction(0.5);
        h.sleep(Duration::from_millis(800)).await;
        let y = self.number(scripts::SCROLL_Y).await;
        h.check(
            "scroll_sync_editor_to_preview",
            (y - 0.5 * max).abs() <= 2.0 && fractions.borrow().len() == seen,
            format!("scrollY {y} for 0.5 of {max}; reports after the set: {:?}", &fractions.borrow()[seen..]),
        );
        self.preview.did_scroll().disconnect(handler);
    }

    // ── zoom and context menu ─────────────────────────────────────────────────────────────────

    async fn chrome(&self) {
        let h = self.h;
        let view = self.preview.view();
        view.set_zoom_level(2.0);
        h.sleep(Duration::from_millis(100)).await;
        h.check(
            "zoom_is_pinned",
            (view.zoom_level() - 1.0).abs() < f64::EPSILON,
            format!("zoom {}", view.zoom_level()),
        );

        if !h.input() {
            return;
        }
        let _ = self
            .eval("window.scrollTo(0, 0); window.getSelection().selectAllChildren(document.querySelector('h2')); 0")
            .await;
        let rect = self
            .json("(() => { const r = document.querySelector('h2').getBoundingClientRect(); return [r.left + 20, r.top + r.height / 2]; })()")
            .await;
        let (x, y) = match &rect {
            Json::Array(v) => (as_num(v.first()), as_num(v.get(1))),
            _ => (f64::NAN, f64::NAN),
        };
        self.preview.probe().menu_items.replace(None);
        h.focus(self.window.upcast_ref()).await;
        let opened = match self.screen_point(x, y).await {
            Some((sx, sy)) => {
                h.xdotool(&["mousemove", "--sync", &sx.to_string(), &sy.to_string(), "click", "3"]).await.0
            }
            None => false,
        };
        h.sleep(Duration::from_millis(600)).await;
        h.screenshot("context-menu").await;
        let items = *self.preview.probe().menu_items.borrow();
        h.check(
            "context_menu_copy_only",
            opened && items.is_some_and(|n| (1..=2).contains(&n)),
            format!("right click {opened}; items left: {items:?}"),
        );
        h.key("Escape").await;
        let _ = self.eval("window.getSelection().removeAllRanges(); 0").await;
    }

    /// The screen point of (`x`, `y`) in the preview, for xdotool (`Harness::click` works in logical
    /// pixels, which is off by the scale under `GDK_SCALE=2`).
    async fn screen_point(&self, x: f64, y: f64) -> Option<(i32, i32)> {
        let xid = self.h.xid(self.window.upcast_ref()).await?;
        let (_, geometry) = self.h.xdotool(&["getwindowgeometry", "--shell", &xid]).await;
        let field =
            |name: &str| geometry.lines().find_map(|l| l.strip_prefix(name)).and_then(|v| v.parse::<f64>().ok());
        let (wx, wy) = (field("X=")?, field("Y=")?);
        let point = self.preview.view().compute_point(&self.window, &gtk::graphene::Point::new(x as f32, y as f32))?;
        let (sx, sy) = self.window.surface_transform();
        // X11 coordinates are device pixels; GTK's are logical (GDK_SCALE=2 doubles them).
        let scale = self.window.surface().map_or(1.0, |surface| surface.scale());
        Some((
            (wx + (sx + f64::from(point.x())) * scale).round() as i32,
            (wy + (sy + f64::from(point.y())) * scale).round() as i32,
        ))
    }

    // ── the web process dies ──────────────────────────────────────────────────────────────────

    async fn crash_recovery(&self) {
        let h = self.h;
        let text = self.example("07-Diagrams.md");
        if self.show(&text, "07-Diagrams", false).await.is_none() {
            h.check("crash_reloads_once", false, "the diagrams never rendered");
            return;
        }

        // md's own termination is not a crash: nothing reloads.
        let processes = web_processes();
        let probe = self.preview.probe();
        let (started, counted) = (probe.loads_started.get(), probe.terminations.borrow().len());
        self.preview.view().terminate_web_process();
        h.sleep(Duration::from_millis(800)).await;
        h.check(
            "terminated_by_api_is_ignored",
            probe.loads_started.get() == started && probe.terminations.borrow().len() == counted,
            format!(
                "{} loads started, {} deaths counted",
                probe.loads_started.get() - started,
                probe.terminations.borrow().len() - counted
            ),
        );

        // A fresh process for this view, so its pid can be told apart from every other.
        let before = probe.loads_finished.get();
        self.preview.surface().navigate(self.preview.index_url());
        let loaded = self.wait_loaded(before).await && self.wait_rendered().await.is_some();
        let mut known = processes;
        let mine: Vec<u32> = web_processes().difference(&known).copied().collect();
        known.extend(mine.iter().copied());
        if mine.is_empty() {
            h.check("crash_reloads_once", false, format!("no web process of this view found (loaded {loaded})"));
            return;
        }

        // First death: one reload. (Every process that appeared for the view dies, should WebKit
        // have started a spare one beside it.)
        let renders = probe.renders.get();
        let mut killed = true;
        for pid in &mine {
            killed &= h.run(&["kill", "-KILL", &pid.to_string()]).await.0;
        }
        let victim = format!("{mine:?}");
        let reacted = h.wait_until(Duration::from_secs(5), || probe.terminations.borrow().len() > counted).await;
        let first = probe.terminations.borrow().get(counted).copied();
        h.check(
            "crash_reloads_once",
            killed && reacted && first == Some(RetryResponse::Reload),
            format!("kill -KILL {victim}: {first:?}"),
        );

        // Second death before the reload has rendered: the notice. The new process is killed the
        // moment it appears (PlantUML keeps md-init.js busy for a second after the load).
        let mut kills = 0;
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline && !probe.terminations.borrow().contains(&RetryResponse::GiveUp) {
            let fresh: Vec<u32> = web_processes().difference(&known).copied().collect();
            for pid in fresh {
                known.insert(pid);
                if probe.renders.get() == renders {
                    let _ = h.run(&["kill", "-KILL", &pid.to_string()]).await;
                    kills += 1;
                }
            }
            h.sleep(Duration::from_millis(10)).await;
        }
        let gave_up = probe.terminations.borrow().contains(&RetryResponse::GiveUp);
        h.sleep(Duration::from_millis(1500)).await;
        let body = json_script::string(&self.eval("document.body.innerText").await).unwrap_or_default();
        let notice = strings::preview::STOPPED_NOTICE_SOURCE.trim_matches('*');
        h.check(
            "second_crash_shows_the_notice",
            gave_up && body.trim() == notice && self.preview.retry_policy().has_given_up(),
            format!(
                "{kills} more kill(s); deaths {:?}; page: {:?}",
                &probe.terminations.borrow()[counted..],
                body.trim()
            ),
        );
        h.screenshot("crash-notice").await;

        // The next edit tries again, and a render resets the count.
        let before = probe.loads_finished.get();
        let edited = format!("{text}\nEdited after the crash.\n");
        self.edit(&edited, "07-Diagrams", false);
        let back = self.wait_loaded(before).await && self.wait_rendered().await.is_some();
        h.sleep(Duration::from_millis(500)).await;
        let body = json_script::string(&self.eval("document.body.innerText").await).unwrap_or_default();
        let policy = self.preview.retry_policy();
        h.check(
            "an_edit_brings_the_document_back",
            back && body.contains("Edited after the crash.") && !policy.has_given_up() && policy.failures() == 0,
            format!("rendered {back}; policy {policy:?}"),
        );
    }
}

/// The pids of this process's `WebKitWebProcess` descendants (`/proc`, Linux).
fn web_processes() -> BTreeSet<u32> {
    let me = std::process::id();
    let parent_of = |pid: u32| -> Option<u32> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // `pid (comm) state ppid …` — comm may hold spaces and parentheses; the last ')' ends it.
        let rest = &stat[stat.rfind(')')? + 1..];
        rest.split_whitespace().nth(1)?.parse().ok()
    };
    let Ok(entries) = std::fs::read_dir("/proc") else { return BTreeSet::new() };
    entries
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&pid| {
            let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            if !String::from_utf8_lossy(&cmdline).contains("WebKitWebProcess") {
                return false;
            }
            let mut ancestor = pid;
            for _ in 0..6 {
                match parent_of(ancestor) {
                    Some(p) if p == me => return true,
                    Some(p) if p > 1 => ancestor = p,
                    _ => return false,
                }
            }
            false
        })
        .collect()
}

fn json_script_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn as_num(value: Option<&Json>) -> f64 {
    match value {
        Some(Json::Number(n)) => *n,
        _ => f64::NAN,
    }
}

fn num(json: &Json, key: &str) -> f64 {
    as_num(json.member(key))
}

fn text(json: &Json, key: &str) -> String {
    match json.member(key) {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn text_list(json: &Json, key: &str) -> Vec<String> {
    match json.member(key) {
        Some(Json::Array(items)) => {
            items.iter().map(|i| if let Json::String(s) = i { s.clone() } else { String::new() }).collect()
        }
        _ => Vec::new(),
    }
}

fn describe(json: &Json, keys: &[&str]) -> String {
    keys.iter().map(|k| format!("{k}={}", text(json, k))).collect::<Vec<_>>().join(" ")
}
