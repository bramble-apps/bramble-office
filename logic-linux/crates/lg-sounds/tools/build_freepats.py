"""FreePats guitars → guitar-nylon (Spanish Classical Guitar, CC0) and guitar-steel
(FSS Steel-String Acoustic Guitar, GPL-3.0-or-later with the FreePats exception).

Sources (official FreePats pages):
  https://freepats.zenvoid.org/Guitar/SpanishClassicalGuitar/SpanishClassicalGuitar-SFZ+FLAC-20190618.7z
  https://freepats.zenvoid.org/Guitar/FSS-SteelStringGuitar/FSS-SteelStringGuitar-SFZ-20200521.tar.xz

usage: build_freepats.py SRCDIR OUTDIR SFZDIR   (SRCDIR holds the two unpacked folders)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import convert_sfz as C  # noqa: E402

ITEMS = {
    "guitar-nylon": ("SpanishClassicalGuitar-SFZ+FLAC-20190618", "SpanishClassicalGuitar-20190618.sfz",
                     "// Spanish Classical Guitar by roberto@zenvoid.org (FreePats), CC0 1.0.\n// https://freepats.zenvoid.org/Guitar/acoustic-guitar.html"),
    "guitar-steel": ("FSS-SteelStringGuitar-SFZ-20200521", "FSS-SteelStringGuitar-20200521.sfz",
                     "// FSS Steel-String Acoustic Guitar: samples (c) 2008 Gary Campion (FlameStudios), modified 2016-2020 by\n"
                     "// roberto@zenvoid.org for FreePats. GPL-3.0-or-later with the FreePats special exception (see LICENSES.md).\n"
                     "// https://freepats.zenvoid.org/Guitar/steel-acoustic-guitar.html"),
}


def main():
    src, out, sfzdir = sys.argv[1:4]
    for inst, (folder, sfz, header) in ITEMS.items():
        base = None
        for root, dirs, files in os.walk(src):
            if os.path.basename(root) == folder:
                base = root
        text = open(os.path.join(base, sfz), encoding="utf-8", errors="replace").read()
        n, ns = C.convert(inst, text, lambda rel, base=base: os.path.join(base, rel), lambda name: "",
                          out, os.path.join(sfzdir, f"{inst}.sfz"), header=header, max_s=6.0)
        print(inst, n, "regions", ns, "samples")


if __name__ == "__main__":
    main()
