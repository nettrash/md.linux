#!/usr/bin/env bash
# md for Linux — build, test and run the app in the Ubuntu 26.04 dev image.
#
#   scripts/dev.sh image                 build the image (docker/Dockerfile) as md-linux-dev
#   scripts/dev.sh build [cargo args]    cargo build -p md (debug unless --release is passed)
#   scripts/dev.sh test                  fmt --check, clippy -D warnings (workspace + md), cargo test
#   scripts/dev.sh clippy                clippy -D warnings only
#   scripts/dev.sh fmt                   cargo fmt --all
#   scripts/dev.sh run [md args]         run md under Xvfb + a D-Bus session; a screenshot of the
#                                        screen after MD_SHOT_AFTER seconds (default 8) lands in OUT
#   scripts/dev.sh spike                 the GUI spike: md --spike OUT under Xvfb (screenshots,
#                                        PDFs and spike-results.json in OUT)
#   scripts/dev.sh probe [variant…]      md --probe <variant> OUT, one fresh process per variant
#                                        (default: all six scheme registrations)
#   scripts/dev.sh shell                 an interactive shell in the image
#
# OUT is $MD_OUT, default ./dist/spike (gitignored). On the host every subcommand runs inside
# Docker: the tree is bind-mounted at /src, OUT at /out, the cargo registry and the target
# directory are named volumes (md-linux-cargo, md-linux-target-<worktree name>) so rebuilds are
# incremental. On a Linux machine with the -dev packages installed, MD_DEV_NATIVE=1 runs the same
# subcommands directly.
#
# Experiment switches for the headless run (docs/linux-facts.md, "Docker-only workarounds"):
#   MD_WEBKIT_SANDBOX=1   keep WebKit's bubblewrap sandbox (default: disabled, as Docker needs)
#   MD_NO_DBUS=1          run without a D-Bus session bus
#   MD_PROBE_IMAGE=<url>  `probe` also loads this image (mixed-content check); MD_DOCKER_NETWORK
#                         puts the container on a network where an http server can serve it
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${MD_IMAGE:-md-linux-dev}"
OUT_HOST="${MD_OUT:-$ROOT/dist/spike}"

# ---------------------------------------------------------------------------------------------
# Inside the container (or natively on Linux with MD_DEV_NATIVE=1).

headless() {
    # Xvfb on :99 plus a private session bus: GApplication's single-instance hand-off (HANDLES_OPEN)
    # is a D-Bus call, so without a bus every launch would be a primary instance.
    local display=":99"
    Xvfb "$display" -screen 0 1440x900x24 -nolisten tcp >/tmp/xvfb.log 2>&1 &
    local xvfb=$!
    for _ in $(seq 50); do [ -e /tmp/.X11-unix/X99 ] && break; sleep 0.1; done
    export DISPLAY="$display"
    export GDK_BACKEND=x11
    # Docker only: see docs/linux-facts.md "Docker-only workarounds".
    if [ "${MD_WEBKIT_SANDBOX:-0}" != 1 ]; then export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1; fi
    # Cosmetic: Xvfb has no DRI3, and without this Mesa's EGL prints two warnings per process.
    # Rendering is identical either way (llvmpipe).
    export LIBGL_ALWAYS_SOFTWARE=1
    local status=0
    if [ "${MD_NO_DBUS:-0}" = 1 ]; then
        env -u DBUS_SESSION_BUS_ADDRESS "$@" || status=$?
    else
        dbus-run-session -- "$@" || status=$?
    fi
    kill "$xvfb" 2>/dev/null || true
    return "$status"
}

inner() {
    local cmd="${1:-}"
    shift || true
    case "$cmd" in
        build)
            cargo build -p md "$@"
            ;;
        fmt)
            cargo fmt --all
            ;;
        clippy)
            cargo clippy --workspace --all-targets -- -D warnings
            cargo clippy -p md --all-targets -- -D warnings
            ;;
        test)
            cargo fmt --all -- --check
            cargo clippy --workspace --all-targets -- -D warnings
            cargo clippy -p md --all-targets -- -D warnings
            cargo test --workspace
            ;;
        run)
            cargo build -p md
            local bin="${CARGO_TARGET_DIR:-$ROOT/target}/debug/md"
            local after="${MD_SHOT_AFTER:-8}"
            # shellcheck disable=SC2016 # the inner script expands its own arguments and $OUT
            headless bash -c '
                "$1" "${@:3}" &
                app=$!
                sleep "$2"
                import -window root "$OUT/run.png"
                echo "screenshot: $OUT/run.png"
                kill "$app" 2>/dev/null || true
                wait "$app" 2>/dev/null || true
            ' _ "$bin" "$after" "$@"
            ;;
        spike)
            cargo build -p md
            local bin="${CARGO_TARGET_DIR:-$ROOT/target}/debug/md"
            headless "$bin" --spike "$OUT"
            ;;
        probe)
            cargo build -p md
            local bin="${CARGO_TARGET_DIR:-$ROOT/target}/debug/md"
            local variants=("$@")
            [ ${#variants[@]} -eq 0 ] && variants=(plain secure cors secure_cors local secure_cors_csp)
            for v in "${variants[@]}"; do
                headless "$bin" --probe "$v" "$OUT" 2>&1 | grep -E "^PROBE|SECURITY|ERROR|terminated|bwrap|Failed" || true
            done
            ;;
        shell)
            exec bash
            ;;
        *)
            echo "usage: scripts/dev.sh {image|build|test|clippy|fmt|run|spike|probe|shell}" >&2
            exit 2
            ;;
    esac
}

if [ "${MD_DEV_NATIVE:-0}" = 1 ] || [ -f /.dockerenv ]; then
    if [ -f /.dockerenv ]; then OUT=/out; else OUT="$OUT_HOST"; fi
    export OUT
    mkdir -p "$OUT"
    cd "$ROOT"
    inner "$@"
    exit
fi

# ---------------------------------------------------------------------------------------------
# On the host: everything goes through Docker.

if [ "${1:-}" = image ]; then
    exec docker build -t "$IMAGE" -f "$ROOT/docker/Dockerfile" "$ROOT/docker"
fi

mkdir -p "$OUT_HOST"
tty_flags=(-i)
[ -t 0 ] && [ -t 1 ] && tty_flags=(-it)
exec docker run --rm "${tty_flags[@]}" \
    --init \
    --shm-size=1g \
    -v "$ROOT:/src" \
    -v "$OUT_HOST:/out" \
    -v md-linux-cargo:/cargo \
    -v "md-linux-target-$(basename "$ROOT"):/target" \
    ${MD_DOCKER_NETWORK:+--network "$MD_DOCKER_NETWORK"} \
    -e MD_SHOT_AFTER -e MD_WEBKIT_SANDBOX -e MD_NO_DBUS -e MD_PROBE_IMAGE -e MD_PROBE_PDF -e MD_PROBE_PRINTER \
    "$IMAGE" /src/scripts/dev.sh "$@"
