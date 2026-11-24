//! File > New: the theme chooser. Keydeck's own themes are built in (BUILTIN); more themes are
//! any .key files in ~/.local/share/keydeck/themes/ (one slide per layout, as Keynote's own
//! themes have).

use std::path::PathBuf;

use egui::RichText;

use crate::app::App;

static WHITE: &[u8] = include_bytes!("../data/themes/White.key");

/// The built-in themes: (name, theme, preview). A theme's path in the lists is "builtin:Name".
/// On native targets all 19 `.key` themes are embedded; on WebAssembly only `White.key` and the
/// small preview PNGs are embedded, while the other 18 `.key` themes are fetched lazily over HTTP.
#[cfg(not(target_arch = "wasm32"))]
static BUILTIN: &[(&str, &[u8], &[u8])] = &[
    ("White", WHITE, include_bytes!("../data/themes/White.png")),
    ("Bloom", include_bytes!("../data/themes/Bloom.key"), include_bytes!("../data/themes/Bloom.png")),
    ("Cirrus", include_bytes!("../data/themes/Cirrus.key"), include_bytes!("../data/themes/Cirrus.png")),
    ("Daylight", include_bytes!("../data/themes/Daylight.key"), include_bytes!("../data/themes/Daylight.png")),
    ("Dusk", include_bytes!("../data/themes/Dusk.key"), include_bytes!("../data/themes/Dusk.png")),
    ("Forge", include_bytes!("../data/themes/Forge.key"), include_bytes!("../data/themes/Forge.png")),
    ("Gallery", include_bytes!("../data/themes/Gallery.key"), include_bytes!("../data/themes/Gallery.png")),
    ("Ivory", include_bytes!("../data/themes/Ivory.key"), include_bytes!("../data/themes/Ivory.png")),
    ("Journal", include_bytes!("../data/themes/Journal.key"), include_bytes!("../data/themes/Journal.png")),
    ("Linen", include_bytes!("../data/themes/Linen.key"), include_bytes!("../data/themes/Linen.png")),
    ("Midnight", include_bytes!("../data/themes/Midnight.key"), include_bytes!("../data/themes/Midnight.png")),
    ("Night Tide", include_bytes!("../data/themes/Night Tide.key"), include_bytes!("../data/themes/Night Tide.png")),
    ("Onyx", include_bytes!("../data/themes/Onyx.key"), include_bytes!("../data/themes/Onyx.png")),
    ("Paper Craft", include_bytes!("../data/themes/Paper Craft.key"), include_bytes!("../data/themes/Paper Craft.png")),
    ("Prism", include_bytes!("../data/themes/Prism.key"), include_bytes!("../data/themes/Prism.png")),
    ("Storm", include_bytes!("../data/themes/Storm.key"), include_bytes!("../data/themes/Storm.png")),
    ("Studio", include_bytes!("../data/themes/Studio.key"), include_bytes!("../data/themes/Studio.png")),
    ("Tide", include_bytes!("../data/themes/Tide.key"), include_bytes!("../data/themes/Tide.png")),
    ("Vivid", include_bytes!("../data/themes/Vivid.key"), include_bytes!("../data/themes/Vivid.png")),
];

#[cfg(target_arch = "wasm32")]
static BUILTIN: &[(&str, &[u8], &[u8])] = &[
    ("White", WHITE, include_bytes!("../data/themes/White.png")),
    ("Bloom", &[], include_bytes!("../data/themes/Bloom.png")),
    ("Cirrus", &[], include_bytes!("../data/themes/Cirrus.png")),
    ("Daylight", &[], include_bytes!("../data/themes/Daylight.png")),
    ("Dusk", &[], include_bytes!("../data/themes/Dusk.png")),
    ("Forge", &[], include_bytes!("../data/themes/Forge.png")),
    ("Gallery", &[], include_bytes!("../data/themes/Gallery.png")),
    ("Ivory", &[], include_bytes!("../data/themes/Ivory.png")),
    ("Journal", &[], include_bytes!("../data/themes/Journal.png")),
    ("Linen", &[], include_bytes!("../data/themes/Linen.png")),
    ("Midnight", &[], include_bytes!("../data/themes/Midnight.png")),
    ("Night Tide", &[], include_bytes!("../data/themes/Night Tide.png")),
    ("Onyx", &[], include_bytes!("../data/themes/Onyx.png")),
    ("Paper Craft", &[], include_bytes!("../data/themes/Paper Craft.png")),
    ("Prism", &[], include_bytes!("../data/themes/Prism.png")),
    ("Storm", &[], include_bytes!("../data/themes/Storm.png")),
    ("Studio", &[], include_bytes!("../data/themes/Studio.png")),
    ("Tide", &[], include_bytes!("../data/themes/Tide.png")),
    ("Vivid", &[], include_bytes!("../data/themes/Vivid.png")),
];

#[cfg(target_arch = "wasm32")]
mod web_fetch {
    use std::collections::HashMap;
    use std::sync::mpsc::{channel, Receiver};
    use std::sync::{Mutex, OnceLock};

    static CACHE: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
    static PENDING: OnceLock<Mutex<Option<(String, bool, Receiver<Result<Vec<u8>, String>>)>>> = OnceLock::new();

    pub fn cached(name: &str) -> Option<Vec<u8>> {
        CACHE.get()?.lock().ok()?.get(name).cloned()
    }

    pub fn is_loading() -> bool {
        PENDING.get().and_then(|m| m.lock().ok()).is_some_and(|g| g.is_some())
    }

    pub fn request(name: &str, changing: bool) {
        let (tx, rx) = channel();
        let file = format!("{}.key", kn_ui::web::fetch::encode_query_component(name));
        let url = format!("{}/{file}", kn_ui::web::themes_base_url());
        let key_name = name.to_string();
        kn_ui::web::fetch::fetch_bytes(url, move |res| {
            if let Ok(bytes) = &res {
                let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
                if let Ok(mut g) = cache.lock() {
                    g.insert(key_name, bytes.clone());
                }
            }
            let _ = tx.send(res);
        });
        let pend = PENDING.get_or_init(|| Mutex::new(None));
        if let Ok(mut g) = pend.lock() {
            *g = Some((name.to_string(), changing, rx));
        }
    }

    pub fn poll() -> Option<(String, bool, Result<Vec<u8>, String>)> {
        let pend = PENDING.get()?;
        let mut g = pend.lock().ok()?;
        let ready = match g.as_ref() {
            Some((_, _, rx)) => match rx.try_recv() {
                Ok(res) => Some(res),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err("theme download interrupted".into())),
            },
            None => None,
        };
        if let Some(res) = ready {
            let (name, changing, _) = g.take()?;
            Some((name, changing, res))
        } else {
            None
        }
    }
}

/// Whether a WebAssembly theme `.key` download is currently in flight.
pub fn is_loading() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_fetch::is_loading()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// Poll any pending lazy theme download on WebAssembly and open/apply it when ready.
pub fn poll_web(app: &mut App, ctx: &egui::Context) {
    #[cfg(target_arch = "wasm32")]
    if let Some((name, changing, res)) = web_fetch::poll() {
        let file = res
            .map_err(kn_format::Error::Invalid)
            .and_then(|bytes| kn_format::KeyFile::from_zip(&bytes));
        if changing {
            change_to(app, &name, file);
        } else {
            create(app, ctx, &name, file);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (app, ctx);
}

fn builtin(p: &std::path::Path) -> Option<&'static (&'static str, &'static [u8], &'static [u8])> {
    let name = p.to_str()?.strip_prefix("builtin:")?;
    BUILTIN.iter().find(|b| b.0 == name)
}

/// A theme from the lists (`None` = the built-in White).
pub(crate) fn open(path: &Option<PathBuf>) -> Result<kn_format::KeyFile, kn_format::Error> {
    match path {
        Some(p) => match builtin(p) {
            Some(b) if !b.1.is_empty() => kn_format::KeyFile::from_zip(b.1),
            #[cfg(target_arch = "wasm32")]
            Some(b) => match web_fetch::cached(b.0) {
                Some(bytes) => kn_format::KeyFile::from_zip(&bytes),
                None => Err(kn_format::Error::Invalid(format!("theme {} is not downloaded yet", b.0))),
            },
            #[cfg(not(target_arch = "wasm32"))]
            Some(b) => kn_format::KeyFile::from_zip(b.1),
            None => kn_format::KeyFile::open(p),
        },
        None => kn_format::KeyFile::from_zip(WHITE),
    }
}

/// The built-in White theme (PowerPoint import builds on it).
pub(crate) fn white() -> Result<kn_format::KeyFile, kn_format::Error> {
    kn_format::KeyFile::from_zip(WHITE)
}

pub(crate) fn theme_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("keydeck").join("themes"))
}

/// (name, file): the built-in themes first, then the user's and the system's.
pub fn available() -> Vec<(String, Option<PathBuf>)> {
    let mut out: Vec<(String, Option<PathBuf>)> = BUILTIN.iter().map(|b| (b.0.to_string(), Some(PathBuf::from(format!("builtin:{}", b.0))))).collect();
    // the user's own themes, then the ones installed with Keydeck (XDG_DATA_DIRS, e.g.
    // /usr/share/keydeck/themes); a user theme of the same name wins
    let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let dirs = theme_dir().into_iter().chain(system.split(':').filter(|d| !d.is_empty()).map(|d| PathBuf::from(d).join("keydeck/themes")));
    for dir in dirs {
        if let Ok(rd) = std::fs::read_dir(dir) {
            let mut found: Vec<(String, Option<PathBuf>)> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("key")))
                .filter_map(|p| Some((p.file_stem()?.to_string_lossy().to_string(), Some(p))))
                .filter(|(n, _)| !out.iter().any(|(m, _)| m == n))
                .collect();
            found.sort();
            out.extend(found);
        }
    }
    out
}

/// Preview image next to a theme file (<Theme>.png), as a texture (cached by egui by name).
pub(crate) fn preview(ctx: &egui::Context, name: &str, path: &Option<PathBuf>) -> Option<egui::TextureHandle> {
    let p = path.as_ref()?;
    let id = egui::Id::unique(("theme-preview", name));
    if let Some(t) = ctx.data(|d| d.get_temp::<Option<egui::TextureHandle>>(id)) {
        return t;
    }
    let bytes = match builtin(p) {
        Some(b) => Some(b.2.to_vec()),
        None => std::fs::read(p.with_extension("png")).ok(),
    };
    let tex = bytes.and_then(|b| image::load_from_memory(&b).ok()).map(|img| {
        let img = img.thumbnail(320, 180).to_rgba8();
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        ctx.load_texture(format!("theme-{name}"), ci, egui::TextureOptions::LINEAR)
    });
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

pub fn chooser(app: &mut App, ctx: &egui::Context) {
    if !app.show_theme_chooser {
        return;
    }
    let mut open = true;
    let mut pick: Option<(String, Option<PathBuf>)> = None;
    egui::Window::new("Choose a Theme").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).open(&mut open).show(ctx, |ui| {
        ui.label(RichText::new(if app.changing_theme { "Choose a new theme for this presentation" } else { "Pick a theme for your new presentation" }).color(crate::theme::TEXT_DIM()));
        ui.add_space(6.0);
        egui::Grid::new("themes").spacing([12.0, 8.0]).show(ui, |ui| {
            for (i, (name, path)) in available().into_iter().enumerate() {
                ui.vertical(|ui| {
                    let clicked = match preview(ctx, &name, &path) {
                        Some(t) => ui.add(egui::Button::image(egui::Image::new(&t).fit_to_exact_size(egui::vec2(160.0, 90.0)))).clicked(),
                        None => ui.add_sized([160.0, 90.0], egui::Button::new(RichText::new(&name).size(15.0))).clicked(),
                    };
                    ui.label(RichText::new(&name).size(12.0));
                    if clicked {
                        pick = Some((name.clone(), path.clone()));
                    }
                });
                if i % 4 == 3 {
                    ui.end_row();
                }
            }
        });
    });
    app.show_theme_chooser = open;
    if !open {
        app.changing_theme = false;
    }
    if let Some((name, path)) = pick {
        app.show_theme_chooser = false;
        let changing = std::mem::take(&mut app.changing_theme);
        #[cfg(target_arch = "wasm32")]
        if let Some(p) = &path
            && let Some(b) = builtin(p)
            && b.1.is_empty()
            && web_fetch::cached(b.0).is_none()
        {
            web_fetch::request(b.0, changing);
            app.notify(format!("Loading theme \u{201c}{name}\u{201d}\u{2026}"));
            return;
        }
        let file = self::open(&path);
        if changing {
            change_to(app, &name, file);
            return;
        }
        create(app, ctx, &name, file);
    }
}

/// File > Change Theme: the presentation rebuilt on the new theme (same tab, same file).
pub(crate) fn change_to(app: &mut App, name: &str, file: Result<kn_format::KeyFile, kn_format::Error>) {
    crate::textedit::commit(app);
    app.commit_notes();
    let res = match (file, app.doc.as_ref()) {
        (Ok(f), Some(doc)) => doc.with_theme(f),
        (Err(e), _) => Err(e),
        (_, None) => return,
    };
    match res {
        Ok((nd, problems)) => {
            // one undoable step (Edit > Undo Change Theme brings the old theme back)
            let cur = app.cur;
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Change Theme");
                doc.replace_contents(&mut app.hist, nd);
                doc.end(&mut app.hist);
            }
            app.refresh_after_structural_change();
            app.go_to(cur.min(app.slides.len().saturating_sub(1)));
            app.notify(if problems.is_empty() { format!("Changed the theme to {name}") } else { format!("Changed the theme to {name}; {} object(s) couldn't be carried over", problems.len()) });
        }
        Err(e) => app.notify(format!("Couldn't change the theme: {e}")),
    }
}

/// The theme chooser's pick for an open presentation (Change Theme), by its index in `available`.
pub(crate) fn change_to_index(app: &mut App, i: usize) {
    app.show_theme_chooser = false;
    app.changing_theme = false;
    if let Some((name, path)) = available().get(i).cloned() {
        let file = open(&path);
        change_to(app, &name, file);
    }
}

/// A new presentation on a theme (`path` None = the built-in White).
pub fn create_from(app: &mut App, ctx: &egui::Context, name: &str, path: Option<PathBuf>) {
    #[cfg(target_arch = "wasm32")]
    if let Some(p) = &path
        && let Some(b) = builtin(p)
        && b.1.is_empty()
        && web_fetch::cached(b.0).is_none()
    {
        web_fetch::request(b.0, false);
        app.notify(format!("Loading theme \u{201c}{name}\u{201d}\u{2026}"));
        return;
    }
    let file = open(&path);
    create(app, ctx, name, file);
}

fn create(app: &mut App, ctx: &egui::Context, name: &str, file: Result<kn_format::KeyFile, kn_format::Error>) {
    match file.and_then(kn_model::Document::new_from_theme) {
        Ok(doc) => {
            app.new_document(doc);
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("Untitled ({name}) — Keydeck")));
        }
        Err(e) => app.notify(format!("Couldn't create the presentation: {e}")),
    }
}

/// A theme's preview as a PNG file (other front ends draw it; ffi.rs kd_themes): the built-in
/// ones written to the cache once, a user theme's own <Theme>.png beside it.
pub(crate) fn preview_file(name: &str, path: &Option<PathBuf>) -> Option<PathBuf> {
    let p = path.as_ref()?;
    let Some(b) = builtin(p) else {
        let png = p.with_extension("png");
        return png.is_file().then_some(png);
    };
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    let file = base.join("keydeck").join("theme-previews").join(format!("{name}.png"));
    if !file.is_file() {
        std::fs::create_dir_all(file.parent()?).ok()?;
        std::fs::write(&file, b.2).ok()?;
    }
    Some(file)
}
