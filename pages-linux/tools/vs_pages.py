#!/usr/bin/env python3
"""Letters against Pages' own PDF exports: page counts (Pages, Letters, OnlyOffice reading the
.pages, OnlyOffice reading Letters' Word export) and how far each Letters page is from Pages'
(mean pixel difference at 36 dpi, 0 = identical). Writes a sheet per document.
  tools/vs_pages.py outdir doc.pages:pages.pdf …"""
import os, subprocess, sys, glob
from PIL import Image, ImageChops, ImageDraw, ImageStat
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ex = os.path.join(root, "target/release/examples")
out = sys.argv[1]
os.makedirs(out, exist_ok=True)
def pdf_pages(p):
    r = subprocess.run(["pdfinfo", p], capture_output=True, text=True).stdout
    return next((int(l.split()[1]) for l in r.splitlines() if l.startswith("Pages:")), 0)
def raster(pdf, prefix):
    for f in glob.glob(prefix + "-*.png"):
        os.remove(f)
    subprocess.run(["pdftoppm", "-r", "36", "-png", pdf, prefix])
    return sorted(glob.glob(prefix + "-*.png"), key=lambda f: int(os.path.splitext(f)[0].rsplit("-", 1)[1]))
for arg in sys.argv[2:]:
    src, ref = arg.split(":")
    stem = os.path.splitext(os.path.basename(src))[0]
    d = os.path.join(out, stem)
    os.makedirs(d, exist_ok=True)
    cs_pdf = os.path.join(d, "letters.pdf")
    subprocess.run([os.path.join(root, "target/release/letters"), "--pdf", src, cs_pdf], capture_output=True)
    docx = os.path.join(d, stem + ".docx")
    subprocess.run([os.path.join(root, "target/release/letters"), "--docx", src, docx], capture_output=True)
    oo_pages, oo_docx = os.path.join(d, "oo-pages.pdf"), os.path.join(d, "oo-docx.pdf")
    subprocess.run([os.path.join(root, "tools/oo-render.sh"), src, oo_pages], capture_output=True)
    subprocess.run([os.path.join(root, "tools/oo-render.sh"), docx, oo_docx], capture_output=True)
    ims = {k: raster(p, os.path.join(d, k)) for k, p in [("pg", ref), ("cs", cs_pdf), ("op", oo_pages), ("od", oo_docx)] if os.path.exists(p)}
    diffs = []
    for a, b in zip(ims.get("pg", []), ims.get("cs", [])):
        A, B = Image.open(a).convert("L"), Image.open(b).convert("L").resize(Image.open(a).size)
        diffs.append(round(ImageStat.Stat(ImageChops.difference(A, B)).mean[0], 1))
    print(f"{stem}: pages Pages {pdf_pages(ref)} | Letters {pdf_pages(cs_pdf)} | OnlyOffice(.pages) {pdf_pages(oo_pages) if os.path.exists(oo_pages) else '-'} | OnlyOffice(Word export) {pdf_pages(oo_docx) if os.path.exists(oo_docx) else '-'} | diff per page {diffs}")
    n = max(len(v) for v in ims.values())
    W, H = 220, 285
    sheet = Image.new("RGB", (4 * (W + 8), n * (H + 16) + 16), "white")
    dr = ImageDraw.Draw(sheet)
    for k, t in enumerate(["Pages", "Letters", "OnlyOffice (.pages)", "OnlyOffice (Word export)"]):
        dr.text((k * (W + 8) + 2, 2), t, fill="black")
    for k, key in enumerate(["pg", "cs", "op", "od"]):
        for i, f in enumerate(ims.get(key, [])):
            sheet.paste(Image.open(f).convert("RGB").resize((W, H)), (k * (W + 8), 16 + i * (H + 16)))
    sheet.save(os.path.join(d, "sheet.png"))
