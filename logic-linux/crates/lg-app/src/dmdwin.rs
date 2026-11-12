//! The Drum Machine Designer window, as Logic's: the pad grid (16 pads a page, bottom left C1 up
//! to D#2, three pages for the 48 pads), each pad its sound's name, a click plays and selects it;
//! a sample dragged onto a pad goes into that pad's own Quick Sampler; the pad menu (Load Audio
//! File…, Rename, Copy / Paste, Clear, Mute, Solo, Resample, Assign To Exclusive Group, Preview);
//! below the grid the selected pad's sound: its drum (kit, source note, tune, level, pan, decay,
//! exclusive group) or its Quick Sampler (mode, markers, tuning, envelope, filter).
//! The Drum Machine Designer's 16 key commands work with the pointer over the window.
//!
//! In Logic every pad is a track of a summing track stack; here the pads live inside the one
//! instrument (their own mute, solo and exclusive groups), which a stack's subtracks don't show.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_synth::dmd::{self, DrumMachineDesigner, FIRST_PAD, PADS};

use crate::app::App;
use crate::instwin;
use crate::look;

#[derive(Clone, Default)]
pub struct PadDoc {
    pub name: String,
    pub file: Option<PathBuf>,
    pub audio: Option<Arc<lg_synth::wav::Audio>>,
    /// The pad's Quick Sampler settings (id, value).
    pub quick: Vec<(u32, f32)>,
}

#[derive(Clone, Default)]
pub struct Pads {
    pub pads: BTreeMap<u8, PadDoc>,
    pub muted: BTreeSet<u8>,
    pub soloed: BTreeSet<u8>,
}

pub struct Ui {
    pub selected: u8,
    pub page: usize,
    pub clip: Option<(PadDoc, Vec<(u32, f32)>)>,
    pub rename: Option<(u8, String)>,
    /// Kit Action: Select Pad by Key, Show GM Drum Names, Hide Kit Piece Icons.
    pub select_by_key: bool,
    pub gm_names: bool,
    pub hide_icons: bool,
    /// A pad being dragged onto another (they swap), and the pad held down by its speaker.
    pub drag_from: Option<u8>,
    pub held: Option<u8>,
    pub kit_rename: Option<String>,
}

impl Default for Ui {
    fn default() -> Self {
        Ui { selected: FIRST_PAD, page: 0, clip: None, rename: None, select_by_key: false, gm_names: false, hide_icons: false, drag_from: None, held: None, kit_rename: None }
    }
}

#[allow(dead_code)]
pub const COMMANDS: &[&str] = &["Select Pad by Key", "Show GM Drum Names", "Hide Kit Piece Icons", "Clear All Pads", "Update All Kit Names", "Cut Pad"];

/// General MIDI percussion names (the GM standard's list), for Show GM Drum Names.
pub fn gm_name(n: u8) -> &'static str {
    const N: [&str; 47] = ["Acoustic Bass Drum", "Bass Drum 1", "Side Stick", "Acoustic Snare", "Hand Clap", "Electric Snare", "Low Floor Tom", "Closed Hi-Hat", "High Floor Tom", "Pedal Hi-Hat", "Low Tom", "Open Hi-Hat", "Low-Mid Tom", "Hi-Mid Tom", "Crash Cymbal 1", "High Tom", "Ride Cymbal 1", "Chinese Cymbal", "Ride Bell", "Tambourine", "Splash Cymbal", "Cowbell", "Crash Cymbal 2", "Vibraslap", "Ride Cymbal 2", "Hi Bongo", "Low Bongo", "Mute Hi Conga", "Open Hi Conga", "Low Conga", "High Timbale", "Low Timbale", "High Agogo", "Low Agogo", "Cabasa", "Maracas", "Short Whistle", "Long Whistle", "Short Guiro", "Long Guiro", "Claves", "Hi Wood Block", "Low Wood Block", "Mute Cuica", "Open Cuica", "Mute Triangle", "Open Triangle"];
    if (35..82).contains(&n) { N[(n - 35) as usize] } else { "" }
}

/// Select Pad by Key: a played note selects its pad (Logic: live keys only, not regions).
pub fn note_played(app: &mut App, pitch: u8) {
    if app.inst.dmd.select_by_key && (FIRST_PAD..FIRST_PAD + PADS as u8).contains(&pitch) {
        app.inst.dmd.selected = pitch;
        app.inst.dmd.page = ((pitch - FIRST_PAD) / 16) as usize;
    }
}

/// The pads' samples, settings, mute and solo into the running instrument.
pub fn apply(p: &Pads, d: &mut DrumMachineDesigner) {
    for n in 0..128u8 {
        match p.pads.get(&n).and_then(|x| x.audio.as_ref().map(|a| (a, &x.quick))) {
            Some((a, q)) => {
                d.set_pad_sample(n, a);
                if let Some(qs) = d.pad_quick_mut(n) {
                    for &(id, v) in q {
                        lg_dsp::Plugin::set(qs, id, v);
                    }
                }
            }
            None => d.clear_pad_sample(n),
        }
        d.muted[n as usize] = p.muted.contains(&n);
        d.soloed[n as usize] = p.soloed.contains(&n);
    }
}

fn pad_name(app: &App, t: usize, pads: &Pads, note: u8) -> String {
    if let Some(n) = pads.pads.get(&note).map(|p| p.name.clone()).filter(|n| !n.is_empty()) {
        return n;
    }
    // the drum the pad plays: its piece and the kit
    let (kit, src) = instwin::values(app, t).map(|(p, v)| {
        let get = |id: u32| p.iter().position(|x| x.id == id).map(|i| v[i]).unwrap_or(0.0);
        let k = (note - FIRST_PAD) as usize;
        (get(dmd::pad_param(k, 0)).round() as usize, get(dmd::pad_param(k, 1)).round() as u8)
    }).unwrap_or((0, note));
    let piece = match lg_synth::drums::piece_of(src) {
        Some(lg_synth::drums::Group::Kick) => "Kick",
        Some(lg_synth::drums::Group::Snare) => if src == 39 { "Clap" } else if src == 37 { "Rim" } else { "Snare" },
        Some(lg_synth::drums::Group::Tom) => "Tom",
        Some(lg_synth::drums::Group::HiHat) => "Hi-Hat",
        Some(lg_synth::drums::Group::Cymbal) => if src == 51 || src == 59 { "Ride" } else { "Crash" },
        _ => "Perc",
    };
    let kit = lg_synth::drums::KITS.get(kit).copied().unwrap_or("");
    format!("{piece} ({kit})")
}

fn edit(app: &mut App, t: usize, what: &str, f: impl FnOnce(&mut Pads)) {
    let Some(id) = instwin::track_id(app, t) else { return };
    instwin::checkpoint(app, t, what);
    f(app.inst.pads.entry(id).or_default());
    instwin::bump(app, t);
}

fn preview(app: &mut App, t: usize, note: u8) {
    let engine = app.engine.clone();
    if let Ok(mut e) = engine.lock() {
        e.live(t, note, 110);
    }
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        if let Ok(mut e) = engine.lock() {
            e.live(t, note, 0);
        }
    });
}

fn load_file(app: &mut App, t: usize, note: u8, path: &std::path::Path) {
    match lg_synth::wav::read_audio(path) {
        Ok(a) => {
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            edit(app, t, "Load Pad Sample", |p| {
                p.pads.insert(note, PadDoc { name, file: Some(path.to_path_buf()), audio: Some(Arc::new(a)), quick: Vec::new() });
            });
        }
        Err(e) => app.tell(format!("Couldn't load {}: {e}", path.display())),
    }
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some(id) = instwin::track_id(app, t) else { return };
    let pads = app.inst.pads.get(&id).cloned().unwrap_or_default();
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let get = |pid: u32| params.iter().position(|x| x.id == pid).map(|i| vals[i]).unwrap_or(0.0);
    let track = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    let (track_mute, track_solo) = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| (tr.mute, tr.solo)).unwrap_or_default();
    let names: Vec<String> = (FIRST_PAD..FIRST_PAD + PADS as u8).map(|n| pad_name(app, t, &pads, n)).collect();
    let dropped = std::mem::take(&mut app.inst.dropped);
    let drop_pos = app.inst.drop_pos;
    let mut st = std::mem::take(&mut app.inst.dmd);
    let mut open = true;
    let mut set: Vec<(u32, f32)> = Vec::new();
    enum A {
        Play(u8),
        Load(u8),
        Files(u8, Vec<PathBuf>),
        Cmd(&'static str),
        Paste(u8),
        Rename(u8, String),
        QuickSet(u8, u32, f32),
        Swap(u8, u8),
        Stop(u8),
        StopAll,
        TrackMute,
        TrackSolo,
        RenameKit(String),
    }
    let mut acts: Vec<A> = Vec::new();
    let qvals: Option<(Vec<lg_dsp::Param>, Vec<f32>)> = instwin::with_inst(app, t, |d: &mut DrumMachineDesigner| d.pad_quick(st.selected).map(|q| {
        use lg_synth::quick as qk;
        let shown = [qk::MODE, qk::ROOT, qk::START, qk::END, qk::LOOP_MODE, qk::TUNE, qk::REVERSE, qk::FADE_IN, qk::FADE_OUT, qk::VOLUME, qk::FILTER_ON, qk::CUTOFF, qk::RESO, lg_synth::sampler_synth::ENV1 + 2, lg_synth::sampler_synth::ENV1 + 5, lg_synth::sampler_synth::ENV1 + 6, lg_synth::sampler_synth::ENV1 + 7];
        let p: Vec<lg_dsp::Param> = lg_dsp::Plugin::params(q).iter().filter(|x| shown.contains(&x.id)).cloned().collect();
        let v = p.iter().map(|x| lg_dsp::Plugin::get(q, x.id)).collect();
        (p, v)
    })).flatten();
    let resp = egui::Window::new(format!("{track} — Drum Machine Designer")).id(egui::Id::unique(("dmd-window", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(760.0, 720.0)).show(ctx, |ui| {
        // the kit controls bar: the kit icon (a click stops every sound), the kit's name
        // (double-click renames the kit and its track), Mute, Solo and the Action pop-up
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(crate::glyphs::ph::SPEAKER_SIMPLE_X).frame(false)).on_hover_text("Stop all kit pieces").clicked() {
                acts.push(A::StopAll);
            }
            match st.kit_rename.as_mut() {
                Some(n) => {
                    let te = ui.add(egui::TextEdit::singleline(n).desired_width(160.0));
                    crate::widgets::focus_once(ui, &te);
                    if te.lost_focus() {
                        acts.push(A::RenameKit(n.clone()));
                    }
                }
                None => {
                    if ui.add(egui::Label::new(egui::RichText::new(&track).strong()).sense(Sense::click())).on_hover_text("Double-click to rename the kit and its track").double_clicked() {
                        st.kit_rename = Some(track.clone());
                    }
                }
            }
            if ui.add(egui::Button::selectable(track_mute, "M")).on_hover_text("Mute the kit (main track)").clicked() {
                acts.push(A::TrackMute);
            }
            if ui.add(egui::Button::selectable(track_solo, "S")).on_hover_text("Solo the kit").clicked() {
                acts.push(A::TrackSolo);
            }
            ui.menu_button("…", |ui| {
                ui.set_min_width(240.0);
                if ui.add(egui::Button::selectable(st.select_by_key, "Select Pad by Key")).clicked() {
                    st.select_by_key = !st.select_by_key;
                    ui.close();
                }
                if ui.button(if st.gm_names { "Hide GM Drum Names" } else { "Show GM Drum Names" }).clicked() {
                    st.gm_names = !st.gm_names;
                    ui.close();
                }
                if ui.button(if st.hide_icons { "Show Kit Piece Icons" } else { "Hide Kit Piece Icons" }).clicked() {
                    st.hide_icons = !st.hide_icons;
                    ui.close();
                }
                ui.separator();
                for c in ["Update All Kit Names", "Clear All Pads"] {
                    if ui.button(c).clicked() {
                        acts.push(A::Cmd(c));
                        ui.close();
                    }
                }
            });
            ui.separator();
            ui.label(egui::RichText::new("Kit").color(look::text_dim()));
            let cur = get(2).round() as usize;
            ui.menu_button(format!("{} ⌄", dmd::STYLES.get(cur).copied().unwrap_or("")), |ui| {
                for (k, s) in dmd::STYLES.iter().enumerate() {
                    if ui.add(egui::Button::selectable(k == cur, *s)).clicked() {
                        set.push((2, k as f32));
                        ui.close();
                    }
                }
            });
            for (pid, label, min, max) in [(0u32, "Volume", -40.0f32, 12.0f32), (1, "Room", 0.0, 1.0)] {
                ui.label(egui::RichText::new(label).color(look::text_dim()));
                let mut v = get(pid);
                if ui.add(egui::Slider::new(&mut v, min..=max).show_value(true)).changed() {
                    set.push((pid, v));
                }
            }
        });
        // the grid: 4 × 4, bottom left the page's first pad
        let (grid, _) = ui.allocate_exact_size(vec2(ui.available_width(), 330.0), Sense::hover());
        let p = ui.painter().clone();
        let gap = 6.0;
        let pw = (grid.width() - gap * 5.0) / 4.0;
        let ph = (grid.height() - gap * 5.0) / 4.0;
        let first = FIRST_PAD + (st.page * 16) as u8;
        let mut drop_i = 0usize;
        for row in 0..4 {
            for col in 0..4 {
                let k = row * 4 + col;
                let note = first + k as u8;
                let r = Rect::from_min_size(pos2(grid.min.x + gap + col as f32 * (pw + gap), grid.max.y - gap - (row as f32 + 1.0) * (ph + gap) + gap), vec2(pw, ph));
                let resp = ui.interact(r, ui.scope_id().with(("pad", note)), Sense::click_and_drag());
                let sel = st.selected == note;
                let has_sample = pads.pads.get(&note).is_some_and(|x| x.audio.is_some());
                let muted = pads.muted.contains(&note);
                let solo = pads.soloed.contains(&note);
                let base = if has_sample { Color32::from_rgb(0x3d, 0x7f, 0xb8) } else { Color32::from_rgb(0x5a, 0x4a, 0x8a) };
                let any_solo = !pads.soloed.is_empty();
                let dim = muted || (any_solo && !solo);
                let target = st.drag_from.is_some_and(|f| f != note) && resp.contains_pointer();
                p.rect_filled(r, 8.0, if resp.hovered() || target { look::mix(base, Color32::WHITE, 0.15) } else { base }.gamma_multiply(if dim { 0.35 } else { 1.0 }));
                if target {
                    p.rect_stroke(r, 8.0, Stroke::new(2.0, Color32::from_rgb(0x3c, 0x9c, 0xf0)), egui::StrokeKind::Inside);
                }
                if sel {
                    p.rect_stroke(r, 8.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
                }
                let name = names.get((note - FIRST_PAD) as usize).cloned().unwrap_or_default();
                p.with_clip_rect(r.shrink(4.0)).text(r.center(), Align2::CENTER_CENTER, name, look::semibold(12.0), Color32::WHITE);
                if st.gm_names {
                    p.with_clip_rect(r.shrink(4.0)).text(r.center() + vec2(0.0, 16.0), Align2::CENTER_CENTER, gm_name(note), look::regular(10.0), Color32::from_white_alpha(170));
                }
                // hover: Mute, Solo, the speaker (plays while held) and the Action pop-up
                if resp.hovered() || muted || solo {
                    let b = |i: usize| Rect::from_min_size(pos2(r.max.x - 22.0 * (i as f32 + 1.0) - 4.0, r.max.y - 24.0), vec2(20.0, 18.0));
                    for (i, (label, on, cmd)) in [("S", solo, "Solo Pad"), ("M", muted, "Mute Pad")].iter().enumerate() {
                        let br = b(i);
                        let bresp = ui.interact(br, ui.scope_id().with(("padbtn", note, i)), Sense::click());
                        p.rect_filled(br, 4.0, if *on { if i == 0 { Color32::from_rgb(0xe8, 0xc0, 0x3c) } else { Color32::from_rgb(0x3c, 0x9c, 0xf0) } } else { Color32::from_black_alpha(90) });
                        p.text(br.center(), Align2::CENTER_CENTER, *label, look::semibold(10.5), Color32::WHITE);
                        if bresp.clicked() {
                            st.selected = note;
                            acts.push(A::Cmd(cmd));
                        }
                    }
                }
                if resp.hovered() {
                    let sr = Rect::from_min_size(r.left_top() + vec2(6.0, 18.0), vec2(18.0, 18.0));
                    let sresp = ui.interact(sr, ui.scope_id().with(("padspk", note)), Sense::click_and_drag()).on_hover_text("Play (hold)");
                    crate::glyphs::paint(&p, sr, if has_sample || !st.hide_icons { crate::glyphs::ph::SPEAKER_HIGH } else { crate::glyphs::ph::PLUS }, 14.0, Color32::WHITE);
                    if sresp.is_pointer_button_down_on() && st.held.is_none() {
                        st.held = Some(note);
                        acts.push(A::Play(note));
                    }
                }
                if st.held == Some(note) && !ui.input(|i| i.pointer.any_down()) {
                    st.held = None;
                    acts.push(A::Stop(note));
                }
                if resp.double_clicked() {
                    st.rename = Some((note, pads.pads.get(&note).map(|x| x.name.clone()).unwrap_or_default()));
                }
                if resp.drag_started() {
                    st.drag_from = Some(note);
                }
                if resp.drag_stopped() {
                    let to = ui.ctx().pointer_interact_pos().and_then(|pp| (0..16).map(|kk| first + kk as u8).find(|&nn| {
                        let kk = (nn - first) as usize;
                        let (rw, cl) = (kk / 4, kk % 4);
                        Rect::from_min_size(pos2(grid.min.x + gap + cl as f32 * (pw + gap), grid.max.y - gap - (rw as f32 + 1.0) * (ph + gap) + gap), vec2(pw, ph)).contains(pp)
                    }));
                    if let (Some(f), Some(tn)) = (st.drag_from, to)
                        && f != tn
                    {
                        acts.push(A::Swap(f, tn));
                    }
                    st.drag_from = None;
                }
                p.text(r.left_top() + vec2(6.0, 4.0), Align2::LEFT_TOP, lg_synth::sampler_doc::note_name(note), look::regular(10.0), Color32::from_white_alpha(180));
                if has_sample {
                    crate::glyphs::paint(&p, Rect::from_min_size(r.right_top() + vec2(-20.0, 4.0), vec2(16.0, 16.0)), crate::glyphs::ph::WAVEFORM, 13.0, Color32::from_white_alpha(200));
                }
                let mut tags = Vec::new();
                if muted {
                    tags.push("M");
                }
                if solo {
                    tags.push("S");
                }
                let ch = get(dmd::pad_param((note - FIRST_PAD) as usize, 6)).round() as u8;
                let excl = if ch > 0 { format!("Ex {ch}") } else { String::new() };
                p.text(r.left_bottom() + vec2(6.0, -4.0), Align2::LEFT_BOTTOM, format!("{} {excl}", tags.join(" ")), look::regular(10.0), Color32::from_white_alpha(200));
                if resp.clicked() {
                    st.selected = note;
                    acts.push(A::Play(note));
                }
                if !dropped.is_empty() && drop_pos.is_some_and(|q| r.contains(q)) {
                    drop_i = note as usize;
                    acts.push(A::Files(note, dropped.clone()));
                }
                resp.context_menu(|ui| {
                    st.selected = note;
                    ui.set_min_width(220.0);
                    if ui.button("Load Audio File…").clicked() {
                        acts.push(A::Load(note));
                        ui.close();
                    }
                    if ui.button("Rename Pad…").clicked() {
                        st.rename = Some((note, pads.pads.get(&note).map(|x| x.name.clone()).unwrap_or_default()));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Copy Pad").clicked() {
                        let pp: Vec<(u32, f32)> = (0..7).map(|f| (f as u32, get(dmd::pad_param((note - FIRST_PAD) as usize, f)))).collect();
                        st.clip = Some((pads.pads.get(&note).cloned().unwrap_or_default(), pp));
                        ui.close();
                    }
                    if ui.button("Cut Pad").clicked() {
                        let pp: Vec<(u32, f32)> = (0..7).map(|f| (f as u32, get(dmd::pad_param((note - FIRST_PAD) as usize, f)))).collect();
                        st.clip = Some((pads.pads.get(&note).cloned().unwrap_or_default(), pp));
                        acts.push(A::Cmd("Clear Pad"));
                        ui.close();
                    }
                    if ui.add_enabled(st.clip.is_some(), egui::Button::new("Paste Pad")).clicked() {
                        acts.push(A::Paste(note));
                        ui.close();
                    }
                    for c in ["Clear Pad", "Preview Pad", "Mute Pad", "Solo Pad", "Resample Pad"] {
                        if ui.button(c).clicked() {
                            acts.push(A::Cmd(c));
                            ui.close();
                        }
                    }
                    ui.menu_button("Assign To Exclusive Group", |ui| {
                        for c in ["Assign To Exclusive Group: Off", "Assign To Exclusive Group: 1", "Assign To Exclusive Group: 2", "Assign To Exclusive Group: 3", "Assign To Exclusive Group: 4", "Assign To Exclusive Group: 5", "Assign To Exclusive Group: 6", "Assign To Exclusive Group: 7", "Assign To Exclusive Group: 8"] {
                            if ui.button(c.trim_start_matches("Assign To Exclusive Group: ")).clicked() {
                                acts.push(A::Cmd(c));
                                ui.close();
                            }
                        }
                    });
                });
            }
        }
        let _ = drop_i;
        ui.horizontal(|ui| {
            ui.add_space(ui.available_width() / 2.0 - 44.0);
            if ui.add_enabled(st.page > 0, egui::Button::new(crate::glyphs::ph::CARET_LEFT).frame(false)).on_hover_text("Previous pad page").clicked() {
                st.page -= 1;
            }
            for pg in 0..3 {
                let (r, resp) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::click());
                ui.painter().circle(r.center(), 4.0, if st.page == pg { Color32::WHITE } else { Color32::TRANSPARENT }, Stroke::new(1.0, Color32::from_gray(0xb0)));
                if resp.on_hover_text(format!("Pad page {}", pg + 1)).clicked() {
                    st.page = pg;
                }
            }
            if ui.add_enabled(st.page < 2, egui::Button::new(crate::glyphs::ph::CARET_RIGHT).frame(false)).on_hover_text("Next pad page").clicked() {
                st.page += 1;
            }
        });
        ui.separator();
        // the selected pad's sound
        let note = st.selected;
        let k = (note - FIRST_PAD) as usize;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Pad {} — {}", lg_synth::sampler_doc::note_name(note), names.get(k).cloned().unwrap_or_default())).strong());
            if ui.button("Load Audio File…").clicked() {
                acts.push(A::Load(note));
            }
        });
        match &qvals {
            Some((qp, qv)) => {
                // the pad's Quick Sampler
                if let Some(a) = pads.pads.get(&note).and_then(|x| x.audio.clone()) {
                    let (wr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 70.0), Sense::hover());
                    crate::quickwin::draw_wave(ui, wr, &a, qv.get(2).copied().unwrap_or(0.0), qv.get(3).copied().unwrap_or(1.0), None);
                }
                egui::Grid::new("dmd-quick").num_columns(6).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                    for (i, prm) in qp.iter().enumerate() {
                        ui.label(egui::RichText::new(prm.name).size(11.0).color(look::text_dim()));
                        let mut v = qv[i];
                        let changed = match prm.unit {
                            lg_dsp::Unit::Menu => {
                                let mut c = false;
                                ui.menu_button(prm.choices.get(v.round() as usize).copied().unwrap_or(""), |ui| {
                                    for (j, ch) in prm.choices.iter().enumerate() {
                                        if ui.button(*ch).clicked() {
                                            v = j as f32;
                                            c = true;
                                            ui.close();
                                        }
                                    }
                                });
                                c
                            }
                            lg_dsp::Unit::OnOff => {
                                let mut on = v >= 0.5;
                                let c = ui.checkbox(&mut on, "").changed();
                                v = if on { 1.0 } else { 0.0 };
                                c
                            }
                            _ => ui.add(egui::DragValue::new(&mut v).range(prm.min..=prm.max).speed((prm.max - prm.min) / 300.0)).changed(),
                        };
                        if changed {
                            acts.push(A::QuickSet(note, prm.id, v));
                        }
                        if i % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
            }
            None => {
                // the pad's drum
                egui::Grid::new("dmd-pad").num_columns(4).spacing(vec2(10.0, 4.0)).show(ui, |ui| {
                    for f in 0..7 {
                        let pid = dmd::pad_param(k, f);
                        let Some(prm) = params.iter().find(|x| x.id == pid) else { continue };
                        let label = ["Kit", "Source Note", "Tune", "Level", "Pan", "Decay", "Exclusive Group"][f];
                        ui.label(egui::RichText::new(label).size(11.0).color(look::text_dim()));
                        let mut v = get(pid);
                        if f == 0 {
                            ui.menu_button(prm.choices.get(v.round() as usize).copied().unwrap_or(""), |ui| {
                                for (j, ch) in prm.choices.iter().enumerate() {
                                    if ui.button(*ch).clicked() {
                                        set.push((pid, j as f32));
                                        ui.close();
                                    }
                                }
                            });
                        } else if ui.add(egui::DragValue::new(&mut v).range(prm.min..=prm.max).speed(if f == 1 || f == 6 { 0.2 } else { (prm.max - prm.min) / 300.0 }).custom_formatter(|x, _| if f == 1 { lg_synth::sampler_doc::note_name(x.round() as u8) } else if f == 6 && x < 0.5 { "Off".into() } else { format!("{x:.2}") })).changed() {
                            set.push((pid, if f == 1 || f == 6 { v.round() } else { v }));
                        }
                        if f % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
            }
        }
        if let Some((n, name)) = st.rename.as_mut() {
            let n = *n;
            ui.horizontal(|ui| {
                ui.label("Pad name:");
                let te = ui.text_edit_singleline(name);
                crate::widgets::focus_once(ui, &te);
                if te.lost_focus() {
                    acts.push(A::Rename(n, name.clone()));
                }
            });
        }
    });
    app.inst.rect = resp.map(|r| r.response.rect);
    let renaming_done = acts.iter().any(|a| matches!(a, A::Rename(..)));
    if renaming_done {
        st.rename = None;
    }
    app.inst.dmd = st;
    if !set.is_empty() {
        instwin::checkpoint_once(app, ctx, t, "Pad Setting");
        instwin::set_params(app, t, &set);
    }
    for a in acts {
        match a {
            A::Play(n) => preview(app, t, n),
            A::Load(n) => {
                if let Some(p) = kn_ui::files::FileDialog::new().set_title("Load Audio File").add_filter("Audio", &["wav", "aif", "aiff", "caf", "flac", "WAV", "AIF", "AIFF"]).pick_file() {
                    load_file(app, t, n, &p);
                }
            }
            A::Files(n, files) => {
                // several files: on this pad and the next ones
                for (i, f) in files.iter().enumerate() {
                    let note = n + i as u8;
                    if note >= FIRST_PAD + PADS as u8 {
                        break;
                    }
                    load_file(app, t, note, f);
                }
            }
            A::Cmd(c) => {
                command(app, t, c);
            }
            A::Paste(n) => {
                if let Some((doc, pp)) = app.inst.dmd.clip.clone() {
                    edit(app, t, "Paste Pad", |p| {
                        if doc.audio.is_some() {
                            p.pads.insert(n, doc.clone());
                        } else {
                            p.pads.remove(&n);
                        }
                    });
                    let k = (n - FIRST_PAD) as usize;
                    let set: Vec<(u32, f32)> = pp.iter().map(|&(f, v)| (dmd::pad_param(k, f as usize), v)).collect();
                    instwin::set_params(app, t, &set);
                }
            }
            A::Rename(n, name) => edit(app, t, "Rename Pad", |p| p.pads.entry(n).or_default().name = name.clone()),
            A::Swap(a, b) => swap(app, t, a, b),
            A::Stop(n) => {
                if let Ok(mut e) = app.engine.lock() {
                    e.live(t, n, 0);
                }
            }
            A::StopAll => {
                instwin::with_inst(app, t, |d: &mut DrumMachineDesigner| lg_dsp::Plugin::reset(d));
            }
            A::TrackMute | A::TrackSolo => {
                app.checkpoint(if matches!(a, A::TrackMute) { "Mute Track" } else { "Solo Track" });
                if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                    if matches!(a, A::TrackMute) {
                        tr.mute = !tr.mute;
                    } else {
                        tr.solo = !tr.solo;
                    }
                }
                app.dirty = true;
                app.sync_mix();
            }
            A::RenameKit(n) => {
                app.inst.dmd.kit_rename = None;
                if !n.trim().is_empty() {
                    app.checkpoint("Rename Track");
                    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                        tr.name = n.trim().to_string();
                    }
                    app.dirty = true;
                }
            }
            A::QuickSet(n, pid, v) => {
                // straight into the running pad (no rebuild while dragging), and kept
                instwin::checkpoint_once(app, ctx, t, "Pad Setting");
                instwin::with_inst(app, t, |d: &mut DrumMachineDesigner| {
                    if let Some(q) = d.pad_quick_mut(n) {
                        lg_dsp::Plugin::set(q, pid, v);
                    }
                });
                if let Some(id) = instwin::track_id(app, t) {
                    let d = app.inst.pads.entry(id).or_default().pads.entry(n).or_default();
                    if let Some(x) = d.quick.iter_mut().find(|x| x.0 == pid) {
                        x.1 = v;
                    } else {
                        d.quick.push((pid, v));
                    }
                }
                app.dirty = true;
            }
        }
    }
    if !open {
        app.plugin_window = None;
        app.inst.rect = None;
    }
}

/// Two pads trade their sounds (sample, settings, drum parameters, mute / solo); the pads keep
/// their notes (Logic's Reorder Pads ▸ Change Sounds).
fn swap(app: &mut App, t: usize, a: u8, b: u8) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let get = |pid: u32| params.iter().position(|x| x.id == pid).map(|i| vals[i]).unwrap_or(0.0);
    let (ka, kb) = ((a - FIRST_PAD) as usize, (b - FIRST_PAD) as usize);
    let mut set = Vec::new();
    for f in 0..7 {
        let (va, vb) = (get(dmd::pad_param(ka, f)), get(dmd::pad_param(kb, f)));
        // a drum's source note stays the drum's (it moves with the sound)
        set.push((dmd::pad_param(ka, f), vb));
        set.push((dmd::pad_param(kb, f), va));
    }
    edit(app, t, "Move Pad", |p| {
        let (pa, pb) = (p.pads.remove(&a), p.pads.remove(&b));
        if let Some(x) = pa {
            p.pads.insert(b, x);
        }
        if let Some(x) = pb {
            p.pads.insert(a, x);
        }
        for set in [&mut p.muted, &mut p.soloed] {
            let (ia, ib) = (set.remove(&a), set.remove(&b));
            if ia {
                set.insert(b);
            }
            if ib {
                set.insert(a);
            }
        }
    });
    instwin::set_params(app, t, &set);
    app.inst.dmd.selected = b;
}

/// The Drum Machine Designer's key commands.
pub fn command(app: &mut App, t: usize, name: &str) -> bool {
    let sel = app.inst.dmd.selected;
    let k = (sel - FIRST_PAD) as usize;
    match name {
        "Select Next Pad" => {
            app.inst.dmd.selected = (sel + 1).min(FIRST_PAD + PADS as u8 - 1);
            app.inst.dmd.page = ((app.inst.dmd.selected - FIRST_PAD) / 16) as usize;
        }
        "Select Previous Pad" => {
            app.inst.dmd.selected = sel.saturating_sub(1).max(FIRST_PAD);
            app.inst.dmd.page = ((app.inst.dmd.selected - FIRST_PAD) / 16) as usize;
        }
        "Preview Pad" => preview(app, t, sel),
        "Mute Pad" => edit(app, t, "Mute Pad", |p| {
            if !p.muted.remove(&sel) {
                p.muted.insert(sel);
            }
        }),
        "Solo Pad" => edit(app, t, "Solo Pad", |p| {
            if !p.soloed.remove(&sel) {
                p.soloed.insert(sel);
            }
        }),
        "Clear Pad" => {
            edit(app, t, "Clear Pad", |p| {
                p.pads.remove(&sel);
                p.muted.remove(&sel);
                p.soloed.remove(&sel);
            });
            // the pad's drum back to the kit's
            let style = instwin::values(app, t).and_then(|(p, v)| p.iter().position(|x| x.id == 2).map(|i| v[i])).unwrap_or(0.0);
            let layout = dmd::default_layout(style.round() as u8);
            let pad = layout[sel as usize];
            instwin::set_params(app, t, &[(dmd::pad_param(k, 0), pad.kit as f32), (dmd::pad_param(k, 1), if pad.src == 0 { sel as f32 } else { pad.src as f32 }), (dmd::pad_param(k, 2), pad.tune), (dmd::pad_param(k, 3), 0.0), (dmd::pad_param(k, 4), 0.0), (dmd::pad_param(k, 5), 1.0), (dmd::pad_param(k, 6), pad.choke as f32)]);
        }
        "Resample Pad" => {
            let a = instwin::with_inst(app, t, |d: &mut DrumMachineDesigner| d.resample_pad(sel));
            if let Some(a) = a {
                let name = format!("{} (resampled)", lg_synth::sampler_doc::note_name(sel));
                edit(app, t, "Resample Pad", |p| {
                    p.pads.insert(sel, PadDoc { name, file: None, audio: Some(Arc::new(a)), quick: Vec::new() });
                });
            }
        }
        "Clear All Pads" => {
            edit(app, t, "Clear All Pads", |p| {
                p.pads.clear();
                p.muted.clear();
                p.soloed.clear();
            });
        }
        "Update All Kit Names" => {
            let kit = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
            let names: Vec<(u8, String)> = (FIRST_PAD..FIRST_PAD + PADS as u8).map(|n| {
                let pads = instwin::track_id(app, t).and_then(|id| app.inst.pads.get(&id).cloned()).unwrap_or_default();
                let cur = pad_name(app, t, &pads, n);
                let piece = cur.split(" (").next().unwrap_or(&cur).to_string();
                (n, format!("{piece} ({kit})"))
            }).collect();
            edit(app, t, "Update All Kit Names", |p| {
                for (n, name) in names {
                    p.pads.entry(n).or_default().name = name;
                }
            });
        }
        "Select Pad by Key" => app.inst.dmd.select_by_key = !app.inst.dmd.select_by_key,
        "Show GM Drum Names" => app.inst.dmd.gm_names = !app.inst.dmd.gm_names,
        "Hide Kit Piece Icons" => app.inst.dmd.hide_icons = !app.inst.dmd.hide_icons,
        n if n.starts_with("Assign To Exclusive Group: ") => {
            let g = n.trim_start_matches("Assign To Exclusive Group: ").parse::<f32>().unwrap_or(0.0);
            instwin::checkpoint(app, t, "Exclusive Group");
            instwin::set_params(app, t, &[(dmd::pad_param(k, 6), g)]);
        }
        _ => return false,
    }
    true
}

pub fn plain_key(app: &mut App, t: usize, key: egui::Key, m: egui::Modifiers) -> bool {
    if m.any() {
        return false;
    }
    match key {
        egui::Key::ArrowRight => command(app, t, "Select Next Pad"),
        egui::Key::ArrowLeft => command(app, t, "Select Previous Pad"),
        egui::Key::ArrowUp => {
            app.inst.dmd.selected = (app.inst.dmd.selected + 4).min(FIRST_PAD + PADS as u8 - 1);
            true
        }
        egui::Key::ArrowDown => {
            app.inst.dmd.selected = app.inst.dmd.selected.saturating_sub(4).max(FIRST_PAD);
            true
        }
        egui::Key::Delete | egui::Key::Backspace => command(app, t, "Clear Pad"),
        _ => false,
    }
}
