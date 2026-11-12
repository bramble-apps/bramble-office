//! The Quick Sampler window, as Logic's: the plug-in header (the Setting pop-up: Recall Default,
//! Save Setting As…, the saved settings), the mode buttons (Classic, One Shot, Slice,
//! Recorder), the waveform display and, under it, the mode's parameters; then the modulation
//! pane (LFO 1 / LFO 2 / Mod Matrix tabs) and the Pitch, Filter and Amp panes, each with its
//! envelope; MIDI Mono Mode under the footer's disclosure triangle.
//!
//! The waveform display: the sample name pop-up (Load Audio File…, the files used before,
//! Rename Current File…, Remove Current File, Clear History, Show in File Manager), Snap, the
//! vertical and horizontal zoom buttons, the Action pop-up (the same items as the shortcut
//! menu, by mode), blue start / end markers, yellow loop markers with the shaded loop area
//! (drag it to move the loop), the gray crossfade and fade markers, slice markers (yellow:
//! follow Sensitivity, orange: placed or kept by hand; click to add one, drag, double-click to
//! remove, the Play button at a marker's foot plays the slice), Option-drag moves a pair,
//! Control-Option-drag zooms to an area, wheel scrolls, Control-wheel zooms, the scroll bar; the
//! pointer's parameter and value in the bar under the display (double-click a marker to keep
//! it there, × to let go). Dropped files land on the Original or Optimized half.

use std::path::PathBuf;
use std::sync::Arc;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_dsp::Plugin;
use lg_synth::quick::{self as qs, QuickSampler, SliceEdits};
use lg_synth::sampler_synth as ss;
use lg_synth::wav::Audio;

use crate::app::App;
use crate::instwin;
use crate::look;
use crate::synthui::{Pane, Skin};

pub const COMMANDS: &[&str] = &[
    "Auto-Loop",
    "Auto-Loop within Loop Area",
    "Optimize Loop Start",
    "Optimize Loop Crossfade/End",
    "Set Sample Start to Loop Start",
    "Set Sample End to Loop End",
    "Set Loop from Sample Start to Sample End",
    "Crop Sample",
    "Crop Loop",
    "Equal Power Crossfade",
    "Linear Crossfade",
    "Optimize Sample Gain",
    "Retune",
    "Reimport Original",
    "Reimport Optimized",
    "Initialize Synth Parameters",
    "Derive Tempo from Loop Length",
    "Re-Analyze Transients and Tempo",
    "Write Sample Loop to Audio File",
    "Copy MIDI Pattern",
    "Create Drum Machine Designer Track",
    "Create Sampler Track",
    "Add Slice Marker",
    "Delete Slice Marker",
    "Slice Marker ignores Sensitivity",
    "Slice Marker responds to Sensitivity",
];

/// Logic's Quick Sampler colours: the dark window, slate panes, navy envelope displays, the
/// blue curves and gold values (measured on the Sampler's screenshot, the same family).
const SKIN: Skin = Skin::new(0x131212, 0x2a3742, 0xd9b44a).texts(0xe6e8ea, 0x9aa3ab);
const NAVY: Color32 = Color32::from_rgb(0x0c, 0x27, 0x3d);
const BLUE: Color32 = Color32::from_rgb(0x48, 0x9f, 0xf8);
const GOLD: Color32 = Color32::from_rgb(0xf2, 0xc1, 0x4e);
const ORANGE: Color32 = Color32::from_rgb(0xf0, 0x8a, 0x2c);
const WAVE_BG: Color32 = Color32::from_rgb(0x15, 0x19, 0x1d);
const SIZE: egui::Vec2 = vec2(1010.0, 668.0);

#[derive(Clone, Default)]
pub struct QuickDoc {
    pub file: Option<PathBuf>,
    pub audio: Option<Arc<Audio>>,
    /// Slice markers placed, moved or removed by hand.
    pub edits: Option<SliceEdits>,
    /// The sample name pop-up's files used before (newest first).
    pub history: Vec<PathBuf>,
}

/// A marker in the waveform display.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mark {
    Start,
    End,
    LoopStart,
    LoopEnd,
    LoopArea,
    Xfade,
    FadeIn,
    FadeOut,
    Slice(usize),
}

#[derive(Default)]
pub struct Ui {
    /// The visible part of the file (fractions); None: the whole file.
    view: Option<(f32, f32)>,
    /// The last zoom set by hand (the horizontal zoom button goes back to it).
    manual: Option<(f32, f32)>,
    vmax: bool,
    drag: Option<(Mark, bool)>,
    drag_from: (f32, f32, f32),
    pinned: Option<Mark>,
    snap: usize,
    units: usize,
    mono: bool,
    /// 0 LFO 1, 1 LFO 2, 2 Mod Matrix.
    mod_tab: usize,
    attempt: u32,
    rename: Option<String>,
    zoom_from: Option<f32>,
    menu_at: Option<f32>,
    /// Load Audio File's option: Optimized (else Original).
    optimized: bool,
}

const SNAPS: &[&str] = &["Off", "Zero Crossings", "Transients"];

/// What the window asks the app for.
enum Act {
    Load(PathBuf, bool),
    LoadDialog(bool),
    Rename(String),
    RemoveFile,
    ClearHistory,
    ShowFolder,
    Edits(Option<SliceEdits>),
    Play(u8),
    Command(&'static str),
    Record(bool),
    Input(Option<crate::sidechain::Choice>),
}

pub fn apply(d: &QuickDoc, q: &mut QuickSampler) {
    if let Some(a) = &d.audio {
        q.replace_audio((**a).clone());
    }
    q.set_slice_edits(d.edits.clone().unwrap_or_default());
}

/// The waveform (peaks per pixel) with the start / end marked (the Drum Machine Designer's pad
/// view uses it too).
pub fn draw_wave(ui: &egui::Ui, r: Rect, a: &Audio, start: f32, end: f32, _extra: Option<(&[usize], Option<(f32, f32)>)>) {
    let p = ui.painter_at(r);
    p.rect_filled(r, 4.0, WAVE_BG);
    peaks(&p, r, a, (0.0, 1.0), true, 1.0);
    let xf = |f: f32| r.min.x + f * r.width();
    p.rect_filled(Rect::from_min_max(r.min, pos2(xf(start), r.max.y)), 0.0, Color32::from_black_alpha(110));
    p.rect_filled(Rect::from_min_max(pos2(xf(end), r.min.y), r.max), 0.0, Color32::from_black_alpha(110));
    p.vline(xf(start), r.y_range(), Stroke::new(1.5, BLUE));
    p.vline(xf(end), r.y_range(), Stroke::new(1.5, BLUE));
}

/// Peaks of the view (`v0`..`v1` of the file) per pixel: each channel in its own lane, or the
/// mono sum; `gain` zooms vertically.
fn peaks(p: &egui::Painter, r: Rect, a: &Audio, (v0, v1): (f32, f32), mono: bool, gain: f32) {
    let n = a.frames().max(1);
    let w = r.width().max(1.0) as usize;
    let lanes: Vec<Vec<&[f32]>> = if mono || a.channels.len() < 2 { vec![a.channels.iter().map(|c| c.as_slice()).collect()] } else { a.channels.iter().take(2).map(|c| vec![c.as_slice()]).collect() };
    let lh = r.height() / lanes.len() as f32;
    for (li, chans) in lanes.iter().enumerate() {
        let cy = r.min.y + lh * (li as f32 + 0.5);
        let k = chans.len().max(1) as f32;
        for x in 0..w {
            let f0 = v0 + (v1 - v0) * x as f32 / w as f32;
            let f1 = v0 + (v1 - v0) * (x + 1) as f32 / w as f32;
            let i0 = (f0 * n as f32) as usize;
            let i1 = ((f1 * n as f32) as usize).max(i0 + 1).min(n);
            if i0 >= n {
                break;
            }
            let step = ((i1 - i0) / 64).max(1);
            let (mut lo, mut hi) = (0.0f32, 0.0f32);
            for i in (i0..i1).step_by(step) {
                let s: f32 = chans.iter().map(|c| c.get(i).copied().unwrap_or(0.0)).sum::<f32>() / k;
                lo = lo.min(s);
                hi = hi.max(s);
            }
            let amp = lh * 0.46 * gain;
            let xx = r.min.x + x as f32 + 0.5;
            p.vline(xx, (cy - (hi * amp).min(lh / 2.0))..=(cy - (lo * amp).max(-lh / 2.0) + 0.5), Stroke::new(1.0, BLUE.gamma_multiply(0.85)));
        }
        if li > 0 {
            p.hline(r.x_range(), r.min.y + lh * li as f32, Stroke::new(1.0, Color32::from_gray(0x30)));
        }
    }
}

/// Everything the panel shows that isn't a parameter.
struct View<'a> {
    audio: Option<Arc<Audio>>,
    file: Option<PathBuf>,
    history: Vec<PathBuf>,
    slices: Vec<usize>,
    manual: Vec<bool>,
    detected: Vec<usize>,
    edits: SliceEdits,
    play: Option<usize>,
    rec: (bool, bool, f32),
    inputs: Vec<(String, Option<crate::sidechain::Choice>)>,
    input: String,
    dragging_files: bool,
    dropped: Vec<PathBuf>,
    drop_pos: Option<egui::Pos2>,
    bpm: f32,
    st: &'a mut Ui,
    acts: Vec<Act>,
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some(id) = instwin::track_id(app, t) else { return };
    let doc = app.inst.quick.get(&id).cloned().unwrap_or_default();
    // the sample the instrument plays (also one from a project or a patch)
    let audio = doc.audio.clone().or_else(|| instwin::with_inst(app, t, |q: &mut QuickSampler| q.audio().cloned().map(Arc::new)).flatten());
    let (slices, manual, detected, edits, play, rec) = instwin::with_inst(app, t, |q: &mut QuickSampler| (q.slices.clone(), q.slice_manual.clone(), q.detected.clone(), q.edits.clone(), q.play_position(), (q.rec.armed, q.rec.running, q.rec.level))).unwrap_or_default();
    if play.is_some() || rec.0 {
        ctx.request_repaint();
    }
    let mut inputs: Vec<(String, Option<crate::sidechain::Choice>)> = vec![("None".into(), None), ("Input 1-2".into(), Some(crate::sidechain::Choice::Input))];
    if let Some(p) = &app.project {
        for (i, tr) in p.tracks.iter().enumerate() {
            if i != t {
                inputs.push((tr.name.clone(), Some(crate::sidechain::Choice::Track(tr.extra.id))));
            }
        }
    }
    for b in 0..8 {
        inputs.push((format!("Bus {}", b + 1), Some(crate::sidechain::Choice::Bus(b))));
    }
    let input = crate::sidechain::choice(app, t).map(|c| crate::sidechain::label_of(app, c)).unwrap_or_else(|| "None".into());
    let mut st = std::mem::take(&mut app.inst.quick_ui);
    let mut v = View {
        audio,
        file: doc.file.clone(),
        history: doc.history.clone(),
        slices,
        manual,
        detected,
        edits,
        play,
        rec,
        inputs,
        input,
        dragging_files: instwin::dragging_files(ctx),
        dropped: std::mem::take(&mut app.inst.dropped),
        drop_pos: app.inst.drop_pos,
        bpm: app.tempo() as f32,
        st: &mut st,
        acts: Vec::new(),
    };
    let ext = [ss::MONO_MODE, ss::MONO_BEND];
    let named = crate::synthui::window(app, ctx, t, "Quick Sampler", SIZE, SKIN, &ext, |pn| body(pn, &mut v));
    let acts = std::mem::take(&mut v.acts);
    let audio = v.audio.clone();
    drop(v);
    app.inst.quick_ui = st;
    for n in named {
        if let Some(c) = COMMANDS.iter().find(|c| **c == n) {
            command_on(app, t, c, audio.clone());
        }
    }
    for a in acts {
        run(app, ctx, t, a, audio.clone());
    }
    rename_sheet(app, ctx, t);
}

// ---------------------------------------------------------------------------------------------
// The panel

fn body(pn: &mut Pane, v: &mut View) {
    mode_tabs(pn);
    let mode = pn.geti(qs::MODE);
    wave_display(pn, v, mode);
    mode_row(pn, v, mode);
    lower(pn, v);
}

/// The mode buttons: a capsule of four, a dot lit on the chosen one.
fn mode_tabs(pn: &mut Pane) {
    let names = ["CLASSIC", "ONE SHOT", "SLICE", "RECORDER"];
    let w = 118.0;
    let total = w * 4.0;
    let x0 = (SIZE.x - total) / 2.0;
    let r = pn.rect(x0, 8.0, total, 28.0);
    let p = pn.painter();
    p.rect_filled(r, 14.0, Color32::from_rgb(0x24, 0x2d, 0x35));
    let cur = pn.geti(qs::MODE);
    for (k, n) in names.iter().enumerate() {
        let cr = pn.rect(x0 + w * k as f32, 8.0, w, 28.0);
        let resp = pn.ui.interact(cr, pn.id("qs-mode", k as u32), Sense::click());
        if k > 0 {
            p.vline(cr.min.x, cr.y_range().shrink(6.0), Stroke::new(1.0, Color32::from_gray(0x3a)));
        }
        let lit = k == cur;
        p.circle_filled(pos2(cr.min.x + 22.0, cr.center().y), 3.5, if lit { GOLD } else { Color32::from_gray(0x50) });
        p.text(pos2(cr.min.x + 34.0, cr.center().y), Align2::LEFT_CENTER, n, look::regular(12.5), if lit || resp.hovered() { Color32::WHITE } else { Color32::from_gray(0xb8) });
        if resp.clicked() && !lit {
            pn.put(qs::MODE, k as f32);
        }
    }
}

fn file_name(f: &std::path::Path) -> String {
    f.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn fmt_pos(units: usize, frame: usize, rate: u32, bpm: f32) -> String {
    match units {
        1 => format!("{frame} smp"),
        2 => {
            let beats = frame as f32 / rate.max(1) as f32 * bpm / 60.0;
            format!("{} {}", (beats / 4.0).floor() as i32 + 1, format!("{:.2}", beats % 4.0 + 1.0))
        }
        _ => format!("{:.3} s", frame as f32 / rate.max(1) as f32),
    }
}

/// The waveform display with its bar (Snap, sample name, zoom, Action) and the pointer bar.
fn wave_display(pn: &mut Pane, v: &mut View, mode: usize) {
    let (x0, y0, w) = (10.0, 44.0, SIZE.x - 20.0);
    pn.group(x0, y0, w, 262.0, "");
    // the bar
    let bar_y = y0 + 6.0;
    let snap_r = pn.rect(x0 + 8.0, bar_y, 150.0, 22.0);
    let p = pn.painter();
    let sresp = pn.ui.interact(snap_r, pn.id("qs-snap", 0), Sense::click());
    p.rect_filled(snap_r, 4.0, Color32::from_gray(0x22));
    p.text(snap_r.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, format!("Snap: {}", SNAPS[v.st.snap.min(SNAPS.len() - 1)]), look::regular(11.5), SKIN.text);
    egui::Popup::menu(&sresp).show(|ui| {
        for (k, s) in SNAPS.iter().enumerate() {
            if ui.add(egui::Button::selectable(v.st.snap == k, *s)).clicked() {
                v.st.snap = k;
                ui.close();
            }
        }
    });
    // the sample name pop-up
    let name = v.file.as_deref().map(file_name).unwrap_or_else(|| if v.audio.is_some() { "Recording".into() } else { "Load Audio File".into() });
    let nr = pn.rect(SIZE.x / 2.0 - 160.0, bar_y, 320.0, 22.0);
    let nresp = pn.ui.interact(nr, pn.id("qs-name", 0), Sense::click());
    p.rect_filled(nr, 4.0, if nresp.hovered() { Color32::from_gray(0x30) } else { Color32::from_gray(0x22) });
    p.with_clip_rect(nr.shrink(2.0)).text(nr.center(), Align2::CENTER_CENTER, &name, look::regular(12.0), SKIN.text);
    crate::glyphs::paint(&p, Rect::from_center_size(pos2(nr.max.x - 12.0, nr.center().y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_UP_DOWN, 10.0, SKIN.dim);
    egui::Popup::menu(&nresp).show(|ui| {
        ui.set_min_width(240.0);
        if ui.button("Load Audio File…").clicked() {
            v.acts.push(Act::LoadDialog(v.st.optimized));
            ui.close();
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Options:").size(11.0));
            if ui.radio(!v.st.optimized, "Original").clicked() {
                v.st.optimized = false;
            }
            if ui.radio(v.st.optimized, "Optimized").clicked() {
                v.st.optimized = true;
            }
        });
        if !v.history.is_empty() {
            ui.separator();
            for (k, f) in v.history.iter().enumerate() {
                let cur = k == 0 && v.file.as_ref() == Some(f);
                if ui.add(egui::Button::selectable(cur, file_name(f))).on_hover_text(f.display().to_string()).clicked() && !cur {
                    v.acts.push(Act::Load(f.clone(), false));
                    ui.close();
                }
            }
        }
        ui.separator();
        let has = v.file.is_some();
        if ui.add_enabled(has, egui::Button::new("Rename Current File…")).clicked() {
            v.st.rename = v.file.as_deref().map(|f| f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            ui.close();
        }
        if ui.add_enabled(v.audio.is_some(), egui::Button::new("Remove Current File")).clicked() {
            v.acts.push(Act::RemoveFile);
            ui.close();
        }
        if ui.add_enabled(v.history.len() > 1, egui::Button::new("Clear History")).clicked() {
            v.acts.push(Act::ClearHistory);
            ui.close();
        }
        ui.separator();
        if ui.add_enabled(has, egui::Button::new("Show in File Manager")).clicked() {
            v.acts.push(Act::ShowFolder);
            ui.close();
        }
    });
    // zoom buttons and the Action pop-up at the right
    let rx = x0 + w - 8.0;
    let vz = pn.rect(rx - 120.0, bar_y, 26.0, 22.0);
    let hz = pn.rect(rx - 90.0, bar_y, 26.0, 22.0);
    let ar = pn.rect(rx - 58.0, bar_y, 58.0, 22.0);
    for (r, icon, lit, tip, k) in [(vz, crate::glyphs::ph::ARROWS_OUT_LINE_VERTICAL, v.st.vmax, "Zoom vertical: the waveform at its largest or as it is", 0u32), (hz, crate::glyphs::ph::ARROWS_OUT_LINE_HORIZONTAL, v.st.view.is_some(), "Zoom horizontal: the last zoom, or the part between the start and end markers", 1)] {
        let resp = pn.ui.interact(r, pn.id("qs-zoom", k), Sense::click()).on_hover_text(tip);
        p.rect_filled(r, 4.0, if lit { Color32::from_gray(0x48) } else if resp.hovered() { Color32::from_gray(0x30) } else { Color32::from_gray(0x22) });
        crate::glyphs::paint(&p, Rect::from_center_size(r.center(), vec2(14.0, 14.0)), icon, 14.0, if lit { GOLD } else { SKIN.text });
        if resp.clicked() {
            if k == 0 {
                v.st.vmax = !v.st.vmax;
            } else if v.st.view.is_some() {
                v.st.view = None;
            } else {
                let (s, e) = (pn.get(qs::START), pn.get(qs::END));
                v.st.view = v.st.manual.or(if s > 0.0 || e < 1.0 { Some((s, e)) } else { None });
            }
        }
    }
    let aresp = pn.ui.interact(ar, pn.id("qs-action", 0), Sense::click()).on_hover_text("Action");
    p.rect_filled(ar, 4.0, if aresp.hovered() { Color32::from_gray(0x30) } else { Color32::from_gray(0x22) });
    crate::glyphs::paint(&p, Rect::from_center_size(pos2(ar.min.x + 16.0, ar.center().y), vec2(14.0, 14.0)), crate::glyphs::ph::DOTS_THREE_CIRCLE, 14.0, SKIN.text);
    crate::glyphs::paint(&p, Rect::from_center_size(pos2(ar.max.x - 12.0, ar.center().y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_DOWN, 10.0, SKIN.dim);
    let vals = pn_vals(pn);
    egui::Popup::menu(&aresp).show(|ui| {
        ui.set_min_width(300.0);
        action_items(ui, vals, v, mode, true, None);
    });
    // the display
    let wr = pn.rect(x0 + 8.0, bar_y + 28.0, w - 16.0, 178.0);
    let sb = pn.rect(x0 + 8.0, bar_y + 208.0, w - 16.0, 8.0);
    let info = pn.rect(x0 + 8.0, bar_y + 220.0, w - 16.0, 26.0);
    p.rect_filled(wr, 5.0, WAVE_BG);
    let Some(a) = v.audio.clone() else {
        p.text(wr.center(), Align2::CENTER_CENTER, "Drag an audio file or region here, or choose Load Audio File in the sample name pop-up", look::regular(13.0), SKIN.dim);
        drops(pn, v, wr);
        return;
    };
    let n = a.frames().max(1);
    let (v0, v1) = v.st.view.unwrap_or((0.0, 1.0));
    let span = (v1 - v0).max(1e-6);
    let xf = |f: f32| wr.min.x + (f - v0) / span * wr.width();
    let fx = |x: f32| (v0 + (x - wr.min.x) / wr.width() * span).clamp(0.0, 1.0);
    let painter = p.with_clip_rect(wr);
    peaks(&painter, wr, &a, (v0, v1), v.st.mono, if v.st.vmax { 4.0 } else { 1.0 });
    let (s, e) = (pn.get(qs::START), pn.get(qs::END));
    let looping = mode == 0 && pn.geti(qs::LOOP_MODE) > 0;
    let (ls, le) = (pn.get(qs::LOOP_START), pn.get(qs::LOOP_END));
    let rate = a.rate.max(1) as f32;
    let secs = n as f32 / rate;
    // outside the played part: dimmed
    painter.rect_filled(Rect::from_min_max(wr.min, pos2(xf(s), wr.max.y)), 0.0, Color32::from_black_alpha(120));
    painter.rect_filled(Rect::from_min_max(pos2(xf(e), wr.min.y), wr.max), 0.0, Color32::from_black_alpha(120));
    // markers: (mark, fraction, colour)
    let mut marks: Vec<(Mark, f32)> = vec![(Mark::Start, s), (Mark::End, e)];
    let fade = |id: u32| pn.get(id) / secs;
    if mode == 1 || mode == 2 {
        marks.push((Mark::FadeIn, s + fade(qs::FADE_IN)));
        marks.push((Mark::FadeOut, e - fade(qs::FADE_OUT)));
    }
    if looping {
        painter.rect_filled(Rect::from_min_max(pos2(xf(ls), wr.min.y), pos2(xf(le), wr.max.y)), 0.0, GOLD.gamma_multiply(0.12));
        marks.push((Mark::LoopStart, ls));
        marks.push((Mark::LoopEnd, le));
        marks.push((Mark::Xfade, ls + fade(qs::XFADE)));
    }
    if mode == 2 {
        for (i, &x) in v.slices.iter().enumerate() {
            marks.push((Mark::Slice(i), x as f32 / n as f32));
        }
    }
    for &(m, f) in &marks {
        let x = xf(f);
        match m {
            Mark::Start | Mark::End => {
                painter.vline(x, wr.y_range(), Stroke::new(1.5, BLUE));
                let d = if m == Mark::Start { 1.0 } else { -1.0 };
                painter.add(egui::Shape::convex_polygon(vec![pos2(x, wr.min.y), pos2(x + 9.0 * d, wr.min.y), pos2(x, wr.min.y + 9.0)], BLUE, Stroke::NONE));
            }
            Mark::LoopStart | Mark::LoopEnd => {
                painter.vline(x, wr.y_range(), Stroke::new(1.5, GOLD));
                let d = if m == Mark::LoopStart { 1.0 } else { -1.0 };
                painter.add(egui::Shape::convex_polygon(vec![pos2(x, wr.max.y), pos2(x + 9.0 * d, wr.max.y), pos2(x, wr.max.y - 9.0)], GOLD, Stroke::NONE));
            }
            Mark::Xfade => {
                let x0 = xf(ls);
                painter.line_segment([pos2(x0, wr.max.y - 14.0), pos2(x, wr.min.y + 14.0)], Stroke::new(1.0, Color32::from_gray(0x9a)));
                painter.circle_filled(pos2(x, wr.min.y + 14.0), 4.0, Color32::from_gray(0xb0));
            }
            Mark::FadeIn => {
                painter.line_segment([pos2(xf(s), wr.max.y - 4.0), pos2(x, wr.min.y + 4.0)], Stroke::new(1.0, Color32::from_gray(0x9a)));
                painter.circle_filled(pos2(x, wr.min.y + 4.0), 4.0, Color32::from_gray(0xb0));
            }
            Mark::FadeOut => {
                painter.line_segment([pos2(x, wr.min.y + 4.0), pos2(xf(e), wr.max.y - 4.0)], Stroke::new(1.0, Color32::from_gray(0x9a)));
                painter.circle_filled(pos2(x, wr.min.y + 4.0), 4.0, Color32::from_gray(0xb0));
            }
            Mark::Slice(i) => {
                let col = if v.manual.get(i).copied().unwrap_or(false) { ORANGE } else { GOLD };
                painter.vline(x, wr.y_range(), Stroke::new(1.0, col));
                painter.rect_filled(Rect::from_min_size(pos2(x, wr.max.y - 14.0), vec2(12.0, 14.0)), 2.0, col);
            }
            Mark::LoopArea => {}
        }
    }
    if let Some(pp) = v.play {
        painter.vline(xf(pp as f32 / n as f32), wr.y_range(), Stroke::new(1.0, Color32::WHITE));
    }
    // interaction
    let resp = pn.ui.interact(wr, pn.id("qs-wave", 0), Sense::click_and_drag());
    let m = pn.ui.input(|i| i.modifiers);
    let hit = |x: f32, y: f32| -> Option<Mark> {
        let mut best: Option<(Mark, f32)> = None;
        for &(mk, f) in &marks {
            let d = (xf(f) - x).abs();
            // fade / crossfade handles are dots near the top
            let ok = match mk {
                Mark::FadeIn | Mark::FadeOut | Mark::Xfade => d < 6.0 && (y - wr.min.y) < 22.0,
                _ => d < 5.0,
            };
            if ok && best.is_none_or(|b| d < b.1) {
                best = Some((mk, d));
            }
        }
        best.map(|b| b.0).or_else(|| (looping && x > xf(ls) && x < xf(le) && y > wr.center().y).then_some(Mark::LoopArea))
    };
    let hover = resp.hover_pos().and_then(|pp| hit(pp.x, pp.y));
    if let Some(h) = hover {
        pn.ui.ctx().set_cursor_icon(if h == Mark::LoopArea { egui::CursorIcon::Grab } else { egui::CursorIcon::ResizeHorizontal });
    }
    // the Play button at a slice marker's foot
    if mode == 2
        && let Some(pp) = resp.hover_pos()
        && let Some(Mark::Slice(i)) = hover
        && pp.y > wr.max.y - 16.0
    {
        let x = xf(v.slices[i] as f32 / n as f32);
        let pr = Rect::from_min_size(pos2(x + 1.0, wr.max.y - 14.0), vec2(12.0, 14.0));
        painter.rect_filled(pr, 2.0, Color32::WHITE);
        crate::glyphs::paint(&painter, pr.shrink(2.0), crate::glyphs::ph_fill::PLAY, 9.0, Color32::BLACK);
        if resp.clicked() {
            let keys = qs::slice_keys(pn.get(qs::START_KEY).round() as u8, pn.geti(qs::KEY_MAPPING) as u8, v.slices.len());
            if let Some(&k) = keys.get(i) {
                v.acts.push(Act::Play(k));
            }
        }
    }
    let snap_mode = v.st.snap;
    let detected = v.detected.clone();
    let snap = |f: f32| -> f32 {
        let frame = (f * n as f32) as usize;
        let to = match snap_mode {
            1 => {
                let c = &a.channels[0];
                let reach = (rate * 0.005) as usize;
                (frame.saturating_sub(reach)..(frame + reach).min(n.saturating_sub(1))).find(|&i| i + 1 < c.len() && c[i] <= 0.0 && c[i + 1] > 0.0 && (i as i64 - frame as i64).unsigned_abs() as usize <= reach).unwrap_or(frame)
            }
            2 => detected.iter().copied().min_by_key(|&d| d.abs_diff(frame)).filter(|d| d.abs_diff(frame) < (rate * 0.05) as usize).unwrap_or(frame),
            _ => frame,
        };
        to as f32 / n as f32
    };
    // Control-Option-drag: zoom to the area
    if resp.drag_started() && m.ctrl && m.alt {
        v.st.zoom_from = resp.interact_pointer_pos().map(|pp| fx(pp.x));
    } else if resp.drag_started()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let start = pp - resp.drag_delta();
        v.st.drag = hit(start.x, start.y).map(|h| (h, m.alt));
        v.st.drag_from = (fx(start.x), 0.0, 0.0);
    }
    if let (Some(z0), Some(pp)) = (v.st.zoom_from, resp.interact_pointer_pos()) {
        let z1 = fx(pp.x);
        painter.rect_filled(Rect::from_min_max(pos2(xf(z0.min(z1)), wr.min.y), pos2(xf(z0.max(z1)), wr.max.y)), 0.0, Color32::from_white_alpha(30));
        if resp.drag_stopped() {
            if (z1 - z0).abs() * n as f32 > 64.0 {
                v.st.view = Some((z0.min(z1), z0.max(z1)));
                v.st.manual = v.st.view;
            }
            v.st.zoom_from = None;
        }
    }
    if resp.dragged()
        && let (Some((mk, alt)), Some(pp)) = (v.st.drag, resp.interact_pointer_pos())
    {
        let f = snap(fx(pp.x));
        let d = fx(pp.x) - fx(pp.x - resp.drag_delta().x);
        let secs_of = |frac: f32| (frac * secs).max(0.0);
        match mk {
            Mark::Start if alt && (s > 0.0 || e < 1.0) => {
                let d = d.clamp(-s, 1.0 - e);
                pn.put(qs::START, s + d);
                pn.put(qs::END, e + d);
            }
            Mark::End if alt && (s > 0.0 || e < 1.0) => {
                let d = d.clamp(-s, 1.0 - e);
                pn.put(qs::START, s + d);
                pn.put(qs::END, e + d);
            }
            Mark::Start => pn.put(qs::START, f.min(e - 0.001)),
            Mark::End => pn.put(qs::END, f.max(s + 0.001)),
            Mark::LoopStart | Mark::LoopEnd | Mark::LoopArea if alt || mk == Mark::LoopArea => {
                let d = d.clamp(-ls, 1.0 - le);
                pn.put(qs::LOOP_START, ls + d);
                pn.put(qs::LOOP_END, le + d);
            }
            Mark::LoopStart => pn.put(qs::LOOP_START, f.min(le - 0.0005)),
            Mark::LoopEnd => pn.put(qs::LOOP_END, f.max(ls + 0.0005)),
            Mark::Xfade => pn.put(qs::XFADE, secs_of(f - ls).min(2.0)),
            Mark::FadeIn | Mark::FadeOut if alt => {
                let dd = d * secs;
                pn.put(qs::FADE_IN, (pn.get(qs::FADE_IN) + if mk == Mark::FadeIn { dd } else { -dd }).max(0.0));
                pn.put(qs::FADE_OUT, (pn.get(qs::FADE_OUT) + if mk == Mark::FadeIn { dd } else { -dd }).max(0.0));
            }
            Mark::FadeIn => pn.put(qs::FADE_IN, secs_of(f - s)),
            Mark::FadeOut => pn.put(qs::FADE_OUT, secs_of(e - f)),
            Mark::LoopArea => {}
            Mark::Slice(i) => {
                // a moved marker is one placed by hand
                let to = (f * n as f32) as usize;
                let mut ed = v.edits.clone();
                let from = v.slices[i];
                ed.manual.retain(|&x| x != from);
                if !v.manual.get(i).copied().unwrap_or(false) {
                    ed.removed.push(from);
                }
                ed.manual.push(to);
                v.slices[i] = to;
                if let Some(mm) = v.manual.get_mut(i) {
                    *mm = true;
                }
                v.edits = ed.clone();
                v.acts.push(Act::Edits(Some(ed)));
            }
        }
    }
    if resp.drag_stopped() {
        v.st.drag = None;
    }
    if mode == 2
        && let Some(pp) = resp.interact_pointer_pos()
        && !(m.ctrl || m.command)
    {
        if resp.double_clicked() {
            if let Some(Mark::Slice(i)) = hit(pp.x, pp.y)
                && i > 0
            {
                v.acts.push(Act::Edits(Some(delete_slice(&v.edits, v.slices[i], v.manual[i]))));
            }
        } else if resp.clicked() && hit(pp.x, pp.y).is_none() && pp.y < wr.max.y - 16.0 {
            let mut ed = v.edits.clone();
            ed.manual.push((snap(fx(pp.x)) * n as f32) as usize);
            v.acts.push(Act::Edits(Some(ed)));
        }
    }
    if resp.double_clicked()
        && let Some(pp) = resp.interact_pointer_pos()
        && let Some(h) = hit(pp.x, pp.y)
        && !matches!(h, Mark::Slice(_))
    {
        v.st.pinned = Some(h);
    }
    // wheel: scroll; Control-wheel: zoom around the pointer
    if let Some(pp) = resp.hover_pos() {
        let (dx, dy, ctrl) = pn.ui.input(|i| (i.smooth_scroll_delta.x, i.smooth_scroll_delta.y, i.modifiers.ctrl || i.modifiers.command));
        if ctrl && dy != 0.0 {
            let at = fx(pp.x);
            let k = (1.0 - dy / 300.0).clamp(0.5, 2.0);
            let nspan = (span * k).clamp(64.0 / n as f32, 1.0);
            let a0 = (at - (at - v0) * nspan / span).clamp(0.0, 1.0 - nspan);
            v.st.view = if nspan >= 0.999 { None } else { Some((a0, a0 + nspan)) };
            v.st.manual = v.st.view;
        } else if (dx != 0.0 || dy != 0.0) && span < 1.0 {
            let d = -(if dx != 0.0 { dx } else { dy }) / wr.width() * span;
            let a0 = (v0 + d).clamp(0.0, 1.0 - span);
            v.st.view = Some((a0, a0 + span));
            v.st.manual = v.st.view;
        }
    }
    // the shortcut menu
    if resp.secondary_clicked() {
        v.st.menu_at = resp.interact_pointer_pos().map(|pp| fx(pp.x));
    }
    let at = v.st.menu_at;
    resp.context_menu(|ui| {
        ui.set_min_width(280.0);
        action_items(ui, vals, v, mode, false, at);
    });
    // the scroll bar
    let p = pn.painter();
    p.rect_filled(sb, 4.0, Color32::from_gray(0x1c));
    let th = Rect::from_min_max(pos2(sb.min.x + v0 * sb.width(), sb.min.y), pos2(sb.min.x + v1 * sb.width(), sb.max.y));
    let sresp = pn.ui.interact(sb, pn.id("qs-sb", 0), Sense::drag());
    p.rect_filled(th, 4.0, Color32::from_gray(if sresp.dragged() { 0x80 } else { 0x58 }));
    if sresp.dragged() && span < 1.0 {
        let d = sresp.drag_delta().x / sb.width();
        let a0 = (v0 + d).clamp(0.0, 1.0 - span);
        v.st.view = Some((a0, a0 + span));
        v.st.manual = v.st.view;
    }
    // the pointer's parameter (or the pinned marker's)
    p.rect_filled(info, 4.0, Color32::from_gray(0x1c));
    let shown = v.st.pinned.or(v.st.drag.map(|d| d.0)).or(hover);
    let f2t = |f: f32| fmt_pos(v.st.units, (f * n as f32) as usize, a.rate, v.bpm);
    let text = match shown {
        Some(Mark::Start) => format!("Sample Start: {}", f2t(s)),
        Some(Mark::End) => format!("Sample End: {}", f2t(e)),
        Some(Mark::LoopStart) => format!("Loop Start: {}", f2t(ls)),
        Some(Mark::LoopEnd) => format!("Loop End: {}", f2t(le)),
        Some(Mark::LoopArea) => format!("Loop: {} – {}", f2t(ls), f2t(le)),
        Some(Mark::Xfade) => format!("Crossfade: {:.0} ms ({})", pn.get(qs::XFADE) * 1000.0, qs::XFADE_CURVES[pn.geti(qs::XFADE_CURVE).min(1)]),
        Some(Mark::FadeIn) | Some(Mark::FadeOut) => format!("Fade In: {:.0} ms    Fade Out: {:.0} ms", pn.get(qs::FADE_IN) * 1000.0, pn.get(qs::FADE_OUT) * 1000.0),
        Some(Mark::Slice(i)) => format!("Slice {}: {}{}", i + 1, v.slices.get(i).map(|&x| fmt_pos(v.st.units, x, a.rate, v.bpm)).unwrap_or_default(), if v.manual.get(i).copied().unwrap_or(false) { "  (ignores Sensitivity)" } else { "" }),
        None => format!("{}   {} Hz   {} ch   {:.2} s", name, a.rate, a.channels.len(), secs),
    };
    let tx = if v.st.pinned.is_some() {
        let xr = Rect::from_min_size(info.min + vec2(4.0, 4.0), vec2(18.0, 18.0));
        let xresp = pn.ui.interact(xr, pn.id("qs-unpin", 0), Sense::click()).on_hover_text("Back to showing the pointer's parameter");
        crate::glyphs::paint(&p, xr, crate::glyphs::ph::X, 12.0, if xresp.hovered() { Color32::WHITE } else { SKIN.dim });
        if xresp.clicked() {
            v.st.pinned = None;
        }
        info.min.x + 28.0
    } else {
        info.min.x + 10.0
    };
    p.text(pos2(tx, info.center().y), Align2::LEFT_CENTER, text, look::regular(11.5), if shown.is_some() { GOLD } else { SKIN.dim });
    if mode == 2 && v.st.pinned.is_some() {
        // the slice fades sit in the parameter bar of a clicked slice marker
        pn.field(SIZE.x - 300.0, info.min.y - pn.o.y + 2.0, 120.0, 22.0, qs::FADE_IN, 0.001);
        pn.field(SIZE.x - 170.0, info.min.y - pn.o.y + 2.0, 120.0, 22.0, qs::FADE_OUT, 0.001);
    }
    drops(pn, v, wr);
}

/// Files dragged over the display: the Original and Optimized drop zones.
fn drops(pn: &mut Pane, v: &mut View, wr: Rect) {
    let half = |k: usize| Rect::from_min_max(pos2(wr.min.x + wr.width() * k as f32 / 2.0, wr.min.y), pos2(wr.min.x + wr.width() * (k + 1) as f32 / 2.0, wr.max.y));
    if v.dragging_files {
        let p = pn.painter();
        let hov = pn.ui.ctx().pointer_hover_pos();
        for (k, label) in ["Quick Sampler (Original)", "Quick Sampler (Optimized)"].iter().enumerate() {
            let r = half(k).shrink(6.0);
            let on = hov.is_some_and(|h| r.contains(h));
            p.rect_filled(r, 6.0, if on { BLUE.gamma_multiply(0.35) } else { Color32::from_black_alpha(160) });
            p.rect_stroke(r, 6.0, Stroke::new(1.0, BLUE), egui::StrokeKind::Inside);
            p.text(r.center(), Align2::CENTER_CENTER, *label, look::semibold(13.0), Color32::WHITE);
        }
    }
    if let Some(f) = v.dropped.first().cloned() {
        let optimized = v.drop_pos.is_some_and(|d| half(1).contains(d));
        v.acts.push(Act::Load(f, optimized));
        v.dropped.clear();
    }
}

/// Parameter values the menus look at.
#[derive(Clone, Copy)]
struct Vals {
    loop_on: bool,
    xfade_curve: usize,
}

fn pn_vals(pn: &Pane) -> Vals {
    Vals { loop_on: pn.geti(qs::LOOP_MODE) > 0, xfade_curve: pn.geti(qs::XFADE_CURVE) }
}

fn delete_slice(ed: &SliceEdits, frame: usize, manual: bool) -> SliceEdits {
    let mut e = ed.clone();
    if manual {
        e.manual.retain(|&x| x != frame);
    } else {
        e.removed.push(frame);
    }
    e
}

/// The Action pop-up's (`full`) or the shortcut menu's items for the mode; `at` is where the
/// shortcut menu was opened (a fraction of the file).
fn action_items(ui: &mut egui::Ui, vals: Vals, v: &mut View, mode: usize, full: bool, at: Option<f32>) {
    fn cmd(ui: &mut egui::Ui, acts: &mut Vec<Act>, name: &'static str, enabled: bool) {
        if ui.add_enabled(enabled, egui::Button::new(name)).clicked() {
            acts.push(Act::Command(name));
            ui.close();
        }
    }
    let has = v.audio.is_some();
    let n = v.audio.as_ref().map(|a| a.frames()).unwrap_or(1).max(1);
    if mode == 2 {
        // the slice marker under the pointer (shortcut menu) or the last one hovered
        let near = at.and_then(|f| {
            let fr = (f * n as f32) as usize;
            v.slices.iter().enumerate().skip(1).min_by_key(|(_, s)| s.abs_diff(fr)).filter(|(_, s)| s.abs_diff(fr) < n / 100 + 1).map(|(i, _)| i)
        });
        if !full {
            if ui.add_enabled(has, egui::Button::new("Create Slice Marker")).clicked() {
                if let Some(f) = at {
                    let mut ed = v.edits.clone();
                    ed.manual.push((f * n as f32) as usize);
                    v.acts.push(Act::Edits(Some(ed)));
                }
                ui.close();
            }
            if let Some(i) = near {
                if ui.button("Delete Slice Marker").clicked() {
                    v.acts.push(Act::Edits(Some(delete_slice(&v.edits, v.slices[i], v.manual[i]))));
                    ui.close();
                }
                let manual = v.manual.get(i).copied().unwrap_or(false);
                if ui.add(egui::Button::selectable(manual, "Ignore Sensitivity")).clicked() && !manual {
                    let mut ed = v.edits.clone();
                    ed.manual.push(v.slices[i]);
                    v.acts.push(Act::Edits(Some(ed)));
                    ui.close();
                }
                if ui.add(egui::Button::selectable(!manual, "Respond to Sensitivity")).clicked() && manual {
                    let mut ed = v.edits.clone();
                    let fr = v.slices[i];
                    ed.manual.retain(|&x| x != fr);
                    v.acts.push(Act::Edits(Some(ed)));
                    ui.close();
                }
            }
            ui.separator();
        } else {
            cmd(ui, &mut v.acts, "Add Slice Marker", has && v.play.is_some());
            cmd(ui, &mut v.acts, "Delete Slice Marker", v.slices.len() > 1);
            cmd(ui, &mut v.acts, "Slice Marker ignores Sensitivity", v.slices.len() > 1);
            cmd(ui, &mut v.acts, "Slice Marker responds to Sensitivity", v.manual.iter().any(|m| *m));
            ui.separator();
        }
        cmd(ui, &mut v.acts, "Copy MIDI Pattern", has);
        cmd(ui, &mut v.acts, "Create Drum Machine Designer Track", has);
        cmd(ui, &mut v.acts, "Create Sampler Track", has);
        ui.separator();
        cmd(ui, &mut v.acts, "Crop Sample", has);
        cmd(ui, &mut v.acts, "Re-Analyze Transients and Tempo", has);
    } else {
        if mode == 0 {
            cmd(ui, &mut v.acts, "Auto-Loop", has);
            cmd(ui, &mut v.acts, "Auto-Loop within Loop Area", has && vals.loop_on);
            cmd(ui, &mut v.acts, "Optimize Loop Start", has && vals.loop_on);
            cmd(ui, &mut v.acts, "Optimize Loop Crossfade/End", has && vals.loop_on);
            ui.separator();
            cmd(ui, &mut v.acts, "Set Sample Start to Loop Start", has && vals.loop_on);
            cmd(ui, &mut v.acts, "Set Sample End to Loop End", has && vals.loop_on);
            cmd(ui, &mut v.acts, "Set Loop from Sample Start to Sample End", has);
            ui.separator();
            if ui.add(egui::Button::selectable(vals.xfade_curve == 0, "Equal Power Crossfade")).clicked() {
                v.acts.push(Act::Command("Equal Power Crossfade"));
                ui.close();
            }
            if ui.add(egui::Button::selectable(vals.xfade_curve == 1, "Linear Crossfade")).clicked() {
                v.acts.push(Act::Command("Linear Crossfade"));
                ui.close();
            }
            ui.separator();
            cmd(ui, &mut v.acts, "Crop Loop", has && vals.loop_on);
        }
        cmd(ui, &mut v.acts, "Crop Sample", has);
        ui.separator();
        cmd(ui, &mut v.acts, "Optimize Sample Gain", has);
        cmd(ui, &mut v.acts, "Retune", has);
        if full && mode == 0 {
            cmd(ui, &mut v.acts, "Derive Tempo from Loop Length", has && vals.loop_on);
        }
        cmd(ui, &mut v.acts, "Re-Analyze Transients and Tempo", has);
        ui.separator();
        cmd(ui, &mut v.acts, "Create Sampler Track", has);
    }
    if full {
        ui.separator();
        cmd(ui, &mut v.acts, "Reimport Original", v.file.is_some());
        cmd(ui, &mut v.acts, "Reimport Optimized", v.file.is_some());
        if mode == 0 {
            cmd(ui, &mut v.acts, "Write Sample Loop to Audio File", v.file.as_ref().is_some_and(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"))) && vals.loop_on);
        }
        cmd(ui, &mut v.acts, "Initialize Synth Parameters", true);
        ui.separator();
        for (k, u) in ["Display Seconds Units", "Display Sample Units", "Display Beats Units"].iter().enumerate() {
            if ui.add(egui::Button::selectable(v.st.units == k, *u)).clicked() {
                v.st.units = k;
                ui.close();
            }
        }
        ui.separator();
        if ui.add(egui::Button::selectable(!v.st.mono, "Display Stereo Channels")).clicked() {
            v.st.mono = false;
            ui.close();
        }
        if ui.add(egui::Button::selectable(v.st.mono, "Display Mono Sum")).clicked() {
            v.st.mono = true;
            ui.close();
        }
    }
}

/// The mode's parameters under the display, with Flex at the right.
fn mode_row(pn: &mut Pane, v: &mut View, mode: usize) {
    let y = 314.0;
    pn.group(10.0, y, SIZE.x - 20.0, 52.0, "");
    let lab = |pn: &Pane, x: f32, s: &str| pn.text(x, y + 13.0, Align2::LEFT_CENTER, s, 10.5, SKIN.dim);
    let fy = y + 22.0;
    let note = |pn: &mut Pane, x: f32, id: u32, label: &str| {
        lab(pn, x, label);
        pn.field(x, fy, 70.0, 22.0, id, 1.0);
    };
    match mode {
        0 | 1 => {
            note(pn, 20.0, qs::ROOT, "Root Key");
            lab(pn, 100.0, "Tune");
            pn.field(100.0, fy, 70.0, 22.0, qs::TUNE, 1.0);
            lab(pn, 180.0, "Playback");
            pn.seg(180.0, fy, 70.0, 22.0, qs::REVERSE, &["→", "←"], false);
            if mode == 0 {
                lab(pn, 260.0, "Loop");
                pn.menu(260.0, fy, 160.0, 22.0, qs::LOOP_MODE);
                if pn.geti(qs::LOOP_MODE) > 0 {
                    lab(pn, 430.0, "Xfade");
                    pn.field(430.0, fy, 80.0, 22.0, qs::XFADE, 0.001);
                    lab(pn, 520.0, "Curve");
                    pn.menu(520.0, fy, 110.0, 22.0, qs::XFADE_CURVE);
                }
            } else {
                lab(pn, 260.0, "Fade In");
                pn.field(260.0, fy, 80.0, 22.0, qs::FADE_IN, 0.001);
                lab(pn, 350.0, "Fade Out");
                pn.field(350.0, fy, 80.0, 22.0, qs::FADE_OUT, 0.001);
            }
        }
        2 => {
            lab(pn, 20.0, "Mode");
            pn.menu(20.0, fy, 130.0, 22.0, qs::SLICE_MODE);
            match pn.geti(qs::SLICE_MODE) {
                0 => {
                    lab(pn, 160.0, "Sensitivity");
                    pn.hfader(160.0, fy, 140.0, 22.0, qs::SENSITIVITY);
                }
                1 => {
                    lab(pn, 160.0, "Division");
                    pn.menu(160.0, fy, 100.0, 22.0, qs::DIVISION);
                }
                2 => {
                    lab(pn, 160.0, "Slices");
                    pn.field(160.0, fy, 70.0, 22.0, qs::SLICES, 1.0);
                }
                _ => {}
            }
            note(pn, 310.0, qs::START_KEY, "Start Key");
            lab(pn, 390.0, "Mapping");
            pn.menu(390.0, fy, 110.0, 22.0, qs::KEY_MAPPING);
            lab(pn, 510.0, "Gate");
            pn.toggle(510.0, fy, 50.0, 22.0, qs::GATE, "Gate");
            lab(pn, 570.0, "Play to End");
            pn.toggle(570.0, fy, 80.0, 22.0, qs::PLAY_TO_END, "On");
        }
        _ => {
            // Recorder: Record, Input, Monitor, Record Start, the meter with its threshold
            let (armed, running, level) = v.rec;
            let rr = pn.rect(20.0, fy - 6.0, 34.0, 34.0);
            let resp = pn.ui.interact(rr, pn.id("qs-rec", 0), Sense::click()).on_hover_text(if armed { "Stop recording" } else { "Record" });
            let p = pn.painter();
            p.circle_filled(rr.center(), 15.0, Color32::from_gray(if resp.hovered() { 0x48 } else { 0x38 }));
            p.circle_filled(rr.center(), 8.0, if armed && (running || (pn.ui.input(|i| i.time) * 2.0) as i64 % 2 == 0) { Color32::from_rgb(0xf0, 0x3c, 0x3c) } else { Color32::from_rgb(0x90, 0x30, 0x30) });
            if resp.clicked() {
                v.acts.push(Act::Record(!armed));
            }
            lab(pn, 70.0, "Input");
            let ir = pn.rect(70.0, fy, 150.0, 22.0);
            let iresp = pn.ui.interact(ir, pn.id("qs-input", 0), Sense::click());
            p.rect_filled(ir, 4.0, Color32::from_gray(0x22));
            p.with_clip_rect(ir).text(ir.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, &v.input, look::regular(10.5), SKIN.text);
            egui::Popup::menu(&iresp).show(|ui| {
                for (name, c) in &v.inputs {
                    if ui.add(egui::Button::selectable(*name == v.input, name.as_str())).clicked() {
                        v.acts.push(Act::Input(*c));
                        ui.close();
                    }
                }
            });
            lab(pn, 230.0, "Monitor");
            pn.toggle(230.0, fy, 60.0, 22.0, qs::MONITOR, "Monitor");
            lab(pn, 300.0, "Record Start");
            pn.menu(300.0, fy, 220.0, 22.0, qs::RECORD_START);
            // the meter with the threshold slider over it
            lab(pn, 530.0, "Threshold");
            let mr = pn.rect(530.0, fy + 4.0, 180.0, 14.0);
            p.rect_filled(mr, 3.0, Color32::from_gray(0x16));
            let db = 20.0 * level.max(1e-6).log10();
            let lf = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
            p.rect_filled(Rect::from_min_size(mr.min, vec2(mr.width() * lf, mr.height())), 3.0, if db > -3.0 { Color32::from_rgb(0xe0, 0x50, 0x40) } else { Color32::from_rgb(0x4c, 0xc0, 0x60) });
            pn.hfader(530.0, fy, 180.0, 22.0, qs::THRESHOLD);
            if armed {
                p.text(pos2(pn.o.x + 720.0, pn.o.y + fy + 11.0), Align2::LEFT_CENTER, if running { "Recording…" } else { "Waiting for the threshold…" }, look::regular(11.0), GOLD);
            }
        }
    }
    if mode < 3 {
        // Flex
        let fx = SIZE.x - 330.0;
        lab(pn, fx, "Flex");
        pn.toggle(fx, fy, 50.0, 22.0, qs::FLEX, "Flex");
        lab(pn, fx + 60.0, "Follow Tempo");
        pn.toggle(fx + 60.0, fy, 100.0, 22.0, qs::FOLLOW_TEMPO, "Follow Tempo");
        lab(pn, fx + 170.0, "Speed");
        pn.menu(fx + 170.0, fy, 80.0, 22.0, qs::FLEX_SPEED);
        if !pn.on(qs::FLEX) {
            let r = pn.rect(fx + 58.0, fy - 2.0, 196.0, 26.0);
            pn.ui.painter().rect_filled(r, 4.0, SKIN.panel.gamma_multiply(0.6));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The lower section: modulation, Pitch, Filter, Amp

fn header(pn: &Pane, x: f32, y: f32, w: f32, title: &str) {
    let r = pn.rect(x, y, w, 24.0);
    pn.ui.painter().rect_filled(r, egui::CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 }, Color32::from_rgb(0x40, 0x4f, 0x5d));
    pn.ui.painter().text(r.center(), Align2::CENTER_CENTER, title, look::regular(12.5), SKIN.text);
}

fn lower(pn: &mut Pane, v: &mut View) {
    let y = 376.0;
    let h = 284.0;
    let cols = [(10.0, 300.0), (318.0, 210.0), (536.0, 250.0), (794.0, 206.0)];
    for (k, &(x, w)) in cols.iter().enumerate() {
        pn.ui.painter().rect_filled(pn.rect(x, y, w, h), 7.0, if k == 0 { NAVY } else { SKIN.panel });
    }
    modulation(pn, v, cols[0].0, y, cols[0].1, h);
    // Pitch
    let (x, w) = cols[1];
    header(pn, x, y, w, "PITCH");
    pn.knob(x + 38.0, y + 32.0, 42.0, qs::COARSE, "Coarse");
    pn.knob(x + 105.0, y + 32.0, 42.0, qs::FINE, "Fine");
    pn.knob(x + 172.0, y + 32.0, 42.0, ss::GLIDE, "Glide");
    pn.knob(x + 38.0, y + 92.0, 32.0, qs::PITCH_ENV_DEPTH, "Env Depth");
    pn.text(x + 90.0, y + 100.0, Align2::LEFT_CENTER, "Bend Range", 10.5, SKIN.dim);
    pn.field(x + 90.0, y + 110.0, 70.0, 20.0, qs::BEND, 1.0);
    envelope(pn, x, y + 146.0, w, ss::env_base(qs::PITCH_ENV) + 1, "ENV");
    // Filter
    let (x, w) = cols[2];
    header(pn, x, y, w, "");
    pn.toggle(x + 8.0, y + 3.0, 60.0, 18.0, qs::FILTER_ON, "FILTER");
    pn.menu(x + 76.0, y + 3.0, w - 84.0, 18.0, ss::F1_TYPE);
    pn.knob(x + 42.0, y + 32.0, 42.0, qs::CUTOFF, "Cutoff");
    pn.knob(x + 125.0, y + 32.0, 42.0, qs::RESO, "Reso");
    pn.knob(x + 208.0, y + 32.0, 42.0, ss::F1_DRIVE, "Drive");
    pn.knob(x + 70.0, y + 92.0, 32.0, qs::FILTER_ENV_DEPTH, "Env Depth");
    pn.knob(x + 180.0, y + 92.0, 32.0, qs::KEYSCALE, "Keyscale");
    envelope(pn, x, y + 146.0, w, ss::env_base(qs::FILTER_ENV) + 1, "ENV");
    if !pn.on(qs::FILTER_ON) {
        pn.ui.painter().rect_filled(pn.rect(x, y + 24.0, w, h - 24.0), 0.0, SKIN.panel.gamma_multiply(0.55));
    }
    // Amp
    let (x, w) = cols[3];
    header(pn, x, y, w, "AMP");
    pn.knob(x + 45.0, y + 32.0, 42.0, qs::VOLUME, "Volume");
    pn.knob(x + 115.0, y + 32.0, 42.0, ss::PAN, "Pan");
    pn.text(x + 150.0, y + 42.0, Align2::LEFT_CENTER, "Polyphony", 10.5, SKIN.dim);
    pn.menu(x + 150.0, y + 54.0, 50.0, 20.0, qs::POLY);
    envelope(pn, x, y + 146.0, w, ss::ENV1, "ENV");
}

/// An envelope: the type pop-up, the display (drag the points: times sideways, sustain up and
/// down), the values (drag or double-click to type) and Vel; `b` is its type id.
fn envelope(pn: &mut Pane, x: f32, y: f32, w: f32, b: u32, title: &str) {
    pn.text(x + 10.0, y + 10.0, Align2::LEFT_CENTER, title, 11.0, SKIN.dim);
    pn.menu(x + w - 88.0, y, 80.0, 20.0, b);
    let ar = pn.geti(b) == 1;
    let r = pn.rect(x + 8.0, y + 24.0, w - 44.0, 60.0);
    let p = pn.painter();
    p.rect_filled(r, 4.0, NAVY);
    let (a, h, d, s, rl) = (pn.get(b + 2), pn.get(b + 4), pn.get(b + 5), pn.get(b + 6), pn.get(b + 7));
    let total = (a + if ar { 0.0 } else { h + d } + rl + 0.5).max(0.6);
    let sx = (r.width() - 8.0) / total;
    let yv = |lv: f32| r.max.y - 4.0 - lv * (r.height() - 8.0);
    let x0 = r.min.x + 4.0;
    let xa = x0 + a * sx;
    let xh = xa + if ar { 0.0 } else { h * sx };
    let xd = xh + if ar { 0.0 } else { d * sx };
    let sus = if ar { 1.0 } else { s };
    let xs = xd + 0.5 * sx;
    let xr = xs + rl * sx;
    p.add(egui::Shape::line(vec![pos2(x0, yv(0.0)), pos2(xa, yv(1.0)), pos2(xh, yv(1.0)), pos2(xd, yv(sus)), pos2(xs, yv(sus)), pos2(xr, yv(0.0))], Stroke::new(1.6, BLUE)));
    // handles: 0 attack, 1 hold, 2 decay / sustain, 3 release
    let handles = [(xa, yv(1.0)), (xh, yv(1.0)), (xd, yv(sus)), (xr, yv(0.0))];
    for (k, (hx, hy)) in handles.iter().enumerate() {
        if ar && (k == 1 || k == 2) {
            continue;
        }
        p.circle_filled(pos2(*hx, *hy), 3.5, BLUE);
    }
    let resp = pn.ui.interact(r, pn.id("qs-env", b), Sense::drag());
    let part_id = pn.id("qs-env-part", b);
    if resp.drag_started()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let start = pp - resp.drag_delta();
        let k = handles.iter().enumerate().filter(|(k, _)| !(ar && (*k == 1 || *k == 2))).min_by(|a, b| (pos2(a.1.0, a.1.1) - start).length().partial_cmp(&(pos2(b.1.0, b.1.1) - start).length()).unwrap_or(std::cmp::Ordering::Equal)).map(|h| h.0).unwrap_or(0);
        pn.ui.data_mut(|dd| dd.insert_temp(part_id, k));
    }
    if resp.dragged() {
        let k: usize = pn.ui.data(|dd| dd.get_temp(part_id)).unwrap_or(0);
        let dt = resp.drag_delta().x / sx;
        match k {
            0 => pn.put(b + 2, (a + dt).clamp(0.0, 10.0)),
            1 => pn.put(b + 4, (h + dt).clamp(0.0, 10.0)),
            2 => {
                pn.put(b + 5, (d + dt).clamp(0.0, 20.0));
                pn.put(b + 6, (s - resp.drag_delta().y / (r.height() - 8.0)).clamp(0.0, 1.0));
            }
            _ => pn.put(b + 7, (rl + dt).clamp(0.0, 20.0)),
        }
    }
    // Vel at the right of the display
    pn.vfader(x + w - 32.0, y + 24.0, 22.0, 60.0, b + 8, "Vel");
    // the values
    let fields: &[(&str, u32)] = if ar { &[("A", 2), ("R", 7)] } else { &[("A", 2), ("H", 4), ("D", 5), ("S", 6), ("R", 7)] };
    let fw = (w - 16.0) / fields.len() as f32;
    for (k, (l, off)) in fields.iter().enumerate() {
        let fx = x + 8.0 + fw * k as f32;
        pn.text(fx + fw / 2.0, y + 93.0, Align2::CENTER_CENTER, l, 10.0, SKIN.dim);
        pn.field(fx + 1.0, y + 101.0, fw - 2.0, 20.0, b + off, 0.005);
    }
}

/// The modulation pane: LFO 1, LFO 2 and Mod Matrix tabs.
fn modulation(pn: &mut Pane, v: &mut View, x: f32, y: f32, w: f32, _h: f32) {
    let tabs = ["LFO 1", "LFO 2", "MOD MATRIX"];
    let tw = (w - 16.0) / 3.0;
    for (k, tname) in tabs.iter().enumerate() {
        let r = pn.rect(x + 8.0 + tw * k as f32, y + 6.0, tw - 2.0, 22.0);
        if pn.button(r, tname, v.st.mod_tab == k, ("qs-modtab", k)) {
            v.st.mod_tab = k;
        }
    }
    let y = y + 36.0;
    if v.st.mod_tab == 2 {
        // four routings: on, source, target, amount
        pn.text(x + 40.0, y + 4.0, Align2::LEFT_CENTER, "SOURCE", 10.0, SKIN.dim);
        pn.text(x + 150.0, y + 4.0, Align2::LEFT_CENTER, "TARGET", 10.0, SKIN.dim);
        pn.text(x + 236.0, y + 4.0, Align2::LEFT_CENTER, "AMOUNT", 10.0, SKIN.dim);
        for r in 0..qs::ROUTINGS {
            let b = ss::mod_base(r);
            let ry = y + 14.0 + r as f32 * 56.0;
            pn.toggle(x + 8.0, ry + 4.0, 26.0, 20.0, b, "On");
            pn.menu(x + 40.0, ry + 4.0, 104.0, 20.0, b + 1);
            pn.menu(x + 150.0, ry + 4.0, 82.0, 20.0, b + 5);
            pn.toggle(x + 40.0, ry + 28.0, 40.0, 18.0, b + 2, "Inv");
            if pn.geti(b + 3) > 0 {
                pn.range(x + 236.0, ry + 4.0, 56.0, 20.0, b + 6, b + 7, "", GOLD);
            } else {
                pn.hfader(x + 236.0, ry + 4.0, 56.0, 20.0, b + 6);
            }
            pn.text(x + 90.0, ry + 37.0, Align2::LEFT_CENTER, "Via", 10.0, SKIN.dim);
            pn.menu(x + 112.0, ry + 28.0, 120.0, 18.0, b + 3);
        }
        return;
    }
    let k = v.st.mod_tab + 1;
    let b = ss::lfo_base(k);
    let rt = qs::lfo_route(k);
    pn.toggle(x + 8.0, y, 34.0, 20.0, b, "On");
    pn.menu(x + 48.0, y, 130.0, 20.0, b + 1);
    pn.toggle(x + 184.0, y, 40.0, 20.0, b + 2, "♪");
    if pn.on(b + 2) {
        pn.menu(x + 230.0, y, 62.0, 20.0, b + 4);
    } else {
        pn.field(x + 230.0, y, 62.0, 20.0, b + 3, 0.05);
    }
    // the wave
    let r = pn.rect(x + 8.0, y + 26.0, w - 16.0, 50.0);
    let set = ss::LfoSet { on: true, wave: pn.geti(b + 1) as u8, sync: false, hz: 1.0, beats: 1.0, fade: 0.0, fade_out: false, phase: pn.get(b + 7) / 360.0, poly: true, bipolar: pn.on(b + 9), key_trigger: true };
    let mut l = ss::Lfo::default();
    l.restart(&set, 12345);
    let pts: Vec<(f32, f32)> = (0..=120).map(|i| (i as f32 / 120.0, 0.5 + 0.45 * l.advance(&set, 2.0, 1.0 / 120.0))).collect();
    pn.ui.painter().rect_filled(r, 4.0, Color32::from_rgb(0x08, 0x1c, 0x2d));
    pn.curve(r, &pts, BLUE);
    let lb = |pn: &Pane, xx: f32, yy: f32, s: &str| pn.text(xx, yy, Align2::LEFT_CENTER, s, 10.0, SKIN.dim);
    let y2 = y + 84.0;
    lb(pn, x + 8.0, y2 + 6.0, "Fade");
    pn.field(x + 40.0, y2, 64.0, 20.0, b + 5, 0.01);
    pn.seg(x + 108.0, y2, 76.0, 20.0, b + 6, &["In", "Out"], false);
    lb(pn, x + 192.0, y2 + 6.0, "Phase");
    pn.field(x + 230.0, y2, 62.0, 20.0, b + 7, 1.0);
    let y3 = y2 + 26.0;
    pn.seg(x + 8.0, y3, 90.0, 20.0, b + 8, &["Mono", "Poly"], false);
    pn.seg(x + 104.0, y3, 110.0, 20.0, b + 9, &["Unipolar", "Bipolar"], false);
    pn.toggle(x + 220.0, y3, 72.0, 20.0, b + 10, "Key Trigger");
    // the LFO's own routing: amount (two handles with a via), via, target
    let y4 = y3 + 30.0;
    lb(pn, x + 8.0, y4 + 6.0, "Target");
    pn.menu(x + 56.0, y4, 120.0, 20.0, rt + 3);
    lb(pn, x + 182.0, y4 + 6.0, "Via");
    pn.menu(x + 206.0, y4, 86.0, 20.0, rt + 1);
    lb(pn, x + 8.0, y4 + 36.0, "Amount");
    if pn.geti(rt + 1) > 0 {
        pn.range(x + 56.0, y4 + 26.0, w - 64.0, 20.0, rt, rt + 2, "", GOLD);
    } else {
        pn.hfader(x + 56.0, y4 + 26.0, w - 64.0, 20.0, rt);
    }
}

// ---------------------------------------------------------------------------------------------
// After drawing: files, the Action menu, recording

fn load(app: &mut App, t: usize, path: &std::path::Path, optimized: bool) {
    let a = match lg_synth::wav::read_audio(path) {
        Ok(a) => a,
        Err(e) => {
            app.tell(format!("Couldn't load {}: {e}", path.display()));
            return;
        }
    };
    let Some(id) = instwin::track_id(app, t) else { return };
    let (a, pairs) = analyse(a, optimized);
    instwin::checkpoint(app, t, "Load Audio File");
    let mut d = app.inst.quick.get(&id).cloned().unwrap_or_default();
    d.history.retain(|f| f != path);
    d.history.insert(0, path.to_path_buf());
    d.history.truncate(12);
    d.file = Some(path.to_path_buf());
    d.audio = Some(Arc::new(a));
    d.edits = None;
    app.inst.quick.insert(id, d);
    instwin::bump(app, t);
    instwin::set_params(app, t, &pairs);
}

/// The settings a loaded file brings: Original keeps its root, loop and length; Optimized crops
/// silence, finds the pitch and tuning, the gain and (for sustained or rhythmic material) a loop.
fn analyse(a: Audio, optimized: bool) -> (Audio, Vec<(u32, f32)>) {
    let mut a = a;
    if optimized {
        let (s, e) = qs::sound_bounds(&a);
        for c in &mut a.channels {
            *c = c[s.min(c.len())..e.min(c.len())].to_vec();
        }
        a.loop_points = a.loop_points.and_then(|(ls, le)| (ls as usize >= s && (le as usize) <= e).then(|| ((ls as usize - s) as u32, (le as usize - s) as u32)));
    }
    let mut q = QuickSampler::new(a.rate as f32);
    q.set_audio(a.clone());
    let n = a.frames().max(1) as f32;
    let mut pairs: Vec<(u32, f32)> = vec![(qs::START, 0.0), (qs::END, 1.0), (qs::ROOT, q.get(qs::ROOT)), (qs::FILE_TEMPO, q.get(qs::FILE_TEMPO)), (qs::TUNE, 0.0)];
    match a.loop_points {
        Some((ls, le)) => pairs.extend([(qs::LOOP_MODE, 1.0), (qs::LOOP_START, ls as f32 / n), (qs::LOOP_END, le as f32 / n)]),
        None => pairs.push((qs::LOOP_MODE, 0.0)),
    }
    if optimized {
        if let Some(m) = qs::detect_pitch(&a) {
            pairs.push((qs::ROOT, m.round()));
            pairs.push((qs::TUNE, ((m.round() - m) * 100.0).clamp(-50.0, 50.0)));
        }
        pairs.push((qs::VOLUME, qs::optimize_gain(&a, 0, a.frames())));
        if a.loop_points.is_none() {
            if q.get(qs::FILE_TEMPO) > 0.0 {
                // rhythmic: the whole file loops
                pairs.extend([(qs::LOOP_MODE, 1.0), (qs::LOOP_START, 0.0), (qs::LOOP_END, 1.0), (qs::XFADE, 0.005)]);
            } else if a.frames() as f32 / a.rate as f32 > 0.6
                && let Some((ls, le)) = qs::auto_loop(&a, 0, a.frames(), None, 0)
            {
                pairs.extend([(qs::LOOP_MODE, 1.0), (qs::LOOP_START, ls as f32 / n), (qs::LOOP_END, le as f32 / n), (qs::XFADE, qs::optimize_crossfade(&a, ls, le))]);
            }
        }
    }
    (a, pairs)
}

fn run(app: &mut App, ctx: &egui::Context, t: usize, a: Act, audio: Option<Arc<Audio>>) {
    let Some(id) = instwin::track_id(app, t) else { return };
    match a {
        Act::Load(p, opt) => load(app, t, &p, opt),
        Act::LoadDialog(optimized) => {
            // (Logic's Original / Optimized options: in the pop-up, as the file dialog is the system's)
            let dlg = kn_ui::files::FileDialog::new().set_title(if optimized { "Load Audio File (Optimized)" } else { "Load Audio File (Original)" }).add_filter("Audio", &["wav", "aif", "aiff", "caf", "flac", "WAV", "AIF", "AIFF"]);
            if let Some(p) = dlg.pick_file() {
                load(app, t, &p, optimized);
            }
        }
        Act::Rename(name) => {
            let Some(d) = app.inst.quick.get(&id).cloned() else { return };
            let Some(f) = d.file.clone() else { return };
            let ext = f.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
            let to = f.with_file_name(format!("{}{ext}", name.trim()));
            if name.trim().is_empty() || to.exists() {
                app.tell("A file of that name is there already");
                return;
            }
            match std::fs::rename(&f, &to) {
                Ok(()) => {
                    let mut d = d;
                    for h in &mut d.history {
                        if *h == f {
                            *h = to.clone();
                        }
                    }
                    d.file = Some(to);
                    app.inst.quick.insert(id, d);
                }
                Err(e) => app.tell(format!("Couldn't rename the file: {e}")),
            }
        }
        Act::RemoveFile => {
            instwin::checkpoint(app, t, "Remove Current File");
            let mut d = app.inst.quick.get(&id).cloned().unwrap_or_default();
            if let Some(f) = d.file.take() {
                d.history.retain(|h| *h != f);
            }
            d.audio = None;
            d.edits = None;
            app.inst.quick.insert(id, d);
            // a fresh instrument plays nothing
            app.inst.rev.remove(&id);
            app.load_engine();
            instwin::bump(app, t);
        }
        Act::ClearHistory => {
            if let Some(d) = app.inst.quick.get_mut(&id) {
                let keep = d.file.clone();
                d.history.retain(|h| Some(h) == keep.as_ref());
            }
        }
        Act::ShowFolder => {
            if let Some(dir) = app.inst.quick.get(&id).and_then(|d| d.file.as_ref()).and_then(|f| f.parent().map(|p| p.to_path_buf())) {
                let _ = std::process::Command::new("xdg-open").arg(dir).stdin(std::process::Stdio::null()).spawn();
            }
        }
        Act::Edits(e) => {
            instwin::checkpoint_once(app, ctx, t, "Edit Slice Markers");
            let d = app.inst.quick.entry(id).or_default();
            if d.audio.is_none() {
                d.audio = audio;
            }
            d.edits = e.clone();
            instwin::with_inst(app, t, |q: &mut QuickSampler| q.set_slice_edits(e.unwrap_or_default()));
        }
        Act::Play(k) => crate::keyswin::audition(app, t, k, 100),
        Act::Command(c) => command_on(app, t, c, audio),
        Act::Record(on) => {
            if on {
                if crate::sidechain::choice(app, t).is_none() {
                    crate::sidechain::set(app, t, Some(crate::sidechain::Choice::Input));
                }
                instwin::with_inst(app, t, |q: &mut QuickSampler| q.record_start(300.0));
            } else if let Some(a) = instwin::with_inst(app, t, |q: &mut QuickSampler| q.record_stop()).flatten() {
                instwin::checkpoint(app, t, "Record");
                let d = app.inst.quick.entry(id).or_default();
                d.file = None;
                d.audio = Some(Arc::new(a));
                d.edits = None;
                instwin::bump(app, t);
            }
        }
        Act::Input(c) => crate::sidechain::set(app, t, c),
    }
}

/// Runs an Action menu command on the track's Quick Sampler.
fn command_on(app: &mut App, t: usize, c: &str, audio: Option<Arc<Audio>>) {
    let Some(id) = instwin::track_id(app, t) else { return };
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let get = |pid: u32| params.iter().position(|p| p.id == pid).map(|i| vals[i]).unwrap_or(0.0);
    let set = |app: &mut App, what: &str, pairs: &[(u32, f32)]| {
        instwin::checkpoint(app, t, what);
        instwin::set_params(app, t, pairs);
    };
    let file = app.inst.quick.get(&id).and_then(|d| d.file.clone());
    if c == "Initialize Synth Parameters" {
        let keep = [qs::MODE, qs::ROOT, qs::START, qs::END, qs::LEGACY_LOOP, qs::LOOP_START, qs::LOOP_END, qs::SENSITIVITY, qs::PITCH_TRACK, qs::TUNE, qs::REVERSE, qs::LOOP_MODE, qs::FADE_IN, qs::FADE_OUT, qs::XFADE, qs::XFADE_CURVE, qs::SLICE_MODE, qs::DIVISION, qs::SLICES, qs::START_KEY, qs::KEY_MAPPING, qs::GATE, qs::PLAY_TO_END, qs::MONITOR, qs::RECORD_START, qs::THRESHOLD, qs::FLEX, qs::FOLLOW_TEMPO, qs::FLEX_SPEED, qs::FILE_TEMPO];
        let d: Vec<(u32, f32)> = params.iter().filter(|p| !keep.contains(&p.id) && !(7..=10).contains(&p.id)).map(|p| (p.id, p.default)).collect();
        set(app, c, &d);
        return;
    }
    if c == "Reimport Original" || c == "Reimport Optimized" {
        if let Some(f) = file {
            load(app, t, &f, c == "Reimport Optimized");
        }
        return;
    }
    let Some(a) = audio else { return };
    let n = a.frames().max(1);
    let nf = n as f32;
    let (s, e) = ((get(qs::START) * nf) as usize, ((get(qs::END) * nf) as usize).max(1));
    let (ls, le) = ((get(qs::LOOP_START) * nf) as usize, (get(qs::LOOP_END) * nf) as usize);
    let attempt = {
        let st = &mut app.inst.quick_ui;
        let again = c.starts_with("Auto-Loop");
        st.attempt = if again { st.attempt + 1 } else { 0 };
        st.attempt.saturating_sub(1)
    };
    let slices = instwin::with_inst(app, t, |q: &mut QuickSampler| (q.slices.clone(), q.slice_manual.clone(), q.edits.clone(), q.play_position())).unwrap_or_default();
    let set_edits = |app: &mut App, e: SliceEdits| {
        instwin::checkpoint(app, t, c);
        let d = app.inst.quick.entry(id).or_default();
        d.edits = Some(e.clone());
        instwin::with_inst(app, t, |q: &mut QuickSampler| q.set_slice_edits(e));
    };
    let crop = |app: &mut App, a0: usize, a1: usize, pairs: Vec<(u32, f32)>| {
        let mut na = (*a).clone();
        for ch in &mut na.channels {
            *ch = ch[a0.min(ch.len())..a1.min(ch.len())].to_vec();
        }
        instwin::checkpoint(app, t, c);
        let d = app.inst.quick.entry(id).or_default();
        d.audio = Some(Arc::new(na));
        d.edits = None;
        instwin::bump(app, t);
        instwin::set_params(app, t, &pairs);
    };
    match c {
        "Auto-Loop" | "Auto-Loop within Loop Area" => {
            let within = (c != "Auto-Loop").then_some((ls, le));
            match qs::auto_loop(&a, s, e, within, attempt) {
                Some((x, y)) => set(app, c, &[(qs::LOOP_MODE, if get(qs::LOOP_MODE) < 0.5 { 1.0 } else { get(qs::LOOP_MODE) }), (qs::LOOP_START, x as f32 / nf), (qs::LOOP_END, y as f32 / nf), (qs::XFADE, qs::optimize_crossfade(&a, x, y))]),
                None => app.tell("No loop found"),
            }
        }
        "Optimize Loop Start" => set(app, c, &[(qs::LOOP_START, qs::optimize_loop_start(&a, ls, le) as f32 / nf)]),
        "Optimize Loop Crossfade/End" => set(app, c, &[(qs::XFADE, qs::optimize_crossfade(&a, ls, le))]),
        "Set Sample Start to Loop Start" => set(app, c, &[(qs::START, get(qs::LOOP_START))]),
        "Set Sample End to Loop End" => set(app, c, &[(qs::END, get(qs::LOOP_END))]),
        "Set Loop from Sample Start to Sample End" => set(app, c, &[(qs::LOOP_MODE, if get(qs::LOOP_MODE) < 0.5 { 1.0 } else { get(qs::LOOP_MODE) }), (qs::LOOP_START, get(qs::START)), (qs::LOOP_END, get(qs::END))]),
        "Equal Power Crossfade" => set(app, c, &[(qs::XFADE_CURVE, 0.0)]),
        "Linear Crossfade" => set(app, c, &[(qs::XFADE_CURVE, 1.0)]),
        "Crop Sample" => {
            let len = (e - s).max(1) as f32;
            let lp = |x: usize| ((x as f32 - s as f32) / len).clamp(0.0, 1.0);
            crop(app, s, e, vec![(qs::START, 0.0), (qs::END, 1.0), (qs::LOOP_START, lp(ls)), (qs::LOOP_END, lp(le))]);
        }
        "Crop Loop" => crop(app, ls, le.max(ls + 1), vec![(qs::START, 0.0), (qs::END, 1.0), (qs::LOOP_START, 0.0), (qs::LOOP_END, 1.0)]),
        "Optimize Sample Gain" => set(app, c, &[(qs::VOLUME, qs::optimize_gain(&a, s, e))]),
        "Retune" => {
            let part = Audio { rate: a.rate, channels: a.channels.iter().map(|ch| ch[s.min(ch.len())..e.min(ch.len())].to_vec()).collect(), loop_points: None, root: None };
            match qs::detect_pitch(&part) {
                Some(m) => set(app, c, &[(qs::ROOT, m.round()), (qs::TUNE, ((m.round() - m) * 100.0).clamp(-50.0, 50.0))]),
                None => app.tell("No pitch found in the sample"),
            }
        }
        "Derive Tempo from Loop Length" => match qs::tempo_from_length(le.saturating_sub(ls), a.rate) {
            Some(b) => {
                set(app, c, &[(qs::FILE_TEMPO, b)]);
                app.tell(format!("The sample's tempo: {b:.2} BPM"));
            }
            None => app.tell("The loop is too short for a tempo"),
        },
        "Re-Analyze Transients and Tempo" => {
            set(app, c, &[(qs::FILE_TEMPO, qs::detect_tempo(&a).unwrap_or(0.0))]);
            set_edits(app, SliceEdits::default());
        }
        "Write Sample Loop to Audio File" => {
            let Some(f) = file else { return };
            if lg_synth::wav::read_audio(&f).map(|x| x.frames()).ok() != Some(n) {
                app.tell("The sample was edited here; its file no longer matches");
                return;
            }
            match qs::write_wav_loop(&f, get(qs::ROOT).round() as u8, (ls as u32, le as u32)) {
                Ok(()) => app.tell(format!("Wrote the loop into {}", file_name(&f))),
                Err(e) => app.tell(format!("Couldn't write the loop: {e}")),
            }
        }
        "Add Slice Marker" => {
            if let Some(pp) = slices.3 {
                let mut ed = slices.2.clone();
                ed.manual.push(pp);
                set_edits(app, ed);
            }
        }
        "Delete Slice Marker" | "Slice Marker ignores Sensitivity" | "Slice Marker responds to Sensitivity" => {
            // the marker last pointed at (else the last one)
            let (sl, man, ed, _) = slices;
            let at = app.inst.quick_ui.menu_at.map(|f| (f * nf) as usize);
            let Some(i) = at.and_then(|fr| sl.iter().enumerate().skip(1).min_by_key(|(_, x)| x.abs_diff(fr)).map(|(i, _)| i)).or((sl.len() > 1).then(|| sl.len() - 1)) else { return };
            let m = man.get(i).copied().unwrap_or(false);
            let mut ed = ed;
            match c {
                "Delete Slice Marker" => ed = delete_slice(&ed, sl[i], m),
                "Slice Marker ignores Sensitivity" if !m => ed.manual.push(sl[i]),
                "Slice Marker responds to Sensitivity" if m => ed.manual.retain(|&x| x != sl[i]),
                _ => return,
            }
            set_edits(app, ed);
        }
        "Copy MIDI Pattern" => {
            let (sl, ..) = slices;
            app.clipboard = Some(pattern(app, &sl, s, e, a.rate, get(qs::START_KEY).round() as u8, get(qs::KEY_MAPPING).round() as u8, 0));
            app.tell(format!("Copied the slice pattern ({} notes); paste it on a MIDI track", sl.iter().filter(|&&x| x >= s && x < e).count()));
        }
        "Create Drum Machine Designer Track" => {
            let (sl, ..) = slices;
            create_dmd(app, t, &a, &sl, s, e);
        }
        "Create Sampler Track" => create_sampler(app, t, &a, &params, &vals),
        _ => {}
    }
}

/// The slice pattern as a MIDI region: a note per slice between the start and end markers, at
/// the project tempo.
#[allow(clippy::too_many_arguments)]
fn pattern(app: &App, slices: &[usize], s: usize, e: usize, rate: u32, start_key: u8, mapping: u8, at: u32) -> lg_core::Region {
    let bpm = app.tempo().max(1.0);
    let ticks = |frames: usize| (frames as f64 / rate.max(1) as f64 * bpm / 60.0 * lg_core::events::PPQ as f64).round() as u32;
    let keys = qs::slice_keys(start_key, mapping, slices.len());
    let mut notes = Vec::new();
    for (k, &x) in slices.iter().enumerate() {
        if x < s || x >= e {
            continue;
        }
        let Some(&key) = keys.get(k) else { break };
        let next = slices.get(k + 1).copied().unwrap_or(e).min(e);
        notes.push(lg_core::events::Note { channel: 0, pitch: key, velocity: 100, pos: at + ticks(x - s), len: ticks(next - x).max(10), muted: false });
    }
    lg_core::Region { name: "Slice Pattern".into(), start: at, length: ticks(e - s).max(1), notes, audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() }
}

fn new_track(app: &mut App, name: &str) -> Option<usize> {
    let p = app.project.as_mut()?;
    let mut tr = lg_core::Track::new(name, lg_core::TrackKind::Instrument);
    tr.row = p.tracks.len() as u32 + 1;
    p.tracks.push(tr);
    app.programs.push(crate::app::program_for(name, lg_core::TrackKind::Instrument));
    crate::trackarea::sync(app);
    Some(app.project.as_ref()?.tracks.len() - 1)
}

/// Create Drum Machine Designer Track: each slice on its own pad, and a region of the slice
/// notes at the playhead's bar.
fn create_dmd(app: &mut App, t: usize, a: &Audio, slices: &[usize], s: usize, e: usize) {
    let name = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    app.checkpoint("Create Drum Machine Designer Track");
    let Some(nt) = new_track(app, &format!("{name} Slices")) else { return };
    let Some(nid) = instwin::track_id(app, nt) else { return };
    let mut pads = crate::dmdwin::Pads::default();
    let inside: Vec<usize> = slices.iter().copied().filter(|&x| x >= s && x < e).collect();
    for (k, &x) in inside.iter().enumerate().take(48) {
        let y = inside.get(k + 1).copied().unwrap_or(e);
        let piece = Audio { rate: a.rate, channels: a.channels.iter().map(|c| c[x.min(c.len())..y.min(c.len())].to_vec()).collect(), loop_points: None, root: None };
        pads.pads.insert(36 + k as u8, crate::dmdwin::PadDoc { name: format!("Slice {}", k + 1), file: None, audio: Some(Arc::new(piece)), quick: Vec::new() });
    }
    app.instrument_choice.insert(nt, "Drum Machine Designer".into());
    app.inst.pads.insert(nid, pads);
    let bar = app.bar_ticks().max(1);
    let at = (app.position().max(0.0) as u32 / bar) * bar;
    let rg = pattern(app, &inside, s, e, a.rate, 36, 0, at);
    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(nt)) {
        tr.regions.push(rg);
    }
    app.selected_track = Some(nt);
    app.load_engine();
    app.after_edit();
}

/// Create Sampler Track: a new track whose Sampler has the Quick Sampler's sound as a group of
/// zones (one, or one per slice) and its synth settings.
fn create_sampler(app: &mut App, t: usize, a: &Audio, params: &[lg_dsp::Param], vals: &[f32]) {
    let Some(zones) = instwin::with_inst(app, t, |q: &mut QuickSampler| q.zones().to_vec()) else { return };
    let name = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    let Some(id) = instwin::track_id(app, t) else { return };
    let path = app.inst.quick.get(&id).and_then(|d| d.file.clone()).unwrap_or_default();
    app.checkpoint("Create Sampler Track");
    let Some(nt) = new_track(app, &name) else { return };
    let Some(nid) = instwin::track_id(app, nt) else { return };
    let mut doc = lg_synth::sampler_doc::SamplerDoc::new();
    let data = Arc::new(lg_synth::sampler::SampleData { rate: a.rate as f32, ch: a.channels.iter().take(2).map(|c| lg_synth::sampler::Pcm::F32(c.clone())).collect() });
    for z in &zones {
        let zs = doc.add_loaded(vec![(path.clone(), data.clone(), None, Some(z.root))], lg_synth::sampler_doc::Drop::Key(z.root));
        if let Some(&zi) = zs.first() {
            let zz = &mut doc.zones[zi];
            zz.key_lo = z.lokey;
            zz.key_hi = z.hikey;
            zz.root = z.root;
            zz.start = z.start;
            zz.end = z.end;
            zz.one_shot = z.loop_mode == lg_synth::sampler::LoopMode::OneShot;
            zz.loop_on = matches!(z.loop_mode, lg_synth::sampler::LoopMode::Continuous | lg_synth::sampler::LoopMode::Sustain);
            zz.loop_start = z.loop_start;
            zz.loop_end = z.loop_end;
            zz.reverse = z.reverse;
            zz.tune = z.tune_cents;
            zz.pitch = z.keytrack > 0.0;
        }
    }
    app.instrument_choice.insert(nt, "Sampler".into());
    app.inst.docs.insert(nid, doc);
    app.load_engine();
    // the synth settings Quick Sampler shares with Sampler (same ids), and its own mapped
    let mut pairs: Vec<(u32, f32)> = params.iter().zip(vals).filter(|(p, _)| (21..1000).contains(&p.id)).map(|(p, v)| (p.id, *v)).collect();
    let get = |pid: u32| params.iter().position(|p| p.id == pid).map(|i| vals[i]).unwrap_or(0.0);
    pairs.extend([(0, get(qs::VOLUME)), (5, get(qs::FILTER_ON)), (6, get(qs::CUTOFF)), (7, get(qs::RESO)), (10, get(qs::COARSE)), (11, get(qs::FINE)), (12, get(qs::BEND))]);
    instwin::set_params(app, nt, &pairs);
    instwin::bump(app, nt);
    app.selected_track = Some(nt);
    app.after_edit();
}

/// The rename sheet (Rename Current File…): OK or Return renames, Cancel or Escape closes.
pub fn rename_sheet(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some(mut name) = app.inst.quick_ui.rename.take() else { return };
    let mut done: Option<bool> = None;
    egui::Window::new("Rename Current File").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        let te = ui.add(egui::TextEdit::singleline(&mut name).desired_width(260.0));
        te.request_focus();
        if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            done = Some(true);
        }
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
            if ui.button("OK").clicked() {
                done = Some(true);
            }
        });
    });
    match done {
        Some(true) => run(app, ctx, t, Act::Rename(name), None),
        Some(false) => {}
        None => app.inst.quick_ui.rename = Some(name),
    }
}
