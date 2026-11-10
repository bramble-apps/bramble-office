"""Writes the drum-kit SFZs (Drum Kit Designer note map) from the hit metadata that build_drs.py,
build_vd.py and build_orch.py (claps, cajon) produced.

Each kit piece's hits are sorted by attack loudness and split into velocity bands; the hits in a
band become random round robins, level-matched halfway to the band's mean.

usage: drum_sfz.py METADIR SFZDIR
"""
import json
import math
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sndlib as S  # noqa: E402

RANGE_DB = 36.0  # velocity 1..127 spans this much attack level


def bands(hits, max_bands, lo=1, hi=127):
    """→ [(lovel, hivel, [(hit, volume_db)])]"""
    hs = sorted(hits, key=lambda h: h["rms"])
    top = hs[-1]["rms"]
    L = [S.db(h["rms"] / top) for h in hs]
    n = len(hs)
    b = min(max_bands, n)
    groups = []
    for k in range(b):
        a, e = round(k * n / b), round((k + 1) * n / b)
        groups.append(list(range(a, e)))
    centers = []
    for g in groups:
        m = sum(L[i] for i in g) / len(g)
        centers.append(min(hi, max(lo, lo + (hi - lo) * (1 + m / RANGE_DB))))
    out = []
    prev = lo - 1
    for k, g in enumerate(groups):
        hv = hi if k == len(groups) - 1 else int(round((centers[k] + centers[k + 1]) / 2))
        hv = max(hv, prev + 1)
        if k < len(groups) - 1:
            hv = min(hv, hi - (len(groups) - 1 - k))
        mean = sum(L[i] for i in g) / len(g)
        out.append((prev + 1, hv, [(hs[i], 0.5 * (mean - L[i])) for i in g]))
        prev = hv
    return out


def regions(piece_hits, keys, *, max_bands=8, lo=1, hi=127, tune=0, vol=0.0, veltrack=25,
            group=None, off_by=None, center=None):
    lines = []
    for lv, hv, rr in bands(piece_hits, max_bands, lo, hi):
        n = len(rr)
        for i, (h, v) in enumerate(rr):
            for key in keys:
                lines.append("<region>")
                lines.append(f"sample={h['file']}")
                pc = (center if center is not None else key) - tune
                lines.append(f"lokey={key} hikey={key} pitch_keycenter={pc}")
                lines.append(f"lovel={lv} hivel={hv}")
                if n > 1:
                    lines.append(f"lorand={i / n:.4f} hirand={(i + 1) / n:.4f}")
                lines.append(f"volume={vol + v:.2f} amp_veltrack={veltrack}")
                if group is not None:
                    lines.append(f"group={group}")
                if off_by is not None:
                    lines.append(f"off_by={off_by}")
    return lines


HAT_CLOSED, HAT_OPEN = 1, 2  # a closed/pedal hat (group 1) chokes the open ones (off_by 1)

PERC_MAP = [
    (54, "tambourine"), (56, "cowbell"), (58, "vibraslap"), (60, "bongo_hi"), (61, "bongo_lo"),
    (62, "conga_mute"), (63, "conga_open"), (64, "conga_low"), (65, "timbale_hi"),
    (67, "agogo_hi"), (68, "agogo_lo"), (69, "cabasa"), (70, "shaker_d"), (73, "guiro_short"),
    (74, "guiro_long"), (75, "claves"), (76, "woodblock_hi"), (77, "woodblock_lo"),
    (80, "triangle_mute"), (81, "triangle_open"), (82, "shaker_u"), (83, "sleighbells"),
    (84, "belltree"),
]


def vcsl_hits(sfz_path):
    """Claps / cajon converted by build_orch.py → hit list (all regions as one piece)."""
    text = open(sfz_path).read()
    hits = []
    for m in re.finditer(r"sample=(\S+)", text):
        hits.append({"file": m.group(1)})
    return hits


# overall kit levels (matched to the synthesized kits on a Drummer bounce)
KIT_TRIM = {"drums-modern": 4.0, "drums-vintage": 2.5, "drums-brush": 4.0}


def kit(name, title, source, P, hats_other, extra):
    trim = KIT_TRIM.get(name, 0.0)

    def t(kw):
        kw = dict(kw)
        kw["vol"] = kw.get("vol", 0.0) + trim
        return kw

    L = [f"// {title}", "// Drum Kit Designer note map (GM drums + hi-hat articulations below 35).",
         "// " + source, "", "<global>", "loop_mode=one_shot", "ampeg_release=0.05", ""]
    for (keys, piece, kw) in extra:
        L.append(f"// {piece} -> {', '.join(S.note_name(k) + f' ({k})' for k in keys)}")
        L += regions(P[piece], keys, **t(kw))
    # hi-hat articulations below 35
    for (keys, piece, kw) in hats_other:
        L.append(f"// {piece} -> {keys}")
        L += regions(P[piece], keys, **t(kw))
    # percussion
    for key, piece in PERC_MAP:
        if piece in P:
            L.append(f"// {piece} -> {key}")
            L += regions(P[piece], [key], max_bands=4, veltrack=40, vol=trim)
    return "\n".join(L) + "\n"


def main():
    meta, sfzdir = sys.argv[1:3]
    drs = json.load(open(os.path.join(meta, "drs.json")))
    vd = json.load(open(os.path.join(meta, "vd.json")))
    perc = {k: v for k, v in vd.items() if k in dict((p, 1) for _, p in PERC_MAP)}
    # claps / cajon: equal-loudness round robins (their levels are in the SFZ volume)
    claps = os.path.join(sfzdir, "claps.sfz")
    cajon = os.path.join(sfzdir, "cajon.sfz")
    shared = []
    if os.path.exists(claps):
        shared.append(("claps", claps, 39))
    if os.path.exists(cajon):
        shared.append(("cajon", cajon, 66))

    def shared_lines(trim):
        L = []
        for name, path, key in shared:
            text = open(path).read()
            L.append(f"// {name} -> {key} (VCSL, CC0)")
            for block in text.split("<region>")[1:]:
                ops = dict(re.findall(r"^(\w+)=(.*)$", block, re.M))
                L.append("<region>")
                L.append(f"sample={ops['sample']}")
                L.append(f"lokey={key} hikey={key} pitch_keycenter={key}")
                for k in ("lovel", "hivel", "seq_length", "seq_position", "volume"):
                    if k in ops:
                        if k == "volume":
                            L.append(f"volume={float(ops[k]) - 6 + trim:.2f} amp_veltrack=60")
                        else:
                            L.append(f"{k}={ops[k]}")
        return L

    hat = dict(group=HAT_CLOSED)
    opn = dict(group=HAT_OPEN, off_by=HAT_CLOSED)
    other_low = [k for k in range(21, 35) if k not in (26, 30, 33, 34)]

    # ---- modern (DRSKit)
    P = {**drs, **perc}
    extra = [
        ([35, 36], "kick", dict(max_bands=10)),
        ([37], "sidestick", dict(max_bands=5)),
        ([38], "snare", dict(max_bands=10)),
        ([40], "snare", dict(max_bands=10, vol=1.5)),
        ([41], "tom3", dict(max_bands=7)),
        ([43], "tom2", dict(max_bands=7)),
        ([45], "tom2", dict(max_bands=7, tune=2)),
        ([47], "tom1", dict(max_bands=7, tune=-2)),
        ([48], "tom1", dict(max_bands=7)),
        ([50], "tom1", dict(max_bands=7, tune=3)),
        ([42], "hh_closed", dict(max_bands=9, **hat)),
        ([44], "hh_pedal", dict(max_bands=3, **hat)),
        ([46], "hh_open", dict(max_bands=5, **opn)),
        ([49], "crash1", dict(max_bands=4)),
        ([57], "crash2", dict(max_bands=4)),
        ([52], "crash2", dict(max_bands=4, tune=-3)),
        ([55], "crash1", dict(max_bands=4, tune=5)),
        ([51, 59], "ride", dict(max_bands=6)),
        ([53], "ride_bell", dict(max_bands=4)),
    ]
    hats = [([33, 34], "hh_half", dict(max_bands=5, **opn)),
            ([26, 30], "hh_open", dict(max_bands=5, **opn)),
            (other_low, "hh_edge", dict(max_bands=5, **hat))]
    text = kit("drums-modern", "Modern Kit: DRSKit 2.1 by the DrumGizmo team and Jes Eiler (DRSDrums), CC-BY 4.0.",
               "https://drumgizmo.org/wiki/doku.php?id=kits:drskit (close mics, overheads and room mixed to stereo)",
               P, hats, extra)
    S.write(os.path.join(sfzdir, "drums-modern.sfz"), text + "\n".join(shared_lines(KIT_TRIM["drums-modern"])) + "\n")

    # ---- brush (DRSKit whiskers, kick/side stick/pedal hat/bell from the stick set)
    extra = [
        ([35, 36], "kick", dict(max_bands=10, vol=-4)),
        ([37], "sidestick", dict(max_bands=5, vol=-2)),
        ([38], "snare_sweep", dict(max_bands=2, lo=1, hi=44, veltrack=40)),
        ([38], "snare_brush", dict(max_bands=8, lo=45, hi=127)),
        ([40], "snare_brush", dict(max_bands=8, vol=1.0)),
        ([39], "snare_sweep", dict(max_bands=2)),
        ([41], "tom3_brush", dict(max_bands=5)),
        ([43], "tom2_brush", dict(max_bands=5)),
        ([45], "tom2_brush", dict(max_bands=5, tune=2)),
        ([47], "tom1_brush", dict(max_bands=5, tune=-2)),
        ([48], "tom1_brush", dict(max_bands=5)),
        ([50], "tom1_brush", dict(max_bands=5, tune=3)),
        ([42], "hh_closed_brush", dict(max_bands=6, **hat)),
        ([44], "hh_pedal", dict(max_bands=3, **hat)),
        ([46], "hh_open_brush", dict(max_bands=5, **opn)),
        ([49], "crash1_brush", dict(max_bands=3)),
        ([57], "crash2_brush", dict(max_bands=3)),
        ([52], "crash2_brush", dict(max_bands=3, tune=-3)),
        ([55], "crash1_brush", dict(max_bands=3, tune=5)),
        ([51, 59], "ride_brush", dict(max_bands=5)),
        ([53], "ride_bell", dict(max_bands=4, vol=-6)),
    ]
    hats = [([33, 34], "hh_open_brush", dict(max_bands=5, vol=-3, **opn)),
            ([26, 30], "hh_open_brush", dict(max_bands=5, **opn)),
            (other_low, "hh_closed_brush", dict(max_bands=5, **hat))]
    text = kit("drums-brush", "Brush Kit: DRSKit 2.1 played with brushes (whiskers), DrumGizmo team and Jes Eiler (DRSDrums), CC-BY 4.0.",
               "https://drumgizmo.org/wiki/doku.php?id=kits:drskit (soft snare notes play the brush sweep)",
               P, hats, extra)
    S.write(os.path.join(sfzdir, "drums-brush.sfz"), text)

    # ---- vintage (Virtuosity Drums)
    P = dict(vd)
    extra = [
        ([35, 36], "kick", dict(max_bands=10)),
        ([37], "sidestick", dict(max_bands=5)),
        ([38], "snare", dict(max_bands=10)),
        ([40], "rimshot", dict(max_bands=5)),
        ([41], "tom_lo", dict(max_bands=6, tune=-3)),
        ([43], "tom_lo", dict(max_bands=6)),
        ([45], "tom_hi", dict(max_bands=6, tune=-3)),
        ([47], "tom_hi", dict(max_bands=6, tune=-1)),
        ([48], "tom_hi", dict(max_bands=6)),
        ([50], "tom_hi", dict(max_bands=6, tune=2)),
        ([42], "hh_closed", dict(max_bands=6, **hat)),
        ([44], "hh_pedal", dict(max_bands=3, **hat)),
        ([46], "hh_open", dict(max_bands=4, **opn)),
        ([49], "crash1", dict(max_bands=4)),
        ([57], "crash2", dict(max_bands=3)),
        ([52], "crash2", dict(max_bands=3, tune=-2)),
        ([55], "splash", dict(max_bands=2)),
        ([51], "ride", dict(max_bands=5)),
        ([59], "ride2", dict(max_bands=4)),
        ([53], "ride_bell", dict(max_bands=4)),
    ]
    hats = [([33, 34], "hh_half", dict(max_bands=4, **opn)),
            ([26, 30], "hh_34", dict(max_bands=3, **opn)),
            (other_low, "hh_closed", dict(max_bands=5, **hat))]
    text = kit("drums-vintage", "Vintage Kit: Virtuosity Drums 0.925 by Versilian Studios and Karoryfer Samples, CC0 1.0.",
               "https://versilian-studios.com/virtuosity-drums/ (overheads, room and kick/snare close mics mixed to stereo)",
               P, hats, extra)
    S.write(os.path.join(sfzdir, "drums-vintage.sfz"), text + "\n".join(shared_lines(KIT_TRIM["drums-vintage"])) + "\n")
    print("kits written")


if __name__ == "__main__":
    main()
