# Verified facts for the md for Linux GTK shell (W1 spike, measured 2026-10-27)

Everything below was measured in the running app (`app/`, binary `md`) under Xvfb in the
`ubuntu:26.04` dev image on an Apple-silicon Mac (Docker Desktop, aarch64), unless a line says
*unverified*. The spike drives itself: `scripts/dev.sh spike` runs `md --spike OUT`, which steps
through every check with real XTEST input (`xdotool`) and writes screenshots (ImageMagick `import`
of the Xvfb root window), PDFs and `spike-results.json` into `OUT`. `scripts/dev.sh probe` runs the
scheme-registration matrix, one fresh process per variant.

| Command | What |
| --- | --- |
| `scripts/dev.sh image` | build `md-linux-dev` from `docker/Dockerfile` |
| `scripts/dev.sh test` | `cargo fmt --check`, `clippy --workspace --all-targets -D warnings`, `clippy -p md --all-targets -D warnings`, `cargo test --workspace` |
| `scripts/dev.sh spike` | the whole GUI run (≈ 3 min); `MD_OUT=<dir>` picks the output folder |
| `scripts/dev.sh probe [variant…]` | `plain secure cors secure_cors local secure_cors_csp`, each in its own process |
| `scripts/dev.sh run [args]` | run `md` headless and screenshot after `MD_SHOT_AFTER` s |

## 0. Toolchain and crates

- Ubuntu 26.04.1 LTS archive: rustc/cargo/clippy/rustfmt **1.93.1**, GTK **4.22.4**, libadwaita
  **1.9.1**, WebKitGTK 6.0 **2.52.6**, GLib **2.88.0**, libsoup **3.6.6** (`pkg-config --modversion`;
  the same versions report at runtime via `gtk::major_version()…`, `adw::major_version()…`,
  `webkit::functions::major_version()…`).
- crates.io (resolved with the workspace's `rust-version = "1.93"`, resolver 3): `gtk4 0.11.5`
  (`v4_22`), `libadwaita 0.9.2` (`v1_9`), `webkit6 0.6.1` (`v2_52`), `glib`/`gio 0.22.10` (`v2_88`).
  gtk4 0.11 declares MSRV 1.92. Renamed in `app/Cargo.toml` to `gtk`, `adw`, `webkit`.
- `webkit6` re-exports `soup` (`webkit::soup::MessageHeaders`) and `javascriptcore`, so headers
  and JS values need no extra dependency.
- A cold build of the app with all GTK crates takes ≈ 30 s in the container; incremental ≈ 2 s
  (registry and target in the named volumes `md-linux-cargo`, `md-linux-target-<worktree>`).
- gtk4-rs details that cost a compile: `ApplicationWindow` has two `activate_action`s (call
  `gio::prelude::ActionGroupExt::activate_action(&win, …)` or `WidgetExt::activate_action`);
  `webkit::WebContext::default()` is the *inherent* `Option<WebContext>` (the global context), and
  `.unwrap_or_default()` falls back to `WebContext::new()`; `Settings::set_enable_dns_prefetching`
  is deprecated since 2.48 (a warning under `-D warnings`); `gtk::enumerate_printers` wants a
  `Send + Sync` closure; `gtk::style_context_add_provider_for_display` is *not* deprecated in 0.11.

## 1. Application, single instance, menu bar, accelerators — proven

- `adw::Application::builder().application_id("me.nettrash.md").flags(gio::ApplicationFlags::HANDLES_OPEN)`;
  `connect_open(|app, files: &[gio::File], _hint|)`; `app.run_with_args(&args)` after stripping the
  app's own flags (`--spike`, `--probe`).
- **Single instance:** with the primary running, `md /tmp/md-spike-open.md` exited 0 after **20 ms**
  and the primary's `open` received `/tmp/md-spike-open.md` (screenshot 09). The hand-off is a
  D-Bus call on the session bus. Without a session bus the app still runs, prints
  `Gtk-WARNING: Unable to acquire session bus: Failed to execute child process "dbus-launch"`,
  and every launch is its own primary (no hand-off).
- Window: `adw::ApplicationWindow` → `content(adw::ToolbarView)` with two top bars, an
  `adw::HeaderBar` (title widget `adw::WindowTitle`) and the menu bar
  `gtk::PopoverMenuBar::from_model(Some(&gio::Menu))`; content `gtk::Overlay` → `gtk::Paned`.
  Eight menus File, Edit, Format, View, Book, Go, Window, Help (`_File` mnemonics). A real click on
  "View" opened it with the radio items and **Ctrl+1/2/3 shown beside them** (screenshot 07).
- Accelerators: `app.set_accels_for_action("win.view-mode::edit", &["<Control>1"])` (detailed
  names, one per target) on a stateful action
  `gio::SimpleAction::new_stateful("view-mode", Some(glib::VariantTy::STRING), &"split".to_variant())`.
  Six XTEST key presses, focus alternating between `GtkTextView` and `WebKitWebView` (checked with
  `GtkWindowExt::focus`): **every chord switched the mode and fired exactly once**, from either
  widget. WebKitGTK has no equivalent of WebView2's double fire (microsoft-ui-xaml #6231), so
  md.win's 150 ms chord debounce is unnecessary here (harmless if md-logic keeps it).
- Not exercised: a *first* launch with a file path (only the hand-off to a running primary was
  driven), and Ctrl+Z/X/C/V — the spike leaves them to `GtkTextView`'s built-in shortcuts and
  gives Undo/Cut/Copy/Paste no app accelerators. Whether app accels on those chords would shadow
  the text view's own is untested.

## 2. Editor and preview, 350 ms debounce, scroll kept — proven

- `GtkTextView` (`WrapMode::WordChar`, margins 24) in a `GtkScrolledWindow` (no horizontal bar)
  beside the `webkit::WebView` in a horizontal `gtk::Paned`. View modes hide one child
  (`set_visible(false)`); the hidden `WebView` stays in the tree.
- Debounce: `glib::timeout_add_local_once(Duration::from_millis(350), …)` → `glib::SourceId`,
  cancelled with `SourceId::remove()` on every buffer `changed`.
- Measured: 17 characters typed by `xdotool type --delay 60` over 623 ms → **exactly one** reload,
  starting **353 ms** after the last keystroke; the typed text was in the buffer and in the page.
- Scroll: the reload reads `window.scrollY` (1200), calls `WebView::reload()`, and after
  `LoadEvent::Finished` runs `window.scrollTo(0, y)`. **WebKit had already restored 1200 itself**
  at `Finished` (history scroll restoration on `reload()`); the explicit restore is kept for
  `load_uri()` paths. `reload()` re-requests `index.html` from the scheme handler every time
  (`Cache-Control: no-store`), so the new HTML is picked up.
- First load and a new document use `load_uri(index)` (scroll reset), as on the Mac.

## 3. The `md-asset:` origin — proven; security registrations measured

**Handler.** `webkit::WebContext::register_uri_scheme("md-asset", |req: &webkit::URISchemeRequest| …)`
answers synchronously:
`URISchemeResponse::new(&gio::MemoryInputStream::from_bytes(&bytes), len)`, `.set_status(200, Some("OK"))`,
`.set_content_type(mime)`, `.set_http_headers(webkit::soup::MessageHeaders::new(MessageHeadersType::Response))`
(`Cache-Control: no-store`), `req.finish_with_response(&response)`. A miss is a 404 response with
an empty body. `index.html` is in memory per surface; `rich/…` is read from `rich_dir` with a
containment check (no empty/`.`/`..` segment, no `\` or `%`) and the Mac's MIME table (md.win
`AssetMime`). URL shape `md-asset://<surface>/index.html` — the host names the surface, so the
preview and each export renderer are separate origins served by one handler.

- `location.origin` is the tuple origin `md-asset://preview-1` (not opaque); relative `rich/…`,
  `import('./plantuml.js')` and `url(fonts/…)` in katex.min.css all resolve with the golden bytes
  unchanged. Requests seen for the golden document: `index.html`, `katex.min.css`, `katex.min.js`,
  `mhchem.min.js`, `mermaid.min.js`, `viz-global.js`, `highlight.min.js`, `md-init.js`,
  `plantuml.js`, four `KaTeX_*.woff2` — all 200 (WebKit asks for woff2 only).
- `Content-Type: text/html` plus the document's `<meta charset="utf-8">` is enough: em dashes,
  Cyrillic and emoji render.
- **It renders (screenshots 01–06):** golden `document.html` (stylesheet placeholder filled back in)
  → 14 `.katex` (3 display), 2 `code.hljs`, 3 Mermaid SVGs, 3 PlantUML SVGs; the golden
  `examples/07-Diagrams.html` body in the same shell → 3 Mermaid, **2 Graphviz**, 2 PlantUML SVGs.
  Render-complete ≈ 1.0 s after navigation (0.3–0.9 s after `Finished`), PlantUML included.

**Which `WebKitSecurityManager` registrations (fresh process per variant, `scripts/dev.sh probe`):**

| Registration | Engines | Notes |
| --- | --- | --- |
| none | all render | `isSecureContext` **true**, `crypto.subtle` present |
| `register_uri_scheme_as_secure` | all render | same |
| `register_uri_scheme_as_cors_enabled` | all render | same |
| secure + cors | all render | what `SchemeSecurity::SHELL` uses |
| secure + cors + CSP header | all render | CSP below |
| `register_uri_scheme_as_local` | **broken** | `Origin md-asset://probe is not allowed by Access-Control-Allow-Origin` for the module script and the dynamic import: no KaTeX, highlight, Graphviz, PlantUML, no completion flag (Mermaid alone draws, via its own `startOnLoad`) |

- **Registrations leak within a process.** An in-process matrix (several `WebContext::new()` in
  one run) failed the `secure+cors+CSP` variant with the same CORS error after the `local` variant
  had run; in its own process the same variant passes. Register the scheme once, on one context,
  and never as `local`.
- **Remote images:** an `http://` PNG from an nginx container loaded (`naturalWidth` 966) under
  none, secure, and secure+cors+CSP — no mixed-content block or upgrade; the golden's `https://`
  images load too. md's "remote images load" parity holds; `secure` changes nothing measurable.
- **File access:** nothing needed — no `file:` URL is ever loaded. A `file:///etc/hostname` link is
  refused by WebKit itself (`Not allowed to load local resource`) and never reaches
  `decide-policy`; `allow_file_access_from_file_urls` / universal access stay at their defaults.

**Links (`connect_decide_policy`).** `decision.downcast_ref::<webkit::NavigationPolicyDecision>()`
→ `navigation_action()` → `request()?.uri()`, `is_user_gesture()`; `decision.use_()` /
`decision.ignore()`; return `true`. `connect_create(|_, _| None)` so no window is ever made.

| Click (element.click() and, for `javascript:`, a real XTEST click) | `decide-policy` | Result with no guard |
| --- | --- | --- |
| `https://example.com/x` | NavigationAction, gesture=true → OpenExternally (`ignore`; the shell hands it to `gtk::UriLauncher`, not launched in the spike) | stays |
| `target=_blank` https | **NewWindowAction** → OpenExternally | stays |
| `other.md` | NavigationAction `md-asset://preview-1/other.md` → Cancel | stays |
| `#target` | NavigationAction `…/index.html#target` → Allow | scrolls (y 1259) |
| `javascript:…` | **never called** | **script ran** (both click kinds) |
| `onclick="…"` | only the `#` navigation | handler ran |
| `file:///…` | never called | WebKit refuses |

- Unlike WebView2, WebKitGTK raises `decide-policy` for same-document fragment hops, so the
  policy must allow `index.html#…` (md.win's `LinkPolicy` already compares without the fragment).
- `javascript:` cannot be stopped by policy. md.win's capture-phase guard as a document-start user
  script (`UserContentManager::add_script(&UserScript::new(src, UserContentInjectedFrames::TopFrame,
  UserScriptInjectionTime::Start, &[], &[]))`) stopped it for both click kinds and stopped
  `other.md` before policy; it does not stop inline `onclick`.
- A response header `Content-Security-Policy: default-src 'self' data: blob:; script-src 'self'
  'unsafe-eval' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:
  https: http:; font-src 'self' data:` (no `'unsafe-inline'` for scripts) refused both the
  `javascript:` URL and the inline handler (`Refused to execute a script because its hash or
  'unsafe-inline' does not appear…`) while every engine still rendered (probe above).
  Recommendation: guard (parity) **plus** this CSP on `index.html` responses. Guard + CSP *together*
  and the user script/message handler under CSP were not measured as one configuration.

## 4. JavaScript evaluation and render-complete — proven

- `view.evaluate_javascript_future(script, None, None).await` → `Result<javascriptcore::Value, glib::Error>`;
  `value.to_json(0)` gives JSON text (`"1"` with its quotes, `null`); `is_number()`/`to_double()`
  for numbers; a thrown exception is `Err`. `JSON.stringify({...})` from the page round-trips.
- `rich/md-init.js` sets `<html data-md-render-complete="1">` after math, highlight, Mermaid,
  Graphviz and the sequential PlantUML pass. Polling
  `document.documentElement.getAttribute('data-md-render-complete')` every 250 ms (md.win §4.8)
  saw `"1"` in every run.
- **The Mac's event path works unchanged:** md-init.js calls
  `window.webkit.messageHandlers.mdRender.postMessage('complete')`, and WebKitGTK provides
  `window.webkit.messageHandlers` — `UserContentManager::register_script_message_handler("mdRender", None)`
  + `connect_script_message_received(Some("mdRender"), …)` received exactly one message per load.

## 5. PDF without a dialog — proven

```rust
let settings = gtk::PrintSettings::new();
settings.set_printer(&glib::dgettext(Some("gtk40"), "Print to File"));   // see below
settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(&gio::File::for_path(path).uri()));
let paper = gtk::PaperSize::new(Some(gtk::PAPER_NAME_A4));               // or:
let paper = gtk::PaperSize::new_custom("custom_6x9in", "6 × 9 in", 6.0, 9.0, gtk::Unit::Inch);
settings.set_paper_size(&paper);
let setup = gtk::PageSetup::new();
setup.set_paper_size(&paper);
setup.set_orientation(gtk::PageOrientation::Portrait);
setup.set_top_margin(0.5, gtk::Unit::Inch);      // and bottom, left, right
let op = webkit::PrintOperation::new(&view);
op.set_print_settings(&settings);  op.set_page_setup(&setup);
op.connect_finished(…);  op.connect_failed(|_, err| …);
op.print();                                                              // no dialog
```

- GTK 4.22 has the print backends **built into libgtk-4** (`GtkPrintBackendFileBuiltin`,
  `GtkPrintBackendCupsBuiltin`; there is no `printbackends/` module directory any more).
  `gtk::enumerate_printers` in the container lists only `Print to File (virtual, accepts_pdf)`.
- golden document with the export stylesheet: **A4 → 595 × 842 pt, 11 pages** in ≈ 0.19 s;
  **6 × 9 in → 432 × 648 pt, 15 pages** (`pdfinfo`). Margins: the first word's `xMin` is 80.8 pt
  = 36 pt (0.5 in) + 42 pt (the body's 56 CSS px padding) + side bearing. Producer
  `Skia/PDF m145`; real text with embedded subset fonts (Liberation Mono family, DejaVu Sans,
  KaTeX as Type 3); Mermaid/PlantUML stay vector (their labels extract with `pdftotext`); code and
  inline-code backgrounds print (`Settings::set_print_backgrounds(true)`); no headers or footers.
- The same PDF (11 pages) comes from the visible preview, from a never-parented `WebView`, and from
  the hidden overlay renderer of §6.
- **The printer name is translated.** Under `LC_ALL=ru_RU.UTF-8` (with
  `language-pack-gnome-ru-base`) a hard-coded `"Print to File"` failed with `failed`:
  *Принтер не найден*; `glib::dgettext(Some("gtk40"), "Print to File")` returned *Печать в файл*
  and printed. Never leave the printer unset either: WebKit then uses the default (CUPS) printer.

## 6. Snapshots and the offscreen renderer — proven (with a mapped, invisible view)

- `view.snapshot_future(webkit::SnapshotRegion::FullDocument, webkit::SnapshotOptions::NONE).await`
  → `gdk::Texture` at the view's device scale. **2× comes from page zoom:**
  `view.set_zoom_level(2.0)`; zoom shrinks the CSS viewport, so the renderer is made 2× wide
  (`set_size_request(1190, 842)` → 595 CSS px). Crop with `gdk::TextureDownloader`
  (`MemoryFormat::R8g8b8a8`) → `gdk::MemoryTexture::new` → `save_to_png`. The element rect comes
  from `getBoundingClientRect()` + scroll; texture width ÷ `scrollWidth` gives the factor (2.0).
- Result: the first `.katex-display` = 483 × 47.9 CSS px → **966 × 96 px PNG**
  (`katex-2x-overlay.png`). On the live preview the same call works but reflows the pane (the
  element became 242.5 CSS px wide at zoom 2) — never snapshot the live preview.
- **Never-parented `WebView`:** loads, runs JS, reaches render-complete in 0.9 s, prints the same
  11-page PDF — but `innerWidth × innerHeight` is **0 × 0**, `visibilityState` "hidden", the layout
  is zero-width and the snapshot useless (element 0 px wide in a 739 × 69108 texture). Fine for
  PDF and DOM capture, not for images.
- **What works for everything:** the `WebView` as an overlay child of the document window —
  `overlay.add_overlay(&view)` with `set_opacity(0.0)`, `set_can_target(false)`,
  `set_can_focus(false)`, `halign/valign = Start`, `set_size_request(1190, 842)`: mapped,
  `visibilityState` "visible" (no timer throttling), viewport **1190 × 842 even though the overlay
  area was ≈ 1272 × 738** (overlay children get their request and are not measured, so the window's
  minimum size is unaffected). This is md.win's off-canvas renderer on GTK.

## 7. Light/dark and typography — proven / proposed

- `adw::StyleManager::default()`: under Xvfb there is no settings portal —
  `system_supports_color_schemes()` false, `is_dark()` false. `set_color_scheme(adw::ColorScheme::ForceDark)`
  fires `connect_dark_notify`; the window re-tints and re-renders with `data-md-dark="1"` and the
  dark stylesheet (screenshots 10, 11). WebKit's `prefers-color-scheme: dark` **follows the app's
  libadwaita state** (true under ForceDark, false again under ForceLight). `WebView::set_background_color`
  takes the paper colour so a reload never flashes white.
- Fonts an Ubuntu 26.04 desktop installs (`apt-get install -s ubuntu-desktop-minimal`):
  fonts-dejavu-core/-mono (Depends), fonts-liberation, fonts-liberation-sans-narrow,
  fonts-noto-core/-mono/-cjk/-color-emoji, fonts-ubuntu (Ubuntu, Ubuntu Mono, Ubuntu Sans,
  Ubuntu Sans Mono), fonts-droid-fallback, and **fonts-urw-base35** (via ghostscript: Nimbus Mono
  PS, Nimbus Roman/Sans, C059, P052, URW Bookman…); `ubuntu-desktop` adds fonts-lato,
  fonts-opensymbol. No American Typewriter, no Lucida Sans Typewriter, no Courier New.
- What the shared stylesheet gets (`"American Typewriter", "Courier New", serif`): WebKit skips the
  missing American Typewriter and takes "Courier New", which fontconfig's `30-metric-aliases.conf`
  maps to **Liberation Mono** (measured by canvas width) — prose and code end up in the same face.
- **Courier Prime from the archive is unusable:** all four `fonts-courier-prime` OTFs report
  `weight=50 slant=0 style=Light,Regular` (`fc-query`), so fontconfig cannot tell regular, bold and
  italic apart; `fc-match "Courier Prime"` returns the *Bold* file and the specimen line renders all
  bold (screenshot 12). It is not installed by default anyway.
- **Proposal.** Prose: **Nimbus Mono PS** — URW's Courier, the typewriter face Ubuntu actually
  ships, with real Regular/Bold/Italic/Bold Italic (weights 80/200, slants 0/100), Cyrillic and
  Greek coverage; `Depends: fonts-urw-base35, fonts-liberation` in the .deb makes it certain.
  - Editor (`GtkTextView` CSS via `gtk::CssProvider::load_from_string` +
    `gtk::style_context_add_provider_for_display`): `font-family: "Nimbus Mono PS", "Liberation Mono",
    monospace; font-size: 15pt` — the Mac's 15 pt; Pango resolved it to `Nimbus Mono PS 15` (20 px).
  - Preview and paper (Print/PDF) only, never exports — md.win's `md-win-fonts` pattern:
    `<style id="md-linux-fonts">body{font-family:"Nimbus Mono PS","Courier New",serif;}</style>`.
    Code keeps the stylesheet's `"Courier New"` → Liberation Mono, so prose and code differ as on
    the Mac and Windows.
  - Nimbus Mono PS has thin Courier strokes; if it reads too light on screen, the alternative is
    bundling upstream Courier Prime (OFL) — a licence decision for nettrash, as on Windows.

## 8. File dialogs and file monitoring — APIs proven; picking a file is not

- `gtk::FileDialog::builder().title(…).modal(true).filters(&gio::ListStore<gtk::FileFilter>).initial_name(…).build()`.
  Async shapes in gtk4-rs 0.11: `open_future(Some(&win)) -> Result<gio::File, glib::Error>`;
  `open_text_file_future(Some(&win)) -> Result<(gio::File, glib::GString /*encoding*/), glib::Error>`
  (GTK 4.18+: the dialog grows an **Encoding** choice, "Automatically Detected" by default);
  `save_future`, `save_text_file_future` (file, encoding, line ending), `select_folder_future`;
  callback forms take `Option<&gio::Cancellable>`.
- Under Xvfb there is no xdg-desktop-portal, so GTK shows its own chooser (screenshot 13, with the
  Encoding and filter choices). Escape → `Err` with `DialogError::Dismissed` ("Dismissed by
  user"); `cancellable.cancel()` → `DialogError::Cancelled` ("Cancelled by application"). Choosing
  a file headless was not attempted; on the desktop the portal's GNOME chooser appears instead,
  and whether the Encoding choice survives the portal is *unverified*.
- `gio::File::monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)` →
  `GInotifyFileMonitor`; `connect_changed(|_, file, other: Option<&gio::File>, event|)`. Events
  on the container's own filesystem:

| Change to `doc.md` | `WATCH_MOVES` | no flags |
| --- | --- | --- |
| append in place | `Changed`, `ChangesDoneHint` | same |
| write `.doc.md.tmp`, rename over | **`Renamed` file=`.doc.md.tmp` other=`doc.md`** | `Deleted .doc.md.tmp`, `Created doc.md`, `ChangesDoneHint` |
| `g_file_replace_contents` | `Renamed .goutputstream-XXXX → doc.md` | `Deleted …`, `Created`, `ChangesDoneHint` |
| delete | `Deleted` | `Deleted` |

  An atomic save by another editor never says `Changed`: with `WATCH_MOVES` it is a `Renamed`
  whose *other* file is ours. The monitor keeps working across replacements (it watches the
  directory).

## 9. Docker-only workarounds vs. what a real Ubuntu desktop needs

**Only in Docker (all in `scripts/dev.sh headless`):**

- `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1` — **required.** With the sandbox on, bubblewrap
  fails (`bwrap: Creating new namespace failed: Operation not permitted`; Docker's seccomp profile
  and missing `CAP_SYS_ADMIN` deny user namespaces) and WebKit aborts the **whole UI process**:
  `** (md): ERROR **: Failed to fully launch dbus-proxy: Child process exited with code 1`. It is a
  `g_error`, not a recoverable `web-process-terminated`. `MD_WEBKIT_SANDBOX=1` reproduces it.
- `dbus-run-session` — a private session bus for the single-instance hand-off (§1).
- Xvfb `:99` 1440×900×24, `GDK_BACKEND=x11` set explicitly: X11 only; GSK uses `GskGLRenderer`
  on Mesa llvmpipe. No WebKit compositing or DMA-BUF variables were needed.
- `LIBGL_ALWAYS_SOFTWARE=1` — cosmetic: without it Mesa prints `libEGL warning: DRI3 error: Could
  not get DRI3 device`; rendering and every result were identical.
- No window manager: windows and dialogs open at (0, 0); `xdotool windowfocus` works, and the
  window was already active.
- `--shm-size=1g` is set preemptively (not measured as needed). The image omits fonts-noto-cjk
  (CJK shows as boxes in the editor) to keep the image small.

**On a real Ubuntu 26.04 desktop:**

- Keep WebKit's sandbox (never ship the env variable). Ubuntu 26.04 enforces
  `kernel.apparmor_restrict_unprivileged_userns = 1` (`/usr/lib/sysctl.d/10-apparmor.conf`) and
  its `apparmor` package carries an unconfined `userns,` profile for **every** WebKitGTK app it
  knows — devhelp, epiphany, evolution, foliate, geary, wike — besides `bwrap-userns-restrict`.
  The .deb should therefore ship `/etc/apparmor.d/md`:
  `abi <abi/5.0>, include <tunables/global> profile md /usr/bin/md flags=(unconfined) { userns, include if exists <local/md> }`.
  *Unverified here* (Docker Desktop's kernel has no AppArmor) and a launch blocker if wrong,
  because a failed bwrap aborts the app (above).
- The session bus, xdg-desktop-portal(-gnome) for the file dialogs and the colour scheme, and a
  Wayland session are all present on the desktop; the spike only ran X11.
- `Depends: fonts-urw-base35, fonts-liberation` for the typography above.
- The file printer's name is localized — use `glib::dgettext(Some("gtk40"), "Print to File")`.

## 10. Risks for the real shell

1. AppArmor/bubblewrap on 26.04 (above) — test `md` from the .deb on a real 26.04 install first.
2. Wayland: accelerators, focus, dialog parenting and the invisible overlay renderer were proven on
   X11 only. GTK's Wayland backend is the desktop default.
3. The hidden overlay renderer is a child of a document window: an export from a window being
   closed must keep the window alive until it finishes; a never-parented view is only good for PDF
   and DOM capture.
4. Typography: Nimbus Mono PS is light on screen at small sizes — judge on a real display.
5. Portal file chooser: the `open_text_file` encoding choice may not survive xdg-desktop-portal.
6. In dark mode the Mermaid flowchart drew with light nodes (screenshot 11). Likely cause, shared
   by every port: mermaid.min.js renders `.mermaid` blocks itself on `load` (`startOnLoad`
   defaults to true — the `local` probe drew 3 Mermaid SVGs although md-init.js never ran), before
   md-init.js's `initialize({theme: 'dark'})`, whose `run()` then skips the processed nodes.
   md-init.js is byte-identical across ports: compare with the Mac before treating it as a port
   bug.
7. Scheme registrations are process-global in practice (§3): one context, registered once.
