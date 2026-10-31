<p align="center">
  <img src="https://bramble-apps.github.io/logo/bramble-mark.svg" width="88" alt="Bramble Office">
</p>

<h1 align="center">Bramble Office</h1>

<p align="center">
  <b>Keydeck</b> · <b>Values</b> · <b>Letters</b><br>
  Free desktop apps that open, edit and save Apple Keynote, Numbers and Pages files,<br>
  and work with PowerPoint, Excel and Word too. For Linux, Windows, macOS and Android.
</p>

<p align="center">
  <a href="https://bramble-apps.github.io">Website</a> ·
  <a href="https://github.com/bramble-apps/bramble-office/releases/latest">All downloads</a> ·
  <a href="https://www.paypal.com/donate/?hosted_button_id=5EP8EM86U9P7Q">Support development</a>
</p>

## Download

| System | Download |
|---|---|
| **Windows** 10 and 11 (64-bit) | [![Download for Windows](https://img.shields.io/badge/Windows-Download-0078D4?style=for-the-badge&logo=windows&logoColor=white)](https://github.com/bramble-apps/bramble-office/releases/latest/download/Bramble-Office-windows-x86_64.zip) |
| **macOS**, Apple silicon (M1 and later), macOS 11+ | [![Download for Apple silicon](https://img.shields.io/badge/macOS-Apple_silicon-111111?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/bramble-apps/bramble-office/releases/latest/download/Bramble-Office-macos-apple-silicon.zip) |
| **macOS**, Intel Macs, macOS 10.15+ | [![Download for Intel Macs](https://img.shields.io/badge/macOS-Intel-555555?style=for-the-badge&logo=apple&logoColor=white)](https://github.com/bramble-apps/bramble-office/releases/latest/download/Bramble-Office-macos-intel.zip) |
| **Linux** AppImages (x86_64): one file per app, nothing to install | [![Keydeck AppImage](https://img.shields.io/badge/Keydeck-AppImage-c2306a?style=for-the-badge&logo=linux&logoColor=white)](https://github.com/bramble-apps/bramble-office/releases/latest/download/Keydeck-x86_64.AppImage) [![Values AppImage](https://img.shields.io/badge/Values-AppImage-8e3a8a?style=for-the-badge&logo=linux&logoColor=white)](https://github.com/bramble-apps/bramble-office/releases/latest/download/Values-x86_64.AppImage) [![Letters AppImage](https://img.shields.io/badge/Letters-AppImage-5a7f3c?style=for-the-badge&logo=linux&logoColor=white)](https://github.com/bramble-apps/bramble-office/releases/latest/download/Letters-x86_64.AppImage) |
| **Android** 8+, tablets and phones (64-bit ARM): one app each. *Early development* | [![Keydeck for Android](https://img.shields.io/badge/Keydeck-Android-c2306a?style=for-the-badge&logo=android&logoColor=white)](https://github.com/bramble-apps/bramble-office/releases/latest/download/Keydeck-android.apk) [![Values for Android](https://img.shields.io/badge/Values-Android-8e3a8a?style=for-the-badge&logo=android&logoColor=white)](https://github.com/bramble-apps/bramble-office/releases/latest/download/Values-android.apk) [![Letters for Android](https://img.shields.io/badge/Letters-Android-5a7f3c?style=for-the-badge&logo=android&logoColor=white)](https://github.com/bramble-apps/bramble-office/releases/latest/download/Letters-android.apk) |
| **Linux**, all three apps in one archive (x86_64) | [![Download for Linux](https://img.shields.io/badge/Linux-.tar.gz-333333?style=for-the-badge&logo=linux&logoColor=white)](https://github.com/bramble-apps/bramble-office/releases/latest/download/Bramble-Office-linux-x86_64.tar.gz) |

Free and open source (GPL-3.0). No account, no subscription, no tracking.

<p align="center">
  <img src="https://bramble-apps.github.io/img/app-kd1.jpg" width="820" alt="Keydeck editing a presentation">
</p>

## The apps

| | App | For | Opens and saves | Also works with |
|---|---|---|---|---|
| <img src="https://bramble-apps.github.io/img/keydeck.png" width="40"> | **Keydeck** | Presentations | Keynote `.key` | PowerPoint `.pptx`; exports PDF, images, movies, animated GIFs, HTML |
| <img src="https://bramble-apps.github.io/img/values.png" width="40"> | **Values** | Spreadsheets | Numbers `.numbers` | Excel `.xlsx`, CSV, TSV; exports PDF, CSV, Excel |
| <img src="https://bramble-apps.github.io/img/letters.png" width="40"> | **Letters** | Word processing and page layout | Pages `.pages` | Word `.docx`, RTF, HTML, Markdown, text; exports PDF, Word, RTF, EPUB, HTML |

iWork files are read and written natively, not converted: what you don't touch is saved back exactly as it
was, so a document can go back and forth with a Mac. Highlights:

- **Keydeck:** transitions and builds including Magic Move, dynamic backgrounds, presenter display,
  recorded slideshows, charts linked to tables, 18 built-in themes.
- **Values:** formulas, charts, pivot tables, categories, filters, conditional highlighting, Excel files kept
  as Excel.
- **Letters:** live text wrap around pictures and shapes, styles, tables, lists, sections, Word files kept
  as Word, a page layout mode.
- **All three:** the Content Hub (openly licensed photos and illustrations from Wikimedia Commons, and over
  1,200 shapes), a colour picker, liquid glass or solid looks, light and dark themes, adjustable scaling.

## Installing and first launch

### Windows

1. Unzip `Bramble-Office-windows-x86_64.zip` (right-click, **Extract All**).
2. Open the `bramble-office` folder and start `keydeck.exe`, `values.exe` or `letters.exe`.
3. The apps aren't code-signed yet, so Windows may say *Windows protected your PC*: choose **More info**,
   then **Run anyway**.

Needs Windows 10 or 11, 64-bit, and a graphics card with DirectX 12, Vulkan or OpenGL 3.3 (any PC from the
last ten years). Nothing else to install.

### macOS

1. Unzip the download for your Mac: **Apple silicon** for M1 and later, **Intel** for older Macs. (Apple
   menu > About This Mac shows which you have.)
2. Drag Keydeck, Values and Letters into **Applications**.
3. The apps aren't notarized yet: the first time, **right-click (or Control-click) the app and choose Open**,
   then Open again. If macOS says the app is damaged, run this once in Terminal:

   ```sh
   xattr -cr /Applications/Keydeck.app /Applications/Values.app /Applications/Letters.app
   ```

Apple silicon builds need macOS 11 Big Sur or later; Intel builds need macOS 10.15 Catalina or later.

### Linux

**AppImage** (any distribution): download the app, make it executable and run it.

```sh
chmod +x Keydeck-x86_64.AppImage
./Keydeck-x86_64.AppImage
```

**Archive:** unpack it and run the apps from the `bramble-office` folder (copy them to `~/.local/bin` to
have them on your `PATH`).

```sh
tar -xzf Bramble-Office-linux-x86_64.tar.gz
./bramble-office/keydeck
```

**Requirements:** x86_64 with glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36, Linux Mint 21,
openSUSE Leap 15.5 and later), and Vulkan or OpenGL 3.3 graphics drivers (Mesa on most systems), on Wayland
or X11. The AppImages carry the keyboard and input libraries themselves; for the archive, and for video and
sound, install these:

| Distribution | Command |
|---|---|
| Ubuntu, Debian, Mint | `sudo apt install libxkbcommon-x11-0 libxcursor1 libxrandr2 libxi6 mesa-vulkan-drivers ffmpeg` |
| Fedora | `sudo dnf install libxkbcommon-x11 libXcursor libXrandr libXi mesa-vulkan-drivers ffmpeg` |
| Arch, Manjaro | `sudo pacman -S libxkbcommon-x11 libxcursor libxrandr libxi vulkan-icd-loader ffmpeg` |

`ffmpeg` plays and exports movies and sound; everything else works without it. Keydeck downloads the
`yt-dlp` helper itself the first time you add a web video. If an AppImage doesn't start because the system
has no FUSE, run it with `--appimage-extract-and-run`.

### Android (early development)

The Android versions are new and still rough: expect bugs, and keep copies of important files.

1. On the tablet or phone, download `Keydeck-android.apk`, `Values-android.apk` or `Letters-android.apk`.
2. Open it from the notification or the Downloads app. The first time, Android asks to allow installing
   apps from your browser or file manager: allow it, go back and tap **Install**.
3. The apps aren't on Google Play yet, so Play Protect may warn about an unknown app: choose
   **Install anyway**.

Needs Android 8 or later on a 64-bit ARM device (almost every phone and tablet since 2017). The apps
are the full desktop apps, so they're most comfortable on a tablet or a phone held sideways, ideally with
a keyboard and mouse. Files open with Android's file picker (Google Drive, Downloads, USB drives and so
on), and saving writes back to the same place. Things that need desktop programs (movie export, ffmpeg,
printing, recording) aren't available on Android yet.

## Building from source

The three apps share one engine for reading, drawing and writing iWork files, which lives in Keydeck's
folder. Values and Letters build against it by relative path, so keep the three folders side by side.

- `keynote-linux/`: **Keydeck** and the shared crates
  - `crates/kn-format`: iWork package and archive reading and writing (lossless round-trip)
  - `crates/kn-model`: documents, slides, text, tables, charts, shapes and undoable edits
  - `crates/kn-render`: drawing (tiny-skia, cosmic-text), animations and transitions
  - `crates/kn-ui`: the shared interface (themes, colour picker, chart panes, Content Hub, icons)
  - `crates/kn-app`: the `keydeck` app
- `numbers-linux/`: **Values** (`crates/nm-core`, `crates/nm-app` → the `values` app)
- `pages-linux/`: **Letters** (`crates/pg-core`, `crates/pg-app` → the `letters` app)

You need a recent stable Rust (`rustup`, edition 2024). On Linux you also need the windowing libraries'
headers (Debian or Ubuntu: `libxkbcommon-dev libwayland-dev libx11-dev libgtk-3-dev`).

```sh
cd keynote-linux && cargo build --release -p kn-app     # target/release/keydeck
cd numbers-linux && cargo build --release -p nm-app     # target/release/values
cd pages-linux   && cargo build --release -p pg-app     # target/release/letters
```

Android APKs are built with `keynote-linux/tools/android/build-apk.sh` (it needs the Android SDK and NDK
and a JDK; see the script). Each folder has a `run.sh` that builds and starts its app; `packaging/` holds the scripts for Linux packages
and macOS app bundles. Every tagged version (`v*`) is built for all systems by
`.github/workflows/build.yml` and published under [Releases](https://github.com/bramble-apps/bramble-office/releases).

### Tests

```sh
cargo test --workspace     # in each of the three folders
```

Many tests compare against documents made in Keynote, Numbers and Pages. Those files are Apple-made and
aren't part of this repository, so those tests are reported as *ignored* here. Every other test runs.

## Credits

- Shapes: [Phosphor Icons](https://phosphoricons.com) (MIT).
- Theme photos: [Unsplash](https://unsplash.com/license) photographers, credited in
  `keynote-linux/crates/kn-app/data/themes/PHOTO-CREDITS.tsv`.
- Content Hub pictures: [Wikimedia Commons](https://commons.wikimedia.org) Featured pictures, each under its
  own free licence; the app keeps each picture's credit.
- Fonts: Carlito and Caladea (SIL Open Font License, `keynote-linux/crates/kn-render/fonts`) and DejaVu Sans
  (`keynote-linux/crates/kn-ui/data`).
- Patched copies of winit and smithay-client-toolkit (MIT / Apache-2.0, `keynote-linux/vendor`).

## Licence

Bramble Office is free software under the **GNU General Public License, version 3 only** (see `LICENSE`).

## Not affiliated with Apple

Bramble Office is an independent project. It is not affiliated with, endorsed by or sponsored by Apple Inc.
Apple, Keynote, Numbers, Pages and iWork are trademarks of Apple Inc.; Microsoft, PowerPoint, Excel and Word
are trademarks of Microsoft Corporation. The names are used only to say which file formats the apps work
with. Bramble Office contains no source code from Apple or Microsoft; file compatibility was built for
interoperability, by studying how documents are stored. The documents built into the apps as templates
were made in Apple's apps for their structure only, with every image and preview replaced by original or
openly licensed ones.
