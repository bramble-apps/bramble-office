//! The template chooser (dialogs.rs `chooser`, kn_ui welcome.rs) as data, for front ends that
//! draw it themselves (dlgapi.rs window "chooser"): its words, Pages' Basic templates, the recent
//! documents with their thumbnails (as welcome.rs makes them: the preview.jpg a document holds,
//! else its first page, cached in ~/.cache/letters/previews), and its buttons.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Value, json};

use crate::app::App;

/// The template chooser's templates (dialogs.rs ITEMS; menus::new_from_template's indices).
pub const ITEMS: [&str; 6] = ["Blank", "Blank Landscape", "Blank A4", "Note Taking", "Blank Layout", "Essay"];

/// Recent documents' thumbnails asked for: true once made (or found there's none to make).
static MAKING: Mutex<Option<HashMap<PathBuf, bool>>> = Mutex::new(None);

/// The cached thumbnail of a recent document (welcome.rs read_preview's file), None until made.
fn cache_file(path: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let meta = std::fs::metadata(path).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (path, meta.len(), meta.modified().ok()).hash(&mut h);
    let cache = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(cache.join("letters").join("previews").join(format!("{:016x}.jpg", h.finish())))
}

/// A recent document's thumbnail file: made on a thread the first time (None meanwhile, and
/// when the document gives none).
fn thumbnail(path: &Path) -> Option<PathBuf> {
    let file = cache_file(path)?;
    if file.exists() {
        return Some(file);
    }
    let mut g = MAKING.lock().unwrap_or_else(|e| e.into_inner());
    let map = g.get_or_insert_with(HashMap::new);
    if !map.contains_key(path) {
        map.insert(path.to_path_buf(), false);
        let p = path.to_path_buf();
        std::thread::spawn(move || {
            let packaged = || -> Option<Vec<u8>> {
                let k = pg_core::kn_format::keyfile::KeyFile::open(&p).ok()?;
                k.entry_bytes(k.entry("preview.jpg")?).map(|b| b.into_owned())
            };
            if let Some(bytes) = packaged().or_else(|| crate::dialogs::first_page_png(&p)) {
                let _ = std::fs::create_dir_all(file.parent().unwrap_or(Path::new(".")));
                let _ = std::fs::write(&file, &bytes);
            }
            // (made or not: a document that gives none isn't tried again)
            if let Some(m) = MAKING.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                m.insert(p, true);
            }
        });
    }
    None
}

/// Thumbnails still being made (the front end asks again soon).
pub fn pending() -> bool {
    MAKING.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|m| m.values().any(|done| !done))
}

/// The chooser's window, when it shows.
pub fn state(app: &App) -> Option<Value> {
    if !app.chooser {
        return None;
    }
    let recent: Vec<Value> = app
        .recent()
        .iter()
        .map(|p| {
            let thumb = thumbnail(p);
            json!({
                "path": p.to_string_lossy(),
                "name": p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                "thumb": thumb.map(|f| f.to_string_lossy().into_owned()),
            })
        })
        .collect();
    let [r, g, b, a] = kn_ui::theme::CANVAS_BG().to_srgba_unmultiplied();
    Some(json!({
        "id": "chooser",
        "title": "Choose a Template",
        "app": "Letters",
        "section": "Templates",
        "all": "All Templates",
        "hint": "or drop a .pages or .docx file here",
        "templates": ITEMS,
        "default": "Blank",
        "tile": [124.0, 160.0],
        "cancel": true,
        "recent": recent,
        // (welcome.rs overlay's backdrop: the theme's canvas colour)
        "backdrop": (u32::from(a) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
    }))
}

/// Its buttons: "create" (a template's index), "open" (a recent document's path), "browse"
/// (Open…: the front end picks the file and opens it), "cancel".
pub fn set(app: &mut App, button: &str, v: &Value) {
    app.chooser = false;
    match button {
        "create" => crate::menus::new_from_template(app, v.as_u64().unwrap_or(0) as usize),
        "open" => app.open(Path::new(v.as_str().unwrap_or_default())),
        _ => {}
    }
}
