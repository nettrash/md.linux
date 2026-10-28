# shellcheck shell=bash
# md for Linux — what scripts/package.sh (.deb, .tar.gz) and scripts/package-rpm.sh (.rpm) share.
# Sourced, on the host and inside the containers; it only defines functions. The callers define
# `say` and `die`, and check_mime_types' callers `ok` and `fail`.

# The workspace's version: [workspace.package] in $1/Cargo.toml.
tree_version() {
    sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' "$1/Cargo.toml"
}

# Step 2 of both scripts: the source of the checkout $1 from git into the empty folder $2 — HEAD
# itself when the tree is clean, else ($3 = 1, --allow-dirty) the tracked and untracked-but-not-
# ignored files — so nothing stray (a .DS_Store in examples/) can reach a package.
export_source() {
    local root="$1" dest="$2" allow_dirty="$3"
    if [ -z "$(git -C "$root" status --porcelain)" ]; then
        git -C "$root" archive --format=tar HEAD | tar -x -C "$dest"
        say "source: HEAD $(git -C "$root" rev-parse --short HEAD), SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH"
    elif [ "$allow_dirty" = 1 ]; then
        (cd "$root" && git ls-files -z --cached --others --exclude-standard |
            while IFS= read -r -d '' f; do [ -e "$f" ] && printf '%s\0' "$f"; done |
            tar --null -T - -cf -) | tar -x -C "$dest"
        say "source: the working tree with uncommitted changes (--allow-dirty), SOURCE_DATE_EPOCH=$SOURCE_DATE_EPOCH of HEAD"
    else
        die "the working tree has uncommitted changes: commit them, or pass --allow-dirty"
    fi
}

# The release build's flags: the containers' paths in the binary wherever it is built — source $1,
# cargo home $2, target directory $3. Encoded, one per \x1f, so a path with a space survives, and
# replacing any RUSTFLAGS or build.rustflags of the machine's.
export_rustflags() {
    local sep=$'\x1f'
    export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$1=/build/md$sep--remap-path-prefix=$2=/cargo$sep--remap-path-prefix=$3=/build/target"
}

# After installing md: one file of every extension typed as md declares it, md offered for each
# type, and md the default for text/markdown. Sample files go in $1.
check_mime_types() {
    # MIME: one file of every extension, typed by GIO as GNOME Files types it (xdg-mime asks gio on
    # GNOME). Not refreshed by hand: the package manager's trigger (dpkg's, or rpm's file trigger of
    # shared-mime-info and desktop-file-utils) must already have run.
    export XDG_CURRENT_DESKTOP=GNOME GIO_USE_VFS=local
    local dir="$1" ext want got
    mkdir -p "$dir/pack"
    printf '# Title\n\nSome *text*.\n' > "$dir/pack/text.md"
    (cd "$dir/pack" && zip -q -X ../a.textpack text.md)
    while read -r ext want; do
        case "$ext" in
            textpack) ;;
            puml | plantuml | iuml | pu) printf '@startuml\nAlice -> Bob: hello\n@enduml\n' > "$dir/a.$ext" ;;
            gv) printf 'digraph g {\n  a -> b;\n}\n' > "$dir/a.$ext" ;;
            txt | text) printf 'Plain text.\n' > "$dir/a.$ext" ;;
            *) printf '# Title\n\nSome *text*.\n' > "$dir/a.$ext" ;;
        esac
        got="$(xdg-mime query filetype "$dir/a.$ext" 2>&1)"
        if [ "$got" = "$want" ]; then ok "xdg-mime query filetype a.$ext = $got"
        else fail "a.$ext is '$got', want $want"; fi
    done <<EOF
md text/markdown
markdown text/markdown
mdown text/markdown
markdn text/markdown
mdtext text/markdown
mdtxt text/markdown
mkd text/markdown
mkdn text/markdown
mdwn text/markdown
mkdown text/markdown
txt text/plain
text text/plain
puml text/x-plantuml
plantuml text/x-plantuml
iuml text/x-plantuml
pu text/x-plantuml
gv text/vnd.graphviz
textpack application/x-textpack
EOF
    local t
    for t in text/markdown text/plain text/x-plantuml text/vnd.graphviz application/x-textpack; do
        if grep -q "^$t=.*me\.nettrash\.md\.desktop" /usr/share/applications/mimeinfo.cache; then
            ok "mimeinfo.cache offers md for $t"; else fail "mimeinfo.cache does not offer md for $t"; fi
    done
    got="$(xdg-mime query default text/markdown)"
    [ "$got" = me.nettrash.md.desktop ] && ok "xdg-mime query default text/markdown = $got" ||
        fail "default for text/markdown is '$got'"

}
