#!/usr/bin/env python3
"""HelloPlugin: the minimal ChosProxy plugin.

ChosProxy writes one JSON object per line to this program's standard input
and reads one JSON object per line from its standard output (see
PLUGIN_SDK.md). This plugin looks at every response it receives and,
when the response carries an X-Debug-Token header, reports an informational
finding and highlights the request in the history.
"""

import json
import sys

HEADER = "x-debug-token"


def send(message):
    """Writes one message to ChosProxy."""
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def log(text, level="info"):
    send({"type": "log", "level": level, "message": text})


def on_exchange(exchange, granted):
    headers = {name.lower(): value for name, value in exchange["responseHeaders"]}
    token = headers.get(HEADER)
    if token is None:
        return
    log(f"{HEADER} seen on {exchange['url']}")
    if "scanner.passive" in granted:
        send({
            "type": "finding",
            "exchangeId": exchange["id"],
            "key": exchange["host"],
            "kind": "Debug token exposed",
            "title": f"Debug token header on {exchange['host']}",
            "severity": "info",
            "confidence": "certain",
            "description": "The response carries an X-Debug-Token header. Frameworks send it "
            "when a debug profiler is enabled; the profiler may show requests, configuration "
            "and database queries to anyone who opens it.",
            "remediation": "Turn off the debug profiler in production.",
            "evidence": f"X-Debug-Token: {token[:12]}",
        })
    if "project.write" in granted:
        send({
            "type": "annotate",
            "exchangeId": exchange["id"],
            "highlight": "yellow",
            "note": "HelloPlugin: X-Debug-Token header in the response",
        })


def main():
    granted = []
    for line in sys.stdin:
        message = json.loads(line)
        kind = message.get("type")
        if kind == "hello":
            granted = message.get("granted", [])
            send({"type": "ready"})
            log("HelloPlugin started with: " + (", ".join(granted) or "no permissions"))
        elif kind == "exchange":
            on_exchange(message, granted)
        elif kind == "shutdown":
            break


if __name__ == "__main__":
    main()
