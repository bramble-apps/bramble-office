"""DRSKit 2.1 (DrumGizmo, CC-BY-4.0) → the "modern" stick kit and the "brush" kit (whiskers).

Each DrumGizmo hit is one 13-channel WAV (every mic). We mix each hit to stereo: the piece's own
close mics (panned, polarity-checked), the overhead pair and the room pair. Only the members we
use are fetched from the official zip with HTTP range requests.

usage: build_drs.py DLDIR OUTDIR METADIR
"""
import json
import os
import re
import shutil
import sys
import xml.etree.ElementTree as ET

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np  # noqa: E402
import rzip  # noqa: E402
import sndlib as S  # noqa: E402

URL = "https://drumgizmo.org/kits/DRSKit/DRSKit2_1.zip"

# piece → (kit folder, source instrument, max samples, close mics with pan, tail max s)
PIECES = {
    "kick": ("drums-modern", "Kdrum_without_contact", 20, {"Kdrum_back": 0.0, "Kdrum_front": 0.0}, 1.5),
    "snare": ("drums-modern", "Snare", 24, {"Snare_top": -0.05, "Snare_bottom": -0.05}, 1.6),
    "sidestick": ("drums-modern", "Snare_rim", 10, {"Snare_top": -0.05}, 0.8),
    "tom1": ("drums-modern", "Tom1", 14, {"Tom1": -0.15}, 2.5),
    "tom2": ("drums-modern", "Tom2", 14, {"Tom2": 0.25}, 2.8),
    "tom3": ("drums-modern", "Tom3", 14, {"Tom3": 0.45}, 3.0),
    "hh_closed": ("drums-modern", "Hihat_closed", 18, {"Hihat": -0.45}, 0.8),
    "hh_edge": ("drums-modern", "Hihat_closed_shank", 10, {"Hihat": -0.45}, 0.8),
    "hh_pedal": ("drums-modern", "Hihat_foot", 6, {"Hihat": -0.45}, 0.8),
    "hh_half": ("drums-modern", "Hihat_semi_open", 10, {"Hihat": -0.45}, 2.0),
    "hh_open": ("drums-modern", "Hihat_open", 10, {"Hihat": -0.45}, 3.5),
    "crash1": ("drums-modern", "Crash_left_tip", 8, {}, 5.5),
    "crash2": ("drums-modern", "Crash_right_tip", 8, {}, 5.5),
    "ride": ("drums-modern", "Ride_tip", 12, {"Ride": 0.45}, 5.0),
    "ride_bell": ("drums-modern", "Ride_shank_bell", 8, {"Ride": 0.45}, 5.0),
    # brushes (DrumGizmo calls them whiskers)
    "snare_brush": ("drums-brush", "Snare_whisker", 14, {"Snare_top": -0.05, "Snare_bottom": -0.05}, 1.5),
    "snare_sweep": ("drums-brush", "Snare_circle_whisker", 4, {"Snare_top": -0.05, "Snare_bottom": -0.05}, 3.0),
    "tom1_brush": ("drums-brush", "Tom1_whisker", 8, {"Tom1": -0.15}, 2.5),
    "tom2_brush": ("drums-brush", "Tom2_whisker", 8, {"Tom2": 0.25}, 2.8),
    "tom3_brush": ("drums-brush", "Tom3_whisker", 8, {"Tom3": 0.45}, 3.0),
    "hh_closed_brush": ("drums-brush", "Hihat_closed_whisker", 10, {"Hihat": -0.45}, 0.8),
    "hh_open_brush": ("drums-brush", "Hihat_open_whisker", 8, {"Hihat": -0.45}, 3.5),
    "crash1_brush": ("drums-brush", "Crash_left_whisker", 6, {}, 5.0),
    "crash2_brush": ("drums-brush", "Crash_right_whisker", 6, {}, 5.0),
    "ride_brush": ("drums-brush", "Ride_whisker", 8, {"Ride": 0.45}, 5.0),
}

# peak level each piece's loudest hit is normalised to (the kit's internal mix)
TARGET = {
    "kick": 0.55, "snare": 0.55, "sidestick": 0.3, "tom1": 0.5, "tom2": 0.5, "tom3": 0.5,
    "hh_closed": 0.25, "hh_edge": 0.25, "hh_pedal": 0.18, "hh_half": 0.28, "hh_open": 0.3,
    "crash1": 0.4, "crash2": 0.4, "ride": 0.3, "ride_bell": 0.32,
    "snare_brush": 0.4, "snare_sweep": 0.25, "tom1_brush": 0.35, "tom2_brush": 0.35,
    "tom3_brush": 0.35, "hh_closed_brush": 0.18, "hh_open_brush": 0.22, "crash1_brush": 0.3,
    "crash2_brush": 0.3, "ride_brush": 0.25,
}

OH_GAIN = 0.55
AMB_GAIN = 0.35


def samples_of(xml_path):
    """[(power, file, {mic: filechannel})] in the order given (ascending power)."""
    root = ET.parse(xml_path).getroot()
    out = []
    for s in root.iter("sample"):
        files = {}
        f = None
        for a in s.iter("audiofile"):
            files[a.get("channel")] = int(a.get("filechannel")) - 1
            f = a.get("file")
        out.append((float(s.get("power")), f, files))
    return out


def mix(x, chans, close):
    """13-channel hit → stereo."""
    def ch(name):
        return x[chans[name]].astype(np.float64)
    out = np.zeros((2, x.shape[1]))
    ref = None
    for mic, p in close.items():
        m = ch(mic)
        if ref is None:
            ref = m
        else:
            m = m * S.polarity(ref, m, 44100) * 0.6
        out += S.pan(m, p)
    out[0] += OH_GAIN * ch("OHL")
    out[1] += OH_GAIN * ch("OHR")
    out[0] += AMB_GAIN * ch("AmbL")
    out[1] += AMB_GAIN * ch("AmbR")
    return out


def attack_rms(y, rate):
    m = np.max(np.abs(y), axis=0)
    on = int(np.argmax(m > 0.2 * m.max()))
    seg = y[:, on:on + int(0.05 * rate)]
    return float(np.sqrt(np.mean(seg ** 2)))


def main():
    dl, out, meta_dir = sys.argv[1:4]
    only = set(sys.argv[4:])
    z = rzip.open_remote(URL)
    names = {i.filename for i in z.infolist()}
    work = os.path.join(dl, "drs-work")
    os.makedirs(meta_dir, exist_ok=True)
    meta_path = os.path.join(meta_dir, "drs.json")
    meta = json.load(open(meta_path)) if os.path.exists(meta_path) else {}
    for piece, (kit, inst, k, close, tail) in PIECES.items():
        if only and piece not in only:
            continue
        if piece in meta and not only:
            continue
        shutil.rmtree(work, ignore_errors=True)
        xmlm = f"DRSKit/{inst}/{inst}.xml"
        rzip.extract(z, work, names=[xmlm])
        smp = samples_of(os.path.join(work, xmlm))
        pick = S.spread(len(smp), k)
        want = [f"DRSKit/{inst}/{smp[i][1]}" for i in pick]
        assert all(w in names for w in want), want
        rzip.extract_parallel(z, work, want)
        hits = []
        for i in pick:
            power, f, chans = smp[i]
            rate, x = S.load(os.path.join(work, f"DRSKit/{inst}", f))
            y = mix(x, chans, close)
            y = S.trim(y, rate, tail_db=-60, max_s=tail, fade_s=min(0.3, tail / 4), head_db=-50, preroll_s=0.003)
            hits.append(y)
        g = TARGET[piece] / max(S.peak(h) for h in hits)
        entries = []
        for n, (i, h) in enumerate(zip(pick, hits)):
            h = h * g
            rel = f"samples/{kit}/{piece}_{n + 1:02d}.flac"
            S.save_flac(os.path.join(out, rel), h, rate)
            entries.append({"file": rel, "peak": S.peak(h), "rms": attack_rms(h, rate), "len": h.shape[1] / rate})
        meta[piece] = entries
        json.dump(meta, open(meta_path, "w"), indent=1)
        print(piece, len(entries), "hits", f"peak {S.db(max(e['peak'] for e in entries)):.1f} dB", flush=True)
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
