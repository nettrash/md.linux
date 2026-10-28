# md for Linux

The simplest Markdown editor for Ubuntu. Write Markdown on one side and see
it rendered on the other — or switch to a full-window **Edit** or
**Preview**. Written in Rust on GTK 4, libadwaita and WebKitGTK, with a
hand-written Markdown renderer, and shipped as a `.deb` for Ubuntu 26.04 LTS
on amd64 and arm64. **No accounts, no servers** — your files live wherever
you keep them. The Rust side builds on the gtk-rs bindings (`gtk4`,
`libadwaita`, `webkit6`, `glib`, `gio`) and a handful of small crates from
crates.io, and the vendored code is the offline math / diagram engines under
`rich/` (KaTeX with the mhchem chemistry extension, Mermaid, Graphviz,
PlantUML, and highlight.js for code); `/usr/share/doc/md/copyright` names
every one of them and its licence.

> This is the Linux port of [**md**](https://github.com/nettrash/md), the
> iPhone / iPad editor, and of its
> [macOS](https://github.com/nettrash/md.macOS),
> [Android](https://github.com/nettrash/md.Android),
> [Windows](https://github.com/nettrash/md.win) and
> [VS Code](https://github.com/nettrash/md.vscode) siblings, with the
> functionality of md for macOS. All six share the same hand-written block
> parser, renderer, themed HTML export and the HTML / EPUB / SVG / LaTeX /
> TextBundle writers — pinned to one set of golden fixtures byte for byte —
> and this port reimplements them in Rust, in `crates/md-core`. The
> difference worth knowing is where the bytes stop: Ubuntu has no American
> Typewriter, and its WebKit is WebKitGTK. Everything md-core *writes* is the
> family's output to the byte, but the on-screen preview and the printed
> page set the prose in **Nimbus Mono PS** (URW's Courier, which Ubuntu
> ships) through a style rule the app adds to the screen and paper pages
> only, and PDFs are paginated by WebKitGTK — same page sizes, same content,
> a different rasterizer. And Linux has no `NSDocument`, so what the Mac gets
> from its title bar — **Rename**, **Move To**, **Duplicate**, **Revert to
> Saved** — is written into the File menu here, and autosave is a one-second
> timer that writes the file in place behind a clobber guard.

## Features

- **Document-based.** Open, edit and save `.md` / `.markdown` files — and
  every other spelling the md family owns: `.mdown`, `.markdn`, `.mdtext`,
  `.mdtxt`, `.mkd`, `.mkdn`, `.mdwn` and `.mkdown` — through GTK's Open and
  Save dialogs (the desktop's own file chooser on GNOME), with autosave one
  second after the last keystroke, written **in place** so the file keeps
  its inode, owner, mode and hard links, and only after checking that
  nothing else changed the file first. **Rename…**, **Move To…**,
  **Duplicate** and **Revert to Saved** sit in the File menu; **Open
  Recent** shows md's ten newest entries of the desktop's shared recent
  list (the one GNOME Files shows), and **Clear Menu** empties md's menu
  without touching other applications' history; files can be dropped on
  any window; and a double-click in Files lands in the running app — md is
  a single instance, and a file that is already open brings its window
  forward instead of opening twice. Plain-text files (`.txt`, `.text`) open
  too and keep their extension, and every file is saved back in the
  encoding and line endings it arrived in (UTF-8 with or without a
  byte-order mark, UTF-16 only behind one, Windows-1251). A **TextBundle**
  (`.textbundle` — a folder, so it opens through **File ▸ Open TextBundle
  Folder…**, by dropping the folder on a window or with `md x.textbundle`;
  a folder cannot be given a file type) or **TextPack** (`.textpack`, by
  double-click) — the Markdown-with-images container Ulysses, iA Writer and
  Bear write — opens too, imported as its text for editing (the bundle's
  own `assets/` images aren't shown in the preview, and the bundle is never
  written back, so its assets are never lost).
- **Nothing lost.** If a file changes on disk under unsaved edits nothing is
  clobbered: a bar offers **Reload from Disk** or **Keep My Version**. If a
  save fails while a window is closing, the text is kept beside the
  document as `<name> (rescued)` with the original extension — `Notes
  (rescued).md` — and the window says so. Untitled documents are autosaved
  as **drafts** too, so **File ▸ Quit** (Ctrl+Q) and logging out ask
  nothing: the next launch reopens the documents and drafts you had open,
  each in its layout and window size. Closing one untitled window by hand
  still asks whether to save it.
- **Live preview.** A built-in renderer covers the everyday Markdown you
  actually write:
  - Headings (`#`–`######`)
  - **Bold**, *italic*, `inline code`, [links](https://nettrash.me) and
    ~~strikethrough~~
  - Bullet, numbered and **task lists** (`- [ ]` / `- [x]`), with nesting
  - Fenced code blocks (```` ``` ```` and `~~~`), with horizontal scroll —
    **syntax-highlighted** in md's own quiet paper palette when the fence
    names a language (`rust`, `js`, …); a bare fence stays plain
  - Block quotes (including nested)
  - GitHub-style tables, with column alignment
  - **CSV / TSV blocks** (` ```csv `, ` ```tsv `) — data pasted straight
    out of a spreadsheet drawn as a table, quoted fields and all, with
    all-number columns lined up on the right
  - Thematic breaks (`---`) and page breaks (`\newpage` / `\pagebreak`)
  - YAML / TOML **front matter** (`---` … `---` or `+++` … `+++`) at the
    very top of a file — recognised as metadata and hidden from the page,
    print and PDF
  - **Footnotes** (`[^id]` in the text, `[^id]: the note` on a line of its
    own) — gathered under a rule at the foot of the rendered page and
    numbered in the order a reader meets them, each reference linking down
    to its note and each cited note linking back
- **A deliberate subset, not a CommonMark engine.** The renderer is the
  family's renderer and inherits its omissions on purpose: raw HTML is
  escaped rather than passed through, there are no reference-style links, a
  four-space indent is a paragraph continuation rather than a code block,
  and a single `$` in prose is left alone (`$5 and $10` is money, not math).
- **Math and diagrams.** TeX/LaTeX math (`$…$`, `$$…$$` and ` ```math `) —
  with **chemistry** notation (`\ce{…}` / `\pu{…}`) via the bundled mhchem
  extension — plus **Mermaid** (` ```mermaid `), **Graphviz** (` ```dot `,
  ` ```graphviz ` or ` ```gv `, and every layout program — `neato`, `circo`,
  `fdp`, `sfdp`, `twopi`, `osage`, `patchwork` — usable as the block
  language) and **PlantUML** (` ```plantuml `), all drawn on your computer
  by the vendored engines inside WebKitGTK and carried through to print and
  PDF. A raw `.puml` / `.plantuml` / `.iuml` / `.pu` or `.gv` file opens and
  renders as the diagram it describes, source still editable. md's own Open
  dialog never offers `.dot`, which the family leaves to Word templates;
  Ubuntu itself types a `.dot` as Graphviz too, so one that reads as a
  graph lists md under **Open With**.
- **Plots** (` ```plot `). Write a function and md draws it — one line per
  curve, with optional `x` / `y` ranges, `title`, `xlabel`, `ylabel`,
  `legend`, `grid`, `axes`, `width`, `height` and `samples` above them.
  Curves can be named, parametric (`(cos(t), sin(t)) for t in 0..2*pi`) or
  plain data (`measured = points: 0,0 1,2 2,1`). Alone among the rich
  blocks it bundles **no engine at all**: the chart is a real vector
  `<svg>` in the page before any script runs, so it works in the preview,
  in print, in PDF, in an exported HTML page, in an EPUB and in Export
  Diagram as SVG…
- **Three layouts.** *Edit*, *Split* (side by side, re-rendering as you
  type and scrolling as one — it stacks vertically when the window is
  narrower than 640 pixels) and *Preview*, chosen from the **View** menu
  (Ctrl+1 / Ctrl+2 / Ctrl+3). The layout is remembered **for each file**,
  so a document comes back in the one you left it in; the 200 most recently
  opened are kept, on this computer, under a short hash of each path. A
  file md has not seen opens in Split, a new document in Edit; books keep
  one layout throughout.
- **Zen mode.** **View ▸ Zen Mode** (Shift+Ctrl+Return) takes the window
  full screen and leaves one column two-thirds of the screen wide on bare
  paper, with a quiet floating switch between writing and reading that
  fades when the pointer rests; Esc or leaving full screen (F11) drops it.
- **Writer tools.** **Go ▸ Contents** lists every heading and jumps both
  panes to it; **Go ▸ Notes** lists your private author notes — an HTML
  comment on a line of its own whose text begins with `note:` — and jumps
  the editor to one; they never reach the preview, the PDF or the printout.
  A live **word · character** count sits under every page. A **Find and
  Replace** bar across the top of the editor (Ctrl+F, Ctrl+H to replace,
  Ctrl+G / Shift+Ctrl+G for the next and previous match, Ctrl+E for the
  selection) works in document and book windows alike. Matching is
  case-insensitive and wraps round the end; there are no regular
  expressions. **Replace All** is a single undo step.
- **Typewriter feel.** Warm paper background (light "fresh paper" / dark
  "carbon paper", following the system's style) with prose in **Nimbus Mono
  PS** and code in **Liberation Mono** — the Ubuntu stand-ins for the
  family's American Typewriter and Courier New, both from Ubuntu's own font
  packages; md bundles no font. The shared stylesheet stays byte-identical
  with the other ports; the app adds the typewriter rule to the screen and
  print pages only, so every export carries the family's font stack
  untouched.
- **Typing.** Enter on a list item starts the next one — bullets, numbers
  (counting on), task boxes and quotes alike — and on a table row starts
  the next row; Enter on an item you left empty ends the list instead. The
  first letter of a line and of a sentence is capitalized as you type it,
  in every cased script, and never inside a code fence, a table, inline
  code, math, a link address or a path; abbreviations (`e.g.`, `т.д.`) do
  not count as sentence ends. Delete the capital md wrote and type the
  letter again; it stays lowercase — that is how `md`, `iOS` or `npm` start
  a sentence — and Ctrl+Z right after a capital restores the lowercase
  letter as its own undo step. Shift+Enter is always a plain newline. Both
  helpers are switches under **Edit ▸ Typing**, on by default, the same two
  switches on every md port.
- **Formatting.** A **Format** menu and the family's shortcuts: **Bold**
  (Ctrl+B), **Italic** (Ctrl+I), **Code** (Shift+Ctrl+C),
  **Strikethrough** (Shift+Ctrl+X), **Link** (Ctrl+K), **Heading 1–6**
  (Ctrl+Alt+1–6), **Bullet List** (Shift+Ctrl+L), **Task List**
  (Shift+Ctrl+T), **Quote** (Shift+Ctrl+Q), **Indent** / **Outdent**
  (Ctrl+] / Ctrl+[). Each is one Ctrl+Z. Typing `*`, `_`, `` ` `` or `$`
  with a selection wraps it instead of replacing it (**Edit ▸ Typing ▸ Wrap
  Selection When Typing \* _ ` $**, on by default); one Ctrl+Z leaves just
  the typed character, a second brings the selection back. The rules are
  the same on every md app, pinned by one shared set of test vectors
  (`docs/formatting.md`).
- **Print and export.** Print the *rendered* document (**File ▸ Print…**,
  Ctrl+P) through GTK's print dialog. Export it as a PDF at A4 (the
  default), A5, US Letter or US Legal, or a print-on-demand trim size
  (6 × 9″, 5 × 8″, 5.5 × 8.5″) — white paper and dark ink whatever the
  theme, 11 pt, half-inch margins, `\newpage` starting a fresh page; as one
  self-contained `.html` file; as an **EPUB** with the document's headings
  as its table of contents and its front matter's `author`, `language`,
  `date`, `description`, `series` / `series_index` and `cover` as the
  book's metadata; as LaTeX `.tex` source; as a single **diagram** in a
  standalone `.svg`; or as a **TextBundle** folder with the local images it
  references gathered into `assets/`. Linux has no share sheet, so there is
  no Share menu.
- **Writer mode: books.** A book is a folder — subfolders are chapters,
  Markdown files are articles, ordered by a numeric prefix and then by name.
  **Book ▸ New Book…**, **Open Book…**, **Show Book** (Shift+Ctrl+B) and
  **Close Book**; the book window keeps the structure in a resizable
  sidebar and the selected article beside it in Edit / Split / Preview,
  with its own Contents, Notes, find bar and word count, saving as you
  type. Right-click to **Rename**, **Move Up**, **Move Down** or **Delete**
  chapters and articles — reordering renumbers the group and writes the
  order back into the file names. Walk the manuscript with Alt+Up /
  Alt+Down (GNOME keeps Ctrl+Alt+Up / Down for workspaces), take it full
  screen (F11) for a writing room, and compile the whole book to one PDF,
  print it, or export it as **EPUB 3** or a `book`-class LaTeX file.
- **A preview that comes back.** If the preview's web process dies, md
  reloads it; if the same document kills it twice in a row it stops and
  says so in one line — the next edit tries again.
- **Light / dark** throughout, following the desktop. The only things md
  fetches from the network are an image your own document points at by URL
  and the `https:` cover an EPUB export's front matter names — see
  [PRIVACY.md](PRIVACY.md).

## Platforms

- Ubuntu **26.04 LTS** on **amd64** and **arm64**, GNOME on Wayland first;
  other desktops of 26.04 (on Wayland or X11) through GTK.
- Fedora **44** and later on **x86_64** and **aarch64** (the `.rpm`), and
  any distribution with GTK 4.22, libadwaita 1.9, WebKitGTK 6.0 2.52 and
  glibc 2.39 or newer (the portable `.tar.gz`).
- The preview's web content runs inside WebKitGTK's sandbox, which needs
  an unprivileged user namespace. Ubuntu 26.04 grants that only to programs
  an AppArmor profile names, so the package installs
  `/etc/apparmor.d/md` — a profile that confines nothing and only allows
  `userns`, exactly as Ubuntu does for its own WebKitGTK applications.

## Install

Each release on GitHub carries six packages and a `SHA256SUMS` file.

```bash
sudo apt install ./md_1.6.0_amd64.deb                # Ubuntu 26.04; or md_1.6.0_arm64.deb
sudo dnf install ./md-1.6.0-1.fc44.x86_64.rpm        # Fedora 44+;  or .aarch64.rpm
tar -xzf md-1.6.0-linux-x86_64.tar.gz && md-1.6.0/bin/md   # elsewhere; or -aarch64
```

apt brings GTK 4, libadwaita, WebKitGTK and the two font packages from the
Ubuntu archive. `sudo apt remove md` removes the app and keeps the AppArmor
profile (a configuration file); `sudo apt purge md` removes that too. Neither
touches your documents or md's settings, session and drafts in your home
folder. dnf likewise brings the libraries and fonts, and `sudo dnf remove md`
removes the app. The `.tar.gz` holds a `README` with the libraries to install
and how to add md to the app grid from `~/.local`.

## Build

Everything builds in Docker, in the `ubuntu:26.04` image `docker/Dockerfile`
describes (the base image is pinned by digest and the archive by an Ubuntu
snapshot date, so a rebuild gets the same compiler and libraries). The two
libraries also build and test natively anywhere Rust 1.93 runs, and on
Ubuntu 26.04 itself the app and the `.deb` build without Docker (below).

```bash
# The parity core and the shell's logic, any OS (the workspace's default members).
cargo test

# In the ubuntu:26.04 image: fmt, clippy -D warnings and every test, the GTK app included.
scripts/dev.sh image
scripts/dev.sh test

# Run md headless under Xvfb and take a screenshot.
scripts/dev.sh run

# The .deb and the .tar.gz: dist/md_1.6.0_<arch>.deb and
# dist/md-1.6.0-linux-<x86_64|aarch64>.tar.gz, the .deb installed, checked and
# removed again in a fresh ubuntu:26.04 container (amd64 runs under emulation on
# an arm64 machine). `scripts/dev.sh package` does the same.
scripts/package.sh amd64 arm64

# The .rpm: dist/md-1.6.0-1.fc44.<x86_64|aarch64>.rpm, built in Fedora 44 and
# installed, checked and removed again in a fresh one.
scripts/package-rpm.sh amd64 arm64
```

`scripts/package.sh` packages HEAD as committed (`--allow-dirty` packages the
working tree instead) with `SOURCE_DATE_EPOCH` set to the commit time, so two
builds of one commit give one SHA-256 (`--clean` builds in an empty target
volume, to prove it). Before declaring success it installs the package with
apt, checks `md --version`, the file type of every extension md opens,
`desktop-file-validate`, `appstreamcli validate`, the AppArmor profile and
lintian, then removes and purges it. `docs/packaging.md` describes the
package and the manual test on a real Ubuntu desktop.

### On Ubuntu 26.04, without Docker

```bash
sudo apt install build-essential pkg-config git rustc cargo \
     libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev
cargo run -p md                     # run md from the source tree
scripts/package.sh --native         # dist/md_1.6.0_<this machine's arch>.deb
sudo apt install ./dist/md_1.6.0_amd64.deb
```

`--native` builds the package for the machine's own architecture with its
own compiler and libraries, checks its layout, and runs lintian when it is
installed (`sudo apt install lintian`). It does not install the package for
you. While the machine's packages are those of the Docker image's snapshot,
the `.deb` is byte-identical to the Docker build's.

## Project layout

| Path | What lives there |
| --- | --- |
| `crates/md-core` | The parity core, std and `unicode-segmentation` only: block parser and inline renderer, the themed HTML writer and its stylesheet, the plot engine, the HTML / EPUB / LaTeX / TextBundle exporters, the book model, the per-file view-mode memory and the text codec. Pinned byte for byte against the fixtures shared with the other ports. |
| `crates/md-logic` | The shell's logic without a line of GTK: the command table and shortcuts, activation routing, the document session (autosave, clobber guard, rescue copies), the view-mode and Zen controllers, the typing hooks' decisions, the export pipeline, the book navigator, the settings keys and the palette. |
| `app` | The GTK 4 / libadwaita / WebKitGTK shell, binary `md`. |
| `rich/` | The engines, byte-identical to every other md port's copy. |
| `examples/` | The bundled examples and the Example Book. |
| `packaging/` | The desktop entry, MIME types, AppStream metadata, icons, AppArmor profile, manual page, copyright and control template of the `.deb`; `rpm/md.spec`; the `.tar.gz`'s `README`. |
| `docker/`, `scripts/` | The Ubuntu and Fedora build images, `dev.sh` (build, test, run, self-test), `package.sh` (`.deb`, `.tar.gz`) and `package-rpm.sh`. |
| `.github/workflows/release.yml` | On a `v*` tag: the tests, the self-test, and the six packages attached to the release. |
| `docs/` | The port plan, the measured Linux facts, the shell design, the packaging, and the checklist for a real Ubuntu desktop (`docs/desktop-checklist.md`). |

## License

MIT — see [LICENSE](LICENSE). © 2026 nettrash. It is md's own licence
only: the engines under `rich/` keep theirs, and the package's
`/usr/share/doc/md/copyright` (`packaging/copyright`) states, for each one,
what the file itself says and which upstream release it is. What travels
with an **Export ▸ HTML…** page are the two notices it owes for what it
embeds: KaTeX's MIT notice with the fonts' SIL Open Font License notice
when the KaTeX faces are inlined, and Mermaid's MIT notice when a Mermaid
diagram carries its theme stylesheet into the page.
