//! Fonts found on the system at run time (none are bundled): a sans for the "system" look in
//! several weights (variable fonts give real thin digits), a bold monospace for "chicago", and
//! egui's own fonts behind them as the fallback.

use egui::epaint::text::{FontTweak, VariationCoords};
use egui::{FontData, FontDefinitions, FontFamily, FontId};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const WEIGHTS: [u16; 6] = [200, 300, 400, 500, 600, 700];

/// Preferred sans files, best first (lower-case file-name prefixes).
const SANS: &[&str] = &[
    "sf-pro-display",
    "sf-pro-text",
    "sf-pro.ttf",
    "sfns.ttf",
    "intervariable.ttf",
    "inter-variable",
    "inter[",
    "ubuntusans[",
    "roboto[",
    "robotoflex",
    "inter-regular",
    "roboto-regular",
    "sf-pro-rounded-regular",
    "segoeui.ttf",
    "cantarell-vf",
    "cantarell[",
    "cantarell-regular",
    "dejavusans.ttf",
];

const MONO: &[&str] = &["dejavusansmono-bold.ttf", "liberationmono-bold.ttf", "notosansmono-bold.ttf", "consolab.ttf", "menlo.ttc"];

fn font_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = ["/usr/share/fonts", "/usr/local/share/fonts", "/System/Library/Fonts", "/Library/Fonts", "C:\\Windows\\Fonts"]
        .iter()
        .map(PathBuf::from)
        .collect();
    if let Ok(x) = std::env::var("XDG_DATA_HOME") {
        v.push(Path::new(&x).join("fonts"));
    }
    if let Ok(h) = std::env::var("HOME") {
        let h = PathBuf::from(h);
        v.extend([h.join(".local/share/fonts"), h.join(".fonts"), h.join("Library/Fonts")]);
    }
    v
}

/// Font files under the usual folders (symlinks not followed, depth-limited).
fn scan() -> Vec<PathBuf> {
    fn walk(d: &Path, depth: u32, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() && depth < 6 {
                walk(&p, depth + 1, out);
            } else if ft.is_file() {
                let n = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
                if n == "ttf" || n == "otf" || n == "ttc" {
                    out.push(p);
                }
            }
        }
    }
    let mut out = Vec::new();
    for d in font_dirs() {
        walk(&d, 0, &mut out);
    }
    out
}

fn name(p: &Path) -> String {
    p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn pick(files: &[PathBuf], prefs: &[&str]) -> Option<PathBuf> {
    for pre in prefs {
        let mut hits: Vec<&PathBuf> = files.iter().filter(|p| name(p).starts_with(pre) && !name(p).contains("italic")).collect();
        hits.sort_by_key(|p| name(p).len());
        if let Some(p) = hits.first() {
            return Some((*p).clone());
        }
    }
    None
}

/// A sibling static file of another weight ("X-Regular.ttf" → "X-Light.ttf").
fn sibling(main: &Path, files: &[PathBuf], styles: &[&str]) -> Option<PathBuf> {
    let n = name(main);
    let stem = n.trim_end_matches(".ttf").trim_end_matches(".otf").trim_end_matches("-regular").trim_end_matches("regular");
    for s in styles {
        let want = format!("{stem}-{s}");
        if let Some(p) = files.iter().find(|p| {
            let m = name(p);
            m.starts_with(&want) && m.len() <= want.len() + 4 && p.parent() == main.parent()
        }) {
            return Some(p.clone());
        }
    }
    None
}

fn blob(p: &Path) -> Option<Arc<Vec<u8>>> {
    std::fs::read(p).ok().map(Arc::new)
}

fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

/// Loads the fonts into egui. Returns the main font's file name (for the about line).
pub fn install(ctx: &egui::Context) -> String {
    let mut defs = FontDefinitions::default();
    let fb = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mono_fb = defs.families.get(&FontFamily::Monospace).cloned().unwrap_or_default();
    let files = scan();
    let main = std::env::var("CALC_FONT").ok().map(PathBuf::from).filter(|p| p.is_file()).or_else(|| pick(&files, SANS));
    let mut used = String::from("egui default");
    let main_data = main.as_ref().and_then(|p| blob(p).map(|b| (p.clone(), b)));
    if let Some((path, bytes)) = &main_data {
        used = name(path);
        let probe = FontData::from_blob(bytes.clone(), 0);
        let wght = probe.variation_axes().into_iter().find(|a| a.tag.to_string() == "wght");
        for w in WEIGHTS {
            let key = format!("sys{w}");
            let data = if let Some(ax) = &wght {
                let v = (w as f32).clamp(ax.range.min, ax.range.max);
                FontData::from_blob(bytes.clone(), 0).tweak(FontTweak { coords: VariationCoords::new([(b"wght", v)]), ..Default::default() })
            } else {
                let styles: &[&str] = match w {
                    200 => &["thin", "extralight", "light"],
                    300 => &["light", "extralight"],
                    500 => &["medium"],
                    600 => &["semibold", "bold"],
                    700 => &["bold"],
                    _ => &[],
                };
                match sibling(path, &files, styles).and_then(|p| blob(&p)) {
                    Some(b) => FontData::from_blob(b, 0),
                    None => FontData::from_blob(bytes.clone(), 0),
                }
            };
            defs.font_data.insert(key.clone(), Arc::new(data));
            let mut fam = vec![key];
            fam.extend(fb.iter().cloned());
            defs.families.insert(family(&format!("sys{w}")), fam.clone());
        }
        // panels and menus use the regular weight
        let mut prop = vec!["sys400".to_string()];
        prop.extend(fb.iter().cloned());
        defs.families.insert(FontFamily::Proportional, prop);
    } else {
        for w in WEIGHTS {
            defs.families.insert(family(&format!("sys{w}")), fb.clone());
        }
    }
    let mut chicago = Vec::new();
    if let Some(b) = pick(&files, MONO).and_then(|p| blob(&p)) {
        defs.font_data.insert("chicago".into(), Arc::new(FontData::from_blob(b, 0)));
        chicago.push("chicago".to_string());
    }
    chicago.extend(mono_fb.iter().cloned());
    chicago.extend(fb.iter().cloned());
    defs.families.insert(family("chicago"), chicago);
    ctx.set_fonts(defs);
    used
}

/// A font for the theme's face (`"system"` or `"chicago"`) at a CSS-like weight.
pub fn font(face: &str, weight: u16, size: f32) -> FontId {
    if face == "chicago" {
        return FontId::new(size, family("chicago"));
    }
    let w = WEIGHTS.iter().copied().min_by_key(|w| (*w as i32 - weight as i32).abs()).unwrap_or(400);
    FontId::new(size, family(&format!("sys{w}")))
}

/// Panel text (menus, sheets, history).
pub fn ui(weight: u16, size: f32) -> FontId {
    font("system", weight, size)
}
