//! The Sampler's window, as Logic's: one scrolling window with a Navigation bar (Synth, Mod
//! Matrix, Modulators, Mapping, Zone: click scrolls to the pane, the LED hides it, double-click
//! maximizes it, Option-click shows only it; the Action menu at the right; Chromatic and
//! Optimized drop zones while files are dragged in), and the panes:
//!
//! - Synth: Tune, Fine, two filters (on, type, Cutoff, Resonance, Drive), Series / Parallel,
//!   Filter Blend, Volume, Pan; Details (Glide, Pitch Bend Up / Down with Linked, Coarse Tune
//!   Remote, Transpose, Sample Select / Velocity Random, Amp Velocity Curve, Velocity Offset,
//!   Ignore Release Velocity, Amp Key Scale, Polyphony, Mode, Unison, Random Detune, Used Voices);
//!   right-click a knob: Add Modulation, Show Modulation.
//! - Mod Matrix: twenty routings (on, Target, Source + Inv, Via + Inv, Amount or Min / Max),
//!   the view filter, − / +, sorting by Source / Via / Target.
//! - Modulators: Envelope 1–5 (AHDSR / AR, display with handles) and LFO 1–4 (waveform, sync,
//!   rate, fade in / out, phase, mono / poly, unipolar / bipolar, key trigger), + ENV, + LFO, −.
//! - Mapping: the Edit / Group / Zone / View menus, Preview, zoom, and three views: the Key
//!   Mapping Editor (group list, zones on the keyboard grid, drag to move / resize, ⌘⇧-drag
//!   draws a zone, ⌘⌥ / ⇧⌥-click splits, files dropped on keys or above them), Group view and
//!   Zone view (every parameter in columns, sortable, columns hidden / shown).
//! - Zone: the zone's waveform with sample start / end (blue), fades (gray), loop markers and
//!   crossfade (yellow), Option-drag moves pairs, the loop area drags; Loop Mode, One Shot,
//!   Reverse, Play to End on Release, the fields; Edit (Loop commands, Auto-Loop, Retune, Crop)
//!   and View menus, Snap, zoom buttons, File pop-up.

use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_synth::sampler::LoopDir;
use lg_synth::sampler_doc::{self as sd, Drop, Group, SamplerDoc, XfadeType, ZoneDoc};
use lg_synth::sampler_synth::{self as ss, target};

use crate::app::App;
use crate::instwin;
use crate::look;

const PANES: [&str; 5] = ["Synth", "Mod Matrix", "Modulators", "Mapping", "Zone"];
const DEFAULT_H: [f32; 5] = [210.0, 170.0, 230.0, 330.0, 260.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MapView {
    #[default]
    Keyboard,
    Group,
    Zone,
}

#[derive(Clone, Debug)]
enum KDrag {
    Move { start: egui::Pos2, orig: Vec<(usize, ZoneDoc)>, axis: Option<bool>, root_only: bool },
    Edge { zone: usize, edge: u8, orig: ZoneDoc },
    Draw { start: egui::Pos2 },
    Band { start: egui::Pos2 },
}

#[derive(Clone, Debug)]
enum WDrag {
    Marker { which: u8, orig: ZoneDoc, x0: f32, both: bool },
    LoopArea { orig: ZoneDoc, x0: f32 },
    Zoom { x0: f32 },
}

#[derive(Clone, Debug)]
pub enum Dialog {
    RemapNotes { start: f32, width: f32 },
    RemapVelocities { lo: f32, hi: f32 },
    KeyLabels,
    OutputLabels,
    SaveAs { name: String, copy: bool, consolidated: bool, a_copy: bool },
    Rename { zone: usize, name: String },
    RenameGroup { group: usize, name: String },
}

pub struct Ui {
    pub view: MapView,
    pub hidden: [bool; 5],
    pub heights: [f32; 5],
    pub maxed: Option<usize>,
    pub scroll_to: Option<usize>,
    pub pane_rects: [Option<Rect>; 5],
    pub details: bool,
    pub sel_zones: Vec<usize>,
    pub sel_groups: Vec<usize>,
    pub focus_zone: Option<usize>,
    pub focus_group: Option<usize>,
    pub key_w: f32,
    pub group_list: bool,
    pub show_names: bool,
    pub all_groups_zones: bool,
    pub select_from_played: u8,
    pub preview_selected: bool,
    pub protect_unselected: bool,
    pub hidden_cols: std::collections::HashSet<String>,
    pub sort: Option<(MapView, usize, bool)>,
    pub mm_filter_on: bool,
    pub mm_filter: (u8, usize),
    pub mm_last_clicked: bool,
    pub highlighted_mod: Option<(bool, usize)>,
    pub dialog: Option<Dialog>,
    kdrag: Option<KDrag>,
    wdrag: Option<WDrag>,
    pub wave_view: Option<(f64, f64)>,
    pub wave_vmax: bool,
    pub snap: bool,
    pub units: u8,
    pub mono_sum: bool,
    pub show_anchor: bool,
    pub zone_clip: Option<Vec<ZoneDoc>>,
    pub group_clip: Option<Vec<(Group, Vec<ZoneDoc>)>>,
    pub autoloop_attempt: usize,
    in_gesture: bool,
    preview_held: Option<u8>,
    peaks: Option<(usize, i64, i64, usize, bool, Vec<[(f32, f32); 2]>)>,
    pub default_inst: Option<PathBuf>,
}

impl Default for Ui {
    fn default() -> Self {
        Ui {
            view: MapView::Keyboard,
            hidden: [false; 5],
            heights: DEFAULT_H,
            maxed: None,
            scroll_to: None,
            pane_rects: [None; 5],
            details: false,
            sel_zones: Vec::new(),
            sel_groups: vec![0],
            focus_zone: None,
            focus_group: Some(0),
            key_w: 9.0,
            group_list: true,
            show_names: true,
            all_groups_zones: false,
            select_from_played: 0,
            preview_selected: false,
            protect_unselected: false,
            hidden_cols: Default::default(),
            sort: None,
            mm_filter_on: false,
            mm_filter: (0, 0),
            mm_last_clicked: false,
            highlighted_mod: None,
            dialog: None,
            kdrag: None,
            wdrag: None,
            wave_view: None,
            wave_vmax: false,
            snap: false,
            units: 0,
            mono_sum: false,
            show_anchor: false,
            zone_clip: None,
            group_clip: None,
            autoloop_attempt: 0,
            in_gesture: false,
            preview_held: None,
            peaks: None,
            default_inst: None,
        }
    }
}

/// Where sampler instruments are kept (Logic's folder under the user's Music folder).
pub fn instruments_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Music/Audio Music Apps/Sampler Instruments")
}

pub fn samples_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Music/Audio Music Apps/Samples")
}

/// The instruments in the folder (subfolders as menu paths), sorted: SFZ, SoundFonts, EXS.
pub fn instrument_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| ["sfz", "sf2", "exs"].contains(&x.to_string_lossy().to_ascii_lowercase().as_str())) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------------------------
// Small controls

/// A value field (drag up / down, Shift fine, Option-click default, double-click to type).
fn field(ui: &mut egui::Ui, id: egui::Id, w: f32, v: &mut f32, min: f32, max: f32, def: f32, step: f32, shown: &dyn Fn(f32) -> String) -> bool {
    let (r, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
    ui.painter().rect_filled(r, 3.0, look::lcd_bg());
    crate::widgets::value_slot(ui, r, id, v, min, max, def, step, shown)
}

fn ifield<T: Copy + Into<f64> + TryFrom<i64>>(ui: &mut egui::Ui, id: egui::Id, w: f32, v: &mut T, min: f32, max: f32, shown: &dyn Fn(f32) -> String) -> bool {
    let mut f = (*v).into() as f32;
    if field(ui, id, w, &mut f, min, max, min, 1.0, shown) {
        if let Ok(x) = T::try_from(f.round() as i64) {
            *v = x;
            return true;
        }
    }
    false
}

fn note_field(ui: &mut egui::Ui, id: egui::Id, v: &mut u8) -> bool {
    ifield(ui, id, 46.0, v, 0.0, 127.0, &|x| sd::note_name(x as u8))
}

fn toggle(ui: &mut egui::Ui, label: &str, on: &mut bool) -> bool {
    let b = egui::Button::new(egui::RichText::new(label).size(11.0)).fill(if *on { look::accent().gamma_multiply(0.7) } else { look::group_bg() }).min_size(vec2(0.0, 18.0));
    if ui.add(b).clicked() {
        *on = !*on;
        return true;
    }
    false
}

fn choice(ui: &mut egui::Ui, id: egui::Id, w: f32, cur: usize, choices: &[&str]) -> Option<usize> {
    let (r, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
    ui.painter().rect_filled(r, 3.0, look::lcd_bg());
    crate::widgets::choice_slot(ui, r, id, cur, choices)
}

fn db(v: f32) -> String {
    format!("{v:.1} dB")
}

fn pan_s(v: f32) -> String {
    format!("{:+.0}", v * 64.0)
}

/// Parameter values of this frame, by id.
struct Vals {
    vals: Vec<f32>,
    index: std::collections::HashMap<u32, usize>,
    params: Vec<lg_dsp::Param>,
    set: Vec<(u32, f32)>,
}

impl Vals {
    fn get(&self, id: u32) -> f32 {
        self.index.get(&id).map(|&i| self.vals[i]).unwrap_or(0.0)
    }
    fn param(&self, id: u32) -> Option<&lg_dsp::Param> {
        self.index.get(&id).map(|&i| &self.params[i])
    }
    fn put(&mut self, id: u32, v: f32) {
        if let Some(&i) = self.index.get(&id) {
            self.vals[i] = v;
            self.set.push((id, v));
        }
    }
    /// A knob for a parameter; true when changed.
    fn knob(&mut self, ui: &mut egui::Ui, r: Rect, id: u32, label: &str) -> bool {
        let Some(p) = self.param(id).cloned() else { return false };
        let mut v = self.get(id);
        let shown = |x: f32| fmt_param(&p, x);
        if crate::widgets::knob(ui, r, ui.scope_id().with(("sk", id)), &mut v, p.min, p.max, p.default, label, &shown) {
            self.put(id, v);
            return true;
        }
        false
    }
    fn field(&mut self, ui: &mut egui::Ui, id: u32, w: f32) -> bool {
        let Some(p) = self.param(id).cloned() else { return false };
        let mut v = self.get(id);
        let step = match p.unit {
            lg_dsp::Unit::Semitones | lg_dsp::Unit::None | lg_dsp::Unit::Cents => 1.0,
            _ => (p.max - p.min) / 200.0,
        };
        let shown = |x: f32| fmt_param(&p, x);
        if field(ui, ui.scope_id().with(("sf", id)), w, &mut v, p.min, p.max, p.default, step, &shown) {
            self.put(id, v);
            return true;
        }
        false
    }
    fn menu(&mut self, ui: &mut egui::Ui, id: u32, w: f32) -> bool {
        let Some(p) = self.param(id).cloned() else { return false };
        let cur = self.get(id).round().max(0.0) as usize;
        if let Some(k) = choice(ui, ui.scope_id().with(("sm", id)), w, cur, p.choices) {
            self.put(id, k as f32);
            return true;
        }
        false
    }
    fn check(&mut self, ui: &mut egui::Ui, id: u32, label: &str) -> bool {
        let mut on = self.get(id) >= 0.5;
        if toggle(ui, label, &mut on) {
            self.put(id, if on { 1.0 } else { 0.0 });
            return true;
        }
        false
    }
}

fn fmt_param(p: &lg_dsp::Param, v: f32) -> String {
    use lg_dsp::Unit::*;
    match p.unit {
        Menu => p.choices.get(v.round().max(0.0) as usize).copied().unwrap_or("").to_string(),
        OnOff => if v >= 0.5 { "On".into() } else { "Off".into() },
        Db => format!("{v:.1} dB"),
        Hz => if v >= 1000.0 { format!("{:.2} kHz", v / 1000.0) } else { format!("{v:.1} Hz") },
        Ms => format!("{v:.0} ms"),
        Seconds => if v < 1.0 { format!("{:.0} ms", v * 1000.0) } else { format!("{v:.2} s") },
        Percent => format!("{v:.0} %"),
        Ratio => format!("{:.0} %", v * 100.0),
        Semitones => format!("{v:+.0}"),
        Cents => format!("{v:+.0} ct"),
        Octaves => format!("{v:.2} oct"),
        Pan => format!("{:+.0}", v * 64.0),
        None => format!("{v:.2}"),
    }
}

// ---------------------------------------------------------------------------------------------
// The window

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some(id) = instwin::track_id(app, t) else { return };
    if !app.inst.docs.contains_key(&id) {
        // a Sampler from a project or a patch: its zones as an editable instrument
        let doc = instwin::with_inst(app, t, |s: &mut lg_synth::sampler::Sampler| SamplerDoc::from_zones(s.zones(), &s.name)).unwrap_or_else(SamplerDoc::new);
        app.inst.docs.insert(id, doc);
        instwin::bump(app, t);
    }
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let index = params.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    let mut v = Vals { vals, index, params, set: Vec::new() };
    let mut doc = app.inst.docs.remove(&id).unwrap_or_else(SamplerDoc::new);
    let orig = doc.clone();
    let mut st = std::mem::take(&mut app.inst.sampler);
    let mut what: Option<String> = None;
    let mut acts: Vec<Act> = Vec::new();
    if !ctx.input(|i| i.pointer.any_down()) {
        st.in_gesture = false;
    }
    let track = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    let used = instwin::with_inst(app, t, |s: &mut lg_synth::sampler::Sampler| s.used_voices()).unwrap_or(0);
    let mut open = true;
    let dropped = std::mem::take(&mut app.inst.dropped);
    let drop_pos = app.inst.drop_pos;
    let dragging = instwin::dragging_files(ctx);
    let resp = egui::Window::new(format!("{track} — Sampler")).id(egui::Id::unique(("sampler-window", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(940.0, 760.0)).show(ctx, |ui| {
        header(ui, &mut doc, &mut st, &mut acts, &mut what);
        nav_bar(ui, &mut doc, &mut st, &mut v, &mut acts, &mut what, dragging, &dropped, drop_pos);
        let order: Vec<usize> = match st.maxed {
            Some(m) => vec![m],
            None => (0..5).filter(|&k| !st.hidden[k]).collect(),
        };
        egui::ScrollArea::vertical().id_salt(("sampler-scroll", t)).auto_shrink([false, false]).show(ui, |ui| {
            for k in order {
                let h = if st.maxed == Some(k) { (ui.available_height() - 30.0).max(DEFAULT_H[k] * 1.6) } else { st.heights[k] };
                let (title, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                let p = ui.painter().clone();
                p.rect_filled(title, 0.0, look::header_bg());
                p.text(title.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, PANES[k], look::semibold(12.0), look::text());
                let tr = ui.interact(title, ui.scope_id().with(("pane-title", k)), Sense::click());
                if tr.double_clicked() {
                    st.heights[k] = if st.heights[k] > DEFAULT_H[k] { DEFAULT_H[k] } else { DEFAULT_H[k] * 1.8 };
                }
                let body = ui.allocate_ui(vec2(ui.available_width(), h), |ui| {
                    ui.set_min_height(h);
                    ui.set_max_height(h);
                    ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                    match k {
                        0 => synth_pane(ui, &mut v, &mut st, &mut what, used),
                        1 => modmatrix_pane(ui, &mut v, &mut st, &mut what),
                        2 => modulators_pane(ui, &mut v, &mut st, &mut what),
                        3 => mapping_pane(ui, &mut doc, &mut st, &mut acts, &mut what, &dropped, drop_pos),
                        _ => zone_pane(ui, &mut doc, &mut st, &mut acts, &mut what, &dropped, drop_pos),
                    }
                });
                let pr = title.union(body.response.rect);
                st.pane_rects[k] = Some(pr);
                if st.scroll_to == Some(k) {
                    ui.scroll_to_rect(pr, Some(egui::Align::TOP));
                    st.scroll_to = None;
                }
                // the boundary drags the pane's height
                let (b, _) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::hover());
                let br = ui.interact(b, ui.scope_id().with(("pane-edge", k)), Sense::drag());
                if br.hovered() || br.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
                if br.dragged() {
                    st.heights[k] = (st.heights[k] + br.drag_delta().y).clamp(80.0, 1400.0);
                }
                ui.painter().hline(b.x_range(), b.center().y, Stroke::new(1.0, look::line()));
            }
        });
        dialogs(ui, &mut doc, &mut st, &mut acts, &mut what);
    });
    app.inst.rect = resp.map(|r| r.response.rect);
    // ---- after drawing: undo, the engine, actions that need the app
    let changed_doc = what.is_some() || !v.set.is_empty();
    if let Some(w) = &what
        && !st.in_gesture
    {
        app.inst.docs.insert(id, orig.clone());
        instwin::checkpoint(app, t, w);
        app.inst.docs.remove(&id);
        if ctx.input(|i| i.pointer.any_down()) {
            st.in_gesture = true;
        }
    } else if what.is_none() && !v.set.is_empty() && !st.in_gesture {
        // a parameter edit: the window's undo keeps the values as they were
        app.inst.docs.insert(id, orig.clone());
        let name = v.set.first().and_then(|(pid, _)| v.param(*pid).map(|p| p.name.to_string())).unwrap_or_default();
        instwin::checkpoint(app, t, &name);
        app.inst.docs.remove(&id);
        if ctx.input(|i| i.pointer.any_down()) {
            st.in_gesture = true;
        }
    }
    app.inst.docs.insert(id, doc);
    app.inst.sampler = st;
    instwin::set_params(app, t, &v.set);
    if changed_doc && what.is_some() {
        instwin::bump(app, t);
    }
    for a in acts {
        run_act(app, ctx, t, a);
    }
    if !open {
        app.plugin_window = None;
        app.inst.rect = None;
    }
}

/// What the window asks for that needs the app (files, the engine, other instruments).
enum Act {
    Preview(u8, bool),
    PreviewZone,
    LoadInto(usize),
    LoadFiles(Option<usize>),
    OpenInstrument(PathBuf),
    SaveCurrent,
    SaveAs { name: String, copy: bool, a_copy: bool },
    SaveAsDefault,
    RecallDefault,
    ImportSynth,
    ImportMapping,
    InitSynth,
    CopySynth,
    PasteSynth,
    ShowInFolder(usize),
    OpenInEditor(usize),
    StepInstrument(i32),
}

fn run_act(app: &mut App, _ctx: &egui::Context, t: usize, a: Act) {
    let Some(id) = instwin::track_id(app, t) else { return };
    match a {
        Act::Preview(k, on) => {
            if let Ok(mut e) = app.engine.lock() {
                e.live(t, k, if on { 100 } else { 0 });
            }
        }
        Act::PreviewZone => {
            let st = &app.inst.sampler;
            let Some(z) = st.focus_zone.or(st.sel_zones.first().copied()) else { return };
            let Some(zz) = app.inst.docs.get(&id).and_then(|d| d.zones.get(z)) else { return };
            let k = zz.root.clamp(zz.key_lo, zz.key_hi);
            let vel = ((zz.vel_lo as u16 + zz.vel_hi as u16) / 2).max(1) as u8;
            let engine = app.engine.clone();
            if let Ok(mut e) = engine.lock() {
                e.live(t, k, vel);
            }
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(700));
                if let Ok(mut e) = engine.lock() {
                    e.live(t, k, 0);
                }
            });
        }
        Act::LoadInto(z) => {
            let Some(p) = kn_ui::files::FileDialog::new().set_title("Load Audio File").add_filter("Audio", &["wav", "aif", "aiff", "caf", "flac", "WAV", "AIF", "AIFF"]).pick_file() else { return };
            instwin::checkpoint(app, t, "Load Audio File");
            if let Some(d) = app.inst.docs.get_mut(&id) {
                if z >= d.zones.len() {
                    let nz = d.new_zone(app.inst.sampler.focus_group);
                    app.inst.sampler.sel_zones = vec![nz];
                    app.inst.sampler.focus_zone = Some(nz);
                    let _ = d.load_into(nz, &p);
                } else if let Err(e) = d.load_into(z, &p) {
                    app.tell(format!("Couldn't load {}: {e}", p.display()));
                }
            }
            instwin::bump(app, t);
        }
        Act::LoadFiles(group) => {
            let Some(ps) = kn_ui::files::FileDialog::new().set_title("Load Audio Files").add_filter("Audio", &["wav", "aif", "aiff", "caf", "flac", "WAV", "AIF", "AIFF"]).pick_files() else { return };
            instwin::checkpoint(app, t, "Load Audio Files");
            if let Some(d) = app.inst.docs.get_mut(&id) {
                let how = match group {
                    Some(g) => Drop::Group(g),
                    None => Drop::Chromatic,
                };
                let zs = d.add_files(&ps, how);
                app.inst.sampler.sel_zones = zs.clone();
                app.inst.sampler.focus_zone = zs.first().copied();
            }
            instwin::bump(app, t);
        }
        Act::OpenInstrument(p) => match SamplerDoc::load(&p) {
            Ok(d) => {
                instwin::checkpoint(app, t, "Load Instrument");
                let missing = d.zones.iter().filter(|z| z.missing).count();
                app.inst.docs.insert(id, d);
                app.inst.sampler.sel_zones.clear();
                app.inst.sampler.sel_groups = vec![0];
                app.inst.sampler.focus_group = Some(0);
                app.inst.sampler.focus_zone = None;
                instwin::bump(app, t);
                if missing > 0 {
                    app.tell(format!("{missing} audio file(s) of the instrument weren't found"));
                }
            }
            Err(e) => app.tell(format!("Couldn't open {}: {e}", p.display())),
        },
        Act::SaveCurrent => {
            let path = app.inst.docs.get(&id).and_then(|d| d.path.clone()).filter(|p| p.extension().is_some_and(|e| e == "sfz"));
            match path {
                Some(p) => {
                    if let Some(d) = app.inst.docs.get_mut(&id) {
                        let msg = match d.save(&p, None) {
                            Ok(()) => format!("Saved “{}”", d.name),
                            Err(e) => format!("Couldn't save: {e}"),
                        };
                        app.tell(msg);
                    }
                }
                None => {
                    let name = app.inst.docs.get(&id).map(|d| d.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Untitled".into());
                    app.inst.sampler.dialog = Some(Dialog::SaveAs { name, copy: false, consolidated: false, a_copy: false });
                }
            }
        }
        Act::SaveAs { name, copy, a_copy } => {
            let path = instruments_dir().join(format!("{}.sfz", name.replace('/', "-")));
            let audio = copy.then(|| samples_dir().join(&name));
            if let Some(d) = app.inst.docs.get_mut(&id) {
                let mut target = if a_copy { d.clone() } else { std::mem::take(d) };
                target.name = name.clone();
                let r = target.save(&path, audio.as_deref());
                if !a_copy {
                    *d = target;
                }
                match r {
                    Ok(()) => app.tell(format!("Saved “{name}” in {}", instruments_dir().display())),
                    Err(e) => app.tell(format!("Couldn't save: {e}")),
                }
            }
            instwin::bump(app, t);
        }
        Act::SaveAsDefault => {
            let path = instruments_dir().join("#default.sfz");
            if let Some(d) = app.inst.docs.get(&id) {
                let mut c = d.clone();
                let path_keep = c.path.clone();
                match c.save(&path, None) {
                    Ok(()) => app.tell("Saved as the default instrument"),
                    Err(e) => app.tell(format!("Couldn't save: {e}")),
                }
                let _ = path_keep;
            }
        }
        Act::RecallDefault => {
            let path = instruments_dir().join("#default.sfz");
            if path.exists() {
                run_act(app, _ctx, t, Act::OpenInstrument(path));
            } else {
                instwin::checkpoint(app, t, "Recall Default");
                app.inst.docs.insert(id, SamplerDoc::new());
                instwin::bump(app, t);
            }
        }
        Act::ImportSynth => {
            // (the synth settings are kept with saved plug-in settings: take them from one)
            let dir = crate::pluginwin::settings_dir("Sampler").unwrap_or_default();
            let Some(p) = kn_ui::files::FileDialog::new().set_title("Import Synth Parameters").set_directory(&dir).add_filter("Setting", &["txt"]).pick_file() else { return };
            if let Ok(text) = std::fs::read_to_string(&p) {
                let pairs: Vec<(u32, f32)> = text.lines().filter_map(|l| {
                    let mut w = l.split_whitespace();
                    Some((w.next()?.parse().ok()?, w.next()?.parse().ok()?))
                }).filter(|(pid, _)| *pid > 0).collect();
                instwin::checkpoint(app, t, "Import Synth Parameters");
                instwin::set_params(app, t, &pairs);
            }
        }
        Act::ImportMapping => {
            let Some(p) = kn_ui::files::FileDialog::new().set_title("Import Mapping").set_directory(instruments_dir()).add_filter("Sampler Instrument", &["sfz", "sf2", "exs"]).pick_file() else { return };
            match SamplerDoc::load(&p) {
                Ok(d) => {
                    instwin::checkpoint(app, t, "Import Mapping");
                    app.inst.docs.insert(id, d);
                    instwin::bump(app, t);
                }
                Err(e) => app.tell(format!("Couldn't open {}: {e}", p.display())),
            }
        }
        Act::InitSynth => {
            instwin::checkpoint(app, t, "Initialize Synth Parameters");
            let defaults: Vec<(u32, f32)> = lg_synth::sampler::all_params().iter().map(|p| (p.id, p.default)).collect();
            instwin::set_params(app, t, &defaults);
        }
        Act::CopySynth => {
            if let Some((p, v)) = instwin::values(app, t) {
                app.inst.synth_clip = Some(p.iter().zip(v).map(|(p, v)| (p.id, v)).collect());
            }
        }
        Act::PasteSynth => {
            if let Some(c) = app.inst.synth_clip.clone() {
                instwin::checkpoint(app, t, "Paste Synth Parameters");
                instwin::set_params(app, t, &c);
            }
        }
        Act::ShowInFolder(z) => {
            if let Some(dir) = app.inst.docs.get(&id).and_then(|d| d.zones.get(z)).and_then(|z| z.file.as_ref()).and_then(|f| f.parent()).map(Path::to_path_buf) {
                let _ = std::process::Command::new("xdg-open").arg(dir).stdin(std::process::Stdio::null()).spawn();
            }
        }
        Act::OpenInEditor(z) => {
            // Settings ▸ Audio ▸ File Editor's External Sample Editor, else the desktop's editor
            if let Some(f) = app.inst.docs.get(&id).and_then(|d| d.zones.get(z)).and_then(|z| z.file.clone()) {
                let ext = app.logic_settings.get("Audio / File Editor / External Sample Editor:");
                let prog = if ext.trim().is_empty() { "xdg-open".to_string() } else { ext.trim().to_string() };
                match std::process::Command::new(&prog).arg(&f).stdin(std::process::Stdio::null()).spawn() {
                    Ok(_) => app.tell(format!("Opened {} in {prog}", f.display())),
                    Err(e) => app.tell(format!("Couldn't open an editor ({prog}): {e}")),
                }
            } else {
                app.tell("The zone has no audio file");
            }
        }
        Act::StepInstrument(by) => {
            let list = instrument_files(&instruments_dir());
            if list.is_empty() {
                return;
            }
            let cur = app.inst.docs.get(&id).and_then(|d| d.path.clone());
            let at = cur.and_then(|c| list.iter().position(|p| *p == c));
            let next = match at {
                Some(i) => (i as i32 + by).rem_euclid(list.len() as i32) as usize,
                None if by > 0 => 0,
                None => list.len() - 1,
            };
            run_act(app, _ctx, t, Act::OpenInstrument(list[next].clone()));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Header: the Settings pop-up (sampler instruments), ‹ ›

fn header(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, acts: &mut Vec<Act>, _what: &mut Option<String>) {
    ui.horizontal(|ui| {
        if ui.small_button("‹").on_hover_text("Previous Instrument").clicked() {
            acts.push(Act::StepInstrument(-1));
        }
        if ui.small_button("›").on_hover_text("Next Instrument").clicked() {
            acts.push(Act::StepInstrument(1));
        }
        let shown = if doc.name.is_empty() { "#default".to_string() } else { doc.name.clone() };
        ui.menu_button(format!("{shown} ⌄"), |ui| {
            ui.set_min_width(260.0);
            if ui.button("Save Instrument").clicked() {
                acts.push(Act::SaveCurrent);
                ui.close();
            }
            if ui.button("Save Instrument As…").clicked() {
                st.dialog = Some(Dialog::SaveAs { name: shown.trim_start_matches('#').to_string(), copy: false, consolidated: false, a_copy: false });
                ui.close();
            }
            if ui.button("Save A Copy As…").clicked() {
                st.dialog = Some(Dialog::SaveAs { name: format!("{} copy", shown.trim_start_matches('#')), copy: false, consolidated: false, a_copy: true });
                ui.close();
            }
            if ui.button("Save As Default").clicked() {
                acts.push(Act::SaveAsDefault);
                ui.close();
            }
            if ui.button("Recall Default").clicked() {
                acts.push(Act::RecallDefault);
                ui.close();
            }
            ui.separator();
            // the Search Filter
            ui.horizontal(|ui| {
                ui.label("Search Filter:");
                let mut term: String = ui.data(|d| d.get_temp(egui::Id::unique("sampler-search"))).unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut term).desired_width(120.0)).changed() {
                    ui.data_mut(|d| d.insert_temp(egui::Id::unique("sampler-search"), term.clone()));
                }
            });
            let term: String = ui.data(|d| d.get_temp(egui::Id::unique("sampler-search"))).unwrap_or_default();
            let mut on: bool = ui.data(|d| d.get_temp(egui::Id::unique("sampler-search-on"))).unwrap_or(true);
            if ui.checkbox(&mut on, "Enable Search Filter").changed() {
                ui.data_mut(|d| d.insert_temp(egui::Id::unique("sampler-search-on"), on));
            }
            ui.separator();
            let dir = instruments_dir();
            let files = instrument_files(&dir);
            let files: Vec<PathBuf> = files.into_iter().filter(|p| !on || term.is_empty() || p.file_stem().is_some_and(|s| s.to_string_lossy().to_lowercase().contains(&term.to_lowercase()))).collect();
            if files.is_empty() {
                ui.label(egui::RichText::new(format!("No instruments in {}", dir.display())).color(look::text_dim()));
            }
            // subfolders as submenus
            let mut folders: std::collections::BTreeMap<String, Vec<PathBuf>> = Default::default();
            for f in files {
                let rel = f.parent().and_then(|p| p.strip_prefix(&dir).ok()).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                folders.entry(rel).or_default().push(f);
            }
            for (folder, list) in folders {
                let mut item = |ui: &mut egui::Ui, f: &PathBuf| {
                    let n = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    let ext = f.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
                    let label = if ext == "SFZ" { n } else { format!("{n} ({ext})") };
                    if ui.add(egui::Button::selectable(doc.path.as_ref() == Some(f), label)).clicked() {
                        acts.push(Act::OpenInstrument(f.clone()));
                        ui.close();
                    }
                };
                if folder.is_empty() {
                    for f in &list {
                        item(ui, f);
                    }
                } else {
                    ui.menu_button(folder, |ui| {
                        for f in &list {
                            item(ui, f);
                        }
                    });
                }
            }
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Navigation bar

#[allow(clippy::too_many_arguments)]
fn nav_bar(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, _v: &mut Vals, acts: &mut Vec<Act>, what: &mut Option<String>, dragging: bool, dropped: &[PathBuf], drop_pos: Option<egui::Pos2>) {
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(bar, 6.0, look::bar_bg());
    if dragging || !dropped.is_empty() {
        // the drop zones: Chromatic (Zone per File / Split at Silence) and Optimized (Zone per Note)
        let w = (bar.width() - 8.0) / 4.0;
        let zones = [("Chromatic: Zone per File", Drop::ZonePerFile), ("Chromatic: Split at Silence", Drop::SplitAtSilence), ("Optimized: Zone per Note", Drop::Optimized), ("Chromatic", Drop::Chromatic)];
        for (k, (label, how)) in zones.iter().enumerate() {
            let r = Rect::from_min_size(pos2(bar.min.x + 4.0 + k as f32 * w, bar.min.y + 3.0), vec2(w - 4.0, bar.height() - 6.0));
            let hot = drop_pos.or(ui.ctx().pointer_hover_pos()).is_some_and(|q| r.contains(q));
            p.rect_filled(r, 5.0, if hot { look::accent().gamma_multiply(0.6) } else { look::group_bg() });
            p.text(r.center(), Align2::CENTER_CENTER, *label, look::regular(11.0), look::text());
            if hot && !dropped.is_empty() {
                let zs = doc.add_files(dropped, *how);
                st.sel_zones = zs.clone();
                st.focus_zone = zs.first().copied();
                *what = Some("Add Zones".into());
            }
        }
        return;
    }
    let mut x = bar.min.x + 6.0;
    for k in 0..5 {
        let w = p.layout_no_wrap(PANES[k].into(), look::regular(12.0), look::text()).size().x + 30.0;
        let r = Rect::from_min_size(pos2(x, bar.min.y + 4.0), vec2(w, bar.height() - 8.0));
        x += w + 4.0;
        let resp = ui.interact(r, ui.scope_id().with(("nav", k)), Sense::click());
        let shown = !st.hidden[k];
        p.rect_filled(r, 5.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
        // the LED
        let led = Rect::from_center_size(pos2(r.min.x + 10.0, r.center().y), vec2(8.0, 8.0));
        p.circle_filled(led.center(), 3.5, if shown { Color32::from_rgb(0xf5, 0xc5, 0x18) } else { Color32::from_gray(0x55) });
        p.text(pos2(r.min.x + 20.0, r.center().y), Align2::LEFT_CENTER, PANES[k], look::regular(12.0), look::text());
        if resp.clicked() {
            let pos = resp.interact_pointer_pos().unwrap_or_default();
            if led.expand(3.0).contains(pos) {
                st.hidden[k] = !st.hidden[k];
                if st.maxed == Some(k) {
                    st.maxed = None;
                }
            } else if ui.input(|i| i.modifiers.alt) {
                // Option-click: only this pane, expanded
                st.hidden = [true; 5];
                st.hidden[k] = false;
                st.maxed = Some(k);
            } else {
                st.hidden[k] = false;
                st.scroll_to = Some(k);
            }
        }
        if resp.double_clicked() {
            st.maxed = if st.maxed == Some(k) { None } else { Some(k) };
            if st.maxed.is_none() && st.hidden.iter().all(|h| *h) {
                st.hidden = [false; 5];
            }
        }
    }
    // the Action menu at the right
    let ar = Rect::from_min_size(pos2(bar.max.x - 34.0, bar.min.y + 4.0), vec2(28.0, bar.height() - 8.0));
    let aresp = ui.interact(ar, ui.scope_id().with("nav-action"), Sense::click());
    p.rect_filled(ar, 5.0, if aresp.hovered() { look::button_on() } else { look::group_bg() });
    crate::glyphs::paint(&p, ar, crate::glyphs::ph::DOTS_THREE_CIRCLE, 16.0, look::text());
    egui::Popup::menu(&aresp).show(|ui| {
        ui.set_min_width(300.0);
        for (label, a) in [
            ("Initialize Synth Parameters", 0),
            ("Copy Synth Parameters without Mapping", 1),
            ("Paste Synth Parameters without Mapping", 2),
            ("Import Synth Parameters without Mapping…", 3),
            ("New Mapping", 4),
            ("Import Mapping without Synth Parameters…", 5),
        ] {
            if ui.button(label).clicked() {
                match a {
                    0 => acts.push(Act::InitSynth),
                    1 => acts.push(Act::CopySynth),
                    2 => acts.push(Act::PasteSynth),
                    3 => acts.push(Act::ImportSynth),
                    4 => {
                        let keep = (doc.name.clone(), doc.path.clone());
                        *doc = SamplerDoc::new();
                        (doc.name, doc.path) = keep;
                        st.sel_zones.clear();
                        st.sel_groups = vec![0];
                        *what = Some("New Mapping".into());
                    }
                    _ => acts.push(Act::ImportMapping),
                }
                ui.close();
            }
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Synth pane

fn synth_pane(ui: &mut egui::Ui, v: &mut Vals, st: &mut Ui, _what: &mut Option<String>, used: usize) {
    let area = ui.max_rect();
    let p = ui.painter().clone();
    p.rect_filled(area, 0.0, look::pane_bg());
    let kw = 64.0;
    let kh = 78.0;
    let row = area.min.y + 10.0;
    let mut x = area.min.x + 10.0;
    let kn = |ui: &mut egui::Ui, v: &mut Vals, x: &mut f32, y: f32, id: u32, label: &str, tg: Option<usize>| {
        let r = Rect::from_min_size(pos2(*x, y), vec2(kw, kh));
        *x += kw + 6.0;
        v.knob(ui, r, id, label);
        // right-click: Add Modulation ▸ source, Show Modulation
        if let Some(tg) = tg {
            let resp = ui.interact(r, ui.scope_id().with(("knob-menu", id)), Sense::click());
            resp.context_menu(|ui| {
                ui.menu_button("Add Modulation", |ui| {
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for (si, s) in ss::sources().iter().enumerate().skip(1) {
                            if ui.button(*s).clicked() {
                                ui.data_mut(|d| d.insert_temp(egui::Id::unique("sampler-add-mod"), (tg, si)));
                                ui.close();
                            }
                        }
                    });
                });
                if ui.button("Show Modulation").clicked() {
                    ui.data_mut(|d| d.insert_temp(egui::Id::unique("sampler-show-mod"), tg));
                    ui.close();
                }
            });
        }
    };
    kn(ui, v, &mut x, row, 10, "Tune", Some(target::PITCH));
    kn(ui, v, &mut x, row, 11, "Fine", Some(target::PITCH));
    x += 10.0;
    // the two filters
    for (k, (on, ty, cut, res, drv)) in [(5u32, ss::F1_TYPE, 6u32, 7u32, ss::F1_DRIVE), (ss::F2_ON, ss::F2_TYPE, ss::F2_CUTOFF, ss::F2_RES, ss::F2_DRIVE)].into_iter().enumerate() {
        let fx = x;
        let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(fx, row - 4.0), vec2(220.0, 24.0))));
        cell.horizontal(|ui| {
            v.check(ui, on, &format!("Filter {}", k + 1));
            v.menu(ui, ty, 140.0);
        });
        let mut xx = fx;
        let tg = if k == 0 { [target::F1_CUT, target::F1_RES, target::F1_DRIVE] } else { [target::F2_CUT, target::F2_RES, target::F2_DRIVE] };
        kn(ui, v, &mut xx, row + 22.0, cut, "Cutoff", Some(tg[0]));
        kn(ui, v, &mut xx, row + 22.0, res, "Resonance", Some(tg[1]));
        kn(ui, v, &mut xx, row + 22.0, drv, "Drive", Some(tg[2]));
        if v.get(on) < 0.5 {
            p.rect_filled(Rect::from_min_size(pos2(fx, row + 22.0), vec2(xx - fx, kh)), 4.0, look::pane_bg().gamma_multiply(0.6));
        }
        x = xx + 12.0;
    }
    // Series / Parallel and Blend
    let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(x, row), vec2(80.0, 50.0))));
    cell.vertical(|ui| {
        let par = v.get(ss::F_ROUTING) >= 0.5;
        if ui.add(egui::Button::selectable(!par, "Series")).clicked() {
            v.put(ss::F_ROUTING, 0.0);
        }
        if ui.add(egui::Button::selectable(par, "Parallel")).clicked() {
            v.put(ss::F_ROUTING, 1.0);
        }
    });
    let mut xb = x;
    kn(ui, v, &mut xb, row + 52.0, ss::F_BLEND, "Blend", Some(target::BLEND));
    x = xb + 12.0;
    kn(ui, v, &mut x, row, 0, "Volume", Some(target::VOLUME));
    kn(ui, v, &mut x, row, ss::PAN, "Pan", Some(target::PAN));
    // Details
    let dr = Rect::from_min_size(pos2(area.max.x - 80.0, area.min.y + 6.0), vec2(70.0, 20.0));
    let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(dr));
    if cell.add(egui::Button::selectable(st.details, "Details")).clicked() {
        st.details = !st.details;
    }
    if st.details {
        let r = Rect::from_min_max(pos2(area.max.x - 330.0, area.min.y + 30.0), pos2(area.max.x - 6.0, area.max.y - 4.0));
        p.rect_filled(r, 6.0, look::group_bg());
        let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink(6.0)));
        egui::ScrollArea::vertical().id_salt("sampler-details").show(&mut cell, |ui| {
            egui::Grid::new("sampler-details-grid").num_columns(4).spacing(vec2(8.0, 3.0)).show(ui, |ui| {
                let rows: &[(&str, u32)] = &[
                    ("Glide", ss::GLIDE),
                    ("Pitch Bend Up", 12),
                    ("Pitch Bend Down", ss::BEND_DOWN),
                    ("Coarse Tune Remote", ss::COARSE_REMOTE),
                    ("Transpose", ss::TRANSPOSE_NOTES),
                    ("Sample Select Random", ss::SEL_RANDOM),
                    ("Velocity Random", ss::VEL_RANDOM),
                    ("Amp Velocity Curve", ss::VEL_CURVE),
                    ("Velocity Offset", ss::VEL_OFFSET),
                    ("Amp Key Scale", ss::KEY_SCALE),
                    ("Polyphony", 13),
                    ("Random Detune", ss::RANDOM_DETUNE),
                    ("Mono Mode Bend Range", ss::MONO_BEND),
                ];
                for (k, (label, pid)) in rows.iter().enumerate() {
                    ui.label(egui::RichText::new(*label).size(11.0).color(look::text_dim()));
                    if *pid == ss::BEND_DOWN && v.get(*pid) < 0.0 {
                        // Linked: the same range as Up
                        let mut cell = ui.horizontal(|ui| {
                            let mut linked = true;
                            if toggle(ui, "Linked", &mut linked) {
                                let up = v.get(12);
                                v.put(*pid, up);
                            }
                        });
                        let _ = &mut cell;
                    } else if *pid == ss::BEND_DOWN {
                        ui.horizontal(|ui| {
                            v.field(ui, *pid, 60.0);
                            if ui.small_button("Linked").clicked() {
                                v.put(*pid, -1.0);
                            }
                        });
                    } else if *pid == ss::COARSE_REMOTE {
                        let cur = v.get(*pid);
                        let mut n = cur;
                        let shown = |x: f32| if x < 0.0 { "Off".to_string() } else { sd::note_name(x as u8) };
                        if field(ui, ui.scope_id().with("ctr"), 70.0, &mut n, -1.0, 127.0, -1.0, 1.0, &shown) {
                            v.put(*pid, n);
                        }
                    } else {
                        v.field(ui, *pid, 70.0);
                    }
                    if k % 2 == 1 {
                        ui.end_row();
                    }
                }
                ui.end_row();
                ui.label(egui::RichText::new("Mode").size(11.0).color(look::text_dim()));
                v.menu(ui, ss::MODE, 80.0);
                ui.label(egui::RichText::new("Unison").size(11.0).color(look::text_dim()));
                v.menu(ui, ss::UNISON, 60.0);
                ui.end_row();
                ui.label(egui::RichText::new("Ignore Release Velocity").size(11.0).color(look::text_dim()));
                v.check(ui, ss::IGNORE_REL_VEL, "On");
                ui.label(egui::RichText::new("Used Voices").size(11.0).color(look::text_dim()));
                ui.label(format!("{used}"));
                ui.end_row();
                ui.label(egui::RichText::new("MIDI Mono Mode").size(11.0).color(look::text_dim()));
                v.menu(ui, ss::MONO_MODE, 140.0);
                ui.end_row();
            });
        });
    }
    // knob menus asked for modulation
    if let Some((tg, src)) = ui.data_mut(|d| d.remove_temp::<(usize, usize)>(egui::Id::unique("sampler-add-mod"))) {
        ui.data_mut(|d| d.insert_temp(egui::Id::unique("sampler-add-mod-act"), (tg, src)));
    }
    if let Some(tg) = ui.data_mut(|d| d.remove_temp::<usize>(egui::Id::unique("sampler-show-mod"))) {
        st.mm_filter_on = true;
        st.mm_filter = (1, tg);
        st.hidden[1] = false;
        st.scroll_to = Some(1);
    }
}

// ---------------------------------------------------------------------------------------------
// Mod Matrix pane

fn modmatrix_pane(ui: &mut egui::Ui, v: &mut Vals, st: &mut Ui, _what: &mut Option<String>) {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0.0, look::pane_bg());
    // a routing asked for from a Synth knob's menu
    if let Some((tg, src)) = ui.data_mut(|d| d.remove_temp::<(usize, usize)>(egui::Id::unique("sampler-add-mod-act"))) {
        if let Some(r) = (0..ss::ROUTINGS).find(|&r| v.get(ss::mod_base(r) + 5) < 0.5 && v.get(ss::mod_base(r) + 1) < 0.5) {
            let b = ss::mod_base(r);
            v.put(b, 1.0);
            v.put(b + 1, src as f32);
            v.put(b + 5, tg as f32);
            v.put(b + 6, 0.5);
        }
    }
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        toggle(ui, "Filter", &mut st.mm_filter_on);
        let kinds = ["Filter by Source/Via", "Filter by Target"];
        ui.menu_button(kinds[st.mm_filter.0 as usize % 2], |ui| {
            for (k, n) in kinds.iter().enumerate() {
                if ui.add(egui::Button::selectable(st.mm_filter.0 as usize == k, *n)).clicked() {
                    st.mm_filter.0 = k as u8;
                    ui.close();
                }
            }
            ui.separator();
            ui.checkbox(&mut st.mm_last_clicked, "Always select last clicked");
            ui.separator();
            let list: Vec<&str> = if st.mm_filter.0 == 1 { ss::TARGETS.to_vec() } else { ss::sources().to_vec() };
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for (i, n) in list.iter().enumerate().skip(1) {
                    if ui.add(egui::Button::selectable(st.mm_filter.1 == i, *n)).clicked() {
                        st.mm_filter.1 = i;
                        st.mm_filter_on = true;
                        ui.close();
                    }
                }
            });
        });
        if st.mm_filter_on {
            let name = if st.mm_filter.0 == 1 { ss::TARGETS.get(st.mm_filter.1).copied() } else { ss::sources().get(st.mm_filter.1).copied() };
            ui.label(egui::RichText::new(name.unwrap_or("")).color(look::accent()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            if ui.button("+").on_hover_text("Add a routing").clicked()
                && let Some(r) = (0..ss::ROUTINGS).find(|&r| v.get(ss::mod_base(r)) < 0.5 && v.get(ss::mod_base(r) + 5) < 0.5 && v.get(ss::mod_base(r) + 1) < 0.5)
            {
                let b = ss::mod_base(r);
                v.put(b, 1.0);
                // the view filter fills in the new routing's source or target
                if st.mm_filter_on {
                    if st.mm_filter.0 == 1 {
                        v.put(b + 5, st.mm_filter.1 as f32);
                    } else {
                        v.put(b + 1, st.mm_filter.1 as f32);
                    }
                }
                st.highlighted_mod = Some((false, r));
            }
            if ui.button("−").on_hover_text("Delete the selected routing").clicked()
                && let Some((false, r)) = st.highlighted_mod
            {
                let b = ss::mod_base(r);
                for (k, d) in [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0].iter().enumerate() {
                    v.put(b + k as u32, *d);
                }
                st.highlighted_mod = None;
            }
            // sorting by a column
            for (label, off) in [("Target ⇅", 5u32), ("Via ⇅", 3), ("Source ⇅", 1)] {
                if ui.small_button(label).on_hover_text("Sort the routings").clicked() {
                    sort_routes(v, off);
                }
            }
        });
    });
    egui::ScrollArea::vertical().id_salt("sampler-mm").auto_shrink([false, false]).show(ui, |ui| {
        for r in 0..ss::ROUTINGS {
            let b = ss::mod_base(r);
            let used = v.get(b) >= 0.5 || v.get(b + 1) >= 0.5 || v.get(b + 5) >= 0.5;
            if !used {
                continue;
            }
            if st.mm_filter_on {
                let ok = if st.mm_filter.0 == 1 { v.get(b + 5).round() as usize == st.mm_filter.1 } else { v.get(b + 1).round() as usize == st.mm_filter.1 || v.get(b + 3).round() as usize == st.mm_filter.1 };
                if !ok {
                    continue;
                }
            }
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                let sel = st.highlighted_mod == Some((false, r));
                if ui.add(egui::Button::selectable(sel, format!("{}", r + 1))).clicked() {
                    st.highlighted_mod = Some((false, r));
                }
                v.check(ui, b, "On");
                ui.label(egui::RichText::new("Target").size(11.0).color(look::text_dim()));
                v.menu(ui, b + 5, 130.0);
                ui.label(egui::RichText::new("Source").size(11.0).color(look::text_dim()));
                v.menu(ui, b + 1, 110.0);
                v.check(ui, b + 2, "Inv");
                ui.label(egui::RichText::new("Via").size(11.0).color(look::text_dim()));
                v.menu(ui, b + 3, 110.0);
                v.check(ui, b + 4, "Inv");
                // Amount: one handle, or Min / Max with a via
                let via = v.get(b + 3) >= 0.5;
                amount_slider(ui, v, b, via);
            });
        }
    });
}

fn sort_routes(v: &mut Vals, off: u32) {
    let mut rows: Vec<[f32; 8]> = (0..ss::ROUTINGS).map(|r| {
        let b = ss::mod_base(r);
        std::array::from_fn(|k| v.get(b + k as u32))
    }).collect();
    let name = |row: &[f32; 8]| -> String {
        let i = row[off as usize].round() as usize;
        if off == 5 { ss::TARGETS.get(i).copied().unwrap_or("").to_string() } else { ss::sources().get(i).copied().unwrap_or("").to_string() }
    };
    let used = |row: &[f32; 8]| row[0] >= 0.5 || row[1] >= 0.5 || row[5] >= 0.5;
    let asc = rows.windows(2).filter(|w| used(&w[0]) && used(&w[1])).all(|w| name(&w[0]) <= name(&w[1]));
    rows.sort_by(|a, b| match (used(a), used(b)) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => if asc { name(b).cmp(&name(a)) } else { name(a).cmp(&name(b)) },
    });
    for (r, row) in rows.iter().enumerate() {
        let b = ss::mod_base(r);
        for (k, x) in row.iter().enumerate() {
            if (v.get(b + k as u32) - x).abs() > 1e-9 {
                v.put(b + k as u32, *x);
            }
        }
    }
}

fn amount_slider(ui: &mut egui::Ui, v: &mut Vals, b: u32, via: bool) {
    let (r, _) = ui.allocate_exact_size(vec2(170.0, 18.0), Sense::hover());
    let p = ui.painter();
    let track = Rect::from_center_size(r.center(), vec2(r.width() - 12.0, 4.0));
    p.rect_filled(track, 2.0, look::lcd_bg());
    let xof = |a: f32| track.min.x + (a + 1.0) * 0.5 * track.width();
    let (a0, a1) = (v.get(b + 6), v.get(b + 7));
    p.vline(xof(0.0), track.y_range().expand(3.0), Stroke::new(1.0, look::text_dim()));
    if via {
        let (l, h) = (xof(a0.min(a1)), xof(a0.max(a1)));
        p.rect_filled(Rect::from_min_max(pos2(l, track.min.y), pos2(h, track.max.y)), 2.0, look::accent().gamma_multiply(0.5));
        p.circle_filled(pos2(xof(a0), track.center().y), 5.0, Color32::from_gray(0xd0));
        p.circle_filled(pos2(xof(a1), track.center().y), 5.0, look::accent());
    } else {
        let (l, h) = (xof(0.0f32.min(a0)), xof(0.0f32.max(a0)));
        p.rect_filled(Rect::from_min_max(pos2(l, track.min.y), pos2(h, track.max.y)), 2.0, look::accent());
        p.circle_filled(pos2(xof(a0), track.center().y), 5.0, Color32::from_gray(0xe0));
    }
    let resp = ui.interact(r, ui.scope_id().with(("amt", b)), Sense::click_and_drag()).on_hover_text(if via { format!("Min {:.0} %  Max {:.0} %", a0 * 100.0, a1 * 100.0) } else { format!("Amount {:.0} %", a0 * 100.0) });
    let at = |pp: egui::Pos2| (((pp.x - track.min.x) / track.width()) * 2.0 - 1.0).clamp(-1.0, 1.0);
    if resp.double_clicked() {
        // zero: the amount, or the handle on that side with a via
        if !via {
            v.put(b + 6, 0.0);
        } else if let Some(pp) = resp.interact_pointer_pos() {
            if pp.x < xof(a0.min(a1)) {
                v.put(b + 6, 0.0);
            } else {
                v.put(b + 7, 0.0);
            }
        }
        return;
    }
    if resp.dragged()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let x = at(pp);
        if !via {
            v.put(b + 6, x);
        } else {
            let start = resp.interact_pointer_pos().map(|q| q.x - resp.drag_delta().x).unwrap_or(pp.x);
            let (lo, hi) = (xof(a0.min(a1)), xof(a0.max(a1)));
            if start > lo + 4.0 && start < hi - 4.0 {
                // between the handles: both move
                let d = resp.drag_delta().x / track.width() * 2.0;
                v.put(b + 6, (a0 + d).clamp(-1.0, 1.0));
                v.put(b + 7, (a1 + d).clamp(-1.0, 1.0));
            } else if (pp.x - xof(a0)).abs() <= (pp.x - xof(a1)).abs() && !(a0 == a1 && pp.x > xof(a0)) {
                v.put(b + 6, x);
            } else {
                v.put(b + 7, x);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Modulators pane

fn modulators_pane(ui: &mut egui::Ui, v: &mut Vals, st: &mut Ui, _what: &mut Option<String>) {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0.0, look::pane_bg());
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        if ui.button("+ ENV").clicked()
            && let Some(k) = (2..=ss::ENVS).find(|&k| v.get(ss::env_base(k)) < 0.5)
        {
            v.put(ss::env_base(k), 1.0);
            st.highlighted_mod = Some((true, k));
        }
        if ui.button("+ LFO").clicked()
            && let Some(k) = (1..=ss::LFOS).find(|&k| v.get(ss::lfo_base(k)) < 0.5)
        {
            v.put(ss::lfo_base(k), 1.0);
            st.highlighted_mod = Some((true, 100 + k));
        }
        // − removes the highlighted modulator (Envelope 1 stays)
        if ui.button("−").on_hover_text("Remove the highlighted modulator").clicked() {
            match st.highlighted_mod {
                Some((true, k)) if (2..=ss::ENVS).contains(&k) => v.put(ss::env_base(k), 0.0),
                Some((true, k)) if k > 100 => v.put(ss::lfo_base(k - 100), 0.0),
                _ => {}
            }
            st.highlighted_mod = None;
        }
    });
    egui::ScrollArea::horizontal().id_salt("sampler-mods").auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            for k in 1..=ss::ENVS {
                if k > 1 && v.get(ss::env_base(k)) < 0.5 {
                    continue;
                }
                envelope(ui, v, st, k);
            }
            for k in 1..=ss::LFOS {
                if v.get(ss::lfo_base(k)) < 0.5 {
                    continue;
                }
                lfo(ui, v, st, k);
            }
        });
    });
}

fn envelope(ui: &mut egui::Ui, v: &mut Vals, st: &mut Ui, k: usize) {
    // the ids: Envelope 1's start at ENV1 (type), others at env_base + 1
    let b = if k == 1 { ss::ENV1 } else { ss::env_base(k) + 1 };
    let sel = st.highlighted_mod == Some((true, k));
    egui::Frame::new().fill(if sel { look::group_bg() } else { look::lcd_bg() }).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
        ui.set_width(250.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::selectable(sel, egui::RichText::new(if k == 1 { "ENV 1 (Amp)".to_string() } else { format!("ENV {k}") }).strong())).clicked() {
                st.highlighted_mod = Some((true, k));
            }
            v.menu(ui, b, 70.0);
        });
        let ar = v.get(b).round() as i32 == 1;
        // the display: attack, decay / sustain handle, release
        let (r, _) = ui.allocate_exact_size(vec2(238.0, 70.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(r, 4.0, look::window_bg());
        let (a, h, d, s, rl) = (v.get(b + 2), v.get(b + 4), v.get(b + 5), v.get(b + 6), v.get(b + 7));
        let total = (a + if ar { 0.0 } else { h + d } + rl + 0.5).max(0.5);
        let sx = r.width() / total;
        let y = |lv: f32| r.max.y - 4.0 - lv * (r.height() - 8.0);
        let x0 = r.min.x;
        let xa = x0 + a * sx;
        let xh = xa + if ar { 0.0 } else { h * sx };
        let xd = xh + if ar { 0.0 } else { d * sx };
        let sus = if ar { 1.0 } else { s };
        let xs = xd + 0.5 * sx;
        let xr = xs + rl * sx;
        let pts = vec![pos2(x0, y(0.0)), pos2(xa, y(1.0)), pos2(xh, y(1.0)), pos2(xd, y(sus)), pos2(xs, y(sus)), pos2(xr, y(0.0))];
        p.add(egui::Shape::line(pts, Stroke::new(1.6, look::accent())));
        let handles = [(xa, y(1.0), 0u8), (xd, y(sus), 1), (xr, y(0.0), 2)];
        for (hx, hy, _) in handles {
            p.circle_filled(pos2(hx, hy), 4.0, Color32::from_gray(0xe0));
        }
        let resp = ui.interact(r, ui.scope_id().with(("env-disp", k)), Sense::drag());
        if resp.dragged()
            && let Some(pp) = resp.interact_pointer_pos()
        {
            let start = pp - resp.drag_delta();
            let near = handles.iter().min_by(|p1, p2| (pos2(p1.0, p1.1) - start).length().partial_cmp(&(pos2(p2.0, p2.1) - start).length()).unwrap_or(std::cmp::Ordering::Equal)).map(|h| h.2).unwrap_or(0);
            let dt = resp.drag_delta().x / sx;
            match near {
                0 => v.put(b + 2, (a + dt).clamp(0.0, 10.0)),
                1 => {
                    v.put(b + 5, (d + dt).clamp(0.0, 20.0));
                    if !ar {
                        v.put(b + 6, (s - resp.drag_delta().y / (r.height() - 8.0)).clamp(0.0, 1.0));
                    }
                }
                _ => v.put(b + 7, (rl + dt).clamp(0.0, 20.0)),
            }
        }
        egui::Grid::new(("env-grid", k)).num_columns(4).spacing(vec2(4.0, 2.0)).show(ui, |ui| {
            let rows: &[(&str, u32)] = if ar { &[("Delay", 1), ("Attack", 2), ("Curve", 3), ("Release", 7), ("Vel", 8)] } else { &[("Delay", 1), ("Attack", 2), ("Curve", 3), ("Hold", 4), ("Decay", 5), ("Sustain", 6), ("Release", 7), ("Vel", 8)] };
            for (i, (label, off)) in rows.iter().enumerate() {
                ui.label(egui::RichText::new(*label).size(11.0).color(look::text_dim()));
                v.field(ui, b + off, 64.0);
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
    });
}

fn lfo(ui: &mut egui::Ui, v: &mut Vals, st: &mut Ui, k: usize) {
    let b = ss::lfo_base(k);
    let sel = st.highlighted_mod == Some((true, 100 + k));
    egui::Frame::new().fill(if sel { look::group_bg() } else { look::lcd_bg() }).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
        ui.set_width(230.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::selectable(sel, egui::RichText::new(format!("LFO {k}")).strong())).clicked() {
                st.highlighted_mod = Some((true, 100 + k));
            }
            v.menu(ui, b + 1, 110.0);
        });
        // the waveform display: dragging sideways sets the rate
        let (r, _) = ui.allocate_exact_size(vec2(218.0, 46.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(r, 4.0, look::window_bg());
        let set = ss::LfoSet { on: true, wave: v.get(b + 1).round() as u8, sync: false, hz: 1.0, beats: 1.0, fade: 0.0, fade_out: false, phase: v.get(b + 7) / 360.0, poly: true, bipolar: true, key_trigger: true };
        let mut l = ss::Lfo::default();
        l.restart(&set, 12345);
        let cycles = 2.0;
        let pts: Vec<egui::Pos2> = (0..=120).map(|i| {
            let yv = l.advance(&set, cycles, 1.0 / 120.0);
            pos2(r.min.x + r.width() * i as f32 / 120.0, r.center().y - yv * (r.height() * 0.42))
        }).collect();
        p.add(egui::Shape::line(pts, Stroke::new(1.5, look::accent())));
        let resp = ui.interact(r, ui.scope_id().with(("lfo-disp", k)), Sense::drag()).on_hover_text("Drag sideways: Rate");
        if resp.dragged() {
            let d = resp.drag_delta().x;
            if v.get(b + 2) >= 0.5 {
                let cur = v.get(b + 4) - d / 12.0;
                v.put(b + 4, cur.clamp(0.0, (ss::SYNC_RATES.len() - 1) as f32));
            } else {
                let cur = v.get(b + 3) * 2f32.powf(d / 60.0);
                v.put(b + 3, cur.clamp(0.01, 40.0));
            }
        }
        ui.horizontal(|ui| {
            v.check(ui, b + 2, "♪ Sync");
            if v.get(b + 2) >= 0.5 { v.menu(ui, b + 4, 70.0) } else { v.field(ui, b + 3, 70.0) };
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Fade").size(11.0).color(look::text_dim()));
            v.field(ui, b + 5, 60.0);
            let out = v.get(b + 6) >= 0.5;
            if ui.add(egui::Button::selectable(!out, "In")).clicked() {
                v.put(b + 6, 0.0);
            }
            if ui.add(egui::Button::selectable(out, "Out")).clicked() {
                v.put(b + 6, 1.0);
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Phase").size(11.0).color(look::text_dim()));
            v.field(ui, b + 7, 50.0);
        });
        ui.horizontal(|ui| {
            let poly = v.get(b + 8) >= 0.5;
            if ui.add(egui::Button::selectable(poly, if poly { "Poly" } else { "Mono" })).on_hover_text("Mono: one LFO for all voices; Poly: one per voice").clicked() {
                v.put(b + 8, if poly { 0.0 } else { 1.0 });
            }
            let bi = v.get(b + 9) >= 0.5;
            if ui.add(egui::Button::selectable(bi, if bi { "Bipolar" } else { "Unipolar" })).clicked() {
                v.put(b + 9, if bi { 0.0 } else { 1.0 });
            }
            v.check(ui, b + 10, "Key Trigger");
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Mapping pane

fn zone_order(doc: &SamplerDoc, st: &Ui) -> Vec<usize> {
    let groups: Vec<usize> = if st.sel_groups.is_empty() { (0..doc.groups.len()).collect() } else { st.sel_groups.clone() };
    (0..doc.zones.len()).filter(|&z| st.all_groups_zones || groups.contains(&doc.zones[z].group)).collect()
}

#[allow(clippy::too_many_arguments)]
fn mapping_pane(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, acts: &mut Vec<Act>, what: &mut Option<String>, dropped: &[PathBuf], drop_pos: Option<egui::Pos2>) {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0.0, look::pane_bg());
    // the menu bar: Edit, Group, Zone, View, Preview, zoom, the view buttons
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.menu_button("Edit", |ui| edit_menu(ui, doc, st, what));
        ui.menu_button("Group", |ui| group_menu(ui, doc, st, what));
        ui.menu_button("Zone", |ui| zone_menu(ui, doc, st, acts, what));
        ui.menu_button("View", |ui| view_menu(ui, st));
        if ui.button("▶ Preview").on_hover_text("Plays the selected zone").clicked() {
            acts.push(Act::PreviewZone);
        }
        ui.add(egui::Slider::new(&mut st.key_w, 4.0..=30.0).show_value(false).text("Zoom"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            for (v, label) in [(MapView::Zone, "Zone"), (MapView::Group, "Group"), (MapView::Keyboard, "Key Mapping")] {
                if ui.add(egui::Button::selectable(st.view == v, label)).clicked() {
                    st.view = v;
                }
            }
        });
    });
    match st.view {
        MapView::Keyboard => key_mapping(ui, doc, st, acts, what, dropped, drop_pos),
        MapView::Group => group_view(ui, doc, st, acts, what, dropped),
        MapView::Zone => zone_view(ui, doc, st, acts, what, dropped),
    }
}

fn edit_menu(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, what: &mut Option<String>) {
    ui.set_min_width(240.0);
    if ui.button("Cut").clicked() {
        st.zone_clip = Some(st.sel_zones.iter().filter_map(|&z| doc.zones.get(z).cloned()).collect());
        doc.delete_zones(&st.sel_zones.clone());
        st.sel_zones.clear();
        *what = Some("Cut".into());
        ui.close();
    }
    if ui.button("Copy").clicked() {
        if !st.sel_zones.is_empty() {
            st.zone_clip = Some(st.sel_zones.iter().filter_map(|&z| doc.zones.get(z).cloned()).collect());
        } else {
            // groups (with their zones: another Sampler gets them whole)
            st.group_clip = Some(st.sel_groups.iter().filter_map(|&g| doc.groups.get(g).cloned().map(|gr| (gr, doc.zones.iter().filter(|z| z.group == g).cloned().collect()))).collect());
        }
        ui.close();
    }
    if ui.button("Paste").clicked() {
        let g = st.focus_group.unwrap_or(0).min(doc.groups.len().saturating_sub(1));
        if let Some(zs) = st.zone_clip.clone() {
            let start = doc.zones.len();
            for mut z in zs {
                z.group = g;
                doc.zones.push(z);
            }
            st.sel_zones = (start..doc.zones.len()).collect();
            doc.resolve_overlaps(&st.sel_zones.clone());
        } else if let Some(gs) = st.group_clip.clone() {
            for (gr, zs) in gs {
                doc.groups.push(gr);
                let gi = doc.groups.len() - 1;
                for mut z in zs {
                    z.group = gi;
                    doc.zones.push(z);
                }
            }
        }
        *what = Some("Paste".into());
        ui.close();
    }
    if ui.button("Delete").clicked() {
        delete_selection(doc, st);
        *what = Some("Delete".into());
        ui.close();
    }
    ui.separator();
    if ui.button("Select All").clicked() {
        st.sel_zones = (0..doc.zones.len()).collect();
        st.sel_groups = (0..doc.groups.len()).collect();
        ui.close();
    }
    if ui.button("Deselect All").clicked() {
        st.sel_zones.clear();
        st.sel_groups.clear();
        ui.close();
    }
    if ui.add_enabled(st.view != MapView::Group, egui::Button::new("Select Unused")).clicked() {
        let (z, g) = doc.unused();
        st.sel_zones = z;
        st.sel_groups = g;
        ui.close();
    }
    if ui.button("Invert Selection").clicked() {
        st.sel_zones = (0..doc.zones.len()).filter(|z| !st.sel_zones.contains(z)).collect();
        st.sel_groups = (0..doc.groups.len()).filter(|g| !st.sel_groups.contains(g)).collect();
        ui.close();
    }
    ui.menu_button("Select from Last Played Keys", |ui| {
        for (k, n) in ["None", "Groups", "Groups and Zones"].iter().enumerate() {
            if ui.add(egui::Button::selectable(st.select_from_played == k as u8, *n)).clicked() {
                st.select_from_played = k as u8;
                ui.close();
            }
        }
    });
    ui.separator();
    if ui.button("Edit Key Labels…").clicked() {
        st.dialog = Some(Dialog::KeyLabels);
        ui.close();
    }
    if ui.button("Edit Output Labels…").clicked() {
        st.dialog = Some(Dialog::OutputLabels);
        ui.close();
    }
}

fn delete_selection(doc: &mut SamplerDoc, st: &mut Ui) {
    if !st.sel_zones.is_empty() {
        doc.delete_zones(&st.sel_zones.clone());
        st.sel_zones.clear();
        st.focus_zone = None;
    } else if !st.sel_groups.is_empty() {
        doc.delete_groups(&st.sel_groups.clone());
        st.sel_groups = vec![0];
        st.focus_group = Some(0);
    }
}

fn group_menu(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, what: &mut Option<String>) {
    ui.set_min_width(200.0);
    if ui.button("New").clicked() {
        let g = doc.new_group();
        st.sel_groups = vec![g];
        st.focus_group = Some(g);
        *what = Some("New Group".into());
        ui.close();
    }
    if ui.button("Duplicate").clicked() {
        let gs = doc.duplicate_groups(&st.sel_groups.clone());
        if !gs.is_empty() {
            st.sel_groups = gs;
        }
        *what = Some("Duplicate Group".into());
        ui.close();
    }
    if ui.add_enabled(st.sel_groups.len() > 1, egui::Button::new("Create Round Robin")).clicked() {
        let mut gs = st.sel_groups.clone();
        gs.sort_unstable();
        doc.create_round_robin(&gs);
        *what = Some("Create Round Robin".into());
        ui.close();
    }
    if ui.add_enabled(st.sel_groups.len() > 1, egui::Button::new("Merge")).clicked() {
        doc.merge_groups(&st.sel_groups.clone());
        st.sel_groups = vec![*st.sel_groups.iter().min().unwrap_or(&0)];
        *what = Some("Merge Groups".into());
        ui.close();
    }
}

fn zone_menu(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, acts: &mut Vec<Act>, what: &mut Option<String>) {
    ui.set_min_width(260.0);
    let sel = st.sel_zones.clone();
    let done = |what: &mut Option<String>, w: &str, ui: &mut egui::Ui| {
        *what = Some(w.into());
        ui.close();
    };
    if ui.button("New").clicked() {
        let z = doc.new_zone(st.focus_group.or(st.sel_groups.first().copied()));
        st.sel_zones = vec![z];
        st.focus_zone = Some(z);
        st.sel_groups = vec![doc.zones[z].group];
        done(what, "New Zone", ui);
    }
    if ui.button("Load Audio Files…").clicked() {
        match st.focus_zone.filter(|_| sel.len() == 1) {
            Some(z) => acts.push(Act::LoadInto(z)),
            None => acts.push(Act::LoadFiles(st.focus_group)),
        }
        ui.close();
    }
    ui.separator();
    ui.menu_button("Split", |ui| {
        if ui.button("Create Zones Split at Transients").clicked()
            && let Some(&z) = sel.first()
        {
            st.sel_zones = doc.split_zone(z, false);
            done(what, "Split at Transients", ui);
        }
        if ui.button("Create Zones Split at Silence").clicked()
            && let Some(&z) = sel.first()
        {
            st.sel_zones = doc.split_zone(z, true);
            done(what, "Split at Silence", ui);
        }
    });
    ui.menu_button("Automap", |ui| {
        if ui.button("Automap using Current Root Note").clicked() {
            doc.automap_current_root(&sel);
            done(what, "Automap", ui);
        }
        if ui.button("Automap using Pitch Detection").clicked() {
            doc.retune(&sel);
            doc.automap_current_root(&sel);
            done(what, "Automap", ui);
        }
        if ui.button("Automap using Root Note from Audio File Names").clicked() {
            doc.automap_from_names(&sel);
            done(what, "Automap", ui);
        }
        if ui.button("Automap Velocities Only").clicked() {
            doc.automap_velocities(&sel);
            done(what, "Automap", ui);
        }
    });
    ui.menu_button("Remap", |ui| {
        if ui.button("Remap Notes…").clicked() {
            st.dialog = Some(Dialog::RemapNotes { start: 48.0, width: 1.0 });
            ui.close();
        }
        if ui.button("Remap Velocities…").clicked() {
            st.dialog = Some(Dialog::RemapVelocities { lo: 1.0, hi: 127.0 });
            ui.close();
        }
        for (label, k) in [("Remap White Notes", 0), ("Remap Black Notes", 1), ("Remap All Notes", 2), ("Remap To Root Notes", 3), ("Remap Evenly", 4), ("Remap To Round Robin", 5), ("Pivot on Corner", 6), ("Mirror Velocities", 7), ("Fill Gaps", 8), ("Fill Gaps (Notes Only)", 9), ("Fill Gaps (Vel Only)", 10), ("Swap", 11)] {
            if ui.button(label).clicked() {
                match k {
                    0 => doc.remap_colour(&sel, false),
                    1 => doc.remap_colour(&sel, true),
                    2 => doc.remap_all_notes(&sel),
                    3 => doc.remap_to_root(&sel),
                    4 => doc.remap_evenly(&sel),
                    5 => doc.remap_to_round_robin(&sel),
                    6 => doc.pivot(&sel),
                    7 => doc.mirror_velocities(&sel),
                    8 => doc.fill_gaps(&sel, true, true),
                    9 => doc.fill_gaps(&sel, true, false),
                    10 => doc.fill_gaps(&sel, false, true),
                    _ => {
                        if sel.len() == 2 {
                            doc.swap(sel[0], sel[1]);
                        }
                    }
                }
                done(what, label, ui);
            }
        }
    });
    ui.menu_button("Loop", |ui| loop_menu(ui, doc, st, what));
    ui.menu_button("Root Key", |ui| {
        if ui.button("from Audio Analysis").clicked() {
            doc.retune(&sel);
            done(what, "Root Key", ui);
        }
        if ui.button("from Audio File Name").clicked() {
            for &z in &sel {
                if let Some(zz) = doc.zones.get_mut(z)
                    && let Some(r) = sd::root_from_name(&zz.file.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| zz.name.clone()))
                {
                    zz.root = r;
                }
            }
            done(what, "Root Key", ui);
        }
        if ui.button("from Audio File Chunk (SMPL)").clicked() {
            for &z in &sel {
                if let Some(zz) = doc.zones.get_mut(z)
                    && let Some(f) = zz.file.clone()
                    && let Ok(a) = lg_synth::wav::read_audio(&f)
                    && let Some(r) = a.root
                {
                    zz.root = r;
                }
            }
            done(what, "Root Key", ui);
        }
    });
    ui.menu_button("Audio Files", |ui| {
        if ui.button("Detach").clicked() {
            for &z in &sel {
                doc.detach(z);
            }
            done(what, "Detach", ui);
        }
        if ui.button("Update Info from Audio File").clicked() {
            for &z in &sel {
                if let Some(zz) = doc.zones.get_mut(z)
                    && let Some(f) = zz.file.clone()
                    && let Ok(a) = lg_synth::wav::read_audio(&f)
                {
                    if let Some(r) = a.root {
                        zz.root = r;
                    }
                    if let Some((s, e)) = a.loop_points {
                        zz.loop_on = true;
                        zz.loop_start = s as usize;
                        zz.loop_end = e as usize;
                    }
                }
            }
            done(what, "Update Info from Audio File", ui);
        }
    });
    ui.menu_button("Rename", |ui| {
        if ui.button("Rename Zones from Audio File Names").clicked() {
            doc.rename_from_files(&sel, false);
            done(what, "Rename", ui);
        }
        if ui.button("Rename Zones from Audio File Names with Root and Velocity").clicked() {
            doc.rename_from_files(&sel, true);
            done(what, "Rename", ui);
        }
        if ui.button("Add Root and Velocity to Existing Name").clicked() {
            doc.add_root_vel_to_names(&sel);
            done(what, "Rename", ui);
        }
    });
    ui.separator();
    if ui.button("Normalize Loudness").clicked() {
        doc.normalize_loudness(&sel);
        done(what, "Normalize Loudness", ui);
    }
    if ui.button("Retune").clicked() {
        doc.retune(&sel);
        done(what, "Retune", ui);
    }
    if ui.button("Assign to New Group").clicked() {
        let g = doc.assign_to_new_group(&sel);
        st.sel_groups = vec![g];
        st.focus_group = Some(g);
        done(what, "Assign to New Group", ui);
    }
    ui.separator();
    ui.checkbox(&mut doc.move_root, "Always Move Root Key with Zone");
    ui.checkbox(&mut doc.prefer_velocity, "Prefer Splitting Zones by Velocity");
    ui.checkbox(&mut doc.slide_over, "Slide Zones Over");
    ui.checkbox(&mut st.protect_unselected, "Protect Unselected Zones in Zone List");
    ui.checkbox(&mut st.preview_selected, "Preview Selected Zone");
}

fn loop_menu(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, what: &mut Option<String>) {
    let z = st.focus_zone.or(st.sel_zones.first().copied());
    let Some(z) = z else {
        ui.label("Select a zone");
        return;
    };
    if ui.button("Auto-Loop").clicked() {
        if doc.auto_loop(z, st.autoloop_attempt, false) {
            st.autoloop_attempt += 1;
            *what = Some("Auto-Loop".into());
        }
        ui.close();
    }
    if ui.button("Auto-Loop within Loop Area").clicked() {
        if doc.auto_loop(z, st.autoloop_attempt, true) {
            st.autoloop_attempt += 1;
            *what = Some("Auto-Loop".into());
        }
        ui.close();
    }
    for c in ["Set Start to Loop Start", "Set End to Loop End", "Set Loop from Start to End", "Optimize Loop Start", "Optimize Loop Crossfade/End"] {
        if ui.button(c).clicked() {
            doc.loop_command(z, c);
            *what = Some(c.into());
            ui.close();
        }
    }
}

fn view_menu(ui: &mut egui::Ui, st: &mut Ui) {
    ui.set_min_width(240.0);
    ui.checkbox(&mut st.show_names, "Show Zone Names");
    ui.checkbox(&mut st.all_groups_zones, "Show Zones from all Groups");
    ui.checkbox(&mut st.group_list, "Show Group Column in Mapping Editor");
    ui.separator();
    ui.menu_button("Visible Group List Columns", |ui| columns_menu(ui, st, GROUP_COLS));
    ui.menu_button("Visible Zone List Columns", |ui| columns_menu(ui, st, ZONE_COLS));
    if ui.button("Show all columns").clicked() {
        st.hidden_cols.clear();
        ui.close();
    }
    if ui.button("Restore column options").clicked() {
        st.hidden_cols = default_hidden();
        ui.close();
    }
}

fn default_hidden() -> std::collections::HashSet<String> {
    ["Bend", "Channel", "Tempo", "Env 2 Offsets", "Output"].iter().map(|s| s.to_string()).collect()
}

fn columns_menu(ui: &mut egui::Ui, st: &mut Ui, cols: &[&str]) {
    for c in cols.iter().skip(1) {
        let mut on = !st.hidden_cols.contains(*c);
        if ui.checkbox(&mut on, *c).changed() {
            if on {
                st.hidden_cols.remove(*c);
            } else {
                st.hidden_cols.insert(c.to_string());
            }
        }
    }
}

// --- Key Mapping Editor

#[allow(clippy::too_many_arguments)]
fn key_mapping(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, acts: &mut Vec<Act>, what: &mut Option<String>, dropped: &[PathBuf], drop_pos: Option<egui::Pos2>) {
    let area = ui.available_rect_before_wrap();
    let list_w = if st.group_list { 170.0 } else { 0.0 };
    // the group parameters above, the zone's below
    let fields_h = 26.0;
    let kb_h = 44.0;
    let group_r = Rect::from_min_size(area.min, vec2(list_w, area.height()));
    let edit_r = Rect::from_min_max(pos2(area.min.x + list_w, area.min.y), area.max);
    if st.group_list {
        group_list(ui, group_r, doc, st, what, dropped, drop_pos);
    }
    let mut top = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(edit_r.min, vec2(edit_r.width(), fields_h))));
    group_fields(&mut top, doc, st, what);
    let grid = Rect::from_min_max(pos2(edit_r.min.x, edit_r.min.y + fields_h), pos2(edit_r.max.x, edit_r.max.y - kb_h - fields_h));
    let kb = Rect::from_min_max(pos2(edit_r.min.x, grid.max.y), pos2(edit_r.max.x, grid.max.y + kb_h));
    let mut bottom = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(edit_r.min.x, kb.max.y), edit_r.max)));
    zone_fields(&mut bottom, doc, st, what);
    // horizontal scroll: keys 0–127
    let kw = st.key_w;
    let content_w = 128.0 * kw;
    let scroll_id = ui.scope_id().with("kme-scroll");
    let mut off: f32 = ui.data(|d| d.get_temp(scroll_id)).unwrap_or(36.0 * kw);
    off = off.clamp(0.0, (content_w - grid.width()).max(0.0));
    let hov = ui.rect_contains_pointer(grid.union(kb));
    if hov {
        let (dx, dy, zoom) = ui.input(|i| (i.smooth_scroll_delta.x, i.smooth_scroll_delta.y, i.zoom_delta()));
        off -= dx;
        if ui.input(|i| i.modifiers.shift) {
            off -= dy;
        }
        if zoom != 1.0 {
            st.key_w = (st.key_w * zoom).clamp(4.0, 30.0);
        }
    }
    ui.data_mut(|d| d.insert_temp(scroll_id, off.clamp(0.0, (content_w - grid.width()).max(0.0))));
    let kx = |k: f32| grid.min.x - off + k * kw;
    let key_at = |x: f32| ((x - grid.min.x + off) / kw).floor().clamp(0.0, 127.0) as u8;
    let vy = |v: f32| grid.max.y - (v / 127.0) * grid.height();
    let vel_at = |y: f32| (((grid.max.y - y) / grid.height()) * 127.0).round().clamp(1.0, 127.0) as u8;
    let p = ui.painter_at(grid.union(kb));
    p.rect_filled(grid, 0.0, look::lane_bg());
    for k in 0..128 {
        if matches!(k % 12, 1 | 3 | 6 | 8 | 10) {
            p.rect_filled(Rect::from_min_max(pos2(kx(k as f32), grid.min.y), pos2(kx(k as f32 + 1.0), grid.max.y)), 0.0, look::lane_alt());
        }
        if k % 12 == 0 {
            p.vline(kx(k as f32), grid.y_range(), Stroke::new(1.0, look::grid_line()));
        }
    }
    let shown = zone_order(doc, st);
    let focus_g = st.focus_group;
    if shown.is_empty() {
        p.text(grid.center(), Align2::CENTER_CENTER, "Drag audio files here", look::regular(14.0), look::text_dim());
    }
    let zr = |z: &ZoneDoc| Rect::from_min_max(pos2(kx(z.key_lo as f32), vy(z.vel_hi as f32)), pos2(kx(z.key_hi as f32 + 1.0), vy(z.vel_lo as f32 - 1.0)));
    for &zi in &shown {
        let z = &doc.zones[zi];
        let r = zr(z).shrink(0.5);
        let dim = st.sel_groups.len() > 1 && focus_g.is_some_and(|g| g != z.group);
        let sel = st.sel_zones.contains(&zi);
        let base = if z.audio.is_none() { Color32::from_gray(0x60) } else { look::region_color(lg_core::TrackKind::Audio) };
        let fill = if sel { look::mix(base, Color32::WHITE, 0.35) } else { base };
        p.rect_filled(r, 2.0, if dim { fill.gamma_multiply(0.35) } else { fill.gamma_multiply(if z.mute { 0.4 } else { 0.85 }) });
        p.rect_stroke(r, 2.0, Stroke::new(if sel { 1.5 } else { 1.0 }, if sel { Color32::WHITE } else { Color32::from_black_alpha(120) }), egui::StrokeKind::Inside);
        if st.show_names && r.width() > 18.0 {
            p.with_clip_rect(r).text(r.left_top() + vec2(3.0, 2.0), Align2::LEFT_TOP, &z.name, look::regular(10.0), Color32::BLACK);
        }
    }
    // the keyboard; root keys in gold
    p.rect_filled(kb, 0.0, Color32::from_gray(0xee));
    let roots: Vec<u8> = shown.iter().map(|&z| doc.zones[z].root).collect();
    for k in 0..128u8 {
        let r = Rect::from_min_max(pos2(kx(k as f32), kb.min.y), pos2(kx(k as f32 + 1.0), kb.max.y));
        let black = matches!(k % 12, 1 | 3 | 6 | 8 | 10);
        let gold = roots.contains(&k);
        let fill = if gold { Color32::from_rgb(0xe0, 0xb0, 0x30) } else if black { Color32::from_gray(0x28) } else { Color32::from_gray(0xf4) };
        let rr = if black { Rect::from_min_max(r.min, pos2(r.max.x, kb.min.y + kb.height() * 0.62)) } else { r };
        p.rect_filled(rr.shrink2(vec2(0.5, 0.0)), 1.0, fill);
        if k % 12 == 0 && kw >= 7.0 {
            p.text(pos2(r.min.x + 1.0, kb.max.y - 2.0), Align2::LEFT_BOTTOM, sd::note_name(k), look::regular(8.5), Color32::from_gray(0x40));
        }
        if let Some(l) = doc.key_labels.get(&k)
            && kw >= 12.0
        {
            p.with_clip_rect(r).text(pos2(r.center().x, kb.min.y + 2.0), Align2::CENTER_TOP, l, look::regular(8.0), Color32::from_gray(0x30));
        }
    }
    // the keyboard plays the key (held while pressed)
    let kresp = ui.interact(kb, ui.scope_id().with("kme-kb"), Sense::click_and_drag());
    if kresp.is_pointer_button_down_on()
        && let Some(pp) = kresp.interact_pointer_pos()
    {
        let k = key_at(pp.x);
        if st.preview_held != Some(k) {
            if let Some(o) = st.preview_held {
                acts.push(Act::Preview(o, false));
            }
            acts.push(Act::Preview(k, true));
            st.preview_held = Some(k);
        }
    } else if let Some(o) = st.preview_held.take() {
        acts.push(Act::Preview(o, false));
    }
    // dropped files: on a key (or a zone) or above the keyboard (a range that widens upward)
    if !dropped.is_empty()
        && let Some(pp) = drop_pos
        && grid.union(kb).contains(pp)
    {
        let k = key_at(pp.x);
        let hit = shown.iter().copied().rev().find(|&z| zr(&doc.zones[z]).contains(pp));
        if let (Some(z), 1) = (hit, dropped.len()) {
            let _ = doc.load_into(z, &dropped[0]);
            *what = Some("Load Audio File".into());
        } else if kb.contains(pp) {
            let zs = doc.add_files(dropped, Drop::Key(k));
            st.sel_zones = zs;
            *what = Some("Add Zones".into());
        } else {
            let up = ((grid.max.y - pp.y) / grid.height()).clamp(0.0, 1.0);
            let half = (up * 24.0) as i32;
            let (lo, hi) = ((k as i32 - half).max(0) as u8, (k as i32 + half).min(127) as u8);
            let zs = doc.add_files(dropped, Drop::Range(lo, hi));
            st.sel_zones = zs;
            *what = Some("Add Zones".into());
        }
    }
    // the grid's gestures
    let gresp = ui.interact(grid, ui.scope_id().with("kme-grid"), Sense::click_and_drag());
    let m = ui.input(|i| i.modifiers);
    if let Some(pp) = gresp.hover_pos() {
        // the resize pointer at a zone's edges
        if let Some(&zi) = shown.iter().rev().find(|&&z| zr(&doc.zones[z]).expand(3.0).contains(pp)) {
            let r = zr(&doc.zones[zi]);
            if (pp.x - r.min.x).abs() < 4.0 || (pp.x - r.max.x).abs() < 4.0 {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            } else if (pp.y - r.min.y).abs() < 4.0 || (pp.y - r.max.y).abs() < 4.0 {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
        }
    }
    if gresp.clicked()
        && let Some(pp) = gresp.interact_pointer_pos()
    {
        let hit = shown.iter().copied().rev().find(|&z| zr(&doc.zones[z]).contains(pp) && (st.sel_groups.len() <= 1 || st.focus_group == Some(doc.zones[z].group)));
        match hit {
            Some(z) if m.command && m.alt && st.sel_zones.contains(&z) => {
                // divide vertically: two zones side by side
                let mut b = doc.zones[z].clone();
                let mid = ((doc.zones[z].key_lo as u16 + doc.zones[z].key_hi as u16) / 2) as u8;
                if doc.zones[z].key_hi > doc.zones[z].key_lo {
                    doc.zones[z].key_hi = mid;
                    b.key_lo = mid + 1;
                    doc.zones.push(b);
                    *what = Some("Divide Zone".into());
                }
            }
            Some(z) if m.shift && m.alt && st.sel_zones.contains(&z) => {
                // divide horizontally: two velocity layers
                let mut b = doc.zones[z].clone();
                let mid = ((doc.zones[z].vel_lo as u16 + doc.zones[z].vel_hi as u16) / 2) as u8;
                if doc.zones[z].vel_hi > doc.zones[z].vel_lo {
                    doc.zones[z].vel_hi = mid;
                    b.vel_lo = mid + 1;
                    doc.zones.push(b);
                    *what = Some("Divide Zone".into());
                }
            }
            Some(z) => {
                select_zone(st, z, m, &shown, doc);
                if st.preview_selected {
                    acts.push(Act::PreviewZone);
                }
            }
            None => {
                if !m.shift && !m.command {
                    st.sel_zones.clear();
                }
            }
        }
    }
    if gresp.drag_started()
        && let Some(pp) = gresp.interact_pointer_pos()
    {
        let start = pp - gresp.drag_delta();
        let hit = shown.iter().copied().rev().find(|&z| zr(&doc.zones[z]).expand(3.0).contains(start) && (st.sel_groups.len() <= 1 || st.focus_group == Some(doc.zones[z].group)));
        st.kdrag = Some(match hit {
            _ if m.command && m.shift => KDrag::Draw { start },
            Some(z) => {
                let r = zr(&doc.zones[z]);
                let edge = if (start.x - r.min.x).abs() < 4.0 {
                    Some(0)
                } else if (start.x - r.max.x).abs() < 4.0 {
                    Some(1)
                } else if (start.y - r.min.y).abs() < 4.0 {
                    Some(2)
                } else if (start.y - r.max.y).abs() < 4.0 {
                    Some(3)
                } else {
                    None
                };
                if !st.sel_zones.contains(&z) {
                    st.sel_zones = vec![z];
                    st.focus_zone = Some(z);
                }
                match edge {
                    Some(e) if st.sel_zones.len() <= 1 || e >= 2 => KDrag::Edge { zone: z, edge: e, orig: doc.zones[z].clone() },
                    _ => KDrag::Move { start, orig: st.sel_zones.iter().map(|&z| (z, doc.zones[z].clone())).collect(), axis: None, root_only: m.command && m.alt && !doc.move_root },
                }
            }
            None => KDrag::Band { start },
        });
    }
    if let Some(pp) = gresp.interact_pointer_pos().filter(|_| gresp.dragged()) {
        match st.kdrag.clone() {
            Some(KDrag::Move { start, orig, mut axis, root_only }) => {
                let d = pp - start;
                if m.command && axis.is_none() && d.length() > 4.0 {
                    axis = Some(d.x.abs() >= d.y.abs());
                }
                let dk = if axis == Some(false) { 0 } else { (d.x / kw).round() as i32 };
                let dv = if axis == Some(true) { 0 } else { (-d.y / grid.height() * 127.0).round() as i32 };
                let ok = orig.iter().all(|(_, z)| z.key_lo as i32 + dk >= 0 && z.key_hi as i32 + dk <= 127 && z.vel_lo as i32 + dv >= 1 && z.vel_hi as i32 + dv <= 127);
                if ok {
                    for (zi, z) in &orig {
                        let n = &mut doc.zones[*zi];
                        if root_only {
                            n.root = (z.root as i32 + dk).clamp(0, 127) as u8;
                            continue;
                        }
                        n.key_lo = (z.key_lo as i32 + dk) as u8;
                        n.key_hi = (z.key_hi as i32 + dk) as u8;
                        n.vel_lo = (z.vel_lo as i32 + dv) as u8;
                        n.vel_hi = (z.vel_hi as i32 + dv) as u8;
                        if doc.move_root || (m.command && m.alt) {
                            n.root = (z.root as i32 + dk).clamp(0, 127) as u8;
                        }
                    }
                    if dk != 0 || dv != 0 {
                        *what = Some(if root_only { "Move Root Key".into() } else { "Move Zone".into() });
                    }
                }
                st.kdrag = Some(KDrag::Move { start, orig, axis, root_only });
            }
            Some(KDrag::Edge { zone, edge, orig }) => {
                let n = &mut doc.zones[zone];
                match edge {
                    0 => n.key_lo = key_at(pp.x + kw * 0.5).min(orig.key_hi),
                    1 => n.key_hi = key_at(pp.x - kw * 0.5).max(orig.key_lo),
                    2 => {
                        // the top edge: velocity high (Shift-selected zones together)
                        let nv = vel_at(pp.y).max(orig.vel_lo);
                        let dv = nv as i32 - orig.vel_hi as i32;
                        n.vel_hi = nv;
                        for &o in &st.sel_zones {
                            if o != zone && let Some(z) = doc.zones.get_mut(o) {
                                z.vel_hi = (z.vel_hi as i32 + dv).clamp(z.vel_lo as i32, 127) as u8;
                            }
                        }
                    }
                    _ => {
                        let nv = vel_at(pp.y).min(orig.vel_hi).max(1);
                        n.vel_lo = nv;
                    }
                }
                *what = Some("Resize Zone".into());
            }
            Some(KDrag::Draw { start }) => {
                let (a, b) = (key_at(start.x.min(pp.x)), key_at(start.x.max(pp.x)));
                let (vl, vh) = (vel_at(start.y.max(pp.y)), vel_at(start.y.min(pp.y)));
                p.rect_stroke(Rect::from_two_pos(start, pp), 0.0, Stroke::new(1.0, look::accent()), egui::StrokeKind::Inside);
                p.text(pp + vec2(8.0, -8.0), Align2::LEFT_BOTTOM, format!("{}–{}  {vl}–{vh}", sd::note_name(a), sd::note_name(b)), look::regular(11.0), look::text());
            }
            Some(KDrag::Band { start }) => {
                let band = Rect::from_two_pos(start, pp);
                p.rect_filled(band, 0.0, look::accent().gamma_multiply(0.15));
                p.rect_stroke(band, 0.0, Stroke::new(1.0, look::accent()), egui::StrokeKind::Inside);
                let mut sel: Vec<usize> = shown.iter().copied().filter(|&z| zr(&doc.zones[z]).intersects(band)).collect();
                if m.shift {
                    for z in st.sel_zones.clone() {
                        if !sel.contains(&z) {
                            sel.push(z);
                        }
                    }
                }
                st.sel_zones = sel;
            }
            None => {}
        }
    }
    if gresp.drag_stopped() {
        match st.kdrag.take() {
            Some(KDrag::Move { orig, .. }) => {
                let moved: Vec<usize> = orig.iter().map(|(z, _)| *z).collect();
                doc.resolve_overlaps(&moved);
            }
            Some(KDrag::Edge { zone, .. }) => doc.resolve_overlaps(&[zone]),
            Some(KDrag::Draw { start }) => {
                if let Some(pp) = gresp.interact_pointer_pos() {
                    let g = st.focus_group.or(st.sel_groups.first().copied());
                    let z = doc.new_zone(g);
                    let n = &mut doc.zones[z];
                    n.key_lo = key_at(start.x.min(pp.x));
                    n.key_hi = key_at(start.x.max(pp.x));
                    n.root = n.key_lo;
                    n.vel_lo = vel_at(start.y.max(pp.y));
                    n.vel_hi = vel_at(start.y.min(pp.y)).max(n.vel_lo);
                    doc.resolve_overlaps(&[z]);
                    st.sel_zones = vec![z];
                    st.focus_zone = Some(z);
                    *what = Some("New Zone".into());
                }
            }
            _ => {}
        }
    }
    gresp.context_menu(|ui| {
        zone_menu(ui, doc, st, acts, what);
    });
}

fn select_zone(st: &mut Ui, z: usize, m: egui::Modifiers, order: &[usize], doc: &SamplerDoc) {
    if m.shift && !st.sel_zones.is_empty() {
        // a range: the zones in between too (in the list's order)
        let last = *st.sel_zones.last().unwrap_or(&z);
        let (a, b) = (order.iter().position(|&x| x == last), order.iter().position(|&x| x == z));
        if let (Some(a), Some(b)) = (a, b) {
            for &x in &order[a.min(b)..=a.max(b)] {
                if !st.sel_zones.contains(&x) {
                    st.sel_zones.push(x);
                }
            }
        }
    } else if m.command {
        if let Some(i) = st.sel_zones.iter().position(|&x| x == z) {
            st.sel_zones.remove(i);
        } else {
            st.sel_zones.push(z);
        }
    } else {
        st.sel_zones = vec![z];
    }
    st.focus_zone = Some(z);
    if let Some(zz) = doc.zones.get(z) {
        st.focus_group = Some(zz.group);
        if !st.sel_groups.contains(&zz.group) && !st.all_groups_zones {
            st.sel_groups = vec![zz.group];
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn group_list(ui: &mut egui::Ui, r: Rect, doc: &mut SamplerDoc, st: &mut Ui, what: &mut Option<String>, dropped: &[PathBuf], drop_pos: Option<egui::Pos2>) {
    let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(r));
    cell.painter().rect_filled(r, 0.0, look::header_bg());
    egui::ScrollArea::vertical().id_salt("sampler-groups").show(&mut cell, |ui| {
        ui.set_width(r.width() - 4.0);
        for g in 0..doc.groups.len() {
            let sel = st.sel_groups.contains(&g);
            let focus = st.focus_group == Some(g) && st.sel_groups.len() > 1;
            let row = ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{}", g + 1)).size(10.0).color(if focus { look::accent() } else { look::text_dim() }));
                let name = doc.groups[g].name.clone();
                let resp = ui.add(egui::Button::selectable(sel, egui::RichText::new(name).size(11.5)).min_size(vec2(90.0, 18.0)));
                let m = ui.input(|i| i.modifiers);
                if resp.clicked() {
                    if m.shift || m.command {
                        if let Some(i) = st.sel_groups.iter().position(|&x| x == g) {
                            if st.sel_groups.len() > 1 {
                                st.sel_groups.remove(i);
                            }
                        } else {
                            st.sel_groups.push(g);
                        }
                    } else {
                        st.sel_groups = vec![g];
                    }
                    st.focus_group = Some(g);
                    st.sel_zones.retain(|&z| doc.zones.get(z).is_some_and(|zz| zz.group == g));
                }
                if resp.double_clicked() {
                    st.dialog = Some(Dialog::RenameGroup { group: g, name: doc.groups[g].name.clone() });
                }
                if toggle(ui, "M", &mut doc.groups[g].mute) {
                    *what = Some("Group Mute".into());
                }
                if toggle(ui, "S", &mut doc.groups[g].solo) {
                    *what = Some("Group Solo".into());
                }
            });
            // a file dropped on a group's name: a new zone in that group
            if !dropped.is_empty()
                && drop_pos.is_some_and(|p| row.response.rect.contains(p))
            {
                let zs = doc.add_files(dropped, Drop::Group(g));
                st.sel_zones = zs;
                *what = Some("Add Zones".into());
            }
        }
        // below the last group: a new group with the dropped zone
        let (rest, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        if !dropped.is_empty() && drop_pos.is_some_and(|p| rest.contains(p)) {
            let g = doc.new_group();
            let zs = doc.add_files(dropped, Drop::Group(g));
            st.sel_zones = zs;
            st.sel_groups = vec![g];
            *what = Some("Add Zones".into());
        }
    });
}

fn group_fields(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, what: &mut Option<String>) {
    let Some(g) = st.focus_group.or(st.sel_groups.first().copied()).filter(|&g| g < doc.groups.len()) else { return };
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.menu_button(egui::RichText::new(format!("{} ⌄", doc.groups[g].name)).size(11.5), |ui| {
            for k in 0..doc.groups.len() {
                if ui.add(egui::Button::selectable(k == g, &doc.groups[k].name)).clicked() {
                    st.sel_groups = vec![k];
                    st.focus_group = Some(k);
                    ui.close();
                }
            }
        });
        let gr = &mut doc.groups[g];
        let mut ch = false;
        ui.label(egui::RichText::new("Volume").size(11.0).color(look::text_dim()));
        ch |= field(ui, ui.scope_id().with("g-vol"), 60.0, &mut gr.volume_db, -96.0, 24.0, 0.0, 0.5, &db);
        ui.label(egui::RichText::new("Pan").size(11.0).color(look::text_dim()));
        ch |= field(ui, ui.scope_id().with("g-pan"), 44.0, &mut gr.pan, -1.0, 1.0, 0.0, 1.0 / 64.0, &pan_s);
        ui.label(egui::RichText::new("Key Range").size(11.0).color(look::text_dim()));
        ch |= note_field(ui, ui.scope_id().with("g-klo"), &mut gr.key_lo);
        ch |= note_field(ui, ui.scope_id().with("g-khi"), &mut gr.key_hi);
        ui.label(egui::RichText::new("Velocity Range").size(11.0).color(look::text_dim()));
        ch |= ifield(ui, ui.scope_id().with("g-vlo"), 34.0, &mut gr.vel_lo, 1.0, 127.0, &|x| format!("{x:.0}"));
        ch |= ifield(ui, ui.scope_id().with("g-vhi"), 34.0, &mut gr.vel_hi, 1.0, 127.0, &|x| format!("{x:.0}"));
        if ch {
            *what = Some("Group Parameter".into());
        }
    });
}

fn zone_fields(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, what: &mut Option<String>) {
    let Some(z) = st.focus_zone.filter(|&z| z < doc.zones.len()) else {
        ui.label(egui::RichText::new("  No zone selected").color(look::text_dim()));
        return;
    };
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.menu_button(egui::RichText::new(format!("{} ⌄", if doc.zones[z].name.is_empty() { "Zone" } else { &doc.zones[z].name })).size(11.5), |ui| {
            for k in zone_order(doc, st) {
                if ui.add(egui::Button::selectable(k == z, &doc.zones[k].name)).clicked() {
                    st.sel_zones = vec![k];
                    st.focus_zone = Some(k);
                    ui.close();
                }
            }
        });
        let zz = &mut doc.zones[z];
        let mut ch = false;
        ui.label(egui::RichText::new("Root Key").size(11.0).color(look::text_dim()));
        ch |= note_field(ui, ui.scope_id().with("z-root"), &mut zz.root);
        ui.label(egui::RichText::new("Tune").size(11.0).color(look::text_dim()));
        ch |= field(ui, ui.scope_id().with("z-tune"), 60.0, &mut zz.tune, -9600.0, 9600.0, 0.0, 1.0, &|c| format!("{:+.0} {:+.0}", (c / 100.0).trunc(), c % 100.0));
        ch |= toggle(ui, "Pitch", &mut zz.pitch);
        ui.label(egui::RichText::new("Volume").size(11.0).color(look::text_dim()));
        ch |= field(ui, ui.scope_id().with("z-vol"), 60.0, &mut zz.volume_db, -96.0, 24.0, 0.0, 0.5, &db);
        ui.label(egui::RichText::new("Pan").size(11.0).color(look::text_dim()));
        ch |= field(ui, ui.scope_id().with("z-pan"), 44.0, &mut zz.pan, -1.0, 1.0, 0.0, 1.0 / 64.0, &pan_s);
        ui.label(egui::RichText::new("Key Range").size(11.0).color(look::text_dim()));
        ch |= note_field(ui, ui.scope_id().with("z-klo"), &mut zz.key_lo);
        ch |= note_field(ui, ui.scope_id().with("z-khi"), &mut zz.key_hi);
        ui.label(egui::RichText::new("Velocity Range").size(11.0).color(look::text_dim()));
        ch |= ifield(ui, ui.scope_id().with("z-vlo"), 34.0, &mut zz.vel_lo, 1.0, 127.0, &|x| format!("{x:.0}"));
        ch |= ifield(ui, ui.scope_id().with("z-vhi"), 34.0, &mut zz.vel_hi, 1.0, 127.0, &|x| format!("{x:.0}"));
        if ch {
            if zz.key_hi < zz.key_lo {
                zz.key_hi = zz.key_lo;
            }
            if zz.vel_hi < zz.vel_lo {
                zz.vel_hi = zz.vel_lo;
            }
            *what = Some("Zone Parameter".into());
        }
    });
}

// --- Group view and Zone view (tables)

const GROUP_COLS: &[&str] = &["#", "Mute / Solo", "Name", "Volume", "Pan", "Output", "Key Range", "Velocity Range", "Round Robin", "Articulation", "Note", "Control", "Bend", "Channel", "Tempo", "Release Trigger", "Voices", "Exclusive", "Env 1 Offsets", "Filter Offsets"];
const ZONE_COLS: &[&str] = &["#", "Mute", "Name", "Audio File", "Key", "Tune", "Key Range", "Velocity Range", "Volume", "Pan", "Scale", "Output", "Pitch", "1 Shot", "Reverse", "Group", "Loop", "Sample Start / End", "Fades"];

fn header_row(ui: &mut egui::Ui, st: &mut Ui, cols: &[&str], view: MapView) {
    ui.horizontal(|ui| {
        for (k, c) in cols.iter().enumerate() {
            if st.hidden_cols.contains(*c) {
                continue;
            }
            let sorted = st.sort.filter(|s| s.0 == view && s.1 == k);
            let label = match sorted {
                Some((_, _, true)) => format!("{c} ▼"),
                Some(_) => format!("{c} ▲"),
                None => c.to_string(),
            };
            let resp = ui.add(egui::Button::new(egui::RichText::new(label).size(10.5).color(look::text_dim())).frame(false).min_size(vec2(col_w(c), 16.0)));
            if resp.clicked() {
                st.sort = match sorted {
                    Some((_, _, rev)) => Some((view, k, !rev)),
                    None => Some((view, k, false)),
                };
            }
            resp.context_menu(|ui| {
                columns_menu(ui, st, cols);
                ui.separator();
                if ui.button("Show all columns").clicked() {
                    st.hidden_cols.clear();
                    ui.close();
                }
                if ui.button("Restore column options").clicked() {
                    st.hidden_cols = default_hidden();
                    ui.close();
                }
            });
        }
    });
}

fn col_w(c: &str) -> f32 {
    match c {
        "#" => 24.0,
        "Mute" | "Pitch" | "1 Shot" | "Reverse" => 44.0,
        "Mute / Solo" => 60.0,
        "Name" | "Audio File" => 130.0,
        "Key Range" | "Velocity Range" => 170.0,
        "Loop" => 330.0,
        "Sample Start / End" | "Fades" => 150.0,
        "Round Robin" | "Release Trigger" | "Control" | "Bend" | "Tempo" => 150.0,
        "Env 1 Offsets" => 230.0,
        "Filter Offsets" => 130.0,
        _ => 64.0,
    }
}

fn group_view(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, _acts: &mut Vec<Act>, what: &mut Option<String>, dropped: &[PathBuf]) {
    let _ = dropped;
    egui::ScrollArea::both().id_salt("sampler-group-view").auto_shrink([false, false]).show(ui, |ui| {
        header_row(ui, st, GROUP_COLS, MapView::Group);
        let mut order: Vec<usize> = (0..doc.groups.len()).collect();
        if let Some((MapView::Group, col, rev)) = st.sort {
            order.sort_by(|&a, &b| {
                let k = |g: usize| -> (f32, String) {
                    let gr = &doc.groups[g];
                    match GROUP_COLS[col] {
                        "Name" => (0.0, gr.name.clone()),
                        "Volume" => (gr.volume_db, String::new()),
                        "Pan" => (gr.pan, String::new()),
                        "Key Range" => (gr.key_lo as f32, String::new()),
                        "Velocity Range" => (gr.vel_lo as f32, String::new()),
                        "Voices" => (gr.voices as f32, String::new()),
                        "Exclusive" => (gr.exclusive as f32, String::new()),
                        _ => (g as f32, String::new()),
                    }
                };
                let o = k(a).partial_cmp(&k(b)).unwrap_or(std::cmp::Ordering::Equal);
                if rev { o.reverse() } else { o }
            });
        }
        for g in order {
            let hidden = st.hidden_cols.clone();
            let shown = |c: &str| !hidden.contains(c);
            let sel = st.sel_groups.contains(&g);
            let cycle = doc.cycle_label(g);
            let mut ch = false;
            let resp = ui.horizontal(|ui| {
                let id = ui.scope_id().with(("gv", g));
                if ui.add(egui::Button::selectable(sel, format!("{}", g + 1)).min_size(vec2(col_w("#"), 18.0))).clicked() {
                    let m = ui.input(|i| i.modifiers);
                    if m.shift || m.command {
                        if !st.sel_groups.contains(&g) {
                            st.sel_groups.push(g);
                        }
                    } else {
                        st.sel_groups = vec![g];
                    }
                    st.focus_group = Some(g);
                }
                let gr = &mut doc.groups[g];
                if shown("Mute / Solo") {
                    ch |= toggle(ui, "M", &mut gr.mute);
                    ch |= toggle(ui, "S", &mut gr.solo);
                }
                if shown("Name") {
                    let r = ui.add(egui::Label::new(egui::RichText::new(&gr.name).size(11.5)).sense(Sense::click()));
                    if r.double_clicked() {
                        st.dialog = Some(Dialog::RenameGroup { group: g, name: gr.name.clone() });
                    }
                    ui.add_space((col_w("Name") - r.rect.width()).max(0.0));
                }
                if shown("Volume") {
                    ch |= field(ui, id.with("vol"), 60.0, &mut gr.volume_db, -96.0, 24.0, 0.0, 0.5, &db);
                }
                if shown("Pan") {
                    ch |= field(ui, id.with("pan"), 44.0, &mut gr.pan, -1.0, 1.0, 0.0, 1.0 / 64.0, &pan_s);
                }
                if shown("Output") {
                    ch |= ifield(ui, id.with("out"), 60.0, &mut gr.output, 0.0, 15.0, &|x| if x < 0.5 { "Main".into() } else { format!("{:.0}-{:.0}", x * 2.0 + 1.0, x * 2.0 + 2.0) });
                }
                if shown("Key Range") {
                    ch |= note_field(ui, id.with("klo"), &mut gr.key_lo);
                    ch |= note_field(ui, id.with("khi"), &mut gr.key_hi);
                    ch |= ifield(ui, id.with("kxf"), 30.0, &mut gr.key_xfade, 0.0, 127.0, &|x| format!("{x:.0}"));
                    if let Some(k) = choice(ui, id.with("kxt"), 50.0, gr.key_xfade_type.index(), &["dB", "Gain", "EqP"]) {
                        gr.key_xfade_type = XfadeType::from_index(k);
                        ch = true;
                    }
                }
                if shown("Velocity Range") {
                    ch |= ifield(ui, id.with("vlo"), 34.0, &mut gr.vel_lo, 1.0, 127.0, &|x| format!("{x:.0}"));
                    ch |= ifield(ui, id.with("vhi"), 34.0, &mut gr.vel_hi, 1.0, 127.0, &|x| format!("{x:.0}"));
                    ch |= ifield(ui, id.with("vxf"), 30.0, &mut gr.vel_xfade, 0.0, 127.0, &|x| format!("{x:.0}"));
                    if let Some(k) = choice(ui, id.with("vxt"), 50.0, gr.vel_xfade_type.index(), &["dB", "Gain", "EqP"]) {
                        gr.vel_xfade_type = XfadeType::from_index(k);
                        ch = true;
                    }
                }
                if shown("Round Robin") {
                    let mut on = gr.round_robin.is_some();
                    if toggle(ui, "On", &mut on) {
                        gr.round_robin = if on { Some(0) } else { None };
                        ch = true;
                    }
                    ui.menu_button(cycle.clone().unwrap_or_else(|| "—".into()), |ui| {
                        for c in 0..6u8 {
                            if ui.button(format!("Cycle {}", (b'A' + c) as char)).clicked() {
                                gr.round_robin = Some(c);
                                ch = true;
                                ui.close();
                            }
                        }
                        if ui.button("Remove from Cycle").clicked() {
                            gr.round_robin = None;
                            ch = true;
                            ui.close();
                        }
                    });
                }
                let opt_u8 = |ui: &mut egui::Ui, id: egui::Id, v: &mut Option<u8>, def: u8, min: f32, max: f32, shownf: &dyn Fn(f32) -> String| -> bool {
                    let mut on = v.is_some();
                    let mut ch = toggle(ui, "On", &mut on);
                    if ch {
                        *v = if on { Some(def) } else { None };
                    }
                    if let Some(x) = v.as_mut() {
                        ch |= ifield(ui, id, 44.0, x, min, max, shownf);
                    }
                    ch
                };
                if shown("Articulation") {
                    ch |= opt_u8(ui, id.with("art"), &mut gr.articulation, 1, 1.0, 254.0, &|x| format!("{x:.0}"));
                }
                if shown("Note") {
                    ch |= opt_u8(ui, id.with("note"), &mut gr.on_note, 24, 0.0, 127.0, &|x| sd::note_name(x as u8));
                }
                if shown("Control") {
                    let mut on = gr.on_control.is_some();
                    if toggle(ui, "On", &mut on) {
                        gr.on_control = if on { Some((1, 0, 127)) } else { None };
                        ch = true;
                    }
                    if let Some((cc, lo, hi)) = gr.on_control.as_mut() {
                        ch |= ifield(ui, id.with("cc"), 30.0, cc, 0.0, 127.0, &|x| format!("{x:.0}"));
                        ch |= ifield(ui, id.with("cclo"), 30.0, lo, 0.0, 127.0, &|x| format!("{x:.0}"));
                        ch |= ifield(ui, id.with("cchi"), 30.0, hi, 0.0, 127.0, &|x| format!("{x:.0}"));
                    }
                }
                if shown("Bend") {
                    let mut on = gr.on_bend.is_some();
                    if toggle(ui, "On", &mut on) {
                        gr.on_bend = if on { Some((-64, 63)) } else { None };
                        ch = true;
                    }
                    if let Some((lo, hi)) = gr.on_bend.as_mut() {
                        ch |= ifield(ui, id.with("blo"), 34.0, lo, -64.0, 63.0, &|x| format!("{x:.0}"));
                        ch |= ifield(ui, id.with("bhi"), 34.0, hi, -64.0, 63.0, &|x| format!("{x:.0}"));
                    }
                }
                if shown("Channel") {
                    ch |= opt_u8(ui, id.with("chan"), &mut gr.on_channel, 1, 1.0, 16.0, &|x| format!("{x:.0}"));
                }
                if shown("Tempo") {
                    let mut on = gr.on_tempo.is_some();
                    if toggle(ui, "On", &mut on) {
                        gr.on_tempo = if on { Some((60.0, 180.0)) } else { None };
                        ch = true;
                    }
                    if let Some((lo, hi)) = gr.on_tempo.as_mut() {
                        ch |= field(ui, id.with("tlo"), 44.0, lo, 5.0, 990.0, 60.0, 1.0, &|x| format!("{x:.0}"));
                        ch |= field(ui, id.with("thi"), 44.0, hi, 5.0, 990.0, 180.0, 1.0, &|x| format!("{x:.0}"));
                    }
                }
                if shown("Release Trigger") {
                    ch |= toggle(ui, "On", &mut gr.release_trigger);
                    if gr.release_trigger {
                        let mut dec = gr.release_decay.is_some();
                        if toggle(ui, "Decay", &mut dec) {
                            gr.release_decay = if dec { Some(1.0) } else { None };
                            ch = true;
                        }
                        if let Some(tm) = gr.release_decay.as_mut() {
                            ch |= field(ui, id.with("rdt"), 50.0, tm, 0.01, 20.0, 1.0, 0.05, &|x| format!("{x:.2} s"));
                        }
                    }
                }
                if shown("Voices") {
                    ch |= ifield(ui, id.with("voices"), 50.0, &mut gr.voices, 0.0, 64.0, &|x| if x < 0.5 { "Max".into() } else { format!("{x:.0}") });
                }
                if shown("Exclusive") {
                    ch |= ifield(ui, id.with("excl"), 50.0, &mut gr.exclusive, 0.0, 32.0, &|x| if x < 0.5 { "Off".into() } else { format!("{x:.0}") });
                }
                if shown("Env 1 Offsets") {
                    for (k, n) in ["A", "D", "R", "S"].iter().enumerate() {
                        ui.label(egui::RichText::new(*n).size(10.0).color(look::text_dim()));
                        let (min, max, step) = if k == 3 { (-1.0, 1.0, 0.01) } else { (-10.0, 10.0, 0.01) };
                        ch |= field(ui, id.with(("e1o", k)), 42.0, &mut gr.env1_offset[k], min, max, 0.0, step, &|x| format!("{x:+.2}"));
                    }
                }
                if shown("Filter Offsets") {
                    ch |= field(ui, id.with("fco"), 52.0, &mut gr.cutoff_offset, -6.0, 6.0, 0.0, 0.05, &|x| format!("{x:+.2} oct"));
                    ch |= field(ui, id.with("fro"), 44.0, &mut gr.res_offset, -1.0, 1.0, 0.0, 0.01, &|x| format!("{:+.0}%", x * 100.0));
                }
            });
            let _ = resp;
            if ch {
                *what = Some("Group Parameter".into());
            }
        }
    });
}

fn zone_view(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, acts: &mut Vec<Act>, what: &mut Option<String>, dropped: &[PathBuf]) {
    let _ = dropped;
    let missing_col = doc.zones.iter().any(|z| z.missing);
    egui::ScrollArea::both().id_salt("sampler-zone-view").auto_shrink([false, false]).show(ui, |ui| {
        header_row(ui, st, ZONE_COLS, MapView::Zone);
        let mut order = zone_order(doc, st);
        if order.is_empty() {
            ui.label(egui::RichText::new("Drag audio files here").color(look::text_dim()));
        }
        if let Some((MapView::Zone, col, rev)) = st.sort {
            order.sort_by(|&a, &b| {
                let k = |z: usize| -> (f32, String) {
                    let zz = &doc.zones[z];
                    match ZONE_COLS[col] {
                        "Name" => (0.0, zz.name.clone()),
                        "Audio File" => (0.0, zz.file_name()),
                        "Key" => (zz.root as f32, String::new()),
                        "Tune" => (zz.tune, String::new()),
                        "Key Range" => (zz.key_lo as f32, String::new()),
                        "Velocity Range" => (zz.vel_lo as f32, String::new()),
                        "Volume" => (zz.volume_db, String::new()),
                        "Pan" => (zz.pan, String::new()),
                        "Group" => (zz.group as f32, String::new()),
                        _ => (z as f32, String::new()),
                    }
                };
                let o = k(a).partial_cmp(&k(b)).unwrap_or(std::cmp::Ordering::Equal);
                if rev { o.reverse() } else { o }
            });
        }
        let ngroups: Vec<String> = doc.groups.iter().map(|g| g.name.clone()).collect();
        let snapshot_order = order.clone();
        for z in order {
            let hidden = st.hidden_cols.clone();
            let shown = |c: &str| !hidden.contains(c);
            let sel = st.sel_zones.contains(&z);
            let mut ch = false;
            let before = doc.zones[z].clone();
            ui.horizontal(|ui| {
                let id = ui.scope_id().with(("zv", z));
                let num = ui.add(egui::Button::selectable(sel, format!("{}", z + 1)).min_size(vec2(col_w("#"), 18.0)));
                if num.clicked() {
                    let m = ui.input(|i| i.modifiers);
                    // the number of a selected zone focuses it (the selection stays)
                    if sel && st.sel_zones.len() > 1 && !m.shift && !m.command {
                        st.focus_zone = Some(z);
                    } else {
                        select_zone(st, z, m, &snapshot_order, doc);
                    }
                    if st.preview_selected {
                        acts.push(Act::PreviewZone);
                    }
                }
                if missing_col {
                    ui.label(if doc.zones[z].missing { "!" } else { " " });
                }
                let zz = &mut doc.zones[z];
                if shown("Mute") {
                    ch |= toggle(ui, "M", &mut zz.mute);
                }
                if shown("Name") {
                    let r = ui.add(egui::Label::new(egui::RichText::new(&zz.name).size(11.5)).sense(Sense::click()));
                    if r.double_clicked() {
                        st.dialog = Some(Dialog::Rename { zone: z, name: zz.name.clone() });
                    }
                    ui.add_space((col_w("Name") - r.rect.width()).max(0.0));
                }
                if shown("Audio File") {
                    let label = if zz.file.is_none() && zz.audio.is_none() { "Load Audio File…".to_string() } else if zz.file.is_none() { "(in memory)".into() } else { zz.file_name() };
                    ui.menu_button(egui::RichText::new(label).size(11.0), |ui| {
                        if ui.button("Load Audio File").clicked() {
                            acts.push(Act::LoadInto(z));
                            ui.close();
                        }
                        if ui.button("Rename Current File").clicked() {
                            st.dialog = Some(Dialog::Rename { zone: z, name: zz.file_name() });
                            ui.close();
                        }
                        if ui.button("Detach Current File").clicked() {
                            zz.audio = None;
                            zz.file = None;
                            ch = true;
                            ui.close();
                        }
                        if ui.button("Show in Finder").clicked() {
                            acts.push(Act::ShowInFolder(z));
                            ui.close();
                        }
                        if ui.button("Open in Audio File Editor").clicked() {
                            acts.push(Act::OpenInEditor(z));
                            ui.close();
                        }
                    });
                }
                if shown("Key") {
                    ch |= note_field(ui, id.with("root"), &mut zz.root);
                }
                if shown("Tune") {
                    ch |= field(ui, id.with("tune"), 56.0, &mut zz.tune, -9600.0, 9600.0, 0.0, 1.0, &|c| format!("{c:+.0} ct"));
                }
                if shown("Key Range") {
                    ch |= note_field(ui, id.with("klo"), &mut zz.key_lo);
                    ch |= note_field(ui, id.with("khi"), &mut zz.key_hi);
                }
                if shown("Velocity Range") {
                    ch |= ifield(ui, id.with("vlo"), 34.0, &mut zz.vel_lo, 1.0, 127.0, &|x| format!("{x:.0}"));
                    ch |= ifield(ui, id.with("vhi"), 34.0, &mut zz.vel_hi, 1.0, 127.0, &|x| format!("{x:.0}"));
                }
                if shown("Volume") {
                    ch |= field(ui, id.with("vol"), 60.0, &mut zz.volume_db, -96.0, 24.0, 0.0, 0.5, &db);
                }
                if shown("Pan") {
                    ch |= field(ui, id.with("pan"), 44.0, &mut zz.pan, -1.0, 1.0, 0.0, 1.0 / 64.0, &pan_s);
                }
                if shown("Scale") {
                    ch |= field(ui, id.with("scale"), 44.0, &mut zz.scale, -100.0, 100.0, 0.0, 1.0, &|x| format!("{x:+.0}"));
                }
                if shown("Output") {
                    ch |= ifield(ui, id.with("out"), 60.0, &mut zz.output, 0.0, 15.0, &|x| if x < 0.5 { "Main".into() } else { format!("{:.0}-{:.0}", x * 2.0 + 1.0, x * 2.0 + 2.0) });
                }
                if shown("Pitch") {
                    ch |= toggle(ui, "Pitch", &mut zz.pitch);
                }
                if shown("1 Shot") {
                    ch |= toggle(ui, "1 Shot", &mut zz.one_shot);
                }
                if shown("Reverse") {
                    ch |= toggle(ui, "Rev", &mut zz.reverse);
                }
                if shown("Group") {
                    let cur = zz.group;
                    let refs: Vec<&str> = ngroups.iter().map(String::as_str).collect();
                    if let Some(g) = choice(ui, id.with("grp"), 100.0, cur, &refs) {
                        // the group assignment of every selected zone
                        if sel {
                            for &o in &st.sel_zones.clone() {
                                if let Some(oz) = doc.zones.get_mut(o) {
                                    oz.group = g;
                                }
                            }
                        } else {
                            doc.zones[z].group = g;
                        }
                        ch = true;
                    }
                }
                let zz = &mut doc.zones[z];
                if shown("Loop") {
                    ch |= toggle(ui, "Loop", &mut zz.loop_on);
                    ui.add_enabled_ui(zz.loop_on && !zz.one_shot, |ui| {
                        if let Some(k) = choice(ui, id.with("ldir"), 70.0, zz.loop_dir as usize, &["Forward", "Reverse", "Alternate"]) {
                            zz.loop_dir = [LoopDir::Forward, LoopDir::Reverse, LoopDir::Alternate][k];
                            ch = true;
                        }
                        ch |= toggle(ui, "Play to End", &mut zz.play_to_end);
                        let n = zz.frames() as f32;
                        let mut ls = zz.loop_start as f32;
                        let mut le = zz.loop_end as f32;
                        if field(ui, id.with("ls"), 64.0, &mut ls, 0.0, n, 0.0, 1.0, &|x| format!("{x:.0}")) {
                            zz.loop_start = ls as usize;
                            ch = true;
                        }
                        if field(ui, id.with("le"), 64.0, &mut le, 0.0, n, n, 1.0, &|x| format!("{x:.0}")) {
                            zz.loop_end = le as usize;
                            ch = true;
                        }
                        ch |= field(ui, id.with("lt"), 44.0, &mut zz.loop_tune, -50.0, 50.0, 0.0, 1.0, &|x| format!("{x:+.0}"));
                        let mut xf = zz.xfade as f32;
                        if field(ui, id.with("xf"), 56.0, &mut xf, 0.0, n / 2.0, 0.0, 1.0, &|x| format!("{x:.0}")) {
                            zz.xfade = xf as usize;
                            ch = true;
                        }
                        ch |= toggle(ui, "Eq. Pwr", &mut zz.eq_power);
                    });
                }
                if shown("Sample Start / End") {
                    let n = zz.frames() as f32;
                    let (mut s, mut e) = (zz.start as f32, zz.end as f32);
                    if field(ui, id.with("ss"), 70.0, &mut s, 0.0, n, 0.0, 1.0, &|x| format!("{x:.0}")) {
                        zz.start = s as usize;
                        ch = true;
                    }
                    if field(ui, id.with("se"), 70.0, &mut e, 0.0, n, n, 1.0, &|x| format!("{x:.0}")) {
                        zz.end = e as usize;
                        ch = true;
                    }
                }
                if shown("Fades") {
                    let (mut fi, mut fo) = (zz.fade_in as f32, zz.fade_out as f32);
                    let n = zz.frames() as f32;
                    if field(ui, id.with("fi"), 70.0, &mut fi, 0.0, n, 0.0, 1.0, &|x| format!("{x:.0}")) {
                        zz.fade_in = fi as usize;
                        ch = true;
                    }
                    if field(ui, id.with("fo"), 70.0, &mut fo, 0.0, n, 0.0, 1.0, &|x| format!("{x:.0}")) {
                        zz.fade_out = fo as usize;
                        ch = true;
                    }
                }
            });
            if ch {
                // edits on several selected zones move them all (relative)
                if sel && st.sel_zones.len() > 1 {
                    relative_edit(doc, &before, z, &st.sel_zones.clone());
                }
                if !st.protect_unselected {
                    doc.resolve_overlaps(&[z]);
                }
                *what = Some("Zone Parameter".into());
            }
        }
    });
}

/// After editing one of several selected zones: the others change by the same amounts.
fn relative_edit(doc: &mut SamplerDoc, before: &ZoneDoc, edited: usize, sel: &[usize]) {
    let after = doc.zones[edited].clone();
    let du = |a: u8, b: u8| b as i32 - a as i32;
    let (dr, dkl, dkh, dvl, dvh) = (du(before.root, after.root), du(before.key_lo, after.key_lo), du(before.key_hi, after.key_hi), du(before.vel_lo, after.vel_lo), du(before.vel_hi, after.vel_hi));
    let (dt, dvol, dpan, dsc) = (after.tune - before.tune, after.volume_db - before.volume_db, after.pan - before.pan, after.scale - before.scale);
    let add = |v: u8, d: i32, lo: i32| (v as i32 + d).clamp(lo, 127) as u8;
    for &o in sel {
        if o == edited || o >= doc.zones.len() {
            continue;
        }
        let z = &mut doc.zones[o];
        z.root = add(z.root, dr, 0);
        z.key_lo = add(z.key_lo, dkl, 0);
        z.key_hi = add(z.key_hi, dkh, 0).max(z.key_lo);
        z.vel_lo = add(z.vel_lo, dvl, 1);
        z.vel_hi = add(z.vel_hi, dvh, 1).max(z.vel_lo);
        z.tune += dt;
        z.volume_db = (z.volume_db + dvol).clamp(-96.0, 24.0);
        z.pan = (z.pan + dpan).clamp(-1.0, 1.0);
        z.scale = (z.scale + dsc).clamp(-100.0, 100.0);
        // switches are set the same on all
        if before.mute != after.mute {
            z.mute = after.mute;
        }
        if before.pitch != after.pitch {
            z.pitch = after.pitch;
        }
        if before.one_shot != after.one_shot {
            z.one_shot = after.one_shot;
        }
        if before.reverse != after.reverse {
            z.reverse = after.reverse;
        }
        if before.loop_on != after.loop_on {
            z.loop_on = after.loop_on;
        }
        if before.loop_dir != after.loop_dir {
            z.loop_dir = after.loop_dir;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Zone pane: the waveform and markers

#[allow(clippy::too_many_arguments)]
fn zone_pane(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, acts: &mut Vec<Act>, what: &mut Option<String>, dropped: &[PathBuf], drop_pos: Option<egui::Pos2>) {
    let area = ui.max_rect();
    ui.painter().rect_filled(area, 0.0, look::pane_bg());
    let Some(z) = st.focus_zone.or(st.sel_zones.first().copied()).filter(|&z| z < doc.zones.len()) else {
        ui.painter().text(area.center(), Align2::CENTER_CENTER, "Select a zone, or drag an audio file here", look::regular(13.0), look::text_dim());
        if !dropped.is_empty() && drop_pos.is_some_and(|p| area.contains(p)) {
            let zs = doc.add_files(&dropped[..1], Drop::Key(60));
            st.sel_zones = zs.clone();
            st.focus_zone = zs.first().copied();
            *what = Some("Add Zone".into());
        }
        return;
    };
    // the bar: Zone and File pop-ups, Edit, View, Preview, Snap, zoom, the switches
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        let zname = doc.zones[z].name.clone();
        ui.menu_button(format!("{zname} ⌄"), |ui| {
            for k in zone_order(doc, st) {
                if ui.add(egui::Button::selectable(k == z, &doc.zones[k].name)).clicked() {
                    st.focus_zone = Some(k);
                    st.sel_zones = vec![k];
                    ui.close();
                }
            }
        });
        let fname = doc.zones[z].file_name();
        ui.menu_button(if fname.is_empty() { "No Audio File ⌄".to_string() } else { format!("{fname} ⌄") }, |ui| {
            if ui.button("Load Audio File").clicked() {
                acts.push(Act::LoadInto(z));
                ui.close();
            }
            if ui.button("Rename Current File").clicked() {
                st.dialog = Some(Dialog::Rename { zone: z, name: fname.clone() });
                ui.close();
            }
            if ui.button("Detach Current File").clicked() {
                doc.detach(z);
                *what = Some("Detach".into());
                ui.close();
            }
            if ui.button("Show in Finder").clicked() {
                acts.push(Act::ShowInFolder(z));
                ui.close();
            }
            if ui.button("Open in Audio File Editor").clicked() {
                acts.push(Act::OpenInEditor(z));
                ui.close();
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui.button("New").clicked() {
                let nz = doc.new_zone(Some(doc.zones[z].group));
                st.focus_zone = Some(nz);
                st.sel_zones = vec![nz];
                *what = Some("New Zone".into());
                ui.close();
            }
            ui.menu_button("Loop", |ui| loop_menu(ui, doc, st, what));
            wave_commands(ui, doc, st, z, what);
        });
        ui.menu_button("View", |ui| {
            for (k, n) in ["Display Sample Units", "Display Seconds"].iter().enumerate() {
                if ui.add(egui::Button::selectable(st.units == k as u8, *n)).clicked() {
                    st.units = k as u8;
                    ui.close();
                }
            }
            ui.separator();
            if ui.add(egui::Button::selectable(!st.mono_sum, "Display Stereo Channels")).clicked() {
                st.mono_sum = false;
                ui.close();
            }
            if ui.add(egui::Button::selectable(st.mono_sum, "Display Mono Sum")).clicked() {
                st.mono_sum = true;
                ui.close();
            }
            ui.separator();
            ui.checkbox(&mut st.show_anchor, "Show Anchor");
        });
        if ui.button("▶ Preview").clicked() {
            acts.push(Act::PreviewZone);
        }
        ui.menu_button(if st.snap { "Snap: Zero Crossing" } else { "Snap: Off" }, |ui| {
            if ui.add(egui::Button::selectable(!st.snap, "Off")).clicked() {
                st.snap = false;
                ui.close();
            }
            if ui.add(egui::Button::selectable(st.snap, "Zero Crossing")).clicked() {
                st.snap = true;
                ui.close();
            }
        });
        if ui.button("↕").on_hover_text("Zoom vertical (maximum / default)").clicked() {
            st.wave_vmax = !st.wave_vmax;
        }
        if ui.button("↔").on_hover_text("Zoom horizontal (sample start to end / last zoom)").clicked() {
            let zz = &doc.zones[z];
            st.wave_view = match st.wave_view {
                Some(_) => None,
                None => Some((zz.start as f64, zz.end.max(zz.start + 1) as f64)),
            };
        }
        let zz = &mut doc.zones[z];
        let mut ch = false;
        ch |= toggle(ui, "One Shot", &mut zz.one_shot);
        ch |= toggle(ui, "Reverse", &mut zz.reverse);
        ui.add_enabled(false, egui::Button::new("Flex")).on_disabled_hover_text("Flex (constant speed at every pitch) isn't in Studio's Sampler yet");
        ui.add_enabled_ui(!zz.one_shot, |ui| {
            let modes = ["No Loop", "Forward", "Reverse", "Alternate"];
            let cur = if zz.loop_on { 1 + zz.loop_dir as usize } else { 0 };
            if let Some(k) = choice(ui, ui.scope_id().with("zp-loopmode"), 84.0, cur, &modes) {
                zz.loop_on = k > 0;
                if k > 0 {
                    zz.loop_dir = [LoopDir::Forward, LoopDir::Reverse, LoopDir::Alternate][k - 1];
                    if zz.loop_end <= zz.loop_start {
                        zz.loop_start = zz.start;
                        zz.loop_end = zz.end;
                    }
                }
                ch = true;
            }
            ch |= toggle(ui, "Play to End on Release", &mut zz.play_to_end);
        });
        if ch {
            *what = Some("Zone Parameter".into());
        }
    });
    // the waveform
    let rest = ui.available_rect_before_wrap();
    let wave = Rect::from_min_max(rest.min + vec2(6.0, 2.0), pos2(rest.max.x - 6.0, rest.max.y - 48.0));
    let p = ui.painter_at(wave);
    p.rect_filled(wave, 4.0, look::lane_bg());
    if !dropped.is_empty() && drop_pos.is_some_and(|q| wave.contains(q)) {
        let _ = doc.load_into(z, &dropped[0]);
        *what = Some("Load Audio File".into());
    }
    let zz = doc.zones[z].clone();
    let n = zz.frames();
    if n == 0 {
        p.text(wave.center(), Align2::CENTER_CENTER, "Drag an audio file here", look::regular(13.0), look::text_dim());
        let resp = ui.interact(wave, ui.scope_id().with("zp-empty"), Sense::click());
        if resp.clicked() {
            acts.push(Act::LoadInto(z));
        }
        return;
    }
    let (v0, v1) = st.wave_view.unwrap_or((0.0, n as f64));
    let (v0, v1) = (v0.clamp(0.0, n as f64), v1.clamp(v0 + 16.0, n as f64));
    let xof = |f: f64| wave.min.x + ((f - v0) / (v1 - v0)) as f32 * wave.width();
    let fof = |x: f32| (v0 + (x - wave.min.x) as f64 / wave.width() as f64 * (v1 - v0)).clamp(0.0, n as f64) as usize;
    // peaks per pixel column (cached)
    let w = wave.width() as usize;
    let data = zz.audio.clone().unwrap_or_else(|| std::sync::Arc::new(lg_synth::sampler::SampleData { rate: 48000.0, ch: Vec::new() }));
    let key = (std::sync::Arc::as_ptr(&data) as usize, v0 as i64, v1 as i64, w, st.mono_sum);
    let fresh = st.peaks.as_ref().is_none_or(|c| (c.0, c.1, c.2, c.3, c.4) != key);
    if fresh {
        let at = |c: &lg_synth::sampler::Pcm, i: usize| -> f32 {
            match c {
                lg_synth::sampler::Pcm::F32(v) => v.get(i).copied().unwrap_or(0.0),
                lg_synth::sampler::Pcm::I16(v) => v.get(i).map(|&x| x as f32 / 32768.0).unwrap_or(0.0),
            }
        };
        let cols: Vec<[(f32, f32); 2]> = (0..w).map(|x| {
            let a = (v0 + x as f64 / w as f64 * (v1 - v0)) as usize;
            let b = ((v0 + (x + 1) as f64 / w as f64 * (v1 - v0)) as usize).max(a + 1).min(n);
            let step = ((b - a) / 64).max(1);
            let mut out = [(0.0f32, 0.0f32); 2];
            for (ci, c) in data.ch.iter().take(2).enumerate() {
                let (mut lo, mut hi) = (0.0f32, 0.0f32);
                for i in (a..b).step_by(step) {
                    let s = if st.mono_sum { data.ch.iter().map(|c| at(c, i)).sum::<f32>() / data.ch.len() as f32 } else { at(c, i) };
                    lo = lo.min(s);
                    hi = hi.max(s);
                }
                out[ci] = (lo, hi);
            }
            out
        }).collect();
        st.peaks = Some((key.0, key.1, key.2, key.3, key.4, cols));
    }
    let chans = if st.mono_sum { 1 } else { data.ch.len().clamp(1, 2) };
    let gain = if st.wave_vmax {
        let pk = st.peaks.as_ref().map(|c| c.5.iter().map(|x| x[0].0.abs().max(x[0].1.abs())).fold(0.0f32, f32::max)).unwrap_or(1.0);
        1.0 / pk.max(1e-3)
    } else {
        1.0
    };
    if let Some(c) = &st.peaks {
        for ch in 0..chans {
            let lane_h = wave.height() / chans as f32;
            let mid = wave.min.y + lane_h * (ch as f32 + 0.5);
            for (x, col) in c.5.iter().enumerate() {
                let (lo, hi) = col[ch];
                let xx = wave.min.x + x as f32;
                p.vline(xx, (mid - hi * gain * lane_h * 0.48)..=(mid - lo * gain * lane_h * 0.48 + 0.5), Stroke::new(1.0, look::region_color(lg_core::TrackKind::Audio)));
            }
        }
    }
    // shaded areas and markers
    let blue = Color32::from_rgb(0x3c, 0x9c, 0xf0);
    let gray = Color32::from_gray(0xa0);
    let yellow = Color32::from_rgb(0xf5, 0xc5, 0x18);
    p.rect_filled(Rect::from_min_max(wave.min, pos2(xof(zz.start as f64), wave.max.y)), 0.0, Color32::from_black_alpha(110));
    p.rect_filled(Rect::from_min_max(pos2(xof(zz.end as f64), wave.min.y), wave.max), 0.0, Color32::from_black_alpha(110));
    let loop_live = zz.loop_on && !zz.one_shot;
    if loop_live {
        p.rect_filled(Rect::from_min_max(pos2(xof(zz.loop_start as f64), wave.min.y), pos2(xof(zz.loop_end as f64), wave.max.y)), 0.0, yellow.gamma_multiply(0.15));
    }
    let mut markers: Vec<(u8, f64, Color32, &str)> = vec![(0, zz.start as f64, blue, "S"), (1, zz.end as f64, blue, "E"), (2, (zz.start + zz.fade_in) as f64, gray, "◢"), (3, zz.end.saturating_sub(zz.fade_out) as f64, gray, "◣")];
    if loop_live {
        markers.push((4, zz.loop_start as f64, yellow, "LS"));
        markers.push((5, zz.loop_end as f64, yellow, "LE"));
        markers.push((6, zz.loop_end.saturating_sub(zz.xfade) as f64, yellow, "X"));
    }
    if st.show_anchor {
        markers.push((7, zz.anchor as f64, Color32::from_rgb(0x9c, 0xdc, 0xff), "A"));
    }
    for (_, f, c, l) in &markers {
        let x = xof(*f);
        p.vline(x, wave.y_range(), Stroke::new(1.5, *c));
        p.text(pos2(x + 2.0, wave.min.y + 2.0), Align2::LEFT_TOP, *l, look::regular(9.5), *c);
    }
    let resp = ui.interact(wave, ui.scope_id().with("zp-wave"), Sense::click_and_drag());
    let hit_marker = |x: f32| markers.iter().filter(|(_, f, _, _)| (xof(*f) - x).abs() < 5.0).min_by(|a, b| (xof(a.1) - x).abs().partial_cmp(&(xof(b.1) - x).abs()).unwrap_or(std::cmp::Ordering::Equal)).map(|m| m.0);
    if let Some(pp) = resp.hover_pos()
        && hit_marker(pp.x).is_some()
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let m = ui.input(|i| i.modifiers);
    if resp.drag_started()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let x0 = pp.x - resp.drag_delta().x;
        st.wdrag = if m.ctrl && m.alt {
            Some(WDrag::Zoom { x0 })
        } else if let Some(k) = hit_marker(x0) {
            Some(WDrag::Marker { which: k, orig: zz.clone(), x0, both: m.alt })
        } else if loop_live && x0 > xof(zz.loop_start as f64) && x0 < xof(zz.loop_end as f64) {
            Some(WDrag::LoopArea { orig: zz.clone(), x0 })
        } else {
            None
        };
    }
    let snap = |doc: &SamplerDoc, f: usize| -> usize {
        if !st.snap {
            return f;
        }
        let Some(d) = doc.zones[z].audio.as_ref() else { return f };
        let Some(c) = d.ch.first() else { return f };
        let at = |i: usize| match c {
            lg_synth::sampler::Pcm::F32(v) => v.get(i).copied().unwrap_or(0.0),
            lg_synth::sampler::Pcm::I16(v) => v.get(i).map(|&x| x as f32 / 32768.0).unwrap_or(0.0),
        };
        (0..2000).flat_map(|k| [f.saturating_sub(k), f + k]).find(|&i| i > 0 && at(i - 1) < 0.0 && at(i) >= 0.0).unwrap_or(f)
    };
    if resp.dragged()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        match st.wdrag.clone() {
            Some(WDrag::Marker { which, orig, x0, both }) => {
                let f = snap(doc, fof(pp.x));
                let df = f as i64 - fof(x0) as i64;
                let zz = &mut doc.zones[z];
                let mv = |v: usize| (v as i64 + df).clamp(0, n as i64) as usize;
                match which {
                    0 if both => {
                        zz.start = mv(orig.start);
                        zz.end = mv(orig.end).max(zz.start + 1);
                    }
                    1 if both => {
                        zz.start = mv(orig.start);
                        zz.end = mv(orig.end).max(zz.start + 1);
                    }
                    0 => zz.start = f.min(zz.end.saturating_sub(1)),
                    1 => zz.end = f.max(zz.start + 1),
                    2 | 3 if both => {
                        zz.fade_in = (orig.fade_in as i64 + df).clamp(0, (zz.end - zz.start) as i64) as usize;
                        zz.fade_out = (orig.fade_out as i64 - df).clamp(0, (zz.end - zz.start) as i64) as usize;
                    }
                    // fades stay inside the start and end markers
                    2 => zz.fade_in = f.clamp(zz.start, zz.end) - zz.start,
                    3 => zz.fade_out = zz.end - f.clamp(zz.start, zz.end),
                    4 if both => {
                        let len = orig.loop_end - orig.loop_start;
                        zz.loop_start = mv(orig.loop_start).min(n - len.min(n));
                        zz.loop_end = zz.loop_start + len;
                    }
                    5 if both => {
                        let len = orig.loop_end - orig.loop_start;
                        zz.loop_end = mv(orig.loop_end).max(len);
                        zz.loop_start = zz.loop_end - len;
                    }
                    4 => zz.loop_start = f.min(zz.loop_end.saturating_sub(16)),
                    5 => zz.loop_end = f.max(zz.loop_start + 16),
                    6 => zz.xfade = zz.loop_end.saturating_sub(f).min((zz.loop_end - zz.loop_start) / 2),
                    _ => zz.anchor = f,
                }
                *what = Some("Move Marker".into());
            }
            Some(WDrag::LoopArea { orig, x0 }) => {
                let df = fof(pp.x) as i64 - fof(x0) as i64;
                let len = orig.loop_end - orig.loop_start;
                let zz = &mut doc.zones[z];
                zz.loop_start = (orig.loop_start as i64 + df).clamp(0, (n - len.min(n)) as i64) as usize;
                zz.loop_end = zz.loop_start + len;
                *what = Some("Move Loop".into());
            }
            Some(WDrag::Zoom { x0 }) => {
                p.rect_filled(Rect::from_min_max(pos2(x0.min(pp.x), wave.min.y), pos2(x0.max(pp.x), wave.max.y)), 0.0, look::accent().gamma_multiply(0.2));
            }
            None => {}
        }
    }
    if resp.drag_stopped() {
        if let Some(WDrag::Zoom { x0 }) = st.wdrag.clone()
            && let Some(pp) = resp.interact_pointer_pos()
        {
            let (a, b) = (fof(x0.min(pp.x)), fof(x0.max(pp.x)));
            if b > a + 16 {
                st.wave_view = Some((a as f64, b as f64));
            }
        }
        st.wdrag = None;
    }
    if resp.hovered() {
        // pinch / ⌘-scroll zooms, sideways scroll moves
        let (zoom, dx) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta.x));
        if zoom != 1.0 || dx != 0.0 {
            let span = (v1 - v0) / zoom as f64;
            let c = resp.hover_pos().map(|q| fof(q.x) as f64).unwrap_or((v0 + v1) / 2.0);
            let shift = -dx as f64 / wave.width() as f64 * span;
            let a = (c - (c - v0) / zoom as f64 + shift).max(0.0);
            st.wave_view = Some((a, (a + span).min(n as f64)));
        }
    }
    resp.context_menu(|ui| {
        let in_loop = loop_live;
        if in_loop {
            if ui.button("Auto-Loop within Loop Area").clicked() {
                if doc.auto_loop(z, st.autoloop_attempt, true) {
                    st.autoloop_attempt += 1;
                    *what = Some("Auto-Loop".into());
                }
                ui.close();
            }
        }
        if ui.button("Auto-Loop").clicked() {
            if doc.auto_loop(z, st.autoloop_attempt, false) {
                st.autoloop_attempt += 1;
                *what = Some("Auto-Loop".into());
            }
            ui.close();
        }
        wave_commands(ui, doc, st, z, what);
        if ui.button("Open in Audio File Editor").clicked() {
            acts.push(Act::OpenInEditor(z));
            ui.close();
        }
    });
    // the fields under the waveform
    let fr = Rect::from_min_max(pos2(rest.min.x + 6.0, wave.max.y + 4.0), rest.max);
    let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(fr));
    let rate = doc.zones[z].rate();
    let units = st.units;
    let shown = move |x: f32| if units == 1 { format!("{:.3} s", x / rate) } else { format!("{x:.0}") };
    cell.horizontal_wrapped(|ui| {
        let zz = &mut doc.zones[z];
        let nf = n as f32;
        let mut ch = false;
        let f = |ui: &mut egui::Ui, label: &str, v: &mut usize, max: f32, key: &str| -> bool {
            ui.label(egui::RichText::new(label).size(11.0).color(look::text_dim()));
            let mut x = *v as f32;
            if field(ui, ui.scope_id().with(("zpf", key)), 72.0, &mut x, 0.0, max, 0.0, 1.0, &shown) {
                *v = x as usize;
                return true;
            }
            false
        };
        ch |= f(ui, "Sample Start", &mut zz.start, nf, "ss");
        ch |= f(ui, "End", &mut zz.end, nf, "se");
        let mut len = zz.end.saturating_sub(zz.start);
        if f(ui, "Length", &mut len, nf, "sl") {
            zz.end = (zz.start + len).min(n);
            ch = true;
        }
        ch |= f(ui, "Fade In", &mut zz.fade_in, nf, "fi");
        ch |= f(ui, "Fade Out", &mut zz.fade_out, nf, "fo");
        if loop_live {
            ch |= f(ui, "Loop Start", &mut zz.loop_start, nf, "ls");
            ch |= f(ui, "Loop End", &mut zz.loop_end, nf, "le");
            let mut ll = zz.loop_end.saturating_sub(zz.loop_start);
            if f(ui, "Loop Length", &mut ll, nf, "ll") {
                zz.loop_end = (zz.loop_start + ll).min(n);
                ch = true;
            }
            ch |= f(ui, "Crossfade", &mut zz.xfade, nf / 2.0, "xf");
            ch |= toggle(ui, "Equal Power", &mut zz.eq_power);
        }
        if ch {
            zz.fade_in = zz.fade_in.min(zz.end.saturating_sub(zz.start));
            zz.fade_out = zz.fade_out.min(zz.end.saturating_sub(zz.start));
            *what = Some("Zone Parameter".into());
        }
    });
}

fn wave_commands(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, z: usize, what: &mut Option<String>) {
    if ui.button("Retune").clicked() {
        doc.retune(&[z]);
        *what = Some("Retune".into());
        ui.close();
    }
    if ui.button("Optimize Sample Gain").clicked() {
        doc.normalize_loudness(&[z]);
        *what = Some("Optimize Sample Gain".into());
        ui.close();
    }
    if ui.button("Crop Sample").clicked() {
        match doc.crop(z, false, &samples_dir().join("Cropped")) {
            Ok(_) => *what = Some("Crop Sample".into()),
            Err(e) => eprintln!("crop: {e}"),
        }
        ui.close();
    }
    if ui.button("Crop Loop").clicked() {
        match doc.crop(z, true, &samples_dir().join("Cropped")) {
            Ok(_) => *what = Some("Crop Loop".into()),
            Err(e) => eprintln!("crop: {e}"),
        }
        ui.close();
    }
    let _ = st;
}

// ---------------------------------------------------------------------------------------------
// Dialogs

fn dialogs(ui: &mut egui::Ui, doc: &mut SamplerDoc, st: &mut Ui, acts: &mut Vec<Act>, what: &mut Option<String>) {
    let Some(d) = st.dialog.clone() else { return };
    let mut keep = true;
    let mut next = d.clone();
    let title = match &d {
        Dialog::RemapNotes { .. } => "Remap Notes",
        Dialog::RemapVelocities { .. } => "Remap Velocities",
        Dialog::KeyLabels => "Key Labels",
        Dialog::OutputLabels => "Output Labels",
        Dialog::SaveAs { .. } => "Save Instrument As",
        Dialog::Rename { .. } => "Rename",
        Dialog::RenameGroup { .. } => "Rename Group",
    };
    egui::Window::new(title).id(egui::Id::unique("sampler-dialog")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ui.ctx(), |ui| {
        match &mut next {
            Dialog::RemapNotes { start, width } => {
                ui.horizontal(|ui| {
                    ui.label("Start Note");
                    field(ui, ui.scope_id().with("rn-s"), 60.0, start, 0.0, 127.0, 48.0, 1.0, &|x| sd::note_name(x as u8));
                    ui.label("Width");
                    field(ui, ui.scope_id().with("rn-w"), 40.0, width, 1.0, 64.0, 1.0, 1.0, &|x| format!("{x:.0}"));
                });
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                    if ui.button("OK").clicked() {
                        doc.remap_notes(&st.sel_zones, *start as u8, *width as u8);
                        *what = Some("Remap Notes".into());
                        keep = false;
                    }
                });
            }
            Dialog::RemapVelocities { lo, hi } => {
                ui.horizontal(|ui| {
                    ui.label("Low");
                    field(ui, ui.scope_id().with("rv-l"), 40.0, lo, 1.0, 127.0, 1.0, 1.0, &|x| format!("{x:.0}"));
                    ui.label("High");
                    field(ui, ui.scope_id().with("rv-h"), 40.0, hi, 1.0, 127.0, 127.0, 1.0, &|x| format!("{x:.0}"));
                });
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                    if ui.button("OK").clicked() {
                        doc.remap_velocities(&st.sel_zones, *lo as u8, *hi as u8);
                        *what = Some("Remap Velocities".into());
                        keep = false;
                    }
                });
            }
            Dialog::KeyLabels => {
                key_label_sets(ui, doc, what, true);
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    egui::Grid::new("key-labels").num_columns(2).show(ui, |ui| {
                        for k in (0..128u8).rev() {
                            ui.label(sd::note_name(k));
                            let mut s = doc.key_labels.get(&k).cloned().unwrap_or_default();
                            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(160.0)).lost_focus() || s != doc.key_labels.get(&k).cloned().unwrap_or_default() {
                                if s.is_empty() {
                                    doc.key_labels.remove(&k);
                                } else {
                                    doc.key_labels.insert(k, s);
                                }
                                *what = Some("Edit Key Labels".into());
                            }
                            ui.end_row();
                        }
                    });
                });
                if ui.button("Done").clicked() {
                    keep = false;
                }
            }
            Dialog::OutputLabels => {
                key_label_sets(ui, doc, what, false);
                egui::Grid::new("out-labels").num_columns(2).show(ui, |ui| {
                    for k in 0..16u8 {
                        ui.label(if k == 0 { "Main".to_string() } else { format!("{}-{}", k * 2 + 1, k * 2 + 2) });
                        let mut s = doc.output_labels.get(&k).cloned().unwrap_or_default();
                        if ui.add(egui::TextEdit::singleline(&mut s).desired_width(160.0)).changed() {
                            if s.is_empty() {
                                doc.output_labels.remove(&k);
                            } else {
                                doc.output_labels.insert(k, s);
                            }
                            *what = Some("Edit Output Labels".into());
                        }
                        ui.end_row();
                    }
                });
                if ui.button("Done").clicked() {
                    keep = false;
                }
            }
            Dialog::SaveAs { name, copy, consolidated, a_copy } => {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(name);
                });
                ui.label(egui::RichText::new(format!("In {}", instruments_dir().display())).size(11.0).color(look::text_dim()));
                ui.checkbox(copy, "Save with audio data");
                ui.add_enabled_ui(*copy, |ui| {
                    ui.add_enabled(false, egui::Checkbox::new(consolidated, "Consolidated")).on_disabled_hover_text("Consolidating samples into CAF files isn't in Studio yet");
                });
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                    if ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Save")).clicked() {
                        acts.push(Act::SaveAs { name: name.trim().to_string(), copy: *copy, a_copy: *a_copy });
                        keep = false;
                    }
                });
            }
            Dialog::Rename { zone, name } => {
                let te = ui.text_edit_singleline(name);
                crate::widgets::focus_once(ui, &te);
                if te.lost_focus() || ui.button("OK").clicked() {
                    if let Some(z) = doc.zones.get_mut(*zone) {
                        z.name = name.clone();
                    }
                    *what = Some("Rename Zone".into());
                    keep = false;
                }
            }
            Dialog::RenameGroup { group, name } => {
                let te = ui.text_edit_singleline(name);
                crate::widgets::focus_once(ui, &te);
                if te.lost_focus() || ui.button("OK").clicked() {
                    if let Some(g) = doc.groups.get_mut(*group) {
                        g.name = name.clone();
                    }
                    *what = Some("Rename Group".into());
                    keep = false;
                }
            }
        }
    });
    st.dialog = if keep { Some(next) } else { None };
}

/// The Key Labels / Output Labels pop-up: saved label sets (Save … Set As, load, Delete, Reset).
fn key_label_sets(ui: &mut egui::Ui, doc: &mut SamplerDoc, what: &mut Option<String>, keys: bool) {
    let dir = instruments_dir().join(if keys { "Key Labels" } else { "Output Labels" });
    ui.horizontal(|ui| {
        ui.menu_button(if keys { "Key Labels ⌄" } else { "Output Labels ⌄" }, |ui| {
            let name_id = egui::Id::unique(("label-set-name", keys));
            let mut name: String = ui.data(|d| d.get_temp(name_id)).unwrap_or_default();
            ui.horizontal(|ui| {
                if ui.add(egui::TextEdit::singleline(&mut name).hint_text("Set name").desired_width(120.0)).changed() {
                    ui.data_mut(|d| d.insert_temp(name_id, name.clone()));
                }
                if ui.add_enabled(!name.trim().is_empty(), egui::Button::new(if keys { "Save Key Label Set As…" } else { "Save Label Set As…" })).clicked() {
                    let map = if keys { &doc.key_labels } else { &doc.output_labels };
                    let body: String = map.iter().map(|(k, v)| format!("{k}\t{v}\n")).collect();
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::fs::write(dir.join(format!("{}.txt", name.trim())), body);
                    ui.close();
                }
            });
            if !keys && ui.button("Reset Label Set").clicked() {
                doc.output_labels.clear();
                *what = Some("Reset Labels".into());
                ui.close();
            }
            let mut sets: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "txt")).collect();
            sets.sort();
            if !sets.is_empty() {
                ui.separator();
            }
            for s in &sets {
                let n = s.file_stem().map(|x| x.to_string_lossy().into_owned()).unwrap_or_default();
                ui.horizontal(|ui| {
                    if ui.button(&n).clicked() {
                        if let Ok(t) = std::fs::read_to_string(s) {
                            let map: std::collections::BTreeMap<u8, String> = t.lines().filter_map(|l| {
                                let (k, v) = l.split_once('\t')?;
                                Some((k.parse().ok()?, v.to_string()))
                            }).collect();
                            if keys {
                                doc.key_labels = map;
                            } else {
                                doc.output_labels = map;
                            }
                            *what = Some("Load Labels".into());
                        }
                        ui.close();
                    }
                    if ui.small_button("Delete").clicked() {
                        let _ = std::fs::remove_file(s);
                        ui.close();
                    }
                });
            }
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Key commands

pub fn command(app: &mut App, _ctx: &egui::Context, t: usize, name: &str) -> bool {
    let Some(id) = instwin::track_id(app, t) else { return false };
    if !app.inst.docs.contains_key(&id) {
        return false;
    }
    // commands that need files or the engine
    match name {
        "Load or Replace Audio File…" => {
            let z = app.inst.sampler.focus_zone.unwrap_or(usize::MAX);
            run_act(app, _ctx, t, Act::LoadInto(z));
            return true;
        }
        "Load Multiple Audio Files…" => {
            let g = app.inst.sampler.focus_group;
            run_act(app, _ctx, t, Act::LoadFiles(g));
            return true;
        }
        "Open in Audio File Editor" => {
            if let Some(z) = app.inst.sampler.focus_zone {
                run_act(app, _ctx, t, Act::OpenInEditor(z));
            }
            return true;
        }
        "Preview Selected Zone" => {
            run_act(app, _ctx, t, Act::PreviewZone);
            return true;
        }
        "Next Sampler Instrument" | "Next Plug-in Setting or Sampler Instrument" => {
            run_act(app, _ctx, t, Act::StepInstrument(1));
            return true;
        }
        "Previous Sampler Instrument" | "Previous Plug-in Setting or Sampler Instrument" => {
            run_act(app, _ctx, t, Act::StepInstrument(-1));
            return true;
        }
        _ => {}
    }
    let mut doc = app.inst.docs.get(&id).cloned().unwrap_or_default();
    let st = &mut app.inst.sampler;
    let sel = st.sel_zones.clone();
    let mut changed = true;
    match name {
        "New Zone" => {
            let z = doc.new_zone(st.focus_group.or(st.sel_groups.first().copied()));
            st.sel_zones = vec![z];
            st.focus_zone = Some(z);
        }
        "New Group" => {
            let g = doc.new_group();
            st.sel_groups = vec![g];
            st.focus_group = Some(g);
        }
        "Toggle Keyboard/Group/Zone View" => {
            st.view = match st.view {
                MapView::Keyboard => MapView::Group,
                MapView::Group => MapView::Zone,
                MapView::Zone => MapView::Keyboard,
            };
            changed = false;
        }
        "Switch to Keyboard View" => {
            st.view = MapView::Keyboard;
            changed = false;
        }
        "Switch to Group View" => {
            st.view = MapView::Group;
            changed = false;
        }
        "Switch to Zone View" => {
            st.view = MapView::Zone;
            changed = false;
        }
        "Show all List Columns" => {
            st.hidden_cols.clear();
            changed = false;
        }
        "Restore Default List Columns" => {
            st.hidden_cols = default_hidden();
            changed = false;
        }
        "Shift selected Zones/Groups Left" | "Shift selected Zones/Groups Right" | "Shift selected Zones/Groups Left (Zones incl. Root Key)" | "Shift selected Zones/Groups Right (Zones incl. Root Key)" => {
            let by = if name.contains("Left") { -1 } else { 1 };
            let with_root = name.contains("Root") || doc.move_root;
            if !sel.is_empty() {
                doc.shift_zones(&sel, by, with_root);
            } else {
                doc.shift_groups(&st.sel_groups.clone(), by, with_root);
            }
        }
        "Select Zone of Last Played Key" => {
            if let Some(k) = app.inst.last_played {
                let st = &mut app.inst.sampler;
                st.sel_zones = (0..doc.zones.len()).filter(|&z| doc.zones[z].key_lo <= k && doc.zones[z].key_hi >= k).collect();
                st.focus_zone = st.sel_zones.first().copied();
            }
            changed = false;
        }
        "New Mapping" => {
            let keep = (doc.name.clone(), doc.path.clone());
            doc = SamplerDoc::new();
            (doc.name, doc.path) = keep;
            st.sel_zones.clear();
            st.sel_groups = vec![0];
        }
        "Duplicate Group" => {
            let gs = doc.duplicate_groups(&st.sel_groups.clone());
            if !gs.is_empty() {
                st.sel_groups = gs;
            }
        }
        "Crop Sample" | "Crop Loop" => {
            let Some(z) = st.focus_zone else { return true };
            if let Err(e) = doc.crop(z, name == "Crop Loop", &samples_dir().join("Cropped")) {
                app.tell(format!("Couldn't crop: {e}"));
                return true;
            }
        }
        "Toggle Snap mode" => {
            st.snap = !st.snap;
            changed = false;
        }
        "Vertical Autozoom" => {
            st.wave_vmax = !st.wave_vmax;
            changed = false;
        }
        "Horizontal Autozoom" => {
            if let Some(z) = st.focus_zone.and_then(|z| doc.zones.get(z)) {
                st.wave_view = match st.wave_view {
                    Some(_) => None,
                    None => Some((z.start as f64, z.end.max(z.start + 1) as f64)),
                };
            }
            changed = false;
        }
        "Automap using Current Root Note" => doc.automap_current_root(&sel),
        "Automap using Root Note from Audio File Names" => doc.automap_from_names(&sel),
        "Remap All Notes" => doc.remap_all_notes(&sel),
        "Remap Evenly" => doc.remap_evenly(&sel),
        "Remap To Round Robin" => doc.remap_to_round_robin(&sel),
        "Remap Notes…" => {
            st.dialog = Some(Dialog::RemapNotes { start: 48.0, width: 1.0 });
            changed = false;
        }
        "Remap To Root Notes" => doc.remap_to_root(&sel),
        "Fill Gaps" => doc.fill_gaps(&sel, true, true),
        "Fill Gaps (Notes Only)" => doc.fill_gaps(&sel, true, false),
        "Fill Gaps (Vel Only)" => doc.fill_gaps(&sel, false, true),
        "Swap Zones" => {
            if sel.len() == 2 {
                doc.swap(sel[0], sel[1]);
            } else {
                changed = false;
            }
        }
        "Always Move Root Key with Zone" => doc.move_root = !doc.move_root,
        _ => return false,
    }
    if changed {
        instwin::checkpoint(app, t, name);
        app.inst.docs.insert(id, doc);
        instwin::bump(app, t);
    }
    true
}

/// Keys of the editor that aren't key commands: arrows select the previous / next zone or
/// group, Delete removes the selection.
pub fn plain_key(app: &mut App, t: usize, key: egui::Key, m: egui::Modifiers) -> bool {
    let Some(id) = instwin::track_id(app, t) else { return false };
    let Some(doc) = app.inst.docs.get(&id).cloned() else { return false };
    if m.any() && !m.shift {
        return false;
    }
    match key {
        egui::Key::ArrowUp | egui::Key::ArrowDown => {
            let st = &mut app.inst.sampler;
            let dir: i32 = if key == egui::Key::ArrowDown { 1 } else { -1 };
            if st.view == MapView::Group || doc.zones.is_empty() {
                let g = st.focus_group.unwrap_or(0) as i32 + dir;
                if g >= 0 && (g as usize) < doc.groups.len() {
                    st.sel_groups = vec![g as usize];
                    st.focus_group = Some(g as usize);
                }
            } else {
                let order = zone_order(&doc, st);
                let at = st.focus_zone.and_then(|z| order.iter().position(|&x| x == z));
                let next = match at {
                    Some(i) => (i as i32 + dir).clamp(0, order.len() as i32 - 1) as usize,
                    None => 0,
                };
                if let Some(&z) = order.get(next) {
                    st.sel_zones = vec![z];
                    st.focus_zone = Some(z);
                }
            }
            true
        }
        egui::Key::Delete | egui::Key::Backspace => {
            let mut doc = doc;
            instwin::checkpoint(app, t, "Delete");
            delete_selection(&mut doc, &mut app.inst.sampler);
            app.inst.docs.insert(id, doc);
            instwin::bump(app, t);
            true
        }
        _ => false,
    }
}

/// A note played (MIDI keyboard, Musical Typing): Select from Last Played Keys.
pub fn note_played(app: &mut App, pitch: u8) {
    let Some(t) = app.plugin_window else { return };
    let Some(id) = instwin::track_id(app, t) else { return };
    let st = &mut app.inst.sampler;
    if st.select_from_played == 0 {
        return;
    }
    let Some(doc) = app.inst.docs.get(&id) else { return };
    let hits: Vec<usize> = (0..doc.zones.len()).filter(|&z| doc.zones[z].key_lo <= pitch && doc.zones[z].key_hi >= pitch).collect();
    let mut groups: Vec<usize> = hits.iter().map(|&z| doc.zones[z].group).collect();
    groups.sort_unstable();
    groups.dedup();
    if !groups.is_empty() {
        st.sel_groups = groups;
        st.focus_group = st.sel_groups.first().copied();
    }
    if st.select_from_played == 2 && !hits.is_empty() {
        st.sel_zones = hits;
        st.focus_zone = st.sel_zones.first().copied();
    }
}
