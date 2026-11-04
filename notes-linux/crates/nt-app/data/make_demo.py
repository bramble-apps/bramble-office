#!/usr/bin/env python3
"""Writes demo.txt, the app's built-in demo library, from the NO1 research's decoded notes
(Notes-Research/format-analysis/decoded/*.json): the Bramble Research folder as it was on the
Mac, so the shell's snapshots can be compared with Apple's screenshots.
usage: make_demo.py <decoded dir> > demo.txt"""
import glob, json, os, sys

EPOCH = 978307200  # Core Data dates count from 2001
STYLE = {0: "Title", 1: "Heading", 2: "Subheading", 4: "Monostyled", 100: "Bulleted", 101: "Dashed", 102: "Numbered", 103: "Checklist"}

def esc(s):
    return s.replace("\\", "\\\\").replace("\t", "\\t").replace("\n", "\\n")

out = []
for f in sorted(glob.glob(os.path.join(sys.argv[1], "*.json"))):
    d = json.load(open(f))
    row = d["note_row"]
    thumb = ""
    for a in d.get("attachments", []):
        uti = a.get("ZTYPEUTI", "")
        if "png" in uti or "jpeg" in uti:
            thumb = thumb or "image"
        elif "pdf" in uti:
            thumb = thumb or "pdf"
        elif "drawing" in uti or "paper" in uti:
            thumb = thumb or "drawing"
    out.append("\t".join(["N", row["ZIDENTIFIER"], esc(d["title"]), esc(d["folder"]), str(row.get("ZCREATIONDATE3", 0) + EPOCH), str(row.get("ZMODIFICATIONDATE1", 0) + EPOCH), str(row.get("ZISPINNED", 0) or 0), esc(row.get("ZSNIPPET") or ""), thumb]))
    para = None
    for r in d["runs"]:
        ps = r["attrs"].get("paragraph_style", {})
        a = r["attrs"]
        for k, piece in enumerate(r["text"].split("\n")):
            if k > 0:
                para = None
            if para is None and (piece or k < len(r["text"].split("\n")) - 1):
                st = STYLE.get(ps.get("style_type", 3), "Body")
                ck = ps.get("checklist", {}).get("done", 0)
                out.append("\t".join(["P", st, str(ps.get("indent_amount", 0)), str(ck), str(ps.get("block_quote", 0)), str(ps.get("alignment", 0))]))
                para = True
            if not piece:
                continue
            w = a.get("font_weight", 0)
            flags = ("b" if w in (1, 3) else "") + ("i" if w in (2, 3) else "") + ("u" if a.get("underlined") else "") + ("s" if a.get("strikethrough") else "")
            sup = a.get("superscript", 0)
            flags += "^" if sup == 1 else "_" if sup == -1 else ""
            out.append("\t".join(["S", flags, str(a.get("emphasis_style", "")), esc(a.get("link", "")), esc(piece)]))
print("\n".join(out))
