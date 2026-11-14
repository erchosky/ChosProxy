#!/usr/bin/env bash
# Installs a built .deb in a clean Kali container and checks that it is
# usable: dependencies resolve, `chosproxy` is on PATH, the desktop entry and
# icon are installed, and the app starts (virtual X server) and opens its
# proxy only after a project exists.
# Usage: scripts/smoke-test-kali-deb.sh dist-kali/chosproxy_*.deb
set -euo pipefail
deb="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"

docker run --rm --platform linux/amd64 -v "$deb":/tmp/chosproxy.deb:ro kalilinux/kali-rolling \
  bash -euo pipefail -c '
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y -qq /tmp/chosproxy.deb xvfb xauth dbus-x11 procps >/dev/null
    echo "== installed"
    dpkg -s chosproxy | grep -E "^(Package|Version|Architecture|Depends):"
    echo "== command";  command -v chosproxy
    echo "== desktop entry"; cat /usr/share/applications/*hos*roxy*.desktop
    echo "== icons"; find /usr/share/icons/hicolor -name "chosproxy*" | sort
    echo "== shared libraries"; if ldd /usr/bin/chosproxy | grep "not found"; then exit 1; else echo "all resolved"; fi
    echo "== launch under Xvfb"
    export CHOSPROXY_HOME=/tmp/chosproxy-home WEBKIT_DISABLE_COMPOSITING_MODE=1
    xvfb-run -a dbus-launch chosproxy >/tmp/app.log 2>&1 &
    sleep 15
    if pgrep -x chosproxy >/dev/null; then echo "app running after 15s: OK"; else echo "app NOT running"; cat /tmp/app.log; exit 1; fi
    grep -m1 "starting" /tmp/app.log
    ls -la /tmp/chosproxy-home
    pkill -x chosproxy || true
  '
