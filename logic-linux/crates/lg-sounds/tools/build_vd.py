"""Virtuosity Drums 0.925 (Versilian Studios + Karoryfer, CC0) → the "vintage" (jazz) stick kit,
plus the hand percussion every sampled kit shares.

Each hit is mixed to stereo from the overhead pair (stereo), the room pair (stereo, low) and, for
kick and snare, the close mic. Percussion: overhead + close mic. Members are fetched from the
official release zip with HTTP range requests.

usage: build_vd.py DLDIR OUTDIR METADIR [piece...]
"""
import json
import os
import re
import shutil
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np  # noqa: E402
import rzip  # noqa: E402
import sndlib as S  # noqa: E402

URL = "https://github.com/sfzinstruments/virtuosity_drums/releases/download/v0.925/Virtuosity_Drums_v0.925.zip"

# piece → (oh sample regex, max samples, close mic, close pan, target peak, tail s)
KIT = {
    "kick": (r"Samples/oh/kick/oh_kick_snon_vl\d+", 16, "kickmic", 0.0, 0.55, 1.5),
    "snare": (r"Samples/oh/snare/oh_snare_center_vl\d+", 18, "snaremic", 0.0, 0.5, 1.6),
    "rimshot": (r"Samples/oh/snare/oh_snare_rimshot_vl\d+", 10, "snaremic", 0.0, 0.55, 1.6),
    "sidestick": (r"Samples/oh/snare/oh_snare_crossstick_vl\d+", 10, "snaremic", 0.0, 0.3, 0.8),
    "tom_hi": (r"Samples/oh/htom/oh_htom_center_vl\d+", 12, None, 0.0, 0.45, 2.5),
    "tom_lo": (r"Samples/oh/ltom/oh_ltom_center_vl\d+", 12, None, 0.0, 0.45, 3.0),
    "hh_closed": (r"Samples/oh/hh/oh_hh_closed_vl\d+_rr\d+", 16, None, 0.0, 0.22, 0.8),
    "hh_pedal": (r"Samples/oh/hh/oh_hh_pedal_vl\d+_rr\d+", 8, None, 0.0, 0.16, 0.8),
    "hh_half": (r"Samples/oh/hh/oh_hh_half_vl\d+_rr\d+", 12, None, 0.0, 0.26, 2.0),
    "hh_34": (r"Samples/oh/hh/oh_hh_34_vl\d+_rr\d+", 8, None, 0.0, 0.28, 3.0),
    "hh_open": (r"Samples/oh/hh/oh_hh_open_vl\d+_rr\d+", 12, None, 0.0, 0.3, 3.5),
    "crash1": (r"Samples/oh/crash/oh_crash_crash_vl\d+(_rr\d+)?", 8, None, 0.0, 0.4, 5.5),
    "crash2": (r"Samples/oh/crash/oh_crash_sizzle_vl\d+(_rr\d+)?", 6, None, 0.0, 0.38, 5.5),
    "ride": (r"Samples/oh/ride/oh_ride_ride_vl\d+(_rr\d+)?", 12, None, 0.0, 0.3, 5.0),
    "ride_bell": (r"Samples/oh/ride/oh_ride_bell_vl\d+(_rr\d+)?", 8, None, 0.0, 0.32, 5.0),
    "ride2": (r"Samples/oh/flatride/oh_flatride_ride_vl\d+(_rr\d+)?", 8, None, 0.0, 0.28, 5.0),
    "splash": (r"Samples/oh/flatride/oh_flatride_crash_vl\d+(_rr\d+)?", 4, None, 0.0, 0.32, 4.0),
}

# shared hand percussion: piece → (oh sample regex, max samples, close pan, target peak, tail s)
PERC = {
    "tambourine": (r"Samples/perc/oh/tambourine/Tamb1_Shake", 8, -0.3, 0.3, 1.5),
    "cowbell": (r"Samples/perc/oh/cowbell/Cowbell1_Normal", 10, 0.2, 0.3, 1.5),
    "vibraslap": (r"Samples/perc/oh/vibraslap/Vibraslap1", 6, 0.3, 0.3, 2.5),
    "bongo_hi": (r"Samples/perc/oh/bongoh/BongoH_Hit1", 10, -0.2, 0.35, 1.0),
    "bongo_lo": (r"Samples/perc/oh/bongol/BongoL_Hit1", 10, -0.1, 0.35, 1.0),
    "conga_mute": (r"Samples/perc/oh/conga/Conga_17_HitHM1", 8, 0.1, 0.35, 1.0),
    "conga_open": (r"Samples/perc/oh/conga/Conga_22_HitN", 12, 0.15, 0.38, 1.5),
    "conga_low": (r"Samples/perc/oh/tumba/Tumba_24_HitN", 12, 0.25, 0.38, 1.5),
    "timbale_hi": (r"Samples/perc/oh/timbales/timb13", 8, -0.25, 0.35, 1.5),
    "agogo_hi": (r"Samples/perc/oh/agogo/Agogo_High", 8, 0.3, 0.28, 1.5),
    "agogo_lo": (r"Samples/perc/oh/agogo/Agogo_Low", 8, 0.3, 0.28, 1.5),
    "cabasa": (r"Samples/perc/oh/cabasa/Cabasa1_Rub", 8, -0.3, 0.25, 1.0),
    "shaker_d": (r"Samples/perc/oh/shaker/LShaker_Shake1D", 7, -0.3, 0.25, 1.0),
    "shaker_u": (r"Samples/perc/oh/shaker/LShaker_Shake1U", 7, -0.3, 0.25, 1.0),
    "guiro_short": (r"Samples/perc/oh/guiro/Guiro_Fast", 6, 0.2, 0.25, 1.5),
    "guiro_long": (r"Samples/perc/oh/guiro/Guiro_Med", 6, 0.2, 0.25, 2.0),
    "claves": (r"Samples/perc/oh/claves/Claves1_Hit", 10, 0.2, 0.3, 0.8),
    "woodblock_hi": (r"Samples/perc/oh/woodblock/woodblock_hi", 10, 0.2, 0.3, 0.8),
    "woodblock_lo": (r"Samples/perc/oh/woodblock/woodblock_lo", 10, 0.2, 0.3, 0.8),
    "triangle_mute": (r"Samples/perc/oh/triangle/Triangle1_HitM", 6, 0.35, 0.22, 1.0),
    "triangle_open": (r"Samples/perc/oh/triangle/Triangle1_Hit_", 8, 0.35, 0.22, 4.0),
    "sleighbells": (r"Samples/perc/oh/sleighbells/Sleighbells_Hit", 6, -0.3, 0.25, 2.0),
    "belltree": (r"Samples/perc/oh/belltree/BellTree_Stroke", 4, 0.3, 0.25, 4.0),
}

ROOM = 0.3
CLOSE = 0.8


def natkey(s):
    return [int(t) if t.isdigit() else t for t in re.split(r"(\d+)", s)]


def attack_rms(y, rate):
    m = np.max(np.abs(y), axis=0)
    on = int(np.argmax(m > 0.2 * m.max()))
    seg = y[:, on:on + int(0.05 * rate)]
    return float(np.sqrt(np.mean(seg ** 2)))


def stereo(x):
    return np.vstack([x, x]) if x.shape[0] == 1 else x[:2]


def build(z, names, work, out, kit, piece, pat, k, partners, target, tail):
    cand = sorted((n for n in names if re.match(pat, n) and re.search(r"\.(flac|wav)$", n)), key=natkey)
    if not cand:
        raise SystemExit(f"no samples for {piece}: {pat}")
    pick = [cand[i] for i in S.spread(len(cand), k)]
    want = []
    for p in pick:
        want.append(p)
        for (other, _g, _pan) in partners(p):
            if other in names:
                want.append(other)
    shutil.rmtree(work, ignore_errors=True)
    rzip.extract_parallel(z, work, want, workers=8)
    hits = []
    rate = None
    for p in pick:
        rate, x = S.load(os.path.join(work, p))
        y = stereo(x).astype(np.float64)
        for (other, g, pan) in partners(p):
            if other not in names:
                continue
            r2, x2 = S.load(os.path.join(work, other))
            assert r2 == rate
            n = min(y.shape[1], x2.shape[1])
            if x2.shape[0] == 1:
                add = S.pan(x2[0, :n].astype(np.float64), pan) * g
            else:
                add = x2[:2, :n].astype(np.float64) * g
            y = y[:, :n] + add
        y = S.trim(y, rate, tail_db=-60, max_s=tail, fade_s=min(0.3, tail / 4), head_db=-50, preroll_s=0.003)
        hits.append(y)
    g = target / max(S.peak(h) for h in hits)
    entries = []
    for n, h in enumerate(hits):
        h = h * g
        rel = f"samples/{kit}/{piece}_{n + 1:02d}.flac"
        S.save_flac(os.path.join(out, rel), h, rate)
        entries.append({"file": rel, "peak": S.peak(h), "rms": attack_rms(h, rate), "len": h.shape[1] / rate})
    shutil.rmtree(work, ignore_errors=True)
    return entries


def main():
    dl, out, meta_dir = sys.argv[1:4]
    only = set(sys.argv[4:])
    z = rzip.open_remote(URL)
    names = {i.filename for i in z.infolist()}
    work = os.path.join(dl, "vd-work")
    os.makedirs(meta_dir, exist_ok=True)
    meta_path = os.path.join(meta_dir, "vd.json")
    meta = json.load(open(meta_path)) if os.path.exists(meta_path) else {}

    def todo(p):
        return (p in only) if only else p not in meta

    for piece, (pat, k, close, cpan, target, tail) in KIT.items():
        if not todo(piece):
            continue

        def partners(p, close=close, cpan=cpan):
            r = [(p.replace("/oh/", "/room/").replace("/oh_", "/room_"), ROOM, 0.0)]
            if close:
                r.append((p.replace("/oh/", f"/{close}/").replace("/oh_", f"/{close}_"), CLOSE, cpan))
            return r

        meta[piece] = build(z, names, work, out, "drums-vintage", piece, pat, k, partners, target, tail)
        json.dump(meta, open(meta_path, "w"), indent=1)
        print(piece, len(meta[piece]), flush=True)
    for piece, (pat, k, cpan, target, tail) in PERC.items():
        if not todo(piece):
            continue

        def partners(p, cpan=cpan):
            return [(p.replace("/perc/oh/", "/perc/close/").replace("_Overhead", "_Close"), CLOSE, cpan)]

        meta[piece] = build(z, names, work, out, "perc", piece, pat, k, partners, target, tail)
        json.dump(meta, open(meta_path, "w"), indent=1)
        print(piece, len(meta[piece]), flush=True)


if __name__ == "__main__":
    main()
