"""Writes the section SFZs that layer converted instruments by range (no new samples):
strings-section, strings-pizz, brass-section, woodwind-section.

usage: sections_sfz.py SFZDIR
"""
import os
import re
import sys

SECTIONS = {
    "strings-section": ("String section: violins, violas and cellos (VSCO 2 CE ensembles) with the double bass, layered by range. CC0 1.0.",
                        [("violins", 55, 108, -2.0), ("violas", 48, 84, -3.0), ("cellos", 36, 76, -2.0), ("basses", 21, 55, -1.0)]),
    "strings-pizz": ("Pizzicato strings: violin, cello and bass sections (VSCO 2 CE), layered by range. CC0 1.0.",
                     [("violins-pizz", 55, 108, -2.0), ("cellos-pizz", 36, 72, -2.0), ("basses-pizz", 21, 50, -1.0)]),
    "brass-section": ("Brass section: trumpet, French horn, trombone and tuba (VSCO 2 CE), layered by range. CC0 1.0.",
                      [("trumpet", 58, 96, -3.0), ("french-horn", 41, 77, -3.0), ("trombone", 34, 72, -3.0), ("tuba", 21, 50, -2.0)]),
    "woodwind-section": ("Woodwind section: flute, oboe, clarinet and bassoon (VSCO 2 CE), layered by range. CC0 1.0.",
                         [("flute", 65, 108, -3.0), ("oboe", 58, 93, -4.0), ("clarinet", 50, 89, -3.0), ("bassoon", 21, 65, -3.0)]),
}

# overall level of each section (measured with lg-synth's sampled_demo example)
TRIM = {"strings-section": 1.5, "strings-pizz": 3.3, "brass-section": -3.8, "woodwind-section": -1.2}


def main():
    sfzdir = sys.argv[1]
    for name, (title, parts) in SECTIONS.items():
        out = [f"// {title}", "// Layers regions of the per-instrument SFZs; samples are shared with them.", ""]
        for part, lo, hi, vol in parts:
            vol += TRIM.get(name, 0.0)
            text = open(os.path.join(sfzdir, f"{part}.sfz")).read()
            out.append(f"// ---- {part} ({lo}-{hi}, {vol:+.1f} dB)")
            for block in text.split("<region>")[1:]:
                ops = re.findall(r"^(\w+)=(.*)$", block, re.M)
                d = dict(ops)
                lk, hk = int(d.get("lokey", 0)), int(d.get("hikey", 127))
                if hk < lo or lk > hi:
                    continue
                out.append("<region>")
                for k, v in ops:
                    if k == "lokey":
                        v = str(max(lk, lo))
                    elif k == "hikey":
                        v = str(min(hk, hi))
                    elif k == "volume":
                        v = f"{float(v) + vol:.2f}"
                    out.append(f"{k}={v}")
                if "volume" not in d:
                    out.append(f"volume={vol:.2f}")
        with open(os.path.join(sfzdir, f"{name}.sfz"), "w") as f:
            f.write("\n".join(out) + "\n")
        print(name)


if __name__ == "__main__":
    main()
