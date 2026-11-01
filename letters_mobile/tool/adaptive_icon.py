"""Letters' Android adaptive icon from its flat icon: the paper, ink and pen as the foreground
(cut out of the cutting mat), and the mat drawn again edge to edge as the background, so the
launcher can crop it to any shape. usage: adaptive_icon.py <letters-1024.png> <android res dir>"""
import colorsys, sys
from PIL import Image, ImageChops, ImageDraw, ImageFilter

src = Image.open(sys.argv[1]).convert("RGBA")
res = sys.argv[2]
W = src.width

# what is mat: dark grey, or its orange rule lines (low-ish lightness)
mask = Image.new("L", src.size, 0)
px, mp = src.load(), mask.load()
for y in range(W):
    for x in range(W):
        r, g, b, a = px[x, y]
        if a < 200:
            continue
        h, l, s = colorsys.rgb_to_hls(r / 255, g / 255, b / 255)
        grey = l < 0.42 and s < 0.22
        orange = 0.05 < h < 0.14 and s > 0.3 and l < 0.62
        mp[x, y] = 0 if (grey or orange) else 255
# clean up: drop specks, fill holes, smooth the edge
mask = mask.filter(ImageFilter.MedianFilter(7)).filter(ImageFilter.MaxFilter(5)).filter(ImageFilter.MinFilter(5))
holes = mask.copy()
ImageDraw.floodfill(holes, (0, 0), 128)
mask = Image.eval(holes, lambda v: 0 if v == 128 else 255)
mask = mask.filter(ImageFilter.GaussianBlur(1.2))
fg = src.copy()
fg.putalpha(ImageChops.multiply(mask, src.split()[3]))
fg = fg.crop(fg.getbbox())

def mat(n):
    """The cutting mat, edge to edge: dark grey, a fine orange grid, every fourth line bolder,
    and the two diagonals."""
    m = Image.new("RGB", (n, n), (62, 66, 67))
    d = ImageDraw.Draw(m, "RGBA")
    step = n / 18
    for i in range(19):
        v = round(i * step)
        w = max(1, round(n / 300)) if i % 4 else max(2, round(n / 170))
        a = 150 if i % 4 else 210
        d.line([(v, 0), (v, n)], fill=(222, 150, 52, a), width=w)
        d.line([(0, v), (n, v)], fill=(222, 150, 52, a), width=w)
    d.line([(0, 0), (n, n)], fill=(222, 150, 52, 110), width=max(1, round(n / 300)))
    d.line([(0, n), (n, 0)], fill=(222, 150, 52, 110), width=max(1, round(n / 300)))
    # a soft light from the top left, as on the tile
    shade = Image.linear_gradient("L").resize((n, n)).rotate(45, expand=False)
    return Image.composite(m, Image.new("RGB", (n, n), (40, 43, 44)), shade.point(lambda v: 255 - v // 3))

for name, dp in [("mdpi", 1), ("hdpi", 1.5), ("xhdpi", 2), ("xxhdpi", 3), ("xxxhdpi", 4)]:
    n = round(108 * dp)
    out = f"{res}/mipmap-{name}"
    import os
    os.makedirs(out, exist_ok=True)
    mat(n).save(f"{out}/ic_launcher_background.png")
    # the objects inside the 66 dp safe circle (no launcher shape cuts them)
    canvas = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    box = round(56 * dp)
    f = fg.copy()
    f.thumbnail((box, box), Image.LANCZOS)
    canvas.alpha_composite(f, ((n - f.width) // 2, (n - f.height) // 2))
    canvas.save(f"{out}/ic_launcher_foreground.png")
    # the flat icon for launchers before Android 8
    src.resize((round(48 * dp),) * 2, Image.LANCZOS).save(f"{out}/ic_launcher.png")
os.makedirs(f"{res}/mipmap-anydpi-v26", exist_ok=True)
open(f"{res}/mipmap-anydpi-v26/ic_launcher.xml", "w").write("""<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@mipmap/ic_launcher_background" />
    <foreground android:drawable="@mipmap/ic_launcher_foreground" />
</adaptive-icon>
""")
# a preview: the shapes launchers cut
prev = Image.new("RGB", (4 * 230, 230), (30, 30, 30))
bg = Image.open(f"{res}/mipmap-xxxhdpi/ic_launcher_background.png").convert("RGBA")
bg.alpha_composite(Image.open(f"{res}/mipmap-xxxhdpi/ic_launcher_foreground.png"))
full = bg.crop((round(432 * 18 / 108),) * 2 + (round(432 * 90 / 108),) * 2).resize((200, 200), Image.LANCZOS)
for k, shape in enumerate(["circle", "squircle", "rounded", "teardrop"]):
    m = Image.new("L", (200, 200), 0)
    d = ImageDraw.Draw(m)
    if shape == "circle":
        d.ellipse((0, 0, 199, 199), fill=255)
    elif shape == "squircle":
        d.rounded_rectangle((0, 0, 199, 199), 70, fill=255)
    elif shape == "rounded":
        d.rounded_rectangle((0, 0, 199, 199), 36, fill=255)
    else:
        d.rounded_rectangle((0, 0, 199, 199), 100, fill=255)
        d.rectangle((100, 100, 199, 199), fill=255)
    prev.paste(full, (15 + k * 230, 15), m)
prev.save(sys.argv[3] if len(sys.argv) > 3 else "/tmp/icon-preview.png")
