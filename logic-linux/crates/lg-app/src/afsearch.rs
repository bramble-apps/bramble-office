//! The All Files Browser's search, as Logic's: condition rows under the search field (+ adds
//! one, up to ten; a row's – removes it; the – beside the field hides them all and + brings
//! them back) for Name, Comment, File Type, Format, Length, Modified Date, Sample Rate, Size and
//! Bit Depth, each with Logic's operators; the magnifying glass's menu of recent searches with
//! Clear Search History; the field's Clear button. Also the folder bookmarks (a folder's
//! shortcut menu ▸ Bookmark "name"; the Bookmark button lists them) and the list view's columns
//! (Name, Date Created, Size, Type: a header click sorts).

use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Align2, Rect, Sense, Stroke};

use crate::allfiles::FileKind;
use crate::app::App;
use crate::look;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    Comment,
    FileType,
    Format,
    Length,
    Modified,
    SampleRate,
    Size,
    BitDepth,
}

pub const FIELDS: [(Field, &str); 9] = [(Field::Comment, "Comment"), (Field::FileType, "File Type"), (Field::Format, "Format"), (Field::Length, "Length"), (Field::Modified, "Modified Date"), (Field::Name, "Name"), (Field::SampleRate, "Sample Rate"), (Field::Size, "Size"), (Field::BitDepth, "Bit Depth")];

const TEXT_OPS: &[&str] = &["contains", "does not contain", "is", "is not", "starts with", "ends with"];
const IS_OPS: &[&str] = &["is", "is not"];
const NUM_OPS: &[&str] = &["is", "is not", "less than", "greater than"];
const DATE_OPS: &[&str] = &["is", "is not", "after", "before"];
pub const FILE_TYPES: &[(&str, FileKind)] = &[("Audio File", FileKind::Audio), ("MIDI File", FileKind::Midi), ("Logic Project", FileKind::LogicProject), ("GarageBand Project", FileKind::GarageBand), ("Movie", FileKind::Movie), ("Folder", FileKind::Folder)];
pub const FORMATS: &[(&str, &[&str])] = &[("AIFF", &["aif", "aiff"]), ("WAVE", &["wav", "wave"]), ("CAF", &["caf"]), ("MP3", &["mp3"]), ("AAC", &["m4a", "aac"]), ("Apple Lossless", &["m4a"]), ("FLAC", &["flac"]), ("Ogg Vorbis", &["ogg"]), ("MIDI", &["mid", "midi"])];
pub const RATES: &[u32] = &[22050, 32000, 44100, 48000, 88200, 96000, 176400, 192000];
pub const DEPTHS: &[u32] = &[8, 16, 20, 24, 32];
const LEN_UNITS: &[(&str, f64)] = &[("seconds", 1.0), ("minutes", 60.0), ("hours", 3600.0)];
const SIZE_UNITS: &[(&str, f64)] = &[("KB", 1024.0), ("MB", 1024.0 * 1024.0), ("GB", 1024.0 * 1024.0 * 1024.0)];
pub const MAX_CONDITIONS: usize = 10;

/// One condition row.
#[derive(Clone, Debug, PartialEq)]
pub struct Cond {
    pub field: Field,
    pub op: usize,
    pub text: String,
    /// A number (Length, Size) or a menu index (File Type, Format, Sample Rate, Bit Depth).
    pub num: f64,
    pub unit: usize,
}

impl Cond {
    pub fn new(field: Field) -> Cond {
        let num = match field {
            Field::SampleRate => 3.0,
            Field::BitDepth => 1.0,
            _ => 0.0,
        };
        Cond { field, op: 0, text: String::new(), num, unit: 0 }
    }
}

pub fn ops(f: Field) -> &'static [&'static str] {
    match f {
        Field::Name | Field::Comment => TEXT_OPS,
        Field::FileType | Field::Format => IS_OPS,
        Field::Modified => DATE_OPS,
        _ => NUM_OPS,
    }
}

fn text_op(op: usize, hay: &str, needle: &str) -> bool {
    let (h, n) = (hay.to_lowercase(), needle.to_lowercase());
    match op {
        0 => h.contains(&n),
        1 => !h.contains(&n),
        2 => h == n,
        3 => h != n,
        4 => h.starts_with(&n),
        _ => h.ends_with(&n),
    }
}

fn num_op(op: usize, v: f64, want: f64) -> bool {
    let eq = (v - want).abs() <= want.abs() * 0.005 + 0.5;
    match op {
        0 => eq,
        1 => !eq,
        2 => v < want,
        _ => v > want,
    }
}

/// An AIFF file made as an Apple Loop (its "basc" chunk): AIFF searches leave these out.
pub fn is_apple_loop(p: &Path) -> bool {
    let Ok(b) = read_head(p, 65_536) else { return false };
    b.len() > 12 && &b[0..4] == b"FORM" && b.windows(4).any(|w| w == b"basc")
}

fn read_head(p: &Path, n: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(p)?;
    let mut b = vec![0u8; n];
    let got = f.read(&mut b)?;
    b.truncate(got);
    Ok(b)
}

/// A file's comment: a WAVE's INFO ICMT or Broadcast Wave description, an AIFF's COMT/ANNO.
pub fn comment(p: &Path) -> String {
    let Ok(b) = read_head(p, 262_144) else { return String::new() };
    let find = |tag: &[u8], skip: usize, le: bool| -> Option<String> {
        let i = b.windows(tag.len()).position(|w| w == tag)?;
        let lb: [u8; 4] = b.get(i + 4..i + 8)?.try_into().ok()?;
        let len = if le { u32::from_le_bytes(lb) } else { u32::from_be_bytes(lb) } as usize;
        let s = b.get(i + 8 + skip..(i + 8 + len.min(4096)).min(b.len()))?;
        Some(String::from_utf8_lossy(s).trim_matches(char::from(0)).trim().to_string())
    };
    if b.starts_with(b"RIFF") {
        find(b"ICMT", 0, true).filter(|s| !s.is_empty()).or_else(|| find(b"bext", 0, true).map(|s| s.chars().take(256).collect::<String>().trim_matches(char::from(0)).to_string())).unwrap_or_default()
    } else if b.starts_with(b"FORM") {
        find(b"ANNO", 0, false).or_else(|| find(b"COMT", 10, false)).unwrap_or_default()
    } else {
        String::new()
    }
}

/// Days since 1970 for "YYYY-MM-DD".
pub fn parse_date(s: &str) -> Option<i64> {
    let mut it = s.trim().split(['-', '/', '.']);
    let (y, m, d): (i64, i64, i64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // (days from civil, Howard Hinnant's)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Whether file `p` (of `kind`) meets condition `c`.
pub fn matches(c: &Cond, p: &Path, kind: FileKind) -> bool {
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match c.field {
        Field::Name => c.text.is_empty() || text_op(c.op, &name, &c.text),
        Field::Comment => c.text.is_empty() || text_op(c.op, &comment(p), &c.text),
        Field::FileType => {
            let want = FILE_TYPES.get(c.num as usize).map(|f| f.1);
            (Some(kind) == want) == (c.op == 0)
        }
        Field::Format => {
            let (fname, exts) = FORMATS.get(c.num as usize).copied().unwrap_or(FORMATS[0]);
            // (AIFF finds plain AIFF files, not Apple Loops)
            let is = exts.contains(&ext.as_str()) && !(fname == "AIFF" && is_apple_loop(p));
            is == (c.op == 0)
        }
        Field::Length => {
            let Some(l) = (kind == FileKind::Audio).then(|| crate::soundlib::audio_seconds(p)).flatten() else { return false };
            num_op(c.op, l, c.num * LEN_UNITS.get(c.unit).map_or(1.0, |u| u.1))
        }
        Field::Size => {
            let Some(len) = std::fs::metadata(p).ok().map(|m| m.len() as f64) else { return false };
            let want = c.num * SIZE_UNITS.get(c.unit).map_or(1024.0, |u| u.1);
            match c.op {
                0 => (len - want).abs() < SIZE_UNITS.get(c.unit).map_or(1024.0, |u| u.1),
                1 => (len - want).abs() >= SIZE_UNITS.get(c.unit).map_or(1024.0, |u| u.1),
                2 => len < want,
                _ => len > want,
            }
        }
        Field::Modified => {
            let Some(want) = parse_date(&c.text) else { return true };
            let Some(day) = std::fs::metadata(p).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| (d.as_secs() / 86_400) as i64) else { return false };
            match c.op {
                0 => day == want,
                1 => day != want,
                2 => day > want,
                _ => day < want,
            }
        }
        Field::SampleRate | Field::BitDepth => {
            let Some(info) = (kind == FileKind::Audio).then(|| crate::projaudio::file_info(p)).flatten() else { return false };
            let (v, want) = if c.field == Field::SampleRate { (info.rate as f64, RATES.get(c.num as usize).copied().unwrap_or(44100) as f64) } else { (info.bits as f64, DEPTHS.get(c.num as usize).copied().unwrap_or(16) as f64) };
            match c.op {
                0 => v == want,
                1 => v != want,
                2 => v < want,
                _ => v > want,
            }
        }
    }
}

// ---- remembered: recent searches, bookmarks ---------------------------------------------------

#[derive(Default)]
pub struct State {
    loaded: bool,
    pub recent: Vec<String>,
    pub bookmarks: Vec<PathBuf>,
    /// The Bookmark button: the list shows the bookmarks.
    pub showing_bookmarks: bool,
    /// The list's sort: column (0 name, 1 date, 2 size, 3 type) and descending.
    pub sort: (usize, bool),
}

fn file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("all-files.txt"))
}

pub fn ensure(app: &mut App) {
    if app.afs.loaded {
        return;
    }
    app.afs.loaded = true;
    if std::env::var_os("LG_SNAP").is_some() {
        return;
    }
    let Some(t) = file().and_then(|f| std::fs::read_to_string(f).ok()) else { return };
    for l in t.lines() {
        if let Some(s) = l.strip_prefix("recent=") {
            app.afs.recent.push(s.to_string());
        } else if let Some(s) = l.strip_prefix("bookmark=") {
            app.afs.bookmarks.push(PathBuf::from(s));
        }
    }
}

fn save(app: &App) {
    if std::env::var_os("LG_SNAP").is_some() {
        return;
    }
    let Some(f) = file() else { return };
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut t = String::new();
    for r in &app.afs.recent {
        t.push_str(&format!("recent={r}\n"));
    }
    for b in &app.afs.bookmarks {
        t.push_str(&format!("bookmark={}\n", b.display()));
    }
    let _ = std::fs::write(f, t);
}

/// Remembers a search term (most recent first, ten kept).
pub fn remember(recent: &mut Vec<String>, term: &str) {
    let t = term.trim();
    if t.is_empty() {
        return;
    }
    recent.retain(|x| x != t);
    recent.insert(0, t.to_string());
    recent.truncate(10);
}

/// Adds or (bookmarked) removes a folder's bookmark.
pub fn toggle_bookmark(list: &mut Vec<PathBuf>, d: &Path) {
    if let Some(i) = list.iter().position(|b| b == d) {
        list.remove(i);
    } else {
        list.push(d.to_path_buf());
    }
}

pub fn bookmark_label(d: &Path) -> String {
    format!("Bookmark “{}”", d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| d.display().to_string()))
}

/// A folder's shortcut menu item: Bookmark "name" (ticked when bookmarked: again removes it).
pub fn bookmark_item(app: &mut App, ui: &mut egui::Ui, d: &Path) {
    ensure(app);
    let on = app.afs.bookmarks.iter().any(|b| b == d);
    if ui.add(egui::Button::selectable(on, bookmark_label(d))).clicked() {
        toggle_bookmark(&mut app.afs.bookmarks, d);
        save(app);
        ui.close();
    }
}

// ---- the search field, its menus and the condition rows ----------------------------------------

/// The search row: the magnifying glass (recent searches), the field, its Clear button, then
/// − (hide the conditions) and + (add one). Returns the rect used.
pub fn search_row(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    ensure(app);
    use crate::glyphs::ph;
    let p = ui.painter().clone();
    let field = Rect::from_min_max(r.min, pos2(r.max.x - 56.0, r.max.y));
    p.rect_filled(field, 6.0, look::group_bg());
    // the magnifying glass: recent search terms, Clear Search History
    let glass = Rect::from_min_size(field.min, vec2(26.0, field.height()));
    crate::glyphs::paint(&p, glass, ph::MAGNIFYING_GLASS, 12.0, look::text_dim());
    let gresp = ui.interact(glass, ui.scope_id().with("afs-glass"), Sense::click()).on_hover_text("Recent searches");
    egui::Popup::menu(&gresp).show(|ui| {
        ui.set_min_width(180.0);
        ui.label(egui::RichText::new("Recent Searches").color(look::text_dim()));
        let recent = app.afs.recent.clone();
        if recent.is_empty() {
            ui.add_enabled(false, egui::Button::new("No Recent Searches"));
        }
        for t in recent {
            if ui.button(&t).clicked() {
                app.af.search = t;
                ui.close();
            }
        }
        ui.separator();
        if ui.add_enabled(!app.afs.recent.is_empty(), egui::Button::new("Clear Search History")).clicked() {
            app.afs.recent.clear();
            save(app);
            ui.close();
        }
    });
    let te_r = Rect::from_min_max(pos2(glass.max.x, field.min.y), pos2(field.max.x - 22.0, field.max.y));
    let te = ui.put(te_r, egui::TextEdit::singleline(&mut app.af.search).hint_text("Search").font(look::regular(12.5)).frame(egui::Frame::NONE));
    if te.lost_focus() && !app.af.search.trim().is_empty() {
        let t = app.af.search.clone();
        remember(&mut app.afs.recent, &t);
        save(app);
    }
    // Clear
    if !app.af.search.is_empty() {
        let clear = Rect::from_min_size(pos2(field.max.x - 22.0, field.min.y), vec2(22.0, field.height()));
        crate::glyphs::paint(&p, clear, crate::glyphs::ph_fill::X_CIRCLE, 13.0, look::text_dim());
        if ui.interact(clear, ui.scope_id().with("afs-clear"), Sense::click()).on_hover_text("Clear").clicked() {
            app.af.search.clear();
        }
    }
    // − and +
    let minus = Rect::from_min_size(pos2(field.max.x + 4.0, r.min.y), vec2(24.0, r.height()));
    let plus = Rect::from_min_size(pos2(minus.max.x + 2.0, r.min.y), vec2(24.0, r.height()));
    let shown = app.af.show_filters && !app.af.filters.conds.is_empty();
    for (b, icon, tip) in [(minus, ph::MINUS, "Hide the search conditions"), (plus, ph::PLUS, "Add a search condition")] {
        let resp = ui.interact(b, ui.scope_id().with(("afs", tip)), Sense::click()).on_hover_text(tip);
        let enabled = if icon == ph::MINUS { shown } else { !shown || app.af.filters.conds.len() < MAX_CONDITIONS };
        p.rect_filled(b, 5.0, if resp.hovered() && enabled { look::button_on() } else { look::group_bg() });
        crate::glyphs::paint(&p, b, icon, 12.0, if enabled { look::text() } else { look::text_dim().gamma_multiply(0.5) });
        if resp.clicked() && enabled {
            if icon == ph::MINUS {
                app.af.show_filters = false;
            } else if !app.af.show_filters && !app.af.filters.conds.is_empty() {
                // (+ after −: the rows come back)
                app.af.show_filters = true;
            } else {
                app.af.show_filters = true;
                app.af.filters.conds.push(Cond::new(Field::Name));
            }
        }
    }
}

fn combo(ui: &mut egui::Ui, id: (&str, usize), w: f32, cur: usize, items: &[String]) -> Option<usize> {
    let mut out = None;
    egui::ComboBox::from_id_salt(id).width(w).selected_text(items.get(cur).cloned().unwrap_or_default()).show_ui(ui, |ui| {
        for (i, t) in items.iter().enumerate() {
            if ui.selectable_label(i == cur, t).clicked() {
                out = Some(i);
            }
        }
    });
    out
}

/// The condition rows (each: field, operator, value, –). Returns their height.
pub fn condition_rows(app: &mut App, ui: &mut egui::Ui, at: egui::Pos2, width: f32) -> f32 {
    if !app.af.show_filters {
        return 0.0;
    }
    let n = app.af.filters.conds.len();
    let row_h = 26.0;
    let r = Rect::from_min_size(at, vec2(width, n as f32 * row_h));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)));
    let mut remove: Option<usize> = None;
    for k in 0..n {
        child.horizontal(|ui| {
            ui.set_height(row_h - 2.0);
            let c = &mut app.af.filters.conds[k];
            let fields: Vec<String> = FIELDS.iter().map(|f| f.1.to_string()).collect();
            let cur = FIELDS.iter().position(|f| f.0 == c.field).unwrap_or(0);
            if let Some(i) = combo(ui, ("afs-field", k), 92.0, cur, &fields) {
                *c = Cond::new(FIELDS[i].0);
            }
            let opn: Vec<String> = ops(c.field).iter().map(|s| s.to_string()).collect();
            if let Some(i) = combo(ui, ("afs-op", k), 96.0, c.op, &opn) {
                c.op = i;
            }
            match c.field {
                Field::Name | Field::Comment => {
                    ui.add(egui::TextEdit::singleline(&mut c.text).desired_width(110.0));
                }
                Field::Modified => {
                    ui.add(egui::TextEdit::singleline(&mut c.text).desired_width(90.0).hint_text("2026-11-12"));
                }
                Field::FileType | Field::Format | Field::SampleRate | Field::BitDepth => {
                    let items: Vec<String> = match c.field {
                        Field::FileType => FILE_TYPES.iter().map(|f| f.0.to_string()).collect(),
                        Field::Format => FORMATS.iter().map(|f| f.0.to_string()).collect(),
                        Field::SampleRate => RATES.iter().map(|r| format!("{} kHz", *r as f64 / 1000.0)).collect(),
                        _ => DEPTHS.iter().map(|d| format!("{d} Bit")).collect(),
                    };
                    if let Some(i) = combo(ui, ("afs-val", k), 100.0, c.num as usize, &items) {
                        c.num = i as f64;
                    }
                }
                Field::Length | Field::Size => {
                    ui.add(egui::DragValue::new(&mut c.num).range(0.0..=1.0e6).speed(0.5));
                    let units: Vec<String> = if c.field == Field::Length { LEN_UNITS.iter().map(|u| u.0.to_string()).collect() } else { SIZE_UNITS.iter().map(|u| u.0.to_string()).collect() };
                    if let Some(i) = combo(ui, ("afs-unit", k), 64.0, c.unit, &units) {
                        c.unit = i;
                    }
                }
            }
            if ui.small_button("–").on_hover_text("Remove this condition").clicked() {
                remove = Some(k);
            }
        });
    }
    if let Some(k) = remove {
        app.af.filters.conds.remove(k);
        if app.af.filters.conds.is_empty() {
            app.af.show_filters = false;
        }
    }
    n as f32 * row_h
}

/// The clickable path: each folder of it opens that folder. Returns the folder clicked.
pub fn path_bar(ui: &mut egui::Ui, r: Rect, dir: &Path) -> Option<PathBuf> {
    let p = ui.painter().with_clip_rect(r);
    let mut x = r.min.x;
    let mut out = None;
    let mut acc = PathBuf::new();
    let parts: Vec<_> = dir.components().collect();
    for (i, c) in parts.iter().enumerate() {
        acc.push(c.as_os_str());
        let name = if i == 0 { "/".to_string() } else { c.as_os_str().to_string_lossy().to_string() };
        let g = p.layout_no_wrap(name, look::regular(11.0), look::text_dim());
        let w = g.size().x;
        let seg = Rect::from_min_size(pos2(x, r.min.y), vec2(w + 2.0, r.height()));
        let resp = ui.interact(seg, ui.scope_id().with(("afs-path", i)), Sense::click());
        let col = if resp.hovered() { look::text() } else { look::text_dim() };
        p.galley_with_override_text_color(pos2(x, r.center().y - g.size().y / 2.0), g, col);
        if resp.clicked() {
            out = Some(acc.clone());
        }
        x += w + 2.0;
        if i + 1 < parts.len() && i > 0 {
            p.text(pos2(x + 3.0, r.center().y), Align2::LEFT_CENTER, "›", look::regular(11.0), look::text_dim());
            x += 12.0;
        }
    }
    out
}

/// The list's columns: (title, x from the left, width) for a list `w` wide.
pub fn columns(w: f32) -> [(&'static str, f32, f32); 4] {
    let type_w = 90.0f32;
    let size_w = 70.0f32;
    let date_w = 128.0f32;
    let name_w = (w - type_w - size_w - date_w).max(80.0);
    [("Name", 0.0, name_w), ("Date Created", name_w, date_w), ("Size", name_w + date_w, size_w), ("Type", name_w + date_w + size_w, type_w)]
}

pub fn kind_name(k: FileKind) -> &'static str {
    match k {
        FileKind::Folder => "Folder",
        FileKind::LogicProject => "Logic Project",
        FileKind::GarageBand => "GarageBand",
        FileKind::Audio => "Audio File",
        FileKind::Midi => "MIDI File",
        FileKind::Movie => "Movie",
        FileKind::Other => "Document",
    }
}

/// A file's created (else modified) time in seconds, and its size in bytes.
pub fn created_and_size(p: &Path) -> (u64, u64) {
    let m = std::fs::metadata(p).ok();
    let t = m.as_ref().and_then(|m| m.created().or_else(|_| m.modified()).ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    (t, m.map_or(0, |m| if m.is_dir() { 0 } else { m.len() }))
}

pub fn date_text(secs: u64) -> String {
    if secs == 0 {
        return String::new();
    }
    let days = (secs / 86_400) as i64;
    // (civil from days)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let hm = secs % 86_400;
    format!("{y}-{m:02}-{d:02} {:02}:{:02}", hm / 3600, (hm % 3600) / 60)
}

pub fn size_text(b: u64) -> String {
    match b {
        0 => "--".into(),
        b if b < 1024 * 1024 => format!("{:.0} KB", (b as f64 / 1024.0).ceil()),
        b if b < 1024 * 1024 * 1024 => format!("{:.1} MB", b as f64 / 1048576.0),
        b => format!("{:.2} GB", b as f64 / 1073741824.0),
    }
}

/// Sorts list entries by a column (folders stay first under Name).
pub fn sort_entries(v: &mut [(PathBuf, bool)], col: usize, desc: bool) {
    v.sort_by_cached_key(|(p, dir)| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let (t, s) = created_and_size(p);
        let k: (u64, String) = match col {
            1 => (t, name.clone()),
            2 => (s, name.clone()),
            3 => (kind_name(crate::allfiles::kind_of(p, *dir)).len() as u64, kind_name(crate::allfiles::kind_of(p, *dir)).to_string()),
            _ => (!*dir as u64, name.clone()),
        };
        (k, name)
    });
    if desc {
        v.reverse();
    }
}

/// The list's header: a click sorts by that column, again the other way.
pub fn header(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    let p = ui.painter().clone();
    p.rect_filled(r, 0.0, look::header_bg());
    p.hline(r.x_range(), r.max.y - 0.5, Stroke::new(1.0, look::line()));
    for (i, (t, x, w)) in columns(r.width()).iter().enumerate() {
        let c = Rect::from_min_size(pos2(r.min.x + x, r.min.y), vec2(*w, r.height()));
        if i > 0 {
            p.vline(c.min.x, (c.min.y + 3.0)..=(c.max.y - 3.0), Stroke::new(1.0, look::line()));
        }
        let on = app.afs.sort.0 == i;
        p.with_clip_rect(c).text(pos2(c.min.x + if i == 0 { 28.0 } else { 6.0 }, c.center().y), Align2::LEFT_CENTER, *t, if on { look::semibold(11.0) } else { look::regular(11.0) }, if on { look::text() } else { look::text_dim() });
        if on {
            crate::glyphs::paint(&p, Rect::from_center_size(pos2(c.max.x - 9.0, c.center().y), vec2(10.0, 10.0)), if app.afs.sort.1 { crate::glyphs::ph::CARET_DOWN } else { crate::glyphs::ph::CARET_UP }, 9.0, look::text_dim());
        }
        if ui.interact(c, ui.scope_id().with(("afs-head", i)), Sense::click()).clicked() {
            app.afs.sort = if on { (i, !app.afs.sort.1) } else { (i, false) };
        }
    }
}

/// The Bookmark button's list (in place of the folder's files): double-click opens one; its
/// shortcut menu's Bookmark item removes it.
pub fn bookmark_list(app: &mut App, ui: &mut egui::Ui, list: Rect) -> Option<PathBuf> {
    ensure(app);
    let mut open = None;
    let marks = app.afs.bookmarks.clone();
    if marks.is_empty() {
        ui.painter().text(list.center(), Align2::CENTER_CENTER, "No bookmarks: a folder's shortcut menu ▸ Bookmark", look::regular(12.0), look::text_dim());
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    egui::ScrollArea::vertical().id_salt("afs-marks").auto_shrink([false, false]).show(&mut child, |ui| {
        for (k, d) in marks.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(list.width(), 22.0), Sense::click());
            if k % 2 == 1 {
                ui.painter().rect_filled(r, 0.0, look::lane_alt());
            }
            crate::glyphs::paint(ui.painter(), Rect::from_min_size(r.min + vec2(8.0, 4.0), vec2(14.0, 14.0)), crate::glyphs::ph_fill::BOOKMARK_SIMPLE, 12.0, look::accent());
            let name = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| d.display().to_string());
            ui.painter().with_clip_rect(r).text(pos2(r.min.x + 28.0, r.center().y), Align2::LEFT_CENTER, name, look::regular(12.0), look::text());
            if resp.double_clicked() {
                open = Some(d.clone());
            }
            resp.context_menu(|ui| bookmark_item(app, ui, d));
        }
    });
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_number_operators() {
        let p = Path::new("/x/Kick Loop.wav");
        let mut c = Cond::new(Field::Name);
        c.text = "kick".into();
        assert!(matches(&c, p, FileKind::Audio));
        c.op = 1;
        assert!(!matches(&c, p, FileKind::Audio));
        c.op = 4;
        assert!(matches(&c, p, FileKind::Audio));
        c.op = 5;
        c.text = ".wav".into();
        assert!(matches(&c, p, FileKind::Audio));
        c.op = 2;
        c.text = "kick loop.wav".into();
        assert!(matches(&c, p, FileKind::Audio));
        assert!(num_op(2, 3.0, 5.0) && num_op(3, 6.0, 5.0) && num_op(0, 5.0, 5.0) && num_op(1, 9.0, 5.0));
        let mut t = Cond::new(Field::FileType);
        assert!(matches(&t, p, FileKind::Audio));
        t.op = 1;
        assert!(!matches(&t, p, FileKind::Audio));
        let mut f = Cond::new(Field::Format);
        f.num = 1.0;
        assert!(matches(&f, p, FileKind::Audio));
        f.num = 0.0;
        assert!(!matches(&f, p, FileKind::Audio));
        assert_eq!(ops(Field::Name).len(), 6);
        assert_eq!(ops(Field::Size), &["is", "is not", "less than", "greater than"]);
        assert_eq!(ops(Field::Modified), &["is", "is not", "after", "before"]);
    }

    #[test]
    fn files_on_disk() {
        let dir = std::env::temp_dir().join(format!("lg-afs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("one.wav");
        std::fs::write(&wav, crate::audio::wav_bytes(&vec![0.0; 48_000 * 2])).unwrap();
        let mut sr = Cond::new(Field::SampleRate);
        sr.num = RATES.iter().position(|r| *r == crate::audio::RATE as u32).unwrap() as f64;
        assert!(matches(&sr, &wav, FileKind::Audio));
        sr.op = 1;
        assert!(!matches(&sr, &wav, FileKind::Audio));
        let mut len = Cond::new(Field::Length);
        len.op = 2;
        len.num = 2.0;
        assert!(matches(&len, &wav, FileKind::Audio));
        len.op = 3;
        assert!(!matches(&len, &wav, FileKind::Audio));
        let mut size = Cond::new(Field::Size);
        size.op = 3;
        size.num = 1.0;
        assert!(matches(&size, &wav, FileKind::Audio));
        // an AIFF made as an Apple Loop is left out of AIFF searches
        let aif = dir.join("loop.aif");
        std::fs::write(&aif, b"FORM\0\0\0\x20AIFFbasc\0\0\0\x04xxxx").unwrap();
        let mut f = Cond::new(Field::Format);
        f.num = 0.0;
        assert!(!matches(&f, &aif, FileKind::Audio));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dates_recent_bookmarks_and_sorting() {
        assert_eq!(parse_date("1970-01-02"), Some(1));
        assert_eq!(parse_date("2026-11-12"), Some(20_769));
        assert_eq!(date_text(20_769 * 86_400 + 3600 * 13 + 60 * 5), "2026-11-12 13:05");
        let mut r = vec!["a".to_string(), "b".to_string()];
        remember(&mut r, "b");
        assert_eq!(r, vec!["b", "a"]);
        let mut b = Vec::new();
        toggle_bookmark(&mut b, Path::new("/m/Loops"));
        assert_eq!(b.len(), 1);
        assert_eq!(bookmark_label(Path::new("/m/Loops")), "Bookmark “Loops”");
        toggle_bookmark(&mut b, Path::new("/m/Loops"));
        assert!(b.is_empty());
        let mut v = vec![(PathBuf::from("/b.wav"), false), (PathBuf::from("/a"), true), (PathBuf::from("/c.mid"), false)];
        sort_entries(&mut v, 0, false);
        assert_eq!(v[0].0, PathBuf::from("/a"));
        sort_entries(&mut v, 0, true);
        assert_eq!(v[2].0, PathBuf::from("/a"));
        assert_eq!(size_text(2048), "2 KB");
        assert_eq!(columns(500.0)[0].0, "Name");
    }
}
