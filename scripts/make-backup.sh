#!/usr/bin/env bash
# Creates a clean, self-contained zip for one platform: the committed source
# with full git history (editable and rebuildable) plus the ready-to-use app
# for that platform.
#
# Usage: scripts/make-backup.sh kali|mac [output-dir]   (default: backups/)
#   kali: packages from dist-kali/ (.deb, AppImage, SHA256SUMS)
#   mac:  .app and .dmg from target/release/bundle/
set -euo pipefail

platform="${1:-}"
case "$platform" in
  kali | mac) ;;
  *) echo "usage: $0 kali|mac [output-dir]" >&2; exit 2 ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"
out_dir="${2:-$root/backups}"
mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"

if [ -n "$(git -C "$root" status --porcelain)" ]; then
  echo "Uncommitted changes present; commit them first so the backup is complete." >&2
  git -C "$root" status --short >&2
  exit 1
fi

stamp="$(date +%Y%m%d-%H%M)"
rev="$(git -C "$root" rev-parse --short HEAD)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
name="chosproxy-$platform-$version-$stamp-$rev"
work="$(mktemp -d)"
trap 'hdiutil detach -quiet "$work/mnt" 2>/dev/null || true; rm -rf "$work"' EXIT
dest="$work/$name"

# A fresh clone contains exactly the committed tree and the whole history,
# without node_modules/, target/ or other local build output.
git clone --quiet --no-hardlinks "$root" "$dest/source"
git -C "$dest/source" remote remove origin

app_dir="$dest/app"
mkdir -p "$app_dir"
if [ "$platform" = kali ]; then
  compgen -G "$root/dist-kali/*.deb" >/dev/null || {
    echo "No .deb in dist-kali/ — run scripts/build-kali.sh first." >&2; exit 1; }
  cp "$root"/dist-kali/*.deb "$root"/dist-kali/*.AppImage "$root"/dist-kali/SHA256SUMS "$app_dir/"
else
  bundle="$root/target/release/bundle"
  compgen -G "$bundle/dmg/*.dmg" >/dev/null || {
    echo "No release build — run: pnpm --filter @chosproxy/desktop tauri build --bundles app" \
      "&& scripts/make-macos-dmg.sh" >&2; exit 1; }
  # Take the stripped, ad-hoc-signed .app from the DMG (the one in
  # bundle/macos is unsigned). ditto keeps the bundle's symlinks and modes.
  dmg="$(ls -t "$bundle"/dmg/*.dmg | head -1)"
  mnt="$work/mnt"
  mkdir -p "$mnt"
  hdiutil attach -quiet -readonly -nobrowse -mountpoint "$mnt" "$dmg"
  ditto --noextattr --noqtn "$mnt/ChosProxy.app" "$app_dir/ChosProxy.app"
  hdiutil detach -quiet "$mnt"
  codesign --verify --deep --strict "$app_dir/ChosProxy.app"
  cp "$dmg" "$app_dir/"
  (cd "$app_dir" && shasum -a 256 ./*.dmg > SHA256SUMS)
fi

{
  echo "ChosProxy $version — $platform package"
  echo "Created: $(date -u +%Y-%m-%dT%H:%M:%SZ)   Commit: $(git -C "$root" rev-parse HEAD)"
  echo
  echo "Use ChosProxy only on systems you own or are explicitly authorized to test."
  echo
  echo "Contents"
  echo "  app/     ready-to-use application"
  echo "  source/  full source code with git history (edit and rebuild from here)"
  echo
  if [ "$platform" = kali ]; then
    cat <<'TXT'
USE (Kali Linux x86_64)
  sudo apt install ./app/chosproxy_*_amd64.deb
  Then open "ChosProxy" from the applications menu, or run: chosproxy
  Alternative without installing:
    chmod +x app/chosproxy_*_amd64.AppImage && ./app/chosproxy_*_amd64.AppImage
  Uninstall: sudo apt remove chosproxy
  Verify downloads: (cd app && sha256sum -c SHA256SUMS)

EDIT AND REBUILD (on Kali)
  sudo apt install build-essential curl git pkg-config libwebkit2gtk-4.1-dev \
    libsoup-3.0-dev libjavascriptcoregtk-4.1-dev librsvg2-dev libxdo-dev libssl-dev
  curl https://sh.rustup.rs -sSf | sh          # Rust
  # Node.js 22+ and pnpm: https://nodejs.org  then: npm install -g pnpm
  cd source
  pnpm install
  pnpm dev                                      # run with hot reload
  pnpm --filter @chosproxy/desktop tauri build --bundles deb,appimage
  # packages appear in target/release/bundle/
  # (or, from any machine with Docker: scripts/build-kali.sh)
TXT
  else
    cat <<'TXT'
USE (macOS, Apple Silicon)
  Open app/ChosProxy_*.dmg and drag ChosProxy to Applications
  (or copy app/ChosProxy.app to /Applications).
  The app is not signed with an Apple Developer ID: the first time,
  right-click ChosProxy → Open → Open. If macOS still blocks it:
    xattr -dr com.apple.quarantine /Applications/ChosProxy.app

EDIT AND REBUILD (on macOS)
  xcode-select --install                        # Apple build tools
  curl https://sh.rustup.rs -sSf | sh          # Rust
  # Node.js 22+ and pnpm: https://nodejs.org  then: npm install -g pnpm
  cd source
  pnpm install
  pnpm dev                                      # run with hot reload
  pnpm --filter @chosproxy/desktop tauri build --bundles app
  scripts/make-macos-dmg.sh                     # signed (ad-hoc) .dmg
  # output in target/release/bundle/
TXT
  fi
  echo
  echo "Documentation: source/README.md, source/DEVELOPMENT.md, source/ROADMAP.md"
} > "$dest/LEEME-README.txt"

(cd "$work" && zip -qry -X "$out_dir/$name.zip" "$name")
(cd "$out_dir" && shasum -a 256 "$name.zip" > "$name.zip.sha256")
echo "$out_dir/$name.zip"
