//! Auto Sampler's window and the sampling itself. The window: the Preset pop-up (the plug-in
//! header's), a keyboard showing the key range with the notes to be sampled in blue (drag the
//! range's edges; click keys to play the track's instrument), the display (level and clipping
//! while playing — clipping notes red — and while sampling each note's waveform with percent
//! done and time remaining, the note being sampled orange and its name and velocity at the
//! lower left), Input Gain, the settings, the Velocity editor (shown while the pointer is over
//! Velocity Layers or Velocity Response: one line per layer, drag to change; that makes the
//! response Custom), and Sample (a Save dialog for the name, then Start; Cancel stops and
//! nothing is made).
//!
//! Sampling renders the track's own channel (its instrument and the effects before Auto
//! Sampler) offline, note by note, layer by layer and round robin by round robin; each take is
//! trimmed (a completely silent end is cut), looped as Auto Loop says (or One Shot), written as
//! WAV files next to an SFZ instrument in the Sampler Instruments folder, and a new track plays
//! it in a Sampler.
//!
//! No picture of Logic's Auto Sampler is in the research: the layout follows the guide.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Align2, Color32, Rect, Sense, Stroke, pos2, vec2};
use lg_dsp::Param;
use lg_fx::autosampler as asx;

use crate::app::App;
use crate::fxviews::{Action, Live};
use crate::look;

type Set = Vec<(u32, f32)>;

/// What a sampling job is doing (shared with its thread).
#[derive(Default, Clone)]
pub struct Progress {
    pub running: bool,
    pub cancel: bool,
    pub note: u8,
    pub velocity: u8,
    pub done: usize,
    pub total: usize,
    pub started: f64,
    pub wave: Vec<f32>,
    pub clipped: Vec<u8>,
    pub result: Option<Result<PathBuf, String>>,
}

pub fn progress() -> &'static Arc<Mutex<Progress>> {
    static P: std::sync::OnceLock<Arc<Mutex<Progress>>> = std::sync::OnceLock::new();
    P.get_or_init(|| Arc::new(Mutex::new(Progress::default())))
}

/// The sampling settings, from the plug-in's parameters.
#[derive(Clone, Debug)]
pub struct Settings {
    pub notes: Vec<u8>,
    pub velocities: Vec<u8>,
    pub round_robin: usize,
    pub sustain: f32,
    pub auto_loop: usize,
    pub loop_start: f32,
    pub loop_end: f32,
    pub one_shot: bool,
    pub gain: f32,
}

impl Settings {
    pub fn from(params: &[Param], values: &[f32]) -> Settings {
        let g = |id: u32| params.iter().position(|p| p.id == id).map_or(0.0, |i| values[i]);
        let custom: Vec<f32> = (0..asx::MAX_LAYERS).map(|k| g(asx::CUSTOM + k as u32)).collect();
        Settings {
            notes: asx::sample_notes(g(asx::RANGE_START) as u8, g(asx::RANGE_END) as u8, g(asx::EVERY).round() as u8),
            velocities: asx::layer_velocities(g(asx::LAYERS).round() as usize, g(asx::RESPONSE).round() as usize, &custom),
            round_robin: g(asx::ROUND_ROBIN).round() as usize + 1,
            sustain: g(asx::SUSTAIN),
            auto_loop: g(asx::AUTO_LOOP).round() as usize,
            loop_start: g(asx::LOOP_START) / 100.0,
            loop_end: g(asx::LOOP_END) / 100.0,
            one_shot: g(asx::ONE_SHOT) >= 0.5,
            gain: lg_dsp::db_to_gain(g(asx::INPUT_GAIN)),
        }
    }
}

const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
pub fn note_name(n: u8) -> String {
    format!("{}{}", NAMES[n as usize % 12], n as i32 / 12 - 2)
}

/// One take: trimmed and looped. Returns (left, right, loop).
pub fn shape_take(mut l: Vec<f32>, mut r: Vec<f32>, s: &Settings, rate: f32) -> (Vec<f32>, Vec<f32>, Option<(usize, usize)>) {
    // a completely silent end is cut (with a short fade so nothing clicks)
    let thr = 1e-4;
    let last = l.iter().zip(&r).rposition(|(a, b)| a.abs() > thr || b.abs() > thr).map_or(0, |i| i + 1);
    let keep = (last + (0.01 * rate) as usize).min(l.len());
    l.truncate(keep);
    r.truncate(keep);
    if s.one_shot || s.auto_loop == 0 || l.len() < 2048 {
        return (l, r, None);
    }
    let n = l.len();
    let lo = (s.loop_start.min(s.loop_end) * n as f32) as usize;
    let hi = (s.loop_start.max(s.loop_end) * n as f32) as usize;
    let mono: Vec<f32> = l.iter().zip(&r).map(|(a, b)| 0.5 * (a + b)).collect();
    let xf = (0.02 * rate) as usize;
    match s.auto_loop {
        4 => {
            let (a, ls, le) = asx::penrose_loop(&l, lo.max(1), 11);
            let (b, _, _) = asx::penrose_loop(&r, lo.max(1), 23);
            let m = a.len().min(b.len());
            (a[..m].to_vec(), b[..m].to_vec(), Some((ls, le.min(m))))
        }
        _ => {
            let Some((ls, le)) = asx::find_loop(&mono, lo, hi) else { return (l, r, None) };
            match s.auto_loop {
                2 => {
                    asx::crossfade_loop(&mut l, ls, le, xf);
                    asx::crossfade_loop(&mut r, ls, le, xf);
                    (l, r, Some((ls, le)))
                }
                3 => {
                    asx::reverse_xfade_loop(&mut l, ls, le, xf);
                    asx::reverse_xfade_loop(&mut r, ls, le, xf);
                    (l, r, Some((ls, le)))
                }
                5 => {
                    let (a, s0, e0) = asx::bidirectional_loop(&l, ls, le, xf);
                    let (b, _, _) = asx::bidirectional_loop(&r, ls, le, xf);
                    (a, b, Some((s0, e0)))
                }
                _ => (l, r, Some((ls, le))),
            }
        }
    }
}

/// The SFZ text for the takes: (file, root, low key, high key, low vel, high vel, round robin
/// position, round robins, loop).
#[allow(clippy::type_complexity)]
pub fn sfz_text(takes: &[(String, u8, u8, u8, u8, u8, usize, usize, Option<(usize, usize)>)], one_shot: bool) -> String {
    let mut s = String::from("// made by Bramble Studio's Auto Sampler\n<control>\ndefault_path=./\n<global>\nampeg_release=0.3\n");
    for (file, root, lk, hk, lv, hv, pos, rr, lp) in takes {
        s += &format!("<region> sample={file} pitch_keycenter={root} lokey={lk} hikey={hk} lovel={lv} hivel={hv}");
        if *rr > 1 {
            s += &format!(" seq_length={rr} seq_position={pos}");
        }
        if one_shot {
            s += " loop_mode=one_shot";
        } else if let Some((a, b)) = lp {
            s += &format!(" loop_mode=loop_continuous loop_start={a} loop_end={}", b.saturating_sub(1));
        }
        s.push('\n');
    }
    s
}

/// Starts sampling the strip's track into an instrument at `path` (.sfz).
pub fn start(app: &mut App, s: usize, settings: Settings, path: PathBuf) {
    let (track, sf) = match app.engine.lock() {
        Ok(e) => (e.tracks.get(s).cloned(), e.soundfont.clone()),
        Err(_) => return,
    };
    let Some(mut track) = track else {
        app.tell("Auto Sampler samples the instrument on its own track");
        return;
    };
    // the channel up to Auto Sampler: its own effects after it are left out
    if let Some(k) = track.inserts.iter().position(|i| i.0 == "Auto Sampler") {
        track.inserts.truncate(k);
    }
    track.volume = 1.0;
    track.pan = 0.0;
    track.auto_volume.clear();
    track.auto_pan.clear();
    track.mute = false;
    track.solo = false;
    track.sends.clear();
    track.audio.clear();
    let prog = progress().clone();
    let total = settings.notes.len() * settings.velocities.len() * settings.round_robin;
    *prog.lock().unwrap() = Progress { running: true, total, started: now(), ..Default::default() };
    std::thread::spawn(move || {
        let res = run(&prog, track, sf, &settings, &path);
        let mut p = prog.lock().unwrap();
        p.running = false;
        p.result = Some(res);
    });
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn run(prog: &Arc<Mutex<Progress>>, track: crate::audio::PlayTrack, sf: Option<PathBuf>, s: &Settings, path: &Path) -> Result<PathBuf, String> {
    let rate = crate::audio::RATE as f32;
    let dir = path.parent().ok_or("no folder")?.to_path_buf();
    let stem = path.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_else(|| "Auto Sampled".into());
    let wav_dir = dir.join(&stem);
    std::fs::create_dir_all(&wav_dir).map_err(|e| e.to_string())?;
    let mut takes = Vec::new();
    // ticks at 60 bpm: PPQ per second
    let tps = lg_core::events::PPQ;
    let held = (s.sustain * tps as f32) as u32;
    for (ni, &note) in s.notes.iter().enumerate() {
        let lk = if ni == 0 { 0 } else { note };
        let hk = s.notes.get(ni + 1).map_or(127, |n| n - 1);
        for (vi, &vel) in s.velocities.iter().enumerate() {
            let lv = if vi == 0 { 1 } else { s.velocities[vi - 1] + 1 };
            for rr in 1..=s.round_robin {
                if prog.lock().unwrap().cancel {
                    let _ = std::fs::remove_dir_all(&wav_dir);
                    return Err("Sampling cancelled: no instrument made".into());
                }
                {
                    let mut p = prog.lock().unwrap();
                    p.note = note;
                    p.velocity = vel;
                    p.wave.clear();
                }
                let mut t = track.clone();
                t.notes = vec![(0, held.max(1), note, vel)];
                // the note held for Sustain, then its release (the bounce adds two seconds)
                let x = crate::audio::bounce(vec![t], 60.0, Vec::new(), sf.as_deref(), 0, held + tps * 2);
                let l: Vec<f32> = x.iter().step_by(2).map(|v| v * s.gain).collect();
                let r: Vec<f32> = x.iter().skip(1).step_by(2).map(|v| v * s.gain).collect();
                let clipped = l.iter().chain(&r).any(|v| v.abs() >= 1.0);
                let (l, r, lp) = shape_take(l, r, s, rate);
                let file = format!("{stem}_{}_v{vel}_rr{rr}.wav", note_name(note));
                lg_synth::wav::write_wav_24(&wav_dir.join(&file), crate::audio::RATE, &l, &r).map_err(|e| e.to_string())?;
                takes.push((format!("{stem}/{file}"), note, lk, hk, lv, vel, rr, s.round_robin, lp));
                let mut p = prog.lock().unwrap();
                p.done += 1;
                p.wave = l.iter().step_by(64).copied().collect();
                if clipped {
                    p.clipped.push(note);
                }
            }
        }
    }
    std::fs::write(path, sfz_text(&takes, s.one_shot)).map_err(|e| e.to_string())?;
    Ok(path.to_path_buf())
}

/// Each frame: a finished job makes its track.
pub fn poll(app: &mut App) {
    let res = {
        let mut p = progress().lock().unwrap();
        p.result.take()
    };
    match res {
        Some(Ok(path)) => {
            let name = path.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
            app.checkpoint("Auto Sampler");
            let Some(p) = app.project.as_mut() else { return };
            let mut tr = lg_core::Track::new(&name, lg_core::TrackKind::Instrument);
            tr.row = p.tracks.len() as u32 + 1;
            p.tracks.push(tr);
            app.programs.push(crate::app::program_for(&name, lg_core::TrackKind::Instrument));
            let t = app.project.as_ref().map_or(0, |p| p.tracks.len() - 1);
            app.instrument_choice.insert(t, crate::soundlib::sampler_choice("Sampler", &[(60, path.clone())]));
            app.selected_track = Some(t);
            app.after_edit();
            app.tell(format!("Auto Sampler made “{name}” ({})", path.display()));
        }
        Some(Err(e)) => app.tell(e),
        None => {}
    }
}

/// The window's action: Sample (asks for the name), Cancel, or a key played.
pub fn run_action(app: &mut App, s: usize, a: Action, params: &[Param], values: &[f32]) {
    match a {
        Action::AutoSample => {
            let dir = crate::samplerwin::instruments_dir();
            let _ = std::fs::create_dir_all(&dir);
            let path = if app.automated {
                std::env::var_os("LG_AUTOSAMPLE").map(PathBuf::from)
            } else {
                kn_ui::files::FileDialog::new().set_title("Save the sampler instrument").set_directory(&dir).set_file_name("Auto Sampled.sfz").add_filter("Sampler Instrument", &["sfz"]).save_file()
            };
            let Some(mut path) = path else { return };
            if path.extension().is_none_or(|e| e != "sfz") {
                path.set_extension("sfz");
            }
            start(app, s, Settings::from(params, values), path);
        }
        Action::AutoSampleCancel => progress().lock().unwrap().cancel = true,
        Action::PlayKey(n, v) => {
            if let Ok(mut e) = app.engine.lock() {
                e.live_note(s, n, v);
            }
        }
        _ => {}
    }
}

// ---- the window ------------------------------------------------------------------------------

const BG: Color32 = Color32::from_rgb(0x22, 0x24, 0x28);
const DISPLAY: Color32 = Color32::from_rgb(0x10, 0x12, 0x15);
const BLUE: Color32 = Color32::from_rgb(0x3a, 0x9c, 0xf6);
const ORANGE: Color32 = Color32::from_rgb(0xff, 0x95, 0x1e);
const TEXT: Color32 = Color32::from_rgb(0xd4, 0xd6, 0xda);
const DIM: Color32 = Color32::from_rgb(0x8a, 0x8e, 0x94);

fn get(params: &[Param], values: &[f32], set: &Set, id: u32) -> f32 {
    set.iter().rev().find(|x| x.0 == id).map_or_else(|| params.iter().position(|p| p.id == id).map_or(0.0, |i| values[i]), |x| x.1)
}

pub fn view(ui: &mut egui::Ui, params: &[Param], values: &[f32], live: &Live, set: &mut Set, act: &mut Option<Action>) {
    let g = |set: &Set, id| get(params, values, set, id);
    let prog = progress().lock().unwrap().clone();
    let w = ui.available_width();
    // the keyboard (C-1 … G9 shown from the range's octave span)
    let (kr, _) = ui.allocate_exact_size(vec2(w, 70.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(kr, 4.0, BG);
    let (lo_n, hi_n) = (21u8, 108u8);
    let whites: Vec<u8> = (lo_n..=hi_n).filter(|n| ![1, 3, 6, 8, 10].contains(&(n % 12))).collect();
    let ww = (kr.width() - 8.0) / whites.len() as f32;
    let x_of = |n: u8| -> f32 { kr.min.x + 4.0 + whites.iter().position(|x| *x >= n).unwrap_or(whites.len()) as f32 * ww };
    let s = Settings::from(params, &params.iter().map(|q| g(set, q.id)).collect::<Vec<_>>());
    let (rs, re) = (g(set, asx::RANGE_START) as u8, g(set, asx::RANGE_END) as u8);
    let keys = Rect::from_min_max(kr.min + vec2(4.0, 6.0), kr.max - vec2(4.0, 4.0));
    // the Sample Note Range rectangle
    let rr = Rect::from_min_max(pos2(x_of(rs.min(re)), keys.min.y - 3.0), pos2(x_of(rs.max(re)) + ww, keys.max.y + 1.0));
    p.rect_filled(rr, 3.0, BLUE.gamma_multiply(0.25));
    for (k, n) in whites.iter().enumerate() {
        let r = Rect::from_min_size(pos2(keys.min.x + ww * k as f32, keys.min.y), vec2(ww - 1.0, keys.height()));
        let sampled = s.notes.contains(n);
        let col = if prog.running && prog.note == *n { ORANGE } else if sampled { BLUE } else { Color32::from_gray(0xe8) };
        p.rect_filled(r, 2.0, col);
        if n % 12 == 0 {
            p.text(pos2(r.center().x, r.max.y - 7.0), Align2::CENTER_CENTER, note_name(*n), look::regular(7.5), Color32::from_gray(0x30));
        }
    }
    for n in lo_n..=hi_n {
        if [1, 3, 6, 8, 10].contains(&(n % 12)) {
            let x = x_of(n) - ww * 0.3;
            let r = Rect::from_min_size(pos2(x, keys.min.y), vec2(ww * 0.6, keys.height() * 0.6));
            let col = if prog.running && prog.note == n { ORANGE } else if s.notes.contains(&n) { Color32::from_rgb(0x1e, 0x5c, 0xb0) } else { Color32::from_gray(0x1a) };
            p.rect_filled(r, 1.5, col);
        }
    }
    // the range's edges drag; keys play
    for (k, (id, n)) in [(asx::RANGE_START, rs.min(re)), (asx::RANGE_END, rs.max(re))].into_iter().enumerate() {
        let ex = if k == 0 { rr.min.x } else { rr.max.x };
        let er = Rect::from_center_size(pos2(ex, rr.center().y), vec2(8.0, rr.height()));
        let resp = ui.interact(er, egui::Id::unique(("as-edge", k)), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if let Some(pt) = resp.interact_pointer_pos().filter(|_| resp.dragged()) {
            let wi = ((pt.x - keys.min.x) / ww).floor().clamp(0.0, whites.len() as f32 - 1.0) as usize;
            let _ = n;
            set.push((id, whites[wi] as f32));
        }
    }
    let kresp = ui.interact(keys.shrink2(vec2(4.0, 0.0)), egui::Id::unique("as-keys"), Sense::click_and_drag());
    let held_id = egui::Id::unique("as-held");
    let held: Option<u8> = ui.data(|d| d.get_temp(held_id)).flatten();
    if kresp.is_pointer_button_down_on() {
        if let Some(pt) = kresp.interact_pointer_pos() {
            let wi = ((pt.x - keys.min.x) / ww).floor().clamp(0.0, whites.len() as f32 - 1.0) as usize;
            let mut n = whites[wi];
            // a black key over the upper part
            if pt.y < keys.min.y + keys.height() * 0.6 {
                for b in [n.saturating_sub(1), n + 1] {
                    if [1, 3, 6, 8, 10].contains(&(b % 12)) && (pt.x - x_of(b.max(1))).abs() < ww * 0.3 && b != n {
                        n = b;
                    }
                }
            }
            if held != Some(n) {
                if let Some(h) = held {
                    *act = Some(Action::PlayKey(h, 0));
                }
                *act = Some(Action::PlayKey(n, 100));
                ui.data_mut(|d| d.insert_temp(held_id, Some(n)));
            }
        }
    } else if let Some(h) = held {
        *act = Some(Action::PlayKey(h, 0));
        ui.data_mut(|d| d.insert_temp(held_id, None::<u8>));
    }

    // the display: level and clipping, or the take being sampled
    ui.add_space(6.0);
    let (dr, _) = ui.allocate_exact_size(vec2(w, 110.0), Sense::hover());
    p.rect_filled(dr, 4.0, DISPLAY);
    if prog.running || !prog.wave.is_empty() && prog.done < prog.total {
        let n = prog.wave.len().max(1);
        let pk = prog.wave.iter().fold(1e-6f32, |m, v| m.max(v.abs()));
        let pts: Vec<egui::Pos2> = prog.wave.iter().enumerate().map(|(k, v)| pos2(dr.min.x + dr.width() * k as f32 / n as f32, dr.center().y - v / pk * dr.height() * 0.4)).collect();
        if pts.len() > 1 {
            p.add(egui::Shape::line(pts, Stroke::new(1.0, ORANGE)));
        }
        let pct = if prog.total > 0 { prog.done as f32 / prog.total as f32 } else { 0.0 };
        let el = now() - prog.started;
        let left = if pct > 0.0 { el / pct as f64 * (1.0 - pct as f64) } else { 0.0 };
        p.text(dr.right_top() + vec2(-8.0, 10.0), Align2::RIGHT_CENTER, format!("{:.0} %   {:.0} s left", pct * 100.0, left), look::regular(11.0), TEXT);
        p.text(dr.left_bottom() + vec2(8.0, -10.0), Align2::LEFT_CENTER, format!("{}  velocity {}", note_name(prog.note), prog.velocity), look::regular(11.0), ORANGE);
    } else {
        let lv = |n: &str| live.get(n).unwrap_or(-120.0);
        for (k, name) in ["Level L", "Level R"].iter().enumerate() {
            let v = lv(name);
            let f = ((v + 60.0) / 60.0).clamp(0.0, 1.0);
            let r = Rect::from_min_size(dr.min + vec2(10.0, 30.0 + 26.0 * k as f32), vec2((dr.width() - 20.0) * f, 16.0));
            p.rect_filled(r, 2.0, if live.get("Clip").unwrap_or(0.0) > 0.0 { Color32::from_rgb(0xff, 0x3b, 0x30) } else { BLUE });
        }
        if !prog.clipped.is_empty() {
            let t: Vec<String> = prog.clipped.iter().map(|n| note_name(*n)).collect();
            p.text(dr.left_bottom() + vec2(8.0, -10.0), Align2::LEFT_CENTER, format!("Clipped: {}", t.join(" ")), look::regular(11.0), Color32::from_rgb(0xff, 0x3b, 0x30));
        }
    }
    // the settings
    ui.add_space(6.0);
    let ids = [asx::INPUT_GAIN, asx::RANGE_START, asx::RANGE_END, asx::EVERY, asx::ROUND_ROBIN, asx::SUSTAIN, asx::LAYERS, asx::RESPONSE, asx::AUTO_LOOP, asx::LOOP_START, asx::LOOP_END, asx::ONE_SHOT];
    let mut pp = Vec::new();
    let mut vv = Vec::new();
    for id in ids {
        if let Some(q) = params.iter().find(|q| q.id == id) {
            pp.push(q.clone());
            vv.push(g(set, id));
        }
    }
    let texts: Vec<Option<String>> = pp.iter().zip(&vv).map(|(q, v)| match q.id {
        asx::RANGE_START | asx::RANGE_END => Some(note_name(*v as u8)),
        asx::LAYERS => Some(format!("{}", v.round() as u32)),
        _ => None,
    }).collect();
    let top = ui.cursor().min.y;
    set.extend(crate::pluginwin::control_rows(ui, &pp, &vv, &texts, "auto-sampler"));
    // hovering the velocity rows shows the Velocity editor
    let rows_rect = Rect::from_min_max(pos2(ui.min_rect().min.x, top), pos2(ui.min_rect().max.x, ui.cursor().min.y));
    let vel_row = |k: usize| rows_rect.min.y + rows_rect.height() * k as f32 / ids.len() as f32;
    let over = ui.ctx().pointer_hover_pos().is_some_and(|q| q.y >= vel_row(6) && q.y < vel_row(8) && rows_rect.x_range().contains(q.x));
    let ed_id = egui::Id::unique("as-veled");
    let mut show: f64 = ui.data(|d| d.get_temp(ed_id)).unwrap_or(0.0);
    let t = ui.input(|i| i.time);
    if over {
        show = t + 1.5;
    }
    ui.data_mut(|d| d.insert_temp(ed_id, show));
    if show > t {
        let (er, _) = ui.allocate_exact_size(vec2(w, 120.0), Sense::hover());
        p.rect_filled(er, 4.0, DISPLAY);
        p.text(er.min + vec2(8.0, 10.0), Align2::LEFT_CENTER, "Velocity", look::regular(11.0), DIM);
        let n = s.velocities.len();
        let y_of = |v: f32| er.max.y - 8.0 - (er.height() - 24.0) * v / 127.0;
        for (k, v) in s.velocities.iter().enumerate() {
            let y = y_of(*v as f32);
            let line = Rect::from_min_max(pos2(er.min.x + 60.0, y - 4.0), pos2(er.max.x - 50.0, y + 4.0));
            p.line_segment([pos2(line.min.x, y), pos2(line.max.x, y)], Stroke::new(2.0, BLUE));
            p.text(pos2(line.max.x + 6.0, y), Align2::LEFT_CENTER, format!("{v}"), look::regular(10.0), TEXT);
            let resp = ui.interact(line, egui::Id::unique(("as-vl", k)), Sense::drag());
            if resp.dragged() {
                let nv = (*v as f32 - resp.drag_delta().y / (er.height() - 24.0) * 127.0).clamp(1.0, 127.0);
                // the layers become Custom, keeping the others where they are
                for (j, o) in s.velocities.iter().enumerate() {
                    set.push((asx::CUSTOM + j as u32, if j == k { nv } else { *o as f32 }));
                }
                set.push((asx::RESPONSE, 7.0));
            }
            let _ = n;
        }
    }
    // Sample / Cancel
    ui.add_space(6.0);
    let (br, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
    let b = Rect::from_min_size(pos2(br.max.x - 100.0, br.min.y + 2.0), vec2(96.0, 26.0));
    let resp = ui.interact(b, egui::Id::unique("as-sample"), Sense::click());
    p.rect_filled(b, 5.0, if prog.running { Color32::from_gray(0x48) } else { look::accent() });
    p.text(b.center(), Align2::CENTER_CENTER, if prog.running { "Cancel" } else { "Sample" }, look::regular(12.5), Color32::WHITE);
    if resp.clicked() {
        *act = Some(if prog.running { Action::AutoSampleCancel } else { Action::AutoSample });
    }
    let _ = g(set, asx::SUSTAIN);
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_and_sfz() {
        let s = Settings { notes: vec![60], velocities: vec![127], round_robin: 1, sustain: 1.0, auto_loop: 2, loop_start: 0.3, loop_end: 0.9, one_shot: false, gain: 1.0 };
        let rate = 48000.0;
        let mut l: Vec<f32> = (0..48000).map(|i| 0.5 * (std::f32::consts::TAU * 220.0 * i as f32 / rate).sin()).collect();
        l.extend(vec![0.0; 48000]);
        let r = l.clone();
        let (a, _, lp) = shape_take(l, r, &s, rate);
        // the silent second is cut, a loop is found inside the sound
        assert!(a.len() < 49000);
        let (ls, le) = lp.unwrap();
        assert!(ls < le && le <= a.len());
        let text = sfz_text(&[("x/a.wav".into(), 60, 0, 127, 1, 127, 1, 2, Some((ls, le)))], false);
        assert!(text.contains("seq_length=2") && text.contains("loop_mode=loop_continuous"));
        let zones = lg_synth::sampler::parse_sfz(&text, Path::new("/nonexistent")).unwrap_or_default();
        let _ = zones;
        assert_eq!(note_name(60), "C3");
    }

    /// Sampling a synth: one WAV per note / layer / round robin, an SFZ the Sampler reads.
    #[test]
    fn samples_an_instrument() {
        let t = crate::audio::PlayTrack {
            channel: 0,
            program: 0,
            notes: Vec::new(),
            audio: Vec::new(),
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            eq: None,
            instrument: Some("ES1".into()),
            instrument_floats: Vec::new(),
            inserts: vec![("Auto Sampler".into(), Vec::new())],
            sends: Vec::new(),
            auto_volume: Vec::new(),
            auto_pan: Vec::new(),
        };
        let s = Settings { notes: vec![48, 60], velocities: vec![64, 127], round_robin: 1, sustain: 0.3, auto_loop: 1, loop_start: 0.2, loop_end: 0.9, one_shot: false, gain: 1.0 };
        let dir = std::env::temp_dir().join(format!("lg-autosampler-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("Test Synth.sfz");
        let prog = Arc::new(Mutex::new(Progress::default()));
        let out = run(&prog, t, None, &s, &path).unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert_eq!(text.matches("<region>").count(), 4);
        let zones = lg_synth::sampler::parse_sfz(&text, &dir).unwrap();
        assert_eq!(zones.len(), 4);
        assert!(zones.iter().any(|z| z.lovel == 65 && z.hivel == 127));
        assert_eq!(prog.lock().unwrap().done, 4);
        // the takes have sound
        assert!(zones.iter().all(|z| z.end > 1000));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
