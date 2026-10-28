# md for Linux — shell work packages

How the GTK shell of `docs/shell-design.md` is built by **six agents in parallel** after one
skeleton package. Every package owns its files exclusively; packages meet only at the interfaces the
skeleton freezes (§3). md.win's `docs/shell-design.md` §13 and `docs/app-api.md` are the model: the
same split, adapted to a Rust binary crate and to md-logic being ported at the same time.

---

## 1. Rules for every package

- **Ownership.** A package edits only the files §2 gives it. Needing a change in another package's
  file — a signature, a field, a helper — is a message to that owner, not an edit. The skeleton's
  frozen interfaces (§3) change only by agreement of the owner and every caller, recorded in the
  owner's commit message.
- **Where logic goes.** Anything testable without a display belongs in md-logic (owned by the
  md-logic porters, not by these packages) or, when it is GTK-shaped glue, in a **pure module of the
  app crate** with `#[cfg(test)]` unit tests (`editor/offsets.rs`, `menus/accels.rs`,
  `web/retry_policy.rs`, `services/watch_events.rs`, `windows/drafts.rs` …). `cargo test --workspace`
  in Docker runs them. md-logic gaps go to the md-logic porters as requests (shell-design §15) — never
  patched from a shell package.
- **Toolchain.** Ubuntu 26.04's rustc 1.93, edition 2024, `#![forbid(unsafe_code)]` (workspace lint),
  **zero warnings**: `scripts/dev.sh test` runs fmt, clippy `-D warnings` on the workspace and on `md`
  with and without `--features selftest`, and `cargo test --workspace`. No new crate dependency
  without asking nettrash (libsoup, JavaScriptCore and every GTK type come with the four crates
  already in `app/Cargo.toml`).
- **Proof.** Every package adds its checks to its own self-test group file (shell-design §13) and
  ships with `scripts/dev.sh selftest --only <group>` green in the `ubuntu:26.04` image, screenshots
  in `dist/selftest/` looked at, and its real-desktop checklist (§5) written into the package's final
  report as "not verified here".
- **Strings.** None of your own: titles from md-logic's `CommandTable`, everything else from
  `md_logic::strings` (shell-design §11). A missing string is a request to md-logic.
- **Git.** Each package on its own branch from the skeleton's merge (`w4-s0-skeleton`, `w4-p1-shell`,
  … `w4-p6-packaging`), author nettrash per the repo config, the repo's message style, no trailers,
  no mention of any AI or tool; never push, merge or tag.

---

## 2. The file tree and who owns it

```
app/
  Cargo.toml                         S0   (feature `selftest`; no new dependencies)
  src/
    main.rs                          S0   mod tree, XDG redirection for --selftest, run()
    app.rs                           S0 → P1   Shell, startup/activate/open/query-end, quit
    services/
      mod.rs                         S0
      glib_seams.rs                  S0   GlibScheduler (Scheduler+Clock), GlibUiThread, GlibSpawner, SystemClock
      settings_store.rs              S0   KeyFileSettingsStore
      paths.rs                       S0   Paths (data, rich, examples, XDG folders)
      file_system.rs                 P2   StdFileSystem, CanonicalFileIdentity
      file_watcher.rs                P2   GioFileWatcher
      watch_events.rs                P2   pure: monitor event → changed/renamed/deleted (+ tests)
      pickers.rs                     P2   GtkPickers
      alerts.rs                      P2   AdwAlerts
      recent.rs                      P2   RecentFiles (+ pure filter tests)
      examples.rs                    P2   ExampleLibrary
      document_ops.rs                P2   rename / move via gio::File::move_, TextBundle/TextPack import helpers
    theme/
      mod.rs                         S0   CssProvider, palette CSS, dark notify fan-out
    menus/
      mod.rs                         S0 → P1
      menu_bar.rs                    P1   MenuBar: CommandTable → gio::Menu, refresh(&ShellSnapshot)
      actions.rs                     P1   one SimpleAction per CommandId, scopes, enablement / state
      accels.rs                      P1   pure: Chord → GTK accelerator strings (+ tests)
      about.rs                       P1   adw::AboutDialog
    windows/
      mod.rs                         S0 → P1
      host.rs                        S0 (frozen) WindowHost trait, WindowKind
      window_manager.rs              P1   live windows, routing, open/present/quit, Window list
      document_window.rs             P1   the document window
      session.rs                     P1   session.json via md-logic SessionStore
      drafts.rs                      P1   untitled drafts (+ tests with a temp folder)
    editor/
      mod.rs                         S0 → P3
      editor_pane.rs                 P3   EditorPane (TextView, placeholder, CSS class, jumps)
      typing.rs                      P3   TypingHooks / FormattingAdapter glue (user actions, keys, signals)
      offsets.rs                     P3   pure: char ↔ UTF-16 (+ tests)
      find_bar.rs                    P3   FindBar
      article_panes.rs               P3   ArticlePanes (homogeneous box, BreakpointBin, divider)
      zen.rs                         P3   ZenBin (2/3 × 92 %), ZenCapsule
      footer.rs                      P3   Footer (word count, spinner)
      document_bar.rs                P3   conflict / save-error bar widget
    web/
      mod.rs                         S0 → P4
      context.rs                     P4   WebServices: WebContext, ephemeral NetworkSession, Settings factory
      asset_host.rs                  P4   md-asset: handler (from the spike's assets.rs) + CSP
      preview_host.rs                P4   PreviewHost + PreviewSurface impl, policy, menus, messages
      retry_policy.rs                P4   pure: the Mac's PreviewRetryPolicy (+ its tests)
      js.rs                          P4   eval → JSON, two-frame wait, snapshot scroll scripts
    export/
      mod.rs                         S0 → P4
      renderer.rs                    P4   OverlayRenderer (RenderSurface), OverlayRendererFactory
      pdf.rs                         P4   PrintOperation to file, print dialog
      snapshot.rs                    P4   visible-region capture, crop, PNG
      cover_fetch.rs                 P4   https fetch over libsoup for CoverReader
      window_exports.rs              P4   ExportPipeline per window: pickers, alerts, busy, track_output
    book/
      mod.rs                         S0 → P5
      library_host.rs                P5   BookLibraryHost (bookmark, new / open / unpack / close)
      book_window.rs                 P5   BookWindow (WindowHost), header toolbar, stages
      sidebar.rs                     P5   ListBox rows, context menu, book.* actions
    selftest/                        (cfg feature = "selftest")
      mod.rs                         S0   runner, report.json, Harness (xdotool, screenshots, waits)
      shell.rs                       P1
      documents.rs                   P2
      editor.rs                      P3
      preview.rs                     P4
      exports.rs                     P4
      books.rs                       P5
      privacy.rs                     P1
  tests/
    associations.rs                  P6   DocumentLoader extensions ↔ .desktop MimeType ↔ MIME XML
packaging/                           P6   desktop, metainfo, MIME, AppArmor, icons, copyright, man page, control template
scripts/dev.sh                       S0   + `selftest`, `package` (delegates to package.sh)
scripts/package.sh                   P6
scripts/make-icons.sh                P6   one-off: Mac AppIcon PNGs → packaging/icons (committed output)
docker/Dockerfile                    P6   only if packaging needs it (pinned base digest)
README.md, CHANGELOG.md, PRIVACY.md  P6
docs/shell-design.md, docs/shell-work-packages.md, docs/packaging.md   the design (this branch)
```

The spike's `app/src/{assets,export,menu,preview,spike,standin,util,window}.rs` are removed by S0;
their proven code moves to the new homes as starting points (`assets.rs` → `web/asset_host.rs`,
the policy and debounce parts of `preview.rs` → `web/preview_host.rs`, `export.rs` →
`export/pdf.rs` + `export/snapshot.rs`, `util.rs`'s JSON helpers → `web/js.rs`). The spike remains in
git history (commit `571f7df`) and `docs/linux-facts.md` keeps describing it.

---

## 3. S0 — the skeleton (one agent, first)

**Goal:** a binary that starts, shows one empty paper-coloured `adw::ApplicationWindow`, passes
`scripts/dev.sh test`, and in which every other package's files exist with the frozen public
interface below and minimal bodies — so six agents can compile against each other from day one.

### 3.1 What S0 writes

- `main.rs`: the module tree and `app::run()`. A `--selftest OUT` run refuses to start unless
  `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `XDG_DATA_HOME` and `XDG_CACHE_HOME` all lie under `OUT`
  (`scripts/dev.sh selftest` exports them to `OUT/home/…`; a relaunch check spawns the second `md`
  with `std::process::Command::envs`). The process cannot set them itself: `std::env::set_var` is
  `unsafe` in edition 2024 and the workspace forbids `unsafe`.
- `app.rs`: `Shell` (shell-design §1.7) as an `Rc` in a thread-local set in `startup`; `run()` builds
  the `adw::Application` of shell-design §1.1 (`NON_UNIQUE` for `--selftest`), connects
  `startup` / `activate` / `open` / `query-end` / `handle-local-options` to functions in
  `windows::window_manager` (stubs that open one empty window).
- `services/glib_seams.rs`, `services/settings_store.rs` (shell-design §9.1, with unit tests against a
  temp folder: round trip, type separation, `changed` only on difference, corrupt file),
  `services/paths.rs` (§9.3).
- `theme/mod.rs`: the CSS of shell-design §10 from `Palette`, one provider, reload on
  `connect_dark_notify`, and a `theme::changed()` event windows subscribe to.
- `windows/host.rs` (frozen, below), and every other file of §2 with its frozen interface and
  stub bodies. Stubs carry `#![allow(dead_code)] // S0 stub: removed by <owner>`; the integration
  gate (§6) fails while any is left.
- `selftest/mod.rs`: the runner (`--selftest OUT [--only g,…] [--no-input]`), `report.json`, exit
  code, and the `Harness`; the seven group files as empty `run`s.
- `scripts/dev.sh`: `test` also runs clippy with `--features selftest`; new `selftest [args]`
  (build with the feature, export `XDG_*_HOME` under `/out/selftest/home`, run under `headless()`,
  `OUT=/out/selftest`); `GDK_SCALE` and `ADW_DEBUG_COLOR_SCHEME` passed into the container;
  `package [arch]` → `scripts/package.sh`; `spike` and `probe` removed.

### 3.2 Frozen interfaces

Rust, as the owners will implement them (bodies elided). `Rc` everywhere; everything runs on the
GTK main thread.

```rust
// windows/host.rs — S0, frozen; implemented by DocumentWindow (P1) and BookWindow (P5)
pub enum WindowKind { Document, Book }
pub trait WindowHost {
    fn id(&self) -> WindowId;                               // md-logic seams::WindowId
    fn kind(&self) -> WindowKind;
    fn window(&self) -> gtk::Window;
    fn overlay(&self) -> gtk::Overlay;                      // export renderers, Zen capsule
    fn alerts(&self) -> Rc<dyn Alerts>;
    fn pickers(&self) -> Rc<dyn Pickers>;
    fn track_output(&self, output: LocalBoxFuture<'static, ()>);   // close waits ≤ 10 s
    fn snapshot(&self) -> ShellSnapshot;
    fn publish(&self);                                      // recompute the snapshot → menus
    fn is_dark(&self) -> bool;
}

// app.rs — S0, then P1
pub struct Shell {
    pub app: adw::Application,
    pub paths: Paths,
    pub settings: Rc<KeyFileSettingsStore>,
    pub scheduler: Rc<GlibScheduler>,          // also the Clock
    pub ui: Rc<GlibUiThread>,
    pub spawner: Rc<GlibSpawner>,
    pub files: Rc<StdFileSystem>,              // P2
    pub identity: Rc<CanonicalFileIdentity>,   // P2
    pub registry: Rc<md_logic::windows::WindowRegistry>,
    pub recent: Rc<RecentFiles>,               // P2
    pub examples: Rc<ExampleLibrary>,          // P2
    pub web: Rc<WebServices>,                  // P4
    pub words: Rc<dyn WordCounter>,            // md-logic SimpleWordCounter
    pub windows: Rc<WindowManager>,            // P1
    pub book: Rc<BookLibraryHost>,             // P5
}
impl Shell { pub fn get() -> Rc<Shell>; }

// menus — P1
pub struct MenuBar;  impl MenuBar {
    pub fn new(shell: &Rc<Shell>, kind: WindowKind) -> MenuBar;
    pub fn widget(&self) -> gtk::PopoverMenuBar;
    pub fn refresh(&self, snapshot: &ShellSnapshot);       // cheap when equal
}
pub fn install_window_actions(window: &adw::ApplicationWindow, kind: WindowKind,
                              dispatcher: &Rc<CommandDispatcher>);
pub fn install_app_actions(shell: &Rc<Shell>);              // in startup, with every accel
pub fn show_about(parent: &gtk::Window);

// windows — P1
pub struct WindowManager;  impl WindowManager {
    pub fn open_untitled(&self, text: Option<String>, dirty: bool) -> WindowId;
    pub fn open_path(&self, path: &str) -> Option<WindowId>;          // focuses if already open
    pub fn import_text_pack(&self, path: &str);
    pub fn import_text_bundle(&self, folder: &str);
    pub fn present(&self, id: WindowId);
    pub fn show_book(&self);
    pub fn host(&self, id: WindowId) -> Option<Rc<dyn WindowHost>>;
    pub fn active(&self) -> Option<Rc<dyn WindowHost>>;
    pub fn quit(&self);
}

// services — P2 (seams from md-logic)
pub struct StdFileSystem;          impl FileSystem for StdFileSystem
pub struct CanonicalFileIdentity;  impl FileIdentity for CanonicalFileIdentity
pub struct GioFileWatcher;         impl FileWatcher for GioFileWatcher { }  // GioFileWatcher::new() -> Rc<Self>
pub struct GtkPickers;             impl Pickers  // GtkPickers::new(window: &gtk::Window, alerts: Rc<dyn Alerts>) -> Rc<Self>
pub struct AdwAlerts;              impl Alerts   // AdwAlerts::new(window: &gtk::Window) -> Rc<Self>; fn open_changed(&self) -> &Event<bool>
pub struct RecentFiles;  impl RecentFiles {
    pub fn entries(&self) -> Vec<RecentEntry>;     // md-logic commands::RecentEntry; ≤ 10, newest first
    pub fn add(&self, path: &str);  pub fn clear(&self);
    pub fn resolve(&self, token: &str) -> Option<String>;   // None → forgotten + alert by the caller
    pub fn changed(&self) -> &Event<()>;
}
pub struct ExampleLibrary;  impl ExampleLibrary {
    pub fn examples(&self) -> Vec<md_core::document::example_library::Example>;
    pub fn read(&self, file_name: &str) -> Option<String>;
    pub fn book_folder(&self) -> std::path::PathBuf;
}
pub mod document_ops {
    pub fn rename(path: &str, new_file_name: &str) -> Result<String, String>;   // new path / message
    pub fn move_to(path: &str, folder: &str) -> Result<String, String>;
}

// editor — P3
pub struct EditorPane;   // the surface of shell-design §3.1
pub enum PanesRole { Document, BookArticle }
pub struct ArticlePanes;  impl ArticlePanes {
    pub fn new(shell: &Rc<Shell>, role: PanesRole) -> Rc<ArticlePanes>;
    pub fn widget(&self) -> gtk::Widget;                 // a ZenBin around the panes
    pub fn editor(&self) -> Rc<EditorPane>;
    pub fn find_bar(&self) -> Rc<FindBar>;
    pub fn set_preview(&self, preview: &Rc<PreviewHost>);
    pub fn set_mode(&self, mode: ViewMode);              // EffectiveMode from DocumentWindowState
    pub fn set_zen(&self, zen: Option<bool /* reading */>);
    pub fn scroll_sync(&self) -> Rc<ScrollSync>;
    pub fn layout_changed(&self) -> &Event<PaneLayout>;
}
pub struct FindBar;   // show_find(), show_replace(), find_next(forward: bool), use_selection(),
                      // is_open(), has_query(), close()
pub struct ZenCapsule;   // new(overlay: &gtk::Overlay, zen: Rc<ZenController>)
pub struct Footer;       // widget(), update(&DerivedText), set_busy(bool)
pub struct DocumentBar;  // widget(), show_conflict(on_reload, on_keep),
                         // show_save_error(text, on_retry, on_save_as), hide()

// web / export — P4
pub struct WebServices;  impl WebServices {
    pub fn new(paths: &Paths) -> Rc<WebServices>;
    pub fn new_view(&self, user_content: &webkit::UserContentManager) -> webkit::WebView;
    pub fn asset_host(&self) -> Rc<AssetHost>;
    pub fn read_rich_asset(&self, rel: &str) -> Option<Vec<u8>>;
}
pub struct PreviewHost;  // shell-design §4.4
pub struct WindowExports;  impl WindowExports {
    pub fn new(shell: &Rc<Shell>, host: Weak<dyn WindowHost>) -> Rc<WindowExports>;
    pub fn print(&self, source: String, title: String);
    pub fn export_pdf(&self, source: String, title: String);       // page size from md.pdfPageSize
    pub fn export_html(&self, source: String, title: String);
    pub fn export_epub(&self, source: String, file_name: String, document_path: Option<String>);
    pub fn export_latex(&self, source: String, title: String);
    pub fn export_text_bundle(&self, source: String, document_path: Option<String>, title: String);
    pub fn export_diagram_svg(&self, source: String, title: String, ordinal: u32);
    pub fn book_outputs(&self) -> Rc<dyn BookOutputs>;              // md-logic BookOutput's sink
    pub fn busy_changed(&self) -> &Event<bool>;
}

// book — P5
pub struct BookLibraryHost;  impl BookLibraryHost {
    pub fn has_book(&self) -> bool;  pub fn folder(&self) -> Option<String>;
    pub fn new_book(&self, parent: &dyn WindowHost) -> LocalBoxFuture<'static, bool>;
    pub fn open_book(&self, parent: &dyn WindowHost) -> LocalBoxFuture<'static, bool>;
    pub fn unpack_example_book(&self, parent: &dyn WindowHost) -> LocalBoxFuture<'static, bool>;
    pub fn close_book(&self);
    pub fn changed(&self) -> &Event<()>;
}
pub struct BookWindow;  impl BookWindow {
    pub fn show(shell: &Rc<Shell>) -> Rc<BookWindow>;     // create on demand, present
}   // + impl WindowHost
```

The md-logic types named here (`ShellSnapshot`, `CommandDispatcher`, `RecentEntry`, `ViewMode`,
`PaneLayout`, `ScrollSync`, `ZenController`, `DerivedText`, …) are md-logic's; until a module lands,
S0 stubs the use with a `// md-logic pending` local alias in the owning stub file, which the owner
replaces.

### 3.3 S0 acceptance

`scripts/dev.sh test` green (both feature sets); `scripts/dev.sh run` screenshots one paper window,
and with `ADW_DEBUG_COLOR_SCHEME=prefer-dark` passed through, the carbon-paper one;
`scripts/dev.sh selftest` writes `report.json` with zero checks and exits 0; the
`KeyFileSettingsStore` unit tests are green.

---

## 4. The six parallel packages

Start order: **P6 can start at once** (no shared files). P1–P5 start when S0 is merged. Each needs
the md-logic modules listed; until one lands, code against md.win's `docs/app-api.md` names and
adapt at merge (md.win's own packages worked this way).

| Package | Starts | Needs md-logic | Merges after |
| --- | --- | --- | --- |
| P1 Shell & windows | S0 | `commands`, `windows`, `activation`, `view::DocumentWindowState`, `documents::TextFileSession` | P2, P3 (real panes), P4 preview — earlier with stubs |
| P2 Documents & services | S0 | `documents` (`DocumentLoader`, `FileNames`, `RescueCopy`, `AssetReader`), seams | — |
| P3 Editor | S0 | `text` (`TypingHooks`, `SmartTypingAdapter`, `FormattingAdapter`, `NotePreview`), `documents::{TextSearch, LineOffsets, EditorText}`, `view` | — |
| P4 Web: preview, then exports | S0 | `preview` (first), `export` (second wave) | preview half before P1's merge; export half after md-logic `export` |
| P5 Books | S0 | `books` (second wave), `documents::TextFileSession` (`BookArticle`) | P1, P3, P4 |
| P6 Packaging & docs | now | `documents::DocumentLoader` (for its test) | anytime; final version after the integration gate |

Integration points are exactly §3.2 plus three md-logic objects other packages construct:
`TextFileSession` (P1, P5), `PreviewCoordinator` (P1 / P5 through `PreviewHost`), `ExportPipeline`
(inside `WindowExports`).

### P1 — Shell and windows

**Scope.** shell-design §1 (application, activation, windows, closing, quit, session, drafts) and §2
(menus, actions, accelerators, About, the Window menu); the document window's composition and wiring
(§5.1 state, §6.3 bars, §6.4 flows, §7 hooks).

**Files.** `app.rs`, `menus/*`, `windows/*` except `host.rs` (frozen), `selftest/shell.rs`,
`selftest/privacy.rs`.

**Drives.** `commands::{CommandTable, CommandDispatcher, CommandEnablement, ShellSnapshot}`,
`activation::{ActivationRouter, ActivationDescription}`, `windows::{WindowRegistry, WindowTitle,
UntitledNames, WindowPlacement, SessionStore}`, `view::{DocumentWindowState, ViewModeController,
ZenController, DerivedTextScheduler, SettingsViewModeStore}`, `documents::TextFileSession`,
`preview::PreviewCoordinator`; P2's services, P3's `ArticlePanes` / `Footer` / `DocumentBar` /
`ZenCapsule`, P4's `PreviewHost` / `WindowExports`, P5's `BookWindow::show` /
`BookLibraryHost`.

**Pure, unit-tested.** `menus/accels.rs` (every chord of shell-design §2.9 → its accelerator string;
`gtk::accelerator_parse` round trip is a self-test check, the string table a unit test),
`windows/drafts.rs` (create / write / delete / orphan discovery in a temp folder), the session ↔
drafts mapping, the action-name mapping (every `CommandId` → one unique action name and scope).

**Self-test (`shell`, `privacy`).** Eight menus, each `CommandTable` row once with its exact title
(walk the `PopoverMenuBar` model); ticks follow the snapshot (view mode, Typing in every window, PDF
page size in every window, the Window list ticking itself, re-clicking a ticked row keeps the tick);
`app.quit` with an untitled dirty window → no dialog, draft on disk, `session.json` names it; relaunch
(a second `md --selftest` process on the same `OUT/home`) restores it dirty with its text; plain launch
restores saved documents with mode and Zen; a disabled action's chord reaches the editor (Ctrl+Z in
Preview does nothing, in the find field undoes the field); the dialog-open flag disables window
actions; the second-launch hand-off presents the existing window (`md path` with the bus); `privacy`
as shell-design §13.3.

**Real desktop.** Dock grouping by `app_id`; the hand-off raising the window on Wayland; Ctrl+Q and
logout keeping drafts (`query-end`); window sizes restored, positions left to GNOME; F11 / Super+Up
and Zen; the menu bar's look in light and dark with Yaru's accent overridden; F10.

### P2 — Documents and services

**Scope.** shell-design §6 (types, dialogs, I/O, monitor, rename / move, imports, recents, examples)
and §7.8 (alerts).

**Files.** `services/{file_system,file_watcher,watch_events,pickers,alerts,recent,examples,document_ops}.rs`,
`selftest/documents.rs`.

**Drives.** md-logic seams (`FileSystem`, `FileIdentity`, `FileWatcher`, `Pickers`, `Alerts`),
`documents::{DocumentLoader, FileNames, RescueCopy, AssetReader}`; md-core `export::text_bundle`,
`book::book_folder::is_valid_name`, `document::example_library`.

**Pure, unit-tested.** `watch_events.rs` — `(event, file, other, our_path, exists_after_250ms)` →
`Changed | Renamed(old, new) | Deleted | Ignore` for every row of shell-design §6.7 including vim's
rename-away-then-create; `recent.rs`'s filter (app name, cleared-at stamp, missing files, ten newest,
duplicate names with folders) over plain records; the save-dialog extension rule (append the default
extension only when the name has none; report a collision).

**Self-test (`documents`).** The real `GioFileWatcher` against a temp folder: append in place,
`mv tmp over`, `g_file_replace_contents`-style write, delete, rename away, vim's dance — each reported
as the table says; in-place write keeps the inode (`std::os::unix::fs::MetadataExt::ino`); `rename`
refuses an existing target; `move_to` across `/tmp` and the home folder (copy + delete); a
`.textpack` and a `.textbundle` folder import titles; `RecentFiles` add / list / clear-hides /
missing-file forget against `OUT/home`'s XBEL; `AdwAlerts` and `GtkPickers` presented and dismissed
by `Escape` through `xdotool` (`DialogError::Dismissed` → `None`).

**Real desktop.** The chooser on the desktop (GTK's or the portal's; no Encoding row; one filter);
the save dialog's typed name without extension; Files showing md's documents under Recent; monitors
on an SMB/NFS mount (expected: no events, the stamp still guards); drag and drop from Files onto the
editor and the preview.

### P3 — Editor

**Scope.** shell-design §3 (configuration, offsets, typing hooks, jumps, editor scroll, find bar),
§5.2–§5.4 (panes, Zen, footer) and the bar widget of §6.3.

**Files.** `editor/*`, `selftest/editor.rs`.

**Drives.** `text::{TypingHooks, FormattingAdapter, NotePreview}`, `documents::{TextSearch,
LineOffsets, EditorText}`, `view::{SplitLayout, ScrollSync, ScrollSyncGuard, ZenController,
DerivedText}`, `preview::EditorJumpTracker`, `settings::{TypingSettings, wrap_selection_setting}`;
md-core `text::utf16`.

**Pure, unit-tested.** `offsets.rs` (ASCII, Cyrillic, combining marks, emoji, surrogate pairs,
offsets at both ends, out of range clamped); the event-to-hooks sequencing extracted as a small pure
state machine (`typing.rs`'s `UserActionTracker`: begin/end nesting, deletion side from a collapsed
caret, the own-edit and history flags) tested without GTK.

**Self-test (`editor`, input tier).** Every row of shell-design §13.3's `editor` line, typed with
`xdotool` into the real `TextView`, asserting the buffer text, the selection and the undo results
(the capital and the wrap each one undo step — this is the check that GtkTextHistory does not merge
them); Enter continuation, exit and table rows; Shift+Enter; Tab; the format chords and menu rows (one
undo each, focus back in the editor); find as you type, Ctrl+G, Replace, Replace All as one undo,
Escape; the placeholder shown only when empty; Split ↔ stacked at 640 px of pane width; Zen geometry
(the child allocated 2/3 × 92 % of the window) and the capsule fading after 2.5 s; the footer text
for the five md.win word-count vectors.

**Real desktop.** IBus: a dead key (`'` + `e` on US-intl) at a sentence start gives `É`; a CJK
engine composing and committing without a capital or a list edit; the German layout's AltGr+2 types
`²` and left Ctrl+Alt+2 makes Heading 2; Ctrl+[ / Ctrl+] on layouts where brackets need AltGr;
Nimbus Mono PS legibility at 15 pt; the accent caret; touchpad scrolling sync.

### P4 — Web: preview, then exports

**Scope.** shell-design §4 (preview host, origin, CSP, scripts, policy, theme, retry) and §7.1–§7.7
(renderer, print, PDF, HTML, EPUB snapshots, cover, SVG, LaTeX, TextBundle wiring).

**Files.** `web/*`, `export/*`, `selftest/preview.rs`, `selftest/exports.rs`.

**Drives.** `preview::{PreviewCoordinator, LinkPolicy, ScreenHtml, Scripts, JsonScript, AssetMime,
AssetOrigin}`, `export::{ExportPipeline, RichSnapshotter, RenderCompletePoller, CoverReader,
BusyRing}`, seams `PreviewSurface`, `RenderSurface`, `RenderSurfaceFactory`; md-core
`markdown::html::document`, `export::{pdf_export, html_export, epub_export, diagram_svg,
latex_export, text_bundle}`, `document::page_size`.

**Pure, unit-tested.** `retry_policy.rs` (the Mac's `PreviewRetryPolicy` tests, ported);
`asset_host.rs`'s `split_uri`, containment and MIME (the spike's tests, kept); `snapshot.rs`'s crop
arithmetic (zoom, surface scale, scroll clamp → crop rectangle); the CSP header string.

**Self-test (`preview`, `exports`).** As shell-design §13.3. The preview half also proves that
the link guard and the CSP coexist with the message handlers; the exports half writes its PDFs,
HTML, EPUB, SVGs, `.tex` and bundle into `OUT/exports/` and checks them (`pdfinfo` from
poppler-utils in the image; EPUB entries through md-core's `zip_reader`). One run with
`GDK_SCALE=2` (dev.sh passes it through) proves the HiDPI division.

**Real desktop.** The WebKit sandbox actually on (AppArmor profile from the `.deb`); GPU / DMA-BUF
rendering on Wayland (Intel, AMD; NVIDIA if available); printing through GTK's dialog to a real or
CUPS-PDF printer; remote images and the EPUB `https:` cover on a real network; the EPUB in a reader
(Foliate) with its cover.

### P5 — Books

**Scope.** shell-design §8.

**Files.** `book/*`, `selftest/books.rs`.

**Drives.** `books::{BookNavigatorModel, BookStepper, BookOutput, BookSidebarModel,
BookLibraryModel}`, `documents::TextFileSession` (`SessionRole::BookArticle`), md-core
`book::{book_folder, book_compiler, book_order, structured_book}`,
`document::book_article_opens::BookArticleOpens`; P3's `ArticlePanes` / `Footer` / `DocumentBar`, P4's
`PreviewHost` / `WindowExports`, P1's `MenuBar` / `WindowManager`.

**Self-test (`books`).** As shell-design §13.3, against copies of `examples/Example Book` in
`OUT/books/`: sidebar order equals md-core's reading order; selection opens the article; edits
autosave in place; rename / move up / move down renumber on disk; delete picks the neighbour;
new chapter / article; Alt+Up / Alt+Down and the header chevrons step; Open in New Window → hand-off
page → Show Window presents the document window, closing it gives the article back; the sidebar
clamps to 200–320 px; Print Book renders; Export Book PDF / EPUB / LaTeX produce files.

**Real desktop.** A book folder on a removable or network drive (unplugged → the not-accessible
state); the sidebar resize feel; full screen as the writing room.

### P6 — Packaging and docs

**Scope.** `docs/packaging.md` end to end: the `.deb` for amd64 and arm64, the desktop integration
files, the AppArmor profile, icons, copyright, the man page; and README, CHANGELOG (1.6.0, the
family's number), PRIVACY.md (exactly shell-design §9.2 and the network uses of §4.10 / §7.5).

**Files.** `packaging/**`, `scripts/package.sh`, `scripts/make-icons.sh`, `docker/Dockerfile` (only
if needed), `README.md`, `CHANGELOG.md`, `PRIVACY.md`, `app/tests/associations.rs`.

**Acceptance (Docker).** `app/tests/associations.rs` green (every `DocumentLoader` open extension has
a glob in a MIME type the `.desktop` lists, and no extra); `scripts/dev.sh package arm64` and
`… amd64` produce `dist/md_1.6.0_<arch>.deb` twice with identical SHA-256 (reproducible);
`dpkg-deb --info/--contents` match docs/packaging.md §1; a throwaway `ubuntu:26.04` container
`apt install ./md_1.6.0_<arch>.deb` resolves every dependency, `md --version` prints `md 1.6.0`,
`update-mime-database` maps every extension (`xdg-mime query filetype`), `desktop-file-validate` and
`appstreamcli validate --no-net` pass, lintian (installed in that throwaway container only) shows no
error and only the overrides docs/packaging.md §8 lists; `apt remove` / `apt purge` behave as docs/packaging.md §11 (items 13–14)
describes.

**Real desktop.** docs/packaging.md §11 — the manual install / uninstall plan on Ubuntu 26.04 amd64
and arm64.

---

## 5. What only a real Ubuntu 26.04 desktop can check (all packages)

Collected for the person who installs the `.deb` (docs/packaging.md §11 is the procedure): Wayland
focus and activation; the AppArmor `userns` profile letting WebKit's bubblewrap sandbox start; GNOME
Shell's chord grabs; IBus composition; portals for dark mode; HiDPI and fractional scaling; printing;
font rendering; Files integration (double-click, Open With, Recent); logout with unsaved drafts;
drag and drop.

---

## 6. Integration gate and done

The shell is done when, on the merged branch:

1. `scripts/dev.sh test` is green with zero warnings (workspace, `md`, `md --features selftest`) and no
   `S0 stub` allow remains;
2. `scripts/dev.sh selftest` (all groups, input tier on) exits 0 — and once more with `GDK_SCALE=2`;
3. every `CommandId` of the Linux table is wired: P1's `shell` group fails if a row's action has no
   handler in the window kind that can enable it (md.win's `WindowSurfaceTests` rule);
4. `scripts/dev.sh package amd64` and `arm64` produce reproducible `.deb`s that pass P6's checks;
5. each package's real-desktop list is gathered into one checklist for nettrash
   (`docs/desktop-checklist.md`; docs/packaging.md §11 references it).

The order of merges: S0 → P2, P3, P6 → P4 (preview) → P1 → P4 (exports) → P5 → the gate. P1 runs the
gate and routes failures to the owning package.
