"""Shared helpers for building the Bramble Studio sound library from the source libraries.

Audio goes through ffmpeg (decode to float, encode 16-bit FLAC with TPDF dither); the maths is
numpy. Nothing from a downloaded archive is ever executed: archives are read as data only.
"""
import json
import os
import subprocess

import numpy as np

RATE = 48000


def probe(path):
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries",
         "stream=sample_rate,channels", "-of", "json", path],
        check=True, capture_output=True).stdout
    s = json.loads(out)["streams"][0]
    return int(s["sample_rate"]), int(s["channels"])


def load(path, rate=None):
    """→ (rate, float32 array [channels, frames]). Resamples when `rate` is given."""
    sr, ch = probe(path)
    cmd = ["ffmpeg", "-v", "error", "-i", path]
    if rate and rate != sr:
        cmd += ["-ar", str(rate)]
        sr = rate
    cmd += ["-f", "f32le", "-acodec", "pcm_f32le", "-"]
    raw = subprocess.run(cmd, check=True, capture_output=True).stdout
    x = np.frombuffer(raw, dtype=np.float32).reshape(-1, ch).T.copy()
    return sr, x


def save_flac(path, x, rate):
    """x: [channels, frames] float. Writes 16-bit FLAC (TPDF dither)."""
    os.makedirs(os.path.dirname(path), exist_ok=True)
    x = np.atleast_2d(np.asarray(x, dtype=np.float64))
    rng = np.random.default_rng(len(path))
    d = (rng.random(x.shape) - rng.random(x.shape))
    q = np.clip(np.round(x * 32767.0 + d), -32768, 32767).astype("<i2")
    ch = x.shape[0]
    raw = q.T.reshape(-1).tobytes()
    subprocess.run(
        ["ffmpeg", "-v", "error", "-y", "-f", "s16le", "-ar", str(rate), "-ac", str(ch), "-i", "-",
         "-c:a", "flac", "-compression_level", "8", path],
        input=raw, check=True)


def db(x):
    return 20.0 * np.log10(max(float(x), 1e-12))


def peak(x):
    return float(np.max(np.abs(x))) if x.size else 0.0


def trim(x, rate, tail_db=-66.0, max_s=None, fade_s=0.08, head_db=None, preroll_s=0.002):
    """Cuts the tail where the signal falls `tail_db` below its peak (and at `max_s`), with a
    fade-out; with `head_db`, also cuts leading silence (keeping `preroll_s`)."""
    env = np.max(np.abs(x), axis=0)
    p = env.max() if env.size else 0.0
    if p <= 0:
        return x[:, :1]
    thr = p * 10 ** (tail_db / 20)
    above = np.nonzero(env > thr)[0]
    end = int(above[-1]) + int(0.02 * rate) if above.size else env.size
    start = 0
    if head_db is not None:
        hthr = p * 10 ** (head_db / 20)
        first = np.nonzero(env > hthr)[0]
        if first.size:
            start = max(0, int(first[0]) - int(preroll_s * rate))
    if max_s:
        end = min(end, start + int(max_s * rate))
    end = min(end, env.size)
    y = x[:, start:end].copy()
    nf = min(int(fade_s * rate), y.shape[1] // 2)
    if nf > 0:
        # fade only if we actually cut something audible
        y[:, -nf:] *= np.cos(np.linspace(0, np.pi / 2, nf)) ** 2
    return y


def pan(mono, p):
    a = (p + 1) * np.pi / 4
    return np.stack([mono * np.cos(a), mono * np.sin(a)]) * np.sqrt(2)


def polarity(ref, other, rate):
    """+1 or -1: the sign that makes `other` add constructively to `ref` over the first 15 ms
    after the onset."""
    n = min(ref.size, other.size)
    on = int(np.argmax(np.abs(ref[:n]) > 0.1 * np.max(np.abs(ref[:n]))))
    a = ref[on:on + int(0.015 * rate)]
    b = other[on:on + int(0.015 * rate)]
    m = min(a.size, b.size)
    return 1.0 if float(np.dot(a[:m], b[:m])) >= 0 else -1.0


def spread(n, k):
    """k indices spread evenly over range(n) (all of them when k >= n)."""
    if k >= n:
        return list(range(n))
    return sorted(set(int(round(i * (n - 1) / (k - 1))) for i in range(k)))


def note_name(n):
    names = ["c", "c#", "d", "d#", "e", "f", "f#", "g", "g#", "a", "a#", "b"]
    return f"{names[n % 12]}{n // 12 - 1}"


def note_num(s):
    """'C4', 'Db3', 'F#2', 'a#0' → MIDI (C4 = 60)."""
    s = s.strip()
    base = {"c": 0, "d": 2, "e": 4, "f": 5, "g": 7, "a": 9, "b": 11}[s[0].lower()]
    i = 1
    if i < len(s) and s[i] in "#s":
        base += 1
        i += 1
    elif i < len(s) and s[i] == "b" and len(s) > 2 and (s[i + 1].isdigit() or s[i + 1] == "-"):
        base -= 1
        i += 1
    return (int(s[i:]) + 1) * 12 + base


def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)
