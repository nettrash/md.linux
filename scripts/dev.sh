#!/usr/bin/env bash
# md for Linux — build, test and run the app in the Ubuntu 26.04 dev image.
#
#   scripts/dev.sh image                 build the image (docker/Dockerfile) as md-linux-dev
#   scripts/dev.sh build [cargo args]    cargo build -p md (debug unless --release is passed)
#   scripts/dev.sh test                  fmt --check; clippy -D warnings on the workspace, on md and
#                                        on md --features selftest; cargo test (workspace, and md
#                                        with the self-test's own unit tests)
#   scripts/dev.sh clippy                the three clippy runs only
#   scripts/dev.sh fmt                   cargo fmt --all
#   scripts/dev.sh run [md args]         run md under Xvfb + a D-Bus session; a screenshot of the
#                                        screen after MD_SHOT_AFTER seconds (default 8): OUT/run.png
#   scripts/dev.sh selftest [args]       md --selftest OUT/selftest [args] under Xvfb, built with the
#                                        `selftest` feature, its profile in OUT/selftest/home (wiped
#                                        first); args: --only group,… and --no-input
#                                        (docs/shell-design.md §13). Exits with the run's verdict.
#   scripts/dev.sh package [arch]        the .deb — delegates to scripts/package.sh (on the host);
#                                        with MD_DEV_NATIVE=1, to scripts/package.sh --native
#   scripts/dev.sh shell                 an interactive shell in the image
#
# OUT is $MD_OUT, default ./dist (gitignored). On the host every subcommand but `image` and
# `package` runs inside Docker: the tree is bind-mounted at /src, OUT at /out, the cargo registry
# and the target directory are named volumes (md-linux-cargo, md-linux-target-<worktree name>) so
# rebuilds are incremental. On a Linux machine with the -dev packages installed, MD_DEV_NATIVE=1
# runs the same subcommands directly.
#
# Passed into the container when set on the host:
#   GDK_SCALE=2                      HiDPI under Xvfb (the exports' 2× division, §7.5)
#   ADW_DEBUG_COLOR_SCHEME=prefer-dark   libadwaita's system preference — the carbon-paper theme
#   MD_SHOT_AFTER                    `run`'s screenshot delay in seconds
# Experiment switches for the headless run (docs/linux-facts.md §9, "Docker-only workarounds"):
#   MD_WEBKIT_SANDBOX=1   keep WebKit's bubblewrap sandbox (default: disabled, as Docker needs)
#   MD_NO_DBUS=1          run without a D-Bus session bus
#   MD_DOCKER_NETWORK     puts the container on a Docker network (an http server for remote images)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${MD_IMAGE:-md-linux-dev}"
OUT_HOST="${MD_OUT:-$ROOT/dist}"

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
    # Docker only: see docs/linux-facts.md §9. Docker's seccomp profile denies bubblewrap its user
    # namespace, so WebKit's sandbox cannot start, and md's own preflight (docs/shell-design.md
    # §4.1) is skipped for the same reason. A release build ignores MD_SKIP_SANDBOX_PROBE.
    if [ "${MD_WEBKIT_SANDBOX:-0}" != 1 ]; then
        export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
        export MD_SKIP_SANDBOX_PROBE=1
    fi
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
    wait "$xvfb" 2>/dev/null || true
    return "$status"
}

clippy_all() {
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy -p md --all-targets -- -D warnings
    cargo clippy -p md --all-targets --features selftest -- -D warnings
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
            clippy_all
            ;;
        test)
            cargo fmt --all -- --check
            clippy_all
            cargo test --workspace
            cargo test -p md --features selftest
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
        selftest)
            cargo build -p md --features selftest
            local bin="${CARGO_TARGET_DIR:-$ROOT/target}/debug/md"
            local out="$OUT/selftest"
            # A fresh profile and no stale report: a run never inherits another's state.
            rm -rf "$out"
            mkdir -p "$out/home"
            # md refuses to self-test unless all four point into its output folder (main.rs).
            export XDG_CONFIG_HOME="$out/home/.config"
            export XDG_STATE_HOME="$out/home/.local/state"
            export XDG_DATA_HOME="$out/home/.local/share"
            export XDG_CACHE_HOME="$out/home/.cache"
            headless "$bin" --selftest "$out" "$@"
            ;;
        package)
            exec "$ROOT/scripts/package.sh" --native "$@"
            ;;
        shell)
            exec bash
            ;;
        *)
            echo "usage: scripts/dev.sh {image|build|test|clippy|fmt|run|selftest|package|shell}" >&2
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
# On the host: everything goes through Docker, except building the image and packaging (which
# picks its own image per architecture).

if [ "${1:-}" = image ]; then
    exec docker build -t "$IMAGE" -f "$ROOT/docker/Dockerfile" "$ROOT/docker"
fi

if [ "${1:-}" = package ]; then
    shift
    if [ ! -x "$ROOT/scripts/package.sh" ]; then
        echo "scripts/package.sh is not here yet (work package P6, docs/packaging.md)" >&2
        exit 2
    fi
    exec "$ROOT/scripts/package.sh" "$@"
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
    -e MD_SHOT_AFTER -e MD_WEBKIT_SANDBOX -e MD_NO_DBUS -e GDK_SCALE -e ADW_DEBUG_COLOR_SCHEME \
    "$IMAGE" /src/scripts/dev.sh "$@"
