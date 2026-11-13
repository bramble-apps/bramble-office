//! The All Files Browser (Project Files ▸ All Files in the right pane), as Logic's: Back and
//! Forward through the folders visited, Computer / Home / Project for where to look, List or
//! Column view, a search field (by name) whose + shows more filters (kind, format, length,
//! modified, size), the Action menu (Add to Project Audio, Show in Finder, Quick Look, New
//! Folder) and Play. It lists Logic projects (and older ones), GarageBand projects, audio files,
//! movies and the formats Studio imports (MIDI). Double-click: a folder opens, a project opens,
//! a file is imported at the playhead; drag a file to the tracks.

use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::look;

/// What a file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Folder,
    LogicProject,
    GarageBand,
    Audio,
    Midi,
    Movie,
    Other,
}

pub fn kind_of(p: &Path, is_dir: bool) -> FileKind {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "logicx" | "logic" | "lso" => FileKind::LogicProject,
        "band" => FileKind::GarageBand,
        "mov" | "mp4" | "m4v" => FileKind::Movie,
        "mid" | "midi" | "smf" | "kar" => FileKind::Midi,
        _ if crate::soundlib::is_audio(p) => FileKind::Audio,
        _ if is_dir => FileKind::Folder,
        _ => FileKind::Other,
    }
}

/// The search's filters (+ in the search field).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Filters {
    pub kind: Option<FileKind>,
    /// A file extension ("wav").
    pub format: String,
    /// Length in seconds (audio): at least, at most (0: any).
    pub min_len: f64,
    pub max_len: f64,
    /// Modified within this many days (0: any).
    pub days: u32,
    /// Size in KB: at least, at most (0: any).
    pub min_kb: u64,
    pub max_kb: u64,
    /// The search's condition rows (afsearch.rs).
    pub conds: Vec<crate::afsearch::Cond>,
}

impl Filters {
    pub fn active(&self) -> bool {
        *self != Filters::default()
    }

    pub fn matches(&self, p: &Path, kind: FileKind) -> bool {
        if let Some(k) = self.kind {
            if kind != k {
                return false;
            }
        }
        if !self.format.is_empty() && !p.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(self.format.trim_start_matches('.'))) {
            return false;
        }
        let meta = std::fs::metadata(p).ok();
        if self.days > 0 {
            let recent = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|e| e.as_secs() <= self.days as u64 * 86_400);
            if !recent {
                return false;
            }
        }
        let kb = meta.as_ref().map_or(0, |m| m.len() / 1024);
        if (self.min_kb > 0 && kb < self.min_kb) || (self.max_kb > 0 && kb > self.max_kb) {
            return false;
        }
        if !self.conds.iter().all(|c| crate::afsearch::matches(c, p, kind)) {
            return false;
        }
        if self.min_len > 0.0 || self.max_len > 0.0 {
            let Some(l) = (kind == FileKind::Audio).then(|| crate::soundlib::audio_seconds(p)).flatten() else { return false };
            if (self.min_len > 0.0 && l < self.min_len) || (self.max_len > 0.0 && l > self.max_len) {
                return false;
            }
        }
        true
    }
}

#[derive(Default)]
pub struct State {
    pub dir: Option<PathBuf>,
    pub back: Vec<PathBuf>,
    pub forward: Vec<PathBuf>,
    pub columns: bool,
    pub search: String,
    pub show_filters: bool,
    pub filters: Filters,
    pub selected: Option<PathBuf>,
    pub new_folder: Option<String>,
    /// A cached listing: (folder, search, filters) → entries.
    cache: Option<(PathBuf, String, Filters, Vec<(PathBuf, bool)>)>,
}

/// What the Browser shows: Logic's kinds (projects, audio, movies, MIDI) and folders.
fn shown(p: &Path, is_dir: bool) -> bool {
    !matches!(kind_of(p, is_dir), FileKind::Other)
}

pub fn list_dir(d: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(rd) = std::fs::read_dir(d) else { return Vec::new() };
    let mut v: Vec<(PathBuf, bool)> = rd.flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.')).map(|e| {
        let p = e.path();
        // (a .logicx package is a folder on disk but a project here)
        let dir = p.is_dir() && kind_of(&p, true) == FileKind::Folder;
        (p, dir)
    }).filter(|(p, dir)| shown(p, *dir)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.file_name().map(|n| n.to_string_lossy().to_lowercase()).cmp(&b.0.file_name().map(|n| n.to_string_lossy().to_lowercase()))));
    v
}

/// Searches under `d` by name (and the filters), a few levels deep.
pub fn search(d: &Path, q: &str, f: &Filters) -> Vec<(PathBuf, bool)> {
    let q = q.to_lowercase();
    let mut out = Vec::new();
    fn walk(d: &Path, q: &str, f: &Filters, out: &mut Vec<(PathBuf, bool)>, depth: u32) {
        for (p, dir) in list_dir(d) {
            if out.len() >= 2000 {
                return;
            }
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            let k = kind_of(&p, dir);
            if (q.is_empty() || name.contains(q)) && (!dir || f.kind == Some(FileKind::Folder)) && f.matches(&p, k) {
                out.push((p.clone(), dir));
            }
            if dir && depth > 0 {
                walk(&p, q, f, out, depth - 1);
            }
        }
    }
    walk(d, &q, f, &mut out, 4);
    out
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// The open project's folder.
fn project_dir(app: &App) -> Option<PathBuf> {
    let (alt, _) = app.file.as_ref()?;
    let mut p = alt.as_path();
    while let Some(parent) = p.parent() {
        if p.extension().is_some_and(|e| e == "logicx") {
            return parent.to_path_buf().into();
        }
        p = parent;
    }
    alt.parent().map(Path::to_path_buf)
}

fn go(app: &mut App, d: PathBuf) {
    if let Some(cur) = app.af.dir.clone() {
        if cur != d {
            app.af.back.push(cur);
        }
    }
    app.af.forward.clear();
    app.af.dir = Some(d);
    app.af.selected = None;
}

fn icon_for(k: FileKind) -> &'static str {
    use crate::glyphs::ph;
    match k {
        FileKind::Folder => ph::FOLDER_SIMPLE,
        FileKind::LogicProject => ph::FILE_AUDIO,
        FileKind::GarageBand => ph::GUITAR,
        FileKind::Audio => ph::WAVEFORM,
        FileKind::Midi => ph::MUSIC_NOTES,
        FileKind::Movie => ph::FILM_STRIP,
        FileKind::Other => ph::FILE,
    }
}

fn open_item(app: &mut App, p: &Path, dir: bool) {
    match kind_of(p, dir) {
        FileKind::Folder => go(app, p.to_path_buf()),
        FileKind::LogicProject | FileKind::GarageBand => app.open_path(p),
        FileKind::Audio | FileKind::Midi => app.import_file(p),
        FileKind::Movie => app.tell("Movies aren't supported yet"),
        FileKind::Other => {}
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    let dir = app.af.dir.clone().unwrap_or_else(home);
    // ---- the buttons: Back, Forward; Computer, Home, Project; List, Column; the Action menu
    let row = Rect::from_min_size(area.min + vec2(8.0, 6.0), vec2(area.width() - 16.0, 24.0));
    let btn = |i: f32, w: f32| Rect::from_min_size(pos2(row.min.x + i, row.min.y), vec2(w, 24.0));
    let mut x = 0.0;
    let clicked = |ui: &mut egui::Ui, r: Rect, icon: &str, on: bool, tip: &str, enabled: bool| -> bool {
        let resp = ui.interact(r, ui.scope_id().with(("af", tip)), Sense::click()).on_hover_text(tip);
        let pp = ui.painter();
        pp.rect_filled(r, 5.0, if on { look::accent() } else if resp.hovered() && enabled { look::button_on() } else { look::group_bg() });
        crate::glyphs::paint(pp, r, icon, 13.0, if on { Color32::WHITE } else if enabled { look::text() } else { look::text_dim().gamma_multiply(0.5) });
        resp.clicked() && enabled
    };
    use crate::glyphs::ph;
    let can_back = !app.af.back.is_empty();
    let can_fwd = !app.af.forward.is_empty();
    if clicked(ui, btn(x, 26.0), ph::CARET_LEFT, false, "Back", can_back) {
        if let Some(d) = app.af.back.pop() {
            app.af.forward.push(dir.clone());
            app.af.dir = Some(d);
        }
    }
    x += 28.0;
    if clicked(ui, btn(x, 26.0), ph::CARET_RIGHT, false, "Forward", can_fwd) {
        if let Some(d) = app.af.forward.pop() {
            app.af.back.push(dir.clone());
            app.af.dir = Some(d);
        }
    }
    x += 36.0;
    if clicked(ui, btn(x, 26.0), ph::DESKTOP, dir == Path::new("/"), "Computer", true) {
        go(app, PathBuf::from("/"));
    }
    x += 28.0;
    if clicked(ui, btn(x, 26.0), ph::HOUSE, dir == home(), "Home", true) {
        go(app, home());
    }
    x += 28.0;
    let proj = project_dir(app);
    if clicked(ui, btn(x, 26.0), ph::FOLDER_OPEN, proj.as_ref() == Some(&dir), "Project", proj.is_some()) {
        if let Some(d) = proj {
            go(app, d);
        }
    }
    x += 36.0;
    let cols = app.af.columns;
    if clicked(ui, btn(x, 26.0), ph::LIST, !cols, "List view", true) {
        app.af.columns = false;
    }
    x += 28.0;
    if clicked(ui, btn(x, 26.0), ph::COLUMNS, cols, "Column view", true) {
        app.af.columns = true;
    }
    x += 36.0;
    let act = btn(x, 30.0);
    let aresp = ui.interact(act, ui.scope_id().with("af-action"), Sense::click()).on_hover_text("Action");
    p.rect_filled(act, 5.0, if aresp.hovered() { look::button_on() } else { look::group_bg() });
    crate::glyphs::paint(&p, act, ph::GEAR_SIX, 13.0, look::text());
    let sel = app.af.selected.clone();
    egui::Popup::menu(&aresp).show(|ui| {
        let audio = sel.as_ref().is_some_and(|s| kind_of(s, false) == FileKind::Audio);
        if ui.add_enabled(audio, egui::Button::new("Add to Project Audio")).clicked() {
            if let Some(s) = sel.clone() {
                crate::projaudio::add_files(app, vec![s]);
                app.tell("Added to Project Audio");
            }
            ui.close();
        }
        if ui.add_enabled(sel.is_some(), egui::Button::new("Show in Finder")).clicked() {
            if let Some(s) = &sel {
                crate::projaudio::reveal(s);
            }
            ui.close();
        }
        if ui.add_enabled(sel.is_some(), egui::Button::new("Quick Look")).clicked() {
            if let Some(s) = &sel {
                if matches!(kind_of(s, false), FileKind::Audio | FileKind::Midi) {
                    let id = s.display().to_string();
                    crate::preview::toggle(app, &id, &s.clone(), 0.8, None, false);
                } else {
                    let _ = std::process::Command::new("xdg-open").arg(s).stdin(std::process::Stdio::null()).spawn();
                }
            }
            ui.close();
        }
        ui.separator();
        if ui.button("New Folder").clicked() {
            app.af.new_folder = Some("untitled folder".into());
            ui.close();
        }
    });
    // the Bookmark button: the bookmarks in place of the files
    let bm = btn(x + 36.0, 26.0);
    if clicked(ui, bm, ph::BOOKMARK_SIMPLE, app.afs.showing_bookmarks, "Bookmarks", true) {
        crate::afsearch::ensure(app);
        app.afs.showing_bookmarks = !app.afs.showing_bookmarks;
    }
    // Play (at the right)
    let play = Rect::from_min_size(pos2(row.max.x - 26.0, row.min.y), vec2(26.0, 24.0));
    let now = app.ctx.input(|i| i.time);
    let playing = sel.as_ref().is_some_and(|s| app.preview.is_playing(&s.display().to_string(), now));
    if clicked(ui, play, if playing { crate::glyphs::ph_fill::STOP } else { crate::glyphs::ph_fill::PLAY }, playing, "Play the selected file", sel.as_ref().is_some_and(|s| matches!(kind_of(s, false), FileKind::Audio | FileKind::Midi))) {
        if let Some(s) = sel.clone() {
            crate::preview::toggle(app, &s.display().to_string(), &s, 0.8, None, false);
        }
    }
    // ---- the search field (recent searches, Clear, − and +) and the condition rows
    let sr = Rect::from_min_size(pos2(area.min.x + 8.0, row.max.y + 6.0), vec2(area.width() - 16.0, 24.0));
    crate::afsearch::search_row(app, ui, sr);
    let mut y = sr.max.y + 4.0;
    y += crate::afsearch::condition_rows(app, ui, pos2(area.min.x + 8.0, y), area.width() - 16.0);
    // the path
    let path_r = Rect::from_min_size(pos2(area.min.x + 8.0, y), vec2(area.width() - 16.0, 18.0));
    // (each folder of the path opens it)
    if let Some(d) = crate::afsearch::path_bar(ui, path_r, &dir) {
        go(app, d);
        app.afs.showing_bookmarks = false;
    }
    y = path_r.max.y + 2.0;
    p.hline(area.x_range(), y, Stroke::new(1.0, look::line()));
    let list = Rect::from_min_max(pos2(area.min.x, y + 1.0), area.max);
    // (hidden condition rows don't apply)
    let mut filters = app.af.filters.clone();
    if !app.af.show_filters {
        filters.conds.clear();
    }
    let searching = !app.af.search.trim().is_empty() || filters.active();
    let mut open: Option<(PathBuf, bool)> = None;
    let mut drag: Option<PathBuf> = None;
    if crate::trackimport::active(app) {
        crate::trackimport::view(app, ui, list);
        return;
    }
    if app.afs.showing_bookmarks {
        if let Some(d) = crate::afsearch::bookmark_list(app, ui, list) {
            go(app, d);
            app.afs.showing_bookmarks = false;
        }
    } else if app.af.columns && !searching {
        // Column view: the folders from Home (or /) down to this one, side by side
        let root = if dir.starts_with(home()) { home() } else { PathBuf::from("/") };
        let mut chain = vec![root.clone()];
        if let Ok(rest) = dir.strip_prefix(&root) {
            let mut c = root.clone();
            for part in rest.components() {
                c = c.join(part);
                chain.push(c.clone());
            }
        }
        let col_w = 180.0f32;
        let shown_cols = ((list.width() / col_w).floor() as usize).max(1);
        let first = chain.len().saturating_sub(shown_cols);
        for (ci, d) in chain[first..].iter().enumerate() {
            let cr = Rect::from_min_size(pos2(list.min.x + ci as f32 * col_w, list.min.y), vec2(col_w, list.height()));
            p.vline(cr.max.x, cr.y_range(), Stroke::new(1.0, look::line()));
            let entries = list_dir(d);
            let next = chain.get(first + ci + 1).cloned();
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cr));
            egui::ScrollArea::vertical().id_salt(("af-col", ci)).auto_shrink([false, false]).show_rows(&mut child, 20.0, entries.len(), |ui, range| {
                for k in range {
                    let (path, is_dir) = &entries[k];
                    let (r, resp) = ui.allocate_exact_size(vec2(col_w, 20.0), Sense::click_and_drag());
                    let on = next.as_ref() == Some(path) || app.af.selected.as_ref() == Some(path);
                    if on {
                        ui.painter().rect_filled(r, 0.0, look::accent().gamma_multiply(0.45));
                    }
                    let kind = kind_of(path, *is_dir);
                    crate::glyphs::paint(ui.painter(), Rect::from_min_size(r.min + vec2(4.0, 3.0), vec2(14.0, 14.0)), icon_for(kind), 11.0, look::text_dim());
                    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    ui.painter().with_clip_rect(r).text(pos2(r.min.x + 22.0, r.center().y), Align2::LEFT_CENTER, name, look::regular(11.5), look::text());
                    if *is_dir {
                        crate::glyphs::paint(ui.painter(), Rect::from_min_size(pos2(r.max.x - 16.0, r.min.y + 3.0), vec2(14.0, 14.0)), ph::CARET_RIGHT, 9.0, look::text_dim());
                    }
                    if resp.clicked() {
                        if *is_dir {
                            app.af.dir = Some(path.clone());
                            app.af.selected = None;
                        } else {
                            app.af.selected = Some(path.clone());
                        }
                    }
                    if resp.double_clicked() && !*is_dir {
                        open = Some((path.clone(), false));
                    }
                    if resp.drag_started() && matches!(kind, FileKind::Audio | FileKind::Midi) {
                        drag = Some(path.clone());
                    }
                }
            });
        }
    } else {
        let entries = if searching {
            let key = (dir.clone(), app.af.search.clone(), filters.clone());
            match &app.af.cache {
                Some((d, s, f, v)) if (d, s, f) == (&key.0, &key.1, &key.2) => v.clone(),
                _ => {
                    let v = search(&dir, &app.af.search, &app.af.filters);
                    app.af.cache = Some((key.0, key.1, key.2, v.clone()));
                    v
                }
            }
        } else {
            list_dir(&dir)
        };
        // the columns' header: a click sorts
        let head = Rect::from_min_size(list.min, vec2(list.width(), 20.0));
        crate::afsearch::header(app, ui, head);
        let list = Rect::from_min_max(pos2(list.min.x, head.max.y), list.max);
        let mut entries = entries;
        crate::afsearch::sort_entries(&mut entries, app.afs.sort.0, app.afs.sort.1);
        let cols = crate::afsearch::columns(list.width());
        if entries.is_empty() {
            p.text(list.center(), Align2::CENTER_CENTER, if searching { "No matches" } else { "No projects, audio or MIDI files here" }, look::regular(12.0), look::text_dim());
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
        egui::ScrollArea::vertical().id_salt("af-list").auto_shrink([false, false]).show_rows(&mut child, 22.0, entries.len(), |ui, range| {
            for k in range {
                let (path, is_dir) = &entries[k];
                let (r, resp) = ui.allocate_exact_size(vec2(list.width(), 22.0), Sense::click_and_drag());
                let pp = ui.painter();
                if app.af.selected.as_ref() == Some(path) {
                    pp.rect_filled(r, 0.0, look::accent().gamma_multiply(0.45));
                } else if k % 2 == 1 {
                    pp.rect_filled(r, 0.0, look::lane_alt());
                }
                let kind = kind_of(path, *is_dir);
                crate::glyphs::paint(pp, Rect::from_min_size(r.min + vec2(8.0, 4.0), vec2(14.0, 14.0)), icon_for(kind), 12.0, look::text_dim());
                let name = if searching { path.strip_prefix(&dir).map(|x| x.display().to_string()).unwrap_or_default() } else { path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() };
                let name_r = Rect::from_min_size(r.min, vec2(cols[0].2 - 4.0, r.height()));
                pp.with_clip_rect(name_r).text(pos2(r.min.x + 28.0, r.center().y), Align2::LEFT_CENTER, name, look::regular(12.0), look::text());
                let (created, size) = crate::afsearch::created_and_size(path);
                for (i, t) in [(1usize, crate::afsearch::date_text(created)), (2, if *is_dir { "--".into() } else { crate::afsearch::size_text(size) }), (3, crate::afsearch::kind_name(kind).to_string())] {
                    let c = Rect::from_min_size(pos2(r.min.x + cols[i].1 + 6.0, r.min.y), vec2(cols[i].2 - 8.0, r.height()));
                    pp.with_clip_rect(c).text(pos2(c.min.x, r.center().y), Align2::LEFT_CENTER, t, look::regular(11.0), look::text_dim());
                }
                if resp.clicked() {
                    app.af.selected = Some(path.clone());
                }
                if *is_dir {
                    let d = path.clone();
                    resp.context_menu(|ui| crate::afsearch::bookmark_item(app, ui, &d));
                }
                if resp.double_clicked() {
                    open = Some((path.clone(), *is_dir));
                }
                if resp.drag_started() && matches!(kind, FileKind::Audio | FileKind::Midi) {
                    drag = Some(path.clone());
                }
            }
        });
    }
    // a project selected: Import (its tracks: Track Import)
    if let Some(sel) = app.af.selected.clone().filter(|s| matches!(kind_of(s, true), FileKind::LogicProject | FileKind::GarageBand)) {
        let b = Rect::from_min_size(pos2(list.max.x - 76.0, list.max.y - 30.0), vec2(68.0, 22.0));
        let resp = ui.interact(b, ui.scope_id().with("af-import"), Sense::click()).on_hover_text("Import tracks from this project");
        ui.painter().rect_filled(b, 5.0, if resp.hovered() { look::accent().gamma_multiply(0.85) } else { look::accent() });
        ui.painter().text(b.center(), Align2::CENTER_CENTER, "Import", look::regular(12.0), Color32::WHITE);
        if resp.clicked() {
            crate::trackimport::open_source(app, &sel);
        }
    }
    if let Some((path, is_dir)) = open {
        open_item(app, &path, is_dir);
        if is_dir {
            app.af.search.clear();
        }
    }
    if let Some(f) = drag {
        crate::browser::start_file_drag(app, f, None);
    }
    // New Folder
    if let Some(mut name) = app.af.new_folder.take() {
        let mut keep = true;
        let mut ok = false;
        egui::Window::new("New Folder").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ui.ctx(), |ui| {
            let te = ui.text_edit_singleline(&mut name);
            crate::widgets::focus_once(ui, &te);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
                ok = ui.button("Create").clicked() || (te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            });
        });
        if ok {
            let d = dir.join(name.replace('/', "-"));
            if let Err(e) = std::fs::create_dir(&d) {
                app.tell(format!("Couldn't make the folder: {e}"));
            }
            app.af.cache = None;
        } else if keep {
            app.af.new_folder = Some(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_filters() {
        assert_eq!(kind_of(Path::new("a/Song.logicx"), true), FileKind::LogicProject);
        assert_eq!(kind_of(Path::new("a/Song.band"), true), FileKind::GarageBand);
        assert_eq!(kind_of(Path::new("x.WAV"), false), FileKind::Audio);
        assert_eq!(kind_of(Path::new("x.mid"), false), FileKind::Midi);
        assert_eq!(kind_of(Path::new("x.mov"), false), FileKind::Movie);
        assert_eq!(kind_of(Path::new("dir"), true), FileKind::Folder);
        let dir = std::env::temp_dir().join(format!("lg-af-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("Kick.wav"), crate::audio::wav_bytes(&vec![0.0; 48_000 * 2])).unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        std::fs::write(dir.join("tune.mid"), "x").unwrap();
        let l = list_dir(&dir);
        assert_eq!(l.len(), 2, "{l:?}");
        assert!(l[0].1, "folders first");
        let found = search(&dir, "kick", &Filters::default());
        assert_eq!(found.len(), 1);
        let f = Filters { kind: Some(FileKind::Audio), min_len: 0.5, max_len: 2.0, ..Default::default() };
        assert_eq!(search(&dir, "", &f).len(), 1);
        let f = Filters { min_len: 3.0, ..Default::default() };
        assert!(search(&dir, "", &f).is_empty());
        let f = Filters { format: "mid".into(), ..Default::default() };
        assert_eq!(search(&dir, "", &f).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
