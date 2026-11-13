//! Track Import (File ▸ Import ▸ Logic Projects…, or a project selected in the All Files
//! Browser and its Import button), as Logic's: the other project's tracks and aux channel strips
//! in a table in the All Files Browser, with filter buttons above it (which kinds are listed)
//! and the columns Num, Name, Type, Content, Cells, Plug-ins, Sends, I/O, Keep Bus Number,
//! Automation and Notes. Tracks in a stack number as 3-1, 3-2 under their main track (double-
//! click the main track to show them); channel strips with no track have no number. Right Arrow
//! ticks a row's boxes, Left Arrow clears them. Add (⌘↓) puts the ticked data on new tracks
//! below the selected track; Replace puts one row's ticked data on the selected track. With
//! Keep Bus Number off, imported sends and outputs move to buses not used here. Control-click
//! the header to show or hide columns (kept in Studio's settings, for every project). Import
//! Project Settings… takes the project's settings too.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::{Project, Track, TrackKind};

use crate::app::App;
use crate::look;

/// The commands this file runs (build.rs and tool/checklist.py read the list).
pub const COMMANDS: &[&str] = &["Logic Projects…", "Import Logic Projects…"];

/// The table's columns after Num, Name and Type: the ticked data imported.
pub const DATA: [&str; 8] = ["Content", "Cells", "Plug-ins", "Sends", "I/O", "Keep Bus Number", "Automation", "Notes"];
const CONTENT: usize = 0;
const CELLS: usize = 1;
const PLUGINS: usize = 2;
const SENDS: usize = 3;
const IO: usize = 4;
const KEEP_BUS: usize = 5;
const AUTOMATION: usize = 6;
const NOTES: usize = 7;
/// The filter buttons: (label, which row types they show).
pub const FILTERS: [&str; 4] = ["Audio", "Instrument", "Drummer", "Aux"];

#[derive(Clone, Debug, PartialEq)]
pub enum What {
    Track(usize),
    Aux(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub what: What,
    /// "3", "3-1"; empty for a channel strip without a track.
    pub num: String,
    pub name: String,
    pub ty: &'static str,
    /// A subtrack: the main track's row.
    pub parent: Option<usize>,
    pub checks: [bool; 8],
}

pub struct Source {
    pub path: PathBuf,
    pub project: Project,
    pub rows: Vec<Row>,
    /// Main tracks whose subtracks are shown.
    pub open: HashSet<usize>,
}

#[derive(Default)]
pub struct State {
    pub src: Option<Source>,
    pub selected: Option<usize>,
    /// Filter buttons switched off.
    pub off: HashSet<String>,
    pub hidden_cols: Vec<String>,
    loaded: bool,
}

fn type_of(t: &Track) -> &'static str {
    match t.kind {
        TrackKind::Audio => "Audio",
        TrackKind::Drummer => "Drummer",
        TrackKind::Instrument => "Instrument",
    }
}

/// The table's rows for a project: its tracks (subtracks numbered under their main track) and
/// then its aux strips.
pub fn rows_of(p: &Project) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    let mut top = 0usize;
    // (the main track rows by depth, to number the subtracks under them)
    let mut parents: Vec<(u8, usize, usize)> = Vec::new();
    for (i, t) in p.tracks.iter().enumerate() {
        let d = t.extra.depth;
        while parents.last().is_some_and(|x| x.0 >= d) {
            parents.pop();
        }
        let (num, parent) = match parents.last_mut() {
            Some((_, row, kids)) => {
                *kids += 1;
                (format!("{}-{}", rows[*row].num, kids), Some(*row))
            }
            None => {
                top += 1;
                (top.to_string(), None)
            }
        };
        rows.push(Row { what: What::Track(i), num, name: t.name.clone(), ty: if t.extra.stack.is_some() { "Folder" } else { type_of(t) }, parent, checks: [false; 8] });
        if t.extra.stack.is_some() {
            parents.push((d, rows.len() - 1, 0));
        }
    }
    for (s, st) in p.mixer.strips.iter().enumerate() {
        if st.kind == lg_core::mixer::StripKind::Aux {
            rows.push(Row { what: What::Aux(s), num: String::new(), name: if st.name.is_empty() { format!("Aux {}", st.number + 1) } else { st.name.clone() }, ty: "Aux", parent: None, checks: [false; 8] });
        }
    }
    rows
}

/// The sends of a source track (bus, level, pre).
fn source_sends(p: &Project, t: usize) -> Vec<crate::app::SendSlot> {
    let Some(Some(s)) = p.mixer.track_strip.get(t) else { return Vec::new() };
    p.mixer.sends_of(*s).into_iter().map(|x| crate::app::SendSlot { bus: x.bus, level_db: x.level_db, pre: x.pre }).collect()
}

fn source_output(p: &Project, t: usize) -> Option<usize> {
    let s = (*p.mixer.track_strip.get(t)?)?;
    p.mixer.strips.get(s)?.output_bus
}

/// Keep Bus Number off: each source bus moves to the lowest bus not in `used` (and not given
/// already).
pub fn reassign_buses(buses: &[usize], used: &dyn Fn(usize) -> bool) -> Vec<(usize, usize)> {
    let mut map: Vec<(usize, usize)> = Vec::new();
    let mut next = 0usize;
    for &b in buses {
        if map.iter().any(|m| m.0 == b) {
            continue;
        }
        while used(next) || map.iter().any(|m| m.1 == next) {
            next += 1;
        }
        map.push((b, next));
    }
    map
}

fn config() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("track-import.txt"))
}

fn ensure(app: &mut App) {
    if app.tim.loaded {
        return;
    }
    app.tim.loaded = true;
    if let Some(t) = config().and_then(|f| std::fs::read_to_string(f).ok()) {
        app.tim.hidden_cols = t.lines().filter_map(|l| l.strip_prefix("hide=")).map(str::to_string).collect();
    }
}

fn save_cols(app: &App) {
    let Some(f) = config() else { return };
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, app.tim.hidden_cols.iter().map(|c| format!("hide={c}\n")).collect::<String>());
}

/// Reads another project for Track Import.
pub fn open_source(app: &mut App, path: &Path) -> bool {
    ensure(app);
    let pkg = lg_core::project::package_of(path);
    let read = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("band")) { lg_core::project::open_band(path) } else { lg_core::project::open(&pkg) };
    match read {
        Ok(p) => {
            let rows = rows_of(&p);
            app.tim.src = Some(Source { path: pkg, project: p, rows, open: HashSet::new() });
            app.tim.selected = None;
            // (shown in the All Files Browser)
            app.right_pane = Some(crate::app::RightPane::Files);
            app.pf_tab = 1;
            true
        }
        Err(e) => {
            app.tell(format!("Couldn't read the project: {e}"));
            false
        }
    }
}

pub fn active(app: &App) -> bool {
    app.tim.src.is_some()
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Logic Projects…" | "Import Logic Projects…" => {
            if app.automated {
                if let Some(p) = std::env::var_os("LG_TRACK_IMPORT") {
                    open_source(app, Path::new(&p));
                }
                return true;
            }
            if let Some(p) = kn_ui::files::FileDialog::new().set_title("Import tracks from a Logic Pro or GarageBand project").pick_folder() {
                open_source(app, &p);
            }
            true
        }
        _ => false,
    }
}

/// The rows shown: filtered by type, subtracks only under an open main track.
fn visible(s: &Source, off: &HashSet<String>) -> Vec<usize> {
    (0..s.rows.len())
        .filter(|&i| {
            let r = &s.rows[i];
            let shown_type = match r.ty {
                "Folder" => true,
                t => !off.contains(t),
            };
            let mut up = r.parent;
            let mut open = true;
            while let Some(pp) = up {
                open &= s.open.contains(&pp);
                up = s.rows[pp].parent;
            }
            shown_type && open
        })
        .collect()
}

/// Builds the new track for row `r` from the ticked data.
fn build_track(p: &Project, r: &Row) -> Option<Track> {
    let What::Track(t) = r.what else { return None };
    let src = p.tracks.get(t)?;
    let mut nt = Track::new(&src.name, src.kind);
    nt.volume_db = src.volume_db;
    nt.pan = src.pan;
    nt.extra.set = src.extra.set.clone();
    if r.checks[CONTENT] {
        nt.regions = src.regions.iter().cloned().map(|mut rg| {
            // (another file's regions: saved as new ones)
            rg.source = None;
            rg.origin = None;
            rg
        }).collect();
    }
    if r.checks[PLUGINS] {
        nt.plugins = src.plugins.clone();
        nt.extra.midi_fx = src.extra.midi_fx.clone();
    }
    if r.checks[AUTOMATION] {
        nt.automation = src.automation.clone();
    }
    Some(nt)
}

/// Add: the ticked rows as new tracks below the selected track (aux rows: new aux strips).
pub fn add(app: &mut App) {
    let Some(s) = app.tim.src.as_ref() else { return };
    let rows: Vec<Row> = s.rows.iter().filter(|r| r.checks.iter().any(|c| *c)).cloned().collect();
    if rows.is_empty() {
        app.tell("Tick what to import first");
        return;
    }
    let src = s.project.clone();
    app.checkpoint("Import Tracks");
    let n_now = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let mut at = app.selected_track.map_or(n_now, |t| app.project.as_ref().map_or(n_now, |p| lg_core::arrange::group(&p.tracks, t).end));
    // the buses the imported sends and outputs use, and where they go
    let mut buses: Vec<usize> = Vec::new();
    for r in &rows {
        match r.what {
            What::Track(t) => {
                if r.checks[SENDS] {
                    buses.extend(source_sends(&src, t).iter().map(|x| x.bus));
                }
                if r.checks[IO] {
                    buses.extend(source_output(&src, t));
                }
            }
            What::Aux(s) => buses.extend(src.mixer.strips.get(s).and_then(|st| st.input_bus)),
        }
    }
    let keep = rows.iter().any(|r| r.checks[KEEP_BUS]);
    let map: Vec<(usize, usize)> = if keep { buses.iter().map(|b| (*b, *b)).collect() } else { reassign_buses(&buses, &|b| crate::mixstate::bus_used(app, b)) };
    let bus = |b: usize| map.iter().find(|m| m.0 == b).map_or(b, |m| m.1);
    let mut added = 0;
    let mut takes: Vec<(usize, usize)> = Vec::new();
    for r in &rows {
        match r.what {
            What::Track(t) => {
                let Some(nt) = build_track(&src, r) else { continue };
                if let Some(p) = app.project.as_mut() {
                    // (take folders on later tracks move down)
                    for tf in p.takes.iter_mut().filter(|tf| tf.track >= at) {
                        tf.track += 1;
                    }
                    p.tracks.insert(at.min(p.tracks.len()), nt);
                }
                app.programs.insert(at.min(app.programs.len()), 0);
                crate::trackarea::sync(app);
                if r.checks[SENDS] {
                    let v: Vec<crate::app::SendSlot> = source_sends(&src, t).into_iter().map(|mut x| {
                        x.bus = bus(x.bus);
                        x
                    }).collect();
                    if !v.is_empty() {
                        app.sends.insert(at, v);
                    }
                }
                if r.checks[IO] {
                    if let Some(b) = source_output(&src, t) {
                        app.mix.outputs.insert(at, crate::audio::Out::Bus(bus(b)));
                    }
                }
                if r.checks[CONTENT] {
                    takes.push((t, at));
                }
                at += 1;
                added += 1;
            }
            What::Aux(s) => {
                let Some(st) = src.mixer.strips.get(s) else { continue };
                let b = st.input_bus.map(bus);
                let k = crate::mixstate::new_aux(app, b);
                if let Some(a) = app.auxes.get_mut(k) {
                    a.name = st.name.clone();
                    a.volume_db = st.volume_db;
                    a.pan = st.pan;
                    if r.checks[PLUGINS] {
                        a.inserts = st.plugins.iter().filter(|p| p.slot >= 3).cloned().collect();
                    }
                }
                added += 1;
            }
        }
    }
    // the take folders (with all their takes and comps)
    if let Some(p) = app.project.as_mut() {
        for (from, to) in takes {
            for tf in src.takes.iter().filter(|tf| tf.track == from) {
                let mut tf = tf.clone();
                tf.track = to;
                p.takes.push(tf);
            }
        }
    }
    app.dirty = true;
    app.load_engine();
    app.tell(format!("Imported {added} {}", if added == 1 { "track" } else { "tracks" }));
}

fn s_path(app: &App) -> PathBuf {
    app.tim.src.as_ref().map(|s| s.path.clone()).unwrap_or_default()
}

/// Replace: one row's ticked data onto the selected track.
pub fn replace(app: &mut App) {
    let Some(s) = app.tim.src.as_ref() else { return };
    let ticked: Vec<&Row> = s.rows.iter().filter(|r| r.checks.iter().any(|c| *c)).collect();
    if ticked.len() != 1 {
        app.tell("Replace works with one row ticked");
        return;
    }
    let Some(dst) = app.selected_track else {
        app.tell("Select the track to replace first");
        return;
    };
    let r = ticked[0].clone();
    let src = s.project.clone();
    let What::Track(t) = r.what else { return };
    let Some(nt) = build_track(&src, &r) else { return };
    app.checkpoint("Replace Track");
    if let Some(p) = app.project.as_mut() {
        if let Some(d) = p.tracks.get_mut(dst) {
            if r.checks[CONTENT] {
                d.regions = nt.regions;
            }
            if r.checks[PLUGINS] {
                d.plugins = nt.plugins;
                d.extra.midi_fx = nt.extra.midi_fx;
                d.kind = nt.kind;
            }
            if r.checks[AUTOMATION] {
                d.automation = nt.automation;
            }
        }
    }
    if r.checks[SENDS] {
        app.sends.insert(dst, source_sends(&src, t));
    }
    if r.checks[IO] {
        if let Some(b) = source_output(&src, t) {
            app.mix.outputs.insert(dst, crate::audio::Out::Bus(b));
        }
    }
    app.dirty = true;
    app.load_engine();
}

/// Right Arrow / Left Arrow: the selected row's boxes all ticked / cleared; ⌘↓ adds.
pub fn keys(app: &mut App, ctx: &egui::Context) {
    if !active(app) || app.tim.selected.is_none() || ctx.egui_wants_keyboard_input() {
        return;
    }
    let (right, left, add_key) = ctx.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight), i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft), i.consume_key(egui::Modifiers::COMMAND, egui::Key::ArrowDown)));
    if let (Some(k), Some(s)) = (app.tim.selected, app.tim.src.as_mut()) {
        if let Some(r) = s.rows.get_mut(k) {
            if right {
                r.checks = [true; 8];
                r.checks[CELLS] = false;
                r.checks[NOTES] = false;
            }
            if left {
                r.checks = [false; 8];
            }
        }
    }
    if add_key {
        add(app);
    }
}

/// The Track Import view (in the All Files Browser's list area).
pub fn view(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    ensure(app);
    let p = ui.painter().clone();
    p.rect_filled(area, 0.0, look::pane_bg());
    let Some(name) = app.tim.src.as_ref().map(|s| s.path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()) else { return };
    // the title and the filter buttons
    p.text(pos2(area.min.x + 8.0, area.min.y + 12.0), Align2::LEFT_CENTER, format!("Track Import: {name}"), look::semibold(12.0), look::text());
    let mut x = area.min.x + 8.0;
    let fy = area.min.y + 26.0;
    for f in FILTERS {
        let w = p.layout_no_wrap(f.to_string(), look::regular(11.0), look::text()).size().x + 14.0;
        let b = Rect::from_min_size(pos2(x, fy), vec2(w, 20.0));
        let on = !app.tim.off.contains(f);
        p.rect_filled(b, 5.0, if on { look::accent() } else { look::group_bg() });
        p.text(b.center(), Align2::CENTER_CENTER, f, look::regular(11.0), if on { Color32::WHITE } else { look::text_dim() });
        if ui.interact(b, ui.scope_id().with(("ti-filter", f)), Sense::click()).on_hover_text(format!("Show {f} tracks")).clicked() {
            if on {
                app.tim.off.insert(f.to_string());
            } else {
                app.tim.off.remove(f);
            }
        }
        x = b.max.x + 4.0;
    }
    // the header
    let head = Rect::from_min_size(pos2(area.min.x, fy + 26.0), vec2(area.width(), 20.0));
    p.rect_filled(head, 0.0, look::header_bg());
    let hidden = app.tim.hidden_cols.clone();
    let mut cols: Vec<(&str, f32)> = vec![("Num", 34.0), ("Name", 0.0), ("Type", 66.0)];
    for d in DATA {
        if !hidden.iter().any(|h| h == d) {
            cols.push((d, 54.0));
        }
    }
    let fixed: f32 = cols.iter().map(|c| c.1).sum();
    let name_w = (area.width() - fixed).max(90.0);
    let mut xs: Vec<(String, f32, f32)> = Vec::new();
    let mut cx = area.min.x;
    for (c, w) in &cols {
        let w = if *c == "Name" { name_w } else { *w };
        xs.push((c.to_string(), cx, w));
        cx += w;
    }
    for (c, x0, w) in &xs {
        let r = Rect::from_min_size(pos2(*x0, head.min.y), vec2(*w, head.height()));
        let short = match c.as_str() {
            "Keep Bus Number" => "Keep Bus",
            "Automation" => "Auto",
            "Plug-ins" => "Plug-ins",
            o => o,
        };
        p.with_clip_rect(r).text(pos2(r.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, short, look::regular(10.5), look::text_dim());
        p.vline(r.min.x, (r.min.y + 3.0)..=(r.max.y - 3.0), Stroke::new(1.0, look::line()));
    }
    let hresp = ui.interact(head, ui.scope_id().with("ti-head"), Sense::click());
    egui::Popup::context_menu(&hresp).show(|ui| {
        for d in DATA {
            let on = !app.tim.hidden_cols.iter().any(|h| h == d);
            if ui.add(egui::Button::selectable(on, d)).clicked() {
                if on {
                    app.tim.hidden_cols.push(d.to_string());
                } else {
                    app.tim.hidden_cols.retain(|h| h != d);
                }
                save_cols(app);
                ui.close();
            }
        }
    });
    // the rows
    let buttons_h = 34.0;
    let list = Rect::from_min_max(pos2(area.min.x, head.max.y), pos2(area.max.x, area.max.y - buttons_h));
    let shown = app.tim.src.as_ref().map(|s| visible(s, &app.tim.off)).unwrap_or_default();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    let mut toggle_open: Option<usize> = None;
    let mut select: Option<usize> = None;
    let mut tick: Option<(usize, usize)> = None;
    egui::ScrollArea::vertical().id_salt("ti-rows").auto_shrink([false, false]).show_rows(&mut child, 22.0, shown.len(), |ui, range| {
        for k in range {
            let i = shown[k];
            let Some(r) = app.tim.src.as_ref().map(|s| s.rows[i].clone()) else { continue };
            let (rr, resp) = ui.allocate_exact_size(vec2(list.width(), 22.0), Sense::click());
            let pp = ui.painter();
            if app.tim.selected == Some(i) {
                pp.rect_filled(rr, 0.0, look::accent().gamma_multiply(0.45));
            } else if k % 2 == 1 {
                pp.rect_filled(rr, 0.0, look::lane_alt());
            }
            for (c, x0, w) in &xs {
                let cell = Rect::from_min_size(pos2(*x0, rr.min.y), vec2(*w, rr.height()));
                match c.as_str() {
                    "Num" => {
                        pp.text(pos2(cell.min.x + 4.0, cell.center().y), Align2::LEFT_CENTER, &r.num, look::regular(11.0), look::text_dim());
                    }
                    "Name" => {
                        let indent = if r.parent.is_some() { 14.0 } else { 0.0 };
                        let folder = r.ty == "Folder";
                        if folder {
                            let open = app.tim.src.as_ref().is_some_and(|s| s.open.contains(&i));
                            crate::glyphs::paint(pp, Rect::from_min_size(pos2(cell.min.x + 2.0, cell.min.y + 4.0), vec2(14.0, 14.0)), if open { crate::glyphs::ph::CARET_DOWN } else { crate::glyphs::ph::CARET_RIGHT }, 10.0, look::text_dim());
                        }
                        pp.with_clip_rect(cell).text(pos2(cell.min.x + 18.0 + indent, cell.center().y), Align2::LEFT_CENTER, &r.name, look::regular(12.0), look::text());
                    }
                    "Type" => {
                        pp.text(pos2(cell.min.x + 4.0, cell.center().y), Align2::LEFT_CENTER, r.ty, look::regular(11.0), look::text_dim());
                    }
                    d => {
                        let Some(j) = DATA.iter().position(|x| *x == d) else { continue };
                        // (Cells and Notes: Studio doesn't read another project's Live Loops
                        // cells or track notes; Content and Automation need a track)
                        let usable = j != CELLS && j != NOTES && !(matches!(r.what, What::Aux(_)) && matches!(j, CONTENT | AUTOMATION | NOTES | SENDS | IO));
                        let b = Rect::from_center_size(cell.center(), vec2(13.0, 13.0));
                        let on = r.checks[j];
                        pp.rect_filled(b, 3.0, if on { look::accent() } else { look::group_bg() });
                        pp.rect_stroke(b, 3.0, Stroke::new(1.0, if usable { look::text_dim() } else { look::line() }), egui::StrokeKind::Inside);
                        if on {
                            crate::glyphs::paint(pp, b, crate::glyphs::ph::CHECK, 10.0, Color32::WHITE);
                        }
                        if usable && ui.interact(b.expand(3.0), ui.scope_id().with(("ti-check", i, j)), Sense::click()).clicked() {
                            tick = Some((i, j));
                        }
                    }
                }
            }
            if resp.clicked() {
                select = Some(i);
            }
            if resp.double_clicked() && r.ty == "Folder" {
                toggle_open = Some(i);
            }
        }
    });
    if let Some(s) = app.tim.src.as_mut() {
        if let Some((i, j)) = tick {
            s.rows[i].checks[j] = !s.rows[i].checks[j];
        }
        if let Some(i) = toggle_open {
            if !s.open.remove(&i) {
                s.open.insert(i);
            }
        }
    }
    if let Some(i) = select {
        app.tim.selected = Some(i);
    }
    // the buttons: Import Project Settings…, Close · Replace, Add
    let by = area.max.y - buttons_h + 6.0;
    let mut bx = area.max.x - 8.0;
    let ticked = app.tim.src.as_ref().map_or(0, |s| s.rows.iter().filter(|r| r.checks.iter().any(|c| *c)).count());
    let mut run: Option<&str> = None;
    for (label, enabled) in [("Add", ticked > 0), ("Replace", ticked == 1 && app.selected_track.is_some())] {
        let w = p.layout_no_wrap(label.to_string(), look::regular(12.0), look::text()).size().x + 22.0;
        let b = Rect::from_min_size(pos2(bx - w, by), vec2(w, 22.0));
        let resp = ui.interact(b, ui.scope_id().with(("ti-btn", label)), Sense::click());
        p.rect_filled(b, 5.0, if label == "Add" && enabled { look::accent() } else if resp.hovered() && enabled { look::button_on() } else { look::group_bg() });
        p.text(b.center(), Align2::CENTER_CENTER, label, look::regular(12.0), if enabled { if label == "Add" { Color32::WHITE } else { look::text() } } else { look::text_dim() });
        if resp.clicked() && enabled {
            run = Some(label);
        }
        bx = b.min.x - 6.0;
    }
    let mut lx = area.min.x + 8.0;
    for label in ["Import Project Settings…", "Done"] {
        let w = p.layout_no_wrap(label.to_string(), look::regular(12.0), look::text()).size().x + 22.0;
        let b = Rect::from_min_size(pos2(lx, by), vec2(w, 22.0));
        let resp = ui.interact(b, ui.scope_id().with(("ti-btn", label)), Sense::click());
        p.rect_filled(b, 5.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
        p.text(b.center(), Align2::CENTER_CENTER, label, look::regular(12.0), look::text());
        if resp.clicked() {
            run = Some(label);
        }
        lx = b.max.x + 6.0;
    }
    match run {
        Some("Add") => add(app),
        Some("Replace") => replace(app),
        Some("Done") => app.tim.src = None,
        Some(_) => {
            let path = s_path(app);
            crate::projsettings::import_from(app, path);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_number_subtracks_and_list_auxes() {
        let mut p = crate::demo::song();
        let n = p.tracks.len();
        assert!(n >= 3);
        p.tracks[1].extra.stack = Some(lg_core::arrange::Stack { kind: lg_core::arrange::StackKind::Folder, open: true });
        p.tracks[2].extra.depth = 1;
        let rows = rows_of(&p);
        assert_eq!(rows[0].num, "1");
        assert_eq!(rows[1].num, "2");
        assert_eq!(rows[1].ty, "Folder");
        assert_eq!(rows[2].num, "2-1");
        assert_eq!(rows[2].parent, Some(1));
        if n > 3 {
            assert_eq!(rows[3].num, "3");
        }
        // subtracks show only under an open main track
        let mut s = Source { path: PathBuf::new(), project: p.clone(), rows, open: HashSet::new() };
        assert!(!visible(&s, &HashSet::new()).contains(&2));
        s.open.insert(1);
        assert!(visible(&s, &HashSet::new()).contains(&2));
    }

    #[test]
    fn buses_move_to_unused_ones() {
        let used = |b: usize| b == 0 || b == 1;
        assert_eq!(reassign_buses(&[0, 5, 0], &used), vec![(0, 2), (5, 3)]);
        let none = |_: usize| false;
        assert_eq!(reassign_buses(&[3], &none), vec![(3, 0)]);
    }

    #[test]
    fn ticked_data_builds_the_track() {
        let p = crate::demo::song();
        let mut r = rows_of(&p)[0].clone();
        let bare = build_track(&p, &r).unwrap();
        assert!(bare.regions.is_empty() && bare.automation.is_empty());
        assert_eq!(bare.name, p.tracks[0].name);
        r.checks[CONTENT] = true;
        let full = build_track(&p, &r).unwrap();
        assert_eq!(full.regions.len(), p.tracks[0].regions.len());
        assert!(full.regions.iter().all(|x| x.source.is_none()));
        assert_eq!(full.extra.id, 0);
    }
}
