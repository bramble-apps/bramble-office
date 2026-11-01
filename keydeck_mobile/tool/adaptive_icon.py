"""Keydeck's Android adaptive icon from its SVG (keynote-linux/crates/kn-app/data/icons/keydeck.svg):
the slide printout as the foreground, and the lectern (the desk, the near ledge and the body
below) drawn again edge to edge as the background, so the launcher can crop it to any shape.
Both layers keep the flat icon's drawing 1:1 (the tile's 112 units are the 72 dp a launcher shows).
usage: adaptive_icon.py <keydeck.svg> <android res dir> [preview.png]"""
import os, re, sys

import gi
gi.require_version("Rsvg", "2.0")
from gi.repository import Rsvg
from PIL import Image, ImageDraw

svg = open(sys.argv[1]).read()
res = sys.argv[2]

# the 108 dp layer: the 72 dp a launcher shows is the tile (8..120), so 168 units round its centre
VIEW = 'viewBox="-20 -20 168 168"'

head, body = svg.split("</defs>", 1)
head += "</defs>"
head = head.replace('width="128" height="128" viewBox="0 0 128 128"', 'width="168" height="168" ' + VIEW, 1)
# no tile: its outline paths (before and after the scene) and its clip go
body = re.sub(r'\n  <path d="M46\.52[^\n]*\n', "\n", body)
body = body.replace('<g clip-path="url(#sq)">', "<g>", 1)
paper_start = body.index('    <g transform="translate(63 86) scale(1.16)')
paper_end = body.index("    </g>\n\n    <!-- near ledge")
paper = body[paper_start:paper_end + len("    </g>\n")]
scene = body[:paper_start] + body[paper_end + len("    </g>\n"):]

# the background: the lectern wider and taller, to the layer's edges
bg = scene
bg = bg.replace('<rect width="128" height="92"', '<rect x="-30" y="-30" width="188" height="122"')
bg = re.sub(r"M-10 (-?[\d.]+) H138", r"M-50 \1 H178", bg)
bg = bg.replace('<rect y="86" width="128" height="16"', '<rect x="-30" y="86" width="188" height="16"')
bg = re.sub(r"M0 ([\d.]+) H128", r"M-30 \1 H158", bg)
bg = bg.replace('<rect y="102" width="128" height="26"', '<rect x="-30" y="102" width="188" height="60"')
bg = bg.replace('<rect y="102" width="128" height="6"', '<rect x="-30" y="102" width="188" height="6"')
bg = bg.replace('<rect x="22" y="110" width="84" height="22"', '<rect x="22" y="110" width="84" height="44"')
bg = bg.replace("V128 M105.5 111 V128", "V150 M105.5 111 V150")
bg_head = head.replace('<rect y="86" width="128" height="16"/>', '<rect x="-30" y="86" width="188" height="16"/>').replace('<rect y="102" width="128" height="26"/>', '<rect x="-30" y="102" width="188" height="60"/>')
bg_svg = bg_head + bg

# the foreground: the papers alone, cut where the ledge hides them
fg_head = head.replace("</defs>", '  <clipPath id="aboveledge"><rect x="-20" y="-20" width="168" height="106"/></clipPath>\n  </defs>')
# (at the flat icon's own size, not its 1.16: inside the 66 dp circle no launcher shape cuts)
paper = paper.replace("scale(1.16)", "scale(1.0)", 1)
fg_svg = fg_head + '\n  <g clip-path="url(#aboveledge)">\n' + paper + "  </g>\n</svg>\n"


def render(text, n, path):
    # (librsvg draws it at the size the root element gives)
    text = text.replace('width="168" height="168"', f'width="{n}" height="{n}"', 1)
    Rsvg.Handle.new_from_data(text.encode()).get_pixbuf().savev(path, "png", [], [])


flat = sys.argv[1].replace(".svg", "-1024.png")
for name, dp in [("mdpi", 1), ("hdpi", 1.5), ("xhdpi", 2), ("xxhdpi", 3), ("xxxhdpi", 4)]:
    n = round(108 * dp)
    out = f"{res}/mipmap-{name}"
    os.makedirs(out, exist_ok=True)
    render(bg_svg, n, f"{out}/ic_launcher_background.png")
    render(fg_svg, n, f"{out}/ic_launcher_foreground.png")
    # the flat icon for launchers before Android 8
    Image.open(flat).convert("RGBA").resize((round(48 * dp),) * 2, Image.LANCZOS).save(f"{out}/ic_launcher.png")
os.makedirs(f"{res}/mipmap-anydpi-v26", exist_ok=True)
open(f"{res}/mipmap-anydpi-v26/ic_launcher.xml", "w").write("""<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@mipmap/ic_launcher_background" />
    <foreground android:drawable="@mipmap/ic_launcher_foreground" />
</adaptive-icon>
""")
# a preview: the shapes launchers cut
prev = Image.new("RGB", (4 * 230, 230), (30, 30, 30))
bgi = Image.open(f"{res}/mipmap-xxxhdpi/ic_launcher_background.png").convert("RGBA")
bgi.alpha_composite(Image.open(f"{res}/mipmap-xxxhdpi/ic_launcher_foreground.png").convert("RGBA"))
full = bgi.crop((round(432 * 18 / 108),) * 2 + (round(432 * 90 / 108),) * 2).resize((200, 200), Image.LANCZOS)
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
