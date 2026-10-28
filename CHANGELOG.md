# Changelog

All notable changes to this project are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The number this file tracks is the md family's, shared by every md edition,
and not a count of this port's releases. md.win began at its own 1.0 and
joined the family's number at the next family release, 1.5 — its CHANGELOG
tells that story. The Linux edition arrives after that decision and simply
starts where the family is: **its first release is 1.6**, with everything the
other editions have through their own 1.6. Nothing is missing before it. The
`.deb` carries the number as `1.6.0`, the workspace version in `Cargo.toml`
that `scripts/package.sh` reads; a rebuild that changes no behaviour is not
tracked here.

## [1.6] — 2026-10-27

### Added

- **md joins the family on Linux.** The first release for Ubuntu 26.04 LTS,
  amd64 and arm64, with the functionality of md for macOS 1.6, written in
  Rust on GTK 4, libadwaita and WebKitGTK: the hand-written Markdown
  renderer — headings, lists and task lists, fenced code with
  syntax-highlighting, block quotes, tables, CSV / TSV blocks drawn as
  tables, thematic and page breaks, images, footnotes gathered under a rule
  and numbered in reading order, YAML / TOML front matter kept off the page
  — with TeX/LaTeX math and mhchem chemistry, Mermaid, Graphviz and
  PlantUML diagrams drawn on the computer by the bundled engines (KaTeX
  0.17.0 with mhchem, Mermaid 11.16.0, Graphviz 14.1.1 through Viz.js
  3.24.0, PlantUML 1.2026.4beta4, highlight.js 11.11.1), and the
  ` ```plot ` block that needs none of them. The parser, the HTML writer and
  its stylesheet, the plot engine and the HTML / EPUB / SVG / LaTeX /
  TextBundle writers live in `crates/md-core` and are pinned byte for byte
  to the fixtures every md port shares — the same document exported here
  and on a Mac is one file.
- **Documents, and every spelling md owns.** Open, edit and save `.md`,
  `.markdown`, `.mdown`, `.markdn`, `.mdtext`, `.mdtxt`, `.mkd`, `.mkdn`,
  `.mdwn` and `.mkdown`, plain text (`.txt`, `.text`), PlantUML (`.puml`,
  `.plantuml`, `.iuml`, `.pu`) and Graphviz (`.gv`) through GTK's dialogs,
  saved back in the encoding and line endings they arrived in. Ubuntu's
  shared-mime-info knows Markdown only as `.md`, `.mkd` and `.markdown`, so
  the package teaches it the other seven spellings, `.text`, a
  `text/x-plantuml` type and an `application/x-textpack` type, and a
  double-click in Files reaches md for every one of them. `.dot` stays
  unclaimed in md's own dialogs, as on macOS and Windows. A `.textpack`
  opens by double-click and a `.textbundle` folder through **File ▸ Open
  TextBundle Folder…**, by dropping it on a window or with
  `md x.textbundle`, imported as its text and never written back. md is a
  single instance: a second launch hands its files to the running one, and
  a file already open brings its window forward.
- **Autosave that cannot clobber, and drafts that are kept.** One second
  after the last keystroke the file is written in place, after checking
  that its modification time and size are the ones md last saw; a file
  changed elsewhere under unsaved edits raises **Reload from Disk** / **Keep
  My Version** instead. A save that fails while a window closes keeps the
  text beside the document as `<name> (rescued)`. Untitled documents are
  autosaved too, as drafts in `~/.local/share/md/drafts`, so **File ▸ Quit**
  and logging out ask nothing and the next launch brings every document and
  draft back in its layout and window size. Window positions are not
  restored: Wayland lets the compositor place windows.
- **Rename…, Move To…, Duplicate and Revert to Saved** in the File menu,
  and **Open Recent** from the desktop's shared recent list — md's ten
  newest, with **Clear Menu** emptying md's menu without touching any other
  application's history.
- **Layouts, Zen and the writer's tools.** Edit / Split / Preview
  (Ctrl+1 / Ctrl+2 / Ctrl+3), remembered for each of the 200 most recently
  opened files; Split stacks below 640 pixels. Zen Mode (Shift+Ctrl+Return)
  with the floating write / read switch. **Go ▸ Contents** and **Go ▸
  Notes**, the word · character count, and md's own **Find and Replace**
  bar (Ctrl+F, Ctrl+H, Ctrl+G, Shift+Ctrl+G, Ctrl+E) — in book windows as
  well as document windows, as on the Mac.
- **Typing and the Format menu, as every md 1.6 has them.** Enter continues
  lists, task lists, quotes and tables; sentences capitalize themselves,
  Markdown-aware, with the delete-and-retype escape and a separate undo
  step; Bold (Ctrl+B), Italic (Ctrl+I), Code (Shift+Ctrl+C), Strikethrough
  (Shift+Ctrl+X), Link (Ctrl+K), Heading 1–6 (Ctrl+Alt+1–6), Bullet List
  (Shift+Ctrl+L), Task List (Shift+Ctrl+T), Quote (Shift+Ctrl+Q), Indent and
  Outdent (Ctrl+] and Ctrl+[); typing `*`, `_`, `` ` `` or `$` over a
  selection wraps it. The three switches live under **Edit ▸ Typing**.
- **Print and export.** Print through GTK's print dialog; PDF at seven page
  sizes with half-inch margins; one self-contained HTML page; EPUB 3 with
  the front matter's `author`, `language`, `date`, `description`, `series`
  / `series_index` and `cover`; LaTeX; a diagram as SVG — large PlantUML
  diagrams included, past the engine's old 4096-pixel gate; and TextBundle.
  Linux has no share sheet, so there is no Share menu.
- **Writer mode.** A book is a folder of chapters and articles, in a window
  with a resizable sidebar, compiled to PDF, EPUB or LaTeX, or printed.
  Previous / Next Article are **Alt+Up / Alt+Down**, because GNOME keeps
  Ctrl+Alt+Up / Down for switching workspaces.
- **At home on the desktop.** Paper and carbon-paper palettes that follow
  the system's light or dark style, prose in Nimbus Mono PS and code in
  Liberation Mono from Ubuntu's own font packages (md bundles no font), the
  Mac's app icon, a desktop entry with a **New Document** action, AppStream
  metadata for the software centres, and a manual page.
- **The package.** `md_1.6.0_amd64.deb` and `md_1.6.0_arm64.deb`, built
  reproducibly — two builds of one commit give one SHA-256 — in a pinned
  `ubuntu:26.04` image. It installs `/etc/apparmor.d/md`, a profile that
  confines nothing and only lets WebKitGTK's sandbox create the user
  namespace Ubuntu 26.04 otherwise refuses it, as Ubuntu does for its own
  WebKitGTK applications. Its copyright file names, for each bundled
  engine, what the file states and which upstream release it is — the first
  time an md edition publishes those statements in its package.
