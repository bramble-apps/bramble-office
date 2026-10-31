"""One-time: turn an app crate's main.rs into lib.rs (everything, plus android_main) and a thin
main.rs, so the same code builds as the desktop program and as Android's native library.
usage: split-lib.py <crate dir> <lib name> <app name>"""
import re, sys, pathlib
crate, lib, name = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
src = (crate / "src/main.rs").read_text()
attr = '// Windows: a window app, without a console window behind it\n#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]\n'
assert attr in src
body = src.replace(attr, "")
assert body.count("fn main() -> eframe::Result {") == 1
body = body.replace("fn main() -> eframe::Result {", "/// Starts the app: the desktop program's main, and android_main on Android.\npub fn run() -> eframe::Result {")
# Android: the activity's app goes into the options, and the surface is opaque
m = re.search(r"\n(    let options = eframe::NativeOptions \{.*?\n    \};\n)", body, re.S)
assert m, "options"
body = body.replace(m.group(1), m.group(1) + """    // Android: the activity the app runs in, drawn opaque
    #[cfg(target_os = "android")]
    let options = eframe::NativeOptions { android_app: kn_ui::android::app(), viewport: options.viewport.with_transparent(false), ..options };
""")
body = body.rstrip("\n") + f"""

/// Android: the activity (app.bramble.BrambleActivity, tools/android) starts the app here.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: kn_ui::android::AndroidApp) {{
    kn_ui::android::init(&app, "{name}");
    let _ = run();
}}
"""
(crate / "src/lib.rs").write_text(body)
doc = "".join(l for l in src.splitlines(True) if l.startswith("//!"))
(crate / "src/main.rs").write_text(doc + attr + f"\nfn main() -> eframe::Result {{\n    {lib}::run()\n}}\n")
toml = (crate / "Cargo.toml").read_text()
toml = toml.replace("[[bin]]", f'# the app itself (also Android\'s native library: tools/android/build-apk.sh)\n[lib]\nname = "{lib}"\npath = "src/lib.rs"\n\n[[bin]]', 1)
toml = re.sub(r'\nrfd = "[^"]*"\n', "\n", toml)
(crate / "Cargo.toml").write_text(toml)
for f in (crate / "src").rglob("*.rs"):
    t = f.read_text()
    if "rfd::" in t:
        f.write_text(t.replace("rfd::FileDialog", "kn_ui::files::FileDialog"))
