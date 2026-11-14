# Build environment for ChosProxy Linux packages on Kali (x86_64).
# Contains toolchains only; the source is streamed in at build time.
FROM kalilinux/kali-rolling

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential ca-certificates curl file git pkg-config xz-utils patchelf \
      libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
      librsvg2-dev libxdo-dev libssl-dev libayatana-appindicator3-dev \
    && rm -rf /var/lib/apt/lists/*

# Rust (stable) — matches rust-toolchain.toml.
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile minimal --default-toolchain stable -c rustfmt -c clippy

# Node.js 22 LTS + pnpm (version pinned by package.json#packageManager).
ARG NODE_VERSION=22.23.2
RUN curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
      | tar -xJ -C /usr/local --strip-components=1 \
    && npm install -g pnpm@11.21.0

# linuxdeploy (used for the AppImage) is itself an AppImage; FUSE is not
# available in containers, so let AppImages extract themselves instead.
ENV APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=true

# The appimage plugin of linuxdeploy ships a runtime whose ELF header carries
# the AppImage magic bytes ("AI\x02" at offset 8). x86_64 emulation (Rosetta
# or QEMU binfmt on ARM hosts) refuses to execute such files ("Exec format
# error"). Pre-extract the plugin once with the magic zeroed and put a wrapper
# where the Tauri bundler looks for it. Harmless on native x86_64 hosts; the
# AppImage that is produced keeps a normal header.
RUN curl -fsSL -o /tmp/plugin.AppImage \
      https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage \
    && dd if=/dev/zero of=/tmp/plugin.AppImage bs=1 count=3 seek=8 conv=notrunc \
    && chmod +x /tmp/plugin.AppImage \
    && cd /opt && /tmp/plugin.AppImage --appimage-extract >/dev/null \
    && mv /opt/squashfs-root /opt/linuxdeploy-plugin-appimage && rm /tmp/plugin.AppImage \
    && mkdir -p /root/.cache/tauri \
    && printf '#!/bin/sh\nexec /opt/linuxdeploy-plugin-appimage/AppRun "$@"\n' \
         > /root/.cache/tauri/linuxdeploy-plugin-appimage.AppImage \
    && chmod +x /root/.cache/tauri/linuxdeploy-plugin-appimage.AppImage

WORKDIR /src
