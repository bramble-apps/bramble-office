#!/usr/bin/env python3
"""Text line positions on page 1: Pages' preview against Letters. Prints, per document, how far
Letters' 5th / 10th / 20th text line is from Pages' (points, + = lower) and the line counts.
  tools/line_drift.py doc.pages …"""
import os, subprocess, sys, zipfile, io, tempfile
from PIL import Image
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
def rows(im):
    im = im.convert("L").resize((612, int(612 * im.height / im.width)))
    w, h = im.size
    px = im.load()
    out, inl = [], False
    for y in range(h):
        v = sum(1 for x in range(60, 552, 2) if px[x, y] < 140)
        if v > 1 and not inl:
            s, inl = y, True
        elif v <= 1 and inl:
            inl = False
            if y - s >= 3:
                out.append(s)
    return out
tmp = tempfile.mkdtemp()
tot = []
for src in sys.argv[1:]:
    try:
        ref = Image.open(io.BytesIO(zipfile.ZipFile(src).read("preview.jpg")))
    except Exception:
        continue
    for f in os.listdir(tmp):
        os.remove(os.path.join(tmp, f))
    subprocess.run([os.path.join(root, "target/release/examples/snap"), src, os.path.join(tmp, "cs"), "1"], capture_output=True, timeout=300)
    p = os.path.join(tmp, "cs-1.png")
    if not os.path.exists(p):
        continue
    a, b = rows(ref), rows(Image.open(p))
    d = [b[k] - a[k] for k in (4, 9, 19) if k < min(len(a), len(b))]
    tot.extend(abs(x) for x in d[-1:])
    print(f"{os.path.basename(src)[:44]:44} lines {len(a):3}/{len(b):3}  drift {d}")
print("mean |drift| at the last checked line:", round(sum(tot) / max(len(tot), 1), 1))
