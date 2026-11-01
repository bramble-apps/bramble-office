"""Values' Android adaptive icon from its SVG (numbers-linux/crates/nm-app/data/icons/numdeck.svg):
the calculator and the pencil as the foreground, the ledger page drawn again edge to edge (no
rounded square) as the background, so the launcher can crop it to any shape. The pencil comes in
from the corner past the icon's edge, as on the flat icon; the calculator stays inside the 66 dp
safe circle. usage: adaptive_icon.py <numdeck.svg> <android res dir> [preview.png]"""
import io, os, re, sys

import gi

gi.require_version("Rsvg", "2.0")
from gi.repository import Rsvg  # noqa: E402
from PIL import Image, ImageDraw  # noqa: E402

svg = open(sys.argv[1]).read()
res = sys.argv[2]
head = svg[: svg.index("</defs>") + len("</defs>")]
body = svg[len(head):]
# the parts of the body: the ledger page, the pencil and the calculator (inside the rounded square's clip)
pencil_at = body.index('<g transform="translate(-12 141)')
clip_open = re.match(r'\s*<g clip-path="url\(#sq\)">', body)
paper = body[clip_open.end():pencil_at]
objects = body[pencil_at:body.index('<rect width="128" height="128" fill="url(#vign)"/>')]


def svg_with(view, content):
    """The original's defs with `content`, its viewBox `view` (x, y, size)."""
    x, y, s = view
    h = head.replace('width="128" height="128" viewBox="0 0 128 128"', f'width="{s}" height="{s}" viewBox="{x} {y} {s} {s}"', 1)
    return h + content + "</svg>"


def render(text, px):
    """`text` drawn `px` pixels square (librsvg, at the SVG's own size: set to px)."""
    text = re.sub(r'(<svg[^>]*?)width="[^"]*" height="[^"]*"', lambda m: f'{m.group(1)}width="{px}" height="{px}"', text, count=1)
    pb = Rsvg.Handle.new_from_data(text.encode()).get_pixbuf()
    ok, data = pb.save_to_bufferv("png", [], [])
    return Image.open(io.BytesIO(data)).convert("RGBA").resize((px, px), Image.LANCZOS)


# the page edge to edge: the 128 square without its corners rounded, its top band kept
BG_VIEW = (4, 4, 120)
# the objects: the 108 dp layer is 172 units about the calculator and pencil's middle (a little
# right of and below the icon's), so the calculator's corners land inside the 33 dp safe circle
FG_VIEW = (70 - 86, 72 - 86, 172)

for name, dp in [("mdpi", 1), ("hdpi", 1.5), ("xhdpi", 2), ("xxhdpi", 3), ("xxxhdpi", 4)]:
    n = round(108 * dp)
    out = f"{res}/mipmap-{name}"
    os.makedirs(out, exist_ok=True)
    render(svg_with(BG_VIEW, paper), n).convert("RGB").save(f"{out}/ic_launcher_background.png")
    render(svg_with(FG_VIEW, objects), n).save(f"{out}/ic_launcher_foreground.png")
    # the flat icon for launchers before Android 8
    render(svg, round(48 * dp)).save(f"{out}/ic_launcher.png")
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
