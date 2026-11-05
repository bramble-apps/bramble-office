"""Snap Store packaging for Keydeck, Letters, Values and Bramble Notes: one folder per snap with its
snap/snapcraft.yaml (built from a bramble-office checkout) and its .desktop file and icon.

  make_snaps.py <out dir> <bramble-office dir>   (builds the pushed commit that checkout is at)

Build one: cd <out>/<snap> && snapcraft pack --destructive-mode   (on Ubuntu 26.04, base core26)
"""
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SITE = "https://bramble-apps.github.io"
SOURCE = "https://github.com/bramble-apps/bramble-office"
VERSION = "0.4.1"

# (common plugs: windows, files in home and on USB drives, the network; media apps also sound)
APPS = [
    dict(snap="bramble-keydeck", name="Keydeck", bin="keydeck", ws="keynote-linux", pkg="kn-app",
         icon="keynote-linux/crates/kn-app/data/icons/keydeck-512.png",
         summary="Make and play presentations",
         desc="Keydeck makes and plays slideshows. It opens and saves Keynote files, and opens and exports "
              "PowerPoint presentations.\n\nBuild slides from themes, text, shapes, charts, tables, pictures, "
              "audio and video, add transitions and builds, and present with a presenter display, notes and a timer.",
         categories="Office;Presentation;",
         mime="application/x-iwork-keynote-sffkey;application/vnd.apple.keynote;application/vnd.openxmlformats-officedocument.presentationml.presentation;",
         media=True),
    dict(snap="bramble-letters", name="Letters", bin="letters", ws="pages-linux", pkg="pg-app",
         icon="pages-linux/crates/pg-app/data/icons/letters-512.png",
         summary="Write documents and lay out pages",
         desc="Letters is a word processor and page layout app. It opens and saves Pages files, and opens and "
              "saves Word documents, RTF and Markdown.\n\nWrite with styles, lists, tables, charts, footnotes "
              "and tracked changes, or lay out flyers, newsletters and posters from templates.",
         categories="Office;WordProcessor;",
         mime="application/x-iwork-pages-sffpages;application/vnd.apple.pages;application/vnd.openxmlformats-officedocument.wordprocessingml.document;application/rtf;text/markdown;text/plain;",
         media=True),
    dict(snap="bramble-values", name="Values", bin="values", ws="numbers-linux", pkg="nm-app",
         icon="numbers-linux/crates/nm-app/data/icons/numdeck-512.png",
         summary="Work with spreadsheets",
         desc="Values is a spreadsheet app made of sheets with free-standing tables, charts, text and pictures. "
              "It opens and saves Numbers files, and opens and saves Excel workbooks and CSV.\n\nUse hundreds of "
              "functions, categories, filters, sorting, conditional highlighting and interactive charts.",
         categories="Office;Spreadsheet;",
         mime="application/x-iwork-numbers-sffnumbers;application/vnd.apple.numbers;text/csv;application/vnd.openxmlformats-officedocument.spreadsheetml.sheet;",
         media=True),
    dict(snap="bramble-notes", name="Bramble Notes", bin="bramble-notes", ws="notes-linux", pkg="nt-app",
         icon="notes-linux/crates/nt-app/data/icons/notes-512.png",
         summary="Write notes, checklists and tables",
         desc="Bramble Notes keeps notes with formatting, checklists, tables, drawings, pictures, PDFs and audio, "
              "in folders, Smart Folders and tags, with search across them all.\n\nIt can import a library from "
              "Apple Notes or UpNote, lock notes with a password, and sync between your devices through your "
              "own WebDAV storage, such as Nextcloud or Koofr.",
         categories="Office;",
         mime="",
         media=True),
]


def yaml(a, src):
    COMMIT = __import__('subprocess').run(['git', '-C', src, 'rev-parse', 'HEAD'], capture_output=True, text=True, check=True).stdout.strip()
    plugs = ["home", "removable-media", "network"]
    if a["media"]:
        plugs += ["audio-playback", "audio-record", "cups"]
    desc = "\n".join("  " + l if l else "" for l in a["desc"].split("\n"))
    stage = ["ffmpeg", "poppler-utils", "curl"] if a["media"] else ["curl"]
    return f"""name: {a['snap']}
title: {a['name']}
version: '{VERSION}'
summary: {a['summary']}
description: |
{desc}
website: {SITE}
source-code: {SOURCE}
issues: {SOURCE}/issues
contact: {SOURCE}/issues
license: GPL-3.0-only
base: core26
grade: stable
confinement: strict

apps:
  {a['snap']}:
    command: bin/{a['bin']}
    desktop: usr/share/applications/{a['snap']}.desktop
    extensions: [gnome]
    plugs: [{', '.join(plugs)}]

parts:
  {a['snap']}:
    plugin: nil
    source: {SOURCE}.git
    source-commit: '{COMMIT}'
    override-build: |
      rustup default stable
      cd {a['ws']}
      cargo build --release -p {a['pkg']}
      install -Dm755 target/release/{a['bin']} $CRAFT_PART_INSTALL/bin/{a['bin']}
    build-snaps: [rustup]
    build-packages: [pkg-config, build-essential, cmake, libasound2-dev, libdbus-1-dev, libwayland-dev, libxkbcommon-dev, libfontconfig-dev, libssl-dev]
    stage-packages: [{', '.join(stage)}]
  desktop:
    plugin: dump
    source: gui
    organize:
      {a['snap']}.desktop: usr/share/applications/{a['snap']}.desktop
"""


def desktop(a):
    out = [
        "[Desktop Entry]",
        "Type=Application",
        f"Name={a['name']}",
        f"Comment={a['summary']}",
        f"Exec={a['bin']} %f" if a["mime"] else f"Exec={a['bin']}",
        "Icon=${SNAP}/meta/gui/icon.png",
        "Terminal=false",
        f"Categories={a['categories']}",
    ]
    if a["mime"]:
        out.append(f"MimeType={a['mime']}")
    return "\n".join(out) + "\n"


def main():
    out, src = sys.argv[1], os.path.abspath(sys.argv[2])
    for a in APPS:
        d = os.path.join(out, a["snap"])
        os.makedirs(os.path.join(d, "snap", "gui"), exist_ok=True)
        os.makedirs(os.path.join(d, "gui"), exist_ok=True)
        with open(os.path.join(d, "snap", "snapcraft.yaml"), "w") as f:
            f.write(yaml(a, src))
        with open(os.path.join(d, "gui", f"{a['snap']}.desktop"), "w") as f:
            f.write(desktop(a))
        shutil.copy(os.path.join(src, a["icon"]), os.path.join(d, "snap", "gui", "icon.png"))
    print(f"{out}: {len(APPS)} snaps")


if __name__ == "__main__":
    main()
