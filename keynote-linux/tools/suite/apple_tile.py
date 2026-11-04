"""The Bramble suite's app-icon tile: Apple's macOS app icon grid. On a 1024 canvas the body is
824 x 824, inset 100, with continuous ("squircle") corners of radius 185.4 and a shadow 10 px down,
10 px blur, 30% black. Every suite icon (Keydeck, Values, Letters, Calculator, Notes) uses it.

  apple_tile.py retile <in-1024.png> <out-dir> <name>   re-mask an existing icon's artwork onto the
                                                         tile and write <name>-16 … -1024.png
  apple_tile.py preview <out.png> <icon-1024.png> …      icons side by side, large and small
Other scripts import mask() and finish() to draw on the tile directly.
"""
import os
import sys

from PIL import Image, ImageDraw, ImageFilter

CANVAS = 1024
BODY = 824
INSET = 100
RADIUS = 185.4
SHADOW = (0, 10, 10, 0.30)  # x, y offset, blur, opacity
SIZES = (16, 24, 32, 48, 64, 128, 256, 512, 1024)


def _bezier(p0, p1, p2, p3, n=24):
    out = []
    for i in range(1, n + 1):
        t = i / n
        u = 1 - t
        out.append((u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0],
                    u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1]))
    return out


def outline(x, y, w, h, r):
    """Apple's continuous-corner rounded rectangle (UIBezierPath's, as PaintCode reconstructs it)."""
    r = min(r, w / 2 / 1.52866483, h / 2 / 1.52866483)
    # one corner (top right) in a local frame: (along the top edge from the corner, down from it)
    k = [(1.52866471, 0), (1.08849323, 0), (0.86840689, 0), (0.66993427, 0.06549600),
         (0.63149399, 0.07491100), (0.37282392, 0.16905899), (0.16906013, 0.37282401), (0.07491176, 0.63149399),
         (0.06549569, 0.66993427), (0, 0.86840689), (0, 1.08849323), (0, 1.52866471)]
    def corner(cx, cy, sx, sy, swap):
        pts = []
        q = [(cx - sx * a * r, cy + sy * b * r) if not swap else (cx + sx * b * r, cy + sy * a * r) for a, b in k]
        if swap:
            q = q
        pts.append(q[0])
        pts += _bezier(q[0], q[1], q[2], q[3])
        pts.append(q[4])
        pts += _bezier(q[4], q[5], q[6], q[7])
        pts.append(q[8])
        pts += _bezier(q[8], q[9], q[10], q[11])
        return pts
    x1, y1 = x + w, y + h
    pts = []
    pts += corner(x1, y, 1, 1, False)                       # top right: from the top edge down the right
    pts += [(px, py) for px, py in reversed(corner(x1, y1, 1, -1, False))]  # bottom right
    pts += corner(x, y1, -1, -1, False)                     # bottom left
    pts += [(px, py) for px, py in reversed(corner(x, y, -1, 1, False))]   # top left
    return pts


def mask(size):
    """The tile's mask on a size x size canvas (anti-aliased)."""
    ss = 4
    S = size * ss
    f = S / CANVAS
    m = Image.new("L", (S, S), 0)
    ImageDraw.Draw(m).polygon(outline(INSET * f, INSET * f, BODY * f, BODY * f, RADIUS * f), fill=255)
    return m.resize((size, size), Image.LANCZOS)


def finish(art):
    """Clips `art` (RGBA, any square size) to the tile and adds Apple's shadow."""
    n = art.width
    m = mask(n)
    body = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    body.paste(art, (0, 0), m)
    f = n / CANVAS
    dx, dy, blur, op = SHADOW
    sh = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    sh.paste((0, 0, 0, int(255 * op)), (round(dx * f), round(dy * f)), m)
    sh = sh.filter(ImageFilter.GaussianBlur(blur * f / 2))
    sh.alpha_composite(body)
    return sh


def retile(src):
    """An existing icon's artwork on Apple's tile at its own scale: its tile maps onto Apple's body
    exactly (nothing is cropped), and where Apple's corners reach past the old tile's, the artwork's
    own edge colours are carried out to fill them."""
    im = Image.open(src).convert("RGBA")
    if im.size != (CANVAS, CANVAS):
        im = im.resize((CANVAS, CANVAS), Image.LANCZOS)
    solid = im.split()[3].point(lambda v: 255 if v > 245 else 0)
    x0, y0, x1, y1 = solid.getbbox()
    art = im.crop((x0, y0, x1, y1))
    art_mask = solid.crop((x0, y0, x1, y1))
    w, h = BODY, BODY
    art = art.resize((w, h), Image.LANCZOS)
    art_mask = art_mask.resize((w, h), Image.LANCZOS)
    # the fill: the artwork's edge colours smeared outward (repeatedly grown and blurred)
    fill = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    fill.paste(art, (0, 0), art_mask)
    grown = fill
    for r in (2, 4, 8, 16, 32):
        g = grown.filter(ImageFilter.GaussianBlur(r))
        # un-premultiply the blur so colours keep their strength where alpha thins
        rgb = Image.new("RGBA", (w, h))
        px = []
        for (cr, cg, cb, ca) in g.getdata():
            px.append((min(255, cr * 255 // ca), min(255, cg * 255 // ca), min(255, cb * 255 // ca), 255) if ca else (0, 0, 0, 0))
        rgb.putdata(px)
        under = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        under.paste(rgb, (0, 0), g.split()[3].point(lambda v: 255 if v > 8 else 0))
        under.alpha_composite(grown)
        grown = under
    body = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    body.paste(grown, (0, 0))
    body.alpha_composite(Image.composite(art, Image.new("RGBA", (w, h), (0, 0, 0, 0)), art_mask))
    canvas = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    canvas.paste(body, (INSET, INSET))
    return finish(canvas)


def write_sizes(big, out, name):
    os.makedirs(out, exist_ok=True)
    for n in SIZES:
        (big if n == CANVAS else big.resize((n, n), Image.LANCZOS)).save(f"{out}/{name}-{n}.png")


def main():
    mode = sys.argv[1]
    if mode == "retile":
        write_sizes(retile(sys.argv[2]), sys.argv[3], sys.argv[4])
    elif mode == "preview":
        icons = [Image.open(p).convert("RGBA") for p in sys.argv[3:]]
        sheet = Image.new("RGBA", (len(icons) * 330 + 10, 430), (236, 236, 238, 255))
        dark = Image.new("RGBA", (len(icons) * 330 + 10, 120), (40, 34, 48, 255))
        for i, ic in enumerate(icons):
            sheet.alpha_composite(ic.resize((320, 320), Image.LANCZOS), (10 + i * 330, 10))
            sheet.alpha_composite(ic.resize((64, 64), Image.LANCZOS), (10 + i * 330, 340))
            sheet.alpha_composite(ic.resize((32, 32), Image.LANCZOS), (90 + i * 330, 356))
            dark.alpha_composite(ic.resize((96, 96), Image.LANCZOS), (10 + i * 330, 12))
        out = Image.new("RGBA", (sheet.width, sheet.height + dark.height))
        out.paste(sheet, (0, 0))
        out.paste(dark, (0, sheet.height))
        out.save(sys.argv[2])


if __name__ == "__main__":
    main()
