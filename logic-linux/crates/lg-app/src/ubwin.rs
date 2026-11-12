//! Ultrabeat's window, laid out as Logic's: the Assignment section at the left (the 25 drum
//! sounds on a small keyboard with each sound's name, volume, pan, mute, solo and output), the
//! Synthesizer section of the selected sound beside it (oscillators 1 and 2, ring modulator,
//! noise generator, the filter with its distortion circuit, the two-band EQ, pan modulation /
//! stereo spread, voice volume and trigger settings, below them the four envelopes and two LFOs
//! with the mod / via routing), and the step sequencer along the bottom (pattern, length,
//! resolution, swing, accent, the trigger / velocity-and-gate / parameter-offset rows, the
//! full view, MIDI control of pattern switching). MIDI controllers A–D are under Extended
//! Parameters.
//!
//! Exporting a pattern: dragging the handle left of the Pattern menu leaves the pattern number
//! in the window's data under `EXPORT_KEY` for the host to turn into a MIDI region
//! (`Ultrabeat::pattern_notes`).

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_dsp::{Instrument, Plugin};
use lg_synth::ultrabeat::{self as ub, vid, Cell, Init, Ultrabeat};
use lg_synth::ultrabeat_dsp as dspu;

use crate::app::App;
use crate::instwin;
use crate::look;
use crate::synthui::{self, rgb, Pane, Skin};

const SKIN: Skin = Skin::new(0x3a3d42, 0x2e3135, 0x7fb0e0);
const BLUE: Color32 = rgb(0x4a90e2);
const GREEN: Color32 = rgb(0x5fc46a);
const RED: Color32 = rgb(0xe0504a);
const YELLOW: Color32 = rgb(0xe8c840);
const DARK: Color32 = rgb(0x1c1e21);

/// Where a pattern drag leaves its number (egui temp data: `Id::new((EXPORT_KEY, track))`
/// holds the pattern index, 0…23).
pub const EXPORT_KEY: &str = "ub-export";

const W: f32 = 1020.0;
const H: f32 = 925.0;
const SEQ_TOP: f32 = 732.0;

/// What the panel reads from the instrument each frame.
struct Snap {
    sel: usize,
    pat: usize,
    len: usize,
    res: usize,
    cells: Vec<Cell>,
    hits: [f32; 25],
    step: Option<usize>,
    names: Vec<String>,
    sample: Option<String>,
    preview: Vec<f32>,
    learning: Option<usize>,
    pm: Option<usize>,
}

fn snapshot(app: &App, t: usize) -> Option<Snap> {
    instwin::with_inst::<Ultrabeat, _>(app, t, |u| {
        let sel = u.selected();
        let pat = (u.get(ub::G_PAT).round().max(0.0) as usize).min(23);
        let mut hits = [0.0; 25];
        for (v, h) in hits.iter_mut().enumerate() {
            *h = u.hit_level(v);
        }
        Snap {
            sel,
            pat,
            len: u.pattern_len(pat),
            res: u.pattern_res(pat),
            cells: (0..25).map(|v| u.cell(pat, v).clone()).collect(),
            hits,
            step: u.playing_step(),
            names: (0..25).map(|v| u.voice_name(v).to_string()).collect(),
            sample: u.sample_name(sel).map(|s| s.to_string()),
            preview: u.sample_preview(sel, 200),
            learning: u.learning(),
            pm: u.pattern_mode_playing(),
        }
    })
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some(snap) = snapshot(app, t) else { return };
    let ext: Vec<u32> = (0..4).map(|i| ub::G_CTRL + i).collect();
    let acts = synthui::window(app, ctx, t, "Ultrabeat", vec2(W, H), SKIN, &ext, |p| body(p, &snap));
    for a in acts {
        act(app, ctx, t, &a);
    }
    // while a controller is being learned the window keeps repainting; after 20 seconds
    // without a suitable controller it reverts to the previous assignment
    let lid = egui::Id::unique(("ub-learn", t));
    if snap.learning.is_some() {
        let now = ctx.input(|i| i.time);
        let start: f64 = ctx.data(|d| d.get_temp(lid)).unwrap_or(now);
        ctx.data_mut(|d| d.insert_temp(lid, start));
        if now - start > 20.0 {
            instwin::with_inst::<Ultrabeat, _>(app, t, |u| u.cancel_learn());
        }
        ctx.request_repaint();
    } else {
        ctx.data_mut(|d| d.remove::<f64>(lid));
    }
    if snap.step.is_some() || snap.hits.iter().any(|h| *h > 0.02) {
        ctx.request_repaint();
    }
}

// ---------------------------------------------------------------------------------------------
// actions the panel asks for (the instrument itself changes, not only parameters)

#[derive(Clone, Default)]
struct Clip {
    vp: Vec<f32>,
    cells: Vec<Cell>,
    pattern: Vec<Cell>,
    pat_len: usize,
    pat_res: usize,
}

fn clip_id(t: usize) -> egui::Id {
    egui::Id::unique(("ub-clip", t))
}

/// Export Ultrabeat patterns as MIDI regions: the pattern's notes as a MIDI region on the
/// track, at the bar the playhead is in (one undo step).
pub fn export_pattern(app: &mut App, t: usize, pattern: usize) -> bool {
    let Some(notes) = instwin::with_inst::<Ultrabeat, _>(app, t, |u| u.pattern_notes(pattern)) else { return false };
    let ppq = lg_core::events::PPQ as f64;
    let bar = app.bar_ticks().max(1);
    let pos = app.engine.lock().map(|e| e.pos).unwrap_or(0.0).max(0.0) as u32;
    let at = pos / bar * bar;
    let end = notes.iter().map(|n| ((n.0 + n.3) * ppq) as u32).max().unwrap_or(0);
    let length = end.div_ceil(bar).max(1) * bar;
    let Some(name) = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| format!("{} Pattern {}", tr.name, pattern + 1)) else { return false };
    app.checkpoint("Export Ultrabeat Pattern");
    let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return false };
    let notes = notes.iter().map(|&(b, pitch, velocity, len)| lg_core::events::Note { channel: 0, pitch, velocity, pos: at + (b * ppq).round() as u32, len: ((len * ppq).round() as u32).max(1), muted: false }).collect();
    tr.regions.push(lg_core::Region { name, start: at, length, notes, audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
    let r = tr.regions.len() - 1;
    app.selected_region = Some((t, r));
    app.after_edit();
    true
}

fn key_note(v: usize) -> u8 {
    if v < 24 { 36 + v as u8 } else { 60 }
}

fn act(app: &mut App, ctx: &egui::Context, t: usize, a: &str) {
    let parts: Vec<&str> = a.split(':').collect();
    let n = |i: usize| parts.get(i).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
    let mut clip: Clip = ctx.data(|d| d.get_temp(clip_id(t))).unwrap_or_default();
    let cur_pat = |app: &App| instwin::with_inst::<Ultrabeat, _>(app, t, |u| (u.get(ub::G_PAT).round().max(0.0) as usize).min(23)).unwrap_or(0);
    match parts[0] {
        "kdn" => {
            instwin::with_inst::<Ultrabeat, _>(app, t, |u| u.note_on(key_note(n(1)), 100));
        }
        "kup" => {
            instwin::with_inst::<Ultrabeat, _>(app, t, |u| u.note_off(key_note(n(1))));
        }
        "copy" => {
            let v = n(1);
            if let Some((vp, cells)) = instwin::with_inst::<Ultrabeat, _>(app, t, |u| (u.voice_params(v).to_vec(), (0..24).map(|p| u.cell(p, v).clone()).collect::<Vec<_>>())) {
                clip.vp = vp;
                clip.cells = cells;
            }
        }
        "paste" if !clip.vp.is_empty() => {
            let mut src = [0.0f32; 256];
            src.copy_from_slice(&clip.vp);
            instwin::checkpoint(app, t, "Paste Voice");
            instwin::set_params(app, t, &Ultrabeat::assign_pairs(n(1), &src, false));
        }
        "pasteseq" if !clip.cells.is_empty() => {
            // 0…23: one sequence into the active pattern, 24: all of them
            let (v, which) = (n(1), n(2));
            let pat = cur_pat(app);
            let mut pairs = Vec::new();
            instwin::with_inst::<Ultrabeat, _>(app, t, |u| {
                if which >= 24 {
                    for p in 0..24 {
                        pairs.extend(u.cell_pairs(p, v, &clip.cells[p]));
                    }
                } else {
                    pairs.extend(u.cell_pairs(pat, v, &clip.cells[which]));
                }
            });
            instwin::checkpoint(app, t, "Paste Sequence");
            instwin::set_params(app, t, &pairs);
        }
        "swapclip" if !clip.vp.is_empty() => {
            let v = n(1);
            let cur = instwin::with_inst::<Ultrabeat, _>(app, t, |u| (u.voice_params(v).to_vec(), (0..24).map(|p| u.cell(p, v).clone()).collect::<Vec<_>>()));
            if let Some((vp, cells)) = cur {
                let mut src = [0.0f32; 256];
                src.copy_from_slice(&clip.vp);
                let mut pairs = Ultrabeat::assign_pairs(v, &src, false);
                instwin::with_inst::<Ultrabeat, _>(app, t, |u| {
                    for p in 0..24 {
                        pairs.extend(u.cell_pairs(p, v, &clip.cells[p]));
                    }
                });
                instwin::checkpoint(app, t, "Swap with Clipboard");
                instwin::set_params(app, t, &pairs);
                clip.vp = vp;
                clip.cells = cells;
            }
        }
        "init" => {
            let kind = ub::INITS.get(n(2)).map(|x| x.1).unwrap_or(Init::Standard);
            if let Some(pairs) = instwin::with_inst::<Ultrabeat, _>(app, t, |u| u.init_pairs(n(1), kind)) {
                instwin::checkpoint(app, t, "Init Sound");
                instwin::set_params(app, t, &pairs);
            }
        }
        "swap" | "dup" => {
            let (a, b) = (n(1), n(2));
            if a != b {
                let pa = instwin::with_inst::<Ultrabeat, _>(app, t, |u| u.voice_params(a));
                let pb = instwin::with_inst::<Ultrabeat, _>(app, t, |u| u.voice_params(b));
                if let (Some(pa), Some(pb)) = (pa, pb) {
                    // the sounds with their mixer settings change places; the sequences stay
                    let mut pairs = Ultrabeat::assign_pairs(b, &pa, false);
                    if parts[0] == "swap" {
                        pairs.extend(Ultrabeat::assign_pairs(a, &pb, false));
                    }
                    instwin::checkpoint(app, t, if parts[0] == "swap" { "Swap Sounds" } else { "Copy Sound" });
                    instwin::set_params(app, t, &pairs);
                }
            }
        }
        "kit" => {
            let fresh = Ultrabeat::new(48000.0);
            let mut pairs = Vec::new();
            for v in 0..25 {
                pairs.extend(Ultrabeat::assign_pairs(v, &fresh.voice_params(v), true));
            }
            instwin::checkpoint(app, t, "Load Tutorial Kit");
            instwin::set_params(app, t, &pairs);
        }
        "load-sample" => {
            let file = kn_ui::files::FileDialog::new().set_title("Load Sample").add_filter("Audio", &["wav", "aif", "aiff", "caf", "WAV", "AIF", "AIFF"]).pick_file();
            if let Some(f) = file {
                match lg_synth::wav::read_audio(&f) {
                    Ok(au) => {
                        let mut mono = vec![0.0f32; au.frames()];
                        for ch in &au.channels {
                            for (m, x) in mono.iter_mut().zip(ch) {
                                *m += x / au.channels.len() as f32;
                            }
                        }
                        let name = f.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                        let v = instwin::with_inst::<Ultrabeat, _>(app, t, |u| {
                            let v = u.selected();
                            u.load_sample(v, mono, au.rate as f32, &name);
                            v
                        });
                        if let Some(v) = v {
                            instwin::checkpoint(app, t, "Load Sample");
                            instwin::set_params(app, t, &[(vid(v, ub::O2_ON), 1.0), (vid(v, ub::O2_MODE), 1.0)]);
                        }
                    }
                    Err(e) => app.tell(format!("Couldn't load {}: {e}", f.display())),
                }
            }
        }
        "unload-sample" => {
            instwin::with_inst::<Ultrabeat, _>(app, t, |u| {
                let v = u.selected();
                u.unload_sample(v);
            });
        }
        "learn" => {
            instwin::with_inst::<Ultrabeat, _>(app, t, |u| u.start_learn(n(1)));
            ctx.data_mut(|d| d.remove::<f64>(egui::Id::unique(("ub-learn", t))));
        }
        "unlearn" => {
            instwin::with_inst::<Ultrabeat, _>(app, t, |u| u.cancel_learn());
        }
        "export" => {
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique((EXPORT_KEY, t)), n(1)));
            export_pattern(app, t, n(1));
        }
        "copyp" => {
            let pat = cur_pat(app);
            if let Some(c) = instwin::with_inst::<Ultrabeat, _>(app, t, |u| (u.pattern_len(pat), u.pattern_res(pat), (0..25).map(|v| u.cell(pat, v).clone()).collect::<Vec<_>>())) {
                clip.pat_len = c.0;
                clip.pat_res = c.1;
                clip.pattern = c.2;
            }
        }
        "pastep" | "clearp" => {
            let paste = parts[0] == "pastep";
            if !paste || !clip.pattern.is_empty() {
                let pat = cur_pat(app);
                let mut pairs = Vec::new();
                instwin::with_inst::<Ultrabeat, _>(app, t, |u| {
                    for v in 0..25 {
                        let c = if paste { clip.pattern[v].clone() } else { Cell::default() };
                        pairs.extend(u.cell_pairs(pat, v, &c));
                    }
                });
                if paste {
                    pairs.push((ub::pat_len_id(pat), clip.pat_len as f32));
                    pairs.push((ub::pat_res_id(pat), clip.pat_res as f32));
                }
                instwin::checkpoint(app, t, if paste { "Paste Pattern" } else { "Clear Pattern" });
                instwin::set_params(app, t, &pairs);
            }
        }
        _ => {}
    }
    ctx.data_mut(|d| d.insert_temp(clip_id(t), clip));
}

// ---------------------------------------------------------------------------------------------
// small helpers

fn vg(p: &Pane, v: usize, k: u32) -> f32 {
    p.get(vid(v, k))
}
fn vput(p: &mut Pane, v: usize, k: u32, x: f32) {
    p.put(vid(v, k), x);
}
fn vtoggle(p: &mut Pane, v: usize, x: f32, y: f32, w: f32, h: f32, k: u32, label: &str) {
    p.toggle(x, y, w, h, vid(v, k), label);
}
fn vseg(p: &mut Pane, v: usize, x: f32, y: f32, w: f32, h: f32, k: u32, labels: &[&str]) {
    p.seg(x, y, w, h, vid(v, k), labels, false);
}

fn st<T: Clone + Send + Sync + 'static>(p: &Pane, name: &str, default: T) -> T {
    p.ui.ctx().data(|d| d.get_temp(egui::Id::unique(("ub-st", p.t, name)))).unwrap_or(default)
}
fn set_st<T: Clone + Send + Sync + 'static>(p: &Pane, name: &str, x: T) {
    p.ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(("ub-st", p.t, name)), x));
}

/// A ring round a knob: the modulated range, blue for the mod amount, green for the via amount.
fn mod_ring(p: &Pane, x: f32, y: f32, d: f32, frac: f32, mod_end: f32, via_end: Option<f32>) {
    let c = p.o + vec2(x, y + d / 2.0 + 2.0);
    let start = std::f32::consts::PI * (0.5 + 1.0 / 6.0);
    let sweep = std::f32::consts::PI * 5.0 / 3.0;
    let pt = p.painter();
    let arc = |a: f32, b: f32, col: Color32| {
        let n = 24;
        let pts: Vec<egui::Pos2> = (0..=n)
            .map(|k| {
                let ang = start + sweep * (a + (b - a) * k as f32 / n as f32);
                c + vec2(ang.cos(), ang.sin()) * (d / 2.0 + 2.5)
            })
            .collect();
        pt.add(egui::Shape::line(pts, Stroke::new(2.5, col)));
    };
    arc(frac.min(mod_end), frac.max(mod_end), BLUE);
    if let Some(v) = via_end {
        arc(mod_end.min(v), mod_end.max(v), GREEN);
    }
}

/// One mod or via chip: a click opens the source menu, a drag up or down sets the amount.
#[allow(clippy::too_many_arguments)]
fn chip(p: &mut Pane, v: usize, r: Rect, label: &str, active: bool, col: Color32, srck: u32, ampk: u32, choices: &[&str], outline: bool) {
    let resp = p.ui.interact(r, egui::Id::unique(("ub-chip", p.t, v, srck)), Sense::click_and_drag());
    let pt = p.ui.painter();
    pt.rect_filled(r, 3.0, if active { col } else if resp.hovered() { Color32::from_gray(0x46) } else { Color32::from_gray(0x34) });
    pt.text(r.center(), Align2::CENTER_CENTER, label, look::regular(8.5), if active { Color32::from_gray(0x10) } else { p.skin.dim });
    if outline {
        pt.rect_stroke(r.expand(1.0), 3.0, Stroke::new(1.5, YELLOW), egui::StrokeKind::Outside);
    }
    let cur = p.geti(vid(v, srck));
    if active {
        let amt = p.get(vid(v, ampk));
        let resp = resp.clone().on_hover_text(format!("{label}: {amt:+.2}"));
        if resp.dragged() {
            p.put(vid(v, ampk), (amt - resp.drag_delta().y / 100.0).clamp(-1.0, 1.0));
        }
        if resp.double_clicked() {
            let d = p.prm(vid(v, ampk)).default;
            p.put(vid(v, ampk), d);
        }
    }
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        for (i, c) in choices.iter().enumerate() {
            if ui.add(egui::Button::selectable(i == cur, *c)).clicked() {
                pick = Some(i);
                ui.close();
            }
        }
    });
    if let Some(i) = pick {
        p.put(vid(v, srck), i as f32);
    }
}

/// The mod (blue) and via (green) menus under a modulatable control; with the mod Off the via
/// chip is hidden.
fn chips(p: &mut Pane, v: usize, t: usize, cx: f32, y: f32, via_only: bool) {
    let (ks, ka, kv, kva) = (ub::mod_k(t, 0), ub::mod_k(t, 1), ub::mod_k(t, 2), ub::mod_k(t, 3));
    let src = vg(p, v, ks).round() as usize;
    let via = vg(p, v, kv).round() as usize;
    let hi: i32 = st(p, "hi", -1);
    if !via_only {
        let r = p.rect(cx - 28.0, y, 27.0, 12.0);
        let hit = hi >= 0 && src as i32 == hi && src != 0;
        chip(p, v, r, if src == 0 { "mod" } else { ub::MOD_SRC[src] }, src != 0, BLUE, ks, ka, ub::MOD_SRC, hit);
    }
    if via_only || src != 0 {
        let r = if via_only { p.rect(cx - 13.0, y, 27.0, 12.0) } else { p.rect(cx + 1.0, y, 27.0, 12.0) };
        chip(p, v, r, if via == 0 { "via" } else { ub::VIA_SRC[via] }, via != 0, GREEN, kv, kva, ub::VIA_SRC, false);
    }
}

/// A knob for a sound parameter with mod / via chips and its modulation ring when modulated
/// (`t`: the modulation target).
fn mknob(p: &mut Pane, v: usize, x: f32, y: f32, d: f32, k: u32, t: Option<usize>, label: &str) {
    p.knob(x, y, d, vid(v, k), label);
    if let Some(t) = t {
        let src = vg(p, v, ub::mod_k(t, 0)).round() as usize;
        if src != 0 {
            let pr = p.prm(vid(v, k));
            let span = (pr.max - pr.min).max(1e-9);
            let frac = (p.get(vid(v, k)) - pr.min) / span;
            let rel = ub::target_span(t) / span;
            let end = (frac + vg(p, v, ub::mod_k(t, 1)) * rel).clamp(0.0, 1.0);
            let via = vg(p, v, ub::mod_k(t, 2)).round() as usize;
            let vend = if via != 0 { Some((frac + vg(p, v, ub::mod_k(t, 3)) * rel).clamp(0.0, 1.0)) } else { None };
            mod_ring(p, x, y, d, frac, end, vend);
        }
        chips(p, v, t, x, y + d + 19.0, false);
    }
}

fn flow_button(p: &mut Pane, v: usize, r: Rect, k: u32, salt: &str) {
    let on = p.on(vid(v, k));
    if p.button(r, if on { "→ Filter" } else { "→ EQ" }, false, (salt, v)) {
        vput(p, v, k, if on { 0.0 } else { 1.0 });
    }
    if on {
        p.ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(3.0, r.height())), 1.0, RED);
    }
}

// ---------------------------------------------------------------------------------------------

fn body(p: &mut Pane, s: &Snap) {
    let v = s.sel;
    assignment(p, s);
    let r = p.rect(306.0, 8.0, 706.0, 552.0);
    p.ui.painter().rect_filled(r, 8.0, Color32::from_gray(0x26));
    oscillator1(p, v);
    oscillator2(p, s, v);
    center_column(p, v);
    output_column(p, v);
    routing_display(p, v);
    env_lfo(p, v, 314.0, 566.0);
    if p.on(ub::G_FULL) {
        full_view(p, s);
    }
    sequencer(p, s);
}

// ---------------------------------------------------------------------------------------------
// Assignment section

fn assignment(p: &mut Pane, s: &Snap) {
    p.group(8.0, 8.0, 292.0, 552.0, "ASSIGNMENT");
    p.text(16.0, 30.0, Align2::LEFT_CENTER, "Master", 10.5, p.skin.dim);
    p.hfader(54.0, 22.0, 90.0, 16.0, ub::G_MASTER);
    p.toggle(150.0, 22.0, 40.0, 16.0, ub::G_AUTOSEL, "Auto");
    let kr = p.rect(196.0, 22.0, 96.0, 16.0);
    let kresp = p.ui.interact(kr, egui::Id::unique(("ub-kit", p.t)), Sense::click());
    p.ui.painter().rect_filled(kr, 4.0, if kresp.hovered() { Color32::from_gray(0x4a) } else { Color32::from_gray(0x3a) });
    p.ui.painter().text(kr.center(), Align2::CENTER_CENTER, "Settings", look::regular(10.5), p.skin.text);
    let mut acts: Vec<String> = Vec::new();
    egui::Popup::menu(&kresp).show(|ui| {
        ui.menu_button("Tutorial Settings", |ui| {
            if ui.button("Tutorial Kit").clicked() {
                acts.push("kit".into());
                ui.close();
            }
        });
    });
    // the rows: sound 1 at the bottom, as the keyboard
    let row_h = 20.0;
    for row in 0..25usize {
        let vo = 24 - row;
        let y = 42.0 + row as f32 * row_h;
        let rr = p.rect(14.0, y, 280.0, row_h - 1.0);
        if vo == s.sel {
            p.ui.painter().rect_stroke(rr, 3.0, Stroke::new(1.5, Color32::from_gray(0xa0)), egui::StrokeKind::Inside);
        }
        // the key: blue while the sound is played; a click plays it
        let black = matches!(key_note(vo) % 12, 1 | 3 | 6 | 8 | 10);
        let kr = p.rect(16.0, y + 1.0, 20.0, row_h - 3.0);
        let kresp = p.ui.interact(kr, egui::Id::unique(("ub-key", p.t, vo)), Sense::click_and_drag());
        let lit = s.hits[vo] > 0.3 || kresp.is_pointer_button_down_on();
        p.ui.painter().rect_filled(kr, 2.0, if lit { BLUE } else if black { Color32::from_gray(0x18) } else { Color32::from_gray(0xe0) });
        if kresp.clicked() {
            p.actions.push(format!("kdn:{vo}"));
            p.actions.push(format!("kup:{vo}"));
        }
        if kresp.drag_started() {
            p.actions.push(format!("kdn:{vo}"));
        }
        if kresp.drag_stopped() {
            p.actions.push(format!("kup:{vo}"));
        }
        // the name: click selects, double-click renames, drag swaps (Option: copies)
        let nr = p.rect(40.0, y, 92.0, row_h - 1.0);
        let renaming: Option<usize> = st(p, "rename", None::<usize>);
        if renaming == Some(vo) {
            let mut text: String = st(p, "rename-text", s.names[vo].clone());
            let te = p.ui.put(nr, egui::TextEdit::singleline(&mut text).desired_width(90.0).font(look::regular(11.0)));
            if te.lost_focus() || p.ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                for (id, x) in Ultrabeat::name_pairs(vo, &text) {
                    p.put(id, x);
                }
                set_st(p, "rename", None::<usize>);
            } else {
                te.request_focus();
                set_st(p, "rename-text", text);
            }
        } else {
            let resp = p.ui.interact(nr, egui::Id::unique(("ub-name", p.t, vo)), Sense::click_and_drag());
            p.ui.painter().text(pos2(nr.min.x + 2.0, nr.center().y), Align2::LEFT_CENTER, &s.names[vo], look::regular(11.0), if vo == s.sel { Color32::WHITE } else { p.skin.text });
            if resp.clicked() {
                p.put(ub::G_SEL, vo as f32);
            }
            if resp.double_clicked() {
                set_st(p, "rename", Some(vo));
                set_st(p, "rename-text", s.names[vo].clone());
            }
            if resp.dragged() {
                if let Some(pp) = resp.interact_pointer_pos() {
                    let ty = ((pp.y - p.o.y - 42.0) / row_h).floor() as i32;
                    if (0..25).contains(&ty) {
                        let tr = p.rect(14.0, 42.0 + ty as f32 * row_h, 280.0, row_h - 1.0);
                        p.ui.painter().rect_filled(tr, 3.0, YELLOW.gamma_multiply(0.25));
                        set_st(p, "drag-target", 24 - ty);
                    }
                }
            }
            if resp.drag_stopped() {
                let target: i32 = st(p, "drag-target", -1);
                if target >= 0 && target as usize != vo {
                    let alt = p.ui.input(|i| i.modifiers.alt);
                    p.actions.push(format!("{}:{vo}:{target}", if alt { "dup" } else { "swap" }));
                }
                set_st(p, "drag-target", -1i32);
            }
            resp.context_menu(|ui| {
                if ui.button("Copy (Voice & Seq)").clicked() {
                    acts.push(format!("copy:{vo}"));
                    ui.close();
                }
                if ui.button("Paste Voice").clicked() {
                    acts.push(format!("paste:{vo}"));
                    ui.close();
                }
                ui.menu_button("Paste Sequence", |ui| {
                    if ui.button("All").clicked() {
                        acts.push(format!("pasteseq:{vo}:24"));
                        ui.close();
                    }
                    for i in 0..24 {
                        if ui.button(format!("{}", i + 1)).clicked() {
                            acts.push(format!("pasteseq:{vo}:{i}"));
                            ui.close();
                        }
                    }
                });
                if ui.button("Swap with Clipboard").clicked() {
                    acts.push(format!("swapclip:{vo}"));
                    ui.close();
                }
                ui.menu_button("Init", |ui| {
                    for (i, (n, _)) in ub::INITS.iter().enumerate() {
                        if ui.button(*n).clicked() {
                            acts.push(format!("init:{vo}:{i}"));
                            ui.close();
                        }
                    }
                });
            });
        }
        p.hfader(136.0, y, 56.0, row_h - 2.0, vid(vo, ub::MIX_VOL));
        p.hfader(194.0, y, 32.0, row_h - 2.0, vid(vo, ub::MIX_PAN));
        p.toggle(228.0, y + 3.0, 14.0, 13.0, vid(vo, ub::MUTE), "M");
        p.toggle(244.0, y + 3.0, 14.0, 13.0, vid(vo, ub::SOLO), "S");
        p.menu(260.0, y + 2.0, 34.0, 15.0, vid(vo, ub::OUTPUT));
    }
    // Command-dragging a volume or an output sets it for all sounds
    if p.ui.input(|i| i.modifiers.command) {
        for (id, x) in p.set.clone() {
            if id >= ub::VBASE && id < ub::VBASE + 25 * ub::VSTRIDE {
                let k = (id - ub::VBASE) % ub::VSTRIDE;
                if k == ub::MIX_VOL || k == ub::OUTPUT {
                    for o in 0..25 {
                        p.put(vid(o, k), x);
                    }
                }
            }
        }
    }
    p.actions.extend(acts);
}

// ---------------------------------------------------------------------------------------------
// oscillators

fn osc_curve(slope: f32, sat: f32, asym: f32) -> Vec<(f32, f32)> {
    (0..=120)
        .map(|i| {
            let ph = (i as f32 / 120.0 * 2.0).fract();
            (i as f32 / 120.0, dspu::phase_osc(ph, slope, sat, asym) * 0.45 + 0.5)
        })
        .collect()
}

fn oscillator1(p: &mut Pane, v: usize) {
    p.group(314.0, 14.0, 222.0, 200.0, "OSC 1");
    vtoggle(p, v, 420.0, 16.0, 30.0, 14.0, ub::O1_ON, "On");
    let mode = p.geti(vid(v, ub::O1_MODE));
    vseg(p, v, 316.0, 34.0, 214.0, 15.0, ub::O1_MODE, &["Phase Osc", "FM", "Side Chain"]);
    let d = p.display(318.0, 54.0, 150.0, 48.0);
    match mode {
        0 => p.curve(d, &osc_curve(vg(p, v, ub::O1_SLOPE), vg(p, v, ub::O1_SAT), vg(p, v, ub::O1_ASYM)), BLUE),
        1 => {
            let fm = vg(p, v, ub::O1_FM);
            let pts: Vec<(f32, f32)> = (0..=120)
                .map(|i| {
                    let x = i as f32 / 120.0;
                    (x, 0.5 + 0.42 * (std::f32::consts::TAU * (2.0 * x + fm * 0.8 * (std::f32::consts::TAU * 6.0 * x).sin())).sin())
                })
                .collect();
            p.curve(d, &pts, BLUE);
        }
        _ => p.text(d.center().x - p.o.x, d.center().y - p.o.y, Align2::CENTER_CENTER, "Side Chain", 11.0, p.skin.dim),
    }
    let fr = p.rect(472.0, 54.0, 56.0, 15.0);
    flow_button(p, v, fr, ub::O1_FLOW, "o1flow");
    p.text(318.0, 114.0, Align2::LEFT_CENTER, "Pitch", 10.5, p.skin.dim);
    p.field(350.0, 106.0, 60.0, 16.0, vid(v, ub::O1_PITCH), 1.0);
    chips(p, v, ub::T_O1_PITCH, 452.0, 108.0, false);
    mknob(p, v, 346.0, 128.0, 34.0, ub::O1_VOL, Some(ub::T_O1_VOL), "Volume");
    if mode == 0 {
        mknob(p, v, 402.0, 128.0, 34.0, ub::O1_SLOPE, Some(ub::T_O1_SLOPE), "Slope");
        mknob(p, v, 458.0, 128.0, 34.0, ub::O1_SAT, Some(ub::T_O1_SAT), "Sat");
        mknob(p, v, 514.0, 128.0, 34.0, ub::O1_ASYM, Some(ub::T_O1_ASYM), "Asym");
    } else if mode == 1 {
        mknob(p, v, 402.0, 128.0, 34.0, ub::O1_FM, Some(ub::T_O1_FM), "FM Amt");
    }
}

fn oscillator2(p: &mut Pane, s: &Snap, v: usize) {
    p.group(314.0, 220.0, 222.0, 332.0, "OSC 2");
    vtoggle(p, v, 420.0, 222.0, 30.0, 14.0, ub::O2_ON, "On");
    let mode = p.geti(vid(v, ub::O2_MODE));
    vseg(p, v, 316.0, 240.0, 214.0, 15.0, ub::O2_MODE, &["Phase Osc", "Sample", "Model"]);
    let d = p.display(318.0, 260.0, 150.0, 48.0);
    let mut acts: Vec<String> = Vec::new();
    match mode {
        0 => p.curve(d, &osc_curve(vg(p, v, ub::O2_SLOPE), vg(p, v, ub::O2_SAT), vg(p, v, ub::O2_ASYM)), BLUE),
        1 => {
            if s.preview.is_empty() {
                p.text(d.center().x - p.o.x, d.center().y - p.o.y, Align2::CENTER_CENTER, "No sample", 11.0, p.skin.dim);
            } else {
                let pts: Vec<(f32, f32)> = s.preview.iter().enumerate().map(|(i, x)| (i as f32 / s.preview.len() as f32, 0.5 + x * 0.45)).collect();
                p.curve(d, &pts, BLUE);
            }
            // the arrow at the upper left: load or unload
            let ar = p.rect(318.0, 260.0, 18.0, 14.0);
            let resp = p.ui.interact(ar, egui::Id::unique(("ub-sample-menu", p.t)), Sense::click());
            crate::glyphs::paint(p.ui.painter(), ar, crate::glyphs::ph::CARET_DOWN, 10.0, Color32::WHITE);
            egui::Popup::menu(&resp).show(|ui| {
                if ui.button("Load Sample…").clicked() {
                    acts.push("load-sample".into());
                    ui.close();
                }
                if ui.add_enabled(s.sample.is_some(), egui::Button::new("Unload Sample")).clicked() {
                    acts.push("unload-sample".into());
                    ui.close();
                }
            });
        }
        _ => {
            // the string's partials, as the model builds them
            let (stf, lo) = (vg(p, v, ub::M_STIFF), vg(p, v, ub::M_LOSS));
            let pts: Vec<(f32, f32)> = (1..=14)
                .flat_map(|k| {
                    let x = (k as f32 * (1.0 + 0.08 * stf.powi(3) * (k * k) as f32).sqrt()).min(40.0) / 40.0;
                    let a = (1.0 / k as f32) / (1.0 + (k as f32 - 1.0).powi(2) * (lo * lo * 2.0 + 0.01));
                    [(x, 0.05), (x, 0.05 + a * 0.9), (x, 0.05)]
                })
                .collect();
            p.curve(d, &pts, BLUE);
        }
    }
    p.actions.extend(acts);
    flow_button(p, v, p.rect(472.0, 260.0, 56.0, 15.0), ub::O2_FLOW, "o2flow");
    p.text(318.0, 320.0, Align2::LEFT_CENTER, "Pitch", 10.5, p.skin.dim);
    p.field(350.0, 312.0, 60.0, 16.0, vid(v, ub::O2_PITCH), 1.0);
    chips(p, v, ub::T_O2_PITCH, 452.0, 314.0, false);
    mknob(p, v, 346.0, 334.0, 34.0, ub::O2_VOL, Some(ub::T_O2_VOL), "Volume");
    match mode {
        0 => {
            mknob(p, v, 402.0, 334.0, 34.0, ub::O2_SLOPE, Some(ub::T_O2_SLOPE), "Slope");
            mknob(p, v, 458.0, 334.0, 34.0, ub::O2_SAT, Some(ub::T_O2_SAT), "Sat");
            mknob(p, v, 514.0, 334.0, 34.0, ub::O2_ASYM, Some(ub::T_O2_ASYM), "Asym");
        }
        1 => {
            p.text(318.0, 404.0, Align2::LEFT_CENTER, s.sample.as_deref().unwrap_or("(no sample)"), 10.5, p.skin.text);
            p.range(326.0, 430.0, 200.0, 24.0, vid(v, ub::S_MIN), vid(v, ub::S_MAX), "Start Min / Max (velocity 1 / 127)", BLUE);
            let rv = p.on(vid(v, ub::S_REV));
            vtoggle(p, v, 318.0, 466.0, 80.0, 16.0, ub::S_REV, if rv { "Backward" } else { "Forward" });
        }
        _ => {
            vseg(p, v, 402.0, 340.0, 126.0, 16.0, ub::M_EXC, &["Exciter 1", "Exciter 2"]);
            // the Material Pad: Inner Loss to the right, Stiffness upward
            p.xy(326.0, 404.0, 120.0, 100.0, vid(v, ub::M_LOSS), vid(v, ub::M_STIFF));
            p.text(386.0, 516.0, Align2::CENTER_CENTER, "Inner Loss →   Stiffness ↑", 9.5, p.skin.dim);
            p.vfader(470.0, 400.0, 26.0, 106.0, vid(v, ub::M_RES), "Res");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// ring modulator, filter and distortion, noise

fn center_column(p: &mut Pane, v: usize) {
    p.group(544.0, 14.0, 180.0, 72.0, "RING MOD");
    vtoggle(p, v, 640.0, 16.0, 30.0, 14.0, ub::RM_ON, "On");
    flow_button(p, v, p.rect(672.0, 16.0, 46.0, 14.0), ub::RM_FLOW, "rmflow");
    mknob(p, v, 580.0, 30.0, 26.0, ub::RM_VOL, Some(ub::T_RM_VOL), "Level");
    // filter and distortion
    p.group(544.0, 92.0, 180.0, 236.0, "FILTER");
    let fon = p.on(vid(v, ub::F_ON));
    let fr = p.rect(590.0, 94.0, 40.0, 14.0);
    if p.button(fr, "Filter", fon, ("fon", v)) {
        vput(p, v, ub::F_ON, if fon { 0.0 } else { 1.0 });
    }
    p.ui.painter().text(fr.min + vec2(-4.0, 7.0), Align2::RIGHT_CENTER, "●", look::regular(9.0), if fon { RED } else { Color32::from_gray(0x70) });
    vseg(p, v, 548.0, 112.0, 130.0, 15.0, ub::F_TYPE, &["LP", "HP", "BP", "BR"]);
    vseg(p, v, 682.0, 112.0, 38.0, 15.0, ub::F_SLOPE, &["12", "24"]);
    mknob(p, v, 590.0, 134.0, 40.0, ub::F_CUT, Some(ub::T_F_CUT), "Cut");
    mknob(p, v, 670.0, 134.0, 40.0, ub::F_RES, Some(ub::T_F_RES), "Res");
    // the arrow: the distortion before (up) or after (down) the filter
    let before = p.on(vid(v, ub::D_POS));
    let ar = p.rect(560.0, 206.0, 156.0, 14.0);
    if p.button(ar, if before { "▲ Distortion → Filter" } else { "▼ Filter → Distortion" }, false, ("dpos", v)) {
        vput(p, v, ub::D_POS, if before { 0.0 } else { 1.0 });
    }
    vseg(p, v, 548.0, 224.0, 170.0, 15.0, ub::D_MODE, &["Off", "Crush", "Distort"]);
    mknob(p, v, 572.0, 244.0, 28.0, ub::D_DRIVE, Some(ub::T_D_DRIVE), "Drive");
    mknob(p, v, 634.0, 244.0, 28.0, ub::D_COLOR, None, "Color");
    mknob(p, v, 692.0, 244.0, 28.0, ub::D_LEVEL, None, "Level");
    // noise generator
    p.group(544.0, 334.0, 180.0, 218.0, "NOISE");
    vtoggle(p, v, 590.0, 336.0, 30.0, 14.0, ub::NZ_ON, "On");
    flow_button(p, v, p.rect(672.0, 336.0, 46.0, 14.0), ub::NZ_FLOW, "nzflow");
    vseg(p, v, 548.0, 354.0, 170.0, 15.0, ub::NZ_TYPE, &["LP", "HP", "BP", "byp"]);
    mknob(p, v, 570.0, 376.0, 34.0, ub::NZ_CUT, Some(ub::T_NZ_CUT), "Cut");
    mknob(p, v, 632.0, 376.0, 34.0, ub::NZ_RES, Some(ub::T_NZ_RES), "Res");
    mknob(p, v, 694.0, 376.0, 34.0, ub::NZ_DIRT, Some(ub::T_NZ_DIRT), "Dirt");
    mknob(p, v, 570.0, 452.0, 34.0, ub::NZ_VOL, Some(ub::T_NZ_VOL), "Volume");
}

// ---------------------------------------------------------------------------------------------
// output: EQ, pan modulation / spread, voice volume, trigger

fn eq_response(p: &Pane, v: usize) -> Vec<f32> {
    use lg_synth::dsp::Biquad;
    let rate = 48000.0;
    let mut bq = [Biquad::default(), Biquad::default()];
    let mut on = [false; 2];
    for b in 0..2u32 {
        let o = ub::EQ1_ON + b * 5;
        let (hz, q, g) = (vg(p, v, o + 2), vg(p, v, o + 3), vg(p, v, o + 4));
        on[b as usize] = vg(p, v, o) >= 0.5;
        if vg(p, v, o + 1) < 0.5 {
            if b == 0 {
                bq[0].low_shelf(hz, g, rate)
            } else {
                bq[1].high_shelf(hz, g, rate)
            }
        } else {
            bq[b as usize].peaking(hz, q, g, rate);
        }
    }
    let n = 512;
    let mut h = vec![0.0f32; n];
    for (i, o) in h.iter_mut().enumerate() {
        let mut x = if i == 0 { 1.0 } else { 0.0 };
        for b in 0..2 {
            if on[b] {
                x = bq[b].run(x);
            }
        }
        *o = x;
    }
    (0..64)
        .map(|k| {
            let f = 20.0 * 1000f32.powf(k as f32 / 63.0);
            let w = std::f32::consts::TAU * f / rate;
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, x) in h.iter().enumerate() {
                re += x * (w * i as f32).cos();
                im -= x * (w * i as f32).sin();
            }
            20.0 * (re * re + im * im).sqrt().max(1e-4).log10()
        })
        .collect()
}

fn output_column(p: &mut Pane, v: usize) {
    p.group(732.0, 14.0, 274.0, 156.0, "EQ");
    let d = p.display(738.0, 30.0, 262.0, 40.0);
    let resp = eq_response(p, v);
    let pts: Vec<(f32, f32)> = resp.iter().enumerate().map(|(i, db)| (i as f32 / 63.0, 0.5 + db / 40.0)).collect();
    p.curve(d, &pts, RED);
    for b in 0..2u32 {
        let o = ub::EQ1_ON + b * 5;
        let y = 76.0 + b as f32 * 46.0;
        p.text(738.0, y + 8.0, Align2::LEFT_CENTER, &format!("Band {}", b + 1), 10.0, if b == 0 { RED } else { BLUE });
        vtoggle(p, v, 786.0, y, 30.0, 16.0, o, "On");
        vseg(p, v, 820.0, y, 70.0, 16.0, o + 1, &["Shelf", "Peak"]);
        p.text(738.0, y + 28.0, Align2::LEFT_CENTER, "Freq", 9.5, p.skin.dim);
        p.field(764.0, y + 20.0, 56.0, 16.0, vid(v, o + 2), 10.0);
        p.text(826.0, y + 28.0, Align2::LEFT_CENTER, "Q", 9.5, p.skin.dim);
        p.field(838.0, y + 20.0, 40.0, 16.0, vid(v, o + 3), 0.05);
        p.text(886.0, y + 28.0, Align2::LEFT_CENTER, "Gain", 9.5, p.skin.dim);
        p.field(912.0, y + 20.0, 56.0, 16.0, vid(v, o + 4), 0.25);
    }
    p.group(732.0, 176.0, 274.0, 84.0, "PAN MOD / SPREAD");
    vseg(p, v, 738.0, 192.0, 150.0, 16.0, ub::PAN_MODE, &["Off", "Pan Mod", "Spread"]);
    match p.geti(vid(v, ub::PAN_MODE)) {
        1 => {
            // the red line shows the pan position; only the mixer's Pan control moves it
            let r = p.rect(742.0, 220.0, 150.0, 8.0);
            p.ui.painter().rect_filled(r, 3.0, DARK);
            let x = r.min.x + r.width() * (vg(p, v, ub::MIX_PAN) * 0.5 + 0.5);
            p.ui.painter().line_segment([pos2(x, r.min.y - 3.0), pos2(x, r.max.y + 3.0)], Stroke::new(2.0, RED));
            chips(p, v, ub::T_PAN, 940.0, 200.0, false);
            p.text(940.0, 226.0, Align2::CENTER_CENTER, "Intensity", 9.5, p.skin.dim);
        }
        2 => {
            p.text(742.0, 226.0, Align2::LEFT_CENTER, "Lo", 9.5, p.skin.dim);
            p.hfader(760.0, 218.0, 100.0, 16.0, vid(v, ub::SPREAD_LO));
            p.text(742.0, 246.0, Align2::LEFT_CENTER, "Hi", 9.5, p.skin.dim);
            p.hfader(760.0, 238.0, 100.0, 16.0, vid(v, ub::SPREAD_HI));
        }
        _ => {}
    }
    p.group(732.0, 266.0, 274.0, 114.0, "VOICE");
    mknob(p, v, 770.0, 284.0, 36.0, ub::VOICE_VOL, None, "Voice Vol");
    chips(p, v, ub::T_VOICE_VOL, 770.0, 284.0 + 36.0 + 19.0, true);
    p.text(826.0, 300.0, Align2::LEFT_CENTER, "Trigger", 9.5, p.skin.dim);
    p.menu(866.0, 292.0, 62.0, 16.0, vid(v, ub::TRIG));
    p.text(826.0, 324.0, Align2::LEFT_CENTER, "Group", 9.5, p.skin.dim);
    p.menu(866.0, 316.0, 62.0, 16.0, vid(v, ub::GROUP));
    vtoggle(p, v, 936.0, 292.0, 56.0, 16.0, ub::GATE, "Gate");
    vtoggle(p, v, 936.0, 316.0, 56.0, 16.0, ub::SWING_EN, "Swing");
    vtoggle(p, v, 936.0, 340.0, 56.0, 16.0, ub::ACC_EN, "Accent");
}

/// The modulation target display: every routing of the selected sound with its source and via
/// (the ones using the source chosen by its number are highlighted).
fn routing_display(p: &mut Pane, v: usize) {
    p.group(732.0, 386.0, 274.0, 166.0, "MOD ROUTINGS");
    let hi: i32 = st(p, "hi", -1);
    let src_name = if (3..=6).contains(&hi) { format!("Env {}", hi - 2) } else if hi == 1 || hi == 2 { format!("LFO {hi}") } else { "none".to_string() };
    p.text(738.0, 410.0, Align2::LEFT_CENTER, &format!("Highlighted source: {src_name}"), 9.0, p.skin.dim);
    let mut y = 424.0;
    for t in 0..ub::T_COUNT {
        let src = vg(p, v, ub::mod_k(t, 0)).round() as usize;
        if src == 0 || y > 540.0 {
            continue;
        }
        let via = vg(p, v, ub::mod_k(t, 2)).round() as usize;
        let hit = hi >= 0 && src as i32 == hi;
        let vtxt = if via != 0 { format!("  via {} {:+.2}", ub::VIA_SRC[via], vg(p, v, ub::mod_k(t, 3))) } else { String::new() };
        p.text(738.0, y, Align2::LEFT_CENTER, &format!("{} ← {} {:+.2}{vtxt}", ub::TARGET_NAMES[t], ub::MOD_SRC[src], vg(p, v, ub::mod_k(t, 1))), 9.5, if hit { YELLOW } else { p.skin.text });
        y += 12.0;
    }
}

// ---------------------------------------------------------------------------------------------
// envelopes and LFOs

fn env_lfo(p: &mut Pane, v: usize, x0: f32, y0: f32) {
    let e: usize = st(p, "env", 0usize);
    let l: usize = st(p, "lfo", 0usize);
    p.group(x0, y0, 420.0, 152.0, "ENVELOPES");
    for k in 0..4usize {
        let r = p.rect(x0 + 8.0 + k as f32 * 30.0, y0 + 18.0, 28.0, 16.0);
        if p.button(r, &format!("{}", k + 1), e == k, ("envsel", k)) {
            set_st(p, "env", k);
            set_st(p, "hi", 3 + k as i32);
        }
    }
    let b = ub::ENV0 + e as u32 * 8;
    let mut arr = [0.0f32; 256];
    for i in 0..8 {
        arr[ub::ENV0 as usize + e * 8 + i] = vg(p, v, b + i as u32);
    }
    let ep = dspu::env_params(&arr, e, 0.75);
    let d = p.display(x0 + 8.0, y0 + 40.0, 250.0, 78.0);
    // time runs on a log axis from 1 ms to 10 s, so dragging a handle never moves the scale
    let xf = |t: f32| ((t.max(1e-3) / 1e-3).log10() / 4.0).clamp(0.0, 1.0);
    let pts: Vec<(f32, f32)> = (0..=240)
        .map(|i| {
            let fx = i as f32 / 240.0;
            (fx, dspu::env_at(&ep, 1e-3 * 10f32.powf(4.0 * fx), -1.0).0)
        })
        .collect();
    p.curve(d, &pts, if e == 3 { YELLOW } else { BLUE });
    let ax = d.min.x + d.width() * xf(ep.a);
    let ex = d.min.x + d.width() * xf(ep.a + ep.d);
    p.ui.painter().line_segment([pos2(ax, d.min.y), pos2(ax, d.max.y)], Stroke::new(1.0, Color32::from_gray(0x60)));
    for (hid, hx, key) in [(0, ax, b), (1, ex, b + 2)] {
        let hr = Rect::from_center_size(pos2(hx, d.max.y - 8.0), vec2(14.0, 14.0));
        let resp = p.ui.interact(hr.expand(2.0), egui::Id::unique(("ub-envh", p.t, hid)), Sense::drag());
        p.ui.painter().rect_filled(hr, 3.0, if resp.dragged() || resp.hovered() { Color32::WHITE } else { Color32::from_gray(0xc0) });
        if resp.dragged() {
            if let Some(pp) = resp.interact_pointer_pos() {
                let fx = ((pp.x - d.min.x) / d.width()).clamp(0.0, 1.0);
                if hid == 0 {
                    p.put(vid(v, key), fx);
                } else {
                    let dd = (1e-3 * 10f32.powf(4.0 * fx) - ep.a).max(1e-3);
                    p.put(vid(v, key), ((dd / 1e-3).log10() / 4.0).clamp(0.0, 1.0));
                }
            }
        }
    }
    // reshaping by dragging on the curve itself: the segment under the pointer
    let cresp = p.ui.interact(d.shrink2(vec2(0.0, 20.0)), egui::Id::unique(("ub-envcurve", p.t)), Sense::drag());
    if cresp.dragged() {
        if let Some(pp) = cresp.interact_pointer_pos() {
            let key = if pp.x < ax { b + 1 } else { b + 3 };
            let cur = p.get(vid(v, key));
            p.put(vid(v, key), (cur - cresp.drag_delta().y * 0.012).clamp(-1.0, 1.0));
        }
    }
    if ep.sus {
        let ht = ep.a + ep.hold * ep.d;
        let hy = dspu::env_at(&ep, ht, -1.0).0;
        let c = pos2(d.min.x + d.width() * xf(ht), d.max.y - hy * d.height());
        let resp = p.ui.interact(Rect::from_center_size(c, vec2(12.0, 12.0)), egui::Id::unique(("ub-envsus", p.t)), Sense::drag());
        p.ui.painter().circle_filled(c, 4.5, RED);
        if resp.dragged() {
            if let Some(pp) = resp.interact_pointer_pos() {
                let t = 1e-3 * 10f32.powf(4.0 * ((pp.x - d.min.x) / d.width()).clamp(0.0, 1.0));
                p.put(vid(v, b + 5), ((t - ep.a) / ep.d).clamp(0.0, 1.0));
            }
        }
    }
    let txt = format!("A {}   D {}", p.shown(vid(v, b), vg(p, v, b)), p.shown(vid(v, b + 2), vg(p, v, b + 2)));
    p.text(x0 + 8.0, y0 + 134.0, Align2::LEFT_CENTER, &txt, 10.0, p.skin.text);
    if e == 3 {
        p.text(x0 + 160.0, y0 + 134.0, Align2::LEFT_CENTER, "Env 4 → Voice Volume", 10.0, YELLOW);
    }
    vtoggle(p, v, x0 + 140.0, y0 + 18.0, 56.0, 16.0, b + 4, "Sustain");
    p.text(x0 + 268.0, y0 + 48.0, Align2::LEFT_CENTER, "Mod", 10.0, BLUE);
    p.menu(x0 + 296.0, y0 + 40.0, 70.0, 16.0, vid(v, b + 6));
    p.text(x0 + 268.0, y0 + 74.0, Align2::LEFT_CENTER, "via Vel", 10.0, GREEN);
    p.hfader(x0 + 306.0, y0 + 66.0, 100.0, 16.0, vid(v, b + 7));
    // the LFOs
    let lx = x0 + 424.0;
    p.group(lx, y0, 274.0, 152.0, "LFO");
    for k in 0..2usize {
        let r = p.rect(lx + 8.0 + k as f32 * 30.0, y0 + 18.0, 28.0, 16.0);
        if p.button(r, &format!("{}", k + 1), l == k, ("lfosel", k)) {
            set_st(p, "lfo", k);
            set_st(p, "hi", 1 + k as i32);
        }
    }
    let lb = ub::LFO0 + l as u32 * 6;
    vtoggle(p, v, lx + 76.0, y0 + 18.0, 34.0, 16.0, lb, "On");
    p.seg(lx + 114.0, y0 + 18.0, 80.0, 16.0, vid(v, lb + 1), &["Free", "Sync"], false);
    let wave = vg(p, v, lb + 3);
    let cycles = vg(p, v, lb + 4).round();
    let d = p.display(lx + 8.0, y0 + 40.0, 150.0, 50.0);
    let shown_cycles = if cycles >= 100.0 { 2.0 } else { cycles.clamp(1.0, 2.0) };
    let pts: Vec<(f32, f32)> = (0..=160)
        .map(|i| {
            let x = i as f32 / 160.0;
            let cyc = x * shown_cycles;
            let live = cycles >= 100.0 || cyc < cycles;
            let sh = (cyc.floor() * 12.9898).sin().fract() * 2.0 - 1.0;
            (x, 0.5 + if live { dspu::lfo_shape(cyc.fract(), wave, sh) * 0.4 } else { 0.0 })
        })
        .collect();
    p.curve(d, &pts, BLUE);
    p.knob(lx + 190.0, y0 + 38.0, 34.0, vid(v, lb + 2), "Rate");
    p.knob(lx + 238.0, y0 + 38.0, 34.0, vid(v, lb + 4), "Cycles");
    p.knob(lx + 238.0, y0 + 96.0, 24.0, vid(v, lb + 5), "Ramp");
    p.text(lx + 8.0, y0 + 108.0, Align2::LEFT_CENTER, "Waveform", 10.0, p.skin.dim);
    p.hfader(lx + 66.0, y0 + 100.0, 150.0, 16.0, vid(v, lb + 3));
    let txt = format!("Rate {}   Cycles {}   Ramp {}", p.shown(vid(v, lb + 2), vg(p, v, lb + 2)), if cycles >= 100.0 { "∞".to_string() } else { format!("{cycles}") }, p.shown(vid(v, lb + 5), vg(p, v, lb + 5)));
    p.text(lx + 8.0, y0 + 134.0, Align2::LEFT_CENTER, &txt, 10.0, p.skin.text);
}

// ---------------------------------------------------------------------------------------------
// the step sequencer

const GX0: f32 = 74.0;
const CW: f32 = 28.5;

fn res_per_beat(res: usize) -> usize {
    [1, 2, 3, 4, 6, 8][res.min(5)]
}

fn put_cell(p: &mut Pane, pat: usize, v: usize, old: &Cell, new: &Cell) {
    for w in 0..64 {
        if old.word(w) != new.word(w) {
            p.put(ub::cell_id(pat, v, w), new.word(w));
        }
    }
}

fn sequencer(p: &mut Pane, s: &Snap) {
    let v = s.sel;
    let top = SEQ_TOP;
    let r = p.rect(8.0, top - 8.0, 1004.0, H - top);
    p.ui.painter().rect_filled(r, 8.0, Color32::from_gray(0x2a));
    let mut acts: Vec<String> = Vec::new();
    // ---- global and pattern parameters
    let mut x = 16.0;
    p.toggle(x, top, 36.0, 18.0, ub::G_SEQ_ON, "On");
    x += 42.0;
    p.seg(x, top, 70.0, 18.0, ub::G_EDIT, &["Voice", "Step"], false);
    x += 76.0;
    p.toggle(x, top, 62.0, 18.0, ub::G_RUN, "Transport");
    x += 68.0;
    // the drag area: leaves the pattern for the host to make a MIDI region
    let dr = p.rect(x, top, 22.0, 18.0);
    let dresp = p.ui.interact(dr, egui::Id::unique(("ub-pdrag", p.t)), Sense::drag()).on_hover_text("Drag to a track: the pattern as a MIDI region");
    p.ui.painter().rect_filled(dr, 3.0, if dresp.dragged() { BLUE } else { Color32::from_gray(0x44) });
    p.ui.painter().text(dr.center(), Align2::CENTER_CENTER, "⠿", look::regular(12.0), Color32::WHITE);
    if dresp.drag_started() {
        acts.push(format!("export:{}", s.pat));
    }
    x += 28.0;
    p.text(x, top + 9.0, Align2::LEFT_CENTER, "Pattern", 10.0, p.skin.dim);
    x += 42.0;
    p.menu(x, top, 46.0, 18.0, ub::G_PAT);
    let presp = p.ui.interact(p.rect(x, top, 46.0, 18.0), egui::Id::unique(("ub-pat-ctx", p.t)), Sense::click());
    presp.context_menu(|ui| {
        for (n, a) in [("Copy", "copyp"), ("Paste", "pastep"), ("Clear", "clearp")] {
            if ui.button(n).clicked() {
                acts.push(a.into());
                ui.close();
            }
        }
    });
    x += 52.0;
    p.text(x, top + 9.0, Align2::LEFT_CENTER, "Length", 10.0, p.skin.dim);
    x += 40.0;
    p.field(x, top, 40.0, 18.0, ub::pat_len_id(s.pat), 1.0);
    x += 48.0;
    p.text(x, top + 9.0, Align2::LEFT_CENTER, "Res", 10.0, p.skin.dim);
    x += 24.0;
    p.menu(x, top, 56.0, 18.0, ub::pat_res_id(s.pat));
    x += 64.0;
    let acc_on = p.on(vid(v, ub::ACC_EN));
    if p.button(p.rect(x, top, 14.0, 18.0), "", acc_on, ("accled", v)) {
        vput(p, v, ub::ACC_EN, if acc_on { 0.0 } else { 1.0 });
    }
    p.text(x + 18.0, top + 9.0, Align2::LEFT_CENTER, "Accent", 10.0, p.skin.dim);
    p.hfader(x + 56.0, top, 64.0, 18.0, ub::G_ACCENT);
    x += 126.0;
    vtoggle(p, v, x, top, 44.0, 18.0, ub::SWING_EN, "Swing");
    p.hfader(x + 48.0, top, 70.0, 18.0, ub::G_SWING);
    x += 124.0;
    p.toggle(x, top, 82.0, 18.0, ub::G_PMODE, "Pattern Mode");
    p.menu(x + 86.0, top, 96.0, 18.0, ub::G_PBMODE);
    x += 190.0;
    p.toggle(x, top, 76.0, 18.0, ub::G_MUTEMODE, "Voice Mute");
    p.toggle(x + 80.0, top, 62.0, 18.0, ub::G_FULL, "Full View");
    if let Some(pp) = s.pm {
        p.text(x + 150.0, top + 9.0, Align2::LEFT_CENTER, &format!("▶ {}", pp + 1), 10.5, YELLOW);
    }
    // the MIDI controllers: assignment menus (also under Extended Parameters) and Learn
    let ry = top + 24.0;
    p.text(16.0, ry + 9.0, Align2::LEFT_CENTER, "MIDI Ctrl", 10.0, p.skin.dim);
    for i in 0..4usize {
        let cx = 70.0 + i as f32 * 128.0;
        p.text(cx, ry + 9.0, Align2::LEFT_CENTER, ["A", "B", "C", "D"][i], 10.0, GREEN);
        p.menu(cx + 12.0, ry, 64.0, 18.0, ub::G_CTRL + i as u32);
        let learning = s.learning == Some(i);
        if p.button(p.rect(cx + 80.0, ry, 38.0, 18.0), if learning { "…" } else { "Learn" }, learning, ("learn", i)) {
            acts.push(if learning { "unlearn".into() } else { format!("learn:{i}") });
        }
    }
    // ---- the step grid
    let ty = top + 52.0;
    let (len, res) = (s.len, s.res);
    let orig = s.cells[v].clone();
    let mut work = orig.clone();
    let step_mode = p.geti(ub::G_EDIT) == 1 && !p.ui.input(|i| i.modifiers.command && i.modifiers.alt);
    for i in 0..32usize {
        let cx = GX0 + i as f32 * CW;
        let live = i < len;
        let tint = if (i / res_per_beat(res).max(1)) % 2 == 0 { 0x44 } else { 0x38 };
        let r = p.rect(cx + 1.0, ty + 16.0, CW - 2.0, 24.0);
        p.ui.painter().rect_filled(r, 3.0, if live { Color32::from_gray(tint) } else { Color32::from_gray(0x2e) });
        p.text(cx + CW / 2.0, ty + 6.0, Align2::CENTER_CENTER, &format!("{}", i + 1), 8.5, if live { p.skin.dim } else { Color32::from_gray(0x50) });
        if s.step == Some(i) {
            p.ui.painter().rect_stroke(r.expand(1.0), 3.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
        }
    }
    p.text(16.0, ty + 28.0, Align2::LEFT_CENTER, "Trigger", 10.0, p.skin.dim);
    // the accent LEDs above the triggers
    for i in 0..len.min(32) {
        let cx = GX0 + i as f32 * CW;
        let r = p.rect(cx + CW / 2.0 - 4.0, ty + 42.0, 8.0, 6.0);
        let resp = p.ui.interact(r.expand(3.0), egui::Id::unique(("ub-acc", p.t, i)), Sense::click());
        p.ui.painter().rect_filled(r, 2.0, if work.accent(i) { BLUE } else { Color32::from_gray(0x24) });
        if resp.clicked() {
            work.set_accent(i, !work.accent(i));
        }
    }
    // triggers: a click toggles, dragging across paints them on or off
    let paint_id = egui::Id::unique(("ub-paint", p.t));
    let down = p.ui.input(|i| i.pointer.primary_down());
    let mut paint: Option<bool> = if down { p.ui.ctx().data(|d| d.get_temp(paint_id)) } else { None };
    let ptr = p.ui.input(|i| i.pointer.interact_pos());
    let seed = p.ui.input(|i| (i.time * 1000.0) as u32);
    for i in 0..len.min(32) {
        let cx = GX0 + i as f32 * CW;
        let r = p.rect(cx + 2.0, ty + 18.0, CW - 4.0, 20.0);
        let resp = p.ui.interact(r, egui::Id::unique(("ub-trig", p.t, i)), Sense::click_and_drag());
        if resp.is_pointer_button_down_on() && paint.is_none() && down {
            let pv = !work.trig(i);
            paint = Some(pv);
            p.ui.ctx().data_mut(|d| d.insert_temp(paint_id, pv));
        }
        if let (Some(pv), Some(pp)) = (paint, ptr) {
            if r.min.y - 4.0 <= pp.y && pp.y <= r.max.y + 4.0 && pp.x >= r.min.x - 1.0 && pp.x <= r.max.x + 1.0 && work.trig(i) != pv {
                work.set_trig(i, pv);
            }
        }
        p.ui.painter().rect_filled(r, 3.0, if work.trig(i) { BLUE } else { Color32::from_gray(0x22) });
        let mut menu_acts: Vec<u8> = Vec::new();
        resp.context_menu(|ui| trigger_menu(ui, &mut menu_acts));
        for a in menu_acts {
            apply_trigger_menu(&mut work, a, len, res, seed, p);
        }
    }
    if !down {
        p.ui.ctx().data_mut(|d| d.remove::<bool>(paint_id));
    }
    // ---- velocity and gate, or in Step mode the parameter offsets
    let vy = ty + 56.0;
    let vh = 74.0;
    if !step_mode {
        p.text(16.0, vy + 12.0, Align2::LEFT_CENTER, "Vel / Gate", 10.0, p.skin.dim);
        if p.button(p.rect(16.0, vy + 24.0, 46.0, 16.0), "Reset", false, "velreset") {
            work.reset_vel_gate();
        }
        let mut vg_acts: Vec<u8> = Vec::new();
        for i in 0..len.min(32) {
            let cx = GX0 + i as f32 * CW;
            let col = p.rect(cx + 1.0, vy, CW - 2.0, vh);
            p.ui.painter().rect_filled(col, 2.0, Color32::from_gray(0x20));
            for q in 1..4 {
                let qx = col.min.x + (CW - 2.0) * q as f32 / 4.0;
                p.ui.painter().line_segment([pos2(qx, col.min.y), pos2(qx, col.max.y)], Stroke::new(0.5, Color32::from_gray(0x30)));
            }
            let resp = p.ui.interact(col, egui::Id::unique(("ub-vel", p.t, i)), Sense::click_and_drag());
            if resp.dragged() || resp.clicked() {
                if let Some(pp) = resp.interact_pointer_pos() {
                    work.set_vel(i, ((col.max.y - pp.y) / vh).clamp(0.02, 1.0));
                    let sec = (((pp.x - col.min.x) / (col.width() / 4.0)).ceil() as i32).clamp(1, 4);
                    work.set_gate(i, ((1u16 << sec) - 1) as u8);
                }
            }
            resp.context_menu(|ui| {
                for (n, a) in [("Alter Vel", 0u8), ("Alter Gate", 1), ("Randomize Vel", 2), ("Randomize Gate", 3)] {
                    if ui.button(n).clicked() {
                        vg_acts.push(a);
                        ui.close();
                    }
                }
            });
            let bar = Rect::from_min_max(pos2(col.min.x, col.max.y - work.vel(i) * vh), pos2(col.min.x + (CW - 2.0) * work.gate_len(i), col.max.y));
            p.ui.painter().rect_filled(bar, 1.0, if work.trig(i) { BLUE } else { BLUE.gamma_multiply(0.4) });
        }
        let mut rng = lg_synth::dsp::Rng::new(seed | 1);
        for a in vg_acts {
            match a {
                0 => work.alter_vel(len, &mut rng),
                1 => work.alter_gate(len, &mut rng),
                2 => work.randomize_vel(len, &mut rng),
                _ => work.randomize_gate(len, &mut rng),
            }
        }
    } else {
        offset_row(p, v, &mut work, vy, vh, len);
    }
    if work != orig {
        put_cell(p, s.pat, v, &orig, &work);
    }
    p.actions.extend(acts);
}

fn trigger_menu(ui: &mut egui::Ui, out: &mut Vec<u8>) {
    let mut item = |ui: &mut egui::Ui, n: &str, a: u8| {
        if ui.button(n).clicked() {
            out.push(a);
            ui.close();
        }
    };
    item(ui, "Copy", 0);
    item(ui, "Paste", 1);
    item(ui, "Clear", 2);
    ui.separator();
    item(ui, "Add Every Downbeat", 3);
    item(ui, "Add Every Upbeat", 4);
    item(ui, "Alter Existing Randomly", 5);
    item(ui, "Reverse Existing", 6);
    ui.separator();
    item(ui, "Shift Left 1 Step", 7);
    item(ui, "Shift Right 1 Step", 8);
    item(ui, "Shift Left 1/2 Beat", 9);
    item(ui, "Shift Right 1/2 Beat", 10);
    item(ui, "Shift Left 1 Beat", 11);
    item(ui, "Shift Right 1 Beat", 12);
    ui.separator();
    ui.menu_button("Create & Replace Randomly", |ui| {
        item(ui, "Few", 13);
        item(ui, "Some", 14);
        item(ui, "Many", 15);
    });
}

fn apply_trigger_menu(c: &mut Cell, a: u8, len: usize, res: usize, seed: u32, p: &Pane) {
    let per = res_per_beat(res);
    let half = (per / 2).max(1) as i32;
    let mut rng = lg_synth::dsp::Rng::new(seed | 1);
    let cid = egui::Id::unique(("ub-trigclip", p.t));
    match a {
        0 => {
            let tr: Vec<bool> = (0..32).map(|i| c.trig(i)).collect();
            p.ui.ctx().data_mut(|d| d.insert_temp(cid, tr));
        }
        1 => {
            if let Some(tr) = p.ui.ctx().data(|d| d.get_temp::<Vec<bool>>(cid)) {
                for (i, on) in tr.iter().enumerate() {
                    c.set_trig(i, *on);
                }
            }
        }
        2 => c.clear_triggers(),
        3 => c.add_every(len, per, 0),
        4 => c.add_every(len, per, half as usize),
        5 => c.alter_randomly(len, &mut rng),
        6 => c.reverse(len),
        7 => c.shift(len, -1),
        8 => c.shift(len, 1),
        9 => c.shift(len, -half),
        10 => c.shift(len, half),
        11 => c.shift(len, -(per as i32)),
        12 => c.shift(len, per as i32),
        13 => c.create_randomly(len, (len / 8).max(1), &mut rng),
        14 => c.create_randomly(len, (len / 4).max(1), &mut rng),
        _ => c.create_randomly(len, (len / 2).max(1), &mut rng),
    }
}

/// Step automation: the Parameter Offset lane menu, M / S / Reset (then Delete), and the bars
/// of the offsets (positive above the centre line, negative below). Moving a Synthesizer
/// control in Step mode records an offset for the step last clicked in this row.
fn offset_row(p: &mut Pane, v: usize, c: &mut Cell, vy: f32, vh: f32, len: usize) {
    p.text(16.0, vy + 8.0, Align2::LEFT_CENTER, "Offset", 10.0, YELLOW);
    let sel = (st(p, "lane", 0i32).clamp(0, 3)) as usize;
    let mut lane = sel;
    let pr = p.rect(16.0, vy + 18.0, 54.0, 16.0);
    let resp = p.ui.interact(pr, egui::Id::unique(("ub-lane-menu", p.t)), Sense::click());
    p.ui.painter().rect_filled(pr, 4.0, Color32::from_gray(0x22));
    p.ui.painter().text(pr.center(), Align2::CENTER_CENTER, format!("Lane {}", sel + 1), look::regular(10.0), p.skin.text);
    egui::Popup::menu(&resp).show(|ui| {
        for l in 0..4usize {
            let name = if c.lane[l] == 0 { "(empty)".to_string() } else { ub::vspec(c.lane[l] as u32 - 1).map(|s| s.name.to_string()).unwrap_or_default() };
            if ui.add(egui::Button::selectable(l == sel, format!("{}  {name}", l + 1))).clicked() {
                lane = l;
                ui.close();
            }
        }
    });
    if lane != sel {
        set_st(p, "lane", lane as i32);
    }
    // a Synthesizer control moved in Step mode: the change becomes an offset
    let step: i32 = st(p, "last-step", 0i32);
    let step = (step.max(0) as usize).min(len.max(1) - 1);
    let moved: Vec<(u32, f32)> = p.set.iter().copied().filter(|(id, _)| *id >= ub::VBASE && *id < ub::VBASE + 25 * ub::VSTRIDE && (*id - ub::VBASE) / ub::VSTRIDE == v as u32 && ub::automatable((*id - ub::VBASE) % ub::VSTRIDE)).collect();
    if !moved.is_empty() {
        p.set.retain(|(id, _)| !(*id >= ub::VBASE && *id < ub::VBASE + 25 * ub::VSTRIDE && (*id - ub::VBASE) / ub::VSTRIDE == v as u32 && ub::automatable((*id - ub::VBASE) % ub::VSTRIDE)));
        for (id, x) in moved.iter().rev().fold(Vec::<(u32, f32)>::new(), |mut a, m| {
            if !a.iter().any(|q| q.0 == m.0) {
                a.push(*m);
            }
            a
        }) {
            let k = (id - ub::VBASE) % ub::VSTRIDE;
            let base = p.idx(id).map(|i| p.vals[i]).unwrap_or(x);
            if let (Some(sp), Some(l)) = (ub::vspec(k), c.lane_for(k)) {
                let acc_id = ("off-acc", l, step);
                let acc: f32 = st(p, &format!("{acc_id:?}"), 0.0f32);
                let total = acc + (x - base) / (sp.max - sp.min) * 31.0;
                let whole = total.round();
                set_st(p, &format!("{acc_id:?}"), total - whole);
                c.offs[l][step] = (c.offs[l][step] as f32 + whole).clamp(-32.0, 31.0) as i8;
                set_st(p, "lane", l as i32);
                lane = l;
            }
        }
    }
    let name = if c.lane[lane] == 0 { "no parameter".to_string() } else { ub::vspec(c.lane[lane] as u32 - 1).map(|s| s.name.to_string()).unwrap_or_default() };
    p.text(16.0, vy + 46.0, Align2::LEFT_CENTER, &name, 8.5, p.skin.dim);
    let (mute, solo) = (c.flags & (1 << lane) != 0, c.flags & (1 << (lane + 4)) != 0);
    if p.button(p.rect(16.0, vy + 54.0, 16.0, 16.0), "M", mute, ("offm", lane)) {
        c.flags ^= 1 << lane;
    }
    if p.button(p.rect(34.0, vy + 54.0, 16.0, 16.0), "S", solo, ("offs", lane)) {
        c.flags ^= 1 << (lane + 4);
    }
    let armed: bool = st(p, "reset-armed", false);
    if p.button(p.rect(52.0, vy + 54.0, 22.0, 16.0), if armed { "Del" } else { "Rst" }, armed, ("offr", lane)) && c.lane[lane] != 0 {
        if armed {
            c.delete_lane(lane);
            set_st(p, "reset-armed", false);
        } else {
            c.zero_lane(lane);
            set_st(p, "reset-armed", true);
        }
    }
    let mut menu_acts: Vec<u8> = Vec::new();
    for i in 0..len.min(32) {
        let cx = GX0 + i as f32 * CW;
        let col = p.rect(cx + 1.0, vy, CW - 2.0, vh);
        p.ui.painter().rect_filled(col, 2.0, if i == step { Color32::from_gray(0x2c) } else { Color32::from_gray(0x20) });
        let mid = col.center().y;
        p.ui.painter().line_segment([pos2(col.min.x, mid), pos2(col.max.x, mid)], Stroke::new(0.5, Color32::from_gray(0x50)));
        let resp = p.ui.interact(col, egui::Id::unique(("ub-off", p.t, i)), Sense::click_and_drag());
        if resp.clicked() || resp.drag_started() {
            set_st(p, "last-step", i as i32);
        }
        if c.lane[lane] != 0 && (resp.dragged() || resp.clicked()) {
            if let Some(pp) = resp.interact_pointer_pos() {
                c.offs[lane][i] = (((mid - pp.y) / (vh / 2.0)) * 32.0).round().clamp(-32.0, 31.0) as i8;
            }
        }
        resp.context_menu(|ui| {
            for (n, a) in [("Alter", 0u8), ("Randomize", 1), ("Delete", 2)] {
                if ui.button(n).clicked() {
                    menu_acts.push(a);
                    ui.close();
                }
            }
        });
        let o = c.offs[lane][i];
        if o != 0 {
            let h = o as f32 / 32.0 * (vh / 2.0);
            let bar = Rect::from_min_max(pos2(col.min.x + 3.0, mid.min(mid - h)), pos2(col.max.x - 3.0, mid.max(mid - h)));
            p.ui.painter().rect_filled(bar, 1.0, YELLOW);
        }
    }
    let seed = p.ui.input(|i| (i.time * 1000.0) as u32);
    let mut rng = lg_synth::dsp::Rng::new(seed | 1);
    for a in menu_acts {
        match a {
            0 => c.alter_lane(lane, len, &mut rng),
            1 => c.randomize_lane(lane, len, &mut rng),
            _ => c.delete_lane(lane),
        }
    }
}

/// Full view: a trigger button for each step of each of the 25 sounds.
fn full_view(p: &mut Pane, s: &Snap) {
    let r = p.rect(306.0, 8.0, 706.0, 552.0);
    p.ui.painter().rect_filled(r, 8.0, Color32::from_gray(0x22));
    p.text(316.0, 20.0, Align2::LEFT_CENTER, "FULL VIEW", 10.5, p.skin.dim);
    let (cw, rh) = (19.0, 19.5);
    let mut all: Vec<Cell> = s.cells.clone();
    for row in 0..25usize {
        let vo = 24 - row;
        let y = 32.0 + row as f32 * rh;
        if vo == s.sel {
            let rr = p.rect(312.0, y, 690.0, rh - 1.0);
            p.ui.painter().rect_filled(rr, 3.0, Color32::from_gray(0x44));
        }
        p.text(316.0, y + 9.0, Align2::LEFT_CENTER, &s.names[vo], 9.5, p.skin.text);
        for i in 0..s.len.min(32) {
            let br = p.rect(392.0 + i as f32 * cw, y + 1.0, cw - 2.0, rh - 3.0);
            let resp = p.ui.interact(br, egui::Id::unique(("ub-full", p.t, vo, i)), Sense::click());
            let on = all[vo].trig(i);
            p.ui.painter().rect_filled(br, 2.0, if on { BLUE } else { Color32::from_gray(0x30) });
            if resp.clicked() {
                all[vo].set_trig(i, !on);
                p.put(ub::G_SEL, vo as f32);
            }
        }
    }
    for vo in 0..25 {
        if all[vo] != s.cells[vo] {
            put_cell(p, s.pat, vo, &s.cells[vo], &all[vo]);
        }
    }
}
