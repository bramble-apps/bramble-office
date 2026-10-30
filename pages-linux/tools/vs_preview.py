#!/usr/bin/env python3
"""Letters' page 1 against the preview Pages saved inside each .pages (Pages' own render).
Prints a difference score per document, worst first, and writes side by sides.
  tools/vs_preview.py outdir doc.pages …"""
import os, subprocess, sys, zipfile, io
from PIL import Image, ImageChops, ImageStat
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
out = sys.argv[1]
os.makedirs(out, exist_ok=True)
res = []
for src in sys.argv[2:]:
    stem = os.path.splitext(os.path.basename(src))[0]
    try:
        with zipfile.ZipFile(src) as z:
            names = z.namelist()
            pv = next((n for n in ["preview.jpg", "QuickLook/Thumbnail.jpg"] if n in names), None)
            if not pv:
                continue
            ref = Image.open(io.BytesIO(z.read(pv))).convert("RGB")
    except Exception:
        continue
    prefix = os.path.join(out, "cs")
    for f in os.listdir(out):
        if f.startswith("cs-"):
            os.remove(os.path.join(out, f))
    r = subprocess.run([os.path.join(root, "target/release/examples/snap"), src, prefix, "0.5"], capture_output=True, timeout=300)
    first = os.path.join(out, "cs-1.png")
    if not os.path.exists(first):
        res.append((999, stem, "no render"))
        continue
    cs = Image.open(first).convert("RGB")
    size = (306, int(306 * ref.height / ref.width))
    a, b = ref.resize(size), cs.resize(size)
    score = round(ImageStat.Stat(ImageChops.difference(a.convert("L"), b.convert("L"))).mean[0], 1)
    side = Image.new("RGB", (size[0] * 2 + 10, size[1]), "white")
    side.paste(a, (0, 0))
    side.paste(b, (size[0] + 10, 0))
    side.save(os.path.join(out, f"{score:05.1f} {stem}.png"))
    res.append((score, stem, ""))
for s, n, note in sorted(res, reverse=True):
    print(f"{s:6.1f}  {n} {note}")
