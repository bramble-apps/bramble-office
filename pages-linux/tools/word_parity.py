#!/usr/bin/env python3
"""Word by word: where OnlyOffice puts each word (its PDF, pdftotext -bbox) against where
Letters does (examples/words --tsv), page by page. Words are paired by text in reading order;
the vertical offset (Letters' baseline vs the PDF box's bottom, which depends on the text's
size) is taken out per page and box height as the median; pairs far apart (a repeated word
paired with the wrong one) are left out.
  tools/word_parity.py oo.pdf file.docx [tolerance-pt]
Prints per page: words paired, how many sit more than the tolerance (default 1 pt) off, the
largest offsets, and the first few words that differ, then a summary line."""
import difflib, html, re, statistics, subprocess, sys, os

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
pdf, doc = sys.argv[1], sys.argv[2]
tol = float(sys.argv[3]) if len(sys.argv) > 3 else 1.0
words_bin = os.path.join(root, "target/release/examples/words")

bbox = subprocess.run(["pdftotext", "-bbox", pdf, "-"], capture_output=True, text=True).stdout
pages = re.findall(r"<page[^>]*>(.*?)</page>", bbox, re.S)
total = bad_total = 0
worst = 0.0
for i, pg in enumerate(pages):
    oo = [(float(a), float(c), float(d), html.unescape(t), round(float(d) - float(b))) for a, b, c, d, t in re.findall(r'xMin="([\d.]+)" yMin="([\d.]+)" xMax="([\d.]+)" yMax="([\d.]+)">([^<]*)<', pg)]
    out = subprocess.run([words_bin, doc, str(i), "--tsv"], capture_output=True, text=True).stdout
    cs = []
    for ln in out.splitlines():
        p = ln.split("\t")
        if len(p) == 4:
            cs.append((float(p[0]), float(p[1]), float(p[2]), p[3]))
    # pair by text (pdftotext splits at spaces; Letters' words are glyph runs between spaces)
    norm = lambda t: re.sub(r"\s+", "", t)
    sm = difflib.SequenceMatcher(None, [norm(w[3]) for w in oo], [norm(w[3]) for w in cs], autojunk=False)
    pairs = [(oo[a + k], cs[b + k]) for a, b, n in sm.get_matching_blocks() for k in range(n)]
    if not pairs:
        print(f"page {i+1}: no words paired ({len(oo)} / {len(cs)})")
        continue
    pairs = [(o, c) for o, c in pairs if abs(c[0] - o[0]) < 40 and abs(c[2] - o[2]) < 60]
    if not pairs:
        continue
    offs = {}
    for o, c in pairs:
        offs.setdefault(o[4], []).append(c[2] - o[2])
    off = {h: statistics.median(v) for h, v in offs.items()}
    bad = []
    for o, c in pairs:
        dx, dy = c[0] - o[0], c[2] - o[2] - off[o[4]]
        m = max(abs(dx), abs(dy))
        worst = max(worst, m)
        if m > tol:
            bad.append((m, dx, dy, o[3]))
    total += len(pairs)
    bad_total += len(bad)
    unpaired = len(oo) - len(pairs)
    if bad or unpaired > 2:
        ex = ", ".join(f"{t!r} dx {dx:+.1f} dy {dy:+.1f}" for _, dx, dy, t in bad[:4])
        print(f"page {i+1}: {len(pairs)} words, {len(bad)} off by >{tol}pt, {unpaired} not paired; {ex}")
print(f"summary: {len(pages)} pages, {total} words paired, {bad_total} off by >{tol}pt, worst {worst:.2f}pt")
