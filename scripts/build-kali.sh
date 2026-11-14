#!/usr/bin/env bash
# Builds ChosProxy .deb and AppImage for Kali Linux x86_64 inside a Kali
# container. Requires Docker (on macOS: Colima with Rosetta works).
# Usage: scripts/build-kali.sh [output-dir]   (default: dist-kali/)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$(cd "$root" && mkdir -p "${1:-dist-kali}" && cd "${1:-dist-kali}" && pwd)"
image=chosproxy-kali-builder

docker build --platform linux/amd64 -t "$image" -f "$root/packaging/kali/Dockerfile" "$root/packaging/kali"

# Stream the committed source (no node_modules/target from the host).
git -C "$root" archive --format=tar HEAD | docker run --rm -i --platform linux/amd64 \
  -v chosproxy-kali-cargo:/opt/cargo/registry \
  -v chosproxy-kali-target:/src/target \
  -v "$out":/out \
  "$image" bash -euo pipefail -c '
    tar -x -C /src
    cd /src
    pnpm install --frozen-lockfile
    # .deb keeps the Kali menu category; the portable AppImage uses a
    # freedesktop-compliant desktop entry (appimagetool rejects custom
    # categories without the X- prefix).
    pnpm --filter @chosproxy/desktop tauri build --bundles deb
    pnpm --filter @chosproxy/desktop tauri build --bundles appimage --config ../../packaging/appimage.conf.json
    version="$(sed -n "s/^version = \"\(.*\)\"/\1/p" Cargo.toml | head -1)"
    rm -f /out/*.deb /out/*.AppImage /out/SHA256SUMS

    # Tauri names the Debian package after the kebab-cased product name
    # ("chos-proxy") and writes an invalid Maintainer field. Repack with the
    # package name the docs promise (`chosproxy`) and a valid Maintainer.
    work="$(mktemp -d)"
    dpkg-deb -R target/release/bundle/deb/*.deb "$work"
    sed -i \
      -e "s/^Package: .*/Package: chosproxy/" \
      -e "s/^Maintainer: .*/Maintainer: ChosProxy maintainers <maintainers@chosproxy.invalid>/" \
      "$work/DEBIAN/control"
    echo "Homepage: https://github.com/chosproxy/chosproxy" >> "$work/DEBIAN/control"
    # The headless command (M28) ships in the same package.
    cargo build --release -p chosproxy-cli
    install -D -m 0755 target/release/chosproxy-cli "$work/usr/bin/chosproxy-cli"
    dpkg-deb --build --root-owner-group "$work" "/out/chosproxy_${version}_amd64.deb" >/dev/null
    cp target/release/bundle/appimage/*.AppImage "/out/chosproxy_${version}_amd64.AppImage"
    cd /out && sha256sum ./*.deb ./*.AppImage > SHA256SUMS
  '
echo "Packages written to $out"
ls -la "$out"
