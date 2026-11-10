"""Karoryfer Samples libraries (CC0) → guitar-electric (Emilyguitar), bass-electric
(Fashionbass), bass-upright (Meatbass pizzicato). The official release zips on GitHub are read
with HTTP range requests; only the SFZ and the samples its default state plays are fetched.

usage: build_karo.py DLDIR OUTDIR SFZDIR [id...]
"""
import os
import posixpath
import shutil
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import convert_sfz as C  # noqa: E402
import rzip  # noqa: E402

GH = "https://github.com/sfzinstruments/"
INSTRUMENTS = {
    "guitar-electric": (GH + "karoryfer.emilyguitar/releases/download/v1.001/Karoryfer.Emilyguitar.v1.001.zip",
                        "Emilyguitar/emily_clean.sfz",
                        "// Emilyguitar by Karoryfer Samples (D. Smolken), CC0 1.0. Epiphone guitar, direct.\n// https://github.com/sfzinstruments/karoryfer.emilyguitar",
                        dict(max_s=6.0, mono=True)),
    "bass-electric": (GH + "karoryfer.fashionbass/releases/download/v1.001/Karoryfer.Fashionbass.v1.001.zip",
                      "Fashionbass/fashionbass_clean.sfz",
                      "// Fashionbass by Karoryfer Samples (D. Smolken), CC0 1.0. Bass guitar, neck pickup, direct.\n// https://github.com/sfzinstruments/karoryfer.fashionbass",
                      dict(max_s=5.0, mono=True)),
    "bass-upright": (GH + "karoryfer.meatbass/releases/download/v1.001/Karoryfer.Meatbass.v1.001.zip",
                     "Meatbass/Programs/04_pizz.sfz",
                     "// Meatbass by Karoryfer Samples (D. Smolken, recorded by L. Zamenhof), CC0 1.0. 1958 double bass, pizzicato.\n// https://github.com/sfzinstruments/karoryfer.meatbass",
                     dict(max_s=4.0)),
}


def main():
    dl, out, sfzdir = sys.argv[1:4]
    only = sys.argv[4:]
    for inst, (url, sfz, header, opts) in INSTRUMENTS.items():
        if only and inst not in only:
            continue
        z = rzip.open_remote(url)
        names = {i.filename for i in z.infolist()}
        sfz_dir = posixpath.dirname(sfz)

        def read(member):
            return z.read(member).decode("utf-8", "replace")

        def read_include(name, sfz_dir=sfz_dir):
            return read(posixpath.normpath(posixpath.join(sfz_dir, name)))

        text = read(sfz)
        control, regions = C.flatten(text, read_include)
        dp = control.get("default_path", "").replace("\\", "/")
        rels = sorted({(dp + r["sample"]).replace("\\", "/") for r in regions
                       if "sample" in r and C.plays_by_default(r, control)
                       and r.get("trigger", "attack") == "attack"})
        members = [posixpath.normpath(posixpath.join(sfz_dir, r)) for r in rels]
        missing = [m for m in members if m not in names]
        if missing:
            print("missing", len(missing), missing[:3])
        cache = os.path.join(dl, "karo-cache", inst)
        shutil.rmtree(cache, ignore_errors=True)
        rzip.extract_parallel(z, cache, [m for m in members if m in names], workers=8)

        def local(rel, cache=cache, sfz_dir=sfz_dir):
            return os.path.join(cache, posixpath.normpath(posixpath.join(sfz_dir, rel)))

        flt = lambda r, cache=cache: os.path.exists(local((dp + r["sample"]).replace("\\", "/")))  # noqa: E731
        n, ns = C.convert(inst, text, local, read_include, out, os.path.join(sfzdir, f"{inst}.sfz"),
                          header=header, region_filter=flt, **opts)
        print(inst, n, "regions", ns, "samples", flush=True)
        shutil.rmtree(cache, ignore_errors=True)


if __name__ == "__main__":
    main()
