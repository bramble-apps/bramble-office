#!/usr/bin/env python3
"""Text lines of two renders of the same page, by ink: each line's top and bottom (points) and
where it starts and ends across, side by side, with the difference. For checking that two apps
(Letters, OnlyOffice, Pages) put lines in the same places.
  tools/ink_lines.py a.png b.png [points-per-pixel]   (renders at the same size)"""
import sys
from PIL import Image

def lines(path, k):
    im = Image.open(path).convert('L')
    w, h = im.size
    px = im.load()
    rows = [any(px[x, y] < 140 for x in range(w)) for y in range(h)]
    out, y = [], 0
    while y < h:
        if rows[y]:
            y0 = y
            while y < h and rows[y]:
                y += 1
            xs = [x for x in range(w) for yy in range(y0, y) if px[x, yy] < 140]
            out.append((y0 * k, y * k, min(xs) * k, max(xs) * k))
        y += 1
    return out

k = float(sys.argv[3]) if len(sys.argv) > 3 else 1.0
a, b = lines(sys.argv[1], k), lines(sys.argv[2], k)
print(f"{'a top':>7} {'b top':>7} {'dy':>6}   {'a x':>6} {'b x':>6} {'a end':>6} {'b end':>6}")
for i in range(max(len(a), len(b))):
    la = a[i] if i < len(a) else None
    lb = b[i] if i < len(b) else None
    if la and lb:
        print(f"{la[0]:7.1f} {lb[0]:7.1f} {lb[0]-la[0]:6.1f}   {la[2]:6.1f} {lb[2]:6.1f} {la[3]:6.1f} {lb[3]:6.1f}")
    else:
        print(la or '', lb or '')
