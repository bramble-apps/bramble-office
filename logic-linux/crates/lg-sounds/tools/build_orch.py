"""VSCO 2 CE and VCSL (Versilian Studios, CC0) → strings, brass, woodwinds, harp, timpani,
mallets, saxophone, claps and cajon. Their own SFZ mappings (SFZ / sfz branches) are converted;
samples come from the official GitHub repositories (master branch).

usage: build_orch.py DLDIR OUTDIR SFZDIR [id...]
"""
import os
import shutil
import sys
import urllib.parse
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import convert_sfz as C  # noqa: E402

LIBS = {
    "vsco": ("https://raw.githubusercontent.com/sgossner/VSCO-2-CE/SFZ/",
             "https://raw.githubusercontent.com/sgossner/VSCO-2-CE/master/"),
    "vcsl": ("https://raw.githubusercontent.com/sgossner/VCSL/sfz/",
             "https://raw.githubusercontent.com/sgossner/VCSL/master/"),
}

HEAD = {
    "vsco": "// From VS Chamber Orchestra 2: Community Edition (Versilian Studios), CC0 1.0.\n// https://github.com/sgossner/VSCO-2-CE",
    "vcsl": "// From the Versilian Community Sample Library (Versilian Studios), CC0 1.0.\n// https://github.com/sgossner/VCSL",
}

# id → (library, sfz path in its SFZ branch, sample prefix dir (relative to repo root), options)
INSTRUMENTS = {
    "violins": ("vsco", "ViolinEnsSusVib.sfz", "", dict(max_s=8.0, loop=True)),
    "violas": ("vsco", "ViolaEnsSusVib.sfz", "", dict(max_s=8.0, loop=True)),
    "cellos": ("vsco", "CelloEnsSusVib.sfz", "", dict(max_s=8.0, loop=True)),
    "basses": ("vsco", "ContrabassSusVB.sfz", "", dict(max_s=8.0, loop=True)),
    "violin-solo": ("vsco", "SViolinVib.sfz", "", dict(max_s=8.0, loop=True)),
    "violins-pizz": ("vsco", "ViolinEnsPizz.sfz", "", dict(max_s=2.0)),
    "cellos-pizz": ("vsco", "CelloEnsPizz.sfz", "", dict(max_s=2.5)),
    "basses-pizz": ("vsco", "ContrabassPizz.sfz", "", dict(max_s=3.0)),
    "harp": ("vsco", "Harp.sfz", "", dict(max_s=6.0)),
    "trumpet": ("vsco", "TrumpetSus.sfz", "", dict(max_s=8.0, loop=True)),
    "french-horn": ("vsco", "FHornSus.sfz", "", dict(max_s=8.0, loop=True)),
    "trombone": ("vsco", "TromboneSus.sfz", "", dict(max_s=8.0, loop=True)),
    "tuba": ("vsco", "TubaSus.sfz", "", dict(max_s=8.0, loop=True)),
    "flute": ("vsco", "FluteSusVib.sfz", "", dict(max_s=8.0, loop=True)),
    "oboe": ("vsco", "OboeSusVib.sfz", "", dict(max_s=8.0, loop=True)),
    "clarinet": ("vsco", "ClarinetSus.sfz", "", dict(max_s=8.0, loop=True)),
    "bassoon": ("vsco", "BassoonSus.sfz", "", dict(max_s=8.0, loop=True)),
    "timpani": ("vsco", "Timpani.sfz", "", dict(max_s=5.0)),
    "vibraphone": ("vcsl", "Idiophones/Struck Idiophones/Vibraphone - Soft Mallets.sfz", "Idiophones/Struck Idiophones/", dict(max_s=7.0)),
    "marimba": ("vcsl", "Idiophones/Struck Idiophones/Marimba.sfz", "Idiophones/Struck Idiophones/", dict(max_s=4.0)),
    "xylophone": ("vcsl", "Idiophones/Struck Idiophones/Xylophone - Medium Mallets.sfz", "Idiophones/Struck Idiophones/", dict(max_s=2.5)),
    "glockenspiel": ("vcsl", "Idiophones/Struck Idiophones/Glockenspiel.sfz", "Idiophones/Struck Idiophones/", dict(max_s=4.0)),
    "tubular-bells": ("vcsl", "Idiophones/Struck Idiophones/Tubular Bells 1.sfz", "Idiophones/Struck Idiophones/", dict(max_s=7.0)),
    "kalimba": ("vcsl", "Idiophones/Plucked Idiophones/Kalimba, Kenya.sfz", "Idiophones/Plucked Idiophones/", dict(max_s=4.0)),
    "tenor-sax": ("vcsl", "Aerophones/Reed Aerophones/Tenor Saxophone - Vibrato.sfz", "Aerophones/Reed Aerophones/", dict(max_s=8.0, loop=True)),
    "claps": ("vcsl", "Idiophones/Struck Idiophones/Claps.sfz", "Idiophones/Struck Idiophones/", dict(max_s=1.0)),
    "cajon": ("vcsl", "Idiophones/Struck Idiophones/Cajon.sfz", "Idiophones/Struck Idiophones/", dict(max_s=1.0)),
}


def get(url):
    for _ in range(5):
        try:
            with urllib.request.urlopen(url, timeout=120) as r:
                return r.read()
        except Exception as e:  # retry
            err = e
    raise err


def q(path):
    return urllib.parse.quote(path)


def main():
    dl, out, sfzdir = sys.argv[1:4]
    only = sys.argv[4:]
    for inst, (lib, sfz, prefix, opts) in INSTRUMENTS.items():
        if only and inst not in only:
            continue
        sfz_out = os.path.join(sfzdir, f"{inst}.sfz")
        if not only and os.path.exists(sfz_out):
            continue
        sfz_base, smp_base = LIBS[lib]
        sfz_dir = os.path.dirname(sfz)
        text = get(sfz_base + q(sfz)).decode("utf-8", "replace")

        def read_include(name, sfz_dir=sfz_dir, sfz_base=sfz_base):
            return get(sfz_base + q(os.path.normpath(os.path.join(sfz_dir, name)))).decode("utf-8", "replace")

        cache = os.path.join(dl, "orch-cache", inst)
        shutil.rmtree(cache, ignore_errors=True)
        control, regions = C.flatten(text, read_include)
        dp = control.get("default_path", "").replace("\\", "/")
        rels = sorted({(dp + r["sample"]).replace("\\", "/") for r in regions
                       if "sample" in r and C.plays_by_default(r, control)
                       and r.get("trigger", "attack") == "attack"})

        def src_path(rel, prefix=prefix):
            return os.path.normpath(prefix + rel)

        def local(rel, cache=cache):
            return os.path.join(cache, src_path(rel))

        def fetch(rel, cache=cache, smp_base=smp_base):
            p = local(rel)
            os.makedirs(os.path.dirname(p), exist_ok=True)
            with open(p, "wb") as f:
                f.write(get(smp_base + q(src_path(rel))))

        C.prefetch(rels, fetch)
        n, ns = C.convert(inst, text, local, read_include, out, sfz_out, header=HEAD[lib], **opts)
        print(inst, n, "regions", ns, "samples", flush=True)
        shutil.rmtree(cache, ignore_errors=True)


if __name__ == "__main__":
    main()
