//! Content Hub (all three apps), laid out like the Mac's: a search field, round tabs (Browse,
//! Photos, Illustrations, Backgrounds, Shapes), category chips and rounded tiles; a click
//! inserts. Photos and illustrations are Wikimedia Commons' Featured pictures (inserted with
//! their credit); shapes are the app's basic shapes, the shape library (Phosphor Icons) and
//! My Shapes. Downloads run in background threads (ureq, with its own TLS, so nothing needs
//! installing), and thumbnails are cached on disk.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use egui::{pos2, vec2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, TextureHandle, TextureOptions};
use kn_model::shapelib::{self, LibShape};

const API: &str = "https://commons.wikimedia.org/w/api.php";
const UA: &str = "BrambleOffice/1.0 (https://bramble-apps.github.io)";
const FEATURED: &str = "Featured_pictures_on_Wikimedia_Commons";
const PAGE: usize = 40;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Browse,
    Photos,
    Illustrations,
    Backgrounds,
    Shapes,
}

/// (tab, label, icon from the shape library)
const TABS: &[(Tab, &str, &str)] = &[
    (Tab::Browse, "Browse", "Binoculars"),
    (Tab::Photos, "Photos", "Image"),
    (Tab::Illustrations, "Illustrations", "Paint brush"),
    (Tab::Backgrounds, "Backgrounds", "Squares four"),
    (Tab::Shapes, "Shapes", "Shapes"),
];

/// A picture category: (chip label, chip icon, search words, only drawings).
pub type Cat = (&'static str, &'static str, &'static str, bool);

pub const PHOTOS: &[Cat] = &[
    ("Animals", "Dog", "animal", false),
    ("Birds", "Bird", "bird", false),
    ("Nature", "Tree", "forest", false),
    ("Landscapes", "Mountains", "landscape", false),
    ("Water", "Drop", "lake", false),
    ("Plants and Flowers", "Flower lotus", "flower", false),
    ("Food", "Fork knife", "food", false),
    ("Architecture", "Bank", "architecture", false),
    ("Cities", "Buildings", "city", false),
    ("Interiors", "Armchair", "interior", false),
    ("Travel", "Globe", "landmark", false),
    ("People", "User", "people", false),
    ("Space", "Planet", "galaxy", false),
    ("Science", "Atom", "laboratory", false),
    ("Technology", "Cpu", "technology", false),
    ("Transport", "Car", "train", false),
    ("Sports", "Soccer ball", "sport", false),
    ("Art", "Palette", "painting", false),
];

pub const ILLUSTRATIONS: &[Cat] = &[
    ("Illustrations", "Paint brush", "", true),
    ("Diagrams", "Chart pie", "diagram", true),
    ("Maps", "Map trifold", "map", true),
    ("Anatomy", "Heart", "anatomy", true),
    ("Heraldry", "Star", "coat of arms", true),
];

pub const BACKGROUNDS: &[Cat] = &[
    ("Textures", "Squares four", "texture", false),
    ("Mosaics", "Grid four", "mosaic", false),
    ("Skies", "Cloud", "clouds", false),
    ("Water", "Waves", "waves", false),
    ("Sunsets", "Sparkle", "sunset", false),
];

fn cats(tab: Tab) -> &'static [Cat] {
    match tab {
        Tab::Photos => PHOTOS,
        Tab::Illustrations => ILLUSTRATIONS,
        Tab::Backgrounds => BACKGROUNDS,
        _ => &[],
    }
}

#[derive(Clone, Debug)]
pub struct Item {
    pub title: String,
    pub file: String,
    pub author: String,
    pub licence: String,
    pub credit_needed: bool,
    pub page_url: String,
    pub thumb_url: String,
    pub w: u32,
    pub h: u32,
}

impl Item {
    /// "Title" by Author, Licence, via Wikimedia Commons
    pub fn credit(&self) -> String {
        let by = if self.author.is_empty() { String::new() } else { format!(" by {}", self.author) };
        format!("“{}”{by}, {}, via Wikimedia Commons ({})", self.title, self.licence, self.page_url)
    }

    fn aspect(&self) -> f32 {
        if self.w == 0 || self.h == 0 { 1.4 } else { (self.w as f32 / self.h as f32).clamp(0.4, 3.0) }
    }
}

/// How an app's own shape (a basic shape, a My Shape) is drawn on its tile.
pub enum Glyph {
    Text,
    Line { arrow: bool },
    /// a quarter-circle curve (Numbers' Curve tile)
    Curve,
    /// the shape's name (a My Shape the app can't draw here)
    Label,
    Path { path: kn_model::Path, w: f32, h: f32 },
}

/// One of the app's own shapes; `key` comes back in [`Picked::App`].
pub struct AppShape {
    pub key: String,
    pub name: String,
    pub glyph: Glyph,
}

/// A picture to insert.
pub struct Picture {
    pub bytes: Vec<u8>,
    /// a file name with the right extension (png or jpg)
    pub name: String,
    pub credit: String,
}

/// What the app inserts.
pub enum Picked {
    Image(Picture),
    Library(&'static LibShape),
    App(String),
    /// Delete from My Shapes (a tile's context menu)
    Remove(String),
}

type Found = Result<(Vec<Item>, Option<usize>), String>;

#[derive(Default)]
struct Feed {
    items: Vec<Item>,
    next: Option<usize>,
    loading: Option<Receiver<Found>>,
    error: Option<String>,
    started: bool,
}

enum Thumb {
    Loading,
    Failed,
    Ready(TextureHandle),
}

#[derive(Default)]
pub struct Hub {
    pub open: bool,
    /// the app's basic shapes and My Shapes (it fills these when [`Hub::needs_shapes`] says so)
    pub basic: Vec<AppShape>,
    pub mine: Vec<AppShape>,
    shapes_wanted: bool,
    tab: Tab,
    /// the chosen chip per tab (0 = All on the Shapes tab)
    cat: [usize; 5],
    query: String,
    /// the search shown: set on Enter or a moment after typing stops
    searched: String,
    typed_at: Option<f64>,
    feeds: HashMap<String, Feed>,
    thumbs: HashMap<String, Thumb>,
    thumb_rx: Option<Receiver<(String, Option<image::RgbaImage>)>>,
    thumb_tx: Option<Sender<(String, Option<image::RgbaImage>)>>,
    queue: Vec<String>,
    running: usize,
    /// the picture being downloaded for insertion (its thumbnail url)
    fetching: Option<(String, Receiver<Result<Picture, String>>)>,
    credits: Vec<String>,
    note: Option<(String, f64)>,
    /// when the window last opened (its grow-in animation)
    opened_at: Option<f64>,
    picked_for_check: bool,
}

fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(40))).user_agent(UA).build().into())
}

fn get(url: &str, params: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    let mut req = agent().get(url);
    for (k, v) in params {
        req = req.query(*k, *v);
    }
    let mut resp = req.call().map_err(|_| "Couldn't reach Wikimedia Commons (no internet connection?)".to_string())?;
    resp.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|_| "The download stopped part way".to_string())
}

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(if cfg!(target_os = "macos") { "Library/Caches" } else { ".cache" })))
        .unwrap_or_else(std::env::temp_dir);
    base.join("bramble-office").join("contenthub")
}

/// Removes HTML tags and decodes the few entities Commons uses in author names.
fn plain(html: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for ch in html.chars() {
        match ch {
            '<' => tag = true,
            '>' => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&amp;", "&").replace("&quot;", "\"").replace("&#039;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn meta(ii: &serde_json::Value, key: &str) -> String {
    ii["extmetadata"][key]["value"].as_str().map(plain).unwrap_or_default()
}

/// Parses an imageinfo search answer into items.
pub fn parse(json: &[u8]) -> Found {
    let v: serde_json::Value = serde_json::from_slice(json).map_err(|_| "Wikimedia Commons sent an answer that couldn't be read".to_string())?;
    if v.get("error").is_some() {
        return Err(BUSY.into());
    }
    let next = v["continue"]["gsroffset"].as_u64().map(|n| n as usize);
    let mut pages: Vec<&serde_json::Value> = v["query"]["pages"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    pages.sort_by_key(|p| p["index"].as_u64().unwrap_or(0));
    let mut items = Vec::new();
    for p in pages {
        let Some(ii) = p["imageinfo"].get(0) else { continue };
        let mime = ii["mime"].as_str().unwrap_or("");
        if !(mime.starts_with("image/jpeg") || mime.starts_with("image/png") || mime.starts_with("image/svg") || mime.starts_with("image/tiff") || mime.starts_with("image/webp")) {
            continue;
        }
        let Some(thumb) = ii["thumburl"].as_str() else { continue };
        let file = p["title"].as_str().unwrap_or("").to_string();
        let mut title = meta(ii, "ObjectName");
        if title.is_empty() {
            title = file.trim_start_matches("File:").rsplit_once('.').map(|(a, _)| a).unwrap_or(&file).to_string();
        }
        let licence = meta(ii, "LicenseShortName");
        let lic = licence.to_lowercase();
        // only free licences (Commons also hosts a few fair-use or unclear files)
        if !(lic.starts_with("cc") || lic.contains("public domain") || lic == "pd" || lic.starts_with("pd-") || lic.contains("gfdl") || lic.contains("attribution")) {
            continue;
        }
        let credit_needed = meta(ii, "AttributionRequired") != "false" && !(lic.contains("public domain") || lic.starts_with("cc0") || lic.starts_with("pd"));
        items.push(Item {
            title,
            author: meta(ii, "Artist"),
            licence,
            credit_needed,
            page_url: ii["descriptionurl"].as_str().unwrap_or("").to_string(),
            thumb_url: thumb.to_string(),
            w: ii["width"].as_u64().unwrap_or(0) as u32,
            h: ii["height"].as_u64().unwrap_or(0) as u32,
            file,
        });
    }
    Ok((items, next))
}

/// Left out of the suggested feeds (a search for them still finds them): anime-style
/// characters and the like, which Commons' featured drawings include.
const NOT_SUGGESTED: &[&str] = &["anime", "manga", "wikipe", "waifu", "chibi", "kawaii", "hentai", "ecchi", "moe", "cosplay", "vtuber", "loli"];

/// Commons search words for a category.
fn cat_words(c: &Cat) -> String {
    let (_, _, words, drawing) = *c;
    let not: String = NOT_SUGGESTED.iter().map(|w| format!(" -{w}")).collect();
    if drawing {
        format!("{words} incategory:\"{FEATURED}_-_vector\"{not}")
    } else {
        format!("{words} incategory:{FEATURED} filetype:bitmap{not}")
    }
}

/// A suggested-feed item that slipped past the search's exclusions (by its title or file name;
/// drawings of girls too, which are mostly anime-style characters there).
fn not_suggested(it: &Item, drawing: bool) -> bool {
    let t = format!("{} {}", it.title, it.file).to_lowercase();
    let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    words.iter().any(|w| NOT_SUGGESTED.contains(w) || *w == "tan" || (drawing && (*w == "girl" || *w == "girls" || *w == "maid")))
}

fn query_words(q: &str) -> String {
    format!("{} incategory:{FEATURED} filetype:bitmap|drawing", q.trim())
}

const BUSY: &str = "Wikimedia Commons is busy: try again in a moment";

/// One page of results for search words (from `cat_words` / `query_words`); asks again when
/// Commons is busy (it limits how fast one app may ask).
pub fn search(words: &str, offset: usize) -> Found {
    let mut r = search_once(words, offset);
    for wait in [1500, 3000, 6000] {
        if !matches!(&r, Err(e) if e == BUSY) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(wait));
        r = search_once(words, offset);
    }
    // the suggested feeds (category words, not a search) leave out what NOT_SUGGESTED names
    if words.contains(" -anime")
        && let Ok((items, _)) = &mut r
    {
        let drawing = words.contains("_-_vector");
        items.retain(|it| !not_suggested(it, drawing));
    }
    r
}

fn search_once(words: &str, offset: usize) -> Found {
    let off = offset.to_string();
    let limit = PAGE.to_string();
    let mut params = [
        ("action", "query"),
        ("format", "json"),
        ("formatversion", "2"),
        ("generator", "search"),
        ("gsrsearch", words),
        ("gsrnamespace", "6"),
        ("gsrlimit", limit.as_str()),
        ("gsroffset", off.as_str()),
        ("prop", "imageinfo"),
        ("iiprop", "url|size|mime|extmetadata"),
        ("iiextmetadatafilter", "LicenseShortName|Artist|AttributionRequired|ObjectName"),
        ("iiurlwidth", "480"),
    ];
    let found = parse(&get(API, &params)?)?;
    // a free search with nothing featured falls back to Commons' Quality images
    if found.0.is_empty()
        && offset == 0
        && let Some(q) = words.strip_suffix(&format!(" incategory:{FEATURED} filetype:bitmap|drawing"))
    {
        let fallback = format!("{q} incategory:Quality_images");
        params[4].1 = fallback.as_str();
        return parse(&get(API, &params)?);
    }
    Ok(found)
}

/// The picture at up to 2000 pixels wide, as png or jpeg.
pub fn download(item: &Item) -> Result<Picture, String> {
    let width = item.w.clamp(1, 2000).to_string();
    let v: serde_json::Value = serde_json::from_slice(&get(
        API,
        &[("action", "query"), ("format", "json"), ("formatversion", "2"), ("titles", item.file.as_str()), ("prop", "imageinfo"), ("iiprop", "url"), ("iiurlwidth", width.as_str())],
    )?)
    .map_err(|_| "Wikimedia Commons sent an answer that couldn't be read".to_string())?;
    let url = v["query"]["pages"][0]["imageinfo"][0]["thumburl"].as_str().ok_or("This picture isn't available any more")?.to_string();
    let bytes = get(&url, &[])?;
    let ext = match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Png) => "png",
        Ok(image::ImageFormat::Jpeg) => "jpg",
        _ => {
            // (webp and others) re-encode as png, which every app here reads
            let img = image::load_from_memory(&bytes).map_err(|_| "This picture couldn't be read".to_string())?;
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
            return Ok(Picture { bytes: out.into_inner(), name: file_name(item, "png"), credit: item.credit() });
        }
    };
    Ok(Picture { bytes, name: file_name(item, ext), credit: item.credit() })
}

fn file_name(item: &Item, ext: &str) -> String {
    let stem: String = item.title.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).take(60).collect();
    format!("{}.{ext}", stem.trim())
}

fn load_thumb(url: &str) -> Option<image::RgbaImage> {
    let dir = cache_dir();
    let key: u64 = url.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    let path = dir.join(format!("{key:016x}"));
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            let b = get(url, &[]).ok()?;
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(&path, &b);
            b
        }
    };
    Some(image::load_from_memory(&bytes).ok()?.to_rgba8())
}

// ------------------------------------------------------------------ look

fn light() -> bool {
    crate::theme::is_light()
}
/// Tile colours (the Content Hub and the Shape popovers).
pub fn tile_bg() -> Color32 {
    if light() { Color32::from_rgb(242, 242, 247) } else { Color32::from_gray(58) }
}
pub fn tile_hover() -> Color32 {
    if light() { Color32::from_rgb(228, 228, 235) } else { Color32::from_gray(74) }
}
fn tile_edge() -> Color32 {
    if light() { Color32::from_rgb(226, 226, 232) } else { Color32::from_gray(66) }
}
/// The shapes' colour on tiles (dark on light tiles, light on dark ones), shared with the Shape popovers.
pub fn ink() -> Color32 {
    if light() { Color32::from_gray(51) } else { Color32::from_gray(228) }
}
fn window_bg() -> Color32 {
    if light() { Color32::WHITE } else { Color32::from_gray(36) }
}
const TILE_R: u8 = 14;

fn lib_icon(ui: &egui::Ui, name: &str, color: Color32) -> Option<TextureHandle> {
    crate::shapelib::named(name).and_then(|s| crate::shapelib::icon(ui.ctx(), s, color))
}

fn shape_tex(ui: &egui::Ui, key: &str, g: &Glyph, color: Color32) -> Option<TextureHandle> {
    match g {
        Glyph::Path { path, w, h } => crate::shapelib::texture(ui.ctx(), &format!("app-{key}"), path, *w, *h, color),
        _ => None,
    }
}

impl Hub {
    /// Opens the window (Insert > Browse Content Hub: on Browse).
    pub fn show_hub(&mut self) {
        self.show_tab(Tab::Browse);
    }

    /// Opens the window on a tab (the Shape popover's Browse Content Hub: on Shapes).
    pub fn show_tab(&mut self, tab: Tab) {
        if !self.open {
            self.opened_at = None;
        }
        self.open = true;
        self.tab = tab;
        self.shapes_wanted = true;
        if std::env::var_os("BRAMBLE_HUB_PICK").is_some() {
            self.tab = Tab::Photos;
        }
        match std::env::var("BRAMBLE_HUB_TAB").as_deref() {
            Ok("shapes") => self.tab = Tab::Shapes,
            Ok("photos") => self.tab = Tab::Photos,
            Ok("search") => {
                self.query = "lighthouse".into();
                self.searched = self.query.clone();
            }
            _ => {}
        }
    }

    /// Asks the app for its shapes again (after My Shapes changed).
    pub fn refresh_shapes(&mut self) {
        self.shapes_wanted = true;
    }

    /// True once after opening: the app fills [`Hub::basic`] and [`Hub::mine`] then.
    pub fn needs_shapes(&mut self) -> bool {
        std::mem::take(&mut self.shapes_wanted)
    }

    /// Still loading something (automated checks wait for this before their snapshot).
    pub fn busy(&self) -> bool {
        let picking = std::env::var_os("BRAMBLE_HUB_PICK").is_some() && !self.picked_for_check;
        self.open && (self.feeds.values().any(|f| f.loading.is_some()) || self.fetching.is_some() || self.running > 0 || !self.queue.is_empty() || picking)
    }

    /// The feed for search words, started if new.
    fn feed(&mut self, ctx: &egui::Context, words: &str) -> &Feed {
        if !self.feeds.get(words).is_some_and(|f| f.started) {
            self.more(ctx, words, 0);
        }
        &self.feeds[words]
    }

    fn more(&mut self, ctx: &egui::Context, words: &str, offset: usize) {
        let (tx, rx) = channel();
        let (w, c) = (words.to_string(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(search(&w, offset));
            c.request_repaint();
        });
        let f = self.feeds.entry(words.to_string()).or_default();
        f.started = true;
        f.error = None;
        f.loading = Some(rx);
    }

    fn poll(&mut self, ctx: &egui::Context) {
        for f in self.feeds.values_mut() {
            if let Some(rx) = &f.loading
                && let Ok(r) = rx.try_recv()
            {
                f.loading = None;
                match r {
                    Ok((items, next)) => {
                        f.items.extend(items);
                        f.next = next;
                    }
                    Err(e) => f.error = Some(e),
                }
            }
        }
        if self.thumb_tx.is_none() {
            let (tx, rx) = channel();
            self.thumb_tx = Some(tx);
            self.thumb_rx = Some(rx);
        }
        if let Some(rx) = &self.thumb_rx {
            while let Ok((url, img)) = rx.try_recv() {
                self.running = self.running.saturating_sub(1);
                let t = match img {
                    Some(img) => {
                        let size = [img.width() as usize, img.height() as usize];
                        let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                        Thumb::Ready(ctx.load_texture(format!("hub-{url}"), ci, TextureOptions::LINEAR))
                    }
                    None => Thumb::Failed,
                };
                self.thumbs.insert(url, t);
            }
        }
        // six thumbnails at a time, the newest asked first (what's on screen now)
        while self.running < 6 && !self.queue.is_empty() {
            let url = self.queue.pop().unwrap();
            let tx = self.thumb_tx.clone().unwrap();
            let c = ctx.clone();
            self.running += 1;
            std::thread::spawn(move || {
                let img = load_thumb(&url);
                let _ = tx.send((url, img));
                c.request_repaint();
            });
        }
    }

    fn thumb(&mut self, url: &str) -> Option<TextureHandle> {
        match self.thumbs.get(url) {
            Some(Thumb::Ready(t)) => Some(t.clone()),
            Some(Thumb::Failed) => None,
            Some(Thumb::Loading) => {
                // still wanted: move it up the queue
                if let Some(i) = self.queue.iter().position(|u| u == url) {
                    let u = self.queue.remove(i);
                    self.queue.push(u);
                }
                None
            }
            None => {
                self.thumbs.insert(url.to_string(), Thumb::Loading);
                self.queue.push(url.to_string());
                None
            }
        }
    }

    fn insert(&mut self, ctx: &egui::Context, item: &Item) {
        if self.fetching.is_some() {
            return;
        }
        let (tx, rx) = channel();
        let (it, c) = (item.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(download(&it));
            c.request_repaint();
        });
        self.fetching = Some((item.thumb_url.clone(), rx));
    }

    fn say(&mut self, ctx: &egui::Context, s: impl Into<String>) {
        self.note = Some((s.into(), ctx.input(|i| i.time)));
    }

    /// Draws the Content Hub window when it's open; returns what to insert.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Picked> {
        if !self.open {
            return None;
        }
        self.poll(ctx);
        let mut picked = None;
        if let Some((_, rx)) = &self.fetching
            && let Ok(r) = rx.try_recv()
        {
            self.fetching = None;
            self.picked_for_check = true;
            match r {
                Ok(p) => {
                    // the credit goes with the picture: on the clipboard and in the Credits list
                    ctx.copy_text(p.credit.clone());
                    if !self.credits.contains(&p.credit) {
                        self.credits.push(p.credit.clone());
                    }
                    self.say(ctx, "Inserted. Its credit is on the clipboard (and under Credits).");
                    picked = Some(Picked::Image(p));
                }
                Err(e) => self.say(ctx, e),
            }
        }
        // BRAMBLE_HUB_PICK=<n>: insert the n-th photo of the first category (automated checks)
        if let Some(n) = std::env::var("BRAMBLE_HUB_PICK").ok().and_then(|s| s.parse::<usize>().ok())
            && !self.picked_for_check
            && self.fetching.is_none()
            && let Some(it) = self.feeds.get(&cat_words(&PHOTOS[self.cat[1]])).and_then(|f| f.items.get(n)).cloned()
        {
            self.insert(ctx, &it);
        }
        // a search starts a moment after typing stops
        let now = ctx.input(|i| i.time);
        if let Some(t) = self.typed_at {
            if now - t > 0.6 {
                self.typed_at = None;
                self.searched = self.query.trim().to_string();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
        let screen = ctx.content_rect();
        let mut open = self.open;
        // opening: rises into place as it fades in, easing out (a quarter of a second)
        let t0 = *self.opened_at.get_or_insert(now);
        let t = ((now - t0) / 0.26).clamp(0.0, 1.0) as f32;
        let ease = 1.0 - (1.0 - t).powi(3);
        if t < 1.0 {
            ctx.request_repaint();
        }
        let frame = egui::Frame::window(&ctx.global_style()).fill(window_bg()).corner_radius(16.0).inner_margin(egui::Margin::same(16));
        let mut win = egui::Window::new("Content Hub");
        if t < 1.0 {
            // (egui fades the whole window in; it rises with the fade)
            win = win.current_pos(screen.center() + vec2(0.0, (1.0 - ease) * 28.0));
        }
        win
            .id(egui::Id::unique("bramble-content-hub"))
            .title_bar(false)
            .frame(frame)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(screen.center())
            .default_size(vec2(860.0, 620.0).min(screen.size() - vec2(40.0, 80.0)))
            .max_size(screen.size() - vec2(40.0, 60.0))
            .resizable(true)
            .collapsible(false)
            .show(ctx, |ui| {
                if let Some(p) = self.body(ui, &mut open) {
                    picked = Some(p);
                }
            });
        self.open = open;
        picked
    }

    fn body(&mut self, ui: &mut egui::Ui, open: &mut bool) -> Option<Picked> {
        let ctx = ui.ctx().clone();
        let mut picked = None;
        // top: close, Credits, search
        ui.horizontal(|ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
            ui.painter().circle_filled(r.center(), 13.0, if resp.hovered() { tile_hover() } else { tile_bg() });
            let d = 4.5;
            let s = Stroke::new(1.6, ink());
            ui.painter().line_segment([r.center() + vec2(-d, -d), r.center() + vec2(d, d)], s);
            ui.painter().line_segment([r.center() + vec2(d, -d), r.center() + vec2(-d, d)], s);
            if resp.on_hover_text("Close").clicked() {
                *open = false;
            }
            if !self.credits.is_empty() {
                ui.add_space(8.0);
                ui.menu_button(format!("Credits ({})", self.credits.len()), |ui| {
                    ui.set_max_width(520.0);
                    for c in &self.credits {
                        ui.label(egui::RichText::new(c).size(11.0));
                    }
                    if ui.button("Copy All").clicked() {
                        ctx.copy_text(self.credits.join("\n"));
                        ui.close();
                    }
                });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let w = (ui.available_width() * 0.5).clamp(220.0, 380.0);
                let (r, _) = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover());
                ui.painter().rect_filled(r, 17.0, tile_bg());
                // a magnifying glass
                let (c, dim) = (pos2(r.min.x + 17.0, r.center().y - 1.0), Stroke::new(1.6, crate::theme::TEXT_DIM()));
                ui.painter().circle_stroke(c, 5.5, dim);
                ui.painter().line_segment([c + vec2(4.0, 4.0), c + vec2(8.0, 8.0)], dim);
                let te = Rect::from_min_max(pos2(r.min.x + 34.0, r.min.y + 7.0), pos2(r.max.x - 30.0, r.max.y - 7.0));
                let resp = ui.put(te, egui::TextEdit::singleline(&mut self.query).hint_text("Search").frame(egui::Frame::NONE));
                if resp.changed() {
                    self.typed_at = Some(ctx.input(|i| i.time));
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.typed_at = None;
                    self.searched = self.query.trim().to_string();
                }
                if !self.query.is_empty() {
                    let c = Rect::from_center_size(pos2(r.max.x - 16.0, r.center().y), vec2(18.0, 18.0));
                    let cr = ui.interact(c, ui.scope_id().with("hub-clear"), Sense::click());
                    ui.painter().circle_filled(c.center(), 8.0, crate::theme::TEXT_DIM());
                    let s = Stroke::new(1.4, window_bg());
                    ui.painter().line_segment([c.center() + vec2(-3.0, -3.0), c.center() + vec2(3.0, 3.0)], s);
                    ui.painter().line_segment([c.center() + vec2(3.0, -3.0), c.center() + vec2(-3.0, 3.0)], s);
                    if cr.clicked() {
                        self.query.clear();
                        self.searched.clear();
                        self.typed_at = None;
                    }
                }
            });
        });
        ui.add_space(10.0);
        let searching = !self.searched.is_empty();
        if !searching {
            self.tabs(ui);
            ui.add_space(10.0);
            if self.tab != Tab::Browse {
                self.chips(ui);
                ui.add_space(8.0);
            }
        }
        // the footer line first (bottom up), so the content gets exactly the height left: a
        // guess here made the window grow a little every frame
        let now = ctx.input(|i| i.time);
        let text = match &self.note {
            Some((n, t)) if now - t < 6.0 => {
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
                n.clone()
            }
            _ => "Photos and illustrations: Wikimedia Commons (most need a credit) · Shapes: Phosphor Icons (MIT)".into(),
        };
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.label(egui::RichText::new(text).size(11.0).color(crate::theme::TEXT_DIM()));
            ui.add_space(4.0);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                let h = if ui.is_sizing_pass() { 380.0 } else { ui.available_height().max(120.0) };
                egui::ScrollArea::vertical().id_salt(("hub-content", self.tab as u8, self.cat[self.tab as usize], searching)).max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let p = if searching {
                        self.search_page(ui)
                    } else {
                        match self.tab {
                            Tab::Browse => self.browse_page(ui),
                            Tab::Shapes => self.shapes_page(ui),
                            t => {
                                let c = cats(t)[self.cat[t as usize].min(cats(t).len() - 1)];
                                self.pictures(ui, &cat_words(&c), usize::MAX)
                            }
                        }
                    };
                    if p.is_some() {
                        picked = p;
                    }
                });
            });
        });
        let _ = &ctx;
        picked
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        let accent = crate::theme::ACCENT();
        let d = 38.0;
        let label_font = FontId::proportional(15.0);
        let widths: Vec<f32> = TABS
            .iter()
            .map(|(t, label, _)| if *t == self.tab { d + 8.0 + ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), label_font.clone(), Color32::WHITE).size().x) + 16.0 } else { d })
            .collect();
        let gap = 8.0;
        let total: f32 = widths.iter().sum::<f32>() + gap * (TABS.len() - 1) as f32;
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), d), Sense::hover());
        let mut x = row.center().x - total / 2.0;
        for ((t, label, icon), w) in TABS.iter().zip(widths) {
            let r = Rect::from_min_size(pos2(x, row.min.y), vec2(w, d));
            x += w + gap;
            let resp = ui.interact(r, ui.scope_id().with(("hub-tab", *label)), Sense::click());
            let on = *t == self.tab;
            let fill = if on { accent } else if resp.hovered() { tile_hover() } else { tile_bg() };
            ui.painter().rect_filled(r, d / 2.0, fill);
            let col = if on { Color32::WHITE } else { ink() };
            let ic = Rect::from_center_size(pos2(r.min.x + d / 2.0 + if on { 6.0 } else { 0.0 }, r.center().y), vec2(19.0, 19.0));
            if let Some(tex) = lib_icon(ui, icon, col) {
                ui.painter().image(tex.id(), ic, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            if on {
                ui.painter().text(pos2(ic.max.x + 8.0, r.center().y), egui::Align2::LEFT_CENTER, *label, label_font.clone(), Color32::WHITE);
            }
            if resp.on_hover_text(*label).clicked() {
                self.tab = *t;
            }
        }
    }

    /// The category chips of the tab (a row that scrolls sideways).
    fn chips(&mut self, ui: &mut egui::Ui) {
        let mut list: Vec<(String, String)> = Vec::new();
        if self.tab == Tab::Shapes {
            list.push(("All".into(), "Star".into()));
            list.push(("Basic".into(), "Square".into()));
            for c in shapelib::categories() {
                let icon = match c {
                    "Activities" => "Soccer ball",
                    "Arrows" => "Arrow right",
                    "Communication" => "Chat",
                    "Media" => "Music note",
                    "Nature" => "Leaf",
                    "Objects" => "Cube",
                    "People" => "User",
                    "Places" => "House",
                    "Symbols" => "Heart",
                    "Technology" => "Cpu",
                    "Work" => "Briefcase",
                    _ => "Shapes",
                };
                list.push((c.to_string(), icon.to_string()));
            }
            if !self.mine.is_empty() {
                list.push(("My Shapes".into(), "User".into()));
            }
        } else {
            for (name, icon, _, _) in cats(self.tab) {
                list.push((name.to_string(), icon.to_string()));
            }
        }
        let accent = crate::theme::ACCENT();
        let font = FontId::proportional(14.0);
        let tab = self.tab as usize;
        egui::ScrollArea::horizontal().id_salt(("hub-chips", tab)).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for (i, (name, icon)) in list.iter().enumerate() {
                    let tw = ui.fonts_mut(|f| f.layout_no_wrap(name.clone(), font.clone(), Color32::WHITE).size().x);
                    let (r, resp) = ui.allocate_exact_size(vec2(tw + 52.0, 32.0), Sense::click());
                    let on = self.cat[tab] == i;
                    if on {
                        ui.painter().rect_filled(r, 16.0, accent);
                    } else {
                        ui.painter().rect(r, 16.0, if resp.hovered() { tile_hover() } else { window_bg() }, Stroke::new(1.0, tile_edge()), egui::StrokeKind::Inside);
                    }
                    let col = if on { Color32::WHITE } else { ink() };
                    if let Some(t) = lib_icon(ui, icon, col) {
                        ui.painter().image(t.id(), Rect::from_center_size(pos2(r.min.x + 22.0, r.center().y), vec2(16.0, 16.0)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    ui.painter().text(pos2(r.min.x + 38.0, r.center().y), egui::Align2::LEFT_CENTER, name, font.clone(), col);
                    if resp.clicked() {
                        self.cat[tab] = i;
                    }
                }
            });
        });
    }

    fn header(&mut self, ui: &mut egui::Ui, title: &str, icon: &str, show_all: Option<(Tab, usize)>) {
        ui.add_space(6.0);
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
        if let Some(t) = lib_icon(ui, icon, crate::theme::TEXT_DIM()) {
            ui.painter().image(t.id(), Rect::from_center_size(pos2(row.min.x + 10.0, row.center().y), vec2(20.0, 20.0)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        ui.painter().text(pos2(row.min.x + 28.0, row.center().y), egui::Align2::LEFT_CENTER, title, FontId::proportional(19.0), crate::theme::TEXT());
        if let Some((tab, cat)) = show_all {
            let g = ui.painter().layout_no_wrap("Show All".into(), FontId::proportional(14.0), crate::theme::ACCENT());
            let r = Rect::from_min_size(pos2(row.max.x - g.size().x - 4.0, row.center().y - g.size().y / 2.0), g.size());
            let resp = ui.interact(r.expand(4.0), ui.scope_id().with(("hub-all", title)), Sense::click());
            ui.painter().galley(r.min, g, crate::theme::ACCENT());
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.tab = tab;
                self.cat[tab as usize] = cat;
            }
        }
        ui.add_space(6.0);
    }

    fn browse_page(&mut self, ui: &mut egui::Ui) -> Option<Picked> {
        let mut picked = None;
        self.header(ui, "Shapes", "Shapes", Some((Tab::Shapes, 0)));
        let basic: Vec<usize> = (0..self.basic.len()).collect();
        if let Some(p) = self.shape_tiles(ui, &basic, &[], 1) {
            picked = Some(p);
        }
        for (tab, title, icon, cat, rows) in [(Tab::Photos, "Photos", "Image", 3usize, 2usize), (Tab::Illustrations, "Illustrations", "Paint brush", 0, 1), (Tab::Backgrounds, "Backgrounds", "Squares four", 0, 1)] {
            ui.add_space(10.0);
            self.header(ui, title, icon, Some((tab, cat)));
            if let Some(p) = self.pictures(ui, &cat_words(&cats(tab)[cat]), rows) {
                picked = Some(p);
            }
        }
        picked
    }

    fn shapes_page(&mut self, ui: &mut egui::Ui) -> Option<Picked> {
        let lib_cats = shapelib::categories();
        let chip = self.cat[Tab::Shapes as usize];
        let basic: Vec<usize> = (0..self.basic.len()).collect();
        let mine: Vec<usize> = (0..self.mine.len()).collect();
        let mut picked = None;
        match chip {
            0 => {
                self.header(ui, "Basic", "Square", None);
                picked = picked.or(self.shape_tiles(ui, &basic, &[], usize::MAX));
                for c in &lib_cats {
                    let list: Vec<&'static LibShape> = shapelib::shapes().iter().filter(|s| s.category == *c).collect();
                    ui.add_space(8.0);
                    self.header(ui, c, "Shapes", None);
                    picked = picked.or(self.lib_tiles(ui, &list));
                }
                if !mine.is_empty() {
                    ui.add_space(8.0);
                    self.header(ui, "My Shapes", "User", None);
                    picked = picked.or(self.mine_tiles(ui, &mine));
                }
            }
            1 => picked = self.shape_tiles(ui, &basic, &[], usize::MAX),
            n if n - 2 < lib_cats.len() => {
                let c = lib_cats[n - 2];
                let list: Vec<&'static LibShape> = shapelib::shapes().iter().filter(|s| s.category == c).collect();
                picked = self.lib_tiles(ui, &list);
            }
            _ => picked = self.mine_tiles(ui, &mine),
        }
        picked
    }

    fn search_page(&mut self, ui: &mut egui::Ui) -> Option<Picked> {
        let q = self.searched.clone();
        let lower = q.to_lowercase();
        let mut picked = None;
        let basic: Vec<usize> = (0..self.basic.len()).filter(|&i| self.basic[i].name.to_lowercase().contains(&lower)).collect();
        let mine: Vec<usize> = (0..self.mine.len()).filter(|&i| self.mine[i].name.to_lowercase().contains(&lower)).collect();
        let lib = shapelib::search(&q);
        if !basic.is_empty() || !mine.is_empty() || !lib.is_empty() {
            self.header(ui, "Shapes", "Shapes", None);
            picked = picked.or(self.shape_tiles(ui, &basic, &mine, usize::MAX));
            picked = picked.or(self.lib_tiles(ui, &lib));
            ui.add_space(10.0);
        }
        self.header(ui, "Photos and Illustrations", "Image", None);
        picked = picked.or(self.pictures(ui, &query_words(&q), usize::MAX));
        picked
    }

    /// Square tiles for the grid; how many per row and their size.
    fn grid(ui: &egui::Ui) -> (usize, f32, f32) {
        let gap = 10.0;
        let w = ui.available_width();
        let cols = ((w + gap) / (88.0 + gap)).floor().max(3.0) as usize;
        ((cols), (w - gap * (cols as f32 - 1.0)) / cols as f32 - 0.5, gap)
    }

    fn tile(ui: &mut egui::Ui, r: Rect, id: egui::Id) -> egui::Response {
        let resp = ui.interact(r, id, Sense::click());
        ui.painter().rect(r, TILE_R, if resp.hovered() { tile_hover() } else { tile_bg() }, Stroke::new(1.0, tile_edge()), egui::StrokeKind::Inside);
        resp
    }

    /// Lays out `n` square tiles, `rows` rows at most; calls `draw` for each.
    fn squares(ui: &mut egui::Ui, n: usize, rows: usize, mut draw: impl FnMut(&mut egui::Ui, usize, Rect)) {
        let (cols, size, gap) = Self::grid(ui);
        let n = n.min(cols.saturating_mul(rows));
        let lines = n.div_ceil(cols);
        if lines == 0 {
            return;
        }
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), lines as f32 * (size + gap) - gap), Sense::hover());
        for i in 0..n {
            let (row, col) = (i / cols, i % cols);
            let r = Rect::from_min_size(pos2(area.min.x + col as f32 * (size + gap), area.min.y + row as f32 * (size + gap)), vec2(size, size));
            if ui.is_rect_visible(r) {
                draw(ui, i, r);
            }
        }
    }

    /// The app's basic shapes (by index) then the given My Shapes.
    fn shape_tiles(&mut self, ui: &mut egui::Ui, basic: &[usize], mine: &[usize], rows: usize) -> Option<Picked> {
        let mut picked = None;
        let all: Vec<(bool, usize)> = basic.iter().map(|&i| (false, i)).chain(mine.iter().map(|&i| (true, i))).collect();
        Self::squares(ui, all.len(), rows, |ui, k, r| {
            let (is_mine, i) = all[k];
            let s = if is_mine { &self.mine[i] } else { &self.basic[i] };
            let resp = Self::tile(ui, r, ui.scope_id().with(("hub-app", is_mine, i)));
            Self::draw_glyph(ui, r, &s.key, &s.name, &s.glyph);
            if is_mine {
                resp.context_menu(|ui| {
                    if ui.button("Insert").clicked() {
                        picked = Some(Picked::App(s.key.clone()));
                        ui.close();
                    }
                    if ui.button("Delete from My Shapes").clicked() {
                        picked = Some(Picked::Remove(s.key.clone()));
                        ui.close();
                    }
                });
            }
            if resp.on_hover_text(&s.name).clicked() {
                picked = Some(Picked::App(s.key.clone()));
            }
        });
        picked
    }

    fn mine_tiles(&mut self, ui: &mut egui::Ui, mine: &[usize]) -> Option<Picked> {
        if mine.is_empty() {
            ui.label(egui::RichText::new("Format > Shapes and Lines > Save to My Shapes adds shapes here.").color(crate::theme::TEXT_DIM()));
            return None;
        }
        self.shape_tiles(ui, &[], mine, usize::MAX)
    }

    fn draw_glyph(ui: &egui::Ui, r: Rect, key: &str, name: &str, g: &Glyph) {
        let p = ui.painter();
        let inner = r.shrink(r.width() * 0.24);
        match g {
            Glyph::Text => {
                p.text(r.center(), egui::Align2::CENTER_CENTER, "Text", FontId::proportional((r.width() * 0.2).clamp(13.0, 18.0)), ink());
            }
            Glyph::Line { arrow } => {
                let (a, b) = (pos2(inner.min.x, inner.max.y), pos2(inner.max.x, inner.min.y));
                p.line_segment([a, b], Stroke::new(1.3, ink()));
                if *arrow {
                    let dir = (b - a).normalized();
                    let n = vec2(-dir.y, dir.x);
                    let tip = b;
                    let back = tip - dir * 9.0;
                    p.add(egui::Shape::convex_polygon(vec![tip, back + n * 3.5, back - n * 3.5], ink(), Stroke::NONE));
                }
            }
            Glyph::Curve => {
                let pts: Vec<egui::Pos2> = (0..=24)
                    .map(|i| {
                        let a = std::f32::consts::FRAC_PI_2 * i as f32 / 24.0;
                        pos2(inner.min.x + (1.0 - a.cos()) * inner.width(), inner.min.y + (1.0 - a.sin()) * inner.height())
                    })
                    .collect();
                p.add(egui::Shape::line(pts, Stroke::new(1.3, ink())));
                p.circle_filled(pos2(inner.min.x, inner.max.y), 2.5, ink());
                p.circle_filled(pos2(inner.max.x, inner.min.y), 2.5, ink());
            }
            Glyph::Label => {
                let g = p.layout(name.to_string(), FontId::proportional(12.0), ink(), r.width() - 12.0);
                p.galley(r.center() - g.size() / 2.0, g, ink());
            }
            Glyph::Path { .. } => {
                if let Some(t) = shape_tex(ui, key, g, ink()) {
                    p.image(t.id(), inner, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
            }
        }
    }

    fn lib_tiles(&mut self, ui: &mut egui::Ui, list: &[&'static LibShape]) -> Option<Picked> {
        let mut picked = None;
        Self::squares(ui, list.len(), usize::MAX, |ui, i, r| {
            let s = list[i];
            let resp = Self::tile(ui, r, ui.scope_id().with(("hub-lib", s.category, s.name, i)));
            if let Some(t) = crate::shapelib::icon(ui.ctx(), s, ink()) {
                ui.painter().image(t.id(), r.shrink(r.width() * 0.24), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            if resp.on_hover_text(s.name).clicked() {
                picked = Some(Picked::Library(s));
            }
        });
        picked
    }

    /// Photos in justified rows (as on the Mac), `rows` rows at most; loads more at the end.
    fn pictures(&mut self, ui: &mut egui::Ui, words: &str, rows: usize) -> Option<Picked> {
        let ctx = ui.ctx().clone();
        let feed = self.feed(&ctx, words);
        let items: Vec<Item> = feed.items.clone();
        let (loading, next, error) = (feed.loading.is_some(), feed.next, feed.error.clone());
        if let Some(e) = error {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(e).color(crate::theme::TEXT_DIM()));
                if ui.button("Try Again").clicked() {
                    self.more(&ctx, words, 0);
                }
            });
            return None;
        }
        if items.is_empty() {
            ui.horizontal(|ui| {
                if loading {
                    ui.spinner();
                    ui.label(egui::RichText::new("Loading…").color(crate::theme::TEXT_DIM()));
                } else {
                    ui.label(egui::RichText::new("Nothing found").color(crate::theme::TEXT_DIM()));
                }
            });
            return None;
        }
        let gap = 10.0;
        let target = 150.0;
        // (a little room for the scroll bar)
        let w = ui.available_width() - 4.0;
        // rows: as many pictures as fill the width at the target height, then scaled to fit
        let mut lines: Vec<(Vec<usize>, f32)> = Vec::new();
        let mut cur: Vec<usize> = Vec::new();
        let mut sum = 0.0;
        for (i, it) in items.iter().enumerate() {
            cur.push(i);
            sum += it.aspect();
            let width = sum * target + gap * (cur.len() as f32 - 1.0);
            if width >= w {
                let h = (w - gap * (cur.len() as f32 - 1.0)) / sum;
                lines.push((std::mem::take(&mut cur), h));
                sum = 0.0;
            }
        }
        let last_full = next.is_none() || rows != usize::MAX;
        if !cur.is_empty() && (last_full || lines.is_empty()) {
            lines.push((cur, target));
        }
        lines.truncate(rows);
        let mut picked = None;
        let fetching = self.fetching.as_ref().map(|(u, _)| u.clone());
        for (line, h) in &lines {
            let (row, _) = ui.allocate_exact_size(vec2(w, *h), Sense::hover());
            let mut x = row.min.x;
            for &i in line {
                let it = &items[i];
                let r = Rect::from_min_size(pos2(x, row.min.y), vec2(it.aspect() * h, *h));
                x += r.width() + gap;
                if !ui.is_rect_visible(r) {
                    continue;
                }
                let resp = ui.interact(r, ui.scope_id().with(("hub-pic", &it.thumb_url)), Sense::click());
                ui.painter().rect_filled(r, 10.0, tile_bg());
                if let Some(t) = self.thumb(&it.thumb_url) {
                    // cover the tile (the thumbnail's own aspect may differ a little)
                    let s = t.size_vec2();
                    let k = (r.width() / s.x).max(r.height() / s.y);
                    let (u, v) = ((r.width() / (s.x * k)) / 2.0, (r.height() / (s.y * k)) / 2.0);
                    let uv = Rect::from_min_max(pos2(0.5 - u, 0.5 - v), pos2(0.5 + u, 0.5 + v));
                    egui::Image::new(&t).uv(uv).corner_radius(CornerRadius::same(10)).paint_at(ui, r);
                }
                if resp.hovered() {
                    ui.painter().rect_stroke(r, 10.0, Stroke::new(3.0, crate::theme::ACCENT()), egui::StrokeKind::Inside);
                }
                if fetching.as_deref() == Some(it.thumb_url.as_str()) {
                    ui.painter().rect_filled(r, 10.0, Color32::from_black_alpha(90));
                    egui::Spinner::new().size(28.0).color(Color32::WHITE).paint_at(ui, Rect::from_center_size(r.center(), vec2(28.0, 28.0)));
                }
                let tip = format!("{}\n{}{}", it.title, if it.author.is_empty() { String::new() } else { format!("{} · ", it.author) }, it.licence);
                if resp.on_hover_text(tip).clicked() {
                    self.insert(&ctx, it);
                }
            }
            ui.add_space(gap);
        }
        // the end of the list in sight: the next page
        if rows == usize::MAX {
            if loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                });
            } else if let Some(n) = next {
                let (r, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
                if ui.is_rect_visible(r) {
                    self.more(&ctx, words, n);
                }
            }
        }
        let _ = &mut picked;
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_results_and_skips_unfree() {
        let json = br#"{"continue":{"gsroffset":40},"query":{"pages":[
          {"title":"File:B.jpg","index":2,"imageinfo":[{"mime":"image/jpeg","width":4000,"height":3000,"thumburl":"https://t/b.jpg","descriptionurl":"https://c/B","extmetadata":{"LicenseShortName":{"value":"CC BY-SA 4.0"},"Artist":{"value":"<a href=\"x\">Ann &amp; Bo</a>"},"ObjectName":{"value":"Bee"}}}]},
          {"title":"File:A.png","index":1,"imageinfo":[{"mime":"image/png","width":10,"height":10,"thumburl":"https://t/a.png","descriptionurl":"https://c/A","extmetadata":{"LicenseShortName":{"value":"Public domain"},"AttributionRequired":{"value":"false"}}}]},
          {"title":"File:C.jpg","index":3,"imageinfo":[{"mime":"image/jpeg","width":1,"height":1,"thumburl":"https://t/c.jpg","extmetadata":{"LicenseShortName":{"value":"Fair use"}}}]},
          {"title":"File:D.ogv","index":4,"imageinfo":[{"mime":"video/ogg","thumburl":"https://t/d.jpg","extmetadata":{"LicenseShortName":{"value":"CC0"}}}]}
        ]}}"#;
        let (items, next) = parse(json).unwrap();
        assert_eq!(next, Some(40));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "A");
        assert!(!items[0].credit_needed);
        assert_eq!(items[1].author, "Ann & Bo");
        assert!(items[1].credit_needed);
        assert_eq!(items[1].credit(), "“Bee” by Ann & Bo, CC BY-SA 4.0, via Wikimedia Commons (https://c/B)");
        assert_eq!(file_name(&items[1], "jpg"), "Bee.jpg");
    }

    #[test]
    fn suggested_feeds_leave_out_anime() {
        let it = |title: &str, file: &str| Item { title: title.into(), file: file.into(), author: String::new(), licence: "CC0".into(), credit_needed: false, page_url: String::new(), thumb_url: String::new(), w: 1, h: 1 };
        assert!(not_suggested(&it("Anime Girl", "File:Anime Girl.svg"), true));
        assert!(not_suggested(&it("Wikipe-tan full length", "File:Wikipe-tan full length.svg"), true));
        assert!(!not_suggested(&it("Tanzania map-fr", "File:Tanzania map-fr.svg"), true));
        assert!(!not_suggested(&it("Girl with a Pearl Earring", "File:Girl.jpg"), false));
        assert!(cat_words(&ILLUSTRATIONS[0]).contains(" -anime"));
        assert!(!query_words("anime").contains(" -anime"));
    }

    #[test]
    fn every_icon_exists() {
        let mut names: Vec<&str> = TABS.iter().map(|t| t.2).collect();
        for c in PHOTOS.iter().chain(ILLUSTRATIONS).chain(BACKGROUNDS) {
            names.push(c.1);
        }
        names.extend(["Star", "Square", "Soccer ball", "Arrow right", "Chat", "Music note", "Leaf", "Cube", "User", "House", "Heart", "Cpu", "Briefcase", "Shapes"]);
        for n in names {
            assert!(crate::shapelib::named(n).is_some(), "no shape named {n}");
        }
    }

    #[test]
    #[ignore = "needs the internet"]
    fn live_search_and_download() {
        let (items, _) = search(&cat_words(&PHOTOS[0]), 0).unwrap();
        assert!(items.len() > 10, "{}", items.len());
        let p = download(&items[0]).unwrap();
        assert!(p.bytes.len() > 10_000);
        assert!(load_thumb(&items[1].thumb_url).is_some());
        // every category has pictures, and a search finds some
        for c in PHOTOS.iter().chain(ILLUSTRATIONS).chain(BACKGROUNDS) {
            let n = search(&cat_words(c), 0).unwrap().0.len();
            assert!(n >= 8, "{}: {n}", c.0);
        }
        assert!(!search(&query_words("lighthouse"), 0).unwrap().0.is_empty());
    }
}
