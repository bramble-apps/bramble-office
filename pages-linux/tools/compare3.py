#!/usr/bin/env python3
"""Side by side, page by page: a .pages document in Letters, its Word export in OnlyOffice and
the Word export read back into Letters (plus Pages' own first-page preview, or Pages' PDF when
one is given). Prints page counts; writes a contact sheet.
  tools/compare3.py doc.pages outdir [pages-pdf] [page,page,…]"""
import os, subprocess, sys, zipfile, glob
from PIL import Image, ImageDraw
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ex = os.path.join(root, "target/release/examples")
src, out = sys.argv[1], sys.argv[2]
pages_pdf = sys.argv[3] if len(sys.argv) > 3 and sys.argv[3].endswith(".pdf") else None
which = [int(x) for x in (sys.argv[-1] if len(sys.argv) > 3 and not sys.argv[-1].endswith(".pdf") else "1,2,3,5,8").split(",")]
os.makedirs(out, exist_ok=True)
stem = os.path.splitext(os.path.basename(src))[0]
docx = os.path.join(out, stem + ".rt.docx")
subprocess.run([os.path.join(ex, "rtcheck"), out, src], check=True, capture_output=True)
for pre in ("orig", "back", "oo", "pg"):
    for f in glob.glob(os.path.join(out, f"{pre}-*.png")):
        os.remove(f)
subprocess.run([os.path.join(ex, "snap"), src, os.path.join(out, "orig"), "0.5"], capture_output=True)
subprocess.run([os.path.join(ex, "snap"), docx, os.path.join(out, "back"), "0.5"], capture_output=True)
oo = os.path.join(out, "oo.pdf")
subprocess.run([os.path.join(root, "tools/oo-render.sh"), docx, oo], check=True)
def count_pdf(p):
    for l in subprocess.run(["pdfinfo", p], capture_output=True, text=True).stdout.splitlines():
        if l.startswith("Pages:"):
            return int(l.split()[1])
subprocess.run(["pdftoppm", "-r", "36", "-l", str(max(which)), "-png", oo, os.path.join(out, "oo")])
ref = None
if pages_pdf:
    subprocess.run(["pdftoppm", "-r", "36", "-l", str(max(which)), "-png", pages_pdf, os.path.join(out, "pg")])
    ref = "Pages PDF"
else:
    try:
        with zipfile.ZipFile(src) as z:
            open(os.path.join(out, "pg-1.png"), "wb").write(z.read("preview.jpg"))
        ref = "Pages preview (p1)"
    except Exception:
        pass
def page(pre, i):
    for c in sorted(glob.glob(os.path.join(out, f"{pre}-*.png"))):
        n = os.path.splitext(c)[0].rsplit("-", 1)[1]
        if n.isdigit() and int(n) == i:
            return Image.open(c).convert("RGB")
    return None
n_orig = len(glob.glob(os.path.join(out, "orig-*.png")))
n_back = len(glob.glob(os.path.join(out, "back-*.png")))
print(f"{stem}: Letters {n_orig} pages | OnlyOffice {count_pdf(oo)} | read back {n_back}" + (f" | Pages {count_pdf(pages_pdf)}" if pages_pdf else ""))
W, H = 306, 396
cols = [("pg", ref or "—"), ("orig", "Letters"), ("oo", "OnlyOffice (Word export)"), ("back", "Letters (Word read back)")]
sheet = Image.new("RGB", (len(cols) * (W + 10), len(which) * (H + 20) + 20), "white")
d = ImageDraw.Draw(sheet)
for k, (_, t) in enumerate(cols):
    d.text((k * (W + 10) + 4, 4), t, fill="black")
for r, i in enumerate(which):
    y = 20 + r * (H + 20)
    d.text((2, y + 2), f"p{i}", fill="red")
    for k, (pre, _) in enumerate(cols):
        im = page(pre, i)
        if im:
            sheet.paste(im.resize((W, H)), (k * (W + 10), y))
sheet.save(os.path.join(out, "compare.png"))
