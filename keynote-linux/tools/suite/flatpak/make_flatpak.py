"""Flathub packaging for the five Bramble apps: for each, a flatpak-builder manifest, an AppStream
metainfo file and a .desktop file, plus the crates it builds from (cargo_sources.py). The manifests
build from the public bramble-office repo at COMMIT; export.sh copies this folder there as flatpak/.

  make_flatpak.py meta <dir>                      the .desktop and metainfo files (kept here)
  make_flatpak.py submit <out dir> <commit>       one Flathub submission folder per app

Each app also needs its Flathub submission (github.com/flathub/flathub, one pull request per app):
see README.md here.
"""
import os
import subprocess
import sys
from html import escape

HERE = os.path.dirname(os.path.abspath(__file__))
P = os.path.abspath(os.path.join(HERE, "../../../.."))  # (the folder holding the app repos)
ORG = "io.github.bramble_apps"
REPO = "https://github.com/bramble-apps/bramble-office.git"
SITE = "https://bramble-apps.github.io"
ISSUES = "https://github.com/bramble-apps/bramble-office/issues"
DONATE = "https://www.paypal.com/donate/?hosted_button_id=5EP8EM86U9P7Q"
RUNTIME = "26.08"
VERSION, DATE = "0.4.0", "2026-11-04"

# (the external programs the apps call that a Flatpak has to bring: ffmpeg for audio and video,
# poppler's pdftoppm / pdfinfo for PDF pages; curl, xdg-open, gsettings come with the runtime)
MEDIA = ["ffmpeg.json", "poppler.json"]

APPS = [
    dict(
        id="Keydeck", name="Keydeck", bin="keydeck", ws="keynote-linux", pkg="kn-app",
        icons="keynote-linux/crates/kn-app/data/icons/keydeck-{s}.png",
        summary="Make and play presentations",
        categories="Office;Presentation;",
        mime="application/x-iwork-keynote-sffkey;application/vnd.apple.keynote;application/vnd.openxmlformats-officedocument.presentationml.presentation;",
        keywords="slides;presentation;keynote;powerpoint;pptx;",
        desc=[
            "Keydeck makes and plays slideshows. It opens and saves Keynote files, and opens and "
            "exports PowerPoint presentations.",
            "Build slides from themes, text, shapes, charts, tables, pictures, audio and video, "
            "add transitions and builds, and present with a presenter display, notes and a timer.",
        ],
        modules=MEDIA, network=True, audio=True, cups=False,
    ),
    dict(
        id="Letters", name="Letters", bin="letters", ws="pages-linux", pkg="pg-app",
        icons="pages-linux/crates/pg-app/data/icons/letters-{s}.png",
        summary="Write documents and lay out pages",
        categories="Office;WordProcessor;",
        mime="application/x-iwork-pages-sffpages;application/vnd.apple.pages;application/vnd.openxmlformats-officedocument.wordprocessingml.document;application/rtf;text/markdown;text/plain;",
        keywords="word;document;writer;pages;docx;layout;",
        desc=[
            "Letters is a word processor and page layout app. It opens and saves Pages files, and "
            "opens and saves Word documents, RTF and Markdown.",
            "Write with styles, lists, tables, charts, footnotes and tracked changes, or lay out "
            "flyers, newsletters and posters from templates with text boxes, shapes and pictures.",
        ],
        modules=MEDIA, network=True, audio=True, cups=True,
    ),
    dict(
        id="Values", name="Values", bin="values", ws="numbers-linux", pkg="nm-app",
        icons="numbers-linux/crates/nm-app/data/icons/numdeck-{s}.png",
        summary="Work with spreadsheets",
        categories="Office;Spreadsheet;",
        mime="application/x-iwork-numbers-sffnumbers;application/vnd.apple.numbers;text/csv;application/vnd.openxmlformats-officedocument.spreadsheetml.sheet;",
        keywords="spreadsheet;numbers;excel;xlsx;csv;table;chart;",
        desc=[
            "Values is a spreadsheet app made of sheets with free-standing tables, charts, text "
            "and pictures. It opens and saves Numbers files, and opens and saves Excel workbooks "
            "and CSV.",
            "Use hundreds of functions, categories, filters, sorting, conditional highlighting "
            "and interactive charts, starting from a blank sheet or a template.",
        ],
        modules=MEDIA, network=True, audio=True, cups=True,
    ),
    dict(
        id="Calculator", name="Bramble Calculator", bin="calculator", ws="calculator", pkg="calc-app",
        icons="calculator/crates/calc-app/assets/icon/calculator-{s}.png",
        summary="Do basic, scientific and programmer arithmetic",
        categories="Utility;Calculator;",
        mime="",
        keywords="calculator;math;scientific;programmer;convert;currency;",
        desc=[
            "A calculator with basic, scientific and programmer modes, a paper tape of past "
            "results and unit and currency conversion.",
        ],
        modules=[], network=True, audio=False, cups=False,
        submit=False,  # (not on Flathub for now: the user's call)
    ),
    dict(
        id="Notes", name="Bramble Notes", bin="bramble-notes", ws="notes-linux", pkg="nt-app",
        icons="notes-linux/crates/nt-app/data/icons/notes-{s}.png",
        summary="Write notes, checklists and tables",
        categories="Office;",
        mime="",
        keywords="notes;notepad;checklist;todo;sync;webdav;",
        desc=[
            "Bramble Notes keeps notes with formatting, checklists, tables, drawings, pictures, "
            "PDFs and audio, in folders, Smart Folders and tags, with search across them all.",
            "It can import a library from Apple Notes on a Mac, lock notes with a password, and "
            "sync between your devices through your own WebDAV storage, such as Nextcloud or "
            "Koofr.",
        ],
        modules=MEDIA, network=True, audio=True, cups=True, fprint=True,
    ),
]

SIZES = (16, 24, 32, 48, 64, 128, 256, 512)

SHOTS = {
    "Keydeck": [("keydeck-3.jpg", "A bold title slide"), ("keydeck-1.jpg", "A title slide on a photo theme"), ("keydeck-2.jpg", "Slides with pictures and text, in a dark theme")],
    "Letters": [("letters-2.jpg", "Styles, pictures and pull quotes"), ("letters-1.jpg", "A newsletter laid out with pictures and columns")],
    "Values": [("values-2.jpg", "A chart beside its table, in dark mode"), ("values-1.jpg", "A sheet with a table and a chart")],
    "Calculator": [("calculator-1.png", "The basic calculator"), ("calculator-2.png", "The scientific calculator"), ("calculator-3.png", "Converting units")],
    "Notes": [("notes-2.jpg", "A checklist in dark mode"), ("notes-1.jpg", "Folders, tags and a note with lists and checklists")],
}


def manifest(a, commit):
    app_id = f"{ORG}.{a['id']}"
    mod = a["bin"].replace("-", "_")
    finish = ["--share=ipc", "--socket=wayland", "--socket=fallback-x11", "--device=dri"]
    if a["network"]:
        finish.append("--share=network")
    if a["audio"]:
        finish.append("--socket=pulseaudio")
    if a["cups"]:
        finish.append("--socket=cups")
    if a.get("fprint"):
        finish.append("--system-talk-name=net.reactivated.Fprint")
    build = [
        f"cd {a['ws']} && cargo --offline build --release -p {a['pkg']}",
        f"install -Dm755 {a['ws']}/target/release/{a['bin']} /app/bin/{a['bin']}",
        f"install -Dm644 flatpak/{app_id}.desktop /app/share/applications/{app_id}.desktop",
        f"install -Dm644 flatpak/{app_id}.metainfo.xml /app/share/metainfo/{app_id}.metainfo.xml",
    ]
    for s in SIZES:
        build.append(f"install -Dm644 {a['icons'].format(s=s)} /app/share/icons/hicolor/{s}x{s}/apps/{app_id}.png")
    lines = [
        f"id: {app_id}",
        "runtime: org.freedesktop.Platform",
        f"runtime-version: '{RUNTIME}'",
        "sdk: org.freedesktop.Sdk",
        "sdk-extensions:",
        "  - org.freedesktop.Sdk.Extension.rust-stable",
        f"command: {a['bin']}",
        "finish-args:",
        *[f"  - {f}" for f in finish],
        "modules:",
        *[f"  - {m}" for m in a["modules"]],
        f"  - name: {mod}",
        "    buildsystem: simple",
        "    build-options:",
        "      append-path: /usr/lib/sdk/rust-stable/bin",
        "      env:",
        f"        CARGO_HOME: /run/build/{mod}/cargo",
        "        CARGO_NET_OFFLINE: 'true'",
        "    build-commands:",
        *[f"      - {c}" for c in build],
        "    sources:",
        "      - type: git",
        f"        url: {REPO}",
        f"        commit: {commit}",
        f"      - {a['ws']}-cargo-sources.json",
    ]
    return "\n".join(lines) + "\n"


def desktop(a):
    app_id = f"{ORG}.{a['id']}"
    out = [
        "[Desktop Entry]",
        "Type=Application",
        f"Name={a['name']}",
        f"Comment={a['summary']}",
        f"Exec={a['bin']} %f" if a["mime"] else f"Exec={a['bin']}",
        f"Icon={app_id}",
        "Terminal=false",
        f"Categories={a['categories']}",
        f"Keywords={a['keywords']}",
        f"StartupWMClass={app_id}",
    ]
    if a["mime"]:
        out.append(f"MimeType={a['mime']}")
    return "\n".join(out) + "\n"


def metainfo(a):
    app_id = f"{ORG}.{a['id']}"
    desc = "\n".join(f"      <p>{escape(p)}</p>" for p in a["desc"])
    # (the store screenshots, on the website: bramble-site/screenshots)
    shots = "\n".join(
        f'    <screenshot{" type=\"default\"" if i == 0 else ""}>\n      <image>{SITE}/screenshots/{f}</image>\n      <caption>{escape(c)}</caption>\n    </screenshot>'
        for i, (f, c) in enumerate(SHOTS[a["id"]])
    )
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>{app_id}</id>
  <name>{escape(a['name'])}</name>
  <summary>{escape(a['summary'])}</summary>
  <metadata_license>CC0-1.0</metadata_license>
  <project_license>GPL-3.0-only</project_license>
  <developer id="io.github.bramble_apps">
    <name>Bramble Office</name>
  </developer>
  <description>
{desc}
  </description>
  <launchable type="desktop-id">{app_id}.desktop</launchable>
  <url type="homepage">{SITE}</url>
  <url type="bugtracker">{ISSUES}</url>
  <url type="vcs-browser">https://github.com/bramble-apps/bramble-office</url>
  <url type="donation">{DONATE}</url>
  <branding>
    <color type="primary" scheme_preference="light">#f6f5f4</color>
    <color type="primary" scheme_preference="dark">#3d3846</color>
  </branding>
  <screenshots>
{shots}
  </screenshots>
  <content_rating type="oars-1.1"/>
  <supports>
    <control>pointing</control>
    <control>keyboard</control>
    <control>touch</control>
  </supports>
  <requires>
    <display_length compare="ge">768</display_length>
  </requires>
  <releases>
    <release version="{VERSION}" date="{DATE}">
      <description>
        <p>First release on Flathub.</p>
      </description>
    </release>
  </releases>
</component>
"""


def write(path, text):
    with open(path, "w") as f:
        f.write(text)


def main():
    mode, out = sys.argv[1], sys.argv[2]
    if mode == "meta":
        # the .desktop and metainfo files, kept here and copied into bramble-office's flatpak/
        for a in APPS:
            app_id = f"{ORG}.{a['id']}"
            write(os.path.join(out, f"{app_id}.desktop"), desktop(a))
            write(os.path.join(out, f"{app_id}.metainfo.xml"), metainfo(a))
    elif mode == "submit":
        # one folder per app, as its Flathub repo holds it: the manifest (building bramble-office at
        # the given commit), its crates and the modules it uses
        commit = sys.argv[3]
        for a in [a for a in APPS if a.get("submit", True)]:
            app_id = f"{ORG}.{a['id']}"
            d = os.path.join(out, app_id)
            os.makedirs(d, exist_ok=True)
            write(os.path.join(d, f"{app_id}.yml"), manifest(a, commit))
            for m in a["modules"]:
                with open(os.path.join(HERE, m)) as f:
                    write(os.path.join(d, m), f.read())
            subprocess.run([sys.executable, os.path.join(HERE, "cargo_sources.py"), os.path.join(P, a["ws"], "Cargo.lock"), os.path.join(d, f"{a['ws']}-cargo-sources.json")], check=True)
    print(f"{out}: {len(APPS)} apps")


if __name__ == "__main__":
    main()
