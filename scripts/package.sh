#!/usr/bin/env bash
# md for Linux — the .deb for Ubuntu 26.04 (docs/packaging.md).
#
#   scripts/package.sh [options] [amd64|arm64 …]      default: the host's architecture
#
#   --clean         build in a new, empty target volume (removed afterwards): what proves that two
#                   builds of one commit give one SHA-256, since an incremental build would not
#   --no-check      skip the install / uninstall checks in a fresh ubuntu:26.04 container
#   --allow-dirty   package the working tree although it has uncommitted changes (see below)
#   --native        no Docker: build on this machine, an Ubuntu 26.04 with the build packages
#                   (below), for its own architecture only
#
# Self-contained: it needs only Docker with buildx (amd64 runs under emulation on an arm64 Mac).
# `scripts/dev.sh package [arch…]` delegates here. Per architecture it
#
#   1. builds the image md-linux-dev:<arch> from docker/Dockerfile (pinned base and archive);
#   2. exports the source from git — HEAD itself when the tree is clean, else (--allow-dirty) the
#      tracked and untracked-but-not-ignored files — so nothing stray (a .DS_Store in examples/)
#      can reach the package;
#   3. in md-linux-dev:<arch>: cargo build --release --locked -p md, stages docs/packaging.md §1
#      in a volume and computes Depends (§8.1) and the control data; then, in md-linux-dev of the
#      machine's own architecture, builds dist/md_<version>_<arch>.deb with dpkg-deb and checks
#      its info and contents against §1 (see deb_inside for why dpkg-deb runs natively), and from
#      the same files the portable dist/md-<version>-linux-<x86_64|aarch64>.tar.gz (§12);
#   4. in a fresh ubuntu:26.04 of the same architecture (§9.3; the image md-linux-ubuntu:<arch>,
#      the pinned base and nothing else): apt installs the .deb from the live archive, runs
#      md --version, the MIME mapping of every extension, desktop-file-validate, appstreamcli
#      validate --no-net, an AppArmor parse of the profile, then apt remove and purge, and a
#      reinstall; and lintian, in a throwaway ubuntu:26.04 of the machine's own architecture.
#
# The package's SOURCE_DATE_EPOCH is the commit time of HEAD.
#
# --native runs step 3 on this machine instead: the build packages are docker/Dockerfile's
#
#   sudo apt install build-essential pkg-config git rustc cargo \
#        libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev
#
# and the target directory is target/package (--clean: a new one, removed afterwards). Of step 4
# it runs lintian only, if installed (sudo apt install lintian): it does not install the package
# on the machine that builds it. The bytes match a Docker build's when this machine's compiler and
# libraries are the snapshot's; a later archive can change them.
#
# Environment: MD_PACKAGE_SKIP_IMAGE=1 reuses the existing images md-linux-dev:<arch> and
# md-linux-ubuntu:<arch>.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG=md

say() { printf '%s\n' "package: $*" >&2; }
die() { say "FAILED: $*"; exit 1; }
# shellcheck source=scripts/package-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/package-lib.sh"

# Where the steps find the source, the staging area, the output, the target directory and the
# cargo home: the containers' mounts, unless --native points them at this machine.
SRC=/src STAGE=/stage OUT=/out
TARGET="${CARGO_TARGET_DIR:-/target}" CARGO_DIR="${CARGO_HOME:-/cargo}"

# The packages dpkg-shlibdeps may name for /usr/bin/md: the libraries behind the -sys crates of
# app/Cargo.toml, plus libc and libgcc. Anything else is a new linkage to review, not to ship.
ALLOWED_DEPENDS="libc6 libgcc-s1 libglib2.0-0t64 libgtk-4-1 libadwaita-1-0 libwebkitgtk-6.0-4
libjavascriptcoregtk-6.0-1 libsoup-3.0-0 libpango-1.0-0 libcairo2 libcairo-gobject2
libgdk-pixbuf-2.0-0 libgraphene-1.0-0"

# Floors raised to what the crates are built against (v4_22, v1_9, v2_52, v2_88) and md is tested
# on: dpkg-shlibdeps' floors only reflect the symbols the binary happens to reference.
RAISED_FLOORS="libgtk-4-1:4.22 libadwaita-1-0:1.9 libwebkitgtk-6.0-4:2.52 libjavascriptcoregtk-6.0-1:2.52
libglib2.0-0t64:2.88"

# Every crate linked into the binary must be named in packaging/copyright and offer MIT.
ALLOWED_CRATE_LICENSES="MIT|MIT OR Apache-2.0|Apache-2.0 OR MIT|Unlicense OR MIT"

# ---------------------------------------------------------------------------------------------
# Inside md-linux-dev:<arch>: build, stage, package, check the layout.

build_inside() {
    local arch="$1"
    local version stage
    cd "$SRC"
    [ "$(dpkg --print-architecture)" = "$arch" ] || die "this system is $(dpkg --print-architecture), not $arch"
    [ -n "${SOURCE_DATE_EPOCH:-}" ] || die "SOURCE_DATE_EPOCH is not set"

    version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' Cargo.toml)"
    [ -n "$version" ] || die "no [workspace.package] version in Cargo.toml"
    grep -q "^$PKG ($version) " packaging/changelog || die "packaging/changelog does not start at $version"
    grep -q "<release version=\"$version\"" packaging/me.nettrash.md.metainfo.xml ||
        die "the metainfo has no release $version"

    check_crates

    # Reproducible: fixed paths in the binary (the containers' ones, wherever it is built), no debug
    # info, symbols stripped, one codegen unit (the workspace's [profile.release]); the selftest
    # feature is never built into the package.
    export_rustflags "$SRC" "$CARGO_DIR" "$TARGET"
    export CARGO_TARGET_DIR="$TARGET" CARGO_PROFILE_RELEASE_STRIP=true CARGO_PROFILE_RELEASE_DEBUG=false
    say "cargo build --release --locked -p md ($arch)"
    cargo build --release --locked -p md
    local bin="$TARGET/release/md"
    if readelf -S "$bin" | grep -qE '\.(symtab|debug_[a-z]+)'; then die "the binary is not stripped"; fi

    stage="$STAGE/tree"
    rm -rf "$stage"
    mkdir -p "$stage"
    install -D -m 0755 "$bin" "$stage/usr/bin/md"
    # What dh_strip also removes: the compilers' .comment and the .note section.
    strip --remove-section=.comment --remove-section=.note "$stage/usr/bin/md"
    mkdir -p "$stage/usr/share/md"
    cp -R rich examples "$stage/usr/share/md/"
    # The KaTeX faces' TrueType alternates are left out: WebKitGTK loads the woff2 the stylesheet
    # names first, and md-core's HTML export inlines only woff2, so nothing on Linux reads a .ttf —
    # and lintian rightly calls them duplicates of Ubuntu's fonts-katex. Every file shipped stays
    # byte-identical to the other ports' rich/.
    rm -f "$stage"/usr/share/md/rich/fonts/*.ttf
    install -D -m 0644 packaging/me.nettrash.md.desktop "$stage/usr/share/applications/me.nettrash.md.desktop"
    install -D -m 0644 packaging/me.nettrash.md.xml "$stage/usr/share/mime/packages/me.nettrash.md.xml"
    install -D -m 0644 packaging/me.nettrash.md.metainfo.xml \
        "$stage/usr/share/metainfo/me.nettrash.md.metainfo.xml"
    mkdir -p "$stage/usr/share/icons"
    cp -R packaging/icons/hicolor "$stage/usr/share/icons/"
    install -D -m 0644 packaging/apparmor/md "$stage/etc/apparmor.d/md"
    install -D -m 0644 packaging/lintian-overrides "$stage/usr/share/lintian/overrides/md"
    mkdir -p "$stage/usr/share/man/man1" "$stage/usr/share/doc/md"
    gzip -9n -c packaging/md.1 > "$stage/usr/share/man/man1/md.1.gz"
    gzip -9n -c packaging/changelog > "$stage/usr/share/doc/md/changelog.gz"
    local doc
    for doc in CHANGELOG.md README.md PRIVACY.md; do
        gzip -9n -c "$doc" > "$stage/usr/share/doc/md/$doc.gz"
    done
    install -m 0644 packaging/copyright "$stage/usr/share/doc/md/copyright"

    # Modes and owners: directories 0755, files 0644, the binary and the scripts 0755; dpkg-deb
    # --root-owner-group makes every entry root:root.
    find "$stage" -type d -exec chmod 0755 {} +
    find "$stage" -type f -exec chmod 0644 {} +
    chmod 0755 "$stage/usr/bin/md"

    # DEBIAN/ first: dpkg-shlibdeps finds the package root of the binary by it.
    mkdir -p "$stage/DEBIAN"
    local depends installed_size
    depends="$(compute_depends "$stage/usr/bin/md")"
    installed_size="$(installed_size "$stage")"

    sed -e "s/@VERSION@/$version/" -e "s/@ARCH@/$arch/" -e "s/@INSTALLED_SIZE@/$installed_size/" \
        -e "s/@DEPENDS@/$depends/" packaging/control.in > "$stage/DEBIAN/control"
    if grep -q '@[A-Z_]*@' "$stage/DEBIAN/control"; then die "control.in has an unfilled field"; fi
    printf '%s\n' /etc/apparmor.d/md > "$stage/DEBIAN/conffiles"
    install -m 0755 packaging/postinst "$stage/DEBIAN/postinst"
    install -m 0755 packaging/postrm "$stage/DEBIAN/postrm"
    (cd "$stage" && find . -path ./DEBIAN -prune -o -type f -printf '%P\n' | LC_ALL=C sort |
        xargs -d '\n' md5sum) > "$stage/DEBIAN/md5sums"
    chmod 0644 "$stage/DEBIAN/control" "$stage/DEBIAN/conffiles" "$stage/DEBIAN/md5sums"

    find "$stage" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
    printf '%s\n' "$depends" > "$STAGE/depends"
    say "staged for $arch"
}

# In md-linux-dev of the machine's own architecture: the .deb from the staged tree, and its
# layout check. dpkg-deb runs natively because under Docker Desktop's Rosetta emulation GNU tar
# cannot stat a path given by name (`Cannot stat: Function not implemented`), and dpkg-deb always
# names its files to tar; what dpkg-deb writes depends on its version and its input, not on the
# processor it runs on, and both images carry the same dpkg from the same snapshot.
deb_inside() {
    local arch="$1" version stage="$STAGE/tree" deb depends
    cd "$SRC"
    version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' Cargo.toml)"
    [ -f "$stage/DEBIAN/control" ] || die "nothing staged for $arch"
    grep -qx "Architecture: $arch" "$stage/DEBIAN/control" || die "the staged tree is not $arch"
    depends="$(cat "$STAGE/depends")"
    deb="$OUT/${PKG}_${version}_${arch}.deb"
    rm -f "$deb"
    # One xz thread: liblzma's multi-threaded output is laid out in blocks, so the bytes would
    # depend on how many CPUs the building machine has.
    dpkg-deb --root-owner-group --threads-max=1 -Zxz -z9 --build "$stage" "$deb" >/dev/null
    check_layout "$deb" "$version" "$arch" "$depends"
    say "built $(basename "$deb"): $(stat -c %s "$deb") bytes"
    (cd "$OUT" && sha256sum "$(basename "$deb")")
    make_tarball "$stage" "$version" "$arch"
    rm -rf "$stage" "$STAGE/depends"
}

# The portable archive md-<version>-linux-<x86_64|aarch64>.tar.gz (docs/packaging.md §12): the
# staged /usr as md-<version>/, whose bin/md finds share/md beside it (shell-design §9.3), without
# lintian's overrides, plus packaging/tarball/README and the AppArmor profile as a template.
# Reproducible like the .deb: sorted names, root-owned, SOURCE_DATE_EPOCH, gzip without a name.
make_tarball() {
    local stage="$1" version="$2" arch="$3" machine name tmp tarball status=0 listing want
    case "$arch" in amd64) machine=x86_64 ;; arm64) machine=aarch64 ;; *) die "no tarball name for $arch" ;; esac
    name="$PKG-$version"
    tarball="$OUT/$PKG-$version-linux-$machine.tar.gz"
    tmp="$(mktemp -d /tmp/md-tarball.XXXXXX)"
    cp -a "$stage/usr" "$tmp/$name"
    rm -rf "$tmp/$name/share/lintian"
    sed "s/@VERSION@/$version/g" packaging/tarball/README > "$tmp/$name/README"
    install -m 0644 packaging/apparmor/md "$tmp/$name/share/doc/md/apparmor-profile"
    chmod 0644 "$tmp/$name/README"
    find "$tmp" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
    rm -f "$tarball"
    tar --sort=name --format=gnu --owner=root:0 --group=root:0 -C "$tmp" -cf - "$name" |
        gzip -9n > "$tarball"
    rm -rf "$tmp"

    listing="$(tar -tzvf "$tarball")"
    for want in bin/md README share/md/rich/md-init.js share/md/examples/01-Welcome.md \
        "share/md/examples/Example Book/01-Preface.md" share/applications/me.nettrash.md.desktop \
        share/metainfo/me.nettrash.md.metainfo.xml share/mime/packages/me.nettrash.md.xml \
        share/man/man1/md.1.gz share/doc/md/copyright share/doc/md/apparmor-profile; do
        printf '%s\n' "$listing" | grep -qF " $name/$want" || { say "tarball: no $name/$want"; status=1; }
    done
    printf '%s\n' "$listing" | grep -qE "^-rwxr-xr-x root/root .* $name/bin/md\$" ||
        { say "tarball: bin/md is not an executable owned by root"; status=1; }
    [ "$(printf '%s\n' "$listing" | grep -c " $name/share/icons/hicolor/.*/apps/me\.nettrash\.md\.png\$")" = 13 ] ||
        { say "tarball: not 13 icon sizes"; status=1; }
    printf '%s\n' "$listing" | grep -qE " $name/(share/lintian|DEBIAN|etc)/?" &&
        { say "tarball: holds a Debian-only file"; status=1; }
    [ "$status" = 0 ] || die "$(basename "$tarball") is not as docs/packaging.md §12 describes"
    say "built $(basename "$tarball"): $(stat -c %s "$tarball") bytes"
    (cd "$OUT" && sha256sum "$(basename "$tarball")")
}

# Every crate the binary links (normal edges, no proc macros: their code runs at compile time)
# is named in packaging/copyright, under a licence that offers MIT.
check_crates() {
    local line name license status=0
    while IFS='|' read -r line license; do
        name="${line%% *}"
        case "$name" in md | md-core | md-logic) continue ;; esac
        if ! printf '%s\n' "$license" | grep -qxE "$ALLOWED_CRATE_LICENSES"; then
            say "crate $line has licence '$license', which does not offer MIT"; status=1
        fi
        if ! tr -s ' \n' '\n\n' < packaging/copyright | tr -d ',.' | grep -qx "$name"; then
            say "crate $name is linked into md but not named in packaging/copyright"; status=1
        fi
    done < <(cargo tree --locked -p md -e normal,no-proc-macro --prefix none --format '{p}|{l}' |
        sed 's/ (\*)$//' | sort -u)
    [ "$status" = 0 ] || die "packaging/copyright does not cover the crates linked into md"
}

# dpkg-shlibdeps' Depends for the binary, every package checked against ALLOWED_DEPENDS, the
# floors of RAISED_FLOORS raised, the fonts appended.
compute_depends() {
    local bin="$1" scratch shlibs entry pkg floor raised result=""
    scratch="$(mktemp -d /tmp/md-shlibs.XXXXXX)"
    mkdir -p "$scratch/debian"
    printf '%s\n' "Source: $PKG" "" "Package: $PKG" "Architecture: any" > "$scratch/debian/control"
    shlibs="$(cd "$scratch" && dpkg-shlibdeps -O -e"$bin" 2>"$scratch/warnings")" ||
        { cat "$scratch/warnings" >&2; die "dpkg-shlibdeps failed"; }
    # On amd64, Ubuntu's usr-merge diverts /lib64/ld-linux-x86-64.so.2, so dpkg-shlibdeps warns
    # that diversions are involved and, unable to attribute symbols through the diverted loader,
    # that linking it is useless. libc6 provides the loader and is in Depends either way; those
    # two messages are dropped, anything else is shown.
    grep -vE 'diversions involved - output may be incorrect|diversion by libc6 (from|to): /lib64/ld-linux-x86-64\.so\.2|could avoid a useless dependency if .* was not linked against ld-linux-x86-64\.so\.2' \
        "$scratch/warnings" | sed 's/^/package: dpkg-shlibdeps: /' >&2 || true
    rm -rf "$scratch"
    shlibs="${shlibs#shlibs:Depends=}"
    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        case "$entry" in *'|'*) die "an alternative in Depends: $entry" ;; esac
        pkg="${entry%% *}"
        printf '%s\n' $ALLOWED_DEPENDS | grep -qxF "$pkg" ||
            die "dpkg-shlibdeps names $pkg, which ALLOWED_DEPENDS does not expect: review the new linkage"
        raised="$(printf '%s\n' $RAISED_FLOORS | awk -F: -v p="$pkg" '$1 == p { print $2 }')"
        if [ -n "$raised" ]; then
            floor="$(printf '%s' "$entry" | sed -n 's/^.*(>= \(.*\))$/\1/p')"
            if [ -z "$floor" ] || dpkg --compare-versions "$floor" lt "$raised"; then
                entry="$pkg (>= $raised)"
            fi
        fi
        result="${result:+$result, }$entry"
    done <<EOF
$(printf '%s\n' "$shlibs" | tr ',' '\n' | sed 's/^ *//; s/ *$//')
EOF
    printf '%s' "$result, fonts-urw-base35, fonts-liberation"
}

# What dpkg-gencontrol computes: each file rounded up to KiB, every other entry 1 KiB.
installed_size() {
    (cd "$1" && find . -mindepth 1 -path ./DEBIAN -prune -o -printf '%y %s\n') |
        awk '{ if ($1 == "f") total += int(($2 + 1023) / 1024); else total += 1 } END { print total }'
}

# dpkg-deb --info / --contents against docs/packaging.md §1.
check_layout() {
    local deb="$1" version="$2" arch="$3" depends="$4" status=0 tmp
    tmp="$(mktemp -d /tmp/md-layout.XXXXXX)"

    dpkg-deb --info "$deb" > "$tmp/info"
    local field value
    while IFS='|' read -r field value; do
        if ! grep -qxF " $field: $value" "$tmp/info"; then say "control: want '$field: $value'"; status=1; fi
    done <<EOF
Package|$PKG
Version|$version
Architecture|$arch
Maintainer|nettrash <nettrash@nettrash.me>
Depends|$depends
Section|editors
Priority|optional
Homepage|https://nettrash.me/linux/md/support.html
Description|Markdown editor and live previewer
EOF
    grep -qE '^ Installed-Size: [0-9]+$' "$tmp/info" || { say "control: no Installed-Size"; status=1; }
    local members
    members="$(dpkg-deb --ctrl-tarfile "$deb" | tar -t | sed 's#^\./##' | grep -v '^$' | LC_ALL=C sort | tr '\n' ' ')"
    [ "$members" = "conffiles control md5sums postinst postrm " ] ||
        { say "control members: $members"; status=1; }

    # Every entry root:root, mtime SOURCE_DATE_EPOCH, mode 0755 for directories, the binary and
    # nothing else, 0644 for every other file. tar lists the times in the local zone, so in UTC like
    # the stamp: a desktop's own zone (--native) would shift every one of them.
    TZ=UTC0 dpkg-deb --contents "$deb" > "$tmp/contents"
    local stamp
    stamp="$(date -u -d "@$SOURCE_DATE_EPOCH" '+%Y-%m-%d %H:%M')"
    awk -v stamp="$stamp" '
        { path = $6; for (i = 7; i <= NF; i++) path = path " " $i }
        $2 != "root/root" { print "owner " $2 ": " path; bad = 1 }
        ($4 " " $5) != stamp { print "mtime " $4 " " $5 ": " path; bad = 1 }
        /^d/ && $1 != "drwxr-xr-x" { print "mode " $1 ": " path; bad = 1 }
        /^-/ && path == "./usr/bin/md" && $1 != "-rwxr-xr-x" { print "mode " $1 ": " path; bad = 1 }
        /^-/ && path != "./usr/bin/md" && $1 != "-rw-r--r--" { print "mode " $1 ": " path; bad = 1 }
        /^[^d-]/ { print "not a file or directory: " path; bad = 1 }
        END { exit bad }' "$tmp/contents" >&2 || status=1

    # The file list is exactly §1: the fixed files, rich/ and examples/ as in the source, the icons.
    awk '/^-/ { path = $6; for (i = 7; i <= NF; i++) path = path " " $i; sub(/^\.\//, "", path); print path }' \
        "$tmp/contents" | LC_ALL=C sort > "$tmp/actual"
    {
        printf '%s\n' etc/apparmor.d/md usr/bin/md \
            usr/share/applications/me.nettrash.md.desktop \
            usr/share/doc/md/CHANGELOG.md.gz usr/share/doc/md/PRIVACY.md.gz usr/share/doc/md/README.md.gz \
            usr/share/doc/md/changelog.gz usr/share/doc/md/copyright \
            usr/share/lintian/overrides/md usr/share/man/man1/md.1.gz \
            usr/share/metainfo/me.nettrash.md.metainfo.xml usr/share/mime/packages/me.nettrash.md.xml
        find rich examples -type f ! -path 'rich/fonts/*.ttf' | sed 's#^#usr/share/md/#'
        (cd packaging/icons && find hicolor -type f) | sed 's#^#usr/share/icons/#'
    } | LC_ALL=C sort > "$tmp/expected"
    if ! cmp -s "$tmp/expected" "$tmp/actual"; then
        say "contents differ from docs/packaging.md §1 (< expected, > in the .deb):"
        diff "$tmp/expected" "$tmp/actual" >&2 || true
        status=1
    fi
    if ! grep -qE '^usr/share/icons/hicolor/(16x16|16x16@2|24x24|32x32|32x32@2|48x48|64x64|128x128|128x128@2|256x256|256x256@2|512x512|512x512@2)/apps/me\.nettrash\.md\.png$' "$tmp/actual"; then
        say "no hicolor icons"; status=1
    fi
    [ "$(grep -c '^usr/share/icons/' "$tmp/actual")" = 13 ] || { say "not 13 icon sizes"; status=1; }
    rm -rf "$tmp"
    [ "$status" = 0 ] || die "the .deb does not match docs/packaging.md §1"
    say "info and contents match docs/packaging.md §1"
}

# ---------------------------------------------------------------------------------------------
# Inside a fresh ubuntu:26.04: install, check, remove, purge (docs/packaging.md §9.3, §11 13–14).

check_inside() {
    local arch="$1" deb version status=0
    version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' /src/Cargo.toml)"
    deb="/dist/${PKG}_${version}_${arch}.deb"
    [ -f "$deb" ] || die "no $deb"
    export DEBIAN_FRONTEND=noninteractive
    fail() { say "CHECK FAILED: $*"; status=1; }
    ok() { say "ok: $*"; }

    say "apt: the checking tools"
    apt-get update -qq
    # Installed first so that their triggers (shared-mime-info, desktop-file-utils) run when md is.
    apt-get install -y -qq --no-install-recommends \
        shared-mime-info desktop-file-utils xdg-utils libglib2.0-bin appstream apparmor \
        zip ca-certificates >/dev/null

    say "apt install ./$(basename "$deb") — every Depends from the archive"
    cp "$deb" /tmp/
    apt-get install -y --no-install-recommends "/tmp/$(basename "$deb")" > /tmp/apt-install.log 2>&1 ||
        { cat /tmp/apt-install.log >&2; die "apt could not install the package"; }
    grep -E '^(W|E):' /tmp/apt-install.log >&2 && fail "apt printed warnings or errors"
    ok "installed: $(dpkg-query -W -f '${Package} ${Version} ${Architecture} ${db:Status-Status}' "$PKG")"
    dpkg -L "$PKG" | LC_ALL=C sort > /tmp/md-files

    if ldd /usr/bin/md | grep -q 'not found'; then ldd /usr/bin/md >&2; fail "unresolved libraries"; else
        ok "ldd: every library resolves"; fi

    local v
    v="$(timeout 30 md --version 2>&1 | head -1 || true)"
    if [ "$v" = "md $version" ]; then ok "md --version: $v"
    else fail "md --version printed '$v', not 'md $version'"; fi

    # The AppArmor profile: postinst created its local include; the profile parses (no kernel
    # here, so it is compiled but not loaded — loading it is the first desktop check, §11).
    [ -f /etc/apparmor.d/local/md ] && ok "postinst created /etc/apparmor.d/local/md" ||
        fail "no /etc/apparmor.d/local/md"
    if apparmor_parser --skip-kernel-load --skip-cache -Q /etc/apparmor.d/md 2>/tmp/aa.log; then
        ok "apparmor_parser compiles /etc/apparmor.d/md"
    else cat /tmp/aa.log >&2; fail "apparmor_parser rejects /etc/apparmor.d/md"; fi

    local dir=/tmp/md-mime got
    check_mime_types "$dir"
    local out
    out="$(desktop-file-validate /usr/share/applications/me.nettrash.md.desktop 2>&1)" || true
    if [ -z "$out" ]; then ok "desktop-file-validate: clean"; else printf '%s\n' "$out" >&2
        printf '%s\n' "$out" | grep -qE '(error|warning):' && fail "desktop-file-validate"; fi
    if out="$(appstreamcli validate --no-net /usr/share/metainfo/me.nettrash.md.metainfo.xml 2>&1)"; then
        printf '%s\n' "$out" | sed 's/^/  /' >&2; ok "appstreamcli validate --no-net"
    else printf '%s\n' "$out" >&2; fail "appstreamcli validate --no-net"; fi

    # The user's own files, which no step below may touch.
    mkdir -p /root/.config/md /root/.local/state/md /root/.local/share/md/drafts
    printf '[md]\nmd.pdfPageSize=a5\n' > /root/.config/md/settings.conf
    printf '{"version":1}\n' > /root/.local/state/md/session.json
    printf 'draft\n' > /root/.local/share/md/drafts/d.md
    local user_files="/root/.config/md/settings.conf /root/.local/state/md/session.json /root/.local/share/md/drafts/d.md"

    say "apt remove $PKG"
    apt-get remove -y -qq "$PKG" >/dev/null
    [ "$(dpkg-query -W -f '${db:Status-Status}' "$PKG")" = config-files ] && ok "removed: config-files remain" ||
        fail "status after remove is $(dpkg-query -W -f '${db:Status-Status}' "$PKG")"
    [ -f /etc/apparmor.d/md ] && ok "/etc/apparmor.d/md kept (a conffile)" || fail "/etc/apparmor.d/md is gone after remove"
    [ -f /etc/apparmor.d/local/md ] && ok "/etc/apparmor.d/local/md kept" || fail "/etc/apparmor.d/local/md is gone after remove"
    local path left=""
    while IFS= read -r path; do
        case "$path" in /etc/apparmor.d | /etc/apparmor.d/md) continue ;; esac
        if [ -f "$path" ] || [ -L "$path" ]; then left="$left $path"; fi
    done < /tmp/md-files
    [ -z "$left" ] && ok "remove left no packaged file but the conffile" || fail "left after remove:$left"
    [ -e /usr/share/md ] && fail "/usr/share/md remains"
    grep -q me.nettrash.md.desktop /usr/share/applications/mimeinfo.cache 2>/dev/null &&
        fail "mimeinfo.cache still offers md"
    got="$(xdg-mime query default text/markdown)"
    [ -z "$got" ] && ok "xdg-mime query default text/markdown no longer names md" || fail "default is still '$got'"
    got="$(xdg-mime query filetype "$dir/a.mdown")"
    [ "$got" != text/markdown ] && ok "a.mdown is $got again (md's MIME globs gone)" || fail "a.mdown is still text/markdown"
    for path in $user_files; do [ -f "$path" ] || fail "remove deleted the user's $path"; done

    say "apt purge $PKG"
    apt-get purge -y -qq "$PKG" >/dev/null
    dpkg-query -W "$PKG" >/dev/null 2>&1 && fail "dpkg still knows $PKG after purge" || ok "purged: dpkg no longer knows md"
    if dpkg -L "$PKG" >/dev/null 2>&1; then fail "dpkg -L md still lists files"; else ok "dpkg -L md reports nothing"; fi
    [ -e /etc/apparmor.d/md ] && fail "/etc/apparmor.d/md remains after purge"
    [ -e /etc/apparmor.d/local/md ] && fail "/etc/apparmor.d/local/md remains after purge"
    [ ! -e /etc/apparmor.d/md ] && [ ! -e /etc/apparmor.d/local/md ] && ok "purge removed the profile and its local include"
    for path in $user_files; do [ -f "$path" ] || fail "purge deleted the user's $path"; done
    ok "the user's settings, session and draft are untouched"

    say "apt install again"
    apt-get install -y -qq --no-install-recommends "/tmp/$(basename "$deb")" >/dev/null 2>&1 &&
        [ -x /usr/bin/md ] && [ -f /etc/apparmor.d/local/md ] && ok "reinstall works" || fail "reinstall"

    [ "$status" = 0 ] || die "checks failed for $(basename "$deb")"
    say "every check passed for $(basename "$deb")"
}

# lintian (docs/packaging.md §8.2), in its own throwaway ubuntu:26.04 of the machine's own
# architecture: lintian inspects the .deb and never runs it, and under Rosetta its unpacking hits
# the tar limitation deb_inside describes. No error or warning; overrides only for the tags §8.2
# lists; and the font overrides must be there, or lintian did not look at the package at all.
lintian_inside() {
    local arch="$1" version deb
    version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' /src/Cargo.toml)"
    deb="/dist/${PKG}_${version}_${arch}.deb"
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y -qq --no-install-recommends lintian >/dev/null
    cp "$deb" /tmp/lintian.deb && chmod 0644 /tmp/lintian.deb
    lintian_check /tmp/lintian.deb "$(basename "$deb")" runuser -u nobody -- env HOME=/tmp
}

# lintian on the .deb $1 (named $2 in the messages), started through the command in the remaining
# arguments (none for --native, which runs as the user already), and the verdict.
lintian_check() {
    local deb="$1" name="$2" log code=0 status=0
    shift 2
    log="$(mktemp /tmp/md-lintian.XXXXXX)"
    say "lintian $(dpkg-query -W -f '${Version}' lintian) on $name"
    "$@" lintian --display-info --show-overrides --tag-display-limit 0 "$deb" > "$log" 2>&1 || code=$?
    grep -v '^N: ' "$log" | sed 's/^/  /' >&2
    if [ "$code" -gt 1 ]; then say "CHECK FAILED: lintian could not run (exit $code)"; status=1; fi
    if grep -qE '^(E|W): ' "$log"; then say "CHECK FAILED: lintian reports errors or warnings"; status=1; fi
    if grep -E '^O: ' "$log" | grep -vqE ' (font-in-non-font-package|font-outside-font-dir|embedded-javascript-library|package-contains-documentation-outside-usr-share-doc|hardening-no-fortify-functions) '; then
        say "CHECK FAILED: lintian overrides a tag docs/packaging.md §8.2 does not list"; status=1
    fi
    if ! grep -qE '^O: md: font-in-non-font-package ' "$log"; then
        say "CHECK FAILED: lintian reported nothing about the KaTeX fonts: it did not analyse the package"; status=1
    fi
    rm -f "$log"
    [ "$status" = 0 ] || die "lintian checks failed for $name"
    say "ok: lintian: no error or warning, overrides only from docs/packaging.md §8.2"
}

# --native: this machine should be the release it packages for (Depends comes from its libraries),
# with docker/Dockerfile's build packages.
check_native_machine() {
    local os tool missing=""
    os="$(. /etc/os-release 2>/dev/null && printf '%s %s' "${ID:-?}" "${VERSION_ID:-?}")" || os="unknown"
    [ "$os" = "ubuntu 26.04" ] || say "warning: this system is '$os', not ubuntu 26.04, which the package is for"
    for tool in git cargo rustc pkg-config dpkg-deb dpkg-shlibdeps readelf strip; do
        command -v "$tool" >/dev/null || missing="$missing $tool"
    done
    if command -v pkg-config >/dev/null && ! pkg-config --exists gtk4 libadwaita-1 webkitgtk-6.0; then
        missing="$missing gtk4/libadwaita-1/webkitgtk-6.0 (pkg-config)"
    fi
    [ -z "$missing" ] || die "missing:$missing — install the build packages: sudo apt install build-essential pkg-config git rustc cargo libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev"
}

# ---------------------------------------------------------------------------------------------
# On the host.

case "${1:-}" in
    --build-inside) build_inside "$2"; exit ;;
    --deb-inside) deb_inside "$2"; exit ;;
    --lintian-inside) lintian_inside "$2"; exit ;;
    --check-inside) check_inside "$2"; exit ;;
esac

clean=0 check=1 allow_dirty=0 native_build=0
arches=()
while [ $# -gt 0 ]; do
    case "$1" in
        --clean) clean=1 ;;
        --no-check) check=0 ;;
        --allow-dirty) allow_dirty=1 ;;
        --native) native_build=1 ;;
        amd64 | arm64) arches+=("$1") ;;
        *) echo "usage: scripts/package.sh [--native] [--clean] [--no-check] [--allow-dirty] [amd64|arm64 …]" >&2; exit 2 ;;
    esac
    shift
done
if [ ${#arches[@]} -eq 0 ]; then
    case "$(uname -m)" in arm64 | aarch64) arches=(arm64) ;; *) arches=(amd64) ;; esac
fi

if [ "$native_build" = 1 ]; then
    check_native_machine
else
    command -v docker >/dev/null || die "docker is not installed (on Ubuntu 26.04, --native builds without it)"
    BASE="$(sed -n 's/^FROM \(ubuntu:26\.04@sha256:[0-9a-f]*\).*$/\1/p' "$ROOT/docker/Dockerfile")"
    [ -n "$BASE" ] || die "docker/Dockerfile does not pin ubuntu:26.04 by digest"
fi
git -C "$ROOT" rev-parse --verify HEAD >/dev/null 2>&1 || die "$ROOT is not a git checkout"
SOURCE_DATE_EPOCH="$(git -C "$ROOT" log -1 --format=%ct HEAD)"
export SOURCE_DATE_EPOCH

# The source, from git (step 2).
export_dir="$ROOT/dist/.package-source"
rm -rf "$export_dir"
mkdir -p "$export_dir"
export_source "$ROOT" "$export_dir" "$allow_dirty"
mkdir -p "$ROOT/dist"

# --native: step 3 on this machine, then lintian (step 4's other checks install the package).
if [ "$native_build" = 1 ]; then
    host="$(dpkg --print-architecture)"
    for arch in "${arches[@]}"; do
        [ "$arch" = "$host" ] || die "--native builds for this machine's architecture ($host) only, not $arch"
    done
    SRC="$export_dir" OUT="$ROOT/dist" CARGO_DIR="${CARGO_HOME:-$HOME/.cargo}"
    STAGE="$(mktemp -d "${TMPDIR:-/tmp}/md-package-stage.XXXXXX")"
    if [ "$clean" = 1 ]; then TARGET="$ROOT/target/package-clean-$$"; else TARGET="$ROOT/target/package"; fi
    trap 'rm -rf "$STAGE" "$export_dir"; if [ "$clean" = 1 ]; then rm -rf "$TARGET"; fi' EXIT
    build_inside "$host"
    deb_inside "$host"
    version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' "$SRC/Cargo.toml")"
    deb="$OUT/${PKG}_${version}_${host}.deb"
    if [ "$check" = 0 ]; then :
    elif command -v lintian >/dev/null; then lintian_check "$deb" "$(basename "$deb")"
    else say "lintian is not installed, so it did not run (sudo apt install lintian)"; fi
    say "install it with: sudo apt install $(printf '%q' "$deb")"
    exit 0
fi

case "$(docker version --format '{{.Server.Arch}}')" in
    arm64 | aarch64) native=arm64 ;;
    *) native=amd64 ;;
esac
build_image() {
    if [ "${MD_PACKAGE_SKIP_IMAGE:-0}" != 1 ]; then
        say "image md-linux-dev:$1 (docker/Dockerfile, linux/$1)"
        docker buildx build --quiet --platform "linux/$1" --load -t "md-linux-dev:$1" "$ROOT/docker" >/dev/null
    fi
}
built_native=0

for arch in "${arches[@]}"; do
    image="md-linux-dev:$arch"
    build_image "$arch"
    if [ "$arch" = "$native" ]; then built_native=1
    elif [ "$built_native" = 0 ]; then build_image "$native"; built_native=1; fi
    stage_volume="md-linux-package-stage-$(basename "$ROOT")-$arch"
    if [ "$clean" = 1 ]; then
        target_volume="md-linux-package-clean-$arch-$$"
    else
        target_volume="md-linux-target-$(basename "$ROOT")-$arch"
    fi
    status=0
    docker run --rm --platform "linux/$arch" \
        -v "$export_dir:/src:ro" \
        -v md-linux-cargo:/cargo \
        -v "$target_volume:/target" \
        -v "$stage_volume:/stage" \
        -e SOURCE_DATE_EPOCH \
        "$image" bash /src/scripts/package.sh --build-inside "$arch" || status=$?
    if [ "$clean" = 1 ]; then docker volume rm "$target_volume" >/dev/null || true; fi
    if [ "$status" = 0 ]; then
        docker run --rm --platform "linux/$native" \
            -v "$export_dir:/src:ro" \
            -v "$ROOT/dist:/out" \
            -v "$stage_volume:/stage" \
            -e SOURCE_DATE_EPOCH \
            "md-linux-dev:$native" bash /src/scripts/package.sh --deb-inside "$arch" || status=$?
    fi
    docker volume rm "$stage_volume" >/dev/null 2>&1 || true
    [ "$status" = 0 ] || die "the $arch build failed"

    if [ "$check" = 1 ]; then
        if [ "${MD_PACKAGE_SKIP_IMAGE:-0}" != 1 ]; then
            base_image "$BASE" "$arch" "md-linux-ubuntu:$arch"
            base_image "$BASE" "$native" "md-linux-ubuntu:$native"
        fi
        docker run --rm --platform "linux/$arch" \
            -v "$export_dir:/src:ro" \
            -v "$ROOT/dist:/dist:ro" \
            "md-linux-ubuntu:$arch" bash /src/scripts/package.sh --check-inside "$arch"
        docker run --rm --platform "linux/$native" \
            -v "$export_dir:/src:ro" \
            -v "$ROOT/dist:/dist:ro" \
            "md-linux-ubuntu:$native" bash /src/scripts/package.sh --lintian-inside "$arch"
    fi
done
rm -rf "$export_dir"
