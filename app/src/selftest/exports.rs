//! The `exports` self-test group (shell-design §13.3): every export writes a valid file into
//! `OUT/exports/` — PDF at A4, 6 × 9 in and (through the book sink) A5, three `\newpage` sections
//! three pages, checked with `pdfinfo` / `pdffonts`; HTML standing alone (opened in a fresh view
//! with no `rich/` request, KaTeX inlined only with math); EPUB with as many 2× snapshots as the
//! markup has rich elements, valid non-blank PNGs, the front-matter cover from a file beside the
//! document; a 40-page document's last formula (the visible-region path); an SVG per diagram kind
//! and the alert for one that failed to draw; LaTeX; a TextBundle with its image and the Replace
//! question; Print reaching GTK's print dialog and cancelled; the footer ring; the libsoup cover
//! fetch against a local server; no `md-linux-fonts` in any exported file; no renderer left behind.
//!
//! The group's window is its own — an overlay for the renderers, scripted pickers and alerts
//! (md-logic's fakes) — and its exports write through md-logic's `LocalFileSystem`, so it proves
//! the export half without depending on the document window or the file dialogs of the other
//! packages. Print goes through `WindowExports::new` exactly as a window builds it.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::gdk;
use md_core::document::page_size::PageSize;
use md_core::export::{epub_export, zip_reader};
use md_logic::commands::ShellSnapshot;
use md_logic::documents::file_identity::LocalFileIdentity;
use md_logic::documents::local_file_system::LocalFileSystem;
use md_logic::exec::{LocalBoxFuture, Spawner};
use md_logic::export::{CoverFetcher, cover_reader};
use md_logic::preview::screen_html;
use md_logic::seams::{Alerts, Pickers, RenderKind, RenderSurface, WindowId};
use md_logic::settings::{SettingsStore, settings_keys};
use md_logic::strings::exports as words;
use md_logic::testing::{FakeAlerts, FakePickers};
use webkit::prelude::*;
use webkit::soup;

use crate::export::cover_fetch::SoupCoverFetcher;
use crate::export::pdf::PrintOutcome;
use crate::export::renderer;
use crate::export::window_exports::WindowExports;
use crate::selftest::Harness;
use crate::windows::host::{WindowHost, WindowKind};

const TITLE: &str = "md self-test: exports";

/// How long one export may take (an EPUB of diagrams waits for PlantUML).
const EXPORT_LIMIT: Duration = Duration::from_secs(180);

/// The window the exports render in: an overlay, md-logic's scripted dialogs, and a count of the
/// outputs handed to `track_output`.
struct TestHost {
    window: adw::ApplicationWindow,
    overlay: gtk::Overlay,
    alerts: Rc<FakeAlerts>,
    pickers: Rc<FakePickers>,
    started: Cell<u32>,
    finished: Rc<Cell<u32>>,
}

impl WindowHost for TestHost {
    fn id(&self) -> WindowId {
        WindowId(u64::MAX - 4)
    }

    fn kind(&self) -> WindowKind {
        WindowKind::Document
    }

    fn window(&self) -> gtk::Window {
        self.window.clone().upcast()
    }

    fn overlay(&self) -> gtk::Overlay {
        self.overlay.clone()
    }

    fn alerts(&self) -> Rc<dyn Alerts> {
        self.alerts.clone()
    }

    fn pickers(&self) -> Rc<dyn Pickers> {
        self.pickers.clone()
    }

    fn track_output(&self, output: LocalBoxFuture<'static, ()>) {
        self.started.set(self.started.get() + 1);
        let finished = Rc::clone(&self.finished);
        crate::services::glib_seams::GlibSpawner.spawn_local(Box::pin(async move {
            output.await;
            finished.set(finished.get() + 1);
        }));
    }

    fn snapshot(&self) -> ShellSnapshot {
        ShellSnapshot::empty()
    }

    fn publish(&self) {}

    fn is_dark(&self) -> bool {
        false
    }
}

struct Exports<'a> {
    h: &'a Harness,
    out: PathBuf,
    host: Rc<TestHost>,
    exports: Rc<WindowExports>,
    busy: Rc<RefCell<Vec<bool>>>,
}

pub async fn run(h: &Harness) {
    let shell = h.shell();
    let out = h.out().join("exports");
    let _ = std::fs::remove_dir_all(&out);
    if let Err(err) = std::fs::create_dir_all(&out) {
        h.check("output_folder", false, format!("{}: {err}", out.display()));
        return;
    }

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&gtk::Label::new(Some(TITLE))));
    let window = adw::ApplicationWindow::builder()
        .application(&shell.app)
        .title(TITLE)
        .default_width(1000)
        .default_height(760)
        .content(&overlay)
        .build();
    window.present();
    h.next_frame(&window).await;

    let host = Rc::new(TestHost {
        window,
        overlay,
        alerts: Rc::new(FakeAlerts::new()),
        pickers: Rc::new(FakePickers::new()),
        started: Cell::new(0),
        finished: Rc::new(Cell::new(0)),
    });
    let dyn_host: Rc<dyn WindowHost> = host.clone();
    let weak: Weak<dyn WindowHost> = Rc::downgrade(&dyn_host);
    let exports = WindowExports::with_files(&shell, weak, Rc::new(LocalFileSystem), Rc::new(LocalFileIdentity));
    let busy = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&busy);
    exports.busy_changed().connect(move |&b| sink.borrow_mut().push(b));
    let t = Exports { h, out, host, exports, busy };

    t.renderer_is_live().await;
    t.pdf().await;
    t.html().await;
    t.epub().await;
    t.long_epub().await;
    t.svg().await;
    t.latex().await;
    t.text_bundle().await;
    t.book_outputs().await;
    t.cover_fetch().await;
    t.print(&dyn_host).await;
    t.leftovers();

    t.exports.cancel();
    t.host.window.close();
    drop(dyn_host);
}

impl Exports<'_> {
    fn path(&self, name: &str) -> PathBuf {
        self.out.join(name)
    }

    /// Waits until every output handed to the window has finished.
    async fn settle(&self) -> bool {
        let host = &self.host;
        self.h.wait_until(EXPORT_LIMIT, || host.finished.get() == host.started.get()).await
    }

    /// Runs one export answering its save dialog with `file`; true when it finished in time.
    async fn save_as(&self, file: &Path, export: impl FnOnce(&WindowExports)) -> bool {
        let _ = std::fs::remove_file(file);
        self.host.pickers.queue_save_answer(Some(&file.to_string_lossy()));
        export(&self.exports);
        self.settle().await
    }

    fn warnings_since(&self, count: usize) -> Vec<String> {
        self.host.alerts.warnings()[count..].iter().map(|w| format!("{}: {}", w.title, w.message)).collect()
    }

    // ── the renderer ──────────────────────────────────────────────────────────────────────────

    /// The overlay renderer is mapped and live: the page is visible at 595 × 842, frames come, and
    /// the engines finish — the conditions the snapshots rely on (docs/linux-facts.md §6).
    async fn renderer_is_live(&self) {
        let h = self.h;
        let web = &h.shell().web;
        let renderer = renderer::create_in(&self.host.overlay, web, RenderKind::Export).await;
        let started = std::time::Instant::now();
        let page = md_core::markdown::html::document(
            "```mermaid\nflowchart LR\n  A --> B\n```\n\n$x^2$\n",
            "Live",
            false,
            true,
        );
        let loaded =
            h.with_timeout(Duration::from_secs(150), renderer.load(page, md_logic::exec::CancelToken::new())).await;
        let load_time = started.elapsed();
        let view = renderer.view().clone();
        let state = crate::web::js::eval_json(&view, "[document.visibilityState, window.innerWidth, window.innerHeight, document.querySelectorAll('pre.mermaid svg').length, document.documentElement.getAttribute('data-md-render-complete')]").await;
        let frames = h.with_timeout(Duration::from_secs(3), crate::web::js::two_frames(&view)).await;
        renderer.close().await;
        h.check(
            "renderer_is_live",
            matches!(loaded, Some(Ok(())))
                && state.as_deref() == Ok("[\"visible\",595,842,1,\"1\"]")
                && matches!(frames, Some(Ok(()))),
            format!("loaded {loaded:?} in {load_time:?}; page {state:?}; two frames {frames:?}"),
        );
    }

    // ── PDF ───────────────────────────────────────────────────────────────────────────────────

    async fn pdf(&self) {
        let h = self.h;
        let source = "# One\n\nThe first section.\n\n\\newpage\n\n# Two\n\nThe second section.\n\n\\newpage\n\n# Three\n\nThe third section.\n";
        let settings = &h.shell().settings;
        for (id, file, (w, ht)) in
            [("a4", "three-pages-a4.pdf", (595.2, 841.8)), ("6x9", "three-pages-6x9.pdf", (432.0, 648.0))]
        {
            settings.set_string(settings_keys::PDF_PAGE_SIZE, id);
            let warned = self.host.alerts.warnings().len();
            let path = self.path(file);
            let done = self.save_as(&path, |e| e.export_pdf(source.to_string(), "Three Pages".to_string())).await;
            let info = pdf_info(h, &path).await;
            let fonts = h.run(&["pdffonts", &path.to_string_lossy()]).await.1;
            let suggested = self.host.pickers.save_calls().last().map(|c| c.suggested_name.clone()).unwrap_or_default();
            h.check(
                &format!("pdf_{}", id.replace('x', "_by_")),
                done && info
                    .as_ref()
                    .is_some_and(|i| i.pages == 3 && (i.width - w).abs() <= 1.0 && (i.height - ht).abs() <= 1.0)
                    && fonts.contains("NimbusMonoPS")
                    && suggested == "Three Pages.pdf"
                    && self.warnings_since(warned).is_empty(),
                format!(
                    "{}: {info:?}; Nimbus Mono PS embedded: {}; suggested {suggested:?}; alerts {:?}",
                    path.display(),
                    fonts.contains("NimbusMonoPS"),
                    self.warnings_since(warned)
                ),
            );
        }
        settings.set_string(settings_keys::PDF_PAGE_SIZE, PageSize::A4.id);

        // The book sink renders through the same pipeline, at the size it is handed.
        let path = self.path("book-a5.pdf");
        let _ = std::fs::remove_file(&path);
        self.host.pickers.queue_save_answer(Some(&path.to_string_lossy()));
        let output = self.exports.book_outputs().export_pdf(source.to_string(), "Book".to_string(), PageSize::A5);
        let done = h.with_timeout(EXPORT_LIMIT, output).await.is_some();
        let info = pdf_info(h, &path).await;
        h.check(
            "book_pdf_a5",
            done && info
                .as_ref()
                .is_some_and(|i| i.pages == 3 && (i.width - 419.5).abs() <= 1.0 && (i.height - 595.3).abs() <= 1.0),
            format!("{info:?}"),
        );
    }

    // ── HTML ──────────────────────────────────────────────────────────────────────────────────

    async fn html(&self) {
        let h = self.h;
        let math = "# Math\n\nInline $e^{i\\pi} + 1 = 0$ and a display:\n\n$$\\int_0^1 x^2\\,dx = \\frac{1}{3}$$\n";
        let plain = "# Plain\n\nNo math here, only *words*.\n";
        let with_math = self.path("math.html");
        let without = self.path("plain.html");
        let done = self.save_as(&with_math, |e| e.export_html(math.to_string(), "Math".to_string())).await
            && self.save_as(&without, |e| e.export_html(plain.to_string(), "Plain".to_string())).await;
        let page = std::fs::read_to_string(&with_math).unwrap_or_default();
        let plain_page = std::fs::read_to_string(&without).unwrap_or_default();
        h.check(
            "html_inlines_katex_only_with_math",
            done && page.contains("data:font/woff2")
                && page.contains("class=\"katex")
                && !page.contains("<script")
                && !page.contains("rich/")
                && !plain_page.is_empty()
                && !plain_page.contains("@font-face")
                && !plain_page.contains("KaTeX_"),
            format!("math.html {} bytes, plain.html {} bytes", page.len(), plain_page.len()),
        );
        h.check(
            "html_has_no_linux_fonts",
            !page.is_empty() && !page.contains(screen_html::ID_MARKER) && !plain_page.contains(screen_html::ID_MARKER),
            "",
        );

        // Stands alone: served as a page of its own (under the CSP), it asks for nothing else and
        // its inlined KaTeX faces load.
        let web = &h.shell().web;
        let assets = web.asset_host();
        let view = web.new_view(&webkit::UserContentManager::new());
        view.set_opacity(renderer::OPACITY);
        view.set_can_target(false);
        view.set_halign(gtk::Align::Start);
        view.set_valign(gtk::Align::Start);
        view.set_size_request(595, 842);
        self.host.overlay.add_overlay(&view);
        assets.set_page("standalone-1", page);
        let _ = assets.take_log();
        let loaded: md_logic::exec::Completion<()> = md_logic::exec::Completion::new();
        let signal = loaded.clone();
        view.connect_load_changed(move |_, event| {
            if event == webkit::LoadEvent::Finished {
                signal.complete(());
            }
        });
        view.load_uri("md-asset://standalone-1/index.html");
        let finished = h.with_timeout(Duration::from_secs(30), loaded.wait()).await.is_some();
        let faces = view
            .call_async_javascript_function_future(
                "await document.fonts.ready; \
                 return [document.querySelectorAll('.katex').length, document.compatMode, \
                         [...document.fonts].filter(f => f.status === 'loaded' && f.family.includes('KaTeX')).length];",
                None,
                None,
                None,
            )
            .await
            .ok()
            .and_then(|v| v.to_json(0))
            .map(|s| s.to_string())
            .unwrap_or_default();
        let requests: Vec<String> = assets.take_log().into_iter().map(|s| format!("{} {}", s.status, s.uri)).collect();
        let parsed = md_logic::preview::json_script::parse(&faces);
        let (katex, mode, loaded_faces) = match &parsed {
            Some(md_logic::preview::json_script::Json::Array(v)) if v.len() == 3 => (
                json_number(&v[0]),
                match &v[1] {
                    md_logic::preview::json_script::Json::String(s) => s.clone(),
                    _ => String::new(),
                },
                json_number(&v[2]),
            ),
            _ => (f64::NAN, String::new(), f64::NAN),
        };
        h.check(
            "html_stands_alone",
            finished && katex >= 2.0 && mode == "CSS1Compat" && loaded_faces >= 1.0 && requests.len() == 1,
            format!(".katex {katex}, {mode}, KaTeX faces loaded {loaded_faces}; requests {requests:?}"),
        );
        h.screenshot("html-standalone").await;
        self.host.overlay.remove_overlay(&view);
        assets.remove_page("standalone-1");
    }

    // ── EPUB ──────────────────────────────────────────────────────────────────────────────────

    async fn epub(&self) {
        let h = self.h;
        let folder = self.path("epub-source");
        let _ = std::fs::create_dir_all(&folder);
        let cover = solid_png(60, 90, [0x8a, 0x2b, 0x1e]);
        let _ = std::fs::write(folder.join("cover.png"), &cover);
        let document = folder.join("snapshots.md");
        let source = "---\ntitle: Snapshot Test\ncover: cover.png\n---\n\n# Snapshots\n\n\
            Inline $E = mc^2$ and a display:\n\n$$\\sum_{n=1}^{\\infty} \\frac{1}{n^2} = \\frac{\\pi^2}{6}$$\n\n\
            ```mermaid\nflowchart LR\n  A[Write] --> B[Export]\n```\n\n\
            ```dot\ndigraph { rankdir=LR; Draft -> Review -> Published; }\n```\n\n\
            ```plantuml\n@startuml\nAlice -> Bob: Hello\n@enduml\n```\n\n\
            $$\\ce{H2O}$$\n";
        let _ = std::fs::write(&document, source);
        let expected: usize = epub_export::plan_document(source, "Snapshot Test")
            .rich_units()
            .iter()
            .map(|u| u.rich_elements.len())
            .sum();

        let warned = self.host.alerts.warnings().len();
        let path = self.path("snapshots.epub");
        let done = self
            .save_as(&path, |e| {
                e.export_epub(
                    source.to_string(),
                    "snapshots.md".to_string(),
                    Some(document.to_string_lossy().into_owned()),
                )
            })
            .await;
        let suggested = self.host.pickers.save_calls().last().map(|c| c.suggested_name.clone()).unwrap_or_default();
        let book = Epub::read(&path);
        let shots = book.snapshots();
        let checked: Vec<String> = shots.iter().map(|s| s.describe()).collect();
        h.check(
            "epub_snapshot_count",
            done && book.valid() && expected == 6 && shots.len() == expected && suggested == "Snapshot Test.epub",
            format!(
                "{} images for {expected} rich elements; suggested {suggested:?}; alerts {:?}",
                shots.len(),
                self.warnings_since(warned)
            ),
        );
        let displays: Vec<&Snapshot> =
            shots.iter().filter(|s| s.name.ends_with("rich-1.png") || s.name.ends_with("rich-5.png")).collect();
        h.check(
            "epub_snapshots_are_2x_pngs",
            !shots.is_empty()
                && shots.iter().all(Snapshot::is_good)
                && displays.len() == 2
                && displays.iter().all(|s| s.is_centred()),
            checked.join("; "),
        );
        let stored = book.entry_named("images/cover.png");
        h.check(
            "epub_cover_from_beside",
            stored.as_deref() == Some(&cover[..]) && book.text_containing("cover-image"),
            format!("cover {} bytes stored", stored.map_or(0, |c| c.len())),
        );
        h.check("epub_has_no_linux_fonts", book.valid() && !book.text_containing(screen_html::ID_MARKER), "");
        let busy = self.busy.borrow().clone();
        h.check(
            "busy_ring_shows_and_hides",
            busy.first() == Some(&true) && busy.last() == Some(&false),
            format!("{busy:?}"),
        );
        if let Some(png) = shots.iter().find(|s| s.name.ends_with("rich-2.png")) {
            let _ = std::fs::write(self.path("snapshot-mermaid.png"), &png.bytes);
        }
        if let Some(png) = shots.first() {
            let _ = std::fs::write(self.path("snapshot-formula.png"), &png.bytes);
        }
    }

    /// A 40-page document's last formula: the snapshot scrolls to the end of the page.
    async fn long_epub(&self) {
        let h = self.h;
        let mut source = String::from("# A long read\n\n");
        for n in 1..=500 {
            source.push_str(&format!(
                "Paragraph {n}. A typewriter page holds only so much, and this one says the same thing twice \
                 so that forty pages of it pass before the formula at the very end.\n\n"
            ));
        }
        source.push_str("$$\\oint_{\\partial \\Sigma} \\mathbf{B} \\cdot d\\mathbf{l} = \\mu_0 I$$\n");

        let pdf = self.path("long.pdf");
        let _ = self.save_as(&pdf, |e| e.export_pdf(source.clone(), "Long".to_string())).await;
        let pages = pdf_info(h, &pdf).await.map_or(0, |i| i.pages);

        let path = self.path("long.epub");
        let done = self.save_as(&path, |e| e.export_epub(source.clone(), "long.md".to_string(), None)).await;
        let book = Epub::read(&path);
        let shots = book.snapshots();
        let last = shots.last();
        h.check(
            "epub_last_formula_of_a_long_document",
            done && pages >= 40 && shots.len() == 1 && last.is_some_and(|s| s.is_good() && s.is_centred()),
            format!("{pages} A4 pages; {}", last.map_or("no image".to_string(), Snapshot::describe)),
        );
        if let Some(last) = last {
            let _ = std::fs::write(self.path("snapshot-last-formula.png"), &last.bytes);
        }
    }

    // ── SVG, LaTeX, TextBundle ────────────────────────────────────────────────────────────────

    async fn svg(&self) {
        let h = self.h;
        let source = "# Diagrams\n\n```mermaid\nflowchart TD\n  A --> B\n```\n\n\
            ```plantuml\n@startuml\nAlice -> Bob: Hi\n@enduml\n```\n\n\
            ```dot\ndigraph { a -> b -> c; }\n```\n\n```plot\nsin(x)\n```\n";
        let mut rows = Vec::new();
        let mut ok = true;
        for (ordinal, kind) in ["mermaid", "plantuml", "graphviz", "plot"].iter().enumerate() {
            let path = self.path(&format!("diagram-{kind}.svg"));
            let done = self
                .save_as(&path, |e| e.export_diagram_svg(source.to_string(), "Diagrams".to_string(), ordinal as u32))
                .await;
            let svg = std::fs::read_to_string(&path).unwrap_or_default();
            let suggested = self.host.pickers.save_calls().last().map(|c| c.suggested_name.clone()).unwrap_or_default();
            let good = done
                && svg.starts_with("<?xml")
                && svg.contains("<svg")
                && svg.contains("xmlns=\"http://www.w3.org/2000/svg\"")
                && svg.trim_end().ends_with("</svg>")
                && svg.len() > 200
                && suggested == format!("Diagrams-{}.svg", ordinal + 1)
                && !svg.contains(screen_html::ID_MARKER);
            ok &= good;
            rows.push(format!("{kind}: {} bytes as {suggested:?} {}", svg.len(), if good { "ok" } else { "WRONG" }));
        }
        h.check("svg_for_each_diagram_kind", ok, rows.join("; "));

        // A diagram that failed to draw: the alert, never an empty file.
        let warned = self.host.alerts.warnings().len();
        let dialogs = self.host.pickers.save_calls().len();
        self.exports.export_diagram_svg("```dot\ndigraph { a -> }\n```\n".to_string(), "Broken".to_string(), 0);
        let done = self.settle().await;
        let warnings = self.host.alerts.warnings()[warned..].to_vec();
        h.check(
            "svg_of_a_failed_diagram_alerts",
            done && self.host.pickers.save_calls().len() == dialogs
                && warnings.len() == 1
                && warnings[0].title == words::COULD_NOT_EXPORT_SVG
                && warnings[0].message == words::DIAGRAM_NOT_CAPTURED,
            format!("{:?}", self.warnings_since(warned)),
        );
    }

    async fn latex(&self) {
        let path = self.path("document.tex");
        let source = "# LaTeX\n\nSome *emphasis* and $x^2$.\n";
        let done = self.save_as(&path, |e| e.export_latex(source.to_string(), "LaTeX".to_string())).await;
        let tex = std::fs::read_to_string(&path).unwrap_or_default();
        self.h.check(
            "latex",
            done && tex.starts_with("\\documentclass")
                && tex.contains("\\begin{document}")
                && tex.contains("\\end{document}"),
            format!("{} bytes", tex.len()),
        );
    }

    async fn text_bundle(&self) {
        let h = self.h;
        let folder = self.path("bundle-source");
        let _ = std::fs::create_dir_all(&folder);
        let photo = solid_png(8, 8, [0x20, 0x60, 0xa0]);
        let _ = std::fs::write(folder.join("photo.png"), &photo);
        let document = folder.join("note.md");
        let source = "# Bundle\n\n![A photo](photo.png)\n";
        let _ = std::fs::write(&document, source);
        let bundle = self.out.join("Bundle Test.textbundle");
        let _ = std::fs::remove_dir_all(&bundle);

        let pickers = &self.host.pickers;
        let export = || {
            pickers.queue_folder_answer(Some(&self.out.to_string_lossy()));
            self.exports.export_text_bundle(
                source.to_string(),
                Some(document.to_string_lossy().into_owned()),
                "Bundle Test".to_string(),
            );
        };
        export();
        let done = self.settle().await;
        let text = std::fs::read_to_string(bundle.join("text.md")).unwrap_or_default();
        let copied = std::fs::read(bundle.join("assets/photo.png")).unwrap_or_default();
        h.check(
            "text_bundle_with_its_image",
            done && copied == photo && text.contains("assets/photo.png") && bundle.join("info.json").is_file(),
            format!("text.md {text:?}; photo {} bytes", copied.len()),
        );

        let asked = self.host.alerts.replace_confirmations().len();
        self.host.alerts.queue_replace_answer(true);
        export();
        let done = self.settle().await;
        let questions = self.host.alerts.replace_confirmations()[asked..].to_vec();
        h.check(
            "text_bundle_asks_before_replacing",
            done && questions == [words::folder_exists("Bundle Test.textbundle")] && bundle.join("text.md").is_file(),
            format!("{questions:?}"),
        );
    }

    // ── the book sink ─────────────────────────────────────────────────────────────────────────

    /// md-logic's `BookOutput` over this window's `book_outputs()` — what the Book window drives —
    /// on the bundled Example Book: Export Book PDF / EPUB / LaTeX write files, Print Book reaches
    /// the dialog.
    async fn book_outputs(&self) {
        let h = self.h;
        let root = h.shell().paths.examples_dir().join("Example Book").to_string_lossy().into_owned();
        let book_root = root.clone();
        let settings = Rc::new(md_logic::settings::InMemorySettingsStore::new());
        settings.set_string(settings_keys::PDF_PAGE_SIZE, PageSize::SIX_BY_NINE.id);
        let output = md_logic::books::BookOutput::new(
            self.exports.book_outputs(),
            settings,
            Rc::new(md_core::book::book_article_session::BookFlushHub::new()),
            move || md_core::book::book_model::load(&book_root),
        );
        let alerts = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&alerts);
        output.alert_requested().connect(move |(title, message)| sink.borrow_mut().push(format!("{title}: {message}")));
        let structured = md_core::book::book_model::load(&root).and_then(|book| {
            md_core::book::book_compiler::read_structured_book(&book, md_core::book::book_folder::read_article).ok()
        });
        let expected: Option<usize> = structured
            .as_ref()
            .map(|book| epub_export::plan_book(book).rich_units().iter().map(|u| u.rich_elements.len()).sum());

        let pdf = self.path("example-book.pdf");
        self.host.pickers.queue_save_answer(Some(&pdf.to_string_lossy()));
        let done = h.with_timeout(EXPORT_LIMIT, output.export_pdf()).await.is_some();
        let info = pdf_info(h, &pdf).await;
        h.check(
            "book_export_pdf",
            done && info
                .as_ref()
                .is_some_and(|i| i.pages >= 3 && (i.width - 432.0).abs() <= 1.0 && (i.height - 648.0).abs() <= 1.0),
            format!("{info:?}; alerts {:?}", alerts.borrow()),
        );

        let epub = self.path("example-book.epub");
        self.host.pickers.queue_save_answer(Some(&epub.to_string_lossy()));
        let done = h.with_timeout(EXPORT_LIMIT, output.export_epub()).await.is_some();
        let book = Epub::read(&epub);
        let shots = book.snapshots();
        h.check(
            "book_export_epub",
            done && book.valid()
                && Some(shots.len()) == expected
                && shots.iter().all(Snapshot::is_good)
                && !book.text_containing(screen_html::ID_MARKER),
            format!(
                "{} entries, {} images for {expected:?} rich elements; alerts {:?}",
                book.entries.len(),
                shots.len(),
                alerts.borrow()
            ),
        );

        let tex = self.path("example-book.tex");
        self.host.pickers.queue_save_answer(Some(&tex.to_string_lossy()));
        let done = h.with_timeout(EXPORT_LIMIT, output.export_latex()).await.is_some();
        let text = std::fs::read_to_string(&tex).unwrap_or_default();
        h.check(
            "book_export_latex",
            done && text.starts_with("\\documentclass") && text.contains("\\chapter"),
            format!("{} bytes; alerts {:?}", text.len(), alerts.borrow()),
        );

        let known = Self::toplevels();
        let printing = output.print();
        let printed = Rc::new(Cell::new(false));
        let flag = Rc::clone(&printed);
        crate::services::glib_seams::GlibSpawner.spawn_local(Box::pin(async move {
            printing.await;
            flag.set(true);
        }));
        let dismissed = self.dismiss_print_dialog(known, "print-book-dialog").await;
        let ended = h.wait_until(Duration::from_secs(30), || printed.get()).await;
        let outcomes = self.exports.print_outcomes();
        h.check(
            "book_print_reaches_the_dialog",
            dismissed.is_some()
                && ended
                && outcomes.last() == Some(&PrintOutcome::Cancelled)
                && webviews(&self.host.overlay) == 0,
            format!("{dismissed:?}; outcomes {outcomes:?}; alerts {:?}", alerts.borrow()),
        );
    }

    // ── the cover fetch ───────────────────────────────────────────────────────────────────────

    async fn cover_fetch(&self) {
        let h = self.h;
        let image = solid_png(32, 48, [0x10, 0x80, 0x40]);
        let requests: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let server = soup::Server::builder().build();
        {
            let (image, requests) = (image.clone(), Rc::clone(&requests));
            server.add_handler(None, move |_, message, path, _| {
                requests.borrow_mut().push(path.to_string());
                match path {
                    "/redirect" => {
                        message.set_status(302, None);
                        if let Some(headers) = message.response_headers() {
                            headers.append("Location", "/cover.png");
                        }
                    }
                    "/cover.png" => {
                        message.set_status(200, None);
                        message.set_response(Some("image/png"), soup::MemoryUse::Copy, &image);
                    }
                    _ => message.set_status(404, None),
                }
            });
        }
        let port = match server.listen_local(0, soup::ServerListenOptions::IPV4_ONLY) {
            Ok(()) => server.uris().first().map(|u| u.port()).unwrap_or(0),
            Err(err) => {
                h.check("cover_fetch_one_get", false, format!("no local server: {err}"));
                return;
            }
        };
        let base = format!("http://127.0.0.1:{port}");
        let fetcher = SoupCoverFetcher::new();

        let redirect = fetcher.get(format!("{base}/redirect")).await;
        let redirect_row = match &redirect {
            Ok(r) => format!("{} → {:?}", r.status, r.location),
            Err(e) => e.clone(),
        };
        let not_followed = requests.borrow().clone();
        let got = fetcher.get(format!("{base}/cover.png")).await;
        let (status, length, body) = match got {
            Ok(response) => {
                let body =
                    cover_reader::read_bounded(&*response.body, 1 << 20).await.ok().flatten().unwrap_or_default();
                (response.status, response.content_length, body)
            }
            Err(e) => (0, None, e.into_bytes()),
        };
        let missing = fetcher.get(format!("{base}/nope")).await.map(|r| r.status).unwrap_or(0);
        h.check(
            "cover_fetch_one_get",
            redirect.as_ref().is_ok_and(|r| r.status == 302 && r.location.as_deref() == Some("/cover.png"))
                && not_followed == ["/redirect"]
                && status == 200
                && length == Some(image.len() as u64)
                && body == image
                && missing == 404,
            format!("redirect {redirect_row} (requests then: {not_followed:?}); cover {status} {length:?} {} bytes; missing {missing}", body.len()),
        );

        // md-logic's reader refuses a plain-http cover before a request is made.
        let before = requests.borrow().len();
        let scheduler = crate::services::glib_seams::GlibScheduler;
        let refused = cover_reader::fetch(
            &fetcher,
            &scheduler,
            &format!("{base}/cover.png"),
            &md_logic::exec::CancelToken::new(),
        )
        .await;
        h.check(
            "cover_fetch_https_only",
            matches!(refused, Ok(None)) && requests.borrow().len() == before,
            format!("{refused:?}"),
        );
        soup::prelude::ServerExt::disconnect(&server);
    }

    // ── Print ─────────────────────────────────────────────────────────────────────────────────

    /// Waits for GTK's print dialog (a new visible toplevel whose type names it), screenshots it as
    /// `shot` and cancels it — Escape through xdotool when input is on, else `close()`. Returns the
    /// dialog's type name and how it went, or `None` when no dialog appeared.
    async fn dismiss_print_dialog(&self, known: Vec<gtk::Window>, shot: &str) -> Option<(String, &'static str)> {
        let h = self.h;
        let dialog = Rc::new(RefCell::new(None::<gtk::Window>));
        let found = {
            let dialog = Rc::clone(&dialog);
            h.wait_until(Duration::from_secs(60), move || {
                let fresh = gtk::Window::list_toplevels()
                    .into_iter()
                    .filter_map(|w| w.downcast::<gtk::Window>().ok())
                    .find(|w| !known.contains(w) && w.is_visible() && w.type_().name().contains("Print"));
                let found = fresh.is_some();
                *dialog.borrow_mut() = fresh;
                found
            })
            .await
        };
        let window = dialog.borrow().clone();
        let window = window.filter(|_| found)?;
        let kind = window.type_().name().to_string();
        h.screenshot(shot).await;
        let mut how = "Escape";
        if !(h.focus(&window).await && h.key("Escape").await) {
            how = "close()";
            window.close();
        }
        if !h.wait_until(Duration::from_secs(5), || !window.is_visible()).await {
            how = "close() after Escape";
            window.close();
        }
        Some((kind, how))
    }

    fn toplevels() -> Vec<gtk::Window> {
        gtk::Window::list_toplevels().into_iter().filter_map(|w| w.downcast().ok()).collect()
    }

    async fn print(&self, host: &Rc<dyn WindowHost>) {
        let h = self.h;
        let shell = h.shell();
        // As a window builds it.
        let exports = WindowExports::new(&shell, Rc::downgrade(host));
        let known = Self::toplevels();
        exports.print("# Print me\n\nOn paper, in Nimbus Mono PS.\n".to_string(), "Print me".to_string());
        let dismissed = self.dismiss_print_dialog(known, "print-dialog").await;
        let ended = self.settle().await;
        h.sleep(Duration::from_millis(300)).await;
        let outcomes = exports.print_outcomes();
        h.check(
            "print_reaches_the_dialog",
            dismissed.is_some() && ended && outcomes == [PrintOutcome::Cancelled] && webviews(&self.host.overlay) == 0,
            format!("{dismissed:?}; outcomes {outcomes:?}; renderers left {}", webviews(&self.host.overlay)),
        );
        exports.cancel();
    }

    fn leftovers(&self) {
        let warnings = self.host.alerts.warnings();
        let unexpected: Vec<String> = warnings
            .iter()
            .filter(|w| w.message != words::DIAGRAM_NOT_CAPTURED)
            .map(|w| format!("{}: {}", w.title, w.message))
            .collect();
        self.h.check(
            "renderers_closed_and_no_unexpected_alerts",
            webviews(&self.host.overlay) == 0 && unexpected.is_empty(),
            format!("renderers left {}; alerts {unexpected:?}", webviews(&self.host.overlay)),
        );
    }
}

/// `pdfinfo`'s page count and first page size.
#[derive(Debug)]
struct PdfInfo {
    pages: u32,
    width: f64,
    height: f64,
}

async fn pdf_info(h: &Harness, path: &Path) -> Option<PdfInfo> {
    let (ok, out) = h.run(&["pdfinfo", &path.to_string_lossy()]).await;
    if !ok {
        return None;
    }
    let field = |name: &str| {
        out.lines().find_map(|l| l.strip_prefix(name)).map(|v| v.trim_start_matches(':').trim().to_string())
    };
    let size = field("Page size")?;
    let mut numbers = size.split_whitespace().filter_map(|w| w.parse::<f64>().ok());
    Some(PdfInfo { pages: field("Pages")?.parse().ok()?, width: numbers.next()?, height: numbers.next()? })
}

/// The overlay's web views (export renderers left behind).
fn webviews(overlay: &gtk::Overlay) -> usize {
    let mut count = 0;
    let mut child = overlay.first_child();
    while let Some(widget) = child {
        if widget.is::<webkit::WebView>() {
            count += 1;
        }
        child = widget.next_sibling();
    }
    count
}

/// A `width` × `height` PNG of one colour.
fn solid_png(width: i32, height: i32, rgb: [u8; 3]) -> Vec<u8> {
    let pixels: Vec<u8> = (0..width * height).flat_map(|_| [rgb[0], rgb[1], rgb[2], 0xff]).collect();
    let bytes = glib::Bytes::from_owned(pixels);
    gdk::MemoryTexture::new(width, height, gdk::MemoryFormat::R8g8b8a8, &bytes, width as usize * 4)
        .save_to_png_bytes()
        .to_vec()
}

fn json_number(json: &md_logic::preview::json_script::Json) -> f64 {
    match json {
        md_logic::preview::json_script::Json::Number(n) => *n,
        _ => f64::NAN,
    }
}

/// An exported EPUB, unzipped with md-core's reader.
struct Epub {
    entries: Vec<(String, Vec<u8>)>,
}

/// One rich-element photograph and the width its `<img>` declares.
struct Snapshot {
    name: String,
    bytes: Vec<u8>,
    declared_width: Option<i32>,
    size: Option<(i32, i32)>,
    inked: usize,
    /// Inked pixels in the leftmost tenth: none for a centred display formula, many for prose.
    left_ink: usize,
}

impl Snapshot {
    /// A PNG that decodes, 2 device px per declared CSS px (±1), with ink on it.
    fn is_good(&self) -> bool {
        match (self.size, self.declared_width) {
            (Some((w, h)), Some(css)) => (w - 2 * css).abs() <= 1 && h > 0 && self.inked > 0,
            _ => false,
        }
    }

    /// A centred display formula: nothing in the leftmost tenth (prose would start there).
    fn is_centred(&self) -> bool {
        self.inked > 0 && self.left_ink == 0
    }

    fn describe(&self) -> String {
        format!(
            "{} {:?} for width={:?}, {} inked px ({} in the left tenth)",
            self.name, self.size, self.declared_width, self.inked, self.left_ink
        )
    }
}

impl Epub {
    fn read(path: &Path) -> Epub {
        let bytes = std::fs::read(path).unwrap_or_default();
        let entries =
            zip_reader::entries(&bytes, None).unwrap_or_default().into_iter().map(|e| (e.name, e.data)).collect();
        Epub { entries }
    }

    /// The first entry is the stored `mimetype`.
    fn valid(&self) -> bool {
        self.entries.first().is_some_and(|(name, data)| name == "mimetype" && data == epub_export::MIMETYPE.as_bytes())
    }

    fn entry_named(&self, suffix: &str) -> Option<Vec<u8>> {
        self.entries.iter().find(|(name, _)| name.ends_with(suffix)).map(|(_, data)| data.clone())
    }

    fn text_containing(&self, needle: &str) -> bool {
        self.entries.iter().any(|(_, data)| String::from_utf8_lossy(data).contains(needle))
    }

    /// The rich-element images in name order, each with its `<img width>`.
    fn snapshots(&self) -> Vec<Snapshot> {
        let markup: String = self
            .entries
            .iter()
            .filter(|(name, _)| name.ends_with(".xhtml"))
            .map(|(_, data)| String::from_utf8_lossy(data).into_owned())
            .collect();
        let mut shots: Vec<Snapshot> = self
            .entries
            .iter()
            .filter(|(name, _)| name.contains("images/unit-") && name.ends_with(".png"))
            .map(|(name, data)| {
                let file = &name[name.find("images/").unwrap_or(0)..];
                let declared_width = markup
                    .find(&format!("src=\"{file}\""))
                    .and_then(|at| markup[at..].find("width=\"").map(|w| at + w + 7))
                    .and_then(|start| markup[start..].split('"').next()?.parse().ok());
                let (size, inked, left_ink) = decode(data);
                Snapshot { name: name.clone(), bytes: data.clone(), declared_width, size, inked, left_ink }
            })
            .collect();
        shots.sort_by(|a, b| a.name.cmp(&b.name));
        shots
    }
}

/// A PNG's size, how many of its pixels are not (near) white or transparent, and how many of those
/// lie in its leftmost tenth.
fn decode(png: &[u8]) -> (Option<(i32, i32)>, usize, usize) {
    let Ok(texture) = gdk::Texture::from_bytes(&glib::Bytes::from(png)) else { return (None, 0, 0) };
    let mut downloader = gdk::TextureDownloader::new(&texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = downloader.download_bytes();
    let (w, h) = (texture.width() as usize, texture.height() as usize);
    let (mut inked, mut left) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let p = &bytes[y * stride + x * 4..y * stride + x * 4 + 4];
            if p[3] > 128 && (p[0] < 200 || p[1] < 200 || p[2] < 200) {
                inked += 1;
                if x < w / 10 {
                    left += 1;
                }
            }
        }
    }
    (Some((texture.width(), texture.height())), inked, left)
}
