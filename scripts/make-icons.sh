#!/usr/bin/env bash
# md for Linux — the app icon in the hicolor layout (docs/packaging.md §4). Run once by hand; the
# output under packaging/icons/ is committed, so the package build never needs ImageMagick.
#
#   scripts/make-icons.sh [AppIcon.appiconset folder]
#
# The source is md.macOS's AppIcon set (default: ../md.macOS/md/Assets.xcassets/AppIcon.appiconset
# beside this repo). Every size but two is a byte copy; 24 and 48 px are resized with ImageMagick
# 7 (`magick`) — from the host if it has one, else from the md-linux-dev image — with the date and
# time chunks left out, so running the script again gives the same bytes.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="${1:-$ROOT/../md.macOS/md/Assets.xcassets/AppIcon.appiconset}"
OUT="$ROOT/packaging/icons/hicolor"
NAME="me.nettrash.md.png"
IMAGE="${MD_IMAGE:-md-linux-dev}"

if [ ! -f "$SRC/icon_16x16.png" ]; then
    echo "make-icons: no AppIcon set at $SRC (pass the .appiconset folder)" >&2
    exit 2
fi
SRC="$(cd "$SRC" && pwd)"

# magick ARGS…, with /in = the source set and /out = the hicolor folder.
magick_run() {
    if command -v magick >/dev/null 2>&1; then
        local args=()
        for a in "$@"; do a="${a//\/in\//$SRC/}"; args+=("${a//\/out\//$OUT/}"); done
        magick "${args[@]}"
    else
        docker run --rm -v "$SRC:/in:ro" -v "$OUT:/out" "$IMAGE" magick "$@"
    fi
}

# hicolor directory → source file (a byte copy) or "resize:<px>:<source>".
PLAN=(
    "16x16:icon_16x16.png"
    "16x16@2:icon_16x16@2x.png"
    "24x24:resize:24:icon_32x32@2x.png"
    "32x32:icon_32x32.png"
    "32x32@2:icon_32x32@2x.png"
    "48x48:resize:48:icon_128x128.png"
    "64x64:icon_32x32@2x.png"
    "128x128:icon_128x128.png"
    "128x128@2:icon_128x128@2x.png"
    "256x256:icon_256x256.png"
    "256x256@2:icon_256x256@2x.png"
    "512x512:icon_512x512.png"
    "512x512@2:icon_512x512@2x.png"
)

rm -rf "$OUT"
for entry in "${PLAN[@]}"; do
    dir="${entry%%:*}"
    rest="${entry#*:}"
    mkdir -p "$OUT/$dir/apps"
    if [ "${rest%%:*}" = resize ]; then
        rest="${rest#resize:}"
        px="${rest%%:*}"
        file="${rest#*:}"
        magick_run "/in/$file" -filter Lanczos -resize "${px}x${px}" -strip \
            -define png:exclude-chunks=date,time "/out/$dir/apps/$NAME"
    else
        cp "$SRC/$rest" "$OUT/$dir/apps/$NAME"
    fi
    chmod 0644 "$OUT/$dir/apps/$NAME"
done

# Every file must be the size its directory names (scale 2 doubles it), and square.
status=0
for entry in "${PLAN[@]}"; do
    dir="${entry%%:*}"
    base="${dir%@2}"
    want="${base%%x*}"
    [ "$dir" != "$base" ] && want=$((want * 2))
    got="$(magick_run identify -format '%wx%h' "/out/$dir/apps/$NAME")"
    if [ "$got" != "${want}x${want}" ]; then
        echo "make-icons: $dir/apps/$NAME is $got, expected ${want}x${want}" >&2
        status=1
    else
        echo "$dir/apps/$NAME  $got"
    fi
done
exit "$status"
