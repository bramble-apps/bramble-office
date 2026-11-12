//! Sample Alchemy's window, laid out as Logic's: the upper strip (Edit Mode buttons, the sample
//! name with its lock, the Amp / Mod switch, the More menu), the middle (source buttons and the
//! Mixer at the left, the waveform with lettered handles, the play mode buttons above it, the
//! Snap, Zoom and Mod Matrix controls under it; Trim mode shows the start and end markers,
//! Motion mode its recording controls and timeline), the envelope, LFO and Mod Matrix band, and
//! the three modules of the lower section: Source, Synthesis (Granular / Additive / Spectral with
//! the effect and its four knobs) and Filter. An audio file dropped on the window, or Sample >
//! Load Audio, loads the sample.

use std::sync::Arc;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_dsp::Plugin;
use lg_synth::sample_alchemy as sa;
use lg_synth::sample_alchemy::{o, SampleAlchemy};

use crate::app::App;
use crate::instwin;
use crate::look;
use crate::synthui::{self, rgb, Pane, Skin};

const SKIN: Skin = Skin::new(0x3a3d42, 0x2e3135, 0x7fb0e0);
const SRC_COL: [Color32; 4] = [rgb(0x4aa3f0), rgb(0x52c27a), rgb(0xf0a030), rgb(0xc070e0)];
const ORANGE: Color32 = rgb(0xf08a30);
const W: f32 = 900.0;
const H: f32 = 590.0;

/// What the window shows of the loaded sample (copied out of the engine once).
struct Wave {
    data: Vec<f32>,
    key: (usize, u32, u32, u32),
}

struct Info {
    wave: Option<Arc<Wave>>,
    dur: f32,
    transients: Vec<f32>,
    path: Option<String>,
    motion: Option<(f32, [Option<(f32, f32)>; 4])>,
}

fn name_of(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn load(app: &mut App, t: usize, path: &std::path::Path) {
    match lg_synth::wav::read_audio(path) {
        Ok(a) => {
            let n = a.frames();
            let mono: Vec<f32> = (0..n).map(|i| a.channels.iter().map(|c| c.get(i).copied().unwrap_or(0.0)).sum::<f32>() / a.channels.len().max(1) as f32).collect();
            let rate = a.rate as f32;
            let p = path.to_string_lossy().into_owned();
            instwin::checkpoint(app, t, "Load Audio");
            let root = a.root;
            instwin::with_inst(app, t, |s: &mut SampleAlchemy| {
                s.set_sample(mono, rate);
                s.set_sample_path(Some(p.clone()));
                if let Some(k) = root {
                    s.set(sa::ROOT_KEY, k as f32);
                }
            });
            // rebuilding the engine loads the same sample again
            app.instrument_choice.insert(t, format!("Sample Alchemy\u{1f}{}", path.display()));
            app.dirty = true;
            instwin::bump(app, t);
        }
        Err(e) => app.tell(format!("Couldn't load {}: {e}", path.display())),
    }
}

fn fetch(app: &App, ctx: &egui::Context, t: usize) -> Info {
    let id = egui::Id::unique(("salch-wave", t));
    let key = instwin::with_inst(app, t, |s: &mut SampleAlchemy| {
        let d = s.sample();
        let n = d.len();
        (n, s.sample_rate() as u32, d.get(n / 3).map(|v| v.to_bits()).unwrap_or(0), d.get(n / 2).map(|v| v.to_bits()).unwrap_or(0))
    });
    let Some(key) = key else { return Info { wave: None, dur: 0.0, transients: Vec::new(), path: None, motion: None } };
    let mut wave: Option<Arc<Wave>> = ctx.data(|d| d.get_temp::<Arc<Wave>>(id));
    if key.0 > 0 && wave.as_ref().map(|w| w.key != key).unwrap_or(true) {
        let data = instwin::with_inst(app, t, |s: &mut SampleAlchemy| s.sample().to_vec()).unwrap_or_default();
        let w = Arc::new(Wave { data, key });
        ctx.data_mut(|d| d.insert_temp(id, w.clone()));
        wave = Some(w);
    }
    if key.0 == 0 {
        wave = None;
    }
    let (dur, transients, path, motion) = instwin::with_inst(app, t, |s: &mut SampleAlchemy| (s.duration(), s.transients().to_vec(), s.sample_path().map(|p| p.to_string()), s.motion_state())).unwrap_or((0.0, Vec::new(), None, None));
    let path = path.or_else(|| app.instrument_choice.get(&t).and_then(|c| c.split('\u{1f}').nth(1)).map(|s| s.to_string()));
    Info { wave, dur, transients, path, motion }
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    if let Some(f) = std::mem::take(&mut app.inst.dropped).first() {
        load(app, t, f);
    }
    let info = fetch(app, ctx, t);
    let bpm = app.tempo() as f32;
    let dragging_files = instwin::dragging_files(ctx);
    let area = std::cell::Cell::new(None);
    let actions = synthui::window(app, ctx, t, "Sample Alchemy", vec2(W, H), SKIN, &[], |p| {
        area.set(Some(Rect::from_min_size(p.o, vec2(W, H))));
        body(p, &info, bpm, dragging_files)
    });
    // where a dropped audio file lands on the window
    app.inst.rect = area.get();
    for a in actions {
        match a.as_str() {
            "load" => {
                if let Some(f) = kn_ui::files::FileDialog::new().set_title("Load Audio").add_filter("Audio", &["wav", "aif", "aiff", "caf", "flac", "WAV", "AIF", "AIFF"]).pick_file() {
                    load(app, t, &f);
                }
            }
            "export" => {
                if let Some(f) = kn_ui::files::FileDialog::new().set_title("Export Audio").set_file_name("Sample Alchemy.wav").save_file() {
                    let r = instwin::with_inst(app, t, |s: &mut SampleAlchemy| s.export_wav(&f));
                    if let Some(Err(e)) = r {
                        app.tell(format!("Couldn't export: {e}"));
                    }
                }
            }
            "reveal" => {
                if let Some(dir) = info.path.as_ref().and_then(|p| std::path::Path::new(p).parent().map(|d| d.to_path_buf())) {
                    let _ = std::process::Command::new("xdg-open").arg(dir).stdin(std::process::Stdio::null()).spawn();
                }
            }
            "derive" => {
                if let Some(bpm) = instwin::with_inst(app, t, |s: &mut SampleAlchemy| s.derive_tempo()) {
                    instwin::checkpoint(app, t, "Derive Tempo");
                    instwin::set_params(app, t, &[(sa::TEMPO, (bpm * 10.0).round() / 10.0)]);
                }
            }
            s if s.starts_with("preview:") => {
                if let Ok(n) = s[8..].parse::<u8>() {
                    crate::keyswin::audition(app, t, n, 100);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// small widgets

fn dot(p: &Pane, c: egui::Pos2, col: Color32) {
    p.painter().circle_filled(c, 2.5, col);
}

/// A button with a drop-down menu: the response to attach the popup to.
fn drop_button(p: &mut Pane, r: Rect, label: &str, salt: &str) -> egui::Response {
    let resp = p.ui.interact(r, egui::Id::unique(("salch-dd", p.t, salt)), Sense::click());
    let pt = p.ui.painter();
    pt.rect_filled(r, 4.0, if resp.hovered() { Color32::from_gray(0x30) } else { Color32::from_gray(0x22) });
    pt.with_clip_rect(r).text(pos2(r.min.x + 6.0, r.center().y), Align2::LEFT_CENTER, label, look::regular(10.5), p.skin.text);
    crate::glyphs::paint(pt, Rect::from_center_size(pos2(r.max.x - 8.0, r.center().y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_DOWN, 9.0, p.skin.dim);
    resp
}

/// A pop-up listing `items` with `cur` ticked; the picked index.
fn pick_menu(resp: &egui::Response, items: &[String], cur: usize) -> Option<usize> {
    let mut pick = None;
    egui::Popup::menu(resp).show(|ui| {
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            for (i, c) in items.iter().enumerate() {
                if ui.add(egui::Button::selectable(i == cur, c.as_str())).clicked() {
                    pick = Some(i);
                    ui.close();
                }
            }
        });
    });
    pick
}

/// Pressing a control makes it the Mod Matrix target.
fn select_target(p: &mut Pane, r: Rect, target: usize) {
    let pressed = p.ui.input(|i| i.pointer.primary_pressed());
    let pos = p.ui.input(|i| i.pointer.interact_pos());
    if pressed && pos.is_some_and(|q| r.contains(q)) && p.geti(sa::MOD_TARGET) != target {
        p.put(sa::MOD_TARGET, target as f32);
    }
}

/// How far the routings move a target (a share of its range), 0 for none.
fn mod_depth(p: &Pane, target: usize) -> f32 {
    (0..sa::ROUTES)
        .filter(|&i| p.get(sa::route_id(i, 0)).round() as usize == target && p.geti(sa::route_id(i, 1)) > 0 && p.on(sa::route_id(i, 3)))
        .map(|i| p.get(sa::route_id(i, 2)))
        .sum()
}

/// A knob that is also a Mod Matrix target: pressing it selects it (a dot in its centre), an
/// orange arc beside the value arc shows the range the modulation covers.
fn knob_t(p: &mut Pane, x: f32, y: f32, d: f32, id: u32, label: &str) {
    p.knob(x, y, d, id, label);
    let Some(t) = sa::target_of(id) else { return };
    let r = p.rect(x - d / 2.0, y, d, d);
    select_target(p, r, t);
    let c = p.o + vec2(x, y + d / 2.0 + 2.0);
    if p.geti(sa::MOD_TARGET) == t {
        dot(p, c, ORANGE);
    }
    let depth = mod_depth(p, t);
    if depth != 0.0 {
        let pr = p.prm(id);
        let f = ((p.get(id) - pr.min) / (pr.max - pr.min).max(1e-9)).clamp(0.0, 1.0);
        let (a, b) = (f.min((f + depth).clamp(0.0, 1.0)), f.max((f + depth).clamp(0.0, 1.0)));
        let start = std::f32::consts::PI * (0.5 + 1.0 / 6.0);
        let sweep = std::f32::consts::PI * 5.0 / 3.0;
        let pts: Vec<egui::Pos2> = (0..=24).map(|k| {
            let ang = start + sweep * (a + (b - a) * k as f32 / 24.0);
            c + vec2(ang.cos(), ang.sin()) * (d / 2.0 + 1.0)
        }).collect();
        p.painter().add(egui::Shape::line(pts, Stroke::new(2.0, ORANGE)));
    }
}

/// Sets an effect: the knobs take that effect's defaults.
fn put_effect(p: &mut Pane, s: usize, mode: usize, fx: usize) {
    p.put(sa::sid(s, o::MODE), mode as f32);
    p.put(sa::sid(s, o::EFFECT), fx as f32);
    let d = sa::knob_defaults(mode, fx);
    for (k, v) in d.iter().enumerate() {
        p.put(sa::sid(s, o::K1 + k as u32), *v);
    }
}

fn pos_to_x(wr: Rect, z: (f32, f32), pos: f32) -> f32 {
    wr.min.x + (pos - z.0) / (z.1 - z.0) * wr.width()
}

fn x_to_pos(wr: Rect, z: (f32, f32), x: f32) -> f32 {
    (z.0 + (x - wr.min.x) / wr.width() * (z.1 - z.0)).clamp(0.0, 1.0)
}

fn draw_wave(p: &Pane, wr: Rect, wave: &Wave, z: (f32, f32), col: Color32) {
    let pt = p.painter().with_clip_rect(wr);
    let n = wave.data.len().max(1);
    let cols = wr.width() as usize;
    let mid = wr.center().y;
    let amp = wr.height() * 0.46;
    pt.hline(wr.x_range(), mid, Stroke::new(1.0, Color32::from_gray(0x2c)));
    let mut shapes = Vec::with_capacity(cols);
    for c in 0..cols {
        let a = ((z.0 + (z.1 - z.0) * c as f32 / cols as f32) * n as f32) as usize;
        let b = (((z.0 + (z.1 - z.0) * (c + 1) as f32 / cols as f32) * n as f32) as usize).max(a + 1).min(n);
        if a >= n {
            break;
        }
        let step = ((b - a) / 48).max(1);
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        let mut i = a;
        while i < b {
            let v = wave.data[i];
            lo = lo.min(v);
            hi = hi.max(v);
            i += step;
        }
        let x = wr.min.x + c as f32 + 0.5;
        shapes.push(egui::Shape::line_segment([pos2(x, mid - hi.clamp(-1.0, 1.0) * amp), pos2(x, mid - lo.clamp(-1.0, 1.0) * amp - 1.0)], Stroke::new(1.0, col)));
    }
    pt.extend(shapes);
}

// ---------------------------------------------------------------------------------------------

fn body(p: &mut Pane, info: &Info, bpm: f32, dragging_files: bool) {
    let sel = p.geti(sa::SRC_SEL).min(4);
    let cur = sel.min(3);
    let edit = p.geti(sa::EDIT_MODE).min(2);
    let play = p.geti(sa::sid(cur, o::PLAY)).min(4);
    let mode = p.geti(sa::sid(cur, o::MODE)).min(2);
    let fx = p.geti(sa::sid(cur, o::EFFECT)).min(sa::effects(mode).len() - 1);
    let tempo = p.get(sa::TEMPO);

    // ---- upper strip
    p.seg(10.0, 8.0, 150.0, 22.0, sa::EDIT_MODE, sa::EDIT_MODES, false);
    let name = info.path.as_ref().map(|s| name_of(s)).unwrap_or_else(|| "No Sample".to_string());
    let nr = p.rect(172.0, 8.0, 250.0, 22.0);
    p.painter().rect_filled(nr, 4.0, Color32::from_gray(0x22));
    p.painter().with_clip_rect(nr).text(pos2(nr.min.x + 8.0, nr.center().y), Align2::LEFT_CENTER, &name, look::regular(11.0), p.skin.text);
    p.toggle(428.0, 8.0, 44.0, 22.0, sa::SAMPLE_LOCK, "Lock");
    let sr = p.rect(478.0, 8.0, 84.0, 22.0);
    let sresp = drop_button(p, sr, "Sample", "sample");
    let mut menu_act: Option<&str> = None;
    egui::Popup::menu(&sresp).show(|ui| {
        if ui.button("Load Audio…").clicked() {
            menu_act = Some("load");
            ui.close();
        }
        if ui.add_enabled(info.wave.is_some(), egui::Button::new("Export Audio…")).clicked() {
            menu_act = Some("export");
            ui.close();
        }
        if ui.add_enabled(info.path.is_some(), egui::Button::new("Show in Finder")).clicked() {
            menu_act = Some("reveal");
            ui.close();
        }
    });
    if let Some(a) = menu_act {
        p.actions.push(a.to_string());
    }
    p.seg(650.0, 8.0, 100.0, 22.0, sa::ENV_SEL, sa::ENVS, false);
    more_menu(p, info, p.rect(796.0, 8.0, 94.0, 22.0));

    // ---- source buttons, mixer, snap, zoom
    for s in 0..4 {
        let r = p.rect(8.0, 64.0 + s as f32 * 28.0, 50.0, 24.0);
        let on = p.on(sa::sid(s, o::ON));
        let resp = p.ui.interact(r, egui::Id::unique(("salch-src", p.t, s)), Sense::click());
        let col = if on { SRC_COL[s] } else { Color32::from_gray(0x3c) };
        p.painter().rect_filled(r, 5.0, col);
        if sel == s || sel == 4 {
            p.painter().rect_stroke(r.expand(2.0), 6.0, Stroke::new(1.5, Color32::from_gray(0xe0)), egui::StrokeKind::Outside);
        }
        p.painter().text(r.center(), Align2::CENTER_CENTER, sa::SOURCES[s], look::semibold(13.0), if on { Color32::from_gray(0x10) } else { p.skin.text });
        if resp.clicked() {
            p.put(sa::sid(s, o::ON), if on { 0.0 } else { 1.0 });
        }
    }
    p.toggle(8.0, 180.0, 50.0, 22.0, sa::MIXER_SHOW, "Mixer");
    let zoom_id = p.id("salch-zoom-on", 0);
    let mut zoom_on: bool = p.ui.data(|d| d.get_temp(zoom_id)).unwrap_or(false);
    let zr = p.rect(8.0, 206.0, 50.0, 22.0);
    if p.button(zr, "Zoom", zoom_on, "zoom") {
        zoom_on = !zoom_on;
        p.ui.data_mut(|d| d.insert_temp(zoom_id, zoom_on));
    }
    p.menu(66.0, 238.0, 110.0, 20.0, sa::SNAP);
    p.label(40.0, 247.0, "Snap");
    p.toggle(796.0, 238.0, 94.0, 20.0, sa::MATRIX_SHOW, "Mod Matrix");

    // ---- play mode buttons / motion controls above the waveform
    match edit {
        0 => p.seg(319.0, 38.0, 320.0, 20.0, sa::sid(cur, o::PLAY), sa::PLAYS, false),
        1 => motion_controls(p, info),
        _ => {
            p.text(479.0, 48.0, Align2::CENTER_CENTER, "Trim: drag the start and end markers", 10.5, p.skin.dim);
        }
    }

    // ---- the waveform
    let wr = p.rect(66.0, 64.0, 826.0, 170.0);
    p.painter().rect_filled(wr, 6.0, Color32::from_gray(0x16));
    let z0 = p.get(sa::ZOOM_LO);
    let z1 = p.get(sa::ZOOM_HI).max(z0 + 0.01);
    let z = (z0, z1.min(1.0).max(z0 + 0.01));
    let (ts, te) = (p.get(sa::TRIM_START), p.get(sa::TRIM_END).max(p.get(sa::TRIM_START) + 0.001));
    match &info.wave {
        Some(w) => {
            draw_wave(p, wr, w, z, Color32::from_rgb(0x8a, 0xb4, 0xd8));
            let pt = p.painter().with_clip_rect(wr);
            let shade = Color32::from_black_alpha(if edit == 2 { 120 } else { 80 });
            let (xa, xb) = (pos_to_x(wr, z, ts), pos_to_x(wr, z, te));
            if xa > wr.min.x {
                pt.rect_filled(Rect::from_min_max(wr.min, pos2(xa.min(wr.max.x), wr.max.y)), 0.0, shade);
            }
            if xb < wr.max.x {
                pt.rect_filled(Rect::from_min_max(pos2(xb.max(wr.min.x), wr.min.y), wr.max), 0.0, shade);
            }
            if tempo > 0.0 && info.dur > 0.0 {
                // beat marks
                let b = 60.0 / tempo / info.dur;
                let mut k = (z.0 / b).ceil();
                while k * b < z.1 && (k - (z.0 / b).ceil()) < 400.0 {
                    let x = pos_to_x(wr, z, k * b);
                    pt.vline(x, wr.max.y - 6.0..=wr.max.y, Stroke::new(1.0, Color32::from_gray(0x60)));
                    k += 1.0;
                }
            }
        }
        None => {
            let msg = if dragging_files { "Drop to load" } else { "Drop an audio file here, or choose Sample > Load Audio" };
            p.painter().text(wr.center(), Align2::CENTER_CENTER, msg, look::regular(13.0), p.skin.dim);
        }
    }
    if dragging_files {
        p.painter().rect_stroke(wr, 6.0, Stroke::new(2.0, p.skin.accent), egui::StrokeKind::Inside);
    }
    let wid = p.id("salch-wave", 0);
    let resp = p.ui.interact(wr, wid, Sense::click_and_drag());
    if p.on(sa::MIXER_SHOW) {
        let mr = p.rect(68.0, 66.0, 150.0, 166.0);
        p.painter().rect_filled(mr, 6.0, Color32::from_black_alpha(190));
        for s in 0..4 {
            let x = 82.0 + s as f32 * 34.0;
            p.vfader(x, 76.0, 22.0, 130.0, sa::sid(s, o::VOL), sa::SOURCES[s]);
            let r = p.rect(x - 4.0, 76.0, 30.0, 130.0);
            select_target(p, r, s * sa::TARGETS_PER_SRC + 8);
            if p.geti(sa::MOD_TARGET) == s * sa::TARGETS_PER_SRC + 8 {
                dot(p, p.o + vec2(x + 11.0, 216.0), ORANGE);
            }
            if sel == s {
                dot(p, p.o + vec2(x + 11.0, 226.0), SRC_COL[s]);
            }
        }
    }

    // handles and the interaction
    let snap_mode = p.geti(sa::SNAP);
    let drag_id = p.id("salch-drag", 0);
    let mut drag: Option<(u8, usize)> = p.ui.data(|d| d.get_temp(drag_id)).flatten();
    let hy = |y: f32| wr.max.y - y * wr.height();
    let (mut xs, mut ys) = ([0.0f32; 4], [0.0f32; 4]);
    for s in 0..4 {
        xs[s] = p.get(sa::sid(s, o::X));
        ys[s] = p.get(sa::sid(s, o::Y));
    }
    let loops: [Option<f32>; 4] = std::array::from_fn(|s| (p.on(sa::sid(s, o::ON)) && p.geti(sa::sid(s, o::PLAY)) == 1).then(|| p.get(sa::sid(s, o::LOOP_END))));
    let pointer = p.ui.input(|i| i.pointer.interact_pos());
    let pressed = p.ui.input(|i| i.pointer.primary_pressed());
    let dist = |a: egui::Pos2, b: egui::Pos2| (a - b).length();
    let mixer_rect = p.on(sa::MIXER_SHOW).then(|| p.rect(68.0, 66.0, 150.0, 166.0));
    let press_pos = if info.wave.is_some() && pressed { pointer.filter(|pp| wr.contains(*pp) && !mixer_rect.is_some_and(|m| m.contains(*pp))) } else { None };
    if let Some(pp) = press_pos {
        drag = None;
        if edit == 2 {
            let (xa, xb) = (pos_to_x(wr, z, ts), pos_to_x(wr, z, te));
            if (pp.x - xa).abs() < 8.0 {
                drag = Some((1, 0));
            } else if (pp.x - xb).abs() < 8.0 {
                drag = Some((2, 0));
            }
        } else {
            let mut best = (f32::MAX, 0usize);
            for s in 0..4 {
                if p.on(sa::sid(s, o::ON)) {
                    let d = dist(pp, pos2(pos_to_x(wr, z, xs[s]), hy(ys[s])));
                    if d < best.0 {
                        best = (d, s);
                    }
                }
            }
            if best.0 < 16.0 {
                drag = Some((0, best.1));
                p.put(sa::SRC_SEL, best.1 as f32);
                if p.on(sa::PREVIEW) && !(edit == 1 && p.on(sa::MOTION_REC)) {
                    p.actions.push(format!("preview:{}", p.get(sa::PREVIEW_KEY).round() as u8));
                }
            } else {
                for (s, le) in loops.iter().enumerate() {
                    if let Some(le) = *le {
                        let (xa, xb) = (pos_to_x(wr, z, xs[s]), pos_to_x(wr, z, le));
                        if (pp.y - hy(ys[s])).abs() < 8.0 && pp.x >= xa.min(xb) - 6.0 && pp.x <= xa.max(xb) + 6.0 {
                            drag = Some((if (pp.x - xb).abs() < 8.0 { 4 } else { 3 }, s));
                            p.put(sa::SRC_SEL, s as f32);
                        }
                    }
                }
            }
        }
        p.ui.data_mut(|d| d.insert_temp(drag_id, drag));
    }
    if resp.dragged() && let (Some((kind, s)), Some(pp)) = (drag, pointer) {
        let x = x_to_pos(wr, z, pp.x);
        let snapped = sa::snap_to(x, snap_mode, tempo, info.dur, &info.transients);
        match kind {
            0 => {
                if snap_mode != 4 {
                    p.put(sa::sid(s, o::X), snapped);
                    xs[s] = snapped;
                }
                let y = ((wr.max.y - pp.y) / wr.height()).clamp(0.0, 1.0);
                p.put(sa::sid(s, o::Y), y);
                ys[s] = y;
                if edit == 1 && p.on(sa::MOTION_REC) {
                    record_motion(p, s, xs[s], ys[s], bpm);
                }
            }
            1 => p.put(sa::TRIM_START, snapped.min(te - 0.001)),
            2 => p.put(sa::TRIM_END, snapped.max(ts + 0.001)),
            3 => {
                let dx = resp.drag_delta().x / wr.width() * (z.1 - z.0);
                let le = loops[s].unwrap_or(1.0);
                let dx = dx.clamp(-xs[s].min(le), 1.0 - xs[s].max(le));
                p.put(sa::sid(s, o::X), xs[s] + dx);
                p.put(sa::sid(s, o::LOOP_END), le + dx);
            }
            _ => p.put(sa::sid(s, o::LOOP_END), snapped),
        }
    }
    if !p.ui.input(|i| i.pointer.primary_down()) {
        if drag.is_some() || p.ui.data(|d| d.get_temp::<(f64, i32)>(p.id("salch-rec", 0)).is_some()) {
            // letting go of the handles ends a motion recording
            if p.on(sa::MOTION_REC) && edit == 1 && drag.map(|d| d.0 == 0).unwrap_or(false) {
                p.put(sa::MOTION_REC, 0.0);
            }
            p.ui.data_mut(|d| d.remove_temp::<(f64, i32)>(p.id("salch-rec", 0)));
        }
        drag = None;
        p.ui.data_mut(|d| d.insert_temp(drag_id, drag));
    }
    // draw trim markers and the handles
    if info.wave.is_some() {
        let pt = p.painter().with_clip_rect(wr.expand(10.0));
        if edit == 2 {
            for x in [pos_to_x(wr, z, ts), pos_to_x(wr, z, te)] {
                pt.vline(x, wr.y_range(), Stroke::new(1.5, Color32::from_rgb(0x3c, 0x9c, 0xf0)));
                pt.rect_filled(Rect::from_center_size(pos2(x, wr.min.y + 6.0), vec2(10.0, 12.0)), 2.0, Color32::from_rgb(0x3c, 0x9c, 0xf0));
            }
        }
        for s in 0..4 {
            if !p.on(sa::sid(s, o::ON)) {
                continue;
            }
            let c = pos2(pos_to_x(wr, z, p.get(sa::sid(s, o::X))), hy(p.get(sa::sid(s, o::Y))));
            if let Some(le) = loops[s] {
                let xe = pos_to_x(wr, z, le);
                pt.line_segment([c, pos2(xe, c.y)], Stroke::new(3.0, SRC_COL[s].gamma_multiply(0.6)));
                pt.rect_filled(Rect::from_center_size(pos2(xe, c.y), vec2(5.0, 16.0)), 2.0, SRC_COL[s]);
            }
            pt.circle_filled(c, 11.0, SRC_COL[s]);
            if sel == s {
                pt.circle_stroke(c, 13.5, Stroke::new(1.5, Color32::WHITE));
            }
            pt.text(c, Align2::CENTER_CENTER, sa::SOURCES[s], look::semibold(12.0), Color32::from_gray(0x10));
        }
        // a motion plays on screen
        if let Some((_, hs)) = &info.motion {
            for (s, h) in hs.iter().enumerate() {
                if let Some((x, y)) = h {
                    pt.circle_stroke(pos2(pos_to_x(wr, z, *x), hy(*y)), 14.0, Stroke::new(2.0, SRC_COL[s]));
                }
            }
        }
    }

    // ---- the zoom slider
    if zoom_on {
        zoom_slider(p, wr, z);
    }
    // ---- the motion timeline in the row under the waveform
    if edit == 1 {
        timeline(p, info);
    }

    // ---- the band: envelope, LFOs, Mod Matrix
    envelope_panel(p, cur);
    lfo_panel(p);
    if p.on(sa::MATRIX_SHOW) {
        matrix_panel(p);
    }

    // ---- the lower modules
    source_module(p, sel, cur, play, tempo);
    synthesis_module(p, cur, mode, fx);
    filter_module(p, cur);

    // editing all sources at once: what is set on A is set on all
    if sel == 4 {
        let mut extra = Vec::new();
        for &(id, v) in &p.set {
            if (sa::SRC_BASE..sa::SRC_BASE + sa::SRC_STRIDE).contains(&id) {
                let off = id - sa::SRC_BASE;
                if ![o::ON, o::X, o::Y, o::LOOP_END].contains(&off) {
                    for s in 1..4 {
                        extra.push((sa::sid(s, off), v));
                    }
                }
            }
        }
        p.set.extend(extra);
    }
}

// ---------------------------------------------------------------------------------------------

fn more_menu(p: &mut Pane, info: &Info, r: Rect) {
    let resp = drop_button(p, r, "More", "more");
    let mut set: Vec<(u32, f32)> = Vec::new();
    let mut act: Vec<String> = Vec::new();
    let get: Vec<(u32, f32)> = (0..64).map(|i| (i, p.get(i))).collect();
    let g = |id: u32| get.iter().find(|x| x.0 == id).map(|x| x.1).unwrap_or(0.0);
    let params: Vec<lg_dsp::Param> = (0..64).map(|i| p.prm(i)).collect();
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(250.0);
        let num = |ui: &mut egui::Ui, set: &mut Vec<(u32, f32)>, label: &str, id: u32, speed: f64| {
            let pr = &params[id as usize];
            let mut v = g(id);
            ui.horizontal(|ui| {
                ui.label(label);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::DragValue::new(&mut v).range(pr.min..=pr.max).speed(speed)).changed() {
                        set.push((id, v));
                    }
                });
            });
        };
        let check = |ui: &mut egui::Ui, set: &mut Vec<(u32, f32)>, label: &str, id: u32| {
            let on = g(id) >= 0.5;
            if ui.add(egui::Button::selectable(on, label)).clicked() {
                set.push((id, if on { 0.0 } else { 1.0 }));
            }
        };
        num(ui, &mut set, "Volume (dB)", sa::VOLUME, 0.1);
        num(ui, &mut set, "Polyphony", sa::POLY, 0.1);
        num(ui, &mut set, "Glide (s)", sa::GLIDE, 0.01);
        ui.menu_button("Play Mode", |ui| {
            for (i, n) in sa::PLAY_MODES.iter().enumerate() {
                if ui.add(egui::Button::selectable(g(sa::PLAY_MODE).round() as usize == i, *n)).clicked() {
                    set.push((sa::PLAY_MODE, i as f32));
                    ui.close();
                }
            }
        });
        num(ui, &mut set, "Bend Up (st)", sa::BEND_UP, 0.1);
        num(ui, &mut set, "Bend Down (st)", sa::BEND_DOWN, 0.1);
        check(ui, &mut set, "Output Limiter", sa::LIMITER);
        ui.separator();
        check(ui, &mut set, "Preview Mode", sa::PREVIEW);
        num(ui, &mut set, "Preview Key", sa::PREVIEW_KEY, 0.1);
        check(ui, &mut set, "Pitch Lock", sa::PITCH_LOCK);
        check(ui, &mut set, "Reverse", sa::REVERSE);
        num(ui, &mut set, "Root Key", sa::ROOT_KEY, 0.1);
        num(ui, &mut set, "Root Cents", sa::ROOT_CENTS, 0.2);
        num(ui, &mut set, "Tempo (0 = None)", sa::TEMPO, 0.2);
        if ui.add_enabled(info.wave.is_some(), egui::Button::new("Derive Tempo from Loop Length")).clicked() {
            act.push("derive".into());
            ui.close();
        }
        ui.separator();
        ui.menu_button("MIDI Mono Mode", |ui| {
            for (i, n) in sa::MONO_MODES.iter().enumerate() {
                if ui.add(egui::Button::selectable(g(sa::MIDI_MONO).round() as usize == i, *n)).clicked() {
                    set.push((sa::MIDI_MONO, i as f32));
                    ui.close();
                }
            }
        });
        num(ui, &mut set, "MPE Pitch Bend Range (st)", sa::MPE_RANGE, 0.2);
        ui.menu_button("MIDI Assign", |ui| {
            for (i, n) in ["Ctrl A", "Ctrl B", "Ctrl C", "Ctrl D"].iter().enumerate() {
                num(ui, &mut set, &format!("{n} follows CC"), sa::CTRL_A + i as u32, 0.1);
            }
        });
        ui.separator();
        ui.add_enabled(false, egui::Button::new("Create Alchemy track"));
    });
    for (id, v) in set {
        p.put(id, if matches!(id, sa::POLY | sa::PLAY_MODE | sa::MIDI_MONO | sa::PREVIEW_KEY | sa::ROOT_KEY) || (sa::CTRL_A..sa::CTRL_A + 4).contains(&id) { v.round() } else { v });
    }
    p.actions.extend(act);
}

fn motion_controls(p: &mut Pane, info: &Info) {
    let armed = p.on(sa::MOTION_REC);
    let flash = armed && p.ui.input(|i| (i.time * 2.0) as i64 % 2 == 0);
    let r = p.rect(66.0, 38.0, 74.0, 20.0);
    p.ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    let resp = p.ui.interact(r, egui::Id::unique(("salch-rec-btn", p.t)), Sense::click());
    p.painter().rect_filled(r, 4.0, if flash { rgb(0xe03030) } else if armed { rgb(0x903030) } else { Color32::from_gray(0x3c) });
    p.painter().text(r.center(), Align2::CENTER_CENTER, "Record", look::regular(10.5), p.skin.text);
    if resp.clicked() {
        p.put(sa::MOTION_REC, if armed { 0.0 } else { 1.0 });
        p.ui.data_mut(|d| d.remove_temp::<(f64, i32)>(p.id("salch-rec", 0)));
    }
    let sr = p.rect(146.0, 38.0, 50.0, 20.0);
    if p.button(sr, "Stop", false, "mstop") {
        p.put(sa::MOTION_REC, 0.0);
    }
    let has = (0..8 * sa::MOTION_STEPS).any(|k| p.get(sa::MOTION_BASE + k as u32) >= 0.0);
    if has {
        let cr = p.rect(202.0, 38.0, 50.0, 20.0);
        if p.button(cr, "Clear", false, "mclear") {
            for k in 0..8 * sa::MOTION_STEPS {
                p.put(sa::MOTION_BASE + k as u32, -1.0);
            }
        }
    }
    for (k, (id, l)) in [(sa::MOTION_DUR, "Duration"), (sa::MOTION_LS, "Loop Start"), (sa::MOTION_LE, "Loop End")].into_iter().enumerate() {
        let x = 480.0 + k as f32 * 140.0;
        p.text(x, 48.0, Align2::RIGHT_CENTER, l, 10.5, p.skin.dim);
        p.field(x + 4.0, 38.0, 56.0, 20.0, id, 0.25);
    }
    let _ = info;
}

/// Writes the handle's place into the motion at the point in time the recording has reached.
fn record_motion(p: &mut Pane, s: usize, x: f32, y: f32, bpm: f32) {
    let rid = p.id("salch-rec", 0);
    let now = p.ui.input(|i| i.time);
    let (t0, last): (f64, i32) = p.ui.data(|d| d.get_temp(rid)).unwrap_or((now, -1));
    let dur = p.get(sa::MOTION_DUR).max(1.0);
    let beats = ((now - t0) * bpm as f64 / 60.0) as f32;
    let step = ((beats / dur * sa::MOTION_STEPS as f32) as usize).min(sa::MOTION_STEPS * 8) as i32;
    // fill the steps since the last frame
    let from = if last < 0 { step } else { last + 1 };
    for k in from..=step {
        let idx = (k as usize) % sa::MOTION_STEPS;
        p.put(sa::motion_id(s, 0, idx), x);
        p.put(sa::motion_id(s, 1, idx), y);
    }
    p.ui.data_mut(|d| d.insert_temp(rid, (t0, step)));
    if step as usize >= sa::MOTION_STEPS {
        // one pass of the recording length ends it
        p.put(sa::MOTION_REC, 0.0);
    }
}

fn timeline(p: &mut Pane, info: &Info) {
    let r = p.rect(250.0, 238.0, 530.0, 20.0);
    p.painter().rect_filled(r, 4.0, Color32::from_gray(0x16));
    let dur = p.get(sa::MOTION_DUR).max(1.0);
    let (ls, le) = (p.get(sa::MOTION_LS), p.get(sa::MOTION_LE));
    let pt = p.painter().with_clip_rect(r);
    pt.rect_filled(Rect::from_min_max(pos2(r.min.x + ls / dur * r.width(), r.min.y), pos2(r.min.x + (le / dur).min(1.0) * r.width(), r.max.y)), 0.0, Color32::from_rgb(0x2a, 0x3c, 0x52));
    let mut b = 0.0;
    while b <= dur + 1e-3 {
        let x = r.min.x + b / dur * r.width();
        pt.vline(x, r.max.y - if (b as i32) % 4 == 0 { 10.0 } else { 5.0 }..=r.max.y, Stroke::new(1.0, Color32::from_gray(0x70)));
        b += 1.0;
    }
    if let Some((beats, _)) = &info.motion {
        let (ls, le) = (ls, le.max(ls + 0.01));
        let t = if *beats < le { *beats } else { ls + (beats - ls) % (le - ls) };
        let x = r.min.x + (t / dur).min(1.0) * r.width();
        pt.vline(x, r.y_range(), Stroke::new(2.0, Color32::WHITE));
        p.ui.ctx().request_repaint();
    }
}

fn zoom_slider(p: &mut Pane, wr: Rect, z: (f32, f32)) {
    let r = Rect::from_min_size(pos2(wr.min.x, wr.max.y + 2.0), vec2(0.0, 0.0));
    let _ = r;
    let bar = p.rect(190.0, 238.0, 590.0, 20.0);
    let bar = if p.get(sa::EDIT_MODE).round() as usize == 1 { p.rect(66.0, 262.0, 826.0, 0.0) } else { bar };
    if bar.height() < 1.0 {
        return;
    }
    p.painter().rect_filled(bar, 4.0, Color32::from_gray(0x16));
    let (xa, xb) = (bar.min.x + z.0 * bar.width(), bar.min.x + z.1 * bar.width());
    p.painter().rect_filled(Rect::from_min_max(pos2(xa, bar.min.y + 3.0), pos2(xb, bar.max.y - 3.0)), 3.0, p.skin.accent.gamma_multiply(0.5));
    for x in [xa, xb] {
        p.painter().rect_filled(Rect::from_center_size(pos2(x, bar.center().y), vec2(8.0, 16.0)), 2.0, Color32::from_gray(0xd8));
    }
    let resp = p.ui.interact(bar, p.id("salch-zoombar", 0), Sense::click_and_drag());
    let did = p.id("salch-zdrag", 0);
    if resp.drag_started() && let Some(pp) = resp.interact_pointer_pos() {
        let k = if (pp.x - xa).abs() < 8.0 { 5u8 } else if (pp.x - xb).abs() < 8.0 { 6 } else { 7 };
        p.ui.data_mut(|d| d.insert_temp(did, k));
    }
    if resp.dragged() && let Some(pp) = resp.interact_pointer_pos() {
        let k: u8 = p.ui.data(|d| d.get_temp(did)).unwrap_or(7);
        let f = ((pp.x - bar.min.x) / bar.width()).clamp(0.0, 1.0);
        match k {
            5 => p.put(sa::ZOOM_LO, f.min(z.1 - 0.01)),
            6 => p.put(sa::ZOOM_HI, f.max(z.0 + 0.01)),
            _ => {
                let dx = resp.drag_delta().x / bar.width();
                let dx = dx.clamp(-z.0, 1.0 - z.1);
                p.put(sa::ZOOM_LO, z.0 + dx);
                p.put(sa::ZOOM_HI, z.1 + dx);
            }
        }
    }
    if resp.double_clicked() {
        p.put(sa::ZOOM_LO, 0.0);
        p.put(sa::ZOOM_HI, 1.0);
    }
}

fn envelope_panel(p: &mut Pane, cur: usize) {
    let amp = p.geti(sa::ENV_SEL) == 0;
    p.group(8.0, 266.0, 296.0, 100.0, if amp { "AMP ENVELOPE" } else { "MOD ENVELOPE" });
    let base = if amp { o::AMP } else { o::MODENV };
    let v = |p: &Pane, k: u32| p.get(sa::sid(cur, base + k));
    let (a, d, s, r) = (v(p, 0), v(p, 1), v(p, 2), v(p, 3));
    let disp = p.display(16.0, 284.0, 140.0, 76.0);
    let t = |x: f32| (0.001 * 10000f32.powf(x)).min(10.0);
    let (ta, td, tr) = (t(a).sqrt(), t(d).sqrt(), t(r).sqrt());
    let tot = ta + td + 0.6 + tr;
    let (xa, xd, xs, xr) = (ta / tot, (ta + td) / tot, (ta + td + 0.6) / tot, 1.0);
    p.curve(disp, &[(0.0, 0.0), (xa, 1.0), (xd, s), (xs, s), (xr, 0.0)], if amp { p.skin.accent } else { ORANGE });
    for (k, l) in ["A", "D", "S", "R"].into_iter().enumerate() {
        let x = 166.0 + (k % 2) as f32 * 68.0;
        let y = 288.0 + (k / 2) as f32 * 36.0;
        p.text(x, y + 6.0, Align2::LEFT_CENTER, l, 10.5, p.skin.dim);
        p.field(x + 12.0, y, 52.0, 20.0, sa::sid(cur, base + k as u32), 0.01);
    }
    p.label(236.0, 358.0, "drag a value up or down");
}

fn lfo_panel(p: &mut Pane) {
    p.group(312.0, 266.0, 280.0, 100.0, "LFOs");
    for l in 0..4 {
        let y = 284.0 + l as f32 * 20.0;
        p.text(320.0, y + 9.0, Align2::LEFT_CENTER, &format!("{}", l + 1), 10.5, SRC_COL[l]);
        p.menu(334.0, y, 92.0, 18.0, sa::lfo_id(l, 0));
        p.field(432.0, y, 56.0, 18.0, sa::lfo_id(l, 1), 0.05);
        p.menu(494.0, y, 90.0, 18.0, sa::lfo_id(l, 2));
    }
}

fn matrix_panel(p: &mut Pane) {
    p.group(598.0, 266.0, 294.0, 100.0, "MOD MATRIX");
    let cur_t = p.geti(sa::MOD_TARGET).min(sa::ROUTES - 1);
    let tr = p.rect(668.0, 270.0, 216.0, 18.0);
    let names: Vec<String> = (0..sa::ROUTES).map(sa::target_name).collect();
    let resp = drop_button(p, tr, &format!("Target: {}", names[cur_t]), "mtarget");
    if let Some(i) = pick_menu(&resp, &names, cur_t) {
        p.put(sa::MOD_TARGET, i as f32);
    }
    let rows: Vec<usize> = (0..sa::ROUTES).filter(|&i| p.get(sa::route_id(i, 0)).round() as usize == cur_t).take(4).collect();
    let sources: Vec<String> = sa::MOD_SOURCES.iter().map(|s| s.to_string()).collect();
    for k in 0..4 {
        let y = 292.0 + k as f32 * 17.0;
        match rows.get(k) {
            Some(&i) => {
                let on = p.on(sa::route_id(i, 3));
                let r = p.rect(604.0, y, 34.0, 15.0);
                if p.button(r, if on { "On" } else { "Off" }, on, ("mon", k)) {
                    p.put(sa::route_id(i, 3), if on { 0.0 } else { 1.0 });
                }
                let sr = p.rect(642.0, y, 128.0, 15.0);
                let cur_s = p.geti(sa::route_id(i, 1));
                let resp = drop_button(p, sr, sa::MOD_SOURCES[cur_s.min(sa::MOD_SOURCES.len() - 1)], &format!("msrc{k}"));
                if let Some(s) = pick_menu(&resp, &sources, cur_s) {
                    if s == 0 {
                        // None removes the routing
                        p.put(sa::route_id(i, 1), 0.0);
                        p.put(sa::route_id(i, 0), sa::ROUTES as f32);
                    } else {
                        p.put(sa::route_id(i, 1), s as f32);
                    }
                }
                p.text(776.0, y + 7.0, Align2::LEFT_CENTER, "Depth", 9.5, p.skin.dim);
                p.field(810.0, y, 74.0, 15.0, sa::route_id(i, 2), 0.01);
            }
            None => {
                let sr = p.rect(642.0, y, 128.0, 15.0);
                let resp = drop_button(p, sr, "None", &format!("msrc{k}"));
                if let Some(s) = pick_menu(&resp, &sources, 0)
                    && s > 0
                    && let Some(free) = (0..sa::ROUTES).find(|&i| p.get(sa::route_id(i, 0)).round() as usize >= sa::ROUTES)
                {
                    p.put(sa::route_id(free, 0), cur_t as f32);
                    p.put(sa::route_id(free, 1), s as f32);
                    p.put(sa::route_id(free, 2), 0.5);
                    p.put(sa::route_id(free, 3), 1.0);
                }
            }
        }
    }
}

fn source_module(p: &mut Pane, sel: usize, cur: usize, play: usize, tempo: f32) {
    p.group(8.0, 372.0, 282.0, 210.0, "SOURCE");
    let names: Vec<String> = ["A", "B", "C", "D", "All"].iter().map(|s| s.to_string()).collect();
    let sr = p.rect(16.0, 392.0, 70.0, 20.0);
    let resp = drop_button(p, sr, &format!("Source {}", names[sel]), "srcsel");
    if let Some(i) = pick_menu(&resp, &names, sel) {
        p.put(sa::SRC_SEL, i as f32);
    }
    let sync_ok = tempo > 0.0;
    match play {
        0 => {
            knob_t(p, 60.0, 424.0, 56.0, sa::sid(cur, o::SPEED), "Playback Speed");
            let on = p.on(sa::sid(cur, o::SPEED_SYNC));
            let r = p.rect(34.0, 502.0, 52.0, 18.0);
            if p.button(r, "Sync", on && sync_ok, "ssync") && sync_ok {
                p.put(sa::sid(cur, o::SPEED_SYNC), if on { 0.0 } else { 1.0 });
            }
        }
        1 => {
            knob_t(p, 60.0, 424.0, 56.0, sa::sid(cur, o::LOOP_SPEED), "Loop Speed");
            let on = p.on(sa::sid(cur, o::LOOP_SYNC));
            let r = p.rect(34.0, 502.0, 52.0, 18.0);
            if p.button(r, "Sync", on && sync_ok, "lsync") && sync_ok {
                p.put(sa::sid(cur, o::LOOP_SYNC), if on { 0.0 } else { 1.0 });
            }
        }
        2 => knob_t(p, 60.0, 424.0, 56.0, sa::sid(cur, o::JITTER), "Scrub Jitter"),
        3 => {
            knob_t(p, 60.0, 424.0, 56.0, sa::sid(cur, o::BOW_RATE), "Bow Rate");
            p.menu(22.0, 502.0, 76.0, 18.0, sa::sid(cur, o::BOW_SYNC));
        }
        _ => {
            p.menu(22.0, 444.0, 76.0, 20.0, sa::sid(cur, o::ARP_RATE));
            p.label(60.0, 478.0, "Arp Rate (tempo)");
        }
    }
    knob_t(p, 130.0, 424.0, 44.0, sa::sid(cur, o::PAN), "Pan");
    knob_t(p, 194.0, 424.0, 44.0, sa::sid(cur, o::COARSE), "Tune Coarse");
    knob_t(p, 254.0, 424.0, 44.0, sa::sid(cur, o::FINE), "Tune Fine");
    p.label(190.0, 530.0, "Pressing a knob makes it the Mod Matrix target");
}

fn synthesis_module(p: &mut Pane, cur: usize, mode: usize, fx: usize) {
    p.group(298.0, 372.0, 322.0, 210.0, "SYNTHESIS");
    let cur_mode = mode;
    let r = p.rect(306.0, 392.0, 306.0, 22.0);
    let w = 306.0 / 3.0;
    for (k, l) in sa::MODES.iter().enumerate() {
        let br = Rect::from_min_size(r.min + vec2(w * k as f32, 0.0), vec2(w - 2.0, 22.0));
        if p.button(br, l, k == cur_mode, ("smode", k)) && k != cur_mode {
            put_effect(p, cur, k, 0);
        }
    }
    let fr = p.rect(306.0, 420.0, 170.0, 20.0);
    let fxs: Vec<String> = sa::effects(mode).iter().map(|s| s.to_string()).collect();
    let resp = drop_button(p, fr, &format!("Effect: {}", fxs[fx]), "fx");
    if let Some(i) = pick_menu(&resp, &fxs, fx) {
        put_effect(p, cur, mode, i);
    }
    let names = sa::knob_names(mode, fx);
    for (k, n) in names.iter().enumerate() {
        if n.is_empty() {
            continue;
        }
        knob_t(p, 340.0 + k as f32 * 76.0, 462.0, 52.0, sa::sid(cur, o::K1 + k as u32), n);
    }
}

fn filter_module(p: &mut Pane, cur: usize) {
    p.group(628.0, 372.0, 264.0, 210.0, "FILTER");
    let global = p.on(sa::GLOBAL_FILTER);
    let fs = if global { 4 } else { cur };
    p.toggle(636.0, 392.0, 44.0, 20.0, sa::sid(fs, o::F_ON), "On");
    p.menu(686.0, 392.0, 100.0, 20.0, sa::sid(fs, o::F_TYPE));
    p.toggle(794.0, 392.0, 90.0, 20.0, sa::GLOBAL_FILTER, "Global");
    let ty = p.geti(sa::sid(fs, o::F_TYPE)).min(5);
    if ty < 3 {
        p.menu(636.0, 420.0, 120.0, 20.0, sa::sid(fs, o::F_VAR));
    }
    let (a, b) = match ty {
        0..=2 => (sa::sid(fs, o::F_CUT), sa::sid(fs, o::F_RES)),
        3 => (sa::sid(fs, o::F_NOTE), sa::sid(fs, o::F_FB)),
        4 => (sa::sid(fs, o::F_FREQ), sa::sid(fs, o::F_MIX)),
        _ => (sa::sid(fs, o::F_NOTE), sa::sid(fs, o::F_MOD)),
    };
    let (la, lb) = match ty {
        0..=2 => ("Cutoff", "Res"),
        3 => ("Note", "Feedback"),
        4 => ("Freq", "Mix"),
        _ => ("Note", "Mod"),
    };
    let on = p.on(sa::sid(fs, o::F_ON));
    let _ = on;
    if global {
        // the global filter is one block: its knobs are not per-source mod targets
        p.knob(690.0, 462.0, 52.0, a, la);
        p.knob(810.0, 462.0, 52.0, b, lb);
    } else {
        knob_t(p, 690.0, 462.0, 52.0, a, la);
        knob_t(p, 810.0, 462.0, 52.0, b, lb);
    }
    p.label(760.0, 540.0, if global { "One filter for every source" } else { "This source's filter" });
}
