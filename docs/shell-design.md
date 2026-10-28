# md for Linux — the GTK shell design

The design for `app/` (binary `md`): the GTK 4 / libadwaita / WebKitGTK 6.0 shell of md 1.6 for
Ubuntu 26.04, written in Rust over `crates/md-core` (the parity core) and `crates/md-logic` (the
shell's logic, port of md.win's `Md.App.Logic`). It is structured like md.win's
`docs/shell-design.md` and reuses its decisions wherever Linux allows; where they differ, this
document says why. `docs/shell-work-packages.md` splits the work between parallel agents and
`docs/packaging.md` covers the `.deb`.

**Sources.** `docs/port-plan.md` (the rules), `docs/linux-facts.md` (what the W1 spike *measured*
under Xvfb in the `ubuntu:26.04` image — cited below as "facts §n"), md.win's `docs/shell-design.md`
and `docs/app-api.md` (the WinUI design and the Logic API its packages published), md.win's
CHANGELOG and `docs/verify-1.6.md`, and md.macOS 1.6 (`README.md`, `CHANGELOG.md`, `md/*.swift`) —
the source of truth for behaviour.

**Conventions.** "Mac" = md.macOS 1.6. "md.win" = the Windows port's design and code. md-logic
types are named as md.win names them, in Rust spelling (`TextFileSession`,
`commands::CommandTable`, `text::TypingHooks` …); the md-logic port is authoritative for the exact
Rust signature, and §15 lists what this shell needs from it beyond md.win's API. GTK types are
`gtk4 0.11.5` (`gtk::`), `libadwaita 0.9.2` (`adw::`), `webkit6 0.6.1` (`webkit::`), `glib`/`gio
0.22.10`; every method named was checked against those crate sources (Appendix A). Chords are
written the GTK way (`Ctrl+1`, `Shift+Ctrl+S`) and given as accelerator strings where it matters
(`<Control>1`). Strings in quotes ship byte-identical from `md_logic::strings` or the command
table (U+2026 ellipsis, U+2014 em dash, U+00B7 middle dot, curly quotes as the Mac has them).

---

## 0. Decisions in one screen

| Topic | Decision | Why (one line) |
| --- | --- | --- |
| Process | One `adw::Application` `me.nettrash.md`, `HANDLES_OPEN`, single instance over D-Bus | Proven (facts §1); the NSDocument app shape |
| Windows | One `DocumentWindow` per document, one app-wide `BookWindow`; the app exits with its last window | The Mac's scenes; a windowless GTK process would be a ghost |
| Chrome | `adw::ToolbarView`: `adw::HeaderBar` (title + folder subtitle), then a `gtk::PopoverMenuBar` row; no toolbar on document windows | Proven (facts §1); GNOME has no global menu bar and CSD is mandatory on Wayland |
| Commands | md-logic's `CommandTable` builds one `gio::Menu` per window; one `gio::SimpleAction` per command (`app.` or `win.`); chords via `set_accels_for_action`; enablement/ticks from the window's `ShellSnapshot` | Proven: accels fire once from the editor or the preview (facts §1) |
| Chords | Ctrl for ⌘; Find family Ctrl+F / Ctrl+H / Ctrl+G / Shift+Ctrl+G / Ctrl+E; **Alt+Up / Alt+Down** for Previous / Next Article | GNOME binds Ctrl+Alt+Up/Down to workspaces (gsettings-desktop-schemas 50, checked) |
| Editor | `gtk::TextView` in a `gtk::ScrolledWindow`, Nimbus Mono PS 15 pt, accent caret, overlay placeholder "# Start writing…", tabs via `accepts_tab` | Typewriter face Ubuntu ships (facts §7); GTK CSS has `caret-color`, so the Mac's accent caret survives |
| Typing hooks | md-logic `TypingHooks` + `FormattingAdapter` fed at the buffer's **user-action boundaries** (snapshot at `begin-user-action`, judge at `end-user-action`, apply the capital / wrap on a `Priority::HIGH` idle as its own user action); Enter in a capture-phase key controller | GTK reports exact edits; the capital and the wrap stay separate undo steps as on every port |
| Find | md's own find & replace bar at the top of the editor column (Mac layout), in document **and** book windows | The Mac has it in both; md.win's book window lacked it |
| Preview origin | `md-asset://<surface>/index.html` from memory, `rich/` from `/usr/share/md/rich`, scheme registered once, secure + CORS, never `local` | Proven (facts §3) |
| Preview safety | Capture-phase link guard (user script) **plus** `Content-Security-Policy` on every `index.html`; `decide-policy` → `LinkPolicy` | `javascript:` never reaches `decide-policy` (facts §3) |
| Web storage | One ephemeral `webkit::NetworkSession` for every WebView | Nothing written to disk; PRIVACY lists less |
| Re-render | md-logic `PreviewCoordinator`: 350 ms debounce, `reload()` keeps scroll, first load / new document navigates | Proven (facts §2) |
| Scroll sync | The Mac's script **verbatim** (`window.webkit.messageHandlers.mdScroll`) | WebKitGTK has the same message-handler API (facts §4) |
| Exports | A fresh `WebView` per export, an invisible **mapped** overlay child of the requesting window; PDF via `webkit::PrintOperation` to "Print to File"; region snapshots at 2× via page zoom | Proven (facts §5, §6) |
| Print | Paper HTML in the overlay renderer → `PrintOperation::run_dialog`, A4 and 0.5 in preset | GTK's own dialog; no Done button needed |
| Share | **Omitted** (no Share submenu, no "Share Book as PDF") | Linux has no share sheet; md-logic has no share seam |
| Documents | `gtk::FileDialog`; md's codec decides encodings; autosave 1 s in place with the (mtime, size) guard; `gio::FileMonitor` (`WATCH_MOVES`) as `FileWatcher` | md.win's session verbatim; monitor events measured (facts §8) |
| Untitled drafts | **Autosaved** to `$XDG_DATA_HOME/md/drafts/`, kept on Quit and at logout, restored next launch; closing a window still asks | The Mac keeps drafts; Linux has a natural home for them (md.win had none) |
| Recent | `gtk::RecentManager` (shared XBEL), md's own 10 newest; Clear Menu hides, never deletes other apps' entries | System-wide recents feed GNOME Files; the Mac's 10 |
| Settings | `glib::KeyFile` at `$XDG_CONFIG_HOME/md/settings.conf` | In glib already, atomic save, readable; no new crate |
| Session | md-logic `SessionStore` JSON at `$XDG_STATE_HOME/md/session.json`; sizes, maximized, mode, Zen — **no positions** | GTK 4 has no window-position API and Wayland forbids it |
| Theme | Paper palette from `settings::Palette` as libadwaita CSS variables; follow the system light/dark | Proven (facts §7) |
| Fonts | Nimbus Mono PS on screen and paper (`<style id="md-linux-fonts">`), pure Core HTML in exports; `Depends: fonts-urw-base35, fonts-liberation` | Facts §7 proposal, adopted |
| Self-test | `md --selftest OUT` (cargo feature `selftest`, off in the `.deb`), headless in Docker with `xdotool` input | md.win's `--selftest`, plus a real keystroke tier Xvfb makes possible |

---

## 1. Application, windows, activation

### 1.1 `main` and the `adw::Application`

```rust
// app/src/main.rs. A --selftest run refuses to start unless XDG_CONFIG_HOME / XDG_STATE_HOME /
// XDG_DATA_HOME / XDG_CACHE_HOME all point into its output folder (scripts/dev.sh sets them),
// so a test never touches a real profile (§13.2).
let app = adw::Application::builder()
    .application_id("me.nettrash.md")
    .flags(gio::ApplicationFlags::HANDLES_OPEN)          // NON_UNIQUE for --selftest only
    .register_session(true)                              // `query-end` at logout (§1.5)
    .build();
app.add_main_option("new-window", glib::Char::from(b'n'), glib::OptionFlags::NONE,
                    glib::OptionArg::None, "Open a new document window", None);
app.add_main_option("version", glib::Char::from(0), glib::OptionFlags::NONE,
                    glib::OptionArg::None, "Print the version and exit", None);
app.connect_handle_local_options(..);   // --version → print "md 1.6.0", Break(ExitCode::SUCCESS);
                                        // --new-window → register(); if is_remote():
                                        //   activate_action("new", None) (forwarded over D-Bus),
                                        //   Break(SUCCESS); else Continue, and the first `activate`
                                        //   opens one untitled window (drafts still restored)
app.connect_startup(..);                // Shell::start (§1.7): services, CSS, actions, accels
app.connect_activate(..);               // §1.3
app.connect_open(..);                   // §1.3
app.connect_query_end(..);              // §1.5
app.run_with_args(&args)                // args after md's own flags; the rest are files
```

`glib::set_application_name("md")`; the program name is the binary's, `md`. GTK 4 uses the
application id as the Wayland `app_id` / X11 `WM_CLASS`, which is what ties the windows to
`me.nettrash.md.desktop` in the dock (docs/packaging.md §2).

### 1.2 Single instance

`HANDLES_OPEN` + the session bus make the first process the primary; a second `md file.md` hands
`file.md` to the primary's `open` over D-Bus and exits (measured: 20 ms, facts §1). Without a
session bus every launch is its own primary (Docker-only; `scripts/dev.sh` runs a private bus).

On Wayland a window can only take focus with an activation token. GTK forwards the launcher's
`XDG_ACTIVATION_TOKEN` through the `GApplication` platform data, so `present()` on the window the
hand-off opened or found is allowed to raise it. *Unverified* — the spike ran X11 only (§12).

### 1.3 Activation routing

md-logic's `activation::ActivationRouter::route(&ActivationDescription) -> Vec<ActivationAction>`
decides; the shell describes what happened and executes the actions (`WindowManager`, §1.7).

| GTK signal | Description passed | Actions (md-logic) | Shell |
| --- | --- | --- | --- |
| `activate`, first in this process | `ForLaunch(is_first: true, has_restorable_session)` | `RestoreSession` when `session.json` names saved documents that exist **or** drafts; else `OpenUntitled` | §1.6 |
| `activate`, from a second launch | `ForLaunch(is_first: false)` | — | **Present the most recently active window** (the Mac: launching a running app activates it; md.win opened a new untitled window instead) |
| `app.new` from `--new-window` / the `.desktop` action | — | — | `OpenUntitled` |
| `open(files)` | `ForFiles(items, is_first)` — each `gio::File` → `ActivationItem::File(path)` or `::Folder(path)` (`query_file_type`); a file with no local path (`File::path() == None`) → alert `strings::documents::COULD_NOT_OPEN` | per item: `.textpack` → `ImportTextPack`; a folder named `*.textbundle` → `ImportTextBundleFolder`; other folders ignored; else `OpenPath` | A path already open (`WindowRegistry::find_by_path`, exact compare — Linux paths are case-sensitive) → `Focus(id)` → present that window |
| `open` as the **first** activation | as above | as above, plus **drafts only** are restored (not saved documents) | Unsaved text is never left behind because the user double-clicked another file |

The same untitled example opened twice gives two windows (Mac). `ActivationRouter::resolve`
performs the "already open → focus" substitution.

### 1.4 Windows

| | `DocumentWindow` | `BookWindow` |
| --- | --- | --- |
| Type | `adw::ApplicationWindow` | `adw::ApplicationWindow` |
| Content | `adw::ToolbarView`: top bars `adw::HeaderBar` + `gtk::PopoverMenuBar`; content `gtk::Overlay` (export renderers, Zen capsule) → `gtk::Box` (vertical): `ArticlePanes` in its `ZenBin` (§5.2, §5.3) · bar slot (conflict / save error, §6.3) · footer (§5.4) | `adw::ToolbarView`: header bar (with the detail toolbar, §8.4) + menu bar; content `gtk::Overlay` → `gtk::Paned` (sidebar, detail) |
| Default size | **900 × 640** (`set_default_size`), last closed size from `md.linux.windowSize.document` | **1000 × 700**, `md.linux.windowSize.book` |
| Minimum | **480 × 320** (`set_size_request`, non-Zen) | 700 × 400 with a book, 260 × 320 empty |
| Count | one per document | one, created on demand by Show Book |
| Title (`gtk::Window::set_title`, also `adw::WindowTitle` title) | `windows::WindowTitle::for(name, is_dirty)` — `"name"` / `"name — Edited"` | `WindowTitle::for_book(book_name, article_title)` |
| Header subtitle | the containing folder, `$HOME` written `~` (hidden for untitled) — the Mac shows it in the title-bar proxy menu | the book folder |

**Placement.** GTK 4 has no API to position a toplevel (no `gtk_window_move`) and Wayland forbids it,
so windows are placed by the compositor: no cascade, no restored positions. `session.json` keeps
width, height and maximized; its `x`/`y` are written `0` (md-logic's `WindowRect` stays as is).

**Full screen.** View ▸ Enter Full Screen (`F11`) → `gtk::Window::fullscreen()` /
`unfullscreen()`; `connect_fullscreened_notify` keeps the row's title ("Enter Full Screen" / "Exit
Full Screen", `CommandTable::display_title`) and Zen (§5.3) in step. On Wayland the state changes
asynchronously; the notify is the only truth.

### 1.5 Closing and quitting — three routes, one policy

1. **A window closes** (close button, Ctrl+W, Alt+F4): `connect_close_request` returns
   `glib::Propagation::Stop` and runs the window's close policy asynchronously; on success it calls
   `destroy()` (never `close()` again, which would re-enter). Policy (md.win §1.4, the Mac's):
   - an untitled **dirty** document → `Alerts::ask_save_changes(title)`
     (`strings::documents::save_changes_question`): **Save** → Save As (§6.4), cancel there aborts the
     close; **Don't Save** → the draft is deleted; **Cancel** → stays open;
   - a saved document → `TextFileSession::close_flush()`; a failed flush writes the rescue copy
     (`RescueCopy`) and alerts `rescue_failed_title` / `rescue_kept_message`, then closes anyway;
   - an export or print in flight (`track_output`, §7.1) is awaited, bounded at **10 s**.
   The window then unregisters (`WindowRegistry::remove`), stops its monitor, drops its WebViews and
   rewrites `session.json`. When the last window is gone `GtkApplication` ends `run()` — the app exits.
2. **Quit** (File ▸ Quit, Ctrl+Q) → `WindowManager::quit()`: **no prompts**. Saved documents are
   flushed (rescue on failure, as above); every untitled dirty document is written to its draft
   (§1.6); the Book window flushes its article (`BookFlushGate`); outputs are awaited (10 s);
   `session.json` is written with every window, drafts included; then every window is destroyed and
   `app.quit()`. This is ⌘Q on the Mac with autosave: nothing is asked, nothing is lost, the next
   launch brings it all back.
3. **Session end** (logout, shutdown): `query-end` runs route 2 **synchronously** (no awaits: flushes
   and draft writes are synchronous file writes; outputs are abandoned). There is no SIGTERM handler
   (it would need a `libc`/`glib-unix` dependency and `unsafe`); the 1 s autosave bounds the loss.

Deactivation (`connect_is_active_notify` → false) flushes the saved document and the draft, as
md.win and the Mac do.

### 1.6 Session restore and drafts

**`session.json`** — `$XDG_STATE_HOME/md/session.json` (`glib::user_state_dir()`, default
`~/.local/state/md/`), md-logic `windows::SessionStore` (version 1, md.win's schema). XDG puts "the
current state of the application that can be reused on a restart (view, layout, open files)" in
STATE, which is exactly this. Written on every window close, on Quit and at `query-end`; read on the
first activation. Each `SessionWindow` carries path, mode, Zen, Zen reading, `WindowRect(0, 0, w, h)`
and maximized; `SessionBook` the Book window's open flag and size. A missing file is skipped.

**Drafts** — `$XDG_DATA_HOME/md/drafts/` (`glib::user_data_dir()`, default `~/.local/share/md/drafts/`,
created `0700`). DATA, not STATE: a draft is the user's text and must not be swept with caches.

- Every untitled session whose text is dirty gets a draft file `<uuid>.md` (`glib::uuid_string_random()`),
  written in place (mode `0600`) 1 s after the last keystroke — the same scheduler and timing as
  autosave — and on deactivation.
- A draft is deleted when its document is saved (Save / Save As) and when its window closes with
  **Don't Save**. An untitled document that was edited stays dirty even when emptied (the Mac's
  rule), so it keeps its draft until one of those happens.
- `session.json` lists a draft as a `SessionWindow` whose `path` lies inside the drafts folder. On
  restore the shell recognises the folder, reads the text, opens it with
  `TextFileSession::open_untitled(text, None, dirty: true)` (title "Untitled", "Untitled 2", … from
  `UntitledNames`) and keeps writing to the same draft file.
- A draft file that no `session.json` names (a crash between writes) is restored too, after the named
  ones, so a crash never strands text.

This is the Mac's "untitled documents are autosaved and reopened" without a Versions browser.

### 1.7 Shell services (created in `startup`)

`Shell` (one per process, `Rc`, reachable from every window) owns:

| Service | Type | Implements / wraps |
| --- | --- | --- |
| settings | `KeyFileSettingsStore` | `settings::SettingsStore` (§9) |
| scheduler / clock / UI thread / spawner | `GlibScheduler`, `SystemClock`, `GlibUiThread`, `GlibSpawner` | `seams::{Scheduler, Clock, UiThread}`, `exec::Spawner` over `glib::timeout_add_local_once`, `glib::idle_add_local_once`, `glib::MainContext::default().spawn_local` |
| files / identity | `StdFileSystem`, `CanonicalFileIdentity` | `seams::{FileSystem, FileIdentity}` (`std::fs`; `std::fs::canonicalize` with md-core's lexical fallback) |
| registry | md-logic `windows::WindowRegistry` | `seams::DocumentRegistry` |
| windows | `WindowManager` | live windows by `WindowId`, open / focus / quit, session and drafts |
| recent | `RecentFiles` | `gtk::RecentManager` (§6.6) |
| examples | `ExampleLibrary` | `/usr/share/md/examples` (§2.3) |
| web | `WebServices` | the one `webkit::WebContext`, the ephemeral `webkit::NetworkSession`, the `md-asset:` handler (§4.1) |
| words | md-logic `text::SimpleWordCounter` | `seams::WordCounter` (md-core `book::writing_stats::words`) |
| book | `BookLibraryHost` | §8.2 |
| paths | `Paths` | install and XDG directories (§9.3) |

---

## 2. The command surface

### 2.1 Structure

Every window carries the **same eight menus**: `File · Edit · Format · View · Book · Go · Window ·
Help` — the Mac's bar minus the `md` app menu (About moves to Help, Quit to File). The bar is a
`gtk::PopoverMenuBar::from_model(Some(&menu))` built per window from md-logic's
`commands::CommandTable::menus()` (`MenuNode` tree): a node → `gio::MenuItem`; a change of
`CommandSpec.group` between siblings → a new `gio::Menu` section (GTK draws sections as dividers);
submenus → `append_submenu`. Mnemonics: the eight menu titles carry one (`_File`, `_Edit`,
`F_ormat`, `_View`, `_Book`, `_Go`, `_Window`, `_Help` — the spike's spelling); item titles are the
table's verbatim. F10 opens the bar (GTK's own binding).

Enablement and ticks come from **the window's own** `commands::ShellSnapshot` through
`CommandEnablement::is_enabled / is_checked / is_submenu_enabled`. A disabled row is a disabled
`gio::SimpleAction` (`set_enabled(false)`), which also disables its accelerator, so the key falls
through to the focused widget (md.win's `TryInvoke` → false).

### 2.2 Actions and accelerators

**One `gio::SimpleAction` per `CommandId`**, kebab-case name (`ExportLaTeX` → `export-latex`),
in one of two scopes:

- `app.` — commands whose state is app-wide: `new`, `open`, `open-recent(s)`, `clear-recent`,
  `open-text-bundle-folder`, `example(s)`, `example-book`, `quit`, the three typing toggles
  (stateful `b`), `pdf-page-size` (stateful `s`), `new-book`, `open-book`, `show-book`, `close-book`,
  `print-book`, `export-book-pdf/-epub/-latex`, `help`, `privacy-policy`, `about`.
- `win.` — everything that acts on a window: File's Close … Export rows, Edit, Format, View, Go,
  Window. Radio groups: `win.view-mode` (stateful `s`, targets `edit` / `split` / `preview` for
  `ViewEdit/Split/Preview`), `win.zen` and `win.show-sidebar` (stateful `b`), `win.activate-window`
  (stateful `t`, each window's state its own id, so each window's Window menu ticks itself).
  Parametrised rows: `win.contents(u)` / `win.notes(u)` (index into the snapshot's outline / notes),
  `win.export-diagram-svg(u)` (the diagram's ordinal; the pipeline re-resolves it from the current
  text, md.win's rule).

Activation runs md-logic's `commands::CommandDispatcher::execute(id, argument)` for that window —
one dispatcher per window, handlers registered by the window (md.win's wiring order: build the menu,
install actions, register handlers, then `refresh(snapshot)`). The dispatcher's 150 ms double-fire
guard is harmless here: WebKitGTK does not double-fire (facts §1).

Accelerators: `app.set_accels_for_action(detailed_name, &[...])`, once, in `startup`, from
`CommandSpec.chord` through `menus::accels::gtk_accelerator(&Chord) -> String` (§2.9). Detailed
names for the radios (`win.view-mode::edit`) — proven (facts §1). GTK then shows the chord beside the
row by itself.

**Editing chords.** Undo, Redo, Cut, Copy, Paste and Select All are registered too, and their
handlers **dispatch to the focused widget**: the editor → `EditorPane` (§3); a `gtk::Text` (the find
fields, a dialog entry) → `widget.activate_action("text.undo" / "clipboard.copy" / …)`; the preview
→ `webkit::WebView::execute_editing_command("Copy" / "SelectAll")`. So Ctrl+C copies a preview
selection in Split exactly as before, and the menu rows still show their chords. (GTK 4.22's
`GtkPopoverMenu` documents no attribute to *display* a chord that is not an accelerator, so md.win's
"shown but not registered" cannot be copied; dispatching is the robust equivalent.) Delete has no
chord on Linux — registering the Delete key would take it from every entry.

**While a dialog is up.** `adw::AlertDialog` is presented inside the window, so window shortcuts
would still reach the window behind it. The snapshot's `dialog_open` (a Linux field, §15) is true
while the window has a visible dialog (an alert, About), `AdwAlerts` is showing one or `GtkPickers`
has a file dialog up (their `open_changed` events republish the snapshot), and every `win.` action
is disabled until it answers — the Mac's sheet blocks the window's menu items the same way.

**Escape** is not an accelerator (it would take Escape from input methods and popovers). A window
`gtk::EventControllerKey` in the **bubble** phase exits Zen when nothing below handled the key; the
find fields close the find bar through `gtk::SearchEntry`'s `stop-search`.

### 2.3 Dynamic submenus

Open Recent, Examples, Diagram as SVG, PDF Page Size, Contents, Notes and the Window list are
`gio::Menu`s the controller keeps; `MenuController::refresh(&ShellSnapshot)` compares the snapshot
with the last one (md-logic's `ShellSnapshot` has structural equality) and, only for a list that
changed, `remove_all()` + re-append. `GtkPopoverMenuBar` follows `items-changed` on its own. A
rebuild while that submenu is open may close it — acceptable, and it only happens when the list
really changed (the derived text arrives on a 250 ms tick, §5.5). "Enter / Exit Full Screen" is a
one-item section rebuilt when `is_full_screen` changes.

**Examples** (`ExampleLibrary`): `/usr/share/md/examples/*.md` (top level only), named and sorted by
md-core `document::example_library::from_listing` (Welcome, Formatting, Tables, Code, Images, Math,
Diagrams, Plots, Writer Tools), then a section with **Example Book…**. An example opens as a new
untitled, **dirty** window holding its text (title "Untitled — Edited"; Split by the open rule).

### 2.4 File

| Item | Chord | Enabled | Action |
| --- | --- | --- | --- |
| New | Ctrl+N | always | `WindowManager::open_untitled()` (mode Edit) |
| Open… | Ctrl+O | always | `gtk::FileDialog::open_multiple_future` (§6.1) → one window per file |
| Open Recent ▸ | — | entries exist | md's ten newest (§6.6), divider, **Clear Menu** |
| Open TextBundle Folder… | — | always | `select_folder_future` → import (§6.5) |
| Examples ▸ | — | always | nine rows, divider, **Example Book…** |
| Close | Ctrl+W | always | §1.5 route 1 |
| Save | Ctrl+S | `doc` | untitled → Save As; else `flush_now(explicit: true)` |
| Save As… | Shift+Ctrl+S | `doc` and not a book article | §6.4 |
| Duplicate | — | `doc` | new untitled window, same text, title `strings::duplicate_title`, dirty |
| Rename… | — | `saved` | §6.4 |
| Move To… | — | `saved` | §6.4 |
| Revert to Saved | — | `saved && dirty` | `TextFileSession::revert_to_saved()` |
| Print… | Ctrl+P | `doc` | §7.2 |
| Export ▸ PDF… / HTML… / EPUB… / LaTeX… / TextBundle… | — | `doc` | §7 |
| Export ▸ Diagram as SVG ▸ | — | diagrams exist | one row per `DiagramSvg` diagram, title `menu_title` |
| Export ▸ PDF Page Size ▸ | — | always | seven radios, `app.pdf-page-size`, ticked from `md.pdfPageSize` |
| Quit | Ctrl+Q | always | §1.5 route 2 |

There is no Share submenu (§14 row 10). "Export ▸" is never disabled (PDF Page Size inside it never
is), as on the Mac.

### 2.5 Edit

| Item | Chord | Enabled | Action |
| --- | --- | --- | --- |
| Undo / Redo | Ctrl+Z / Shift+Ctrl+Z (also Ctrl+Y) | editor visible && `can_undo` / `can_redo` | focused-widget dispatch (§2.2); the editor's path is `EditorPane::undo()` / `redo()` |
| Cut / Copy / Delete | Ctrl+X / Ctrl+C / — | editor visible && selection | dispatch; Delete = `delete_selection` |
| Paste / Select All | Ctrl+V / Ctrl+A | editor visible | dispatch |
| Find ▸ Find… | Ctrl+F | an editing surface | opens the find bar, query focused (§3.6); in Preview / Zen reading it first nudges the editor on screen (the Mac's `showEditor`) |
| Find ▸ Find and Replace… | Ctrl+H | an editing surface | the bar with the replace row, replacement focused |
| Find ▸ Find Next / Find Previous | Ctrl+G / Shift+Ctrl+G | the bar has a query | next / previous hit, selected in the editor |
| Find ▸ Use Selection for Find | Ctrl+E | a selection in the editor | seeds the query |
| Typing ▸ Continue Lists and Tables | — | always | toggles `md.continueLists` |
| Typing ▸ Capitalize Sentences | — | always | toggles `md.capitalizeSentences` |
| Typing ▸ Wrap Selection When Typing * _ ` $ | — | always | toggles `md.wrapSelection` |

The Find rows sit in an **Edit ▸ Find** submenu with the Mac's titles (md.win flattened them and used
F3). The nudge is the Mac's 1.5 behaviour: ⌘F in a preview-only window brings the source back as a
navigation nudge, never as a stored mode.

### 2.6 Format

docs/formatting.md is the specification; md-core `editing::markdown_formatting` the rules; md-logic
`text::FormattingAdapter` the command → op map. Every row is enabled while an editor with a document
is on screen.

| Item | Chord | | Item | Chord |
| --- | --- | --- | --- | --- |
| Bold | Ctrl+B | | Heading ▸ Heading 1 … 6 | Ctrl+Alt+1 … 6 |
| Italic | Ctrl+I | | Bullet List | Shift+Ctrl+L |
| Code | Shift+Ctrl+C | | Task List | Shift+Ctrl+T |
| Strikethrough | Shift+Ctrl+X | | Quote | Shift+Ctrl+Q |
| Link | Ctrl+K | | Indent / Outdent | Ctrl+] / Ctrl+[ |

No AltGr guard is needed: GTK reports AltGr as `ISO_Level3_Shift` (a level, not the Alt modifier),
so German AltGr+2 types "²" and never matches `<Control><Alt>2`. To be confirmed on a real desktop
with a German layout (§13.4).

### 2.7 View

| Item | Chord | Enabled | Behaviour |
| --- | --- | --- | --- |
| Edit / Split / Preview | Ctrl+1 / Ctrl+2 / Ctrl+3 | `viewMode` | `ViewModeController::select(mode)`; radio `win.view-mode` |
| Zen Mode | Shift+Ctrl+Return | document windows | `ZenController::toggle()`; disabled in the Book window |
| Show Sidebar | — | Book window | toggles the sidebar (§8.1) |
| Enter / Exit Full Screen | F11 | always | §1.4 |

In Zen the published mode is `zen_reading ? Preview : Edit`; Ctrl+1 and Ctrl+2 both mean write,
Ctrl+3 read; Split is never ticked in Zen (`ShellSnapshot::published_mode`).

### 2.8 Book, Go, Window, Help

**Book**: New Book…, Open Book…, Show Book (Shift+Ctrl+B), Close Book (`book`) · Print Book…
(`book`) · Export Book ▸ PDF… / EPUB… / LaTeX… (`book`). No "Share Book as PDF".

**Go**: Previous Article (**Alt+Up**, also `<Control><Alt>Up`), Next Article (**Alt+Down**, also
`<Control><Alt>Down`) — enabled by the Book window's stepper · Contents ▸ (row title
`CommandTable::contents_row_title`: two spaces per level below 1) · Notes ▸ (`NotePreview::of`).
Ctrl+Alt+Up/Down stay registered second for desktops that leave them free (KDE, Xfce); GNOME grabs
them for workspaces before the app sees them (`switch-to-workspace-up/down`, checked in the 26.04
schema), so the first, displayed chord is Alt+Up/Down. GtkTextView binds no Alt+arrow.

**Window**: Minimize (`gtk::Window::minimize`), Zoom (`maximize` / `unmaximize` toggle), divider,
one radio row per open window (registry titles + "Book"); activation presents that window.

**Help**: md Help (F1) → `gtk::UriLauncher::new(strings::help::SUPPORT_URL).launch(..)`; Privacy
Policy → `PRIVACY_URL`; divider; **About md** → `adw::AboutDialog`: application name "md", icon
`me.nettrash.md`, version `env!("CARGO_PKG_VERSION")` ("1.6.0"), developer "nettrash", copyright
`strings::help::COPYRIGHT`, `gtk::License::MitX11`, website `SUPPORT_URL`, comments =
`ENGINES_HEADING` + the five `ENGINES` lines. Never "no third-party dependencies"; no legal sections
for the engines until nettrash decides the notices question (docs/packaging.md §6).

### 2.9 The Linux command table

What md-logic's `CommandTable` must say on Linux (md.win's table with these differences; titles
verbatim; the accelerator string is what `gtk_accelerator` produces):

| Id | Title | GTK accels (first is displayed) | Note |
| --- | --- | --- | --- |
| `Exit` | **"Quit"** | `<Control>q` | GNOME's word and chord; md.win "Exit" |
| `OpenTextBundleFolder` | "Open TextBundle Folder…" | — | Linux keeps md.win's row |
| `ShareSource`, `ShareRenderedPdf`, `ShareBookPdf` | — | — | **absent** on Linux |
| `SaveAs` | "Save As…" | `<Shift><Control>s` | |
| `Redo` | "Redo" | `<Shift><Control>z`, `<Control>y` | GNOME shows Shift+Ctrl+Z |
| `Undo` `Cut` `Copy` `Paste` `SelectAll` | as md.win | `<Control>z` `x` `c` `v` `a` | registered (§2.2) |
| `Delete` | "Delete" | — | no chord on Linux |
| `Find` | "Find…" | `<Control>f` | path Edit ▸ Find |
| `Replace` | **"Find and Replace…"** | `<Control>h` | Mac title |
| `FindNext` / `FindPrevious` | "Find Next" / "Find Previous" | `<Control>g` / `<Shift><Control>g` | Mac chords; md.win F3 |
| `UseSelectionForFind` | "Use Selection for Find" | `<Control>e` | |
| `InlineCode` `Strikethrough` | "Code" "Strikethrough" | `<Shift><Control>c` `<Shift><Control>x` | |
| `Heading1…6` | "Heading 1…6" | `<Control><Alt>1…6` | |
| `BulletList` `TaskList` `Quote` | | `<Shift><Control>l` `t` `q` | |
| `Indent` / `Outdent` | | `<Control>bracketright` / `<Control>bracketleft` | |
| `ViewEdit/Split/Preview` | | `<Control>1/2/3` | detailed `win.view-mode::…` |
| `ZenMode` | "Zen Mode" | `<Shift><Control>Return` | |
| `FullScreen` | "Enter Full Screen" | `F11` | |
| `ShowBook` | "Show Book" | `<Shift><Control>b` | |
| `PreviousArticle` / `NextArticle` | | `<Alt>Up`, `<Control><Alt>Up` / `<Alt>Down`, `<Control><Alt>Down` | §2.8 |
| `Help` | "md Help" | `F1` | |
| `Escape` | — | — | not an accelerator on Linux (§2.2) |

`Chord::display_text` stays md.win's for the tooltips that name chords
(`strings::books::PREVIOUS_ARTICLE_TOOLTIP`, `EMPTY_MESSAGE`), which must say **Alt+Up / Alt+Down**
(§15).

---

## 3. The editor (`editor::EditorPane`)

### 3.1 Configuration

| Mac (`NSTextView`) | GTK |
| --- | --- |
| plain text | `gtk::TextView` over its own `gtk::TextBuffer`, inside `gtk::ScrolledWindow` (`hscrollbar_policy(Never)`) |
| undo with the document's manager | the buffer's built-in history (`enable_undo` true, default); grouping = user actions (§3.3); history cleared by `begin_irreversible_action()` / `end_irreversible_action()` |
| American Typewriter 15 pt | CSS `textview.md-editor { font-family: "Nimbus Mono PS", "Liberation Mono", monospace; font-size: 15pt; }` — Pango resolved it to Nimbus Mono PS 15 (20 px), facts §7 |
| ink text, **accent caret** | `textview.md-editor text { color: ink; caret-color: accent; background: transparent }`; selection `text selection { background-color: alpha(accent, .35) }` — painted also when the view is unfocused, so a Replace hit stays visible behind the find bar (md.win had to set a second brush for this) |
| no smart quotes / dashes / spelling | GTK has none on a `TextView`; `input_hints(InputHints::NO_SPELLCHECK)` keeps on-screen keyboards and IMEs from correcting; never `UPPERCASE_SENTENCES` — md owns capitalisation (smart-typing.md §3.2); `input_purpose(Free)` |
| inset 16 × 16 | `top/bottom/left/right_margin(16)` |
| word wrap, vertical scroll | `wrap_mode(WrapMode::WordChar)` |
| placeholder `"# Start writing…"` at the inset | GTK 4.22's `TextView` has no placeholder: a `gtk::Label` (`strings::EDITOR_PLACEHOLDER`, editor font, ink 40 %, `can_target(false)`, `halign/valign = Start`, margins 16) in a `gtk::Overlay` over the scrolled window, visible while the buffer is empty — the Mac draws an overlay too |
| Tab inserts a tab | `accepts_tab(true)` (default) |
| context menu | GTK's own (Cut, Copy, Paste, Delete, Select All, Insert Emoji) — harmless; `set_extra_menu` adds nothing |

`EditorPane` public surface (the window and the book pane drive it): `widget()`, `text() -> String`,
`set_text(&str)` (§3.3), `reset_history()`, `selection_utf16() -> (start, len)`,
`select_utf16(start, len)`, `has_selection()`, `can_undo()`, `can_redo()`, `undo()`, `redo()`,
`cut()`, `copy()`, `paste()`, `delete_selection()`, `select_all()`, `apply_format(CommandId)`,
`apply_jump(EditorJump)`, `replace_range_utf16(start, len, text)` (find & replace), `focus()`,
`use_settings(Option<Rc<dyn SettingsStore>>)`, events `text_edited: Event<str>` (the LF text after
every change), `selection_changed: Event<()>`, `history_changed: Event<()>` (can-undo/redo notifies),
`scrolled: Event<f64>` (§4.7).

### 3.2 Text, line endings and offsets

The buffer holds **LF text only**, which is what `TextFileSession` holds (the codec's dressing keeps
the file's own newlines, md.win §3.2). Any insertion containing `\r` (a paste from a CRLF source) is
intercepted in `insert-text`: emission stopped, the text re-inserted with `EditorText::from_text_box`
normalisation under a re-entrancy flag. The buffer's text and the session's are therefore the same
string, and `text_edited` hands the buffer text to `session.edit(..)` only when it differs (the Mac's
`sync()` guard).

`GtkTextIter` offsets count **Unicode scalars**; md-core / md-logic editing APIs take **UTF-16
units**. `editor::offsets` converts with md-core `text::utf16::{utf16_from_char_offset,
char_offset_from_utf16}` over the buffer text read once per event. Pure, unit-tested (surrogate
pairs, combining marks, CJK, emoji).

### 3.3 Typing — SmartTyping, the override, the wrap (docs/smart-typing.md §3, docs/formatting.md §4)

The pure functions are md-core's (`editing::smart_typing`, `editing::markdown_formatting`, vector-
pinned); the state that sequences them is md-logic `text::TypingHooks` (the tracked capital and the
override of smart-typing.md §3.4, the paste / history / composition / applying / replacing flags) and
`text::FormattingAdapter` (the wrap plan). `EditorPane` holds no typing state of its own and feeds
them exactly what md.win's `EditorPane` feeds them, taken at GTK's natural boundaries:

| Hook | GTK signal / controller | What the pane does |
| --- | --- | --- |
| **Before a change** | buffer `begin-user-action` (outermost only) | snapshot `old_text` and the UTF-16 selection |
| **Deletion side** | buffer `delete-range` inside a user action, collapsed selection | `announce_deletion(Before)` when the range ends at the caret, `(After)` when it starts there — exact, where md.win had to guess from the key |
| **The change** | buffer `end-user-action` (outermost) | `new_text` = buffer text; unless composing / pasting / history / own edit: `wrap = FormattingAdapter::wrap(old, sel_start, sel_len, new)` → `after_insertion(wrap)` kept as pending (when `md.wrapSelection` is on); `hooks.before_text_changing(old, sel_start, sel_len, new)`; `plan = hooks.text_changing(new, caret, 0)` |
| **Apply** | `glib::idle_add_local_full(Priority::HIGH, ..)` — runs before the next input event is dispatched | `FormattingAdapter::still_applies(wrap, ..)` → apply the wrap; `hooks.still_applies(plan, ..)` → apply the capital; each **inside its own** `begin_user_action` / `end_user_action` under `hooks.applying(..)` |
| **Enter** | `gtk::EventControllerKey`, `PropagationPhase::Capture`, on the `TextView`: `Return` / `KP_Enter` / `ISO_Enter`, no Shift/Ctrl/Alt, no preedit | `hooks.enter(text, sel)` → `Some(e)`: one user action under `applying` (delete `[location, location+length)`, insert `replacement`, cursor to `caret`), `Propagation::Stop`; `None` → `Proceed` (GTK inserts `"\n"`, tracked like any edit) |
| **Shift+Enter, Ctrl+Enter** | same controller | insert a plain `"\n"` ourselves (`insert_interactive_at_cursor`) in one user action, `Stop` — never anything else (the Mac's Control-Return parity) |
| **Undo / Redo** | buffer `undo` / `redo` (before the default handler) and `connect_after` | before: snapshot, `announce_history()`; after: `before_text_changing` + `text_changing` (tracked, never judged), `caret_after_history` → place the caret, `settle_history()` on the next idle |
| **Paste** | `TextView::connect_paste_clipboard` (Ctrl+V, Shift+Insert, the context menu) and `EditorPane::paste()` | `announce_paste()`; the async insertion arrives as a user action and consumes it; buffer `paste-done` → `settle_paste()` |
| **Composition** | `TextView::connect_preedit_changed` | non-empty preedit → `composition_started()`; empty → `composition_ended()`. The preedit is drawn, never in the buffer. The committed text arrives after the preedit clears and is judged like typing, so a dead-key `é` at a sentence start becomes `É` (the Mac's rule; md.win skipped compositions) |
| **Settings** | `use_settings(store)`; `SettingsStore::changed` | `hooks.configure(TypingSettings::read(store))`, wrap = `wrap_selection_setting::read(store)`; live in every window |

Why boundaries and not per-signal hooks: GTK's typing over a selection is *two* buffer edits
(`delete_selection` then `insert_interactive_at_cursor`), and the retype rule ("a lowercase letter
typed over its own capital stays lowercase") and the wrap both need them as one replacement —
exactly what md.win's `BeforeTextChanging` sees. Reading the text twice per keystroke is md.win's
cost too (`TextBox.Text`).

**The bridge.** Over a selection longer than one scalar GTK 4.22 even puts the two edits in *two*
user actions (`gtk_text_view_commit_text`: "so that there are two undo operations"), which would
make the typed replacement two undo steps where the Mac and md.win have one — undoing a wrap would
take three Ctrl+Z instead of two. So a user action that removed exactly its selection is held, and
the pane begins an outermost user action of its own **inside** that action's `end-user-action`
handler — before `GtkTextBuffer` hands the end to GtkTextHistory, whose group therefore stays open.
GTK's insertion lands in it as an inner action; the next `Priority::HIGH` idle (before any further
input) ends it. One history group, one undo step, one replacement for the hooks; a deletion nothing
follows (Delete or Cut over a selection) closes the same way on its own. The self-test's wrap row
proves Ctrl+Z ×2 back to the selected word and Shift+Ctrl+Z ×2 forward again.

**Undo units** (smart-typing.md §3.5, formatting.md §4): the typed letter is GTK's typing step (a
typed replacement of a selection one step, through the bridge above); the capital (or the wrap) is
a separate group of a delete and an insert, so one Ctrl+Z restores `m` from `M` (or `*` from
`*word*`), a second removes it (or brings `word` back, selected). That GtkTextHistory never merges the group into the
preceding typing run is asserted by the self-test's keystroke tier (§13.3), not assumed.

**External text** (open, Revert, Reload from Disk, an example, an article switch):
`EditorPane::set_text(text)` asks `hooks.external_text(current, text)`; on `true` it assigns under
`hooks.replacing(..)` inside `begin_irreversible_action()` / `end_irreversible_action()` (clears the
undo history — the Mac's fresh `UndoManager`), then restores the selection clamped
(`EditorText::clamp_selection`). On the session's `undo_generation` change the window calls
`reset_history()` = `hooks.reset()` + an empty irreversible action (md.win §3.6: never the control's
reset alone).

**Format commands** (Format menu, chords): `FormattingAdapter::edit(text, sel_start, sel_len, op)`
→ applied under `hooks.external_edit(..)` as one user action, the selection the edit names, focus
back in the editor (md.win `ApplyFormat`).

### 3.4 Caret jumps

`EditorJump(id, line)` is performed once per id (`EditorJumpTracker::claim`): `LineOffsets::
offset_of_line(line, text)` (UTF-16) → char offset → `buffer.place_cursor(&iter)`,
`text_view.scroll_to_mark(&buffer.get_insert(), 0.0, true, 0.0, 0.3)` (GTK defers it until the
lines are laid out, which covers a Notes jump that just switched Preview → Edit), `grab_focus()`,
then `jump_handled(id)`.

### 3.5 Editor half of the scroll sync

The scrolled window's `vadjustment().connect_value_changed`: unless `ScrollSyncGuard::is_applying`,
`fraction = value / (upper − page_size)` (when positive) → `ScrollSync::editor_did_scroll`. Applying
a remote fraction: `let _g = guard.applying(); adj.set_value(f * (upper − page_size))`. GTK's
adjustment notifies synchronously, so the guard's plain flag suffices (window `Duration::ZERO`).

### 3.6 Find and Replace bar (`editor::FindBar`)

A bar across the **top of the editor column**, revealed with a `gtk::Revealer` (slide-down) — where
the Mac's `NSTextFinder` puts it, and where GNOME's own editors put theirs:

- row 1: a `gtk::ToggleButton` (`pan-end-symbolic` / `pan-down-symbolic`, tooltip
  `strings::find::TOGGLE_REPLACE`) for row 2, the query `gtk::SearchEntry` (placeholder "Find",
  `set_search_delay(0)`), the tally label (`find::tally`), Previous / Next buttons
  (`go-up-symbolic` / `go-down-symbolic`, tooltips "Previous" / "Next"), **Done**;
- row 2: the replacement `gtk::Entry` (placeholder "Replace with"), **Replace**, **Replace All**.

Ctrl+F opens it with row 2 hidden and the query focused; Ctrl+H with row 2 shown and the replacement
focused; both seed the query from a non-empty selection. Behaviour is md.win §3.5 verbatim — find as
you type from the anchor (`FindAsYouType`), Enter / Shift+Enter in the query = next / previous with
focus kept (`SearchEntry` `activate`, `next-match` / `previous-match`), Ctrl+G from the editor selects
and keeps focus in the editor, Enter in the replacement = Replace (focus stays), Shift+Enter =
Replace All (focus back to the editor with the rewritten span selected), Escape (`stop-search`) or
Done closes and returns focus. Every decision is md-logic `documents::TextSearch` over the buffer
text with UTF-16 offsets: `next`, `previous`, `selection_is_match`, `replace` → `(apply,
search_from)`, `replace_all` → one span. Edits go through `EditorPane::replace_range_utf16` under
`hooks.external_edit` as **one** user action, so Replace All is one undo step and the session sees it
like a keystroke (autosave, dirty, clobber guard, word count). No regular expressions.

The bar belongs to `ArticlePanes`, so the **Book window has it too** (the Mac's 1.5; md.win's book
window had none). It collapses with the editor (Preview, Zen reading); Find Next / Previous are
greyed there.

---

## 4. The preview host (`web::PreviewHost` around `webkit::WebView`)

### 4.1 `WebServices` — one per process

- `webkit::WebContext::default()` (the global context, facts §0) with
  `register_uri_scheme("md-asset", ..)` **once** and `security_manager()` →
  `register_uri_scheme_as_secure("md-asset")` + `register_uri_scheme_as_cors_enabled("md-asset")`;
  never `_as_local` (breaks module scripts — facts §3; registrations leak within a process).
- `webkit::NetworkSession::new_ephemeral()`, shared by every WebView (`WebView::builder()
  .web_context(&ctx).network_session(&session)`): cookies, caches and HSTS stay in memory, so md
  writes nothing under `~/.local/share/md/webkitgtk` or `~/.cache/md/webkitgtk`. The self-test lists
  the XDG folders after a run and fails on anything unexpected (§13.3).
- One `webkit::Settings` factory (`web::context::settings()`): `enable_javascript(true)`,
  `javascript_can_open_windows_automatically(false)`, `javascript_can_access_clipboard(false)`,
  `enable_back_forward_navigation_gestures(false)`, `enable_html5_local_storage(false)`,
  `enable_html5_database(false)`, `enable_webaudio(false)`, `enable_webrtc(false)`,
  `enable_media_stream(false)`, `enable_fullscreen(false)`, `print_backgrounds(true)`,
  `default_charset("utf-8")`, `enable_developer_extras(cfg!(debug_assertions))`,
  `enable_write_console_messages_to_stdout(cfg!(debug_assertions))`. (`enable_dns_prefetching` is
  deprecated since 2.48 and left alone.)
- **Sandbox preflight.** Before the first WebView exists, `WebServices` runs `bwrap --unshare-user
  --ro-bind / / true` once as md's child (`gio::Subprocess::newv`, awaited; ≈ 10 ms). WebKit's web
  processes start the same way, and if bubblewrap cannot get a user namespace WebKit `g_error`s and
  aborts md with no word (facts §9). On failure md shows one `adw::AlertDialog` saying the preview
  sandbox could not start and naming `/etc/apparmor.d/md` (docs/packaging.md §7), then quits. md
  never sets `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS`; `scripts/dev.sh` sets it for Docker only and
  skips the probe there through `MD_SKIP_SANDBOX_PROBE=1`, which the `.deb` build ignores
  (`cfg!(debug_assertions)` or the `selftest` feature only).

### 4.2 The `md-asset:` origin (`web::AssetHost`)

The spike's handler, kept (facts §3): `md-asset://<host>/index.html` is the page set for that host
(`AssetHost::set_page(host, html)`); `md-asset://<host>/rich/<rel>` is read from `Paths::rich_dir()`
(`/usr/share/md/rich`; `MD_DATA_DIR` overrides; a build in a checkout reads the checkout's, §9.3) after the
containment check (no empty, `.` or `..` segment, no `\`, `%` or NUL); anything else is a 404 with an
empty body. Each response: `URISchemeResponse::new(&gio::MemoryInputStream::from_bytes(..), len)`,
`set_status`, `set_content_type` (md-logic `preview::AssetMime::for(..)` — the Mac's nine rows;
WebKit sniffs nothing for a custom scheme), headers `Cache-Control: no-store` and, on `index.html`
only:

```
Content-Security-Policy: default-src 'self' data: blob:; script-src 'self' 'unsafe-eval' 'wasm-unsafe-eval';
  style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https: http:; font-src 'self' data:
```

Every engine rendered under it in its own process (facts §3, `secure_cors_csp`); it blocks inline
event handlers and `javascript:` URLs that the guard cannot catch. User scripts and
`evaluate_javascript` are not subject to page CSP. Guard + CSP together is a self-test check.

Hosts: `preview-<n>` per live preview, `export-<n>` per export renderer — separate origins, one
handler. `rich/` is the only directory the page can reach; `index.html` never touches disk.

### 4.3 Two HTML entry points (typography)

md-logic `preview::ScreenHtml` (Linux constants, §15) inserts, before the first `</head>`:

```
\n<style id="md-linux-fonts">body{font-family:"Nimbus Mono PS","Courier New",serif;}</style>
```

Used by the live preview (`RenderKind::Screen`), Print and PDF (`RenderKind::Paper`). HTML, EPUB and
SVG exports (`RenderKind::Export`) load pure Core HTML. Code keeps the stylesheet's `"Courier New"`,
which fontconfig maps to Liberation Mono (facts §7), so prose and code differ as on the Mac.

### 4.4 `PreviewHost`

```rust
pub struct PreviewHost { view: webkit::WebView, ucm: webkit::UserContentManager, host_name: String, .. }
impl PreviewHost {
    pub fn new(web: &Rc<WebServices>) -> Rc<PreviewHost>;
    pub fn widget(&self) -> gtk::Widget;
    pub fn surface(self: &Rc<Self>) -> Rc<dyn PreviewSurface>;   // md-logic seam
    pub fn apply_theme(&self, dark: bool);                         // background first (§4.8)
    pub fn apply_scroll_fraction(&self, fraction: f64);            // __mdSyncScrollTo(f)
    pub fn did_scroll(&self) -> &Event<f64>;
    pub fn set_shown(&self, shown: bool);                          // the pane's visibility, for is_shown
    pub fn close(&self);
}
```

`PreviewSurface` (md-logic seam) is implemented as: `set_html` → `AssetHost::set_page`;
`navigate(url)` → `load_uri`; `reload()` → `reload()` (re-requests `index.html`, WebKit keeps the
scroll by itself and the coordinator restores it anyway — facts §2); `eval(script)` →
`evaluate_javascript_future(script, None, None)` → `javascriptcore::Value::to_json(0)` (`"1"` with its
quotes, `null`; an exception → `"null"`); `navigation_completed` ← `connect_load_changed` with
`LoadEvent::Finished` (true) and `connect_load_failed` (false); `is_shown` = the pane's own flag
(`set_shown`), not GTK visibility of an ancestor.

Re-render policy is md-logic `preview::PreviewCoordinator` unchanged (md.win §4.5, the Mac's): first
load and a new token navigate; edits re-render after **350 ms**, reloading with the saved `scrollY`
restored after `Finished`; a collapsed pane only records "stale" and reloads once on `show()`;
heading navigations are parked while loading. The document window passes `token = None`, the Book
pane the article path.

### 4.5 Injected scripts and message handlers (once per `UserContentManager`)

- **Scroll sync**: the Mac's script **byte for byte** — WebKitGTK exposes
  `window.webkit.messageHandlers`, so md.win's `chrome.webview` substitution is not needed
  (`UserScript::new(src, TopFrame, UserScriptInjectionTime::Start, &[], &[])`);
  `register_script_message_handler("mdScroll", None)` +
  `connect_script_message_received(Some("mdScroll"), ..)` → `value.to_json(0)` →
  `JsonScript::scroll_message` → drop echoes → `ScrollSync::preview_did_scroll`.
- **Link guard**: md.win's capture-phase `click` / `auxclick` guard (facts §3: stops `javascript:`
  for programmatic and real clicks), injected at document start.
- **Render-complete**: `register_script_message_handler("mdRender", None)` — md-init.js posts
  `'complete'` exactly as on the Mac (facts §4). The live preview uses it for the retry policy (§4.9);
  exports keep md-logic's polling `RenderCompletePoller` (250 ms × 480, timeout = success) for parity.

Registered scripts are not part of the DOM, so no export ever carries them.

### 4.6 Navigation policy

`connect_decide_policy(|_, decision, kind|)`: `NavigationAction` and `NewWindowAction` →
`decision.downcast_ref::<webkit::NavigationPolicyDecision>()` → `navigation_action()` →
`request().uri()`, `is_user_gesture()` → md-logic `preview::LinkPolicy::decide(uri,
user_initiated, index_url)`:

| Case | Decision | GTK |
| --- | --- | --- |
| `uri` without fragment == this surface's `index.html` (our loads, `#slug` hops — WebKitGTK raises the policy for these) | Allow | `decision.use_()` |
| any other `md-asset://…` (`[x](other.md)`, another surface) | Cancel | `ignore()` |
| `http`/`https`, user-initiated | OpenExternally | `ignore()` + `gtk::UriLauncher::new(uri).launch(Some(&window), None, ..)` |
| `http`/`https` not user-initiated (script, refresh) | Cancel | `ignore()` |
| anything else (`file:`, `data:`, `mailto:`) | Cancel | `ignore()` |

Responses: default handling. `connect_create(|_, _| None)`: never a new WebView. The context menu
(`connect_context_menu`) keeps only items whose `stock_action()` is `ContextMenuAction::Copy` or
`SelectAll` (WKWebView's Copy parity; Reload / Back / Inspect go). `connect_zoom_level_notify` pins
the live preview's zoom at 1.0 (WebKitGTK zooms on touchpad pinch; WKWebView does not by default).

### 4.7 Scroll sync

md-logic `view::ScrollSync` + `ScrollSyncGuard` wire the two halves: editor → `__mdSyncScrollTo(f)`
(`Scripts::sync_scroll_to`), preview → the message above → the editor's adjustment (§3.5).

### 4.8 Light / dark

`adw::StyleManager::default().connect_dark_notify`: the shell reloads its CSS (§10), then every
window calls `PreviewHost::apply_theme(dark)` — `WebView::set_background_color(&paper)` so the reload
never flashes white — **before** `coordinator.update(text, title, dark, token)`, which builds a new
page with the other `data-md-dark` (md-core `markdown::html::document(.., dark, ..)`) so Mermaid and
PlantUML redraw. WebKit's `prefers-color-scheme` follows libadwaita (facts §7). The Mermaid
first-block light-theme race in dark mode is family-wide (facts §10.6) and is not fixed here.

### 4.9 When the web process dies (Mac 1.5 `PreviewRetryPolicy`)

`connect_web_process_terminated(|_, reason|)`: `Crashed` and `ExceededMemoryLimit` go to a port of
the Mac's `PreviewRetryPolicy` (`web::retry_policy`, pure, its Swift tests ported): the first death
reloads (`load_uri(index)`), a second in a row with no completed render between them gives up and
shows the Mac's one quiet line — `"*The preview stopped unexpectedly. Your document is unchanged —
the next edit will try again.*"` rendered through the normal document pipeline; an edit lifts the
give-up and buys one attempt; an `mdRender 'complete'` message resets the count. "Unresponsive" is
never acted on (a long PlantUML layout looks the same). `TerminatedByApi` is ignored (our own close).

### 4.10 Images beside the document

Not resolved — parity: a relative `photo.png` is `md-asset://preview-1/photo.png` → 404, as on every
port; `http(s):` and `data:` images load (the network fetch PRIVACY describes). Only Export ▸
TextBundle… reads local images (§7.7).

---

## 5. View modes, Zen, footer, Contents and Notes

### 5.1 Per-window state

md-logic `view::DocumentWindowState` (stored mode, Zen, Zen reading, last identity, book-article
flag, navigation nudge, pending preview navigation and editor jump, derived text), driven by
`ViewModeController` (the Mac's `setMode` / `applyViewModeMemory`, md.win §5.2) over
`SettingsViewModeStore` (`md.viewModeMemory`). Identity = md-core `view_mode_memory::identity_for(path)`
(Linux never folds case). `apply_memory(path, text)` runs on window appearance and on every identity
change (first save, Save As, Rename, Move To, a monitor `Renamed`).

### 5.2 Layout (`editor::ArticlePanes`)

md-logic `view::SplitLayout::arrange(mode, width)` → editor only / preview only / side by side /
stacked; side by side from **640 px** up. GTK:

- a `gtk::Box` with `homogeneous(true)` holds the editor column (find bar + editor) and the preview,
  so Split is two equal halves with no splitter (the Mac has none); the 1 px divider is a CSS border
  on the editor column (`border-right` or `border-bottom`, colour `Palette::border()`), not a widget;
- the box sits in an `adw::BreakpointBin` with one `adw::Breakpoint`
  (`BreakpointCondition::new_length(Max, 639.0, LengthUnit::Px)`) whose setter turns the box
  `orientation` to `Vertical` — stacked below 640 px of *pane* width (the Book window's content is the
  window minus the sidebar);
- Edit hides the preview (`set_visible(false)` — it stays in the tree, facts §2) and calls
  `coordinator.hide()`; Preview hides the editor column; `PreviewHost::set_shown` mirrors the slot.

### 5.3 Zen (`editor::zen`)

- Toggle: Shift+Ctrl+Return and the capsule's exit button; Escape exits (§2.2). `ZenController` raises
  `full_screen_requested(bool)` → `fullscreen()` / `unfullscreen()`; `notify::fullscreened` →
  `presenter_changed(is_fullscreen())` → leaving full screen by any means (F11, the shell) drops Zen
  (the `toggling` flag absorbs our own request).
- Geometry: the header bar and menu bar (`ToolbarView::set_reveal_top_bars(false)`), the footer, the
  bar slot and the divider hide; the single pane (editor when writing, preview when reading) is laid
  out by **`ZenBin`**, a small `gtk::Widget` subclass whose `size_allocate` gives its child
  **2/3 of the width and 92 % of the height**, centred — the Mac's fractions, DPI-free (gtk4-rs
  `glib::object_subclass`, no `unsafe`).
- Capsule: a `gtk::Box` (css `md-zen-capsule`: paper-secondary at 80 %, radius 20 px, padding 6 px)
  in the window overlay, top centre, margin 10: write (`document-edit-symbolic`, tooltip "Write"),
  read (`view-reveal-symbolic`, "Read"), a separator, exit (`view-restore-symbolic`, "Exit Zen
  Mode"); the active switch is tinted accent. Fade: a `gtk::Revealer` (`Crossfade`, 300 ms);
  `gtk::EventControllerMotion` on the overlay → `ZenController::reveal()`, which hides it again after
  **2.5 s** (`controls_shown_changed`).
- Zen never touches `md.viewModeMemory` or the stored mode.

### 5.4 Footer (`editor::Footer`)

A bar under the panes: right-aligned `gtk::Label` `strings::footer(words, characters)` ("N words ·
M characters"), typewriter 11 pt, ink 60 %, background paper-secondary, padding 5 × 12 px,
`font-feature-settings: "tnum"`; on the left an `adw::Spinner` shown by md-logic's `BusyRing` 500 ms
into an export. Hidden in Zen.

### 5.5 Derived text

md-logic `view::DerivedTextScheduler`: the first computation immediate, later ones 250 ms after the
last edit, computed **off the UI thread** with `gio::spawn_blocking` (the inputs are a `String`, the
outputs `Send` data: words via md-core `writing_stats::words`, characters = grapheme count, outline,
notes, diagrams) and delivered back on the main context. Feeds the footer, the Go menu, the Book
header's Contents / Notes, and Export ▸ Diagram as SVG.

### 5.6 Contents and Notes jumps

md.win §5.6 verbatim: a heading jump navigates the preview (when shown) and the editor (when shown),
never nudging; a note jump nudges Preview → Edit (never stored) and moves the caret. Note rows use
`NotePreview::of` (first non-empty line, 50 graphemes, `"(empty note)"`).

---

## 6. Documents

### 6.1 Types and dialogs

The extension sets are md-logic `documents::DocumentLoader`'s (the family's): Markdown `.md
.markdown .mdown .markdn .mdtext .mdtxt .mkd .mkdn .mdwn .mkdown`; plain text `.txt .text`; PlantUML
`.puml .plantuml .iuml .pu`; Graphviz `.gv`; `.textpack`. `.dot` stays unclaimed by md's loader
(docs/packaging.md §3 on the MIME side). A unit test pins `DocumentLoader::OPEN_EXTENSIONS` against
the shipped MIME XML and the `.desktop` `MimeType` list in both directions (md.win's
`FileAssociationTests`).

`services::GtkPickers` implements `seams::Pickers` with `gtk::FileDialog` (`modal(true)`, parent =
the window):

- Open: `open_multiple_future(Some(&window))` with **one** `gtk::FileFilter` (suffixes of every open
  extension, plus `text/markdown`, `text/x-markdown`, `text/plain` mime types) named
  `strings::documents::OPEN_FILTER` (§15) — GTK hides the filter popup when there is one. Not
  `open_text_file`: md's codec decides the encoding (UTF-16 behind a BOM, strict UTF-8, CP1251,
  Latin-1), as on the Mac, so GTK's Encoding choice must not appear.
- Save: `save_future` with `initial_name` (current base name or "Untitled" + the default extension),
  `initial_folder` (the document's folder), filters = `DocumentLoader::save_choices()` in the Mac's
  order. GTK's dialog does not create the file (unlike WinRT's). A name typed without an extension
  gets `DocumentLoader::default_extension_for(..)`; if that new name exists, `Alerts::confirm_replace`
  asks (GTK only confirmed the name the user typed).
- Folders: `select_folder_future` with `title` (Example Book, Open Book, TextBundle export,
  Move To). Cancel → `DialogError::Dismissed` → `None`.

GTK decides between its own chooser (what Xvfb shows, facts §8) and the xdg-desktop-portal one
(facts §8 expects the portal on the desktop); either way there is no Encoding row, because md never
calls the `*_text_file` variants.

**Drag and drop**: a `gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY)` on
each window's content in the **capture** phase: files open as documents, `.textpack` imports, a
`.textbundle` folder imports, other folders are ignored. Text drags are not `FileList`s and still go
to the editor.

### 6.2 File I/O

`services::StdFileSystem` (`seams::FileSystem`) over `std::fs`: reads whole files; writes **in
place** (`OpenOptions::new().write(true).truncate(true)`) — never write-then-rename, so the file keeps
its inode, owner, mode, hard links and xattrs, and the monitor sees one change our stamp guard
filters (`FileStamp` = mtime + length). A new file (Save As) is created with the umask's mode.

### 6.3 `TextFileSession` — one state machine for documents and articles

md-logic `documents::TextFileSession`, md.win §6.3 verbatim: open (codec, BOM and newline dressing,
stamp, recent add, monitor on), edit (dirty, autosave 1 s when saved and not conflicted), autosave
(stamp check, encode with upgrade, in-place write, still "— Edited"), Save (clears dirty), Save As,
Revert, external change (ignored when the stamp matches; dirty → conflicted; clean → silent reload,
selection kept), rename, delete (document window keeps the buffer and marks dirty; book article
detaches), close (dialog for untitled dirty, flush otherwise, rescue on failure).

Windows bind the title to `is_dirty` (never `has_unsaved_changes`), add to recents on
`identity_changed`, delete drafts (§1.6), and own the bars:

- **Conflict bar** (`TextFileSession::conflicted`): `strings::documents::FILE_CHANGED_ON_DISK`,
  `dialog-warning-symbolic`, buttons **Reload from Disk** (`resolve_conflict_reloading`) and **Keep
  My Version** (`resolve_conflict_keeping_mine`);
- **Save-error bar** (`save_error_text`): `could_not_save(err)`, **Retry** (`flush_now(true)`) and
  **Save As…**.

Both are one `gtk::Revealer` holding a `gtk::Box` (css `md-bar`) above the footer — `GtkInfoBar` is
deprecated since GTK 4.10 and `adw::Banner` carries one button only. Routine autosave shows nothing.

### 6.4 Save As, Rename, Move To, Duplicate, Revert

- **Save As…**: §6.1 save dialog → `session.save_as(path)` → recent add, monitor retarget,
  `apply_memory(new path)`, draft deleted.
- **Rename…**: `Alerts::prompt_name(RENAME_TITLE, RENAME_MESSAGE, stem, "Rename")`; md-core
  `book_folder::is_valid_name` (the Mac's rule: no `/` or `:`) else alert `COULD_NOT_RENAME` /
  `INVALID_NAME_MESSAGE`; extension kept (`FileNames::with_extension_of`); flush; then
  `gio::File::move_(&target, gio::FileCopyFlags::NONE, ..)` — fails with `G_IO_ERROR_EXISTS` rather
  than overwriting; `session.retarget(new)`, recent, `apply_memory`.
- **Move To…**: `select_folder_future` → the same `gio::File::move_` (it copies + deletes across
  file systems), same follow-up.
- **Duplicate**: new untitled window with the text, title "{name} copy", dirty (a draft at once).
- **Revert to Saved**: `session.revert_to_saved()`; the editor gets the text through §3.3's external
  path (history cleared).

### 6.5 TextBundle / TextPack import

`.textpack` (Open…, `open`, drop): md-core `export::text_bundle::text_from_pack`. `.textbundle`
folders (File ▸ Open TextBundle Folder…, `open` with a folder, drop):
`text_from_bundle_directory`. Both open as untitled documents titled after the bundle, `imported_from`
set, never written back; Save asks for a new `.md` (md.win §6.5). The bundle's images are not shown
(parity).

### 6.6 Recent files (`services::RecentFiles`)

`gtk::RecentManager::default()` — the desktop's shared `~/.local/share/recently-used.xbel`, which
also feeds GNOME Files' Recent. On open and Save As: `add_full(uri, &RecentData)` with app name
`"md"`, app exec `"md %f"`, the file's MIME type and display name. File ▸ Open Recent lists the
items whose `has_application("md")` is true and whose md stamp (`application_info("md")` → the third
field, last registered) is newer than `md.linux.recentClearedAt`, newest first, **at most 10** (the
Mac's NSDocumentController default). A row's title is the file name; two rows with the same name get
their folder appended (`notes.md — ~/a`), since a `GMenu` row has no tooltip. A row whose file is
gone → `remove_item(uri)` (the file no longer exists anywhere) + alert `file_not_found`. **Clear Menu** writes `md.linux.recentClearedAt = now` — md's menu empties while
other applications keep their own history. Untitled documents and bundle imports are never added.

### 6.7 File monitoring (`services::GioFileWatcher`)

`seams::FileWatcher` over `gio::File::monitor_file(FileMonitorFlags::WATCH_MOVES, None)`
(`GInotifyFileMonitor`, which watches the directory and survives replacements — facts §8):

| `gio::FileMonitorEvent` | Reported as |
| --- | --- |
| `Changed`, `ChangesDoneHint`, `Created` for our file, `AttributeChanged` | `changed(path)` (coalesced: one per 100 ms) |
| `Renamed` whose **other** file is ours (another editor's atomic save) | `changed(path)` |
| `Renamed` whose file is ours (renamed away in the same folder) | after **250 ms**, if our path does not exist again: `renamed(old, new)`; if it does (vim's backup dance: rename away, write anew): `changed(path)` |
| `MovedOut` | `deleted(path)` (inotify cannot say where) |
| `Deleted` | `deleted(path)` |

Events are hints; the stamp check before every write is the guard (md.win's FileSystemWatcher is no
different). Network mounts without inotify get no events and rely on the stamp.

---

## 7. Exports and print

### 7.1 The export renderer (`export::OverlayRenderer`)

md-logic `export::ExportPipeline` drives `seams::RenderSurfaceFactory` / `RenderSurface`; the shell
implements them **per window** (the requesting window's `gtk::Overlay`):

- `create(kind)`: a fresh `webkit::WebView` (shared context and ephemeral session, the settings of
  §4.1, no user scripts) added with `overlay.add_overlay(&view)`, `set_opacity(0.0)`,
  `set_can_target(false)`, `set_can_focus(false)`, `halign/valign = Start`,
  `set_size_request(595, 842)` — **mapped and invisible**, `visibilityState` "visible", no timer
  throttling, viewport exactly as requested (facts §6). Host `export-<n>`; HTML per `RenderKind`
  (Screen / Paper / Export, md-logic decides).
- `load(html, cancel)`: `AssetHost::set_page`, `load_uri(index)`, await `LoadEvent::Finished`
  (`load-failed` → `RenderError::Failed`), then md-logic's `RenderCompletePoller` (250 ms × 480,
  timeout = success).
- `eval(script)`: as §4.4.
- `set_height(css_px)`: recorded (md-logic's grow-to-content call); the capture below does not need
  the page to be that tall.
- `capture_region_png(rect, scale)`: see §7.5.
- `pdf(geometry)`: §7.3.
- `close()`: `overlay.remove_overlay(&view)`; the WebView is dropped.

Exports on one window are serialised by the pipeline; the footer spinner follows `busy_changed`
(500 ms delay, `BusyRing`). Every output is handed to the window's `track_output` so a close waits
(10 s). Book outputs render in the Book window's overlay when it exists, else the invoking window's
(md.win §8.7).

### 7.2 Print… (document and book)

`ExportPipeline::print(source, title, show_print)`, where `show_print(paper_html)` is:

1. create an overlay renderer (Paper HTML: Core `export: true` — white paper, light ink, 11 pt — plus
   `md-linux-fonts`), `load` and wait for render-complete;
2. `let op = webkit::PrintOperation::new(&view)`; preset `gtk::PageSetup` A4
   (`PaperSize::new(Some(&gtk::PAPER_NAME_A4))`), portrait, **0.5 in** margins (as the PDF), and
   `gtk::PrintSettings` with the same paper; `connect_finished` / `connect_failed`;
3. `op.run_dialog(Some(&window))` — GTK's print dialog (printer, copies, pages, paper); `Cancel` →
   close the renderer; `Print` → keep it until `finished` / `failed`, then close. Failures are
   swallowed (the Mac does too).

`run_dialog` is modal (WebKit runs a nested loop); it is called from a `glib::idle_add_local_once`
and completes an md-logic `exec::Completion`, so no future is being polled while the nested loop
runs. No Done button (md.win needed one because WebView2's dialog reports nothing).

### 7.3 PDF (Export ▸ PDF…, Book PDF)

The pipeline styles the page (md-core `pdf_export::styled_for_export(html, page_size)` + the Linux
font style) and calls `RenderSurface::pdf(PrintGeometry)` (inches = pt / 72, 0.5 in margins —
md.win's decision, measured: first glyph at 36 + 42 pt, facts §5):

```rust
let settings = gtk::PrintSettings::new();
settings.set_printer(&glib::dgettext(Some("gtk40"), "Print to File"));   // localised name (facts §5)
settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(&gio::File::for_path(&temp).uri()));
let paper = gtk::PaperSize::new_custom(&size.id, &size.label, w_in, h_in, gtk::Unit::Inch);
settings.set_paper_size(&paper);
let setup = gtk::PageSetup::new();  setup.set_paper_size(&paper);  setup.set_orientation(Portrait);
setup.set_top_margin(0.5, gtk::Unit::Inch);  /* bottom, left, right */
let op = webkit::PrintOperation::new(&view);  op.set_print_settings(&settings);  op.set_page_setup(&setup);
op.connect_finished(..);  op.connect_failed(..);  op.print();            // no dialog
```

`temp` is `$XDG_RUNTIME_DIR/md/<uuid>.pdf` (falls back to `std::env::temp_dir()`); the bytes are
read, the temp file deleted, and the pipeline writes them to the path the save dialog returned
**after** rendering (the Mac's order). A4 → 595 × 842 pt (facts §5). Failure → "The PDF pages could
not be produced." under "Could not export PDF".

### 7.4 HTML

md.win §7.4 through the pipeline: `HtmlExport::prepare_page` → Export renderer → the capture script
(`outerHTML` without scripts and stylesheet links) → `html_export::finish(.., read_rich_asset)` (KaTeX
CSS and fonts inlined only for math, the Mermaid notice) → save dialog → UTF-8 without BOM.
`read_rich_asset(rel)` reads `rich/` through `AssetHost`'s containment.

### 7.5 EPUB, snapshots and the cover

Document: the title from front matter first (`epub_export::document_title`), save dialog **before**
building, then `ExportPipeline::export_epub(source, file_name, document_path)`; book: picker first,
then `BookOutput` → `export_book_epub(book)`. md-logic's `RichSnapshotter` loads each unit, grows to
content, waits the Mac's **300 ms** repaint, reads the five-column rects (`Scripts::RICH_ELEMENTS`),
throws on a DOM vs markup count mismatch, and asks `capture_region_png(rect, scale)` per element with
`scale = RichSnapshotter::scale_for(w, h)` (2, less past 16 Mpx).

`capture_region_png` on WebKitGTK — **per element, visible region**, never a full-document texture
(a 100-page document at 2× would be a 200-Mpx texture):

1. `d` = the window's surface scale (`gtk::Native::surface().scale()`, fractional on Wayland);
   `zoom = scale / d`; `view.set_zoom_level(zoom)`; `set_size_request(⌈595 × zoom⌉, ⌈max(842,
   rect.height) × zoom⌉)` — the CSS viewport stays 595 px wide, so rects keep their CSS values;
2. `window.scrollTo(0, rect.y)`; read back `window.scrollY` (the end of the document clamps it);
3. wait one frame (`requestAnimationFrame` twice via `call_async_javascript_function_future`) — the
   300 ms settle is only paid when the zoom or size changed;
4. `snapshot_future(SnapshotRegion::Visible, SnapshotOptions::NONE)` → `gdk::Texture`; factor =
   `texture.width() / 595` (device pixels per CSS pixel — expected `scale`, but measured, not
   assumed, as the spike did); crop `(rect.x, rect.y − scrollY, w, h) × factor` with `gdk::TextureDownloader` (`MemoryFormat::R8g8b8a8`) →
   `gdk::MemoryTexture::new(..)` → `save_to_png_bytes()`;
5. zoom back to 1 and 842 px tall after the last element.

The zoom-and-crop mechanism is proven (966 × 96 px for a 483 × 47.9 px formula, facts §6); the
visible-region variant and the HiDPI division are self-test checks (`GDK_SCALE=2` under Xvfb).

**Cover** (`cover:` front matter, 1.6): md-logic `export::CoverReader::read(reference, beside,
fetch)`; `beside` = `AssetReader::beside(document_path, fs, identity)` (contained in the document's
folder); `fetch` = the shell's `https` fetch over **libsoup 3** (`webkit::soup::Session` — already a
WebKit dependency, re-exported by webkit6, no new crate): `Message::new("GET", url)`,
`set_flags(MessageFlags::NO_REDIRECT)` and redirects followed by hand only to `https:`
(`CoverReader::MAX_REDIRECTS` = 5), `send_and_read_future(&msg, Priority::DEFAULT)` raced against
`CoverReader::FETCH_TIMEOUT` (15 s) with `glib::future_with_timeout`; any failure → `None` → no
cover.

### 7.6 Diagram as SVG and LaTeX

md.win §7.6 through the pipeline: SVG from the Export renderer (`Scripts::diagram_svg(ordinal)`,
`None` → "This diagram couldn't be captured — it may have failed to render.", never an empty file),
`diagram_svg::standalone_document`, save dialog `{sanitized title}-{n}.svg`; LaTeX is pure md-core
(`latex_export::document` / `book`) → save dialog → UTF-8.

### 7.7 TextBundle export

md.win §7.7: `select_folder_future` (title `CHOOSE_TEXT_BUNDLE_FOLDER`) → target
`{parent}/{sanitized}.textbundle`; existing → `confirm_replace(folder_exists(name))`; md-core
`text_bundle::export_rewriting(source, resolve)` with `AssetReader::beside` (only images strictly
inside the saved document's folder) → `BundleWrapper::write` (Core stages and swaps). An unsaved
document carries no images.

### 7.8 Alerts

`services::AdwAlerts` (`seams::Alerts`) over `adw::AlertDialog::new(Some(title), Some(message))`,
`add_response`, `set_response_appearance(.., ResponseAppearance::Destructive | Suggested)`,
`set_default_response`, `set_close_response`, then `present(Some(&window))` and the `response` /
`closed` signals — **never `choose_future`**: libadwaita-rs 0.9's trampoline asserts that
`choose_finish` returned a string, a cancelled choice returns NULL, so dropping the future aborts
the whole process; a caller that stops waiting closes the dialog instead. Name prompts put a
`gtk::Entry` in `set_extra_child` (activates the default response). One at a time per window (a
queue), and `dialog_open` in the snapshot while one is up (§2.2). Titles verbatim from
`strings::exports` / `documents` / `books`.

---

## 8. Books

### 8.1 Window and layout (`book::BookWindow`)

`adw::ApplicationWindow` → `ToolbarView` (header bar with the detail toolbar, §8.4, then the menu
bar) → `gtk::Overlay` → `gtk::Paned` (horizontal): start = the sidebar (§8.3), end = the detail
(§8.5). Sidebar width **240 px**, user-resizable and clamped to **200–320** in `notify::position`
(the Mac's `navigationSplitViewColumnWidth(min: 200, ideal: 240, max: 320)` — md.win could not resize
it); `shrink_start_child(false)`. View ▸ Show Sidebar hides / shows the start child. Full screen (F11)
is the writing room; no Zen (the Mac publishes none). Empty state (no book): an `adw::StatusPage`
(icon `x-office-address-book-symbolic`, title `NO_BOOK_OPEN`, description `NO_BOOK_OPEN_MESSAGE`,
child button **Open Book…**).

### 8.2 `BookLibraryHost` over md-core `book_folder`

| Mac | Linux |
| --- | --- |
| security-scoped bookmark | `md.bookBookmark` = the folder path — the whole record (no sandbox, md-logic settings_keys) |
| `beginAccess()` | the folder exists → use it; else the "not accessible" state, setting kept |
| `closeBook()` | removes `md.bookBookmark`; the folder is untouched |
| `newBook()` | `select_folder_future` (parent) + `prompt_name(NEW_BOOK_TITLE, NEW_BOOK_MESSAGE, "My Book", "Create")` → `create_dir`; exists → "Could not create book" |
| `unpackExampleBook()` | message `CHOOSE_EXAMPLE_BOOK_FOLDER`, folder dialog → copy `/usr/share/md/examples/Example Book` to `{picked}/Example Book`, deduped "Example Book 2", …; a half copy is removed on failure |
| `chooseBook()` | message `CHOOSE_BOOK_FOLDER`, folder dialog |

Listing skips names starting with `.` and `.md-reorder-*` (Linux hides by the dot only).

### 8.3 Sidebar

A `gtk::ListBox` (`SelectionMode::Single`, `activate_on_single_click(false)`) in a
`gtk::ScrolledWindow`, rows built from md-logic `books::BookSidebarModel`: root article rows, a "New
Article…" row, then per chapter a non-selectable header row (folder name, 13 pt) and its article rows
(`text-x-generic-symbolic` + name, 13 pt typewriter) and "New Article…" row. A "New Chapter…"
button (`folder-new-symbolic`, tooltip `NEW_CHAPTER_TOOLTIP`) heads the sidebar.

Context menu: a `gtk::GestureClick` (button 3) and a `gtk::GestureLongPress` per row open a
`gtk::PopoverMenu` from a `gio::Menu` whose items target a `book.` action group on the window
(`book.open-in-window(s)`, `book.rename(s)`, `book.move-up(s)`, `book.move-down(s)`,
`book.delete(s)`; Move Up / Down disabled at the ends) — titles `strings::books::{OPEN_IN_NEW_WINDOW,
RENAME_ITEM, MOVE_UP, MOVE_DOWN, DELETE_ITEM}`. `row-selected` → `navigator.selection_changed(path)`;
`row-activated` (double-click, Enter) → Open in New Window. No per-row handler that races selection
(the Mac's lesson). Prompts verbatim (md.win §8.3) with the Mac's name rule.

### 8.4 Detail toolbar — in the header bar (the Mac's unified toolbar)

Packed at the end of the Book window's `adw::HeaderBar`:

1. View mode: three `gtk::ToggleButton`s in a linked box, grouped (`set_group`) —
   `document-edit-symbolic` Edit, `view-dual-symbolic` Split, `view-reveal-symbolic` Preview; tooltip
   `VIEW_MODE_TOOLTIP`; insensitive without an article.
2. Previous / Next Article: `go-up-symbolic` / `go-down-symbolic`, tooltips
   `PREVIOUS_ARTICLE_TOOLTIP` / `NEXT_ARTICLE_TOOLTIP` (Alt+Up / Alt+Down), the stepper's flags.
3. Contents: `gtk::MenuButton` (`view-list-bullet-symbolic`, `CONTENTS_TOOLTIP`) whose menu is rebuilt
   in `set_create_popup_func` from the current outline.
4. Notes: `gtk::MenuButton` (`accessories-text-editor-symbolic`, `NOTES_TOOLTIP`).
5. Export: `gtk::MenuButton` (`document-send-symbolic`, `SHARE_TOOLTIP`): Export as PDF…, PDF Page
   Size ▸ (`app.pdf-page-size` radios), divider, Export as EPUB…, Export as LaTeX…, divider, Print…,
   divider, **Open Articles in Separate Windows** (`app.book-opens-in-separate-windows`, stateful `b`,
   `md.bookOpensInSeparateWindows`). No "Share as PDF".

A sidebar toggle (`sidebar-show-symbolic`) sits at the header's start.

### 8.5 Detail pane and the article session

One `TextFileSession` in `SessionRole::BookArticle` (md.win §8.5) plus md-logic
`books::BookNavigatorModel` and `BookStepper`. Stages: **Editing** → `ArticlePanes` (the same
editor + find bar + preview composite, a fresh undo history per article via `reset_history()`,
preview `token = Some(path)`, the footer); **Handoff** → `adw::StatusPage` (`HANDOFF_TITLE`,
`handoff_message(title)`, button **Show Window**); **Unreadable** → status page `UNREADABLE_TITLE`;
**Empty** → status page `EMPTY_TITLE` / `EMPTY_MESSAGE`. The footer carries the conflict and
save-error bars and the word count. Mode = `md.bookViewMode` (app-wide) with the transient nudge; a
pick stores the raw value; books never touch `md.viewModeMemory`. The article's monitor is the same
`GioFileWatcher`.

### 8.6 Registry, ownership, outputs

md-logic `windows::WindowRegistry` (exact path compare) gives ownership; the Book session calls
`recheck_ownership()` whenever any md window becomes active (`notify::is-active`), `Show Window` →
`WindowManager::present(id)`, Open in New Window → `hand_off_for_external_open()` → md-core
`BookArticleOpens::mark(path)` (10 s) → `WindowManager::open_path(path)`. Book outputs are md-logic
`books::BookOutput` behind the `BookFlushGate` (a failed flush vetoes silently), compile via md-core
`book_folder` / `book_compiler` (codec everywhere — md.win's recorded divergence, kept), renders
light.

---

## 9. Settings and state on disk

### 9.1 `KeyFileSettingsStore` — the choice

`$XDG_CONFIG_HOME/md/settings.conf` (`glib::user_config_dir()`, default `~/.config/md/`), one group
`[md]`, implemented over `glib::KeyFile`:

- **Why GKeyFile, not JSON.** It is in glib (no new dependency — md-logic has no runtime `serde_json`
  and the app should not add one for twelve keys); it reads and writes typed strings and booleans with
  escaping handled (the `v1` view-mode codec value is ~4.6 KB with no newlines); `save_to_file` is
  `g_file_set_contents`, which writes a temporary file and renames it — atomic, so a crash never
  leaves half a settings file; and it is the format Linux users and PRIVACY readers recognise
  (`key=value`). GSettings was rejected: it needs a compiled schema in `/usr/share/glib-2.0/schemas`,
  stores in the binary dconf database, and complicates running from the build tree.
- Semantics are md-logic `InMemorySettingsStore`'s: the bool keys (`md.bookOpensInSeparateWindows`,
  `md.continueLists`, `md.capitalizeSentences`, `md.wrapSelection`) are read and written with
  `boolean` / `set_boolean`, every other key as a string; a string read of a bool key is absent and
  vice versa; `changed` fires only when the stored value differs, after the write.
- Write-through: every `set_*` / `remove` saves the file (settings change at human speed). A file
  that fails to parse is renamed `settings.conf.corrupt` and the store starts empty; an unwritable
  folder leaves the store in memory with one warning on stderr.

Keys (md-logic `settings_keys`, §9 of md.win with the Linux ones): `md.viewModeMemory`,
`md.pdfPageSize`, `md.bookBookmark`, `md.bookOpensInSeparateWindows`, `md.bookLastArticle`,
`md.bookViewMode`, `md.linux.windowSize.document`, `md.linux.windowSize.book`, `md.continueLists`,
`md.capitalizeSentences`, `md.wrapSelection`, and **`md.linux.recentClearedAt`** (string, seconds
since the epoch; §6.6 — to be added to md-logic, §15).

### 9.2 Other state

| What | Where | Why there |
| --- | --- | --- |
| `session.json` | `$XDG_STATE_HOME/md/session.json` | XDG STATE: layout and open files reused on restart (§1.6) |
| drafts | `$XDG_DATA_HOME/md/drafts/*.md` (`0700` / `0600`) | user text, not cache (§1.6) |
| recent documents | `~/.local/share/recently-used.xbel` (GTK, shared) | the desktop's own list (§6.6) |
| the file chooser's own preferences | `~/.config/dconf/user` (the desktop's settings database, shared) | written by GTK's file chooser (GSettings `org.gtk.gtk4.Settings.FileChooser`: size, sort order, hidden files…) whenever md shows it — never by md's code, never a file name; the `privacy` self-test proves it holds only that schema's keys |
| PDF temp files | `$XDG_RUNTIME_DIR/md/` (deleted after use) | tmpfs, per user |
| web data | nowhere — ephemeral `NetworkSession` | §4.1 |
| per-document encoding, newline, scroll | memory only | as md.win |

PRIVACY.md (packaging work package) enumerates exactly these.

### 9.3 `Paths`

`data_dir`: `$MD_DATA_DIR`; else beside the executable — `<prefix>/share/md` for
`<prefix>/bin/md`, or the checkout for a binary cargo built in it (`target/release/md`), each only
when it holds `rich/md-init.js` and `examples/`; else `/usr/share/md`; else — debug builds only —
the source tree (`rich/`, `examples/`). `rich_dir = data_dir/rich`, `examples_dir = data_dir/examples`. The XDG
folders come from `glib::user_config_dir()`, `user_state_dir()`, `user_data_dir()`,
`user_runtime_dir()`, created on first write.

---

## 10. Theme and typography

One `gtk::CssProvider` at `STYLE_PROVIDER_PRIORITY_APPLICATION`
(`gtk::style_context_add_provider_for_display`), its text generated by `theme::css(Palette)` from
md-logic `settings::Palette::for_dark(StyleManager::is_dark())` and reloaded
(`load_from_string`) on `connect_dark_notify`. The app follows the system colour scheme (the Mac
follows the system appearance); there is no in-app appearance switch (the spike's View ▸ Appearance
goes).

| libadwaita variable / selector | Light | Dark | Role |
| --- | --- | --- | --- |
| `--window-bg-color`, `--view-bg-color`, `--headerbar-bg-color` | paper `#F4EFE2` | `#241E18` | the whole window is paper, as on the Mac |
| `--window-fg-color`, `--view-fg-color`, `--headerbar-fg-color` | ink `#2B2620` | `#E7DBC2` | |
| `--popover-bg-color`, `--dialog-bg-color`, `--sidebar-bg-color` | paper-secondary `#EAE2CF` | `#2F2820` | menus, dialogs, book sidebar |
| `--accent-bg-color`, `--accent-color` | accent `#9C6B2E` | `#C99A55` | toggles, selection, caret (overrides Yaru's accent) |
| `.md-footer`, `.md-bar`, `.md-zen-capsule` | paper-secondary | | footer, bars, capsule (80 %) |
| `.md-divider` border | ink @ 16 % (`Palette::border`) | | the Split divider |
| `.md-placeholder` | ink @ 40 % | | editor placeholder |

`WebView::set_background_color` takes the paper colour (`gdk::RGBA` from `Palette::channels`).

**Fonts** (facts §7, adopted): **Nimbus Mono PS** — URW's Courier, with real Regular / Bold / Italic /
Bold Italic, Cyrillic and Greek — via `Depends: fonts-urw-base35, fonts-liberation`.

| Surface | Mac pt | GTK CSS |
| --- | --- | --- |
| editor, placeholder | 15 | `font: 15pt "Nimbus Mono PS", "Liberation Mono", monospace` |
| footer | 11 | 11pt typewriter, `tnum` |
| book sidebar rows, chapter headers, find fields | 13 | 13pt typewriter |
| "New Article…" | 12 | 12pt |
| status-page titles / messages | 17 / 12 | 17pt / 12pt typewriter |

Menus, dialogs and header bars keep the system UI font (the Mac's menus are the system font too). No
font is bundled. If Nimbus Mono PS reads too thin on a real display, the alternative is bundling
upstream Courier Prime (OFL) — a licence decision (open question).

Icons are Adwaita/Yaru symbolic names (Appendix A lists the ones checked to exist in
`adwaita-icon-theme` on 26.04); the app icon is `me.nettrash.md` in hicolor (docs/packaging.md §4).

---

## 11. Strings

Every user-visible string comes from md-logic: menu and row titles from `commands::CommandTable`,
everything else from `md_logic::strings` (dialogs, bars, placeholders, tooltips, the footer, find
labels, Zen tooltips, book prompts and stages, About). The shell spells none of its own, with four
exceptions that §15 asks md-logic to adopt so they live in one place: the preview give-up line
(§4.9, the Mac's), the Open dialog's filter name (§6.1), "Quit" and "Find and Replace…" (§2.9). The
`.desktop` / metainfo strings are packaging (docs/packaging.md). English only, as every md port.

---

## 12. Wayland and X11

Ubuntu 26.04's GNOME session is Wayland-only, so md runs as a native Wayland client there (GTK picks
the backend; X11 sessions — Xfce, MATE — get the X11 backend; XWayland is not used). The spike
proved everything on **X11 under Xvfb only**. What changes on Wayland:

| Area | Wayland | Design consequence |
| --- | --- | --- |
| Window position | not settable by clients | no cascade, no restored positions (§1.4) — same under GTK 4 on X11 |
| Focus / raise | needs an xdg-activation token | GTK forwards the launch token; the single-instance hand-off and Window-menu activation rely on it (risk) |
| Full screen / maximize | asynchronous, compositor-driven | all state read from `notify::fullscreened` / `maximized` |
| Shortcuts | GNOME Shell grabs its own first (Ctrl+Alt+arrows, Super+…) | §2.8 chord choice |
| Input methods | text-input-v3 (IBus in the compositor) | preedit via `preedit-changed`; dead keys and CJK on the desktop checklist |
| Rendering | WebKit uses DMA-BUF / GPU; GTK uses GL or Vulkan | invisible overlay renderer must stay mapped (opacity 0 is still mapped); NVIDIA blank-page reports are worked around by users with `WEBKIT_DISABLE_DMABUF_RENDERER=1`, never by md |
| Scaling | fractional per monitor | snapshot zoom divides by the surface scale (§7.5) |
| Clipboard | asynchronous | paste flag settles on `paste-done` (§3.3) |

The self-test runs on X11 (Xvfb); `GDK_BACKEND=wayland` under a headless compositor is not part of
the plan (no compositor in the image). Wayland is a real-desktop check for every work package.

---

## 13. The self-test (`md --selftest`)

### 13.1 Why

The shell cannot be unit-tested without a display, and md.win's answer — an in-app self-test that
drives the real windows and the real web engine and writes a JSON report — works better here: Xvfb
gives the container a real display and `xdotool` real XTEST keystrokes (the spike proved both).

### 13.2 Shape

`md --selftest OUT [--only <group>,…] [--no-input]`, compiled only with the cargo feature
`selftest` (`scripts/dev.sh selftest` enables it; the `.deb` build does not, so a shipped binary
answers `--selftest` with "unknown option"). The process is `NON_UNIQUE`, refuses to run unless
every XDG variable points into `OUT/home/` (`scripts/dev.sh` exports them; `set_var` is `unsafe` in
edition 2024), so it has its own settings file, and exits 0 / 1 after writing
`OUT/report.json` (`{"checks": [{"name", "passed", "detail"}], "groups": [...], "started", "finished"}`)
and screenshots (`import -window root`, ImageMagick in the image). Each group lives in its own file
owned by the work package it checks (docs/shell-work-packages.md).

### 13.3 Groups (what each proves)

| Group | Checks (each a named row in the report) |
| --- | --- |
| `shell` | app starts; one window; eight menus in order; every `CommandTable` row present exactly once with its title; accels installed as §2.9; a disabled action's chord reaches the editor; `--version` |
| `documents` | open each associated extension through `open`; a second `open` of the same path focuses the window; textpack / textbundle import titles; autosave writes in place (inode unchanged); external `echo >>` → silent reload; external atomic rename-over under edits → conflict bar; Revert; Rename / Move To keep the session; rescue copy on a read-only file at close; drafts written, survive Quit, restored on relaunch |
| `editor` (input tier) | type `md` → `Md`; Backspace ×2, type `md` → `md`; type `m`, Ctrl+Z → `m` (one undo); select a word, type `*` → `*word*` with `word` selected, Ctrl+Z ×2; Enter continues `- a` → `- `; Shift+Enter plain; Tab inserts `\t`; Ctrl+B / Ctrl+Alt+2 / Ctrl+] edits with one undo each; Find / Replace / Replace All (one undo); Ctrl+G from the editor; Escape closes the bar; Zen enter / exit with F11 |
| `preview` | engines render offline (KaTeX `.katex` ≥ 1, `\ce{}` with 0 `.katex-error`, hljs, Mermaid, Graphviz, PlantUML SVGs, `plot` 1 svg / 2 polylines / no plot script) **under the CSP**; `javascript:` and `onclick` links do nothing; `other.md` cancelled; `#slug` scrolls; 350 ms debounce = one reload; scroll kept; dark switch re-renders with `data-md-dark`; scroll sync both ways; a killed web process (`terminate_web_process`) reloads once, twice → the notice |
| `exports` | PDF A4 595 × 842 pt and 6 × 9 in 432 × 648 pt, three `\newpage` sections → three pages; HTML stands alone (opened from `file://` in a fresh renderer: `.katex` present, `compatMode` CSS1Compat); EPUB snapshot count == markup count, PNGs valid, 2× within ±1 px, also at `GDK_SCALE=2`; a 40-page document's last formula snapshots (visible-region path); SVG for each diagram kind; LaTeX; TextBundle with an image beside; the cover from a file beside; exported HTML/EPUB never contain `md-linux-fonts` |
| `books` | no-book status page; open the Example Book; sidebar rows in order; select → editor; rename / move up / delete through the context actions; Alt+Down / Alt+Up step; Open in New Window → hand-off stage, Show Window; Print Book renders; Export Book PDF / EPUB / LaTeX |
| `privacy` | after all groups: the files under `OUT/home` are only `settings.conf`, `session.json`, the drafts and `recently-used.xbel` — nothing under `.cache/md` or `.local/share/md/webkitgtk`; beside them only system caches under `.cache/` (Mesa's shader cache) and `.config/dconf/user` holding nothing but GTK's file-chooser keys (§9.2) |

`--no-input` skips the keystroke rows (they need focus on the Xvfb display); CI and agents run them —
Xvfb with no window manager gives md the focus.

### 13.4 Only on a real desktop

Wayland behaviour (§12); the WebKit sandbox under AppArmor with the packaged profile; GNOME's file
chooser and portal-provided dark mode; IBus composition (dead keys, CJK) and the capital after a dead
key; the German AltGr layout vs Ctrl+Alt+digit; GNOME Shell's chord grabs; HiDPI / fractional
scaling of the editor font and the snapshots; printing to a real printer; Nimbus Mono PS on a real
screen; `.desktop` / MIME / icon integration (double-click in Files, Open With, the dock);
session-end (`query-end`) at logout; drag and drop from Files.

---

## 14. What the macOS app does that this design changes

Every deliberate deviation, with its reason. Everything not listed is ported as the Mac does it.

| # | macOS | Linux | Reason |
| --- | --- | --- | --- |
| 1 | Global menu bar; windows carry no chrome | A header bar (title, folder) and a menu-bar row in every window | GNOME has no global menu bar; CSD is mandatory on Wayland |
| 2 | The **md** app menu (About, Services, Hide, Quit) | Help ▸ About md; File ▸ Quit (Ctrl+Q); no Services or Hide | No app menu; GNOME's Quit lives with the app's commands |
| 3 | Rename / Move To / Duplicate / Revert from NSDocument | Implemented by md (§6.4) | No document framework |
| 4 | Versions (Revert To ▸ Browse All Versions) | none; Revert to Saved only | No analogue |
| 5 | Document browser (Open panel) at launch | restores the last session, else one untitled window | No document browser; GNOME editors open a document |
| 6 | App keeps running with no windows | exits with its last window | A windowless GTK process has no menu bar to show |
| 7 | Window frames and `@SceneStorage` restored | `session.json`: size, maximized, mode, Zen — **no positions** | GTK 4 cannot place windows; Wayland forbids it |
| 8 | Untitled drafts in Autosave Information | drafts in `~/.local/share/md/drafts`, kept on Quit / logout, restored | Same behaviour, XDG home |
| 9 | Quick Look extension (Space in Finder) | none | GNOME Files / Sushi have no third-party preview extension point |
| 10 | Share ▸ Source… / Rendered PDF…, Share Book as PDF, the book toolbar's Share as PDF | **omitted** | Linux has no share sheet; Export ▸ PDF covers the PDF |
| 11 | Find from `NSTextFinder` (⌘F, ⌥⌘F, ⌘G, ⇧⌘G, ⌘E) | md's own bar, same rows, Ctrl+F / Ctrl+H / Ctrl+G / Shift+Ctrl+G / Ctrl+E | GtkTextView has no find UI |
| 12 | ⌃⌘↑ / ⌃⌘↓ between articles | **Alt+Up / Alt+Down** (Ctrl+Alt+Up/Down also bound) | GNOME binds Ctrl+Alt+Up/Down to workspaces |
| 13 | ⌘ chords, ⌥⌘1–6, ⌘' Quote, ⇧⌘↩ Zen, ⌃⌘F full screen | Ctrl chords, Ctrl+Alt+1–6, Shift+Ctrl+Q, Shift+Ctrl+Return, F11 | Linux conventions; Shift+Ctrl+Q is the family's Quote chord (Windows, Android) — the Mac moved it to ⌘' only because ⇧⌘Q is Log Out there |
| 14 | American Typewriter | **Nimbus Mono PS** in the editor, preview and paper; exports pure | Ubuntu ships no American Typewriter (facts §7) |
| 15 | PDF margins from Page Setup | 0.5 in all round (md.win's decision); WebKitGTK's Skia PDF | A choice had to be made; bytes never match the Mac's |
| 16 | Print panel with live preview | GTK's print dialog (no page preview), A4 + 0.5 in preset | WebKitGTK uses GTK's dialog |
| 17 | `.textbundle` double-clickable | File ▸ Open TextBundle Folder…, drop, `md x.textbundle` | A folder has no MIME association |
| 18 | NSFilePresenter / NSFileCoordinator | `GFileMonitor` + the (mtime, size) stamp | No coordination API |
| 19 | Open Recent: NSDocumentController's 10 | the desktop's shared recent list, md's 10 newest; Clear Menu hides | System-wide list feeds Files |
| 20 | Security-scoped book bookmark | the folder path | No sandbox |
| 21 | Finder's natural order for unnumbered names | md-core `compare_natural` | Parity with Android / Windows |
| 22 | Zen capsule in `.regularMaterial` (blur) | paper-secondary at 80 % | GTK has no backdrop blur |
| 23 | WKWebView's context menu | Copy / Select All only; zoom pinned | Reload / Back / Inspect make no sense |
| 24 | Words by CFStringTokenizer | md-core `writing_stats::words` | No system word segmenter in the stack; CJK counts may differ |
| 25 | WebKit policy blocks `javascript:` | capture-phase guard + CSP | WebKitGTK runs `javascript:` hrefs in-page without a policy call |
| 26 | Menus rebuilt on every open | rebuilt when the snapshot changes | `GMenu` is a model, not a callback |
| 27 | System Edit items (Spelling, Substitutions, Speech, AutoFill, Emoji & Symbols) | none (GTK's Ctrl+. emoji picker remains in the editor) | Spelling is off on the Mac too |
| 28 | Title-bar proxy icon (folder menu) | folder shown as the header subtitle | No proxy icon on Linux |
| 29 | Book compile decodes UTF-8 then Latin-1 | codec everywhere | md.win's recorded divergence, inherited through md-core |
| 30 | Help menu searches a help book | md Help (F1) opens the support page | No help book |

Kept as on the Mac where md.win had to deviate: the accent caret, a resizable book sidebar
(200–320), Find in the book window, the Find submenu and ⌘G-style chords, dead-key capitals, no Done
button on Print, the scroll-sync script byte for byte, untitled drafts.

---

## 15. What the shell needs from md-logic (beyond md.win's API)

For the agents porting md-logic now:

1. **Command table, Linux rows**: §2.9 — `Exit` titled "Quit" (Ctrl+Q); no `ShareSource`,
   `ShareRenderedPdf`, `ShareBookPdf`; `Replace` titled "Find and Replace…" under Edit ▸ Find with
   the other Find rows; Ctrl+G / Shift+Ctrl+G; Redo Shift+Ctrl+Z (+ Ctrl+Y); Delete without a chord;
   Previous / Next Article Alt+Up / Alt+Down with Ctrl+Alt+Up / Down as a second chord (a `Chord`
   list or a `secondary` field); `Escape` not an accelerator. A way to turn a `Chord` into a GTK
   accelerator (or keyvals the shell maps — `menus::accels` owns the mapping either way).
2. **`ShellSnapshot`**: a `dialog_open` flag (§2.2) and `is_full_screen` (exists in md.win).
3. **Strings**: `books::PREVIOUS_ARTICLE_TOOLTIP` / `NEXT_ARTICLE_TOOLTIP` / `EMPTY_MESSAGE` name
   **Alt+Up / Alt+Down**; add the preview give-up line (§4.9, Mac-verbatim), the Open filter name
   (`documents::OPEN_FILTER`, "Supported Documents"), and the Linux titles of row 1.
4. **Settings keys**: `md.linux.recentClearedAt` (string) in `settings_keys::ALL`.
5. **Preview**: `ScreenHtml` with the `md-linux-fonts` style of §4.3; `Scripts::SCROLL_SYNC` = the
   Mac's text unsubstituted (`window.webkit.messageHandlers.mdScroll`); `AssetOrigin` = scheme
   `md-asset`, index URL per host (`md-asset://<host>/index.html`), and `LinkPolicy` treating every
   `md-asset:` URL that is not this surface's `index.html` as Cancel.
6. **Export**: `ExportPipeline` without the share seam (no `share_source`, `share_pdf`) and with the
   cover `fetch` as a parameter the app supplies (§7.5); `BookOutput` without `share_pdf`.
7. **Derived text**: an off-thread hook the app can satisfy with `gio::spawn_blocking` (md.win's
   `RunOffThread`).
8. **TypingHooks / FormattingAdapter**: md.win's API as is — the shell calls `before_text_changing`
   with the user action's start snapshot (§3.3); no change needed, but the history and paste flags'
   "settle a turn later" must be callable explicitly (`settle_history`, `settle_paste`).

---

## 16. Risks, and what cannot be byte-identical

Ranked by how late a failure would be noticed.

1. **The WebKit sandbox on a real 26.04** (launch blocker): the AppArmor profile is copied from
   Ubuntu's own WebKit-app profiles but could not be loaded in Docker. Mitigation: the preflight of
   §4.1 turns a silent abort into a message; the first line of the manual test
   (docs/packaging.md §11) checks it.
2. **Wayland was never run**: activation tokens for the hand-off and the Window menu, asynchronous
   full screen and Zen, the invisible overlay renderer under GTK's Wayland/Vulkan renderer, WebKit's
   DMA-BUF path. Every package's real-desktop list covers its part.
3. **Undo grouping**: that GtkTextHistory keeps the capital and the wrap as their own steps (§3.3) is
   asserted by the keystroke tier, not documented by GTK. If it merges them, the fallback is to apply
   them from a `timeout_add_local_once(Duration::ZERO)` after the IM's own idle work, then from the
   next key event's capture handler — decided by the self-test, not guessed.
4. **Accelerator phase**: whether GTK's application shortcuts run before the TextView's own bindings
   (capture) or after (bubble) is not documented in the 4.22 GIR. md's chords do not collide with
   GtkTextView bindings except the editing ones, which dispatch to the focused widget either way
   (§2.2). The shell self-test presses every chord from the editor, the find field and the preview.
5. **The print dialog's nested loop** (`run_dialog`) inside asynchronous code: called from an idle
   and completed through an `exec::Completion` (§7.2); a re-entrancy bug would show as a hang the
   exports self-test catches (Print to File is the container's only printer).
6. **Visible-region snapshots** (§7.5) are a variant of the proven full-document crop; the HiDPI
   division is reasoned, not measured. Fallback: full-document snapshots for units whose texture
   stays under 64 Mpx.
7. **Input methods**: preedit / commit ordering under IBus on Wayland decides whether a dead-key
   capital works; a wrong order only loses that capital (never a keystroke).
8. **Typography**: Nimbus Mono PS may read thin on screen; Courier Prime (OFL) bundled is the
   alternative (a licence decision).
9. **Shortcut grabs** differ between desktops: Alt+Up/Down is free on GNOME and GtkTextView; a
   desktop that grabs it still has Ctrl+Alt+Up/Down and the header chevrons.
10. **md-logic is being ported at the same time**: names in this document are md.win's in Rust
    spelling; the shell adapts at merge (§15 lists what it needs added).
11. **Family-wide traits kept on purpose** (do not "fix" here): Mermaid's first block light in dark
    mode, local relative images never shown in the preview, `.dot` unclaimed, raw HTML escaped.

**Not byte-identical, by platform fact**: PDF and print (WebKitGTK's Skia PDF, Nimbus Mono PS, 0.5 in
margins); the on-screen typeface; word counts for CJK (md-core's segmenter, not CFStringTokenizer).
**Byte-identical**: every HTML / CSS / SVG / EPUB / LaTeX / TextBundle string md-core produces, the
settings codec and the identity hash, `md-init.js` and `rich/`, the examples, and the HTML / EPUB / SVG
exports (they never see the Linux font style).

---

## Appendix A — API verification log (crate sources, 2026-10-27)

Checked in the cargo registry of the `md-linux-cargo` volume (`gtk4-0.11.5`, `gdk4-0.11.5`,
`libadwaita-0.9.2`, `webkit6-0.6.1`, `soup3-0.9.0`, `javascriptcore6-0.6.0`, `gio-0.22.10`,
`glib-0.22.10`), the GTK 4.22 GIR and the 26.04 image:

| Claim | Found | Used in |
| --- | --- | --- |
| `TextBuffer`: `begin_user_action`, `end_user_action`, `begin_irreversible_action`, `undo`, `redo`, `can_undo`, `connect_begin_user_action`, `connect_end_user_action`, `connect_delete_range`, `connect_insert_text(Fn(&Self, &mut TextIter, &str))`, `connect_undo`, `connect_redo`, `connect_paste_done`, `connect_can_undo_notify`, `connect_has_selection_notify` | yes | §3 |
| `TextView`: `set_accepts_tab`, `set_input_hints`, `set_extra_menu`, `scroll_to_mark`, `connect_preedit_changed`, `connect_paste_clipboard`; **no placeholder** | yes / none | §3.1 |
| `FileDialog`: `open_multiple_future`, `open_text_file_future`, `save_future`, `select_folder_future`, `set_filters`, `set_initial_folder`, `set_initial_name` | yes | §6.1 |
| `RecentManager::default`, `add_full`, `items`, `remove_item`; `RecentInfo::has_application`, `application_info → (exec, count, DateTime)` | yes, not deprecated | §6.6 |
| `gtk::Application`: `set_accels_for_action`, `set_register_session`, `connect_query_end`, `inhibit` | yes | §1, §2 |
| `SearchEntry`: `connect_stop_search`, `connect_next_match`, `connect_previous_match`, `set_search_delay` | yes | §3.6 |
| `MenuButton::set_create_popup_func` | yes | §8.4 |
| `UriLauncher::launch`, `FileLauncher::open_containing_folder` | yes | §2.8, §4.6 |
| `GtkPopoverMenu` menu attributes: label, use-markup, action, target, icon, submenu-action, hidden-when, custom — **no "accel"** documented | GIR | §2.2 |
| `adw::AlertDialog`: `add_response`, `set_response_appearance`, `set_default_response`, `set_close_response`, `set_extra_child`, `choose_future` | yes | §7.8 |
| `adw::AboutDialog`: `set_comments`, `set_license_type`, `set_version`, `set_developer_name`, `set_website`, `add_legal_section` | yes | §2.8 |
| `adw::BreakpointBin::add_breakpoint`, `BreakpointCondition::new_length`, `Breakpoint::add_setter`, `adw::Spinner`, `adw::StyleManager::{is_dark, connect_dark_notify}` | yes | §5, §10 |
| libadwaita 1.9 CSS variables `--window-bg-color`, `--view-bg-color`, `--headerbar-bg-color`, `--popover-bg-color`, `--dialog-bg-color`, `--sidebar-bg-color`, `--accent-bg-color` (+ Ubuntu's `--yaru-accent-bg-color`) | strings in `libadwaita-1.so.0` | §10 |
| `webkit::NetworkSession::new_ephemeral`, `WebView::builder().network_session(..)` | yes | §4.1 |
| `WebView`: `call_async_javascript_function_future`, `evaluate_javascript_future`, `snapshot_future`, `set_zoom_level`, `execute_editing_command`, `terminate_web_process`, `connect_context_menu`, `connect_web_process_terminated(WebProcessTerminationReason::{Crashed, ExceededMemoryLimit, TerminatedByApi})` | yes | §4, §7 |
| `ContextMenu::{items, remove}`, `ContextMenuItem::stock_action`, `ContextMenuAction::{Copy, SelectAll}` | yes | §4.6 |
| `PrintOperation::{print, run_dialog, set_page_setup, set_print_settings, connect_finished, connect_failed}` | yes | §7.2, §7.3 |
| `Settings`: the setters of §4.1; `set_enable_dns_prefetching` deprecated since 2.48 | yes | §4.1 |
| `soup::Session::{new, send_and_read_future, set_timeout}`, `Message::set_flags`, `MessageFlags::NO_REDIRECT`; `webkit6` re-exports `soup` | yes | §7.5 |
| `glib::{user_config_dir, user_state_dir, user_data_dir, user_runtime_dir, uuid_string_random, KeyFile::{save_to_file, boolean, string, set_boolean, set_string}, idle_add_local_full, timeout_add_local_once}`, `gio::spawn_blocking`, `gio::File::{monitor_file, move_}` | yes | §1, §5, §6, §9 |
| `glib` Unix signal sources | **not in glib-rs 0.22** | §1.5 (no SIGTERM handler) |
| `gdk::Surface::scale() -> f64`, `gtk::Native::surface()`, `gdk::Texture::save_to_png_bytes`, `gdk::FileList`, `gtk::DropTarget` | yes | §6.1, §7.5 |
| `WebView::{connect_load_failed, connect_zoom_level_notify}`, `adw::ToolbarView::set_reveal_top_bars`, `adw::Application::builder().register_session(..)` | yes | §1.1, §4, §5.3 |
| GNOME `switch-to-workspace-up/down` = `<Control><Alt>Up/Down` | `org.gnome.desktop.wm.keybindings` (gsettings-desktop-schemas 50.0) | §2.8 |
| Adwaita symbolic icons `document-edit`, `view-dual`, `view-reveal`, `view-restore`, `go-up`, `go-down`, `view-list-bullet`, `accessories-text-editor`, `document-send`, `folder-new`, `list-add`, `sidebar-show`, `text-x-generic`, `x-office-address-book`, `dialog-warning`, `pan-end`, `pan-down` | present in `adwaita-icon-theme` on 26.04 (`accessories-text-editor` under `legacy/`) | §5, §8 |

## Appendix B — scripts the host injects or evaluates

From md-logic `preview::Scripts` (append-only, as in md.win): `SCROLL_SYNC` (Mac text, unchanged),
`LINK_GUARD`, `CAPTURE_HTML`, `RICH_ELEMENTS`, `diagram_svg(index)`, `RENDER_COMPLETE`, `SCROLL_Y`,
`SCROLL_HEIGHT`, `scroll_to(y)`, `sync_scroll_to(f)`, `jump_to_slug(slug)`. The shell adds two of its
own, kept in `web/`: the snapshot scroll (`window.scrollTo(0, y); window.scrollY`) and the two-frame
wait (`await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)))` through
`call_async_javascript_function_future`). Numbers are formatted with Rust's `{}` (never an exponent
for these magnitudes); results are JSON-decoded by `JsonScript`; nothing in `md-init.js` or `rich/`
is edited.
