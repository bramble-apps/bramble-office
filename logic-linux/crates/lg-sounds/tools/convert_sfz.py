"""Converts a source library's SFZ instrument into a Bramble one: flattens the SFZ (#define,
#include, header inheritance), keeps the regions the default state plays, converts each sample to
16-bit FLAC (peak-normalised, tail trimmed, WAV loop points carried into the SFZ), and writes a
flat SFZ with library-relative sample paths.
"""
import os
import re
import struct
import sys
from concurrent.futures import ThreadPoolExecutor

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np  # noqa: E402
import sndlib as S  # noqa: E402

# opcodes our Sampler understands (others are dropped)
KEEP = [
    "lokey", "hikey", "pitch_keycenter", "lovel", "hivel", "seq_length", "seq_position",
    "lorand", "hirand", "trigger", "group", "off_by", "tune", "transpose", "pitch_keytrack",
    "volume", "amplitude", "pan", "offset", "end", "loop_mode", "loop_start", "loop_end",
    "ampeg_delay", "ampeg_attack", "ampeg_hold", "ampeg_decay", "ampeg_sustain", "ampeg_release",
    "amp_veltrack", "cutoff", "resonance", "fil_type",
]
ALIASES = {"loopmode": "loop_mode", "loopstart": "loop_start", "loopend": "loop_end"}


# ---------------------------------------------------------------------------------------------
# parsing

def strip_comments(text):
    out = []
    in_block = False
    for line in text.splitlines():
        if in_block:
            if "*/" in line:
                line = line.split("*/", 1)[1]
                in_block = False
            else:
                continue
        while "/*" in line:
            a, b = line.split("/*", 1)
            if "*/" in b:
                line = a + b.split("*/", 1)[1]
            else:
                line = a
                in_block = True
        i = line.find("//")
        if i >= 0:
            line = line[:i]
        out.append(line)
    return out


OPC = re.compile(r"(?:(?<=\s)|^)([A-Za-z0-9_$]+)=")


def tokens(text, read_include, defines):
    """→ list of ('h', header) / ('o', key, value)."""
    toks = []
    for line in strip_comments(text):
        s = line.strip()
        if not s:
            continue
        m = re.match(r'#define\s+(\$\w+)\s+(.*)$', s)
        if m:
            defines[m.group(1)] = m.group(2).strip()
            continue
        m = re.match(r'#include\s+"([^"]+)"', s)
        if m:
            toks += tokens(read_include(m.group(1)), read_include, defines)
            continue
        for name, val in sorted(defines.items(), key=lambda kv: -len(kv[0])):
            s = s.replace(name, val)
        pos = 0
        while pos < len(s):
            if s[pos] == "<":
                j = s.find(">", pos)
                toks.append(("h", s[pos + 1:j].strip()))
                pos = j + 1
                continue
            if s[pos].isspace():
                pos += 1
                continue
            m = OPC.match(s, pos) if pos == 0 or s[pos - 1].isspace() else None
            if not m:
                m2 = OPC.search(s, pos)
                if not m2:
                    break
                pos = m2.start()
                continue
            key = m.group(1)
            vstart = m.end()
            nxt = len(s)
            h = s.find("<", vstart)
            if h >= 0:
                nxt = h
            for m2 in OPC.finditer(s, vstart):
                if m2.start() < nxt:
                    nxt = m2.start()
                break
            toks.append(("o", ALIASES.get(key, key), s[vstart:nxt].strip()))
            pos = nxt
    return toks


def flatten(text, read_include):
    defines = {}
    toks = tokens(text, read_include, defines)
    control, glob, master, group = {}, {}, {}, {}
    region = None
    level = None
    regions = []
    for t in toks:
        if t[0] == "h":
            if region is not None:
                regions.append(region)
                region = None
            h = t[1]
            if h == "control":
                level = control
            elif h == "global":
                glob, master, group = {}, {}, {}
                level = glob
            elif h == "master":
                master, group = {}, {}
                level = master
            elif h == "group":
                group = {}
                level = group
            elif h == "region":
                region = {**glob, **master, **group}
                level = region
            else:
                level = None
        elif level is not None:
            level[t[1]] = t[2]
    if region is not None:
        regions.append(region)
    return control, regions


def apply_cc_defaults(r, control):
    """Folds `<opcode>_onccN=v` modulations in at the controllers' default values (set_ccN)."""
    add = {}
    for k, v in list(r.items()):
        m = re.match(r"(ampeg_attack|ampeg_hold|ampeg_decay|ampeg_sustain|ampeg_release|ampeg_delay|"
                     r"pan|volume|offset|amplitude|tune)_oncc(\d+)$", k)
        if not m or num(v) is None:
            continue
        cc = float(control.get(f"set_cc{m.group(2)}", 0)) / 127.0
        add.setdefault(m.group(1), []).append(num(v) * cc)
    for op, vals in add.items():
        if op == "amplitude":
            base = num(r.get("amplitude")) or 100.0
            for v in vals:
                base *= v / 100.0
            r["amplitude"] = f"{base:.3f}"
        else:
            base = num(r.get(op)) or (100.0 if op == "ampeg_sustain" and op not in r else 0.0)
            r[op] = f"{base + sum(vals):.4f}"
    return r


def plays_by_default(r, control):
    if r.get("trigger", "attack") not in ("attack", "release"):
        return False
    for k, v in r.items():
        m = re.match(r"(lo|hi)cc(\d+)$", k)
        if m:
            cur = float(control.get(f"set_cc{m.group(2)}", 0))
            if m.group(1) == "lo" and cur < float(v):
                return False
            if m.group(1) == "hi" and cur > float(v):
                return False
    if "sw_last" in r:
        d = r.get("sw_default", control.get("sw_default"))
        if d is None:
            return False
        dv = int(d) if str(d).lstrip("-").isdigit() else S.note_num(d)
        lv = int(r["sw_last"]) if r["sw_last"].lstrip("-").isdigit() else S.note_num(r["sw_last"])
        return dv == lv
    return True


# ---------------------------------------------------------------------------------------------
# WAV loop points

def wav_loop(path):
    try:
        with open(path, "rb") as f:
            b = f.read()
    except OSError:
        return None
    if b[:4] != b"RIFF" or b[8:12] != b"WAVE":
        return None
    o = 12
    while o + 8 <= len(b):
        cid = b[o:o + 4]
        sz = struct.unpack("<I", b[o + 4:o + 8])[0]
        if cid == b"smpl" and sz >= 36:
            n = struct.unpack("<I", b[o + 8 + 28:o + 8 + 32])[0]
            if n >= 1 and sz >= 36 + 24:
                lo = o + 8 + 36
                start, end = struct.unpack("<II", b[lo + 8:lo + 16])
                return start, end
        o += 8 + sz + (sz & 1)
    return None


# ---------------------------------------------------------------------------------------------
# conversion

def autoloop(y, sr, xf_s=0.25, min_s=0.8):
    """Finds a loop in the steady part of a sustained sample and bakes a crossfade into it.
    → (y truncated after the loop, loop_start, loop_end exclusive) or None."""
    mono = y.mean(axis=0)
    hop = int(0.01 * sr)
    nb = mono.size // hop
    if nb < 20:
        return None
    rms = np.sqrt(np.mean(mono[:nb * hop].reshape(nb, hop) ** 2, axis=1))
    ref = np.median(rms[nb // 4: 3 * nb // 4]) if nb > 8 else rms.max()
    steady = np.nonzero(rms >= 0.45 * ref)[0]
    if steady.size < 10:
        return None
    a, b = steady[0] * hop, steady[-1] * hop
    if (b - a) / sr < min_s + 0.4:
        return None
    s0 = int(a + 0.35 * (b - a))
    s0 = max(s0, int(0.35 * sr))
    e0 = int(a + 0.92 * (b - a))
    if (e0 - s0) / sr < min_s:
        return None
    # refine the end: best match of the waveform just before the loop start
    w = int(0.02 * sr)
    tmpl = mono[s0 - w:s0]
    best, be = -2.0, e0
    for e in range(e0 - int(0.012 * sr), e0 + int(0.012 * sr)):
        if e + 1 > mono.size or e - w < s0:
            continue
        seg = mono[e - w:e]
        c = float(np.dot(seg, tmpl) / (np.linalg.norm(seg) * np.linalg.norm(tmpl) + 1e-12))
        if c > best:
            best, be = c, e
    e0 = be
    xf = min(int(xf_s * sr), (e0 - s0) // 3, s0)
    out = y[:, :e0].copy()
    t = np.linspace(0, 1, xf)
    fi, fo = np.sin(t * np.pi / 2), np.cos(t * np.pi / 2)
    out[:, e0 - xf:e0] = y[:, e0 - xf:e0] * fo + y[:, s0 - xf:s0] * fi
    return out, s0, e0


def safe(name):
    base = os.path.splitext(os.path.basename(name.replace("\\", "/")))[0]
    return re.sub(r"[^A-Za-z0-9_.#+-]", "_", base)


def num(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def convert(inst_id, sfz_text, local, read_include, out_root, sfz_out, *, header,
            region_filter=None, tail_db=-70.0, max_s=None, fade_s=0.15, release=False,
            gain_db=0.0, extra_global="", rename=None, mono=False, rate=None, loop=False):
    """`local(rel)` → local path of a source sample (fetched beforehand)."""
    control, regions = flatten(sfz_text, read_include)
    default_path = control.get("default_path", "").replace("\\", "/")
    regions = [r for r in regions if "sample" in r and not r["sample"].startswith("*")]
    regions = [apply_cc_defaults(r, control) for r in regions if plays_by_default(r, control)]
    if not release:
        regions = [r for r in regions if r.get("trigger", "attack") == "attack"]
    if region_filter:
        regions = [r for r in regions if region_filter(r)]
    srcs = {}
    for r in regions:
        rel = (default_path + r["sample"]).replace("\\", "/")
        r["_src"] = rel
        srcs.setdefault(rel, []).append(r)
    names = {}
    used = set()
    for rel in srcs:
        n = rename(rel) if rename else safe(rel)
        while n in used:
            n += "_"
        used.add(n)
        names[rel] = n

    def one(rel):
        p = local(rel)
        sr, x = S.load(p, rate)
        if mono and x.shape[0] > 1:
            x = x.mean(axis=0, keepdims=True)
        x = x[:2]
        lp = wav_loop(p) if p.lower().endswith(".wav") else None
        rs = srcs[rel]
        looped = any(r.get("loop_mode", "loop_continuous" if lp else "no_loop") in
                     ("loop_continuous", "loop_sustain") for r in rs)
        loop_end = None
        if looped:
            le = [num(r.get("loop_end")) for r in rs if num(r.get("loop_end"))]
            loop_end = int(max(le)) + 1 if le else (lp[1] + 1 if lp else None)
        offs = max([int(num(r.get("offset")) or 0) for r in rs] + [0])
        y = x.astype(np.float64)
        made = None
        if loop and not loop_end:
            y = S.trim(y, sr, tail_db=-60, fade_s=0.01)
            made = autoloop(y, sr)
            if made:
                y, ls, le_ = made
                made = (ls, le_ - 1)
        if made:
            pass
        elif loop_end:
            keep = max(loop_end + int(0.05 * sr), 0)
            ty = S.trim(y, sr, tail_db=tail_db, max_s=None, fade_s=fade_s)
            if ty.shape[1] < keep:
                ty = y[:, :min(keep, y.shape[1])]
            y = ty
        else:
            y = S.trim(y, sr, tail_db=tail_db, max_s=(max_s + offs / sr) if max_s else None, fade_s=fade_s)
        pk = S.peak(y)
        f = 0.89 / pk if pk > 0 else 1.0
        y = y * f
        relout = f"samples/{inst_id}/{names[rel]}.flac"
        S.save_flac(os.path.join(out_root, relout), y, sr)
        return rel, relout, -S.db(f), made or lp, y.shape[1], sr

    with ThreadPoolExecutor(6) as ex:
        res = list(ex.map(one, list(srcs)))
    info = {r[0]: r[1:] for r in res}
    lines = [header.rstrip(), "", "<global>"]
    if gain_db:
        lines.append(f"volume={gain_db:.2f}")
    if extra_global:
        lines += extra_global.strip().splitlines()
    for r in regions:
        relout, comp_db, lp, frames, sr = info[r["_src"]]
        lines.append("<region>")
        lines.append(f"sample={relout}")
        o = {k: r[k] for k in KEEP if k in r}
        if "key" in r:
            o.setdefault("lokey", r["key"])
            o.setdefault("hikey", r["key"])
            o.setdefault("pitch_keycenter", r["key"])
        if lp and (loop or ("loop_start" not in o and o.get("loop_mode") not in ("no_loop", "one_shot"))):
            o["loop_mode"] = "loop_continuous"
            o["loop_start"], o["loop_end"] = str(lp[0]), str(lp[1])
            o.setdefault("loop_mode", "loop_continuous")
        if gain_db and "volume" in o:
            pass
        vol = (num(o.get("volume")) or 0.0) + comp_db
        o["volume"] = f"{vol:.2f}"
        if "end" in o and num(o["end"]) and num(o["end"]) >= frames:
            del o["end"]
        for k in KEEP:
            if k in o:
                lines.append(f"{k}={o[k]}")
    S.write(sfz_out, "\n".join(lines) + "\n")
    return len(regions), len(srcs)


def prefetch(rels, fetch_one, workers=8):
    with ThreadPoolExecutor(workers) as ex:
        list(ex.map(fetch_one, rels))
