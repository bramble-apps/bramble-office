"""Bramble Studio icon: a brushed-metal record on a dark square, in a gold record's iridescent
colours (gold, amber, copper, rose, violet, teal). `make_record_icon.py <out_dir>`."""
import math, os, sys
import numpy as np
from PIL import Image

N, SS = 1024, 2
S = N * SS
yy, xx = np.mgrid[0:S, 0:S].astype(np.float32)


def hexc(h):
    return np.array([int(h[i:i + 2], 16) for i in (1, 3, 5)], np.float32) / 255.0


def palette(th, pal):
    t = (th / (2 * math.pi)) % 1.0 * len(pal)
    i0 = np.floor(t).astype(int) % len(pal)
    f = (t - np.floor(t))[..., None]
    P = np.array(pal)
    return P[i0] * (1 - f) + P[(i0 + 1) % len(pal)] * f


def make(pal, sheen_mix):
    img = np.zeros((S, S, 4), np.float32)

    def over(rgb, a):
        a = np.clip(a, 0, 1)[..., None]
        img[..., :3] = rgb * a + img[..., :3] * (1 - a)
        img[..., 3:] = a + img[..., 3:] * (1 - a)

    # the dark square (GNOME-style rounded corners), a soft top-to-bottom gradient
    m, R = 40 * SS, 180 * SS
    cx_, cy_ = np.clip(xx, m + R, S - m - R), np.clip(yy, m + R, S - m - R)
    sq = np.clip(0.5 - (np.sqrt((xx - cx_) ** 2 + (yy - cy_) ** 2) - R), 0, 1)
    t = (yy / S)[..., None]
    over(hexc('#2e2e30') * (1 - t) + hexc('#151516') * t, sq)
    # the record
    cx, cy, rx = S / 2, S / 2, S * 0.365
    r = np.sqrt((xx - cx) ** 2 + (yy - cy) ** 2)
    th = np.arctan2(yy - cy, xx - cx)
    disc = np.clip(rx - r + 0.5, 0, 1)
    # brushed metal: bright and dark sectors (anisotropic highlights), as on a turned disc
    sheen = 0.55 + 0.30 * np.cos(2 * (th + 0.9)) + 0.12 * np.cos(6 * (th + 0.2)) + 0.05 * np.cos(14 * th)
    # the gold record's colours sweep round with the angle (twice, as light does on grooves)
    col = palette(2 * th + 0.8 + r / rx * 0.4, pal)
    base = col * (1 - sheen_mix) + sheen_mix * np.array([1, 0.92, 0.75], np.float32)
    metal = np.clip(base * sheen[..., None] * 1.15, 0, 1)
    # fine turning lines and a few bright rings (the bands between tracks)
    fine = 0.94 + 0.06 * np.sin(r / SS * 2.4)
    metal = metal * fine[..., None]
    for k, rr in enumerate([0.93, 0.84, 0.75, 0.66, 0.57]):
        ring = np.exp(-((r - rr * rx) / (1.4 * SS)) ** 2)
        metal = np.clip(metal + ring[..., None] * 0.35 * (0.6 + 0.4 * np.cos(2 * (th + 0.9)))[..., None], 0, 1)
    over(metal, disc)
    # the label area: a darker turned ring, then the inner disc
    lr = rx * 0.37
    lab = np.clip(lr - r + 0.5, 0, 1)
    lab_col = np.clip(palette(2 * th + 2.0, pal) * 0.55 * (0.7 + 0.3 * np.cos(2 * (th + 0.9)))[..., None] + 0.08, 0, 1)
    over(lab_col, lab)
    inner = np.clip(lr * 0.72 - r + 0.5, 0, 1)
    in_col = np.clip(palette(2 * th + 0.8, pal) * (0.62 + 0.28 * np.cos(2 * (th + 0.9)))[..., None], 0, 1)
    over(in_col, inner)
    edge = np.exp(-((r - lr * 0.72) / (1.2 * SS)) ** 2)
    over(np.array([0.1, 0.08, 0.06], np.float32), edge * 0.7)
    hole = np.clip(rx * 0.03 - r + 0.5, 0, 1)
    over(np.array([0.02, 0.02, 0.02], np.float32), hole)
    # a thin dark rim round the record
    rim = np.exp(-((r - rx) / (1.2 * SS)) ** 2)
    over(np.array([0.05, 0.04, 0.03], np.float32), rim * 0.8)
    return Image.fromarray((np.clip(img, 0, 1) * 255).astype(np.uint8), 'RGBA').resize((N, N), Image.LANCZOS)


GOLD = [hexc(h) for h in ['#f6d98a', '#eab45a', '#d7894a', '#c66a6a', '#a072b8', '#6aa6a6', '#b9cf7a', '#f6d98a']]
WARM = [hexc(h) for h in ['#f7e3a0', '#f0c46a', '#e09a52', '#d97f5e', '#b9779f', '#d9b56a', '#f7e3a0']]
out = sys.argv[1]
os.makedirs(out, exist_ok=True)
for name, pal, mix in [('iridescent', GOLD, 0.15), ('gold', WARM, 0.25), ('rainbow', GOLD, 0.0)]:
    make(pal, mix).save(os.path.join(out, f'studio-record-{name}.png'))
    print(name)
