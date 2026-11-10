"""Bramble Studio app icon: a record on felt in a molded frame (GNOME-style rounded square
with a deeper bottom lip), a nameplate "D. Bramble". Three colour variants."""
import math, os, sys
import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

N = 1024
SS = 2  # supersampling
S = N * SS
OUT = sys.argv[1]
FONT = os.path.expanduser('~/CascadeProjects/bramble-office/keynote-linux/crates/kn-render/fonts/matched/sfera/Sfera-Semibold.ttf')

yy, xx = np.mgrid[0:S, 0:S].astype(np.float32)


def rr_mask(x0, y0, x1, y1, r):
    """Signed-ish coverage of a rounded rectangle (in supersampled pixels)."""
    cx = np.clip(xx, x0 + r, x1 - r)
    cy = np.clip(yy, y0 + r, y1 - r)
    d = np.sqrt((xx - cx) ** 2 + (yy - cy) ** 2) - r
    return np.clip(0.5 - d, 0, 1), d


def lerp(a, b, t):
    return a + (b - a) * t[..., None]


def hexc(h):
    return np.array([int(h[i:i + 2], 16) for i in (1, 3, 5)], np.float32) / 255.0


def make(v):
    img = np.zeros((S, S, 4), np.float32)

    def over(rgb, a):
        a = a[..., None]
        img[..., :3] = rgb * a + img[..., :3] * (1 - a)
        img[..., 3:] = a + img[..., 3:] * (1 - a)

    m = 64 * SS  # margin
    R = 190 * SS  # outer radius
    x0, y0, x1, y1 = m, m, S - m, S - m - 14 * SS
    # GNOME depth: a darker lip under the plate, then a soft shadow
    sh, _ = rr_mask(x0, y0 + 26 * SS, x1, y1 + 26 * SS, R)
    shadow = np.array(Image.fromarray((sh * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(18 * SS))) / 255.0
    over(np.zeros(3, np.float32), shadow * 0.35)
    lip, _ = rr_mask(x0, y0 + 18 * SS, x1, y1 + 18 * SS, R)
    over(v['frame_dark'], lip)
    # the crown molding: bands from the outside in, each a bevel lit from above
    bands = v['bands']
    inset = 0
    for (w, c_hi, c_lo, kind) in bands:
        a, d = rr_mask(x0 + inset, y0 + inset, x1 - inset, y1 - inset, max(R - inset, 8))
        t = (yy - (y0 + inset)) / max(1.0, (y1 - y0 - 2 * inset))
        t = np.clip(t, 0, 1)
        if kind == 'cove':  # concave: dark at top, light at bottom
            col = lerp(c_lo, c_hi, t)
        elif kind == 'bead':  # round bead: a bright line across its middle
            edge = np.clip(-d / (w * SS), 0, 1)
            col = lerp(c_lo, c_hi, 1 - np.abs(edge - 0.5) * 2)
        else:  # ogee / flat: light at top
            col = lerp(c_hi, c_lo, t)
        # brushed-metal / wood grain streaks
        grain = (np.sin(xx * 0.9 / SS + np.sin(yy * 0.013) * 6) * 0.5 + 0.5) * v['grain']
        col = col * (1 - grain[..., None] * 0.18)
        over(col, a)
        inset += w * SS
    # dentil ring just inside the molding (small blocks around the panel)
    a_out, d_out = rr_mask(x0 + inset, y0 + inset, x1 - inset, y1 - inset, max(R - inset, 8))
    dent_w = 10 * SS
    a_in, _ = rr_mask(x0 + inset + dent_w, y0 + inset + dent_w, x1 - inset - dent_w, y1 - inset - dent_w, max(R - inset - dent_w, 8))
    ring = a_out * (1 - a_in)
    ang = np.arctan2(yy - S / 2, xx - S / 2)
    teeth = (np.sin(ang * 96) > 0).astype(np.float32)
    over(v['dentil_hi'] * 0.85 + 0.15 * teeth[..., None] * v['dentil_hi'], ring * (0.55 + 0.45 * teeth))
    inset += dent_w
    # the felt panel: fibrous noise, a soft vignette
    pa, pd = rr_mask(x0 + inset, y0 + inset, x1 - inset, y1 - inset, max(R - inset, 8))
    rng = np.random.default_rng(7)
    n1 = rng.standard_normal((S // 2, S // 2)).astype(np.float32)
    n1 = np.array(Image.fromarray(((n1 * 0.25 + 0.5).clip(0, 1) * 255).astype(np.uint8)).resize((S, S), Image.BILINEAR).filter(ImageFilter.GaussianBlur(1.2 * SS))) / 255.0
    fib = rng.standard_normal((S, S)).astype(np.float32)
    fib = np.array(Image.fromarray(((fib * 0.25 + 0.5).clip(0, 1) * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(0.6 * SS))) / 255.0
    felt = v['felt'] * (0.86 + 0.22 * n1[..., None] + 0.10 * (fib[..., None] - 0.5))
    vign = np.clip(1 - ((xx - S / 2) ** 2 + (yy - S / 2) ** 2) / (S * 0.62) ** 2, 0, 1)
    felt = felt * (0.72 + 0.28 * vign[..., None])
    # inner shadow at the panel's top edge
    edge = np.clip(-pd / (40 * SS), 0, 1)
    felt = felt * (0.55 + 0.45 * edge[..., None])
    over(felt, pa)
    # the record
    cx, cy = S / 2, (y0 + y1) / 2 - 18 * SS
    rx = (x1 - x0 - 2 * inset) * 0.43
    r = np.sqrt((xx - cx) ** 2 + (yy - cy) ** 2)
    th = np.arctan2(yy - cy, xx - cx)
    disc = np.clip(rx - r + 0.5, 0, 1)
    # drop shadow of the record on the felt
    ds = np.array(Image.fromarray((disc * 255).astype(np.uint8)).filter(ImageFilter.GaussianBlur(16 * SS))) / 255.0
    ds = np.roll(ds, 14 * SS, axis=0)
    over(np.zeros(3, np.float32), ds * 0.55 * pa)
    # iridescent sheen: hue cycling with the angle (twice round, as light catches grooves),
    # strongest in two opposite wedges; base dark
    pal = v['vinyl']
    k = (np.cos(2 * (th - v['light'])) * 0.5 + 0.5) ** 1.6
    hue_t = (th / (2 * math.pi) * 2 + r / rx * 0.35) % 1.0
    idx = hue_t * len(pal)
    i0 = np.floor(idx).astype(int) % len(pal)
    i1 = (i0 + 1) % len(pal)
    f = (idx - np.floor(idx))[..., None]
    P = np.array(pal)
    sheen = P[i0] * (1 - f) + P[i1] * f
    grooves = 0.82 + 0.18 * np.sin(r / SS * 1.9) * np.sin(r / SS * 0.23)
    base = v['vinyl_base']
    col = base * (1 - k[..., None]) + sheen * k[..., None]
    col = col * grooves[..., None]
    # a fine bright arc of specular light
    spec = np.exp(-((np.cos(th - v['light']) - 1) ** 2) / 0.002) * 0.35
    col = np.clip(col + spec[..., None], 0, 1)
    # the run-in and run-out bands (smoother, slightly brighter)
    band = ((r > rx * 0.94) | ((r < rx * 0.40) & (r > rx * 0.36))).astype(np.float32)
    col = col * (1 + 0.12 * band[..., None])
    over(col, disc)
    # the centre label and spindle
    lr = rx * 0.33
    lab = np.clip(lr - r + 0.5, 0, 1)
    lc = v['label'] * (0.9 + 0.1 * (np.cos(th - v['light']) * 0.5 + 0.5))[..., None]
    over(lc, lab)
    ring2 = np.clip(lr * 0.62 - r + 0.5, 0, 1) - np.clip(lr * 0.6 - r + 0.5, 0, 1)
    over(v['label_ink'], np.clip(ring2, 0, 1) * 0.6)
    hole = np.clip(rx * 0.035 - r + 0.5, 0, 1)
    over(np.array([0.05, 0.05, 0.05], np.float32), hole)
    out = Image.fromarray((np.clip(img, 0, 1) * 255).astype(np.uint8), 'RGBA')
    d = ImageDraw.Draw(out)
    # the nameplate, low on the panel: a small plate with bevel, engraved "D. Bramble"
    pw, ph = 250 * SS, 46 * SS
    px0 = S / 2 - pw / 2
    py0 = y1 - inset - ph - 34 * SS
    d.rounded_rectangle([px0 + 2 * SS, py0 + 4 * SS, px0 + pw + 2 * SS, py0 + ph + 4 * SS], radius=8 * SS, fill=(0, 0, 0, 110))
    d.rounded_rectangle([px0, py0, px0 + pw, py0 + ph], radius=8 * SS, fill=tuple(int(c * 255) for c in v['plate']) + (255,), outline=tuple(int(c * 255) for c in v['plate_edge']) + (255,), width=3 * SS)
    font = ImageFont.truetype(FONT, 26 * SS)
    text = 'D. Bramble'
    tw = d.textlength(text, font=font)
    d.text((S / 2 - tw / 2, py0 + ph / 2 - 15 * SS + 1 * SS), text, font=font, fill=(255, 255, 255, 120))
    d.text((S / 2 - tw / 2, py0 + ph / 2 - 15 * SS), text, font=font, fill=tuple(int(c * 255) for c in v['plate_ink']) + (255,))
    return out.resize((N, N), Image.LANCZOS)


# the record's colours, from the gold record photo: gold, amber, copper, magenta, violet,
# teal-green, back to gold
IRIS = [hexc(h) for h in ['#f4d27a', '#e9a54a', '#c9643a', '#b2457f', '#6f58b8', '#3f9d8f', '#a9c46a', '#f4d27a']]

V = {
    'a-silver-charcoal': dict(
        bands=[(14, hexc('#f2f4f6'), hexc('#8a9098'), 'ogee'), (10, hexc('#ffffff'), hexc('#6f757d'), 'bead'), (18, hexc('#d9dde2'), hexc('#5d636b'), 'cove'), (8, hexc('#ffffff'), hexc('#8c9299'), 'bead'), (12, hexc('#c7ccd2'), hexc('#4d5259'), 'ogee')],
        frame_dark=hexc('#5a6068'), dentil_hi=hexc('#b9bec5'), grain=0.4,
        felt=hexc('#2b2d31'), vinyl=IRIS, vinyl_base=hexc('#2a2420'), light=-0.7,
        label=hexc('#2f2a26'), label_ink=hexc('#c9a45a'),
        plate=hexc('#d7dbe0'), plate_edge=hexc('#8b9097'), plate_ink=hexc('#3c4046')),
    'b-walnut-brass-green': dict(
        bands=[(14, hexc('#f0d38a'), hexc('#8a6a2c'), 'ogee'), (10, hexc('#fff0bf'), hexc('#7c5c22'), 'bead'), (18, hexc('#8b5a35'), hexc('#3d2412'), 'cove'), (8, hexc('#f6dd95'), hexc('#7d5d24'), 'bead'), (12, hexc('#7a4d2c'), hexc('#2f1b0c'), 'ogee')],
        frame_dark=hexc('#3a2412'), dentil_hi=hexc('#d9b866'), grain=0.9,
        felt=hexc('#1f4a33'), vinyl=IRIS, vinyl_base=hexc('#241e1a'), light=-0.9,
        label=hexc('#5b2a3c'), label_ink=hexc('#e7c77a'),
        plate=hexc('#e2c06a'), plate_edge=hexc('#8a6a2c'), plate_ink=hexc('#3a2a10')),
    'c-bronze-blackberry': dict(
        bands=[(14, hexc('#d9a77c'), hexc('#6b3f22'), 'ogee'), (10, hexc('#f3cfa8'), hexc('#5e3519'), 'bead'), (18, hexc('#a8714a'), hexc('#3a2010'), 'cove'), (8, hexc('#f0c79c'), hexc('#5e3519'), 'bead'), (12, hexc('#8e5c3a'), hexc('#2a160a'), 'ogee')],
        frame_dark=hexc('#3a2010'), dentil_hi=hexc('#c99467'), grain=0.6,
        felt=hexc('#3b2140'), vinyl=IRIS, vinyl_base=hexc('#1f1a1c'), light=-0.6,
        label=hexc('#1e1a2a'), label_ink=hexc('#d8b06a'),
        plate=hexc('#d9a77c'), plate_edge=hexc('#6b3f22'), plate_ink=hexc('#2a160a')),
}

os.makedirs(OUT, exist_ok=True)
for name, v in V.items():
    im = make(v)
    im.save(os.path.join(OUT, f'studio-icon-{name}.png'))
    print(name)
