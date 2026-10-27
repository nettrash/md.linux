//! The application object and the process's one [`Shell`] (shell-design §1.1, §1.7).
//!
//! `run` builds the `adw::Application` (`me.nettrash.md`, `HANDLES_OPEN`, single instance over
//! D-Bus; `NON_UNIQUE` for a self-test run), registers md's own options and connects the
//! application's signals to `windows::window_manager`. `startup` creates the [`Shell`] — every
//! process-wide service — and keeps it in a thread-local that [`Shell::get`] reads; everything runs
//! on the GTK main thread, so `Rc` everywhere.

use std::cell::RefCell;
use std::ffi::OsString;
use std::rc::Rc;

use adw::prelude::*;
use md_logic::exec::LocalBoxFuture;
use md_logic::seams::{OffThread, OffThreadValue, OffThreadWork, WordCounter};
use md_logic::windows::WindowRegistry;

use crate::book::library_host::BookLibraryHost;
use crate::services::examples::ExampleLibrary;
use crate::services::file_system::{CanonicalFileIdentity, StdFileSystem};
use crate::services::glib_seams::{GlibScheduler, GlibSpawner, GlibUiThread};
use crate::services::paths::Paths;
use crate::services::recent::RecentFiles;
use crate::services::settings_store::KeyFileSettingsStore;
use crate::theme;
use crate::web::context::WebServices;
use crate::windows::window_manager::{self, WindowManager};

/// The reverse-DNS id the family shares: the D-Bus name that makes md single-instance, the Wayland
/// `app_id` / X11 `WM_CLASS` that ties windows to `me.nettrash.md.desktop`, and the icon name.
pub const APP_ID: &str = "me.nettrash.md";

/// `md --help`'s line for `--new-window` (shell-design §1.1).
const NEW_WINDOW_HELP: &str = "Open a new document window";
/// `md --help`'s line for `--version`.
const VERSION_HELP: &str = "Print the version and exit";

/// How this process was started.
pub enum Launch {
    /// A desktop launch, a file manager's open, or a second launch handing over to the primary.
    Normal,
    /// `md --selftest OUT …` (cargo feature `selftest`): a private, `NON_UNIQUE` process whose
    /// runner drives the real windows and exits with the report's verdict.
    #[cfg(feature = "selftest")]
    SelfTest(crate::selftest::Options),
}

/// Every process-wide service (shell-design §1.7), created in `startup`. One per process, reached
/// from anywhere on the main thread through [`Shell::get`].
pub struct Shell {
    pub app: adw::Application,
    pub paths: Paths,
    pub settings: Rc<KeyFileSettingsStore>,
    /// Also the [`Clock`](md_logic::seams::Clock).
    pub scheduler: Rc<GlibScheduler>,
    pub ui: Rc<GlibUiThread>,
    pub spawner: Rc<GlibSpawner>,
    pub files: Rc<StdFileSystem>,
    pub identity: Rc<CanonicalFileIdentity>,
    pub registry: Rc<WindowRegistry>,
    pub recent: Rc<RecentFiles>,
    pub examples: Rc<ExampleLibrary>,
    pub web: Rc<WebServices>,
    /// md-logic's `WritingStatsWordCounter` (`text::word_counter::create`).
    pub words: std::sync::Arc<dyn WordCounter>,
    /// Work off the UI thread (the derived text, §5.5): GIO's blocking pool, shared by every
    /// window.
    pub off_thread: Rc<GioOffThread>,
    pub windows: Rc<WindowManager>,
    pub book: Rc<BookLibraryHost>,
}

/// md-logic's [`OffThread`] over `gio::spawn_blocking` (shell-design §5.5): the work runs on GIO's
/// blocking thread pool and the result comes back on the main context; a panic in the work is
/// `None`.
#[derive(Clone, Copy, Debug, Default)]
pub struct GioOffThread;

impl OffThread for GioOffThread {
    fn spawn_blocking(&self, work: OffThreadWork) -> LocalBoxFuture<'static, Option<OffThreadValue>> {
        let handle = gio::spawn_blocking(work);
        Box::pin(async move { handle.await.ok() })
    }
}

thread_local! {
    static SHELL: RefCell<Option<Rc<Shell>>> = const { RefCell::new(None) };
}

impl Shell {
    /// The process's shell. Panics before `startup` (a bug: nothing runs before it).
    pub fn get() -> Rc<Shell> {
        SHELL.with(|shell| shell.borrow().clone()).expect("Shell::get() before the application's startup")
    }

    /// `startup`: services, settings, the theme. Menus, actions and accelerators follow in
    /// `window_manager::startup`.
    fn start(app: &adw::Application) -> Rc<Shell> {
        let paths = Paths::new();
        let settings = Rc::new(KeyFileSettingsStore::open(paths.settings_file()));
        theme::install();
        let shell = Rc::new(Shell {
            app: app.clone(),
            settings: Rc::clone(&settings),
            scheduler: Rc::new(GlibScheduler),
            ui: Rc::new(GlibUiThread),
            spawner: Rc::new(GlibSpawner),
            files: Rc::new(StdFileSystem::new()),
            identity: Rc::new(CanonicalFileIdentity::new()),
            registry: Rc::new(WindowRegistry::new()),
            recent: RecentFiles::new(settings.clone()),
            examples: ExampleLibrary::new(&paths),
            web: WebServices::new(&paths),
            words: md_logic::text::word_counter::create(),
            off_thread: Rc::new(GioOffThread),
            windows: WindowManager::new(&paths),
            book: BookLibraryHost::new(settings),
            paths,
        });
        SHELL.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&shell)));
        shell
    }

    /// `shutdown`: the windows are gone; drop the services before GTK goes.
    fn stop() {
        let shell = SHELL.with(|slot| slot.borrow_mut().take());
        drop(shell);
    }
}

/// Builds the application, connects its signals and runs the main loop. `args` are the process
/// arguments with md's own self-test flags already taken out; GApplication parses the rest
/// (`--new-window`, `--version`, GTK's own) and hands the remaining paths to `open`.
pub fn run(args: &[OsString], launch: Launch) -> glib::ExitCode {
    #[cfg(feature = "selftest")]
    let selftest = match launch {
        Launch::Normal => None,
        Launch::SelfTest(options) => Some(options),
    };
    #[cfg(not(feature = "selftest"))]
    let Launch::Normal = launch;

    glib::set_application_name(md_logic::strings::APP_NAME);

    let flags = gio::ApplicationFlags::HANDLES_OPEN;
    // Never the primary instance, never handing its arguments to a running md.
    #[cfg(feature = "selftest")]
    let flags = if selftest.is_some() { flags | gio::ApplicationFlags::NON_UNIQUE } else { flags };

    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(flags)
        // `query-end` at logout (shell-design §1.5).
        .register_session(true)
        .build();
    app.add_main_option(
        "new-window",
        glib::Char::from(b'n'),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        NEW_WINDOW_HELP,
        None,
    );
    app.add_main_option(
        "version",
        glib::Char::from(0),
        glib::OptionFlags::NONE,
        glib::OptionArg::None,
        VERSION_HELP,
        None,
    );

    app.connect_handle_local_options(window_manager::handle_local_options);

    #[cfg(feature = "selftest")]
    let is_selftest = selftest.is_some();
    #[cfg(feature = "selftest")]
    let selftest = RefCell::new(selftest);
    app.connect_startup(move |app| {
        let shell = Shell::start(app);
        #[cfg(feature = "selftest")]
        if let Some(options) = selftest.borrow_mut().take() {
            crate::selftest::start(&shell, options);
        }
        window_manager::startup(&shell);
    });
    app.connect_activate(|_| window_manager::activate(&Shell::get()));
    app.connect_open(|_, files, hint| window_manager::open(&Shell::get(), files, hint));
    app.connect_query_end(|_| window_manager::query_end(&Shell::get()));
    app.connect_shutdown(|_| Shell::stop());

    let code = app.run_with_args_os(args);

    #[cfg(feature = "selftest")]
    if is_selftest {
        return crate::selftest::exit_code();
    }
    code
}
