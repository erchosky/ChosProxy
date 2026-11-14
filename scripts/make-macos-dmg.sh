#!/usr/bin/env bash
# Packs target/release/bundle/macos/ChosProxy.app into a compressed DMG with
# an Applications shortcut, using only hdiutil (no Finder scripting, so it
# works headless and without Automation permissions).
# Usage: scripts/make-macos-dmg.sh   (after `tauri build --bundles app`)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/target/release/bundle/macos/ChosProxy.app"
[ -d "$app" ] || { echo "missing $app — build the app first" >&2; exit 1; }
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
arch="$(uname -m)"
out_dir="$root/target/release/bundle/dmg"
out="$out_dir/ChosProxy_${version}_${arch}.dmg"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$out_dir"
ditto --noextattr --noqtn "$app" "$stage/ChosProxy.app"
# The repo lives in iCloud-synced ~/Documents, which tags files with
# extended attributes (FinderInfo, provenance); codesign rejects those as
# "resource fork, Finder information, or similar detritus".
xattr -cr "$stage/ChosProxy.app"
# Strip debug and local symbols with Apple's strip (the Rust toolchain's
# own strip step depends on rust-objcopy, which is not always usable).
for bin in "$stage/ChosProxy.app/Contents/MacOS/"*; do
  /usr/bin/strip -S -x "$bin"
done
# Ad-hoc signature for the whole bundle (no Apple Developer ID available):
# lets users open it via right-click → Open instead of a "damaged" error.
codesign --force --deep --sign - "$stage/ChosProxy.app"
codesign --verify --deep --strict "$stage/ChosProxy.app"
ln -s /Applications "$stage/Applications"
rm -f "$out"
hdiutil create -quiet -volname "ChosProxy" -srcfolder "$stage" -ov -format UDZO "$out"
echo "$out"
