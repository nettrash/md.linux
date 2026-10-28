# md for Linux — the development and build image.
#
# Ubuntu 26.04's own toolchain and libraries, so what builds here is what builds on the target:
# rustc/cargo 1.93, GTK 4.22, libadwaita 1.9, WebKitGTK 6.0 2.52. Xvfb and a D-Bus session bus
# let the app run headless; ImageMagick, xdotool and poppler-utils drive and inspect it.
#
# The source tree is bind-mounted at /src; the cargo registry and the target directory live in
# named volumes (see scripts/dev.sh), so a rebuild only recompiles what changed.
#
# The same file builds the .deb for amd64 and arm64 (scripts/package.sh, docs/packaging.md §9), so
# both the base image and the package set are pinned: a rebuild of one commit gets the same
# compiler and the same GTK and WebKit headers, and gives the same package bytes.
#
# The base is the multi-arch index digest of ubuntu:26.04 (Ubuntu 26.04.1, image of 2026-10-24).
FROM ubuntu:26.04@sha256:19fcd1bf268f02b60aca3bd30bd8a6d9a11759a430e32921833e65f16d87348a

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8

# The Ubuntu archive as it was at this moment (snapshot.ubuntu.com). Moving it is a deliberate
# commit, like a dependency bump.
ARG UBUNTU_SNAPSHOT=20261027T000000Z

# Pinning the archive, measured on 2026-10-27:
# - The snapshot service is https only and the base image has no CA bundle, so ca-certificates is
#   installed first from the live archive. It is the one unpinned package; nothing built here
#   reads it.
# - `APT::Snapshot` makes every apt call use the snapshot. apt maps archive.ubuntu.com and
#   security.ubuntu.com (amd64) by itself, but not ports.ubuntu.com (arm64), and maps nothing at
#   all on the first update of an empty list cache — so ports is mapped by hand, to /ubuntu/: the
#   snapshot service serves every architecture there (its /ubuntu-ports/ answers 401).
# - An unreachable snapshot makes apt fall back to the live archive with only an "Ign:" line, so
#   the update is checked: every package index must come from the snapshot.
#
# --no-install-recommends keeps the image lean; everything the build and the headless run need is
# listed. The font list is the set an Ubuntu 26.04 desktop installs (ubuntu-desktop-minimal:
# fonts-dejavu-core Depends, the rest Recommends, fonts-urw-base35 via ghostscript) minus the
# CJK family (fonts-noto-cjk), plus fonts-courier-prime, kept only so the font specimen can show why it was
# rejected (docs/linux-facts.md §7). dpkg-dev (dpkg-shlibdeps) comes with build-essential.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && printf '%s\n' \
      "APT::Snapshot \"${UBUNTU_SNAPSHOT}\";" \
      'Acquire::Snapshots::URI::Host::ports.ubuntu.com "https://snapshot.ubuntu.com/ubuntu/@SNAPSHOTID@/";' \
      > /etc/apt/apt.conf.d/50md-snapshot \
 && apt-get update \
 && apt-cache policy | grep -q "snapshot.ubuntu.com/ubuntu/${UBUNTU_SNAPSHOT} " \
 && if apt-cache policy | grep -E '^ +[0-9]+ https?://' | grep -v "snapshot.ubuntu.com/ubuntu/${UBUNTU_SNAPSHOT} "; then \
      echo "apt is not reading the ${UBUNTU_SNAPSHOT} snapshot alone" >&2; exit 1; \
    fi \
 && apt-get install -y --no-install-recommends \
      git build-essential pkg-config \
      rustc cargo rust-clippy rustfmt \
      libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev \
      xvfb xauth dbus-daemon dbus-bin dbus-user-session at-spi2-core \
      libgl1-mesa-dri libegl-mesa0 \
      imagemagick x11-apps x11-utils xdotool poppler-utils \
      fontconfig fonts-dejavu-core fonts-dejavu-mono fonts-liberation fonts-urw-base35 \
      fonts-noto-core fonts-noto-mono fonts-noto-color-emoji fonts-ubuntu fonts-courier-prime \
 && rm -rf /var/lib/apt/lists/*

ENV CARGO_HOME=/cargo \
    CARGO_TARGET_DIR=/target \
    XDG_RUNTIME_DIR=/run/user/0

RUN mkdir -p /cargo /target /run/user/0 /src && chmod 700 /run/user/0

WORKDIR /src
