"""Calculator's app icon, drawn with PIL: Dieter Rams' Braun ET66 (round convex keys, dark brown
digits, light brown operators, a yellow =, 4 × 4) drawn like the calculator in the Values icon
(a black slab with a lit edge, solar strip, green LCD), on the suite's light tile.

  icon.py desktop <dir>   squircle tile matching the Bramble suite icons (superellipse n=5,
                          88% of the canvas): hicolor sizes, plus icon-256.rgba for the window
  icon.py android <res>   adaptive layers (full-bleed background, foreground in the 66 dp safe
                          zone, monochrome) and legacy flat icons
  icon.py preview <png>   both side by side
"""
import math
import os
import sys

from PIL import Image, ImageChops, ImageDraw, ImageFilter

SS = 2  # supersampling


def vgrad(w, h, top, bottom):
    """A vertical gradient image (RGBA) from `top` to `bottom`."""
    g = Image.linear_gradient("L").resize((max(w, 1), max(h, 1)))
    a = Image.new("RGBA", (w, h), top)
    b = Image.new("RGBA", (w, h), bottom)
    return Image.composite(b, a, g)


def paint(canvas, mask, fill):
    """Composites `fill` (a colour or an image the canvas's size) through `mask`."""
    if isinstance(fill, tuple):
        fill = Image.new("RGBA", canvas.size, fill)
    layer = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    layer.paste(fill, (0, 0), mask)
    canvas.alpha_composite(layer)


def shape(size, draw_fn):
    m = Image.new("L", size, 0)
    draw_fn(ImageDraw.Draw(m))
    return m


def gradient_in(size, box, top, bottom):
    """A full-canvas image with a vertical gradient spanning `box` (y0..y1)."""
    x0, y0, x1, y1 = [int(v) for v in box]
    img = Image.new("RGBA", size, top)
    img.paste(vgrad(size[0], max(y1 - y0, 1), top, bottom), (0, y0))
    img.paste(Image.new("RGBA", (size[0], max(size[1] - y1, 0)), bottom), (0, y1))
    return img


def squircle(n, cx, cy, half, power=5.0, steps=720):
    pts = []
    for i in range(steps):
        t = 2 * math.pi * i / steps
        c, s = math.cos(t), math.sin(t)
        x = half * math.copysign(abs(c) ** (2 / power), c)
        y = half * math.copysign(abs(s) ** (2 / power), s)
        pts.append((cx + x, cy + y))
    return pts


def dome(canvas, cx, cy, d, top, bottom):
    """A glossy 2007-style key: dark bezel, domed gradient, bright highlight cap."""
    size = canvas.size
    r = d / 2
    # soft shadow under the key
    sh = shape(size, lambda g: g.ellipse([cx - r, cy - r + d * 0.06, cx + r, cy + r + d * 0.06], fill=150))
    paint(canvas, sh.filter(ImageFilter.GaussianBlur(d * 0.05)), (0, 0, 0, 255))
    # bezel ring
    paint(canvas, shape(size, lambda g: g.ellipse([cx - r, cy - r, cx + r, cy + r], fill=255)), (8, 8, 9, 255))
    ri = r * 0.9
    body = shape(size, lambda g: g.ellipse([cx - ri, cy - ri, cx + ri, cy + ri], fill=255))
    paint(canvas, body, gradient_in(size, (0, cy - ri, 0, cy + ri), top, bottom))
    # light caught along the lower rim
    rim = shape(size, lambda g: g.ellipse([cx - ri * 0.82, cy - ri * 0.1, cx + ri * 0.82, cy + ri * 0.94], fill=255))
    rim = ImageChops.subtract(rim, shape(size, lambda g: g.ellipse([cx - ri * 0.9, cy - ri * 0.3, cx + ri * 0.9, cy + ri * 0.78], fill=255)))
    rim = ImageChops.multiply(rim.filter(ImageFilter.GaussianBlur(d * 0.03)), body)
    paint(canvas, rim, (255, 255, 255, 70))
    # the glossy cap: top half, white fading downward
    hw, hh = ri * 0.74, ri * 0.56
    hy = cy - ri * 0.86
    cap = shape(size, lambda g: g.ellipse([cx - hw, hy, cx + hw, hy + 2 * hh], fill=255))
    capg = gradient_in(size, (0, hy, 0, hy + hh * 1.5), (255, 255, 255, 215), (255, 255, 255, 10))
    paint(canvas, cap.filter(ImageFilter.GaussianBlur(d * 0.008)), capg)


# Dieter Rams' Braun ET66 (1987) in the skeuomorphic style of the Values icon's calculator: a
# black slab with a lit edge, a green LCD under glass, and the ET66's round convex keys (dark
# brown digits, light brown operators, the yellow =), reduced to 4 × 4.
DIGIT_KEY = ((86, 76, 68, 255), (34, 29, 26, 255))
OP_KEY = ((178, 160, 138, 255), (120, 103, 85, 255))
EQ_KEY = ((255, 214, 64, 255), (226, 158, 0, 255))
KEYS = [["7", "8", "9", "÷"], ["4", "5", "6", "×"], ["1", "2", "3", "−"], ["0", ".", "=", "+"]]
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
SEGS = {"0": "abcdef", "1": "bc", "2": "abged", "3": "abgcd", "4": "fgbc", "5": "afgcd", "6": "afgedc", "7": "abc", "8": "abcdefg", "9": "abcdfg"}


def body_geom(cx, cy, bh):
    bw = bh * 0.64
    return bw, cx - bw / 2, cy - bh / 2


def key_centres(cx, cy, bh):
    """(x, y, diameter) of the 4 × 4 keys."""
    bw, x0, y0 = body_geom(cx, cy, bh)
    step = bw * 0.215
    d = step * 0.84
    return [[(cx + (c - 1.5) * step, y0 + bh * 0.44 + r * step, d) for c in range(4)] for r in range(4)]


def braun_key(canvas, cx, cy, d, top, bottom, label, ink):
    """A round convex ET66 key: a dark well, a matte domed cap lit from above, its label."""
    from PIL import ImageFont
    size = canvas.size
    r = d / 2
    sh = shape(size, lambda g: g.ellipse([cx - r, cy - r + d * 0.07, cx + r, cy + r + d * 0.07], fill=170))
    paint(canvas, sh.filter(ImageFilter.GaussianBlur(d * 0.06)), (0, 0, 0, 255))
    paint(canvas, shape(size, lambda g: g.ellipse([cx - r, cy - r, cx + r, cy + r], fill=255)), (6, 5, 5, 255))
    ri = r * 0.92
    cap = shape(size, lambda g: g.ellipse([cx - ri, cy - ri, cx + ri, cy + ri], fill=255))
    paint(canvas, cap, gradient_in(size, (0, cy - ri, 0, cy + ri), top, bottom))
    # soft sheen on the upper half (matte, not the 2007 gloss)
    hw = ri * 0.7
    sheen = shape(size, lambda g: g.ellipse([cx - hw, cy - ri * 0.86, cx + hw, cy + ri * 0.2], fill=255))
    sheen = ImageChops.multiply(sheen.filter(ImageFilter.GaussianBlur(d * 0.08)), cap)
    paint(canvas, sheen, gradient_in(size, (0, cy - ri, 0, cy + ri * 0.2), (255, 255, 255, 80), (255, 255, 255, 0)))
    # a fine lit lower rim
    rim = ImageChops.subtract(cap, shape(size, lambda g: g.ellipse([cx - ri, cy - ri - d * 0.03, cx + ri, cy + ri - d * 0.03], fill=255)))
    paint(canvas, rim, (255, 255, 255, 60))
    if d < 14:
        return
    f = ImageFont.truetype(FONT, max(1, int(d * 0.46)))
    txt = Image.new("L", size, 0)
    ImageDraw.Draw(txt).text((cx, cy), label, font=f, fill=255, anchor="mm")
    paint(canvas, txt, ink)


def segments(canvas, x1, cy, h, text, ink):
    """Seven-segment digits, right-aligned at x1, `h` tall, centred on cy."""
    m = Image.new("L", canvas.size, 0)
    g = ImageDraw.Draw(m)
    w, t = h * 0.5, h * 0.13
    x = x1
    for ch in reversed(text):
        if ch == ".":
            g.rectangle([x - t * 1.1, cy + h / 2 - t, x - t * 0.1, cy + h / 2], fill=255)
            x -= t * 1.6
            continue
        x -= w
        y0, ym, y1 = cy - h / 2, cy, cy + h / 2
        bars = {"a": (x, y0, x + w, y0 + t), "g": (x, ym - t / 2, x + w, ym + t / 2), "d": (x, y1 - t, x + w, y1),
                "f": (x, y0, x + t, ym), "b": (x + w - t, y0, x + w, ym), "e": (x, ym, x + t, y1), "c": (x + w - t, ym, x + w, y1)}
        for sgm in SEGS[ch]:
            g.rectangle(bars[sgm], fill=255)
        x -= t * 1.8
    paint(canvas, m, ink)


def calculator(canvas, cx, cy, bh, shadow=True):
    """The calculator `bh` tall, centred at (cx, cy)."""
    size = canvas.size
    bw, x0, y0 = body_geom(cx, cy, bh)
    x1, y1 = x0 + bw, y0 + bh
    rad = bw * 0.13
    if shadow:
        sh = shape(size, lambda g: g.rounded_rectangle([x0 + bh * 0.01, y0 + bh * 0.035, x1 + bh * 0.01, y1 + bh * 0.035], rad, fill=180))
        paint(canvas, sh.filter(ImageFilter.GaussianBlur(bh * 0.035)), (0, 0, 0, 255))
    body = shape(size, lambda g: g.rounded_rectangle([x0, y0, x1, y1], rad, fill=255))
    paint(canvas, body, gradient_in(size, (0, y0, 0, y1), (52, 48, 46, 255), (14, 13, 13, 255)))
    edge = ImageChops.subtract(body, shape(size, lambda g: g.rounded_rectangle([x0 + bh * 0.005, y0 + bh * 0.007, x1 - bh * 0.005, y1], rad, fill=255)))
    paint(canvas, edge, (255, 255, 255, 75))
    # (the solar strip, digits and key labels only where there are pixels for them)
    detail = bh > 90
    # the solar strip
    sx1, sy0 = x1 - bw * 0.1, y0 + bh * 0.045
    if detail:
        paint(canvas, shape(size, lambda g: g.rounded_rectangle([sx1 - bw * 0.36, sy0, sx1, sy0 + bh * 0.04], bh * 0.006, fill=255)), (62, 44, 36, 255))
    for i in range(1, 4 if detail else 1):
        lx = sx1 - bw * 0.36 * i / 4
        paint(canvas, shape(size, lambda g: g.line([lx, sy0, lx, sy0 + bh * 0.04], fill=255, width=max(1, int(bh * 0.003)))), (110, 78, 62, 255))
    # the LCD: a black bezel, the green glass, a few digits
    lx0, ly0, lx1, ly1 = x0 + bw * 0.1, y0 + bh * 0.115, x1 - bw * 0.1, y0 + bh * 0.3
    paint(canvas, shape(size, lambda g: g.rounded_rectangle([lx0, ly0, lx1, ly1], bh * 0.012, fill=255)), (0, 0, 0, 255))
    gi = bh * 0.012
    glass = shape(size, lambda g: g.rounded_rectangle([lx0 + gi, ly0 + gi, lx1 - gi, ly1 - gi], bh * 0.008, fill=255))
    paint(canvas, glass, gradient_in(size, (0, ly0, 0, ly1), (214, 227, 196, 255), (172, 192, 150, 255)))
    if detail:
        segments(canvas, lx1 - gi - bw * 0.06, (ly0 + ly1) / 2, (ly1 - ly0) * 0.5, "1987", (43, 51, 38, 235))
    inner = ImageChops.subtract(glass, shape(size, lambda g: g.rounded_rectangle([lx0 + gi, ly0 + gi * 2, lx1 - gi, ly1], bh * 0.008, fill=255)))
    paint(canvas, inner, (0, 0, 0, 90))
    for r, row in enumerate(key_centres(cx, cy, bh)):
        for c, (kx, ky, d) in enumerate(row):
            lab = KEYS[r][c]
            if lab == "=":
                braun_key(canvas, kx, ky, d, *EQ_KEY, lab, (40, 30, 10, 255))
            elif c == 3:
                braun_key(canvas, kx, ky, d, *OP_KEY, lab, (30, 24, 20, 255))
            else:
                braun_key(canvas, kx, ky, d, *DIGIT_KEY, lab, (240, 236, 230, 255))


def tile_fill(size, box):
    return gradient_in(size, box, (238, 238, 241, 255), (196, 196, 201, 255))


def desktop(n):
    S = n * SS
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    half = S * 224 / 512  # the suite tile: 896 of 1024, as Keydeck and Values
    c = S / 2
    tile = shape((S, S), lambda g: g.polygon(squircle(S, c, c, half), fill=255))
    paint(img, tile, tile_fill((S, S), (0, c - half, 0, c + half)))
    # the tile's lit rim
    rim = ImageChops.subtract(tile, shape((S, S), lambda g: g.polygon(squircle(S, c, c + S * 0.004, half - S * 0.004), fill=255)))
    paint(img, rim, (255, 255, 255, 200))
    calculator(img, c, c, half * 2 * 0.75)
    return img.resize((n, n), Image.LANCZOS)


def android_layers(n):
    """(background, foreground, monochrome) for an n-px 108 dp canvas."""
    S = n * SS
    bg = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    paint(bg, Image.new("L", (S, S), 255), tile_fill((S, S), (0, 0, 0, S)))
    fg = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    calculator(fg, S / 2, S / 2, S * 58 / 108)
    mono = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    bh = S * 58 / 108
    bw, x0, y0 = body_geom(S / 2, S / 2, bh)
    m = shape((S, S), lambda g: g.rounded_rectangle([x0, y0, x0 + bw, y0 + bh], bw * 0.13, fill=255))
    holes = Image.new("L", (S, S), 0)
    hd = ImageDraw.Draw(holes)
    hd.rounded_rectangle([x0 + bw * 0.1, y0 + bh * 0.115, x0 + bw * 0.9, y0 + bh * 0.3], bh * 0.012, fill=255)
    for row in key_centres(S / 2, S / 2, bh):
        for kx, ky, d in row:
            kr = d * 0.42
            hd.ellipse([kx - kr, ky - kr, kx + kr, ky + kr], fill=255)
    paint(mono, ImageChops.subtract(m, holes), (255, 255, 255, 255))
    return tuple(im.resize((n, n), Image.LANCZOS) for im in (bg, fg, mono))


def suite_tile(out, name):
    """Puts the drawn icon on the suite's tile (Apple's macOS icon grid, keynote-linux/tools/suite/apple_tile.py)."""
    sys.path.insert(0, os.path.expanduser("~/CascadeProjects/keynote-linux/tools/suite"))
    import apple_tile
    apple_tile.write_sizes(apple_tile.retile(f"{out}/{name}-1024.png"), out, name)


def main():
    mode, out = sys.argv[1], sys.argv[2]
    if mode == "desktop":
        os.makedirs(out, exist_ok=True)
        for n in (16, 24, 32, 48, 64, 128, 256, 512, 1024):
            desktop(n).save(f"{out}/calculator-{n}.png")
        suite_tile(out, "calculator")
        with open(f"{out}/icon-256.rgba", "wb") as f:
            f.write(Image.open(f"{out}/calculator-256.png").convert("RGBA").tobytes())
    elif mode == "android":
        for name, dp in [("mdpi", 1), ("hdpi", 1.5), ("xhdpi", 2), ("xxhdpi", 3), ("xxxhdpi", 4)]:
            d = f"{out}/mipmap-{name}"
            os.makedirs(d, exist_ok=True)
            bg, fg, mono = android_layers(round(108 * dp))
            bg.convert("RGB").save(f"{d}/ic_launcher_background.png")
            fg.save(f"{d}/ic_launcher_foreground.png")
            mono.save(f"{d}/ic_launcher_monochrome.png")
            desktop(round(48 * dp)).save(f"{d}/ic_launcher.png")
        os.makedirs(f"{out}/mipmap-anydpi-v26", exist_ok=True)
        with open(f"{out}/mipmap-anydpi-v26/ic_launcher.xml", "w") as f:
            f.write('<?xml version="1.0" encoding="utf-8"?>\n<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n'
                    '    <background android:drawable="@mipmap/ic_launcher_background" />\n'
                    '    <foreground android:drawable="@mipmap/ic_launcher_foreground" />\n'
                    '    <monochrome android:drawable="@mipmap/ic_launcher_monochrome" />\n</adaptive-icon>\n')
    elif mode == "preview":
        n = 432
        sheet = Image.new("RGBA", (n * 3 + 80, n + 40), (110, 110, 112, 255))
        sheet.alpha_composite(desktop(n), (20, 20))
        bg, fg, _ = android_layers(n)
        a = bg.copy()
        a.alpha_composite(fg)
        # how launchers show it: the circle mask, and a rounded-square mask, at 72/108 of the canvas
        for i, mask in enumerate(("circle", "square")):
            m = Image.new("L", (n, n), 0)
            g = ImageDraw.Draw(m)
            p = n * 18 / 108
            if mask == "circle":
                g.ellipse([p, p, n - p, n - p], fill=255)
            else:
                g.rounded_rectangle([p, p, n - p, n - p], n * 0.12, fill=255)
            cut = Image.new("RGBA", (n, n), (0, 0, 0, 0))
            cut.paste(a, (0, 0), m)
            sheet.alpha_composite(cut, (40 + n * (i + 1), 20))
        sheet.save(out)


if __name__ == "__main__":
    main()
