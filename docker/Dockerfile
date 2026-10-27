# md for Linux — the development and build image.
#
# Ubuntu 26.04's own toolchain and libraries, so what builds here is what builds on the target:
# rustc/cargo 1.93, GTK 4.22, libadwaita 1.9, WebKitGTK 6.0 2.52. Xvfb and a D-Bus session bus
# let the app run headless; ImageMagick, xdotool and poppler-utils drive and inspect it.
#
# The source tree is bind-mounted at /src; the cargo registry and the target directory live in
# named volumes (see scripts/dev.sh), so a rebuild only recompiles what changed.
FROM ubuntu:26.04

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8

# --no-install-recommends keeps the image lean; everything the build and the headless run need is
# listed. The font list is the set an Ubuntu 26.04 desktop installs (ubuntu-desktop-minimal:
# fonts-dejavu-core Depends, the rest Recommends, fonts-urw-base35 via ghostscript) minus the
# CJK family (fonts-noto-cjk), plus fonts-courier-prime, kept only so the font specimen can show why it was
# rejected (docs/linux-facts.md §7).
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates git build-essential pkg-config \
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
