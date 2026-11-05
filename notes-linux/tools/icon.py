"""Notes' app icon, drawn with PIL: a yellow legal pad in GNOME's (Adwaita's) style on the Bramble
suite's Apple tile. Ruled yellow paper with a double red margin, the torn-off sheets' stubs under a
dark slate cap, the cap with a lighter band along its top and a darker lip with a light hairline.

  icon.py desktop <dir>          hicolor sizes notes-16 … notes-1024.png (and notes.ico)
  icon.py android <res dir>      the Android launcher icon (adaptive layers + the flat icon)
  icon.py preview <png>          the 1024 px icon beside Letters', Values' and Calculator's
"""
import os
import random
import sys

from PIL import Image, ImageChops, ImageDraw, ImageFilter

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../keynote-linux/tools/suite"))
import apple_tile  # noqa: E402

PAPER_TOP, PAPER_FOOT = (248, 228, 92), (246, 211, 45)
RULE, MARGIN = (230, 180, 40), (246, 97, 81)
CAP_TOP, CAP_FOOT = (58, 54, 66), (30, 26, 40)
CAP_BAND, CAP_LIP, CAP_HAIRLINE = (72, 68, 82), (18, 15, 26), (70, 66, 80)
STUB, STUB_RIM, STUB_SHADOW = (251, 236, 130), (255, 250, 205), (196, 140, 0)


class Pad:
    """The pad drawn on an n x n canvas. The pad's frame is the square (fx, fy, fx + H, fy + H): the
    tile's body on the desktop, the launcher's visible square on Android. Everything is measured in
    `u`, the canvas pixels per unit of Apple's 1024 grid (the frame is 824 units)."""

    def __init__(self, n, frame):
        self.n = n
        self.fx, self.fy, self.H = frame
        self.u = self.H / apple_tile.BODY

    def blank(self):
        return Image.new("L", (self.n, self.n), 0)

    def rect(self, box):
        m = self.blank()
        ImageDraw.Draw(m).rectangle(box, fill=255)
        return m

    def vgrad(self, y0, y1, top, bot):
        n = self.n
        g = Image.linear_gradient("L").resize((n, max(int(y1 - y0), 1)))
        band = Image.composite(Image.new("RGBA", g.size, bot + (255,)), Image.new("RGBA", g.size, top + (255,)), g)
        img = Image.new("RGBA", (n, n), top + (255,))
        img.paste(band, (0, int(y0)))
        img.paste(Image.new("RGBA", (n, max(n - int(y1), 0)), bot + (255,)), (0, int(y1)))
        return img

    def paint(self, img, m, fill, clip=None):
        if clip is not None:
            m = ImageChops.multiply(m, clip)
        if isinstance(fill, tuple):
            fill = Image.new("RGBA", (self.n, self.n), fill + (255,))
        # (not paste with a mask: that blends the colour toward the empty layer's black at soft edges)
        layer = fill.copy()
        layer.putalpha(ImageChops.multiply(fill.split()[3], m))
        img.alpha_composite(layer)

    def shadow(self, img, m, blur, dy, alpha, colour, clip=None):
        sh = self.blank()
        sh.paste(m, (0, int(dy)))
        sh = sh.filter(ImageFilter.GaussianBlur(blur)).point(lambda v: v * alpha // 255)
        self.paint(img, sh, colour, clip)

    def tear(self, base, depth, seed):
        """A natural tear line across the canvas: a slow wander, mid-size bites and fine fibres."""
        rnd = random.Random(seed)
        n = max(8, round(230 * self.n / self.H))  # the same fibre density at any frame size

        def smooth(k, passes):
            v = [rnd.uniform(-1, 1) for _ in range(n + 1)]
            for _ in range(passes):
                v = [(v[max(i - k, 0)] + v[i] + v[min(i + k, n)]) / 3 for i in range(n + 1)]
            return v

        slow, mid = smooth(18, 6), smooth(1, 2)
        fine = [rnd.uniform(-1, 1) for _ in range(n + 1)]
        pts = []
        for i in range(n + 1):
            d = depth * (1.0 + 0.9 * slow[i] + 0.55 * mid[i] + 0.12 * fine[i])
            pts.append((-4 + (self.n + 8) * i / n, base + max(depth * 0.12, d)))
        return pts

    def draw(self, sheet, top_band=True):
        """The pad on the canvas, clipped to `sheet` (the tile's mask, or the whole canvas)."""
        u, H, n = self.u, self.H, self.n
        img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
        bind = self.fy + H * 0.27

        # ruled yellow paper and the double red margin
        self.paint(img, sheet, self.vgrad(bind, self.fy + H, PAPER_TOP, PAPER_FOOT))
        lines = self.blank()
        g = ImageDraw.Draw(lines)
        y = bind + H * 0.11
        while y < n:
            g.rectangle([0, y - 3.2 * u, n, y + 3.2 * u], fill=255)
            y += H * 0.088
        self.paint(img, lines, RULE, sheet)
        mx = self.fx + H * 0.17
        margin = self.blank()
        for x in (mx, mx + 14 * u):
            ImageDraw.Draw(margin).rectangle([x - 3.4 * u, bind, x + 3.4 * u, n], fill=255)
        self.paint(img, margin, MARGIN, sheet)

        # the torn-off sheets' stubs: a soft torn edge, a warm shadow, a pale rim of fibres
        stubs = self.blank()
        ImageDraw.Draw(stubs).polygon([(-4, bind)] + self.tear(bind, H * 0.024, 11) + [(n + 4, bind)], fill=255)
        stubs = stubs.filter(ImageFilter.GaussianBlur(1.2 * u)).point(lambda v: max(0, min(255, (v - 128) * 3 + 128)))
        stubs = ImageChops.multiply(stubs, sheet)
        self.shadow(img, stubs, 7 * u, 6 * u, 70, STUB_SHADOW, sheet)
        self.paint(img, stubs, STUB)
        rim = ImageChops.subtract(stubs, ImageChops.offset(stubs, 0, -int(5 * u)))
        self.paint(img, rim.filter(ImageFilter.GaussianBlur(2.2 * u)).point(lambda v: v * 170 // 255), STUB_RIM)

        # the cap: dark slate, its shadow on the paper, a lighter band along its top, a darker lip
        cap = ImageChops.multiply(sheet, self.rect([0, 0, n, bind]))
        self.shadow(img, cap, 5 * u, 6 * u, 80, (0, 0, 0), sheet)
        self.paint(img, cap, self.vgrad(self.fy, bind, CAP_TOP, CAP_FOOT))
        lip = H * 0.035
        self.paint(img, self.rect([0, bind - lip, n, bind]), CAP_LIP, cap)
        self.paint(img, self.rect([0, bind - lip - 3 * u, n, bind - lip]), CAP_HAIRLINE, cap)
        if top_band:
            # (it follows the tile's curved top, as thick as the lip)
            band = ImageChops.subtract(cap, ImageChops.offset(cap, 0, int(lip)))
            self.paint(img, ImageChops.multiply(band, cap), CAP_BAND)
        return img


def desktop(n):
    """The icon at n px (drawn at 2x and reduced)."""
    S = n * 2
    k = S / apple_tile.CANVAS
    pad = Pad(S, (apple_tile.INSET * k, apple_tile.INSET * k, apple_tile.BODY * k))
    return apple_tile.finish(pad.draw(apple_tile.mask(S))).resize((n, n), Image.LANCZOS)


def android(res):
    """Android's adaptive icon: the pad drawn edge to edge as the background layer (its 108 dp layer
    shows the launcher's 72 dp square, the pad's frame, with 18 dp of bleed around it); the
    foreground is empty. The flat icon for launchers before Android 8 is the desktop icon."""
    flat = desktop(1024)
    for name, dp in [("mdpi", 1), ("hdpi", 1.5), ("xhdpi", 2), ("xxhdpi", 3), ("xxxhdpi", 4)]:
        n = round(108 * dp)
        S = n * 2
        pad = Pad(S, (S * 18 / 108, S * 18 / 108, S * 72 / 108))
        bg = pad.draw(Image.new("L", (S, S), 255), top_band=False).resize((n, n), Image.LANCZOS)
        out = f"{res}/mipmap-{name}"
        os.makedirs(out, exist_ok=True)
        bg.convert("RGB").save(f"{out}/ic_launcher_background.png")
        Image.new("RGBA", (n, n), (0, 0, 0, 0)).save(f"{out}/ic_launcher_foreground.png")
        flat.resize((round(48 * dp),) * 2, Image.LANCZOS).save(f"{out}/ic_launcher.png")


def main():
    mode, out = sys.argv[1], sys.argv[2]
    if mode == "desktop":
        os.makedirs(out, exist_ok=True)
        big = desktop(1024)
        for n in (16, 24, 32, 48, 64, 128, 256, 512, 1024):
            (big if n == 1024 else big.resize((n, n), Image.LANCZOS)).save(f"{out}/notes-{n}.png")
        big.save(f"{out}/notes.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
    elif mode == "android":
        android(out)
    elif mode == "preview":
        home = os.path.join(os.path.dirname(os.path.abspath(__file__)), "../..")
        others = [f"{home}/pages-linux/crates/pg-app/data/icons/letters-1024.png", f"{home}/numbers-linux/crates/nm-app/data/icons/numdeck-1024.png", f"{home}/calculator/crates/calc-app/assets/icon/calculator-1024.png"]
        icons = [desktop(1024)] + [Image.open(p).convert("RGBA") for p in others]
        sheet = Image.new("RGBA", (len(icons) * 520 + 10, 640), (240, 240, 240, 255))
        for i, ic in enumerate(icons):
            sheet.alpha_composite(ic.resize((500, 500), Image.LANCZOS), (10 + i * 520, 10))
        x = 10
        for s in [desktop(n) for n in (16, 32, 48, 64)]:
            sheet.alpha_composite(s, (x, 560))
            x += s.width + 20
        sheet.save(out)


if __name__ == "__main__":
    main()
