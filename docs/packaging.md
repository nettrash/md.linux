# md for Linux — the `.deb`

md 1.6.0 for Ubuntu 26.04 ("resolute") ships as one binary package, `md`, for **amd64** and
**arm64**, built with `dpkg-deb` in the `ubuntu:26.04` development image — reproducibly, from the
same tree, for both architectures. This document fixes the file layout, every integration file, the
control data, the build, lintian's expectations and the manual test on a real 26.04 desktop. The
facts it relies on were checked in the image on 2026-10-27 (versions below) or are cited from
`docs/linux-facts.md` ("facts §n"). Work package P6 (`docs/shell-work-packages.md`) implements it.

---

## 0. Decisions

| Topic | Decision | Why |
| --- | --- | --- |
| Tooling | `dpkg-deb --root-owner-group` (dpkg) plus `dpkg-shlibdeps` (`dpkg-dev`, already in the image via `build-essential`); no debhelper, no fakeroot | The brief: no new tools; `--root-owner-group` makes every entry root-owned without fakeroot |
| Version | `1.6.0` (native: no Debian revision) | The family's number; the package is its own upstream |
| Architectures | `arm64` natively on Apple silicon, `amd64` under `docker buildx` / QEMU (`docker buildx ls`: both builders list `linux/amd64` and `linux/arm64`) | One Dockerfile, two platforms; no cross toolchain to maintain |
| Reproducible | pinned base image digest + an Ubuntu snapshot date, `SOURCE_DATE_EPOCH` = the commit time, `--remap-path-prefix`, `cargo --locked`, normalised mtimes and modes, `gzip -9n` | Two builds of one commit give one SHA-256 |
| Desktop id | `me.nettrash.md` — the GTK application id, the `.desktop` name, the icon name, the metainfo id | GNOME matches windows to the launcher by `app_id` |
| Sandbox | `/etc/apparmor.d/md` grants `userns` (unconfined profile), as Ubuntu does for every WebKitGTK app | Without it WebKit's bubblewrap fails and aborts md (facts §9) |
| Fonts | `Depends: fonts-urw-base35, fonts-liberation` | Nimbus Mono PS and Liberation Mono are md's faces (facts §7) |

---

## 1. File layout

| Path | Mode | Source in the tree | Notes |
| --- | --- | --- | --- |
| `/usr/bin/md` | 0755 | `target/release/md`, stripped | built without the `selftest` feature |
| `/usr/share/md/rich/**` | 0644 / dirs 0755 | `rich/` (byte-identical to md.win's) | ≈ 13 MB; KaTeX fonts stay in `rich/fonts/` because `katex.min.css` loads `url(fonts/…)` |
| `/usr/share/md/examples/*.md`, `…/examples/Example Book/**` | 0644 | `examples/` (identical to md.macOS's) | File ▸ Examples and Example Book… |
| `/usr/share/applications/me.nettrash.md.desktop` | 0644 | `packaging/me.nettrash.md.desktop` | §2 |
| `/usr/share/mime/packages/me.nettrash.md.xml` | 0644 | `packaging/me.nettrash.md.xml` | §3 |
| `/usr/share/metainfo/me.nettrash.md.metainfo.xml` | 0644 | `packaging/me.nettrash.md.metainfo.xml` | §5 |
| `/usr/share/icons/hicolor/<size>/apps/me.nettrash.md.png` | 0644 | `packaging/icons/hicolor/…` (generated once, committed) | §4 |
| `/etc/apparmor.d/md` | 0644, conffile | `packaging/apparmor/md` | §7 |
| `/usr/share/man/man1/md.1.gz` | 0644 | `packaging/md.1` → `gzip -9n` | lintian wants a manual page |
| `/usr/share/doc/md/copyright` | 0644 | `packaging/copyright` | §6 |
| `/usr/share/doc/md/changelog.gz` | 0644 | `packaging/changelog` (Debian format) → `gzip -9n` | native package's changelog |
| `/usr/share/doc/md/CHANGELOG.md.gz`, `README.md.gz`, `PRIVACY.md.gz` | 0644 | the repo's files → `gzip -9n` | the user-facing history and the privacy statement |
| `/usr/share/lintian/overrides/md` | 0644 | `packaging/lintian-overrides` | §8 |
| `DEBIAN/control`, `md5sums`, `conffiles`, `postinst`, `postrm` | 0644 / scripts 0755 | `packaging/control.in`, scripts | §7, §8 |

The binary finds its data through `Paths` (shell-design §9.3): `$MD_DATA_DIR`, else `/usr/share/md`.
Nothing is installed under `/usr/lib`, `/var` or `/opt`; nothing in the user's home is created by the
package.

Dependencies on the triggers that keep the desktop's caches fresh come for free: on 26.04
`shared-mime-info` (`/usr/share/mime/packages`), `desktop-file-utils` (`/usr/share/applications`)
and the GTK icon cache (`/usr/share/icons/hicolor`) declare dpkg file triggers, so the package ships
**no** maintainer-script code for them.

---

## 2. The desktop entry — `me.nettrash.md.desktop`

```ini
[Desktop Entry]
Type=Application
Name=md
GenericName=Markdown Editor
Comment=Write Markdown and see it rendered
Icon=me.nettrash.md
Exec=md %F
TryExec=md
Terminal=false
Categories=Utility;TextEditor;
Keywords=markdown;editor;preview;writing;book;latex;mermaid;plantuml;
MimeType=text/markdown;text/plain;text/x-plantuml;text/vnd.graphviz;application/x-textpack;
StartupNotify=true
StartupWMClass=me.nettrash.md
Actions=new-document;

[Desktop Action new-document]
Name=New Document
Exec=md --new-window
```

- `%F`: local paths (GIO hands remote locations over its FUSE mount); md works on paths.
- `MimeType` covers every extension md's loader opens (§3) — pinned by `app/tests/associations.rs`
  in both directions, like md.win's `FileAssociationTests`. md lists `text/markdown` itself (the
  Mac's **owner** rank) and is an **alternate** for plain text, PlantUML and Graphviz. Which
  application a double-click picks when several claim a type is the desktop's default list and the
  user's `mimeapps.list`; md writes neither (the manual test records what a stock 26.04 picks, §11).
- `--new-window` activates `app.new` in the running instance (shell-design §1.1).
- Validated by `desktop-file-validate` (desktop-file-utils 0.28) in the test container.

---

## 3. MIME types — `me.nettrash.md.xml`

What Ubuntu 26.04's `shared-mime-info` 2.4 already maps (read from
`/usr/share/mime/packages/freedesktop.org.xml` in the image):

| Extension | 26.04 type | md needs |
| --- | --- | --- |
| `.md`, `.mkd`, `.markdown` | `text/markdown` | — |
| `.mdown`, `.markdn`, `.mdtext`, `.mdtxt`, `.mkdn`, `.mdwn`, `.mkdown` | **none** | globs added to `text/markdown` |
| `.txt` | `text/plain` | — |
| `.text` | **none** | glob added to `text/plain` |
| `.puml`, `.plantuml`, `.iuml`, `.pu` | **none** | new `text/x-plantuml` |
| `.gv` | `text/vnd.graphviz` (also `*.dot`) | — |
| `.dot` | `text/vnd.graphviz` *and* `application/msword-template` | not claimed by md (the family's rule); see below |
| `.textpack` | **none** | new `application/x-textpack` |
| `.textbundle` | a folder (`inode/directory`) | cannot be associated |

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!-- md: the Markdown spellings, PlantUML and TextPack that shared-mime-info 2.4 does not know. -->
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="text/markdown">
    <glob pattern="*.mdown"/>
    <glob pattern="*.markdn"/>
    <glob pattern="*.mdtext"/>
    <glob pattern="*.mdtxt"/>
    <glob pattern="*.mkdn"/>
    <glob pattern="*.mdwn"/>
    <glob pattern="*.mkdown"/>
  </mime-type>
  <mime-type type="text/plain">
    <glob pattern="*.text"/>
  </mime-type>
  <mime-type type="text/x-plantuml">
    <comment>PlantUML diagram</comment>
    <sub-class-of type="text/plain"/>
    <glob pattern="*.puml"/>
    <glob pattern="*.plantuml"/>
    <glob pattern="*.iuml"/>
    <glob pattern="*.pu"/>
  </mime-type>
  <mime-type type="application/x-textpack">
    <comment>TextPack document</comment>
    <sub-class-of type="application/zip"/>
    <glob pattern="*.textpack"/>
  </mime-type>
</mime-info>
```

Adding globs to an existing type from a package file is how shared-mime-info merges; nothing of
freedesktop's definitions is replaced. Because md must list `text/vnd.graphviz` to be offered for
`.gv`, a `.dot` file that sniffs as Graphviz also lists md under Open With; md opens it like any text
file it is handed. The `.dot` rule of the family (unclaimed, Word's on Windows) is kept where md has
a choice: its own loader and Open dialog never list `.dot`.

---

## 4. Icons

The Mac's `AppIcon.appiconset` (`md.macOS/md/Assets.xcassets/AppIcon.appiconset/`) is the source:
ten PNGs, full-bleed squares, measured sizes 16, 32, 32, 64, 128, 256, 256, 512, 512, 1024 px.
`scripts/make-icons.sh` (run once by hand, output committed under `packaging/icons/`) copies them
into the hicolor layout — every size but two is a byte copy:

| hicolor directory | File | Source |
| --- | --- | --- |
| `16x16/apps` | `me.nettrash.md.png` | `icon_16x16.png` |
| `16x16@2/apps` | | `icon_16x16@2x.png` (32 px) |
| `24x24/apps` | | ImageMagick from `icon_32x32@2x.png` |
| `32x32/apps` | | `icon_32x32.png` |
| `32x32@2/apps` | | `icon_32x32@2x.png` (64 px) |
| `48x48/apps` | | ImageMagick from `icon_128x128.png` |
| `64x64/apps` | | `icon_32x32@2x.png` |
| `128x128/apps` | | `icon_128x128.png` |
| `128x128@2/apps` | | `icon_128x128@2x.png` (256 px) |
| `256x256/apps` | | `icon_256x256.png` |
| `256x256@2/apps` | | `icon_256x256@2x.png` (512 px) |
| `512x512/apps` | | `icon_512x512.png` |
| `512x512@2/apps` | | `icon_512x512@2x.png` (1024 px) |

The two resized ones use `magick SRC -filter Lanczos -resize NxN -strip -define
png:exclude-chunks=date,time DST` so the committed bytes carry no timestamp. Committing the output
keeps ImageMagick out of the package build. The `@2` directories are part of hicolor-icon-theme's
index on 26.04. AppStream wants an icon of at least 64 px; it gets 128 and up.

The icon is the Mac's square as is (dark paper, "md" in the typewriter face); GNOME's guidelines draw
app icons as rounded shapes — whether to round it for Linux is nettrash's call (§10).

---

## 5. AppStream — `me.nettrash.md.metainfo.xml`

```xml
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>me.nettrash.md</id>
  <metadata_license>CC0-1.0</metadata_license>
  <project_license>MIT</project_license>
  <name>md</name>
  <summary>Write Markdown and see it rendered</summary>
  <developer id="me.nettrash">
    <name>nettrash</name>
  </developer>
  <description>
    <p>md is the simplest Markdown editor: write on one side and see the page on the other, or
      switch to a full-window editor or preview. The same hand-written renderer as md for macOS,
      iPhone, Android, Windows and VS Code — the same document gives the same page everywhere.</p>
    <p>Math with KaTeX and mhchem, Mermaid, Graphviz and PlantUML diagrams and highlighted code are
      drawn on your computer by the bundled open source engines; plots need no engine at all.</p>
    <ul>
      <li>Export to PDF at seven page sizes, a self-contained HTML page, EPUB, LaTeX and TextBundle</li>
      <li>Writer mode: a book is a folder of chapters and articles, compiled to PDF, EPUB or LaTeX</li>
      <li>Continue lists, capitalize sentences, find and replace, a Format menu, Zen mode</li>
      <li>Files are saved in place, in the encoding and line endings they came with</li>
    </ul>
  </description>
  <launchable type="desktop-id">me.nettrash.md.desktop</launchable>
  <icon type="stock">me.nettrash.md</icon>
  <url type="homepage">https://nettrash.me/linux/md/support.html</url>
  <url type="help">https://nettrash.me/linux/md/support.html</url>
  <url type="vcs-browser">https://github.com/nettrash/md.linux</url>
  <categories>
    <category>Utility</category>
    <category>TextEditor</category>
  </categories>
  <provides>
    <binary>md</binary>
    <mediatype>text/markdown</mediatype>
    <mediatype>text/x-plantuml</mediatype>
    <mediatype>text/vnd.graphviz</mediatype>
    <mediatype>application/x-textpack</mediatype>
  </provides>
  <content_rating type="oars-1.1"/>
  <branding>
    <color type="primary" scheme_preference="light">#F4EFE2</color>
    <color type="primary" scheme_preference="dark">#241E18</color>
  </branding>
  <releases>
    <release version="1.6.0" date="2026-10-27"/>
  </releases>
</component>
```

The copy follows the family's rule: never "no third-party dependencies", "zero permissions" or "no
network" (the preview loads remote images, an EPUB cover may be fetched), and no `<` or `>` in prose.
Screenshots are added when the store copy is written (they need a real desktop). Validated with
`appstreamcli validate --no-net` (appstream 1.1.2) in the test container. The release date moves with
the real release.

---

## 6. Copyright and the bundled engines — `/usr/share/doc/md/copyright`

Debian policy requires a copyright file in every package, in the machine-readable format
(`Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/`). md's own code is
`Expat` (MIT, © 2026 nettrash). The package also redistributes the engines of `rich/` (the same bytes
every md port ships), so the file must name their licences:

| Files | Copyright | Licence (as published upstream) | Text in the file |
| --- | --- | --- | --- |
| `usr/share/md/rich/katex.min.*`, `auto-render.min.js` | Khan Academy and other contributors | Expat | full text |
| `usr/share/md/rich/fonts/*` | the KaTeX authors | OFL-1.1 | full text |
| `usr/share/md/rich/mhchem.min.js` | Martin Hensel | Apache-2.0 | `/usr/share/common-licenses/Apache-2.0` |
| `usr/share/md/rich/mermaid.min.js` | Knut Sveidqvist and contributors | Expat | full text |
| `usr/share/md/rich/viz-global.js` | Michael Daines (Viz.js); AT&T and the Graphviz authors; the Expat authors | Expat; EPL-1.0 (Graphviz, compiled in — the file says so); Expat (libexpat) | full texts |
| `usr/share/md/rich/highlight.min.js` | Josh Goebel and contributors (header: BSD-3-Clause) | BSD-3-clause | full text |
| `usr/share/md/rich/plantuml.js` | Arnaud Roques and contributors (a TeaVM build) | **to be decided** — see below | |
| `usr/share/md/rich/md-init.js`, everything else | 2026 nettrash | Expat | full text |

Two consequences nettrash should know before this file is written (§10):

1. **This is the first published licence statement for the engines.** The family has deliberately
   not shipped a notices file or claimed licences anywhere (the open "notices" question). A `.deb`
   cannot avoid the copyright file. Its content should be agreed once and then reused for the other
   ports' notices if those are ever added.
2. **`plantuml.js` needs a licence line.** nettrash decided (2026-07-24) to leave the PlantUML licence
   situation as it is, and this does not reopen that. The copyright file still has to say something
   true: the upstream project distributes PlantUML under GPL-3+ by default (other licences are
   separate editions), and the bundled file carries no header saying which edition it was built
   from. Writing "GPL-3+" (pointing at `/usr/share/common-licenses/GPL-3`) is the factual default;
   EPL-1.0 for Graphviz similarly obliges an offer of source, which a pointer to graphviz.org in the
   copyright file provides.

The About dialog lists the engines by name and version only (shell-design §2.8), as md.win does.

---

## 7. AppArmor — `/etc/apparmor.d/md` and the maintainer scripts

Ubuntu 26.04 sets `kernel.apparmor_restrict_unprivileged_userns = 1`, and WebKitGTK's web processes
run inside bubblewrap, which needs an unprivileged user namespace; when it cannot get one, WebKit
`g_error`s and the **whole UI process aborts** (facts §9). Ubuntu's own `apparmor` 5.0.2 package
carries, for every WebKitGTK application it knows, a profile that confines nothing and only allows
`userns` — this is `/etc/apparmor.d/wike` from the image, verbatim apart from the names:

```
# This profile allows everything and only exists to give the
# application a name instead of having the label "unconfined"

abi <abi/5.0>,
include <tunables/global>

profile md /usr/bin/md flags=(unconfined) {
  userns,
  mr @{exec_path},

  # Site-specific additions and overrides. See local/README for details.
  include if exists <local/md>
}
```

`/etc/apparmor.d/md` is listed in `DEBIAN/conffiles` (every file under `/etc` must be). The scripts
reproduce what `dh_apparmor` generates (no debhelper here):

```sh
# DEBIAN/postinst
#!/bin/sh
set -e
if [ "$1" = "configure" ]; then
    APP_PROFILE="/etc/apparmor.d/md"
    if [ -f "$APP_PROFILE" ]; then
        LOCAL_APP_PROFILE="/etc/apparmor.d/local/md"
        test -e "$LOCAL_APP_PROFILE" || {
            mkdir -p "$(dirname "$LOCAL_APP_PROFILE")"
            install --mode 644 /dev/null "$LOCAL_APP_PROFILE"
        }
        if aa-enabled --quiet 2>/dev/null; then
            apparmor_parser -r -T -W "$APP_PROFILE" || true
        fi
    fi
fi
exit 0
```

```sh
# DEBIAN/postrm
#!/bin/sh
set -e
if [ "$1" = "purge" ] && ! [ -e "/etc/apparmor.d/md" ]; then
    rm -f /etc/apparmor.d/disable/md /etc/apparmor.d/force-complain/md /etc/apparmor.d/local/md || true
    rm -f /var/cache/apparmor/*/md || true
    rmdir /etc/apparmor.d/disable 2>/dev/null || true
    rmdir /etc/apparmor.d/local 2>/dev/null || true
    rmdir /etc/apparmor.d 2>/dev/null || true
fi
exit 0
```

`aa-enabled` absent or AppArmor off → nothing is loaded and nothing is needed (the restriction is an
AppArmor feature). The profile could not be tested in Docker (Docker Desktop's kernel has no
AppArmor); it is the first item of the manual test (§9). As a guard against an unexplained crash at
launch, the shell checks for the sandbox once before its first WebView (shell-design §4.1): if a
`bwrap --unshare-user` probe fails, md shows one alert naming `/etc/apparmor.d/md` and quits —
**never** setting `WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS`.

---

## 8. Control data and lintian

### 8.1 `DEBIAN/control`

```
Package: md
Version: 1.6.0
Architecture: amd64 | arm64
Maintainer: nettrash <nettrash@nettrash.me>
Installed-Size: <du -sk of the staging tree, without DEBIAN>
Depends: <dpkg-shlibdeps result, floors raised as below>, fonts-urw-base35, fonts-liberation
Section: editors
Priority: optional
Homepage: https://nettrash.me/linux/md/support.html
Description: Markdown editor and live previewer
 md is a Markdown editor with a live preview beside the text: Edit, Split and
 Preview layouts, Zen mode, find and replace, continued lists and
 capitalized sentences, and a Format menu.
 .
 The preview draws KaTeX math with mhchem chemistry, Mermaid, Graphviz and
 PlantUML diagrams and highlighted code with bundled open source engines,
 and plots with none. Documents export to PDF, HTML, EPUB, LaTeX and
 TextBundle; a folder of Markdown files can be written and compiled as a book.
```

**Depends.** `dpkg-shlibdeps -O target/release/md` (run in a scratch directory holding a minimal
`debian/control`, which the tool requires) computes the library packages from the binary's `NEEDED`
entries and the installed symbols files. Its floors reflect only the symbols the binary references,
but the crates are built with `v4_22`, `v1_9`, `v2_52`, `v2_88` and md is tested on nothing older, so
`scripts/package.sh` raises them: `libgtk-4-1 (>= 4.22)`, `libadwaita-1-0 (>= 1.9)`,
`libwebkitgtk-6.0-4 (>= 2.52)`, `libjavascriptcoregtk-6.0-1 (>= 2.52)`, `libglib2.0-0t64 (>= 2.88)`,
and keeps every other entry (`libc6 (>= 2.43)`, `libgcc-s1`, `libpango-1.0-0`, `libcairo2`,
`libsoup-3.0-0`, `libgdk-pixbuf-2.0-0`, `libgraphene-1.0-0`, … as computed). The script fails if
`dpkg-shlibdeps` names a package the raise list does not expect, so a new linkage is reviewed, not
shipped silently. `bubblewrap` and `xdg-dbus-proxy` arrive through `libwebkitgtk-6.0-4`'s own Depends
(checked: 2.52.6). No Recommends.

**md5sums**: `md5sum` of every file under the staging root except `DEBIAN/`, paths relative, sorted.

### 8.2 Lintian (`lintian` 2.129 in a throwaway 26.04 container — not part of the build)

Expected clean, given the layout above (stripped binary, man page, Debian-format `changelog.gz`,
machine-readable copyright, conffile listed, scripts `#!/bin/sh` + `set -e`, root ownership, 0755 /
0644 modes). Expected tags, each overridden in `/usr/share/lintian/overrides/md` with its reason:

| Tag | Why it is right here |
| --- | --- |
| `font-in-non-font-package`, `font-outside-font-dir` | KaTeX's fonts are loaded by `katex.min.css` through `url(fonts/…)` relative to the stylesheet; they are web fonts of the preview, not system fonts |
| `embedded-javascript-library` (if raised for KaTeX, highlight.js or Mermaid) | the engines are pinned, byte-identical across every md port and version-checked by the shared fixtures; Ubuntu's packaged copies differ in version and layout |
| `package-contains-documentation-outside-usr-share-doc` (if raised for `examples/`) | the examples are application data opened by File ▸ Examples |
| `hardening-no-fortify-functions` (if raised) | Rust code; there is no C source to fortify |

Anything else lintian reports is fixed, not overridden. `privacy-breach-*` tags, if raised for URLs
inside the minified engines, are investigated one by one (none of them is fetched by md).

---

## 9. Building it — `scripts/package.sh` (via `scripts/dev.sh package <arch>`)

### 9.1 Images per architecture

```sh
docker buildx build --platform linux/$ARCH --load -t md-linux-dev:$ARCH docker/
```

The Dockerfile pins the base image by digest (`FROM ubuntu:26.04@sha256:…`, the multi-arch index
digest) and its package set by an Ubuntu snapshot (`apt-get update --snapshot <UTC date>`, available
since apt 2.7 / Ubuntu 23.10 — to be confirmed for resolute on first use), so the compiler, GTK and
WebKit headers do not move under a rebuild. The cargo registry volume is shared between
architectures (sources only); each architecture has its own target volume
(`md-linux-target-<worktree>-<arch>`). On Apple silicon `arm64` runs natively and `amd64` under
QEMU (Docker Desktop's binfmt; expect a release build several times slower than native).

### 9.2 Inside the container

The steps of `scripts/package.sh` (an outline; the script is P6's):

```text
export SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)"
export RUSTFLAGS="--remap-path-prefix=/src=/build/md --remap-path-prefix=/cargo=/cargo"
cargo build --release --locked -p md                    # [profile.release] lto thin, cgu 1, strip
stage=/tmp/md-stage; rm -rf "$stage"
install -D -m 0755 /target/release/md            "$stage/usr/bin/md"
cp -r rich examples   → "$stage/usr/share/md/"         (then chmod: dirs 0755, files 0644)
install packaging/{desktop,mime,metainfo,icons,apparmor,lintian} files at the §1 paths
gzip -9n -c packaging/md.1       > "$stage/usr/share/man/man1/md.1.gz"
gzip -9n -c packaging/changelog  > "$stage/usr/share/doc/md/changelog.gz"
gzip -9n each of CHANGELOG.md README.md PRIVACY.md into usr/share/doc/md/
install -m 0644 packaging/copyright "$stage/usr/share/doc/md/copyright"
dpkg-shlibdeps check and Depends (§8.1); control from packaging/control.in (+ Installed-Size)
md5sums (sorted); conffiles; postinst / postrm 0755
find "$stage" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
dpkg-deb --root-owner-group -Zxz -z9 --build "$stage" "dist/md_1.6.0_$ARCH.deb"
```

`[profile.release]` gains `strip = true` and `debug = false` (the workspace already has `lto = "thin"`
and `codegen-units = 1`); the binary is built without `--features selftest`. `dpkg-deb` honours
`SOURCE_DATE_EPOCH` for the archive members; the files' own mtimes are clamped above. The script
prints the `.deb`'s SHA-256; building twice must print the same one (P6's acceptance).

### 9.3 Checks the script runs before it declares success

In a fresh `ubuntu:26.04` container of the same architecture (`--platform linux/$ARCH`): `apt-get
install ./dist/md_1.6.0_$ARCH.deb` (resolving Depends from the archive), `md --version` = `md 1.6.0`,
`xdg-mime query filetype` for one file of every extension (§3 table), `desktop-file-validate`,
`appstreamcli validate --no-net`, `lintian` (§8.2), `dpkg -r md` and `dpkg -P md` leaving no file
behind but nothing of the user's.

### 9.4 Without Docker — `scripts/package.sh --native`

On an Ubuntu 26.04 machine with the image's build packages (`build-essential pkg-config git rustc
cargo libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev`; the script names what is missing), the
same steps run on the machine itself, for its own architecture only:

- the source is exported from git as in the Docker build;
- §9.2 runs unchanged, with the target directory `target/package` (or a fresh one with `--clean`)
  and the cargo home `~/.cargo` remapped to the containers' paths, so the binary holds the same
  paths as a Docker build's;
- §9.3's lintian runs if `lintian` is installed. The install, MIME, AppArmor and remove/purge
  checks do not run, since they would install md on the machine that builds it; §11 covers them.

The compiler and libraries are the machine's, so the bytes equal a Docker build's of the same
commit only while the machine's packages match the image's snapshot. The release build stays the
Docker one. `MD_DEV_NATIVE=1 scripts/dev.sh package` is the same command.

---

## 10. Open questions for nettrash

1. The **copyright file** is the first place the engines' licences are published (§6): agree its
   wording, and the line for `plantuml.js`.
2. **The icon**: ship the Mac's full-bleed square as is, or a rounded variant for GNOME (§4)?
3. **Where users get the `.deb`**: a GitHub release asset, nettrash.me, or a PPA later (a PPA would
   need a source package and debhelper — out of this design's scope).
4. **Support and privacy pages** `https://nettrash.me/linux/md/{support,privacy}.html` must exist
   before release (md-logic's `strings::help` already points there; the nettrash-me site repo).

---

## 11. Manual install and uninstall test — a real Ubuntu 26.04 desktop

Run on a fresh 26.04 desktop install, once on **amd64** and once on **arm64** (a VM is fine for
both), GNOME on Wayland, with the user's own account. Tick each line; note the build's SHA-256.

**Install**

1. `sudo apt install ./md_1.6.0_<arch>.deb` — dependencies resolve from the archive; no warning
   about the profile.
2. `sudo aa-status | grep -w md` lists the `md` profile; `ls /etc/apparmor.d/local/md` exists.
3. md appears in the app grid as "md" with the icon at every zoom; Super, type "markdown" finds it.
4. Launch from the grid: one window, paper colours, the menu bar; the dock shows md's icon (not a
   generic one), grouped with the window.
5. `ps -ef | grep -E 'WebKit(Web|Network)Process'` shows the web processes, and
   `cat /proc/<WebKitWebProcess pid>/status | grep NSpid` shows a namespace (the sandbox is on);
   File ▸ Examples ▸ Diagrams renders Mermaid, Graphviz and PlantUML; Math renders KaTeX.
6. Files: double-click `a.md` → note which application opens it (if not md, Open With ▸ md with
   "Always use for this file type" works); `a.mdown`, `a.mkdown`, `a.text`, `a.puml`, `a.gv`,
   `a.textpack` → right-click ▸ Open With lists md, and opening works;
   `xdg-mime query filetype a.mdown` → `text/markdown`.
7. With md running, double-click another `.md` → it opens in the running md, in front.
8. The `.desktop` action: right-click md in the dock ▸ New Document opens an untitled window.
9. Settings ▸ Appearance ▸ Dark: the window and the preview turn to carbon paper without a white
   flash; back to Light.
10. Type in an untitled window, Ctrl+Q, relaunch: the draft returns, "— Edited"; type in a saved file,
    log out and in, relaunch: the file and its layout return.
11. File ▸ Print… shows GTK's dialog; Print to File writes a PDF; Export ▸ PDF… / HTML… / EPUB… write
    files that open in the Document Viewer, a browser and Foliate.
12. `docs/desktop-checklist.md` on the same machine — every package's real-desktop list, gathered
    and ordered by risk.

**Uninstall**

13. `sudo apt remove md`: the app leaves the grid; `xdg-mime query default text/markdown` no longer
    names md; `/etc/apparmor.d/md` remains (a conffile) and the user's `~/.config/md`,
    `~/.local/state/md`, `~/.local/share/md` remain (Debian never removes user data).
14. `sudo apt purge md`: `/etc/apparmor.d/md` and `/etc/apparmor.d/local/md` are gone;
    `dpkg -L md` reports nothing; reinstalling works and finds the user's settings again.
