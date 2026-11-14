#!/usr/bin/env python3
"""Renders the ChosProxy source icon (1024x1024 PNG).

Usage: python3 scripts/generate-icon.py apps/desktop/src-tauri/icons/source.png
Then:  pnpm --filter @chosproxy/desktop tauri icon src-tauri/icons/source.png

Design: a dark rounded tile with two opposing brackets (traffic passing
through an inspection point) and a central node.
"""
import sys
from PIL import Image, ImageDraw

S = 1024
SS = 4  # supersampling factor for smooth edges
W = S * SS

bg = (16, 22, 30, 255)
accent = (45, 212, 191, 255)   # teal
accent2 = (56, 189, 248, 255)  # sky

img = Image.new("RGBA", (W, W), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
r = int(W * 0.22)
d.rounded_rectangle([0, 0, W - 1, W - 1], radius=r, fill=bg)

stroke = int(W * 0.075)
# Left bracket "<"
cx, cy = W // 2, W // 2
span = int(W * 0.20)
gap = int(W * 0.17)
d.line([(cx - gap, cy - span), (cx - gap - span, cy), (cx - gap, cy + span)],
       fill=accent, width=stroke, joint="curve")
# Right bracket ">"
d.line([(cx + gap, cy - span), (cx + gap + span, cy), (cx + gap, cy + span)],
       fill=accent2, width=stroke, joint="curve")
# Rounded caps
for (x, y), col in [((cx - gap, cy - span), accent), ((cx - gap, cy + span), accent),
                    ((cx - gap - span, cy), accent), ((cx + gap, cy - span), accent2),
                    ((cx + gap, cy + span), accent2), ((cx + gap + span, cy), accent2)]:
    d.ellipse([x - stroke // 2, y - stroke // 2, x + stroke // 2, y + stroke // 2], fill=col)
# Central node
n = int(W * 0.065)
d.ellipse([cx - n, cy - n, cx + n, cy + n], fill=(226, 232, 240, 255))

img = img.resize((S, S), Image.LANCZOS)
img.save(sys.argv[1] if len(sys.argv) > 1 else "source.png")
