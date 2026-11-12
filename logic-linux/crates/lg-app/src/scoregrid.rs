//! The Chord Grid Library window (Logic: Settings ▸ Chord Grid Library, the Score's Layout ▸
//! Chord Grid Library, a chord grid symbol dropped into the score) with its three panes:
//! Instrument Editor (tunings and their libraries: rename, create, delete, import, export),
//! Chord Grid Selector (filter by tuning, capo, root, bass, type, difficulty, favourites,
//! library; frets shown; left-handed; play a grid as a chord or arpeggio; New / Edit / Delete;
//! OK inserts it) and Chord Grid Editor (dots, optional dots, fingers, barres, muted / open
//! strings, first fret; Clear / Reset; Add / Replace in a target library). Also the chord grid
//! symbols' drawing in the score. Libraries: lg-core chordgrid.rs (Studio's own + the user's
//! file in ~/.config/bramble-studio/chord-grids.txt).

use crate::app::App;
use crate::look;
use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke};
use lg_core::chordgrid::{self as cg, Barre, Grid, Library, Tuning};
use std::sync::{Mutex, OnceLock};

pub const COMMANDS: &[&str] = &["Chord Grid Library…", "Open Chord Grid Library…"];

fn lib_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("chord-grids.txt"))
}

/// The tunings and libraries (Studio's own, then the user's file).
fn store() -> &'static Mutex<Vec<Tuning>> {
    static S: OnceLock<Mutex<Vec<Tuning>>> = OnceLock::new();
    S.get_or_init(|| {
        let mut t = cg::builtin();
        if std::env::var_os("LG_SNAP").is_none() {
            if let Some(text) = lib_path().and_then(|p| std::fs::read_to_string(p).ok()) {
                cg::merge_text(&mut t, &text);
            }
        }
        Mutex::new(t)
    })
}

pub fn tunings() -> Vec<Tuning> {
    store().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn save(t: Vec<Tuning>) {
    if std::env::var_os("LG_SNAP").is_none() {
        if let Some(p) = lib_path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, cg::to_text(&t));
        }
    }
    *store().lock().unwrap_or_else(|e| e.into_inner()) = t;
}

/// Where an OK from the Selector goes: (track, tick, staff, the symbol it replaces).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub track: usize,
    pub tick: u32,
    pub staff: u8,
    pub replace: Option<usize>,
}

#[derive(Clone)]
struct Win {
    /// 0 Instrument Editor, 1 Chord Grid Selector, 2 Chord Grid Editor.
    pane: u8,
    /// The Instrument Editor is there only when opened from Settings or the Layout menu.
    instruments: bool,
    tuning: usize,
    capo: u8,
    root: Option<u8>,
    bass: Option<u8>,
    kind: Option<u8>,
    difficulty: Option<u8>,
    favorites: bool,
    library: Option<String>,
    frets: u8,
    left: bool,
    action: u8,
    tempo: u8,
    sel: Option<(usize, usize)>,
    edit: Grid,
    orig: Option<Grid>,
    /// Editing a grid already in a library (lib, index): Replace is offered.
    editing: Option<(usize, usize)>,
    target_lib: usize,
    cleared: bool,
    sel_dot: Option<usize>,
    sel_barre: Option<usize>,
    drag: Option<(usize, u8)>,
    target: Option<Target>,
    renaming: Option<(usize, usize, bool, String)>,
    create: Option<(String, usize, u8)>,
}

impl Default for Win {
    fn default() -> Win {
        Win { pane: 1, instruments: true, tuning: 0, capo: 0, root: None, bass: None, kind: None, difficulty: None, favorites: false, library: None, frets: 5, left: false, action: 0, tempo: 1, sel: None, edit: Grid::empty(6), orig: None, editing: None, target_lib: 0, cleared: false, sel_dot: None, sel_barre: None, drag: None, target: None, renaming: None, create: None }
    }
}

fn win_id() -> egui::Id {
    egui::Id::unique("lg-chord-grid-library")
}

/// Opens the library: from a menu (all three panes) or for inserting / replacing (Selector).
pub fn open(ctx: &egui::Context, target: Option<Target>, pane: u8) {
    ctx.data_mut(|m| {
        let w = m.get_temp_mut_or_default::<Option<Win>>(win_id());
        let mut x = w.take().unwrap_or_default();
        x.pane = pane;
        x.instruments = target.is_none();
        x.target = target;
        *w = Some(x);
    });
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
    }
    open(ctx, None, 1);
    let _ = app;
    true
}

/// The geometry of a grid drawn in `r`: string x's (low string first, mirrored left-handed),
/// fret line y's (nut first), the strip above for open / muted marks.
struct Geo {
    xs: Vec<f32>,
    ys: Vec<f32>,
    top: f32,
}

fn geo(r: Rect, strings: usize, frets: u8, left: bool) -> Geo {
    let n = strings.max(2);
    let mut xs: Vec<f32> = (0..n).map(|k| r.min.x + r.width() * k as f32 / (n - 1) as f32).collect();
    if left {
        xs.reverse();
    }
    let ys = (0..=frets).map(|k| r.min.y + r.height() * k as f32 / frets as f32).collect();
    Geo { xs, ys, top: r.min.y }
}

/// Draws a grid: the name above (unless hidden), x / o marks, the nut (or the first fret's
/// number), dots with fingers, optional (white) dots, barres. `sel` colours a dot or barre.
pub fn draw_grid(p: &egui::Painter, r: Rect, g: &Grid, frets: u8, left: bool, name: bool, ink: Color32, paper: Color32, sel: (Option<usize>, Option<usize>)) {
    let n = g.frets.len();
    let first = g.first_fret();
    let geo = geo(r, n, frets, left);
    let w = r.width() / (n.max(2) - 1) as f32;
    let dot_r = (w * 0.36).min(r.height() / frets as f32 * 0.36);
    let line = Stroke::new((w * 0.06).max(0.8), ink);
    if name {
        p.text(pos2(r.center().x, r.min.y - dot_r * 2.6 - w * 0.6), Align2::CENTER_BOTTOM, &g.name, egui::FontId::new((w * 1.25).max(9.0), egui::FontFamily::Name("bold".into())), ink);
    }
    for &x in &geo.xs {
        p.vline(x, r.y_range(), line);
    }
    for (k, &y) in geo.ys.iter().enumerate() {
        let st = if k == 0 && first == 1 { Stroke::new((w * 0.28).max(2.0), ink) } else { line };
        p.hline(r.x_range(), y, st);
    }
    if first > 1 {
        p.text(pos2(r.min.x - w * 0.4, (geo.ys[0] + geo.ys[1]) / 2.0), Align2::RIGHT_CENTER, format!("{first}fr"), egui::FontId::proportional((w * 0.8).max(7.0)), ink);
    }
    let fy = |f: u8| -> f32 {
        let k = (f as i32 - first as i32).clamp(0, frets as i32 - 1) as usize;
        (geo.ys[k] + geo.ys[k + 1]) / 2.0
    };
    // barres
    for (bi, b) in g.barres.iter().enumerate() {
        let (a, z) = (geo.xs[(b.from as usize).min(n - 1)], geo.xs[(b.to as usize).min(n - 1)]);
        let col = if sel.1 == Some(bi) { Color32::from_rgb(0x2e, 0xa0, 0x43) } else { ink };
        p.rect_filled(Rect::from_min_max(pos2(a.min(z) - dot_r, fy(b.fret) - dot_r * 0.8), pos2(a.max(z) + dot_r, fy(b.fret) + dot_r * 0.8)), dot_r, col);
    }
    for k in 0..n {
        let x = geo.xs[k];
        let f = g.frets[k];
        let mark_y = geo.top - dot_r * 1.4;
        if f < 0 {
            let d = dot_r * 0.7;
            p.line_segment([pos2(x - d, mark_y - d), pos2(x + d, mark_y + d)], Stroke::new(1.2, ink));
            p.line_segment([pos2(x - d, mark_y + d), pos2(x + d, mark_y - d)], Stroke::new(1.2, ink));
        } else if f == 0 && !g.barres.iter().any(|b| (b.from as usize..=b.to as usize).contains(&k)) {
            p.circle_stroke(pos2(x, mark_y), dot_r * 0.7, Stroke::new(1.2, ink));
        }
        if f > 0 {
            let c = pos2(x, fy(f as u8));
            let col = if sel.0 == Some(k) { Color32::from_rgb(0x2e, 0xa0, 0x43) } else { ink };
            p.circle_filled(c, dot_r, col);
            if g.fingers.get(k).copied().unwrap_or(0) > 0 && dot_r > 4.0 {
                let label = if g.fingers[k] == 5 { "T".to_string() } else { g.fingers[k].to_string() };
                p.text(c, Align2::CENTER_CENTER, label, egui::FontId::proportional(dot_r * 1.4), paper);
            }
        }
        let o = g.optional.get(k).copied().unwrap_or(0);
        if o > 0 {
            let c = pos2(x, fy(o));
            p.circle_filled(c, dot_r, paper);
            p.circle_stroke(c, dot_r, Stroke::new(1.2, ink));
        }
    }
}

/// Plays a grid on the selected track: a chord, or an arpeggio up / down at a tempo.
fn play(app: &App, g: &Grid, tuning: &[u8], action: u8, tempo: u8) {
    let Some(t) = app.selected_track.or(app.selected_region.map(|x| x.0)) else { return };
    let mut notes = g.pitches(tuning);
    if action == 2 {
        notes.reverse();
    }
    let step = if action == 0 { 0 } else { [260u64, 150, 80][tempo.min(2) as usize] };
    let engine = app.engine.clone();
    std::thread::spawn(move || {
        for &n in &notes {
            if let Ok(mut e) = engine.lock() {
                e.live(t, n, 90);
            }
            std::thread::sleep(std::time::Duration::from_millis(step));
        }
        std::thread::sleep(std::time::Duration::from_millis(900));
        if let Ok(mut e) = engine.lock() {
            for &n in &notes {
                e.live(t, n, 0);
            }
        }
    });
}

fn pick<T: Copy + PartialEq>(ui: &mut egui::Ui, id: &str, v: &mut T, items: &[(T, String)], w: f32) {
    let cur = items.iter().find(|x| x.0 == *v).map_or("–".to_string(), |x| x.1.clone());
    egui::ComboBox::from_id_salt(id).width(w).selected_text(cur).show_ui(ui, |ui| {
        for (val, n) in items {
            ui.selectable_value(v, *val, n);
        }
    });
}

fn opt_items(names: &[&str]) -> Vec<(Option<u8>, String)> {
    std::iter::once((None, "Any".to_string())).chain(names.iter().enumerate().map(|(k, n)| (Some(k as u8), if n.is_empty() { "Major".to_string() } else { n.to_string() }))).collect()
}

/// The window.
pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut w) = ctx.data(|m| m.get_temp::<Option<Win>>(win_id())).flatten() else { return };
    let mut all = tunings();
    w.tuning = w.tuning.min(all.len().saturating_sub(1));
    let mut open = true;
    let mut changed = false;
    let mut insert: Option<Grid> = None;
    egui::Window::new("Chord Grid Library").open(&mut open).collapsible(false).resizable(true).default_size(vec2(640.0, 460.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            if w.instruments && ui.selectable_label(w.pane == 0, "Instrument Editor").clicked() {
                w.pane = 0;
            }
            if ui.selectable_label(w.pane == 1, "Chord Grid Selector").clicked() {
                w.pane = 1;
            }
            if ui.selectable_label(w.pane == 2, "Chord Grid Editor").clicked() {
                w.pane = 2;
            }
        });
        ui.separator();
        match w.pane {
            0 => changed |= instrument_editor(ui, &mut w, &mut all),
            1 => insert = selector(app, ui, &mut w, &mut all, &mut changed),
            _ => changed |= editor(app, ui, &mut w, &mut all),
        }
    });
    if changed {
        save(all.clone());
    }
    if let (Some(g), Some(t)) = (insert, w.target) {
        place(app, t, &g);
        open = false;
    }
    ctx.data_mut(|m| m.insert_temp(win_id(), open.then_some(w)));
}

/// OK in the Selector: the grid into the score (or replacing the one double-clicked).
fn place(app: &mut App, t: Target, g: &Grid) {
    let Some(id) = app.project.as_ref().and_then(|p| p.tracks.get(t.track)).map(|x| x.extra.id) else { return };
    app.checkpoint(if t.replace.is_some() { "Change Chord Grid" } else { "Insert Chord Grid" });
    if let Some(p) = app.project.as_mut() {
        let ts = p.score.track_mut(id);
        match t.replace.and_then(|k| ts.symbols.get_mut(k)) {
            Some(s) => s.text = g.encode(),
            None => {
                ts.symbols.push(lg_core::score::Symbol { tick: t.tick, kind: lg_core::score::SymbolKind::ChordGrid, text: g.encode(), staff: t.staff, ..Default::default() });
                ts.symbols.sort_by_key(|s| s.tick);
            }
        }
    }
    app.after_edit();
}

fn instrument_editor(ui: &mut egui::Ui, w: &mut Win, all: &mut Vec<Tuning>) -> bool {
    let mut changed = false;
    let mut del: Option<(usize, Option<usize>)> = None;
    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        egui::Grid::new("cg-inst").striped(true).show(ui, |ui| {
            for h in ["Name", "Library", "Tuning", "Strings", "Chords", "Basic Chords"] {
                ui.label(egui::RichText::new(h).strong());
            }
            ui.end_row();
            for ti in 0..all.len() {
                for li in 0..all[ti].libraries.len().max(1) {
                    let t = &all[ti];
                    let on = w.tuning == ti && w.target_lib == li;
                    // Name (double-click: rename the tuning)
                    match &mut w.renaming {
                        Some((a, b, true, text)) if *a == ti && *b == li => {
                            let r = ui.text_edit_singleline(text);
                            if r.lost_focus() {
                                let n = text.trim().to_string();
                                if !n.is_empty() {
                                    all[ti].name = n;
                                    changed = true;
                                }
                                w.renaming = None;
                            }
                        }
                        _ => {
                            let r = ui.selectable_label(on, if li == 0 { t.name.clone() } else { String::new() });
                            if r.clicked() {
                                w.tuning = ti;
                                w.target_lib = li;
                            }
                            if r.double_clicked() && li == 0 {
                                w.renaming = Some((ti, li, true, t.name.clone()));
                            }
                        }
                    }
                    let t = &all[ti];
                    match &mut w.renaming {
                        Some((a, b, false, text)) if *a == ti && *b == li => {
                            let r = ui.text_edit_singleline(text);
                            if r.lost_focus() {
                                let n = text.trim().to_string();
                                if let Some(l) = all[ti].libraries.get_mut(li).filter(|l| !l.builtin && !n.is_empty()) {
                                    l.name = n;
                                    changed = true;
                                }
                                w.renaming = None;
                            }
                        }
                        _ => {
                            let lname = t.libraries.get(li).map_or(String::new(), |l| l.name.clone());
                            let r = ui.selectable_label(on, lname.clone());
                            if r.clicked() {
                                w.tuning = ti;
                                w.target_lib = li;
                            }
                            if r.double_clicked() && t.libraries.get(li).is_some_and(|l| !l.builtin) {
                                w.renaming = Some((ti, li, false, lname));
                            }
                        }
                    }
                    let t = &all[ti];
                    ui.label(t.tuning_text());
                    ui.label(t.strings.len().to_string());
                    ui.label(t.libraries.get(li).map_or(0, |l| l.grids.len()).to_string());
                    ui.label(t.libraries.get(li).map_or(0, |l| l.grids.iter().filter(|g| g.difficulty == 0).count()).to_string());
                    ui.end_row();
                }
            }
        });
    });
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Create…").clicked() {
            w.create = Some(("New Library".into(), w.tuning + 1, 6));
        }
        let user = all.get(w.tuning).and_then(|t| t.libraries.get(w.target_lib)).is_some_and(|l| !l.builtin);
        if ui.add_enabled(user, egui::Button::new("Delete")).on_hover_text("Deletes the library (the tuning goes with its only library)").clicked() {
            del = Some((w.tuning, Some(w.target_lib)));
        }
        if ui.button("Import…").clicked() {
            if let Some(p) = kn_ui::files::FileDialog::new().set_title("Import Chord Grid Library").add_filter("Chord grids", &["txt", "chordgrids"]).pick_file() {
                if let Ok(text) = std::fs::read_to_string(p) {
                    cg::merge_text(all, &text);
                    changed = true;
                }
            }
        }
        if ui.add_enabled(all.get(w.tuning).and_then(|t| t.libraries.get(w.target_lib)).is_some(), egui::Button::new("Export…")).clicked() {
            if let Some(t) = all.get(w.tuning) {
                let one = Tuning { name: t.name.clone(), strings: t.strings.clone(), libraries: t.libraries.get(w.target_lib).cloned().map(|mut l| {
                    l.builtin = false;
                    vec![l]
                }).unwrap_or_default() };
                let fname = format!("{}.chordgrids", one.libraries.first().map_or("Library".into(), |l| l.name.clone()));
                if let Some(p) = kn_ui::files::FileDialog::new().set_title("Export Library").set_file_name(fname).save_file() {
                    let _ = std::fs::write(p, cg::to_text(&[one]));
                }
            }
        }
    });
    // the Create Library dialog: name, tuning (an existing one or Default: a new tuning with
    // the chosen number of strings)
    if let Some((name, tun, strings)) = &mut w.create {
        ui.separator();
        ui.label(egui::RichText::new("Create Library").strong());
        egui::Grid::new("cg-create").show(ui, |ui| {
            ui.label("Library Name:");
            ui.text_edit_singleline(name);
            ui.end_row();
            ui.label("Tuning:");
            let items: Vec<(usize, String)> = std::iter::once((0usize, "Default (new tuning)".to_string())).chain(all.iter().enumerate().map(|(k, t)| (k + 1, t.name.clone()))).collect();
            pick(ui, "cg-ct", tun, &items, 180.0);
            ui.end_row();
            ui.label("Number of Strings:");
            ui.add_enabled(*tun == 0, egui::DragValue::new(strings).range(3..=10));
            ui.end_row();
        });
        let mut done = false;
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                done = true;
            }
            if ui.button("Create").clicked() {
                let lib = Library { name: name.trim().to_string(), grids: Vec::new(), builtin: false };
                if *tun == 0 {
                    // a standard-guitar-like tuning in fourths from E2
                    let s: Vec<u8> = (0..*strings).map(|k| 40 + 5 * k).collect();
                    all.push(Tuning { name: format!("{} Tuning", lib.name), strings: s, libraries: vec![lib] });
                    w.tuning = all.len() - 1;
                    w.target_lib = 0;
                } else if let Some(t) = all.get_mut(*tun - 1) {
                    t.libraries.push(lib);
                    w.tuning = *tun - 1;
                    w.target_lib = t.libraries.len() - 1;
                }
                changed = true;
                done = true;
            }
        });
        if done {
            w.create = None;
        }
    }
    if let Some((ti, li)) = del {
        if let Some(t) = all.get_mut(ti) {
            if let Some(li) = li {
                if li < t.libraries.len() {
                    t.libraries.remove(li);
                }
            }
            if t.libraries.is_empty() {
                all.remove(ti);
            }
        }
        w.target_lib = 0;
        w.tuning = 0;
        changed = true;
    }
    changed
}

/// Tuning, Capo, frets shown, Left-Handed, Action: the rows both Selector and Editor show.
fn common_rows(ui: &mut egui::Ui, w: &mut Win, all: &[Tuning], salt: &str) {
    egui::Grid::new(("cg-common", salt)).num_columns(4).show(ui, |ui| {
        ui.label("Name:");
        let items: Vec<(usize, String)> = all.iter().enumerate().map(|(k, t)| (k, t.name.clone())).collect();
        pick(ui, &format!("cg-name-{salt}"), &mut w.tuning, &items, 150.0);
        ui.label("Capo:");
        let caps: Vec<(u8, String)> = (0..=12u8).map(|k| (k, k.to_string())).collect();
        pick(ui, &format!("cg-capo-{salt}"), &mut w.capo, &caps, 50.0);
        ui.end_row();
        let t = &all[w.tuning.min(all.len() - 1)];
        ui.label("Tuning:");
        ui.label(t.tuning_text());
        ui.label("Number of Strings:");
        ui.label(t.strings.len().to_string());
        ui.end_row();
        ui.label("Number of Frets:");
        let fr: Vec<(u8, String)> = [4u8, 5, 6].iter().map(|&k| (k, k.to_string())).collect();
        pick(ui, &format!("cg-frets-{salt}"), &mut w.frets, &fr, 50.0);
        ui.checkbox(&mut w.left, "Left-Handed");
        ui.end_row();
        ui.label("Action:");
        let acts: Vec<(u8, String)> = ["Chord", "Arpeggio Up", "Arpeggio Down"].iter().enumerate().map(|(k, n)| (k as u8, n.to_string())).collect();
        pick(ui, &format!("cg-act-{salt}"), &mut w.action, &acts, 110.0);
        let tempos: Vec<(u8, String)> = ["Slow", "Medium", "Fast"].iter().enumerate().map(|(k, n)| (k as u8, n.to_string())).collect();
        pick(ui, &format!("cg-tempo-{salt}"), &mut w.tempo, &tempos, 80.0);
        ui.end_row();
    });
}

fn selector(app: &mut App, ui: &mut egui::Ui, w: &mut Win, all: &mut Vec<Tuning>, changed: &mut bool) -> Option<Grid> {
    common_rows(ui, w, all, "sel");
    let t = all[w.tuning].clone();
    ui.horizontal(|ui| {
        ui.label("Root:");
        pick(ui, "cg-root", &mut w.root, &opt_items(&cg::ROOTS), 50.0);
        ui.label("Bass:");
        pick(ui, "cg-bass", &mut w.bass, &opt_items(&cg::ROOTS), 50.0);
        ui.label("Type:");
        pick(ui, "cg-type", &mut w.kind, &opt_items(&cg::TYPES), 60.0);
        ui.label("Difficulty:");
        pick(ui, "cg-diff", &mut w.difficulty, &opt_items(&cg::DIFFICULTY), 60.0);
        ui.checkbox(&mut w.favorites, "Favorites");
        let libs: Vec<(Option<String>, String)> = std::iter::once((None, "All Libraries".to_string())).chain(t.libraries.iter().map(|l| (Some(l.name.clone()), l.name.clone()))).collect();
        let cur = w.library.clone().unwrap_or("All Libraries".into());
        egui::ComboBox::from_id_salt("cg-lib").selected_text(cur).show_ui(ui, |ui| {
            for (v, n) in libs {
                ui.selectable_value(&mut w.library, v, n);
            }
        });
    });
    // the grids that pass the filters
    let shown: Vec<(usize, usize)> = t.libraries.iter().enumerate().filter(|(_, l)| w.library.as_ref().is_none_or(|n| *n == l.name)).flat_map(|(li, l)| {
        l.grids.iter().enumerate().filter(|(_, g)| {
            w.root.is_none_or(|r| r == g.root) && w.bass.is_none_or(|b| b == g.bass || (g.bass == 255 && b == g.root)) && w.kind.is_none_or(|k| k == g.kind) && w.difficulty.is_none_or(|d| d == g.difficulty) && (!w.favorites || g.favorite)
        }).map(move |(gi, _)| (li, gi)).collect::<Vec<_>>()
    }).collect();
    let mut out = None;
    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for &(li, gi) in &shown {
                let g = &t.libraries[li].grids[gi];
                let (r, p) = ui.allocate_painter(vec2(74.0, 96.0), Sense::click());
                let on = w.sel == Some((li, gi));
                p.rect_filled(r.rect, 4.0, if on { look::accent() } else { Color32::from_rgb(0xfe, 0xff, 0xf3) });
                let gr = Rect::from_min_max(r.rect.min + vec2(14.0, 34.0), r.rect.max - vec2(10.0, 8.0));
                let mut shown_g = g.clone();
                shown_g.capo = w.capo;
                draw_grid(&p, gr, &shown_g, w.frets, w.left, true, Color32::from_rgb(0x10, 0x10, 0x10), Color32::WHITE, (None, None));
                if r.clicked() {
                    w.sel = Some((li, gi));
                }
                if r.double_clicked() {
                    w.sel = Some((li, gi));
                    w.edit = g.clone();
                    w.orig = Some(g.clone());
                    w.editing = Some((li, gi));
                    w.target_lib = li;
                    w.pane = 2;
                }
            }
        });
    });
    ui.separator();
    ui.horizontal(|ui| {
        let sel = w.sel.and_then(|(li, gi)| t.libraries.get(li)?.grids.get(gi).cloned());
        if ui.add_enabled(sel.is_some(), egui::Button::new("▶ Play")).clicked() {
            if let Some(g) = &sel {
                let mut g = g.clone();
                g.capo = w.capo;
                play(app, &g, &t.strings, w.action, w.tempo);
            }
        }
        if ui.button("New").clicked() {
            w.edit = Grid::empty(t.strings.len());
            w.orig = None;
            w.editing = None;
            w.pane = 2;
        }
        if ui.add_enabled(sel.is_some(), egui::Button::new("Edit")).clicked() {
            if let (Some(g), Some((li, gi))) = (&sel, w.sel) {
                w.edit = g.clone();
                w.orig = Some(g.clone());
                w.editing = Some((li, gi));
                w.target_lib = li;
                w.pane = 2;
            }
        }
        let user = w.sel.is_some_and(|(li, _)| t.libraries.get(li).is_some_and(|l| !l.builtin));
        if ui.add_enabled(user, egui::Button::new("Delete")).on_hover_text("Deletes a grid you made").clicked() {
            if let Some((li, gi)) = w.sel.take() {
                all[w.tuning].libraries[li].grids.remove(gi);
                *changed = true;
            }
        }
        if w.target.is_some() {
            ui.add_space(20.0);
            if ui.add_enabled(sel.is_some(), egui::Button::new("OK")).clicked() {
                out = sel.map(|mut g| {
                    g.capo = w.capo;
                    g
                });
            }
        }
    });
    out
}

fn editor(app: &mut App, ui: &mut egui::Ui, w: &mut Win, all: &mut Vec<Tuning>) -> bool {
    common_rows(ui, w, all, "ed");
    let t = all[w.tuning].clone();
    if w.edit.frets.len() != t.strings.len() {
        w.edit = Grid::empty(t.strings.len());
    }
    let mut changed = false;
    egui::Grid::new("cg-ed-data").num_columns(4).show(ui, |ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut w.edit.name);
        ui.checkbox(&mut w.edit.favorite, "Favorite");
        ui.end_row();
        let roots: Vec<(u8, String)> = cg::ROOTS.iter().enumerate().map(|(k, n)| (k as u8, n.to_string())).collect();
        let before = w.edit.chord_name();
        ui.label("Root Note:");
        pick(ui, "cg-ed-root", &mut w.edit.root, &roots, 60.0);
        ui.label("Bass Note:");
        let basses: Vec<(u8, String)> = std::iter::once((255u8, "–".to_string())).chain(roots.iter().cloned()).collect();
        pick(ui, "cg-ed-bass", &mut w.edit.bass, &basses, 60.0);
        ui.end_row();
        ui.label("Chord Type:");
        let types: Vec<(u8, String)> = cg::TYPES.iter().enumerate().map(|(k, n)| (k as u8, if n.is_empty() { "Major".to_string() } else { n.to_string() })).collect();
        pick(ui, "cg-ed-type", &mut w.edit.kind, &types, 80.0);
        ui.label("Difficulty:");
        let d: Vec<(u8, String)> = cg::DIFFICULTY.iter().enumerate().map(|(k, n)| (k as u8, n.to_string())).collect();
        pick(ui, "cg-ed-diff", &mut w.edit.difficulty, &d, 80.0);
        ui.end_row();
        // (the name follows the chord data until typed over)
        if w.edit.name.is_empty() || w.edit.name == before {
            w.edit.name = w.edit.chord_name();
        }
    });
    ui.horizontal_top(|ui| {
        // the grid: click a string at a fret for a dot (moved on its string), above the nut to
        // toggle muted / open, Option-click for an optional dot, drag a dot to another fret or
        // sideways for a barre, Control-click for the finger; Delete removes the selected dot
        let first = w.edit.first_fret();
        let (resp, p) = ui.allocate_painter(vec2(220.0, 260.0), Sense::click_and_drag());
        let r = Rect::from_min_max(resp.rect.min + vec2(40.0, 40.0), resp.rect.max - vec2(20.0, 10.0));
        p.rect_filled(resp.rect, 4.0, Color32::from_rgb(0xfe, 0xff, 0xf3));
        let g = geo(r, t.strings.len(), w.frets, w.left);
        let hit = |pp: Pos2| -> Option<(usize, u8)> {
            let s = g.xs.iter().enumerate().min_by(|a, b| (a.1 - pp.x).abs().total_cmp(&(b.1 - pp.x).abs()))?.0;
            if pp.y < g.ys[0] {
                return Some((s, 0));
            }
            let k = g.ys.windows(2).position(|y| pp.y >= y[0] && pp.y < y[1])?;
            Some((s, first + k as u8))
        };
        let alt = ui.input(|i| i.modifiers.alt);
        if resp.drag_started() {
            w.drag = resp.interact_pointer_pos().and_then(hit);
        }
        if resp.drag_stopped() {
            if let (Some((s0, f0)), Some((s1, f1))) = (w.drag.take(), resp.interact_pointer_pos().and_then(hit)) {
                if s0 != s1 && f0 > 0 {
                    // sideways: a barre at the dot's fret
                    let (a, b) = (s0.min(s1) as u8, s0.max(s1) as u8);
                    w.edit.barres.retain(|x| x.fret != f0);
                    w.edit.barres.push(Barre { fret: f0, from: a, to: b });
                    if w.edit.frets[s0] == f0 as i8 {
                        w.edit.frets[s0] = 0;
                    }
                    w.sel_barre = Some(w.edit.barres.len() - 1);
                } else if s0 == s1 && f1 > 0 {
                    // a barre or a dot dragged to another fret
                    if let Some(b) = w.edit.barres.iter_mut().find(|b| b.fret == f0 && (b.from as usize..=b.to as usize).contains(&s0)) {
                        b.fret = f1;
                    } else {
                        w.edit.click(s0, f1);
                    }
                }
            }
        } else if resp.clicked() {
            if let Some((s, f)) = resp.interact_pointer_pos().and_then(hit) {
                if alt && f > 0 {
                    w.edit.optional_click(s, f);
                } else {
                    w.edit.click(s, f);
                }
                w.sel_barre = w.edit.barres.iter().position(|b| b.fret == f && (b.from as usize..=b.to as usize).contains(&s));
                w.sel_dot = (f > 0 && w.sel_barre.is_none()).then_some(s);
                w.cleared = false;
            }
        }
        let rc_pos = resp.interact_pointer_pos();
        resp.context_menu(|ui| {
            if let Some((s, f)) = rc_pos.and_then(hit).filter(|x| x.1 > 0) {
                ui.label(format!("String {} fret {f}", s + 1));
                for (k, n) in ["None", "1", "2", "3", "4", "Thumb"].iter().enumerate() {
                    if ui.button(*n).clicked() {
                        w.edit.fingers[s] = k as u8;
                        ui.close();
                    }
                }
            }
        });
        if crate::focus::in_window(app, ui) && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) && !ui.ctx().egui_wants_keyboard_input() {
            if let Some(b) = w.sel_barre.take() {
                if b < w.edit.barres.len() {
                    w.edit.barres.remove(b);
                }
            } else if let Some(s) = w.sel_dot.take() {
                w.edit.frets[s] = 0;
                w.edit.fingers[s] = 0;
            }
        }
        draw_grid(&p, r, &w.edit, w.frets, w.left, false, Color32::from_rgb(0x10, 0x10, 0x10), Color32::WHITE, (w.sel_dot, w.sel_barre));
        // the first fret's label: a pop-up for higher positions
        ui.vertical(|ui| {
            ui.label("First fret:");
            let mut ff = first;
            let fr: Vec<(u8, String)> = (1..=15u8).map(|k| (k, format!("{k}"))).collect();
            pick(ui, "cg-ed-first", &mut ff, &fr, 50.0);
            if ff != first {
                // move the whole shape
                let d = ff as i32 - first as i32;
                for f in w.edit.frets.iter_mut().filter(|f| **f > 0) {
                    *f = (*f as i32 + d).clamp(1, 24) as i8;
                }
                for b in &mut w.edit.barres {
                    b.fret = (b.fret as i32 + d).clamp(1, 24) as u8;
                }
            }
            ui.add_space(10.0);
            if ui.button("▶ Play").clicked() {
                let mut g = w.edit.clone();
                g.capo = w.capo;
                play(app, &g, &t.strings, w.action, w.tempo);
            }
            // Clear empties the grid and turns into Reset (back to how it was)
            if w.cleared {
                if ui.button("Reset").clicked() {
                    w.edit = w.orig.clone().unwrap_or_else(|| Grid::empty(t.strings.len()));
                    w.cleared = false;
                }
            } else if ui.button("Clear").clicked() {
                let name = w.edit.name.clone();
                w.edit = Grid { name, ..Grid::empty(t.strings.len()) };
                w.cleared = true;
            }
            ui.add_space(10.0);
            ui.label("Target Library:");
            let libs: Vec<(usize, String)> = t.libraries.iter().enumerate().filter(|(_, l)| !l.builtin).map(|(k, l)| (k, l.name.clone())).chain(std::iter::once((usize::MAX, "New “My Chords”".to_string()))).collect();
            if !libs.iter().any(|x| x.0 == w.target_lib) {
                w.target_lib = libs[0].0;
            }
            pick(ui, "cg-ed-target", &mut w.target_lib, &libs, 140.0);
            let lib_index = |all: &mut Vec<Tuning>, w: &Win| -> usize {
                let tl = &mut all[w.tuning].libraries;
                if w.target_lib == usize::MAX || w.target_lib >= tl.len() {
                    tl.push(Library { name: "My Chords".into(), grids: Vec::new(), builtin: false });
                    tl.len() - 1
                } else {
                    w.target_lib
                }
            };
            let can_replace = w.editing.is_some_and(|(li, _)| t.libraries.get(li).is_some_and(|l| !l.builtin));
            ui.horizontal(|ui| {
                if ui.add_enabled(can_replace, egui::Button::new("Replace")).clicked() {
                    if let Some((li, gi)) = w.editing {
                        all[w.tuning].libraries[li].grids[gi] = w.edit.clone();
                        changed = true;
                        w.sel = Some((li, gi));
                        w.pane = 1;
                    }
                }
                if ui.button("Add").clicked() {
                    let li = lib_index(all, w);
                    all[w.tuning].libraries[li].grids.push(w.edit.clone());
                    w.sel = Some((li, all[w.tuning].libraries[li].grids.len() - 1));
                    changed = true;
                    w.pane = 1;
                }
            });
        });
    });
    changed
}

/// The scale of a chord grid symbol (its shortcut menu: reduced, normal, enlarged).
pub fn scale_of(style: &str) -> f32 {
    match style {
        "reduced" => 0.75,
        "enlarged" => 1.35,
        _ => 1.0,
    }
}

/// A chord grid symbol on a staff (above it) at `x`; returns its rect.
pub fn draw_symbol(p: &egui::Painter, s: &lg_core::score::Symbol, x: f32, top: f32, sp: f32, ink: Color32, paper: Color32) -> Option<Rect> {
    let g = Grid::decode(&s.text)?;
    // (normal: a grid of 0.75 staff spaces per string, 1.5 staff spaces above the staff)
    let k = 0.75 * sp * scale_of(&s.style);
    let w = (g.frets.len().max(2) - 1) as f32 * 1.1 * k;
    let h = 5.0 * 1.3 * k;
    let at = pos2(x + s.dx as f32 * sp / 10.0, top - 1.5 * sp - h - s.dy as f32 * sp / 10.0);
    let r = Rect::from_min_size(at, vec2(w, h));
    draw_grid(p, r, &g, 5, false, s.align & 1 == 0, ink, paper, (None, None));
    Some(r.expand2(vec2(0.6 * k, 0.0)).union(Rect::from_min_max(pos2(r.min.x, r.min.y - 3.6 * k), r.max)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_mirrors_left_handed() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(50.0, 50.0));
        let g = geo(r, 6, 5, false);
        let l = geo(r, 6, 5, true);
        assert_eq!((g.xs[0], g.xs[5]), (0.0, 50.0));
        assert_eq!((l.xs[0], l.xs[5]), (50.0, 0.0));
        assert_eq!(g.ys.len(), 6);
        assert_eq!(scale_of("reduced") < scale_of("") && scale_of("") < scale_of("enlarged"), true);
    }
}
