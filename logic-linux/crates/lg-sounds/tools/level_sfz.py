"""Adds a level offset (dB) to every region of SFZ files: level_sfz.py SFZDIR id=dB [id=dB...]
(used to even out the instruments' levels after measuring the demo renders)."""
import os
import re
import sys


def main():
    d = sys.argv[1]
    for arg in sys.argv[2:]:
        name, db = arg.split("=")
        db = float(db)
        p = os.path.join(d, f"{name}.sfz")
        text = open(p).read()
        head, *blocks = text.split("<region>")
        out = [head]
        for b in blocks:
            if re.search(r"^volume=", b, re.M):
                b = re.sub(r"^volume=(\S+)", lambda m: f"volume={float(m.group(1)) + db:.2f}", b, flags=re.M)
            else:
                b = b.rstrip("\n") + f"\nvolume={db:.2f}\n"
            out.append(b)
        open(p, "w").write("<region>".join(out))
        print(name, f"{db:+.1f} dB")


if __name__ == "__main__":
    main()
