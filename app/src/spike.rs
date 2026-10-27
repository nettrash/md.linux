//! `md --spike <dir>`: drives the running app through every W1 check and records what it saw.
//!
//! Real input comes from `xdotool` (XTEST) and screenshots from ImageMagick's `import` on the
//! Xvfb root window, both run as child processes while the main loop keeps turning. Results go
//! to `<dir>/spike-results.json`; the run log on stdout carries WebKit's console messages too.

use crate::assets::{AssetHost, SchemeSecurity};
use crate::export::{self, Paper};
use crate::preview::{self, Preview};
use crate::standin;
use crate::util::{self, eval_json, eval_number, json_str, sleep_ms};
use crate::window::{DocWindow, Page};
use adw::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

struct Spike {
    app: adw::Application,
    win: Rc<DocWindow>,
    out: PathBuf,
    results: RefCell<Vec<(String, String)>>,
    xid: RefCell<String>,
}

pub fn start(app: &adw::Application, win: &Rc<DocWindow>, out: PathBuf) {
    let _ = std::fs::create_dir_all(&out);
    let spike = Rc::new(Spike {
        app: app.clone(),
        win: Rc::clone(win),
        out,
        results: RefCell::new(Vec::new()),
        xid: RefCell::new(String::new()),
    });
    win.preview.launch_external.set(false);
    let hold = app.hold();
    glib::spawn_future_local(async move {
        let finished = util::with_timeout(Duration::from_secs(900), spike.run()).await.is_some();
        spike.set("completed", if finished { "true" } else { "false" });
        spike.write();
        drop(hold);
        spike.app.quit();
    });
}

impl Spike {
    fn set(&self, key: &str, json: &str) {
        println!("SPIKE {key} = {json}");
        self.results.borrow_mut().push((key.to_owned(), json.to_owned()));
    }

    fn set_str(&self, key: &str, value: &str) {
        self.set(key, &json_str(value));
    }

    fn write(&self) {
        let body: Vec<String> =
            self.results.borrow().iter().map(|(k, v)| format!("  {}: {}", json_str(k), v)).collect();
        let text = format!("{{\n{}\n}}\n", body.join(",\n"));
        let _ = std::fs::write(self.out.join("spike-results.json"), text);
    }

    async fn shot(&self, name: &str) {
        sleep_ms(400).await; // one more frame after the last change
        let path = self.out.join(format!("{name}.png"));
        let (ok, msg) = util::run(&["import", "-window", "root", &path.to_string_lossy()]).await;
        let value = if ok { path.display().to_string() } else { msg };
        self.set_str(&format!("shot.{name}"), &value);
    }

    async fn xdo(&self, args: &[&str]) -> (bool, String) {
        let mut argv = vec!["xdotool"];
        argv.extend_from_slice(args);
        util::run(&argv).await
    }

    async fn focus_window(&self) {
        let xid = self.xid.borrow().clone();
        if !xid.is_empty() {
            let _ = self.xdo(&["windowfocus", "--sync", &xid]).await;
        }
        sleep_ms(150).await;
    }

    /// Screen coordinates of a point in `widget`'s coordinate space.
    async fn screen_point(&self, widget: &impl IsA<gtk::Widget>, x: f64, y: f64) -> Option<(i32, i32)> {
        let xid = self.xid.borrow().clone();
        let (_, geo) = self.xdo(&["getwindowgeometry", "--shell", &xid]).await;
        let field = |name: &str| geo.lines().find_map(|l| l.strip_prefix(name)).and_then(|v| v.parse::<f64>().ok());
        let (wx, wy) = (field("X=")?, field("Y=")?);
        let p = widget.compute_point(&self.win.window, &gtk::graphene::Point::new(x as f32, y as f32))?;
        let (sx, sy) = self.win.window.surface_transform();
        Some(((wx + sx + f64::from(p.x())).round() as i32, (wy + sy + f64::from(p.y())).round() as i32))
    }

    async fn click(&self, widget: &impl IsA<gtk::Widget>, x: f64, y: f64) -> bool {
        let Some((sx, sy)) = self.screen_point(widget, x, y).await else { return false };
        let (ok, _) = self.xdo(&["mousemove", "--sync", &sx.to_string(), &sy.to_string(), "click", "1"]).await;
        sleep_ms(300).await;
        ok
    }

    async fn show_and_wait(&self, page: Page, limit: Duration) -> (Duration, bool) {
        let loaded = self.win.preview.next_load();
        self.win.show(page);
        let _ = util::with_timeout(Duration::from_secs(20), loaded).await;
        self.win.preview.wait_render_complete(limit).await
    }

    async fn run(self: &Rc<Self>) {
        sleep_ms(1500).await;
        self.environment().await;
        self.engines().await;
        self.accelerators().await;
        self.typing().await;
        self.single_instance().await;
        self.links().await;
        self.dark_mode().await;
        self.fonts().await;
        self.pdf().await;
        self.snapshots().await;
        self.file_dialogs().await;
        self.file_monitor().await;
    }

    // -- 0. Environment ------------------------------------------------------------------------

    async fn environment(&self) {
        self.set_str("env.gtk", &format!("{}.{}.{}", gtk::major_version(), gtk::minor_version(), gtk::micro_version()));
        self.set_str("env.adw", &format!("{}.{}.{}", adw::major_version(), adw::minor_version(), adw::micro_version()));
        self.set_str(
            "env.webkit",
            &format!(
                "{}.{}.{}",
                webkit::functions::major_version(),
                webkit::functions::minor_version(),
                webkit::functions::micro_version()
            ),
        );
        if let Some(display) = gtk::gdk::Display::default() {
            self.set_str("env.gdk_display", display.type_().name());
        }
        if let Some(renderer) = self.win.window.renderer() {
            self.set_str("env.gsk_renderer", renderer.type_().name());
        }
        for var in [
            "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS",
            "LIBGL_ALWAYS_SOFTWARE",
            "GDK_BACKEND",
            "DBUS_SESSION_BUS_ADDRESS",
            "GSK_RENDERER",
        ] {
            let value = std::env::var(var).unwrap_or_default();
            self.set_str(&format!("env.var.{var}"), &value);
        }
        self.set_str("env.file_printer_name", &export::file_printer_name());
        self.set("env.app_is_remote", if self.app.is_remote() { "true" } else { "false" });
        let printers = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let sink = std::sync::Arc::clone(&printers);
        let start = Instant::now();
        gtk::enumerate_printers(
            move |p| {
                if let Ok(mut v) = sink.lock() {
                    v.push(format!("{} (virtual={}, pdf={})", p.name(), p.is_virtual(), p.accepts_pdf()));
                }
                false
            },
            true,
        );
        let list = printers.lock().map(|v| v.clone()).unwrap_or_default();
        self.set_str("env.printers", &format!("{list:?} in {:?}", start.elapsed()));

        let (_, ids) = self.xdo(&["search", "--onlyvisible", "--name", "^md spike$"]).await;
        let xid = ids.lines().last().unwrap_or_default().to_owned();
        self.set_str("env.xid", &xid);
        *self.xid.borrow_mut() = xid;
        self.set("env.window_active_before_focus", if self.win.window.is_active() { "true" } else { "false" });
        self.focus_window().await;
        self.set("env.window_active_after_focus", if self.win.window.is_active() { "true" } else { "false" });
    }

    // -- 1/3/4. The asset origin, the engines, render-complete ---------------------------------

    async fn counts(&self) -> String {
        eval_json(
            &self.win.preview.view,
            "JSON.stringify({katex: document.querySelectorAll('.katex').length, \
             katexDisplay: document.querySelectorAll('.katex-display').length, \
             mermaidSvg: document.querySelectorAll('.mermaid svg').length, \
             graphvizSvg: document.querySelectorAll('.graphviz svg').length, \
             plantumlSvg: document.querySelectorAll('.plantuml svg').length, \
             hljs: document.querySelectorAll('code.hljs').length, \
             complete: document.documentElement.getAttribute('data-md-render-complete'), \
             origin: location.origin, href: location.href, secure: window.isSecureContext, \
             fonts: document.fonts.status, title: document.title, height: document.documentElement.scrollHeight})",
        )
        .await
        .unwrap_or_else(|e| json_str(&e))
    }

    async fn engines(&self) {
        let msgs_before = self.win.preview.stats.borrow().render_messages.len();
        let start = Instant::now();
        let (waited, complete) = self.show_and_wait(Page::Golden, Duration::from_secs(60)).await;
        self.set(
            "engines.golden.render_complete",
            &format!(
                "{{\"flag\": {complete}, \"waited_ms\": {}, \"since_load_ms\": {}}}",
                waited.as_millis(),
                start.elapsed().as_millis()
            ),
        );
        let msgs = self.win.preview.stats.borrow().render_messages.len() - msgs_before;
        self.set("engines.golden.mdRender_messages", &msgs.to_string());
        let counts = self.counts().await;
        self.set("engines.golden.counts", &counts);
        let served: Vec<String> =
            self.win.host.take_log().iter().map(|s| format!("{} {} {}", s.status, s.mime, s.path)).collect();
        self.set(
            "engines.golden.served",
            &format!("[{}]", served.iter().map(|s| json_str(s)).collect::<Vec<_>>().join(", ")),
        );
        self.shot("01-split-golden-top").await;

        let view = self.win.preview.view.clone();
        for (name, selector) in [
            ("02-math", ".katex-display"),
            ("03-mermaid", ".mermaid"),
            ("04-plantuml", ".plantuml"),
            ("05-code", "code.hljs"),
        ] {
            let _ = eval_json(
                &view,
                &format!("document.querySelector({})?.scrollIntoView({{block: 'start'}}); window.scrollBy(0, -40); window.scrollY", json_str(selector)),
            )
            .await;
            self.shot(&format!("{name}-golden")).await;
        }

        let (waited, complete) = self.show_and_wait(Page::Diagrams, Duration::from_secs(60)).await;
        self.set(
            "engines.diagrams.render_complete",
            &format!("{{\"flag\": {complete}, \"waited_ms\": {}}}", waited.as_millis()),
        );
        let counts = self.counts().await;
        self.set("engines.diagrams.counts", &counts);
        let _ =
            eval_json(&view, "document.querySelector('.graphviz')?.scrollIntoView({block: 'start'}); window.scrollY")
                .await;
        self.shot("06-graphviz-diagrams").await;
        let _ = self.win.host.take_log();
    }

    // -- 1. Accelerators with focus in the editor or in the web view ----------------------------

    async fn accelerators(&self) {
        // The View menu, opened by a real click, shows the accelerators beside the items.
        let mut item = self.win.window.content().and_then(|c| find_label_widget(&c, "View"));
        if let Some(w) = item.take() {
            let (wd, ht) = (f64::from(w.width()), f64::from(w.height()));
            let clicked = self.click(&w, wd / 2.0, ht / 2.0).await;
            self.set("menubar.view_clicked", if clicked { "true" } else { "false" });
            self.shot("07-view-menu").await;
            let _ = self.xdo(&["key", "Escape"]).await;
            sleep_ms(300).await;
        } else {
            self.set_str("menubar.view_clicked", "View item not found");
        }

        let editor = self.win.editor.clone().upcast::<gtk::Widget>();
        let web = self.win.preview.view.clone().upcast::<gtk::Widget>();
        let cases: [(&str, &gtk::Widget, &str, &str); 6] = [
            ("editor", &editor, "ctrl+3", "preview"),
            ("webview", &web, "ctrl+2", "split"),
            ("webview", &web, "ctrl+1", "edit"),
            ("editor", &editor, "ctrl+2", "split"),
            ("webview", &web, "ctrl+3", "preview"),
            ("webview", &web, "ctrl+2", "split"),
        ];
        let mut rows = Vec::new();
        for (i, (focus_name, widget, keys, expect)) in cases.iter().enumerate() {
            self.focus_window().await;
            widget.grab_focus();
            sleep_ms(250).await;
            let focused = gtk::prelude::GtkWindowExt::focus(&self.win.window)
                .map(|w| w.type_().name().to_owned())
                .unwrap_or_else(|| "none".to_owned());
            let before = self.win.mode_log.borrow().len();
            let _ = self.xdo(&["key", "--clearmodifiers", keys]).await;
            sleep_ms(500).await;
            let fired = self.win.mode_log.borrow().len() - before;
            let mode = self.win.mode();
            rows.push(format!(
                "{{\"case\": {i}, \"focus\": {}, \"focused_widget\": {}, \"keys\": {}, \"expect\": {}, \"mode\": {}, \"fired\": {fired}, \"ok\": {}}}",
                json_str(focus_name),
                json_str(&focused),
                json_str(keys),
                json_str(expect),
                json_str(&mode),
                mode == *expect && fired == 1
            ));
        }
        self.set("accelerators", &format!("[{}]", rows.join(", ")));
        // Leave the window in Split.
        gio::prelude::ActionGroupExt::activate_action(&self.win.window, "view-mode", Some(&"split".to_variant()));
    }

    // -- 2. Typing → 350 ms trailing debounce → reload keeping the scroll position --------------

    async fn typing(&self) {
        let (_, complete) = self.show_and_wait(Page::Text, Duration::from_secs(10)).await;
        self.set("typing.initial_complete", if complete { "true" } else { "false" });
        let view = self.win.preview.view.clone();
        let height = eval_number(&view, "document.documentElement.scrollHeight").await;
        let y = eval_number(&view, "window.scrollTo(0, 1200); window.scrollY").await;
        self.set("typing.page_height", &height.to_string());
        self.set("typing.scrolled_to", &y.to_string());

        let buffer = self.win.editor.buffer();
        buffer.place_cursor(&buffer.end_iter());
        self.focus_window().await;
        self.win.editor.grab_focus();
        sleep_ms(200).await;
        let reloads_before = self.win.preview.stats.borrow().reloads;
        let loaded = self.win.preview.next_load();
        let typed = "Typed under Xvfb.";
        let t0 = Instant::now();
        let _ = self.xdo(&["type", "--delay", "60", typed]).await;
        let typing_ms = t0.elapsed().as_millis();
        let finished = util::with_timeout(Duration::from_secs(10), loaded).await.is_some();
        sleep_ms(300).await;
        let (reloads, gap, saved, at_finished) = {
            let stats = self.win.preview.stats.borrow();
            let gap = match (self.win.last_edit.get(), stats.last_reload_started) {
                (Some(edit), Some(reload)) if reload > edit => reload.duration_since(edit).as_millis() as i64,
                _ => -1,
            };
            (stats.reloads - reloads_before, gap, stats.saved_scroll, stats.scroll_at_finished)
        };
        let final_y = eval_number(&view, "window.scrollY").await;
        let in_page = eval_json(&view, &format!("document.body.innerText.includes({})", json_str(typed)))
            .await
            .unwrap_or_default();
        let in_buffer = self.win.text().contains(typed);
        self.set(
            "typing.result",
            &format!(
                "{{\"typed_chars\": {}, \"typing_ms\": {typing_ms}, \"reload_finished\": {finished}, \"debounced_reloads\": {reloads}, \
                 \"last_edit_to_reload_ms\": {gap}, \"saved_scroll\": {saved}, \"scroll_at_load_finished_before_restore\": {at_finished}, \
                 \"scroll_after_restore\": {final_y}, \"text_in_buffer\": {in_buffer}, \"text_in_preview\": {in_page}}}",
                typed.len()
            ),
        );
        self.shot("08-typing-split").await;
    }

    // -- 1. Single instance: a second launch hands its file to us -------------------------------

    async fn single_instance(&self) {
        let path = std::env::temp_dir().join("md-spike-open.md");
        let _ = std::fs::write(
            &path,
            "# Opened by a second launch\n\nThe second `md` process forwarded this path over D-Bus and exited.\n",
        );
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
        let before = self.win.opened.borrow().len();
        let start = Instant::now();
        let (ok, out) = util::run(&[&exe, &path.to_string_lossy()]).await;
        let took = start.elapsed().as_millis();
        sleep_ms(500).await;
        let opened: Vec<String> = self.win.opened.borrow()[before..].to_vec();
        self.set(
            "single_instance",
            &format!(
                "{{\"second_exit_ok\": {ok}, \"second_ms\": {took}, \"second_output\": {}, \"primary_received\": {}}}",
                json_str(out.trim()),
                json_str(&format!("{opened:?}"))
            ),
        );
        self.shot("09-opened-by-second-launch").await;
    }

    // -- 3. Links: decide-policy, javascript:, the guard, CSP ------------------------------------

    async fn link_round(&self, label: &str, real_click_js: bool) {
        let p = &self.win.preview;
        let view = p.view.clone();
        let page = "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>links</title></head><body style=\"font: 20px sans-serif; padding: 20px\">\
            <p><a id=\"js\" href=\"javascript:document.documentElement.dataset.js='ran';void 0\">javascript: link</a></p>\
            <p><a id=\"ext\" href=\"https://example.com/x\">https link</a></p>\
            <p><a id=\"blank\" href=\"https://example.org/y\" target=\"_blank\">target=_blank link</a></p>\
            <p><a id=\"rel\" href=\"other.md\">relative other.md</a></p>\
            <p><a id=\"file\" href=\"file:///etc/hostname\">file: link</a></p>\
            <p><a id=\"frag\" href=\"#target\">#fragment</a></p>\
            <p><a id=\"inline\" href=\"#\" onclick=\"document.documentElement.dataset.inline='ran'\">inline onclick</a></p>\
            <div style=\"height: 1500px\"></div><h2 id=\"target\">target</h2></body></html>";
        let mut rows = Vec::new();
        for id in ["js", "ext", "blank", "rel", "file", "frag", "inline"] {
            let loaded = p.next_load();
            self.win.show(Page::Html(page.to_owned()));
            let _ = util::with_timeout(Duration::from_secs(10), loaded).await;
            let before = p.stats.borrow().decisions.len();
            let _ = eval_json(&view, &format!("document.getElementById('{id}').click(); 1")).await;
            sleep_ms(500).await;
            let decisions: Vec<String> = p.stats.borrow().decisions[before..]
                .iter()
                .map(|(u, k, g, d)| format!("{k} {u} gesture={g} -> {d:?}"))
                .collect();
            let state = eval_json(
                &view,
                "JSON.stringify({js: document.documentElement.dataset.js || null, inline: document.documentElement.dataset.inline || null, href: location.href, y: window.scrollY})",
            )
            .await
            .unwrap_or_else(|e| json_str(&e));
            rows.push(format!(
                "{{\"link\": {}, \"decisions\": {}, \"page\": {state}}}",
                json_str(id),
                json_str(&format!("{decisions:?}"))
            ));
        }
        if real_click_js {
            let loaded = p.next_load();
            self.win.show(Page::Html(page.to_owned()));
            let _ = util::with_timeout(Duration::from_secs(10), loaded).await;
            let rect = eval_json(&view, "(function(){var r=document.getElementById('js').getBoundingClientRect(); return [r.left + 10, r.top + r.height / 2];})()")
                .await
                .unwrap_or_default();
            let nums: Vec<f64> =
                rect.trim_matches(['[', ']']).split(',').filter_map(|s| s.trim().parse().ok()).collect();
            let before = p.stats.borrow().decisions.len();
            let clicked = if let [x, y] = nums[..] { self.click(&view, x, y).await } else { false };
            sleep_ms(500).await;
            let decisions: Vec<String> = p.stats.borrow().decisions[before..]
                .iter()
                .map(|(u, k, g, d)| format!("{k} {u} gesture={g} -> {d:?}"))
                .collect();
            let state = eval_json(&view, "JSON.stringify({js: document.documentElement.dataset.js || null})")
                .await
                .unwrap_or_default();
            rows.push(format!(
                "{{\"link\": \"js (real mouse click)\", \"clicked\": {clicked}, \"decisions\": {}, \"page\": {state}}}",
                json_str(&format!("{decisions:?}"))
            ));
        }
        self.set(&format!("links.{label}"), &format!("[{}]", rows.join(", ")));
    }

    async fn links(&self) {
        let p = &self.win.preview;
        // A: no guard — what decide-policy sees on its own.
        p.ucm.remove_all_scripts();
        self.link_round("no_guard", true).await;
        // B: the md.win link guard as a document-start user script.
        p.ucm.add_script(&preview::link_guard_script());
        self.link_round("guard", true).await;
        // C: no guard, but a CSP header without 'unsafe-inline' on every md-asset response.
        p.ucm.remove_all_scripts();
        self.win.host.set_csp(true);
        self.link_round("csp_only", false).await;
        self.win.host.set_csp(false);
        p.ucm.add_script(&preview::link_guard_script());
    }

    // -- 7. Light/dark ---------------------------------------------------------------------------

    async fn dark_mode(&self) {
        let sm = adw::StyleManager::default();
        self.set(
            "dark.initial",
            &format!(
                "{{\"is_dark\": {}, \"system_supports_color_schemes\": {}, \"color_scheme\": {}}}",
                sm.is_dark(),
                sm.system_supports_color_schemes(),
                json_str(&format!("{:?}", sm.color_scheme()))
            ),
        );
        let loaded = self.win.preview.next_load();
        self.app.activate_action("color-scheme", Some(&"dark".to_variant()));
        let _ = util::with_timeout(Duration::from_secs(10), loaded).await;
        self.win.show(Page::Golden);
        let (_, complete) = self.win.preview.wait_render_complete(Duration::from_secs(60)).await;
        let media = eval_json(&self.win.preview.view, "matchMedia('(prefers-color-scheme: dark)').matches")
            .await
            .unwrap_or_default();
        let body = eval_json(&self.win.preview.view, "document.body.dataset.mdDark").await.unwrap_or_default();
        self.set(
            "dark.forced",
            &format!(
                "{{\"is_dark\": {}, \"window_dark\": {}, \"render_complete\": {complete}, \"web_prefers_dark\": {media}, \"body_md_dark\": {body}}}",
                sm.is_dark(),
                self.win.dark.get()
            ),
        );
        self.shot("10-dark-golden").await;
        let _ = eval_json(&self.win.preview.view, "document.querySelector('.mermaid')?.scrollIntoView(); 1").await;
        self.shot("11-dark-mermaid").await;
        self.app.activate_action("color-scheme", Some(&"light".to_variant()));
        sleep_ms(500).await;
        let media = eval_json(&self.win.preview.view, "matchMedia('(prefers-color-scheme: dark)').matches")
            .await
            .unwrap_or_default();
        self.set("dark.back_to_light", &format!("{{\"is_dark\": {}, \"web_prefers_dark\": {media}}}", sm.is_dark()));
        self.app.activate_action("color-scheme", Some(&"system".to_variant()));
    }

    // -- 7. Fonts --------------------------------------------------------------------------------

    async fn fonts(&self) {
        let families = [
            "American Typewriter",
            "Courier New",
            "Courier",
            "Nimbus Mono PS",
            "Courier Prime",
            "Liberation Mono",
            "DejaVu Sans Mono",
            "Ubuntu Mono",
            "Ubuntu Sans Mono",
            "Noto Sans Mono",
            "Lucida Sans Typewriter",
        ];
        let mut rows = String::new();
        for f in families {
            rows.push_str(&format!(
                "<tr><td class=\"n\">{}</td><td style=\"font-family: '{}', 'Noto Color Emoji', cursive\">The quick brown fox — <b>bold</b> <i>italic</i> <b><i>both</i></b> 0123 Привет</td></tr>",
                standin::escape(f),
                f
            ));
        }
        let specimen = format!(
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>fonts</title><style>body{{background:#F4EFE2;color:#2B2620;padding:24px;font-size:16pt}} td{{padding:4px 10px}} .n{{font: 11pt sans-serif; color:#6B635A}}</style></head><body>\
             <h3 style=\"font-family: sans-serif\">Faces on the Ubuntu 26.04 desktop (cursive fallback = missing)</h3><table>{rows}</table>\
             <p style='font-family: \"American Typewriter\", \"Courier New\", serif'>Shared stylesheet stack: \"American Typewriter\", \"Courier New\", serif.</p>\
             <p style='font-family: \"Nimbus Mono PS\", \"Courier New\", serif'>Proposed Linux prose stack: \"Nimbus Mono PS\", \"Courier New\", serif.</p></body></html>"
        );
        let loaded = self.win.preview.next_load();
        self.win.show(Page::Html(specimen));
        let _ = util::with_timeout(Duration::from_secs(10), loaded).await;
        let script = format!(
            "(function(){{ var c=document.createElement('canvas').getContext('2d'); var s='mmmmmmmmmmlli1WQ@#Ж—'; \
             function w(f){{ c.font='72px '+f; return c.measureText(s).width; }} \
             var out={{}}; {list}.forEach(function(f){{ out[f] = ['monospace','serif','sans-serif'].some(function(b){{ return w('\"'+f+'\", '+b) !== w(b); }}); }}); \
             var stack=w('\"American Typewriter\", \"Courier New\", serif'); \
             out['= shared stack resolves like'] = {list}.filter(function(f){{ return w('\"'+f+'\"') === stack; }}); \
             return JSON.stringify(out); }})()",
            list = format!("[{}]", families.iter().map(|f| json_str(f)).collect::<Vec<_>>().join(","))
        );
        let detected = eval_json(&self.win.preview.view, &script).await.unwrap_or_else(|e| json_str(&e));
        self.set("fonts.webkit_detected", &detected);

        // What Pango gives the editor for its CSS font-family list.
        let ctx = self.win.editor.pango_context();
        let used = ctx
            .font_description()
            .and_then(|desc| ctx.load_font(&desc).map(|font| (desc.to_string(), font.describe().to_string())));
        self.set_str("fonts.editor_pango", &format!("{used:?}"));
        // Leave the editor showing the proposal in both panes for the screenshot.
        let _ = eval_json(&self.win.preview.view, "window.scrollTo(0,0); 1").await;
        self.shot("12-fonts").await;

        let (ok, out) = util::run(&["fc-match", "-s", "American Typewriter"]).await;
        let head: Vec<&str> = out.lines().take(3).collect();
        self.set_str("fonts.fc_match_american_typewriter", &format!("{ok} {head:?}"));
    }

    // -- 5. PDF without a dialog -----------------------------------------------------------------

    async fn pdf(&self) {
        let golden_export = standin::golden_document_export();
        let (_, complete) = self.show_and_wait(Page::Html(golden_export), Duration::from_secs(60)).await;
        self.set("pdf.source_render_complete", if complete { "true" } else { "false" });
        for (name, paper) in [("golden-a4", Paper::A4), ("golden-6x9", Paper::Inches(6.0, 9.0))] {
            let path = self.out.join(format!("{name}.pdf"));
            let _ = std::fs::remove_file(&path);
            let res = export::print_pdf(&self.win.preview.view, &path, paper, 0.5).await;
            let info = export::pdf_info(&path).await;
            self.set(
                &format!("pdf.{name}"),
                &format!(
                    "{{\"result\": {}, \"pdfinfo\": {}}}",
                    json_str(&format!("{res:?}")),
                    json_str(&format!("{info:?}"))
                ),
            );
            let prefix = self.out.join(format!("{name}-page"));
            let _ = util::run(&[
                "pdftoppm",
                "-png",
                "-r",
                "40",
                "-f",
                "1",
                "-l",
                "2",
                &path.to_string_lossy(),
                &prefix.to_string_lossy(),
            ])
            .await;
        }
    }

    // -- 6. Snapshots and the offscreen renderer -------------------------------------------------

    async fn offscreen_case(&self, label: &str, placement: &str) {
        let (ctx, host) = (self.win.context.clone(), Rc::clone(&self.win.host));
        let key = format!("export-{label}");
        let p = Preview::new(&ctx, host, &key, false);
        // 595 CSS px at 2× — the page width md's EPUB snapshots lay out at.
        p.view.set_size_request(1190, 842);
        // "overlay": mapped inside the window, transparent and click-through; anything else is
        // never parented (unrealized, unmapped).
        if placement == "overlay" {
            p.view.set_opacity(0.0);
            p.view.set_can_target(false);
            p.view.set_can_focus(false);
            p.view.set_halign(gtk::Align::Start);
            p.view.set_valign(gtk::Align::Start);
            self.win.overlay.add_overlay(&p.view);
        }
        sleep_ms(200).await;
        let loaded = p.next_load();
        p.update(standin::golden_document_export(), true);
        let load_ok = util::with_timeout(Duration::from_secs(20), loaded).await.is_some();
        let (waited, complete) = p.wait_render_complete(Duration::from_secs(60)).await;
        let viewport = eval_json(&p.view, "JSON.stringify([window.innerWidth, window.innerHeight, document.visibilityState, document.documentElement.scrollHeight])")
            .await
            .unwrap_or_else(|e| json_str(&e));
        let counts = eval_json(
            &p.view,
            "JSON.stringify({katex: document.querySelectorAll('.katex').length, mermaidSvg: document.querySelectorAll('.mermaid svg').length, plantumlSvg: document.querySelectorAll('.plantuml svg').length})",
        )
        .await
        .unwrap_or_else(|e| json_str(&e));
        self.set(
            &format!("offscreen.{label}.load"),
            &format!(
                "{{\"mapped\": {}, \"load_finished\": {load_ok}, \"render_complete\": {complete}, \"waited_ms\": {}, \"viewport\": {viewport}, \"counts\": {counts}}}",
                p.view.is_mapped(),
                waited.as_millis()
            ),
        );
        let shot = self.out.join(format!("katex-2x-{label}.png"));
        let res = export::snapshot_element(&p.view, ".katex-display", 2.0, &shot).await;
        self.set_str(&format!("offscreen.{label}.snapshot"), &format!("{} -> {}", shot_desc(&res), shot.display()));
        let pdf = self.out.join(format!("offscreen-{label}-a4.pdf"));
        let _ = std::fs::remove_file(&pdf);
        let res = export::print_pdf(&p.view, &pdf, Paper::A4, 0.5).await;
        let info = export::pdf_info(&pdf).await;
        self.set_str(&format!("offscreen.{label}.pdf"), &format!("{res:?} {info:?}"));
        if placement == "overlay" {
            self.win.overlay.remove_overlay(&p.view);
        }
    }

    async fn snapshots(&self) {
        // The visible preview first: the reference result.
        let (_, complete) = self.show_and_wait(Page::Golden, Duration::from_secs(60)).await;
        let path = self.out.join("katex-2x-preview.png");
        let res = export::snapshot_element(&self.win.preview.view, ".katex-display", 2.0, &path).await;
        self.set_str("snapshot.preview", &format!("complete={complete} {} -> {}", shot_desc(&res), path.display()));
        let path = self.out.join("katex-1x-preview.png");
        let res = export::snapshot_element(&self.win.preview.view, ".katex-display", 1.0, &path).await;
        self.set_str("snapshot.preview_1x", &shot_desc(&res));
        let path = self.out.join("mermaid-2x-preview.png");
        let res = export::snapshot_element(&self.win.preview.view, ".mermaid", 2.0, &path).await;
        self.set_str("snapshot.preview_mermaid", &shot_desc(&res));
        self.offscreen_case("unmapped", "none").await;
        self.offscreen_case("overlay", "overlay").await;
    }

    // -- 8. GtkFileDialog ------------------------------------------------------------------------

    async fn file_dialogs(&self) {
        // The callback form with our own cancellable, so the run can close the dialog
        // deterministically; the window's actions use the `_future` forms.
        let markdown = gtk::FileFilter::new();
        markdown.set_name(Some("Markdown"));
        markdown.add_suffix("md");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&markdown);
        let dialog = gtk::FileDialog::builder().title("Open").modal(true).filters(&filters).build();
        let cancel = gio::Cancellable::new();
        let (tx, rx) = util::oneshot::<String>();
        dialog.open_text_file(Some(&self.win.window), Some(&cancel), move |res| {
            tx.send(match res {
                Ok((file, enc)) => format!("Ok({:?}, {enc})", file.path()),
                Err(err) => format!("Err({:?}: {})", err.kind::<gtk::DialogError>(), err.message()),
            });
        });
        sleep_ms(2500).await;
        self.shot("13-open-dialog").await;
        let _ = self.xdo(&["key", "Escape"]).await;
        let escaped = util::with_timeout(Duration::from_secs(3), rx).await;
        let result = match escaped {
            Some(r) => format!("dismissed by Escape: {r}"),
            None => {
                cancel.cancel();
                "Escape did not close it; cancelled via GCancellable".to_owned()
            }
        };
        self.set_str("dialogs.open_text_file", &result);

        let dialog = gtk::FileDialog::builder().title("Save As").modal(true).initial_name("Untitled.md").build();
        let cancel = gio::Cancellable::new();
        let (tx, rx) = util::oneshot::<String>();
        dialog.save(Some(&self.win.window), Some(&cancel), move |res| {
            tx.send(match res {
                Ok(file) => format!("Ok({:?})", file.path()),
                Err(err) => format!("Err({:?}: {})", err.kind::<gtk::DialogError>(), err.message()),
            });
        });
        sleep_ms(2500).await;
        self.shot("14-save-dialog").await;
        cancel.cancel();
        let r = util::with_timeout(Duration::from_secs(3), rx).await;
        self.set_str("dialogs.save_cancelled", &format!("{r:?}"));
    }

    // -- 8. GFileMonitor -------------------------------------------------------------------------

    async fn file_monitor(&self) {
        let dir = std::env::temp_dir().join("md-spike-watch");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("doc.md");
        let _ = std::fs::write(&path, "one\n");
        let file = gio::File::for_path(&path);
        let events: Rc<RefCell<Vec<String>>> = Rc::default();
        let mut monitors = Vec::new();
        for (label, flags) in [("moves", gio::FileMonitorFlags::WATCH_MOVES), ("plain", gio::FileMonitorFlags::NONE)] {
            match file.monitor_file(flags, gio::Cancellable::NONE) {
                Ok(m) => {
                    let sink = Rc::clone(&events);
                    m.connect_changed(move |_, f, other, event| {
                        sink.borrow_mut().push(format!(
                            "{label}: {event:?} {} {}",
                            f.basename().map(|b| b.display().to_string()).unwrap_or_default(),
                            other.and_then(|o| o.basename()).map(|b| b.display().to_string()).unwrap_or_default()
                        ));
                    });
                    monitors.push(m);
                }
                Err(err) => self.set_str(&format!("monitor.{label}"), &err.to_string()),
            }
        }
        if let Some(m) = monitors.first() {
            self.set_str("monitor.type", m.type_().name());
        }
        let phase = |name: &str| events.borrow_mut().push(format!("--- {name}"));
        sleep_ms(300).await;

        phase("append in place");
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&path) {
            use std::io::Write;
            let _ = f.write_all(b"two\n");
        }
        sleep_ms(1500).await;

        phase("write temp + rename over (atomic save)");
        let tmp = dir.join(".doc.md.tmp");
        let _ = std::fs::write(&tmp, "three\n");
        let _ = std::fs::rename(&tmp, &path);
        sleep_ms(1500).await;

        phase("g_file_replace_contents (GIO's own atomic save)");
        let _ = file.replace_contents(b"four\n", None, false, gio::FileCreateFlags::NONE, gio::Cancellable::NONE);
        sleep_ms(1500).await;

        phase("delete");
        let _ = std::fs::remove_file(&path);
        sleep_ms(1500).await;

        self.set(
            "monitor.events",
            &format!("[{}]", events.borrow().iter().map(|e| json_str(e)).collect::<Vec<_>>().join(", ")),
        );
        drop(monitors);
    }
}

fn shot_desc(res: &Result<export::ElementShot, String>) -> String {
    match res {
        Ok(shot) => shot.describe(),
        Err(err) => format!("failed: {err}"),
    }
}

/// The first descendant `GtkLabel` whose text (mnemonic stripped) is `text`, returned as its
/// parent (the clickable menubar item).
fn find_label_widget(root: &gtk::Widget, text: &str) -> Option<gtk::Widget> {
    let mut child = root.first_child();
    while let Some(w) = child {
        if let Some(label) = w.downcast_ref::<gtk::Label>()
            && label.text() == text
        {
            return w.parent();
        }
        if let Some(found) = find_label_widget(&w, text) {
            return Some(found);
        }
        child = w.next_sibling();
    }
    None
}

/// `md --probe <variant> <dir>`: one fresh process, one `WebContext`, the scheme registered
/// exactly as `variant` says and nothing else — WebKit keeps scheme registrations per process,
/// so variants must not share one (an in-process matrix showed `local` leaking into the next
/// context's loads). Loads the golden document and the Diagrams example in a visible window and
/// writes `<dir>/probe-<variant>.json`.
pub fn probe(app: &adw::Application, variant: &str, out: PathBuf) {
    let (secure, cors, local, csp) =
        (variant.contains("secure"), variant.contains("cors"), variant.contains("local"), variant.contains("csp"));
    let security = SchemeSecurity { secure, cors, local, csp };
    let ctx = webkit::WebContext::new();
    let host = AssetHost::new(AssetHost::default_rich_dir());
    host.attach(&ctx, security);
    let p = Preview::new(&ctx, Rc::clone(&host), "probe", false);
    let win = gtk::ApplicationWindow::builder()
        .application(app)
        .title("md probe")
        .default_width(900)
        .default_height(700)
        .child(&p.view)
        .build();
    win.present();
    let _ = std::fs::create_dir_all(&out);
    let hold = app.hold();
    let app = app.clone();
    let variant = variant.to_owned();
    glib::spawn_future_local(async move {
        let mut rows = vec![format!("\"security\": {}", json_str(&format!("{security:?}")))];
        // MD_PROBE_IMAGE=<url>: also load that image into the page (mixed-content check).
        let image = std::env::var("MD_PROBE_IMAGE").unwrap_or_default();
        for (page, html) in [("golden", standin::golden_document(false)), ("diagrams", standin::golden_diagrams(false))]
        {
            let html = if image.is_empty() {
                html
            } else {
                html.replacen(
                    "<body data-md-dark=\"0\">",
                    &format!("<body data-md-dark=\"0\"><img id=\"probe-img\" src=\"{image}\">"),
                    1,
                )
            };
            let loaded = p.next_load();
            p.update(html, true);
            let load_ok = util::with_timeout(Duration::from_secs(20), loaded).await.is_some();
            let (waited, complete) =
                if load_ok { p.wait_render_complete(Duration::from_secs(30)).await } else { (Duration::ZERO, false) };
            let counts = eval_json(
                &p.view,
                "JSON.stringify({katex: document.querySelectorAll('.katex').length, hljs: document.querySelectorAll('code.hljs').length, \
                 mermaidSvg: document.querySelectorAll('.mermaid svg').length, graphvizSvg: document.querySelectorAll('.graphviz svg').length, \
                 plantumlSvg: document.querySelectorAll('.plantuml svg').length, katexFont: document.fonts.check('16px KaTeX_Main'), \
                 origin: location.origin, secure: window.isSecureContext, subtle: !!(window.crypto && crypto.subtle), \
                 image: (function(i){ return i ? [i.currentSrc, i.complete, i.naturalWidth] : null; })(document.getElementById('probe-img'))})",
            )
            .await
            .unwrap_or_else(|e| json_str(&e));
            let non_200: Vec<String> =
                host.take_log().into_iter().filter(|s| s.status != 200).map(|s| s.path).collect();
            let terminations = p.stats.borrow().terminations.clone();
            // MD_PROBE_PDF=1: print this page too (MD_PROBE_PRINTER overrides the printer name,
            // to show what a hard-coded English name does under a translated desktop).
            if std::env::var("MD_PROBE_PDF").is_ok_and(|v| v == "1") {
                let pdf = out.join(format!("probe-{variant}-{page}.pdf"));
                let _ = std::fs::remove_file(&pdf);
                let printer = std::env::var("MD_PROBE_PRINTER").unwrap_or_else(|_| export::file_printer_name());
                let res = export::print_pdf_with_printer(&p.view, &pdf, Paper::A4, 0.5, &printer).await;
                let info = export::pdf_info(&pdf).await;
                rows.push(format!(
                    "\"pdf-{page}\": {}",
                    json_str(&format!(
                        "printer={printer:?} localized={:?} {res:?} {info:?}",
                        export::file_printer_name()
                    ))
                ));
            }
            rows.push(format!(
                "{}: {{\"load\": {load_ok}, \"complete\": {complete}, \"waited_ms\": {}, \"counts\": {counts}, \"non_200\": {}, \"terminations\": {}}}",
                json_str(page),
                waited.as_millis(),
                json_str(&format!("{non_200:?}")),
                json_str(&format!("{terminations:?}"))
            ));
        }
        let text = format!("{{{}}}\n", rows.join(", "));
        println!("PROBE {variant} {text}");
        let _ = std::fs::write(out.join(format!("probe-{variant}.json")), text);
        drop(hold);
        app.quit();
    });
}
