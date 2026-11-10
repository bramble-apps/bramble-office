"""Salamander Grand Piano V3 (Alexander Holm, CC-BY 3.0) → piano-grand.

Source: the official 44.1 kHz/16-bit archive on archive.org, stream-extracted (only the velocity
layers we keep plus the release noises):
  curl -sL https://archive.org/download/SalamanderGrandPianoV3/SalamanderGrandPianoV3_44.1khz16bit.tar.bz2 \
    | tar -xjf - --wildcards '*v2.wav' '*v5.wav' '*v8.wav' '*v11.wav' '*v14.wav' '*v16.wav' '*rel*.wav' '*.sfz' '*README*'
Six of the sixteen velocity layers are kept; each takes over the range of the layers it replaces.

usage: build_piano.py SRCDIR OUTDIR SFZDIR
"""
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import convert_sfz as C  # noqa: E402

KEEP = [2, 5, 8, 11, 14, 16]

HEADER = """// Salamander Grand Piano V3 by Alexander Holm (Yamaha C5), CC-BY 3.0.
// https://archive.org/details/SalamanderGrandPianoV3
// Bramble: 6 of 16 velocity layers, tails trimmed, 16-bit FLAC."""


def main():
    src, out, sfzdir = sys.argv[1:4]
    text = open(os.path.join(src, "SalamanderGrandPianoV3.sfz"), encoding="latin-1").read()
    # widen kept layers over the dropped ones: layer k starts where the previous kept one ended
    lo_of = {}
    for m in re.finditer(r"v(\d+)\.wav lokey=(\d+) hikey=\d+ lovel=(\d+)", text):
        lo_of[(int(m.group(2)), int(m.group(1)))] = int(m.group(3))

    def fix(m):
        v = int(m.group(2))
        if v not in KEEP:
            return ""
        key = int(m.group(3))
        prev = KEEP[KEEP.index(v) - 1] if v != KEEP[0] else 0
        lo = lo_of.get((key, prev + 1), 1) if prev else 1
        line = m.group(0)
        return re.sub(r"lovel=\d+", f"lovel={lo}", line)

    text = re.sub(r"<region> sample=44\.1khz16bit\\(\S+?)v(\d+)\.wav lokey=(\d+)[^\n]*", fix, text)
    # drop the string-resonance release layers (not downloaded)
    text = re.sub(r"<region> sample=44\.1khz16bit\\harm[^\n]*", "", text)

    def local(rel):
        return os.path.join(src, rel)

    n, ns = C.convert(
        "piano-grand", text, local, lambda name: "", out, os.path.join(sfzdir, "piano-grand.sfz"),
        header=HEADER, release=True, tail_db=-62, max_s=14.0, fade_s=0.5,
        region_filter=lambda r: os.path.exists(local(r["_src"]) if "_src" in r else local(r["sample"].replace("\\", "/"))))
    print("piano-grand", n, "regions", ns, "samples")


if __name__ == "__main__":
    main()
