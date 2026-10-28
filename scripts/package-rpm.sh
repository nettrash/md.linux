#!/usr/bin/env bash
# md for Linux — the .rpm for Fedora 44 and later (docs/packaging.md §12).
#
#   scripts/package-rpm.sh [options] [amd64|arm64 …]      default: the host's architecture
#
#   --clean         build in a new, empty target volume (removed afterwards)
#   --no-check      skip the install / remove checks in a fresh Fedora container
#   --allow-dirty   package the working tree although it has uncommitted changes
#
# Self-contained: it needs only Docker with buildx (the other architecture runs under emulation).
# The architectures are named as scripts/package.sh names them; the .rpm says x86_64 or aarch64.
# Per architecture it
#
#   1. builds the image md-linux-rpm:<arch> from docker/Dockerfile.fedora (Fedora 44);
#   2. exports the source from git, as scripts/package.sh does;
#   3. in md-linux-rpm:<arch>: cargo build --release --locked -p md with the .deb's path remapping,
#      stages the files of docs/packaging.md §12, and rpmbuild -bb packaging/rpm/md.spec writes
#      dist/md-<version>-1.fc44.<x86_64|aarch64>.rpm; its Requires and file list are checked, and
#      rpmlint must report no error;
#   4. in a fresh Fedora 44 of the same architecture (the image md-linux-fedora:<arch>, the pinned
#      base and nothing else): dnf installs the .rpm, every Requires from the archive;
#      md --version, ldd, the MIME type of every extension, desktop-file-validate, appstreamcli
#      validate --no-net; then dnf remove, which must leave nothing of md's behind and nothing of
#      the user's touched.
#
# The package's build time and file times are SOURCE_DATE_EPOCH, the commit time of HEAD.
#
# Environment: MD_PACKAGE_SKIP_IMAGE=1 reuses the existing images md-linux-rpm:<arch> and
# md-linux-fedora:<arch>.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PKG=md

say() { printf '%s\n' "package-rpm: $*" >&2; }
die() { say "FAILED: $*"; exit 1; }
# shellcheck source=scripts/package-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/package-lib.sh"

machine_of() {
    case "$1" in amd64) echo x86_64 ;; arm64) echo aarch64 ;; *) die "unknown architecture $1" ;; esac
}

# ---------------------------------------------------------------------------------------------
# Inside md-linux-rpm:<arch>: build, stage, rpmbuild, check the package.

build_inside() {
    local arch="$1" machine version stage top bin rpm
    cd /src
    machine="$(machine_of "$arch")"
    [ "$(uname -m)" = "$machine" ] || die "container is $(uname -m), not $machine"
    [ -n "${SOURCE_DATE_EPOCH:-}" ] || die "SOURCE_DATE_EPOCH is not set"
    version="$(tree_version /src)"
    [ -n "$version" ] || die "no [workspace.package] version in Cargo.toml"
    grep -q " - $version-1\$" packaging/rpm/md.spec || die "packaging/rpm/md.spec's %changelog does not start at $version"

    export_rustflags /src "${CARGO_HOME:-/cargo}" "${CARGO_TARGET_DIR:-/target}"
    export CARGO_PROFILE_RELEASE_STRIP=true CARGO_PROFILE_RELEASE_DEBUG=false
    say "cargo build --release --locked -p md ($machine, $(rustc --version))"
    cargo build --release --locked -p md
    bin="${CARGO_TARGET_DIR:-/target}/release/md"
    if readelf -S "$bin" | grep -qE '\.(symtab|debug_[a-z]+)'; then die "the binary is not stripped"; fi

    # §12: the .deb's files (§1) without the AppArmor profile and lintian's overrides; the man page
    # uncompressed (rpm's brp-compress gzips it), the documents uncompressed, as Fedora ships them,
    # and the copyright file under licenses/. The binary keeps its build-id note, which rpm reads.
    stage=/tmp/md-stage
    rm -rf "$stage"
    install -D -m 0755 "$bin" "$stage/usr/bin/md"
    strip --remove-section=.comment "$stage/usr/bin/md"
    mkdir -p "$stage/usr/share/md"
    cp -R rich examples "$stage/usr/share/md/"
    rm -f "$stage"/usr/share/md/rich/fonts/*.ttf   # as the .deb: WebKitGTK reads only the woff2
    install -D -m 0644 packaging/me.nettrash.md.desktop "$stage/usr/share/applications/me.nettrash.md.desktop"
    install -D -m 0644 packaging/me.nettrash.md.xml "$stage/usr/share/mime/packages/me.nettrash.md.xml"
    install -D -m 0644 packaging/me.nettrash.md.metainfo.xml \
        "$stage/usr/share/metainfo/me.nettrash.md.metainfo.xml"
    mkdir -p "$stage/usr/share/icons"
    cp -R packaging/icons/hicolor "$stage/usr/share/icons/"
    install -D -m 0644 packaging/md.1 "$stage/usr/share/man/man1/md.1"
    local doc
    for doc in README.md CHANGELOG.md PRIVACY.md; do
        install -D -m 0644 "$doc" "$stage/usr/share/doc/md/$doc"
    done
    install -D -m 0644 packaging/copyright "$stage/usr/share/licenses/md/copyright"
    find "$stage" -type d -exec chmod 0755 {} +
    find "$stage" -type f -exec chmod 0644 {} +
    chmod 0755 "$stage/usr/bin/md"
    find "$stage" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +

    top=/tmp/md-rpmbuild
    rm -rf "$top"
    say "rpmbuild -bb packaging/rpm/md.spec ($(rpmbuild --version))"
    rpmbuild -bb --quiet \
        --define "_topdir $top" \
        --define "md_version $version" \
        --define "md_stage $stage" \
        --define "_buildhost md-linux" \
        --define "use_source_date_epoch_as_buildtime 1" \
        --define "clamp_mtime_to_source_date_epoch 1" \
        packaging/rpm/md.spec
    rpm="$(find "$top/RPMS/$machine" -name "$PKG-$version-1.*.$machine.rpm" -print -quit)"
    [ -n "$rpm" ] || die "rpmbuild wrote no $PKG-$version-1.*.$machine.rpm"
    check_rpm "$rpm" "$version" "$machine" "$stage"
    rm -f /out/"$PKG-$version"-*."$machine".rpm
    cp "$rpm" /out/
    rm -rf "$stage" "$top"
    say "built $(basename "$rpm"): $(stat -c %s "/out/$(basename "$rpm")") bytes"
    (cd /out && sha256sum "$(basename "$rpm")")
}

# The header, Requires and file list against §12, then rpmlint: no error.
check_rpm() {
    local rpm="$1" version="$2" machine="$3" stage="$4" status=0 tmp want
    tmp="$(mktemp -d /tmp/md-rpmcheck.XXXXXX)"

    [ "$(rpm -qp --qf '%{NAME} %{VERSION} %{ARCH}' "$rpm")" = "$PKG $version $machine" ] ||
        { say "header: $(rpm -qp --qf '%{NAME} %{VERSION} %{ARCH}' "$rpm")"; status=1; }
    rpm -qpR "$rpm" | LC_ALL=C sort -u > "$tmp/requires"
    # dependency generators: the libraries of the -sys crates, by soname; the fonts by hand.
    for want in 'libgtk-4.so.1()(64bit)' 'libadwaita-1.so.0()(64bit)' 'libwebkitgtk-6.0.so.4()(64bit)' \
        'libjavascriptcoregtk-6.0.so.1()(64bit)' 'libglib-2.0.so.0()(64bit)' \
        hicolor-icon-theme 'font(nimbusmonops)' 'font(liberationmono)'; do
        grep -qxF "$want" "$tmp/requires" || { say "Requires: no $want"; status=1; }
    done
    say "Requires: $(grep -v '^rpmlib(' "$tmp/requires" | tr '\n' ' ')"

    # The file list is exactly the staged tree, plus rpm's build-id links and the compressed man page.
    rpm -qpl "$rpm" | grep -v '^/usr/lib/\.build-id' | LC_ALL=C sort > "$tmp/actual"
    (cd "$stage" && find . -type f -printf '/%P\n') | sed 's#^/usr/share/man/man1/md\.1$#&.gz#' |
        LC_ALL=C sort > "$tmp/files"
    # rpm also lists the directories it owns: md's data folder and the doc and licence folders.
    {
        cat "$tmp/files"
        (cd "$stage" && find usr/share/md -type d -printf '/%p\n')
        printf '%s\n' /usr/share/doc/md /usr/share/licenses/md
    } | LC_ALL=C sort > "$tmp/expected"
    if ! cmp -s "$tmp/expected" "$tmp/actual"; then
        say "contents differ from docs/packaging.md §12 (< expected, > in the .rpm):"
        diff "$tmp/expected" "$tmp/actual" >&2 || true
        status=1
    fi
    [ "$(grep -c '^/usr/share/icons/hicolor/.*/apps/me\.nettrash\.md\.png$' "$tmp/actual")" = 13 ] ||
        { say "not 13 icon sizes"; status=1; }

    local code=0
    rpmlint "$rpm" > "$tmp/rpmlint" 2>&1 || code=$?
    sed 's/^/  /' "$tmp/rpmlint" >&2
    if grep -qE '^[^ ]+: E: ' "$tmp/rpmlint" || [ "$code" -gt 1 ]; then
        say "rpmlint reports errors"; status=1
    fi
    rm -rf "$tmp"
    [ "$status" = 0 ] || die "the .rpm does not match docs/packaging.md §12"
    say "header, Requires and contents match docs/packaging.md §12; rpmlint: no error"
}

# ---------------------------------------------------------------------------------------------
# Inside a fresh Fedora 44: install, check, remove.

check_inside() {
    local arch="$1" machine version rpm status=0 dir=/tmp/md-mime got
    machine="$(machine_of "$arch")"
    version="$(tree_version /src)"
    rpm="$(find /dist -maxdepth 1 -name "$PKG-$version-1.*.$machine.rpm" -print -quit)"
    [ -n "$rpm" ] || die "no $PKG-$version-1.*.$machine.rpm in dist/"
    fail() { say "CHECK FAILED: $*"; status=1; }
    ok() { say "ok: $*"; }

    say "dnf: the checking tools"
    # Installed first so that their file triggers run when md is.
    dnf -y -q install --setopt=install_weak_deps=False \
        shared-mime-info desktop-file-utils xdg-utils glib2 appstream findutils zip > /tmp/dnf-tools.log 2>&1 ||
        { cat /tmp/dnf-tools.log >&2; die "dnf could not install the checking tools"; }

    say "dnf install ./$(basename "$rpm") — every Requires from the archive"
    dnf -y install --setopt=install_weak_deps=False "$rpm" > /tmp/dnf-install.log 2>&1 ||
        { cat /tmp/dnf-install.log >&2; die "dnf could not install the package"; }
    ok "installed: $(rpm -q "$PKG")"
    rpm -ql "$PKG" | LC_ALL=C sort > /tmp/md-files

    if ldd /usr/bin/md | grep -q 'not found'; then ldd /usr/bin/md >&2; fail "unresolved libraries"; else
        ok "ldd: every library resolves"; fi
    local v
    v="$(timeout 30 md --version 2>&1 | head -1 || true)"
    if [ "$v" = "md $version" ]; then ok "md --version: $v"
    else fail "md --version printed '$v', not 'md $version'"; fi

    check_mime_types "$dir"

    local out
    out="$(desktop-file-validate /usr/share/applications/me.nettrash.md.desktop 2>&1)" || true
    if [ -z "$out" ]; then ok "desktop-file-validate: clean"; else printf '%s\n' "$out" >&2
        printf '%s\n' "$out" | grep -qE '(error|warning):' && fail "desktop-file-validate"; fi
    if out="$(appstreamcli validate --no-net /usr/share/metainfo/me.nettrash.md.metainfo.xml 2>&1)"; then
        printf '%s\n' "$out" | sed 's/^/  /' >&2; ok "appstreamcli validate --no-net"
    else printf '%s\n' "$out" >&2; fail "appstreamcli validate --no-net"; fi

    # The user's own files, which removing md must not touch.
    mkdir -p /root/.config/md /root/.local/state/md /root/.local/share/md/drafts
    printf '[md]\nmd.pdfPageSize=a5\n' > /root/.config/md/settings.conf
    printf '{"version":1}\n' > /root/.local/state/md/session.json
    printf 'draft\n' > /root/.local/share/md/drafts/d.md
    local user_files="/root/.config/md/settings.conf /root/.local/state/md/session.json /root/.local/share/md/drafts/d.md"

    say "dnf remove $PKG"
    dnf -y remove "$PKG" > /tmp/dnf-remove.log 2>&1 || { cat /tmp/dnf-remove.log >&2; die "dnf could not remove md"; }
    rpm -q "$PKG" >/dev/null 2>&1 && fail "rpm still knows $PKG after remove" || ok "removed: rpm no longer knows md"
    local path left=""
    while IFS= read -r path; do
        if [ -f "$path" ] || [ -L "$path" ]; then left="$left $path"; fi
    done < /tmp/md-files
    [ -z "$left" ] && ok "remove left no packaged file" || fail "left after remove:$left"
    [ -e /usr/share/md ] && fail "/usr/share/md remains"
    grep -q me.nettrash.md.desktop /usr/share/applications/mimeinfo.cache 2>/dev/null &&
        fail "mimeinfo.cache still offers md"
    got="$(xdg-mime query filetype "$dir/a.mdown")"
    [ "$got" != text/markdown ] && ok "a.mdown is $got again (md's MIME globs gone)" || fail "a.mdown is still text/markdown"
    for path in $user_files; do [ -f "$path" ] || fail "remove deleted the user's $path"; done

    [ "$status" = 0 ] || die "checks failed for $(basename "$rpm")"
    say "every check passed for $(basename "$rpm")"
}

# ---------------------------------------------------------------------------------------------
# On the host.

case "${1:-}" in
    --build-inside) build_inside "$2"; exit ;;
    --check-inside) check_inside "$2"; exit ;;
esac

clean=0 check=1 allow_dirty=0
arches=()
while [ $# -gt 0 ]; do
    case "$1" in
        --clean) clean=1 ;;
        --no-check) check=0 ;;
        --allow-dirty) allow_dirty=1 ;;
        amd64 | arm64) arches+=("$1") ;;
        *) echo "usage: scripts/package-rpm.sh [--clean] [--no-check] [--allow-dirty] [amd64|arm64 …]" >&2; exit 2 ;;
    esac
    shift
done
if [ ${#arches[@]} -eq 0 ]; then
    case "$(uname -m)" in arm64 | aarch64) arches=(arm64) ;; *) arches=(amd64) ;; esac
fi

command -v docker >/dev/null || die "docker is not installed"
git -C "$ROOT" rev-parse --verify HEAD >/dev/null 2>&1 || die "$ROOT is not a git checkout"
SOURCE_DATE_EPOCH="$(git -C "$ROOT" log -1 --format=%ct HEAD)"
export SOURCE_DATE_EPOCH
BASE="$(sed -n 's/^FROM \(fedora:[0-9]*@sha256:[0-9a-f]*\).*$/\1/p' "$ROOT/docker/Dockerfile.fedora")"
[ -n "$BASE" ] || die "docker/Dockerfile.fedora does not pin fedora by digest"

export_dir="$ROOT/dist/.package-rpm-source"
rm -rf "$export_dir"
mkdir -p "$export_dir"
export_source "$ROOT" "$export_dir" "$allow_dirty"
mkdir -p "$ROOT/dist"

for arch in "${arches[@]}"; do
    image="md-linux-rpm:$arch"
    if [ "${MD_PACKAGE_SKIP_IMAGE:-0}" != 1 ]; then
        say "image $image (docker/Dockerfile.fedora, linux/$arch)"
        docker buildx build --quiet --platform "linux/$arch" --load -t "$image" \
            -f "$ROOT/docker/Dockerfile.fedora" "$ROOT/docker" >/dev/null
    fi
    if [ "$clean" = 1 ]; then
        target_volume="md-linux-rpm-clean-$arch-$$"
    else
        target_volume="md-linux-rpm-target-$(basename "$ROOT")-$arch"
    fi
    status=0
    # Its own registry volume: Fedora's cargo is newer than Ubuntu's.
    docker run --rm --platform "linux/$arch" \
        -v "$export_dir:/src:ro" \
        -v md-linux-rpm-cargo:/cargo \
        -v "$target_volume:/target" \
        -v "$ROOT/dist:/out" \
        -e SOURCE_DATE_EPOCH \
        "$image" bash /src/scripts/package-rpm.sh --build-inside "$arch" || status=$?
    if [ "$clean" = 1 ]; then docker volume rm "$target_volume" >/dev/null || true; fi
    [ "$status" = 0 ] || die "the $arch build failed"

    if [ "$check" = 1 ]; then
        if [ "${MD_PACKAGE_SKIP_IMAGE:-0}" != 1 ]; then
            base_image "$BASE" "$arch" "md-linux-fedora:$arch"
        fi
        docker run --rm --platform "linux/$arch" \
            -v "$export_dir:/src:ro" \
            -v "$ROOT/dist:/dist:ro" \
            "md-linux-fedora:$arch" bash /src/scripts/package-rpm.sh --check-inside "$arch"
    fi
done
rm -rf "$export_dir"
