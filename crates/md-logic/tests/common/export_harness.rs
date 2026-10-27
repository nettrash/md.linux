//! Port of md.win `tests/Md.App.Logic.Tests/Export/ExportHarness.cs` (and the reason
//! `NoSyncContext.cs` needs no port).
//!
//! One [`ExportPipeline`] wired to fakes, plus the two things every export test needs: a way to
//! script what the page answers, and a way to run the whole flow to completion without a clock.
//! Nothing here sleeps — [`ExportHarness::drain`] pumps the executor and advances the fake
//! scheduler until the export is done, which is why an export that forgot to await something fails
//! loudly instead of hanging.
//!
//! md.win's `NoSyncContext` removes xUnit's synchronization context so a completion's continuation
//! runs inline when a fake timer fires. Rust futures have no ambient context: a [`LocalExecutor`]
//! polls exactly when the test pumps it, and a future polled by hand ([`super::task::Task`]) moves
//! exactly when the test polls it — deterministic by construction.
//!
//! LINUX: md.win's `C:\Temp` and `C:\Docs` become Linux paths; there is no temporary folder (Share
//! is not ported) and the pipeline takes a [`FakeCoverFetcher`] where md.win's harness replaced the
//! fetch function (`Fetch` / `Fetched` are the fetcher's handler and requests).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use md_core::export::BundleWrapper;
use md_logic::export::ExportPipeline;
use md_logic::testing::*;

pub const DOCS: &str = "/home/writer/Docs";
pub const DESTINATION: &str = "/home/writer/Docs/out";

pub struct ExportHarness {
    pub renderers: Rc<FakeRenderSurfaceFactory>,
    pub pickers: Rc<FakePickers>,
    pub alerts: Rc<FakeAlerts>,
    pub files: Rc<FakeFileSystem>,
    pub scheduler: Rc<FakeScheduler>,
    pub identity: Rc<FakeFileIdentity>,
    pub fetcher: Rc<FakeCoverFetcher>,
    pub executor: LocalExecutor,
    pub pipeline: Rc<ExportPipeline>,
    eval: Rc<RefCell<Option<Rc<dyn Fn(&str) -> String>>>>,
    png: Rc<RefCell<Vec<u8>>>,
    pdf: Rc<RefCell<Vec<u8>>>,
    load_failure: Rc<RefCell<Option<RenderError>>>,
    bundles: Rc<RefCell<Vec<(BundleWrapper, String)>>>,
    assets: Rc<RefCell<HashMap<String, Vec<u8>>>>,
    asset_keys: Rc<RefCell<Vec<String>>>,
}

impl ExportHarness {
    pub fn new() -> Self {
        let files = Rc::new(FakeFileSystem::new());
        files.add_directory(DOCS);

        let eval: Rc<RefCell<Option<Rc<dyn Fn(&str) -> String>>>> = Rc::new(RefCell::new(None));
        let png = Rc::new(RefCell::new(vec![0x89, 0x50, 0x4E, 0x47]));
        let pdf = Rc::new(RefCell::new(b"%PDF-1.7\n".to_vec()));
        let load_failure: Rc<RefCell<Option<RenderError>>> = Rc::new(RefCell::new(None));

        let renderers = Rc::new(FakeRenderSurfaceFactory::new());
        {
            let (eval, png, pdf, load_failure) = (eval.clone(), png.clone(), pdf.clone(), load_failure.clone());
            renderers.set_create(move |kind| {
                let surface = FakeRenderSurface::new(kind);
                let eval = eval.clone();
                // Read per call, as md.win's `Eval?.Invoke(script) ?? "null"` is.
                surface.set_eval_handler(move |script| {
                    let handler = eval.borrow().clone();
                    handler.map_or_else(|| "null".to_string(), |handler| handler(script))
                });
                surface.set_png_bytes(&png.borrow());
                surface.set_pdf_bytes(&pdf.borrow());
                surface.set_load_failure(load_failure.borrow().clone());
                Rc::new(surface)
            });
        }

        let pickers = Rc::new(FakePickers::new());
        let alerts = Rc::new(FakeAlerts::new());
        let scheduler = Rc::new(FakeScheduler::new());
        let identity = Rc::new(FakeFileIdentity::new());
        let fetcher = Rc::new(FakeCoverFetcher::new());
        let bundles: Rc<RefCell<Vec<(BundleWrapper, String)>>> = Rc::new(RefCell::new(Vec::new()));
        let assets: Rc<RefCell<HashMap<String, Vec<u8>>>> = Rc::new(RefCell::new(HashMap::new()));
        let asset_keys: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

        let read_asset = {
            let (assets, asset_keys) = (assets.clone(), asset_keys.clone());
            move |key: &str| {
                asset_keys.borrow_mut().push(key.to_string());
                assets.borrow().get(key).cloned()
            }
        };
        let pipeline = ExportPipeline::new(
            renderers.clone(),
            pickers.clone(),
            alerts.clone(),
            files.clone(),
            read_asset,
            scheduler.clone(),
            identity.clone(),
            fetcher.clone(),
        )
        .with_bundle_writer({
            let bundles = bundles.clone();
            move |wrapper: &BundleWrapper, folder: &str| {
                bundles.borrow_mut().push((wrapper.clone(), folder.to_string()));
                Ok(())
            }
        });

        ExportHarness {
            renderers,
            pickers,
            alerts,
            files,
            scheduler,
            identity,
            fetcher,
            executor: LocalExecutor::new(),
            pipeline: Rc::new(pipeline),
            eval,
            png,
            pdf,
            load_failure,
            bundles,
            assets,
            asset_keys,
        }
    }

    /// What each evaluated script answers, as raw JSON. Unscripted is the four characters `null`.
    pub fn set_eval(&self, answer: impl Fn(&str) -> String + 'static) {
        *self.eval.borrow_mut() = Some(Rc::new(answer));
    }

    /// What every capture of a surface created from now on returns.
    pub fn set_png(&self, bytes: &[u8]) {
        *self.png.borrow_mut() = bytes.to_vec();
    }

    pub fn pdf(&self) -> Vec<u8> {
        self.pdf.borrow().clone()
    }

    /// What every PDF of a surface created from now on returns.
    pub fn set_pdf(&self, bytes: &[u8]) {
        *self.pdf.borrow_mut() = bytes.to_vec();
    }

    /// Every load of a surface created from now on fails with this.
    pub fn set_load_failure(&self, failure: Option<RenderError>) {
        *self.load_failure.borrow_mut() = failure;
    }

    /// What an EPUB cover fetch answers for every URL: the bytes, or (`None`) a failed request.
    pub fn set_fetch(&self, fetch: impl Fn(&str) -> Option<Vec<u8>> + 'static) {
        self.fetcher.set_handler(move |url| match fetch(url) {
            Some(bytes) => Ok(FakeCoverAnswer::ok(&bytes)),
            None => Err("offline".to_string()),
        });
    }

    /// Every https cover URL requested, in order.
    pub fn fetched(&self) -> Vec<String> {
        self.fetcher.requests()
    }

    /// The TextBundle folders written, instead of a directory tree on a real disk.
    pub fn bundles(&self) -> Vec<(BundleWrapper, String)> {
        self.bundles.borrow().clone()
    }

    /// A bundled engine asset `read_rich_asset` should find.
    pub fn add_asset(&self, key: &str, bytes: &[u8]) {
        self.assets.borrow_mut().insert(key.to_string(), bytes.to_vec());
    }

    /// Every key `read_rich_asset` was asked for, in order — the math gate leaves its fingerprints
    /// here.
    pub fn asset_keys(&self) -> Vec<String> {
        self.asset_keys.borrow().clone()
    }

    /// The surfaces the flow asked for, in order of creation.
    pub fn created(&self) -> Vec<Rc<FakeRenderSurface>> {
        self.renderers.created()
    }

    /// Every string any surface was asked to load, in order.
    pub fn loaded(&self) -> Vec<String> {
        self.renderers.created().iter().flat_map(|surface| surface.loaded_html()).collect()
    }

    /// The save dialog will hand back [`DESTINATION`] with this extension.
    pub fn will_save(&self, extension: &str) -> String {
        let path = format!("{DESTINATION}{extension}");
        self.pickers.queue_save_answer(Some(&path));
        path
    }

    /// Starts `future` on the harness's executor and pumps it once; the flag goes true when it
    /// finishes.
    pub fn spawn(&self, future: LocalBoxFuture<'static, ()>) -> Rc<Cell<bool>> {
        let done = Rc::new(Cell::new(false));
        let flag = done.clone();
        self.executor.spawn_local(Box::pin(async move {
            future.await;
            flag.set(true);
        }));
        self.executor.run_until_stalled();
        done
    }

    /// Run an export to completion: pump, then advance the clock a second at a time, up to 200
    /// steps. The pipeline turns every failure into an alert, so a panic here is a bug.
    pub fn drain(&self, future: LocalBoxFuture<'static, ()>) {
        let done = self.spawn(future);
        self.drain_flag(&done);
    }

    /// [`ExportHarness::drain`] for a future already spawned with [`ExportHarness::spawn`].
    pub fn drain_flag(&self, done: &Rc<Cell<bool>>) {
        for _ in 0..200 {
            self.executor.run_until_stalled();
            if done.get() {
                return;
            }
            self.scheduler.advance(Duration::from_secs(1));
        }
        self.executor.run_until_stalled();
        assert!(done.get(), "the export never finished; something awaits a timer nobody fires");
    }

    pub fn written_text(&self, path: &str) -> Option<String> {
        self.files.text(path)
    }

    pub fn written_bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.files.bytes(path)
    }

    // ---- scripted answers ----

    /// A JSON string result, as the evaluation encodes one.
    pub fn json(value: Option<&str>) -> String {
        serde_json::to_string(&value).expect("a string serialises")
    }

    /// The five-column answer `scripts::rich_elements` gives, one row per element.
    pub fn rects(count: usize, width: f64, height: f64) -> String {
        let rows: Vec<[f64; 5]> =
            (0..count).map(|i| [10.0, 20.0 + (i as f64 * 60.0), width, height, (i % 2) as f64]).collect();
        serde_json::to_string(&rows).expect("numbers serialise")
    }

    /// [`ExportHarness::rects`] at md.win's default 120 × 40.
    pub fn rects_default(count: usize) -> String {
        Self::rects(count, 120.0, 40.0)
    }
}
