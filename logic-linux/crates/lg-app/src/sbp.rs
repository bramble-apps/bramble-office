//! Selection-Based Processing, as Logic's: effect plug-ins of its own (two sets, A and B, one
//! on at a time) rendered into the selected audio regions or the marquee selection, in the
//! Tracks area and the Audio Track Editor (Functions ▸ Selection-Based Processing…, ⌥⇧P; the
//! region shortcut menu's Processing). Split at Marquee Borders, Create New Take, Add Effect
//! Tail, the Gain choices (No Change, Loudness Compensation, Overload Protection, Normalize),
//! Preview with its volume, the Action menu's Preview enables Solo / Cycle, Apply, and Apply
//! Selection-Based Processing Again.

use egui::vec2;

use crate::app::App;
use crate::freeze::{self, What};
use crate::look;

pub const COMMANDS: &[&str] = &["Selection-Based Processing…", "Apply Selection-Based Processing Again"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Gain {
    #[default]
    NoChange,
    LoudnessCompensation,
    OverloadProtection,
    Normalize,
}

impl Gain {
    pub const ALL: [Gain; 4] = [Gain::NoChange, Gain::LoudnessCompensation, Gain::OverloadProtection, Gain::Normalize];
    pub fn name(self) -> &'static str {
        match self {
            Gain::NoChange => "No Change",
            Gain::LoudnessCompensation => "Loudness Compensation",
            Gain::OverloadProtection => "Overload Protection",
            Gain::Normalize => "Normalize",
        }
    }

    /// The gain on the processed audio (interleaved stereo); `dry` is the unprocessed audio.
    pub fn apply(self, wet: &mut [f32], dry: &[f32]) {
        let peak = wet.iter().fold(0f32, |m, x| m.max(x.abs()));
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt();
        let g = match self {
            Gain::NoChange => 1.0,
            Gain::LoudnessCompensation => {
                let (a, b) = (rms(dry), rms(wet));
                if b > 1e-9 { a / b } else { 1.0 }
            }
            Gain::OverloadProtection => {
                if peak > 1.0 { 1.0 / peak } else { 1.0 }
            }
            Gain::Normalize => {
                if peak > 1e-9 { 0.999 / peak } else { 1.0 }
            }
        };
        if (g - 1.0).abs() > 1e-6 {
            for x in wet.iter_mut() {
                *x *= g;
            }
        }
    }
}

#[derive(Clone)]
pub struct State {
    pub open: bool,
    /// The two plug-in sets (Logic names, settings); `b_on`: Set B is the active one.
    pub sets: [Vec<(String, Vec<f32>)>; 2],
    pub b_on: bool,
    pub split: bool,
    pub new_take: bool,
    pub tail: bool,
    pub gain: Gain,
    pub preview_volume: f32,
    pub preview_solo: bool,
    pub preview_cycle: bool,
    /// The last processing applied (Apply Selection-Based Processing Again).
    pub last: Option<(Vec<(String, Vec<f32>)>, bool, bool, bool, Gain)>,
}

impl Default for State {
    fn default() -> Self {
        State { open: false, sets: [Vec::new(), Vec::new()], b_on: false, split: true, new_take: false, tail: false, gain: Gain::NoChange, preview_volume: 0.8, preview_solo: true, preview_cycle: false, last: None }
    }
}

/// What gets processed: (track, region, from, to) — the marquee's part of a region, or the
/// whole selected audio regions.
fn targets(app: &App) -> Vec<(usize, usize, u32, u32)> {
    let Some(p) = app.project.as_ref() else { return Vec::new() };
    let audio = |t: usize, r: usize| p.tracks.get(t).and_then(|tr| tr.regions.get(r)).filter(|x| x.audio.is_some());
    if let Some(m) = app.ta.regions.marquee.as_ref() {
        let (a, b) = (m.from.min(m.to), m.from.max(m.to));
        let mut v = Vec::new();
        for &t in &m.tracks {
            for (r, rg) in p.tracks.get(t).map(|x| x.regions.iter().enumerate().collect::<Vec<_>>()).unwrap_or_default() {
                if rg.audio.is_some() && rg.start < b && rg.start + rg.length > a {
                    v.push((t, r, a.max(rg.start), b.min(rg.start + rg.length)));
                }
            }
        }
        if !v.is_empty() {
            return v;
        }
    }
    app.region_selection().into_iter().filter_map(|(t, r)| audio(t, r).map(|x| (t, r, x.start, x.start + x.length))).collect()
}

/// Renders a region's audio from `a` to `b` through `fx` (or dry when empty).
fn render(app: &App, t: usize, start: u32, a: u32, b: u32, fx: &[(String, Vec<f32>)], tail: bool) -> Vec<f32> {
    let what = What { tracks: vec![t], only: vec![(t, start, start + 1)], from: a, to: b, bypass_fx: true, fader: false, tail_secs: if tail { 8.0 } else { 0.0 }, until_silent: true };
    let fx = fx.to_vec();
    freeze::job_with(app, &what, move |tracks| {
        if let Some(pt) = tracks.get_mut(t) {
            pt.notes.clear();
            pt.inserts = fx;
        }
    })
    .run()
}

/// Applies `fx` to the targets with the options; returns whether anything was processed.
pub fn apply(app: &mut App, fx: Vec<(String, Vec<f32>)>, split: bool, new_take: bool, tail: bool, gain: Gain) -> bool {
    let mut tg = targets(app);
    if tg.is_empty() {
        return false;
    }
    app.checkpoint("Selection-Based Processing");
    // bottom-most regions first: indexes above stay valid
    tg.sort_by(|x, y| (y.0, y.1).cmp(&(x.0, x.1)));
    for (t, r, a, b) in tg {
        let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { continue };
        let (rs, re) = (rg.start, rg.start + rg.length);
        let marquee = a > rs || b < re;
        // the processed part, and (marquee, no split) the dry rest around it
        let dry_part = render(app, t, rs, a, b, &[], false);
        let mut wet = render(app, t, rs, a, b, &fx, tail);
        gain.apply(&mut wet, &dry_part);
        let name = format!("{} SBP", rg.name);
        if marquee && !split {
            // one region: the dry audio with the processed part spliced in
            let mut all = render(app, t, rs, rs, re, &[], false);
            let at = 2 * ((app.seconds_at(a as f64) - app.seconds_at(rs as f64)) * crate::audio::RATE as f64).round() as usize;
            for (k, x) in wet.iter().enumerate() {
                if let Some(s) = all.get_mut(at + k) {
                    *s = *x;
                } else {
                    all.push(*x);
                }
            }
            let Some((path, secs)) = freeze::write_bounce(app, &name, &all) else { return true };
            replace(app, t, r, freeze::audio_region(&name, rs, rg.length, path, secs), new_take);
            continue;
        }
        let Some((path, secs)) = freeze::write_bounce(app, &name, &wet) else { return true };
        let len = if tail { freeze::ticks_of(app, a, wet.len() / 2).max(b - a) } else { b - a };
        let processed = freeze::audio_region(&name, a, len, path, secs);
        if marquee {
            // Split at Marquee Borders: the region cut at the marquee, the middle processed
            app.selected_regions = vec![(t, r)];
            app.selected_region = Some((t, r));
            let cuts: Vec<u32> = [a, b].into_iter().filter(|c| *c > rs && *c < re).collect();
            let before = app.project.as_ref().map_or(0, |p| p.tracks[t].regions.len());
            app.split_at(&cuts);
            // the piece now covering [a, b)
            let idx = app.project.as_ref().and_then(|p| p.tracks[t].regions.iter().position(|x| x.start == a && x.audio.is_some()));
            let _ = before;
            if let Some(i) = idx {
                replace(app, t, i, processed, new_take);
            }
        } else {
            replace(app, t, r, processed, new_take);
        }
    }
    app.after_edit();
    app.load_engine();
    true
}

/// Puts the processed region in place of region `r` (or, Create New Take, in a take folder
/// with the original).
fn replace(app: &mut App, t: usize, r: usize, new: lg_core::Region, new_take: bool) {
    if new_take {
        crate::takes::make_folder(app, t, vec![r], vec![new]);
    } else if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        *x = new;
    }
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Selection-Based Processing…" => {
            app.sbp.open = true;
            true
        }
        "Apply Selection-Based Processing Again" => {
            let Some((fx, split, take, tail, gain)) = app.sbp.last.clone() else { return false };
            apply(app, fx, split, take, tail, gain)
        }
        _ => false,
    }
}

/// The Selection-Based Processing window.
pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.sbp.open {
        return;
    }
    let mut open = true;
    let mut s = app.sbp.clone();
    let mut do_apply = false;
    let mut do_preview = false;
    let fx_names: Vec<&'static str> = lg_fx::list().into_iter().map(|x| x.0).collect();
    egui::Window::new("Selection-Based Processing").open(&mut open).default_size(vec2(520.0, 360.0)).resizable(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            // the two sets: turning one on turns the other off
            for (i, name) in ["Set A", "Set B"].into_iter().enumerate() {
                ui.vertical(|ui| {
                    ui.set_width(240.0);
                    let mut on = s.b_on == (i == 1);
                    if ui.toggle_value(&mut on, egui::RichText::new(format!("{name}: {}", if s.b_on == (i == 1) { "On" } else { "Off" })).strong()).clicked() {
                        s.b_on = i == 1;
                    }
                    let mut remove = None;
                    for (k, (n, _)) in s.sets[i].iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(n);
                            if ui.small_button("×").on_hover_text("Remove").clicked() {
                                remove = Some(k);
                            }
                        });
                    }
                    if let Some(k) = remove {
                        s.sets[i].remove(k);
                    }
                    // an empty slot: choose a plug-in
                    egui::ComboBox::from_id_salt(("sbp-slot", i)).selected_text("Add Plug-in…").width(200.0).show_ui(ui, |ui| {
                        for n in &fx_names {
                            if ui.selectable_label(false, *n).clicked() {
                                s.sets[i].push((n.to_string(), Vec::new()));
                            }
                        }
                    });
                });
            }
        });
        ui.separator();
        ui.checkbox(&mut s.split, "Split at Marquee Borders");
        ui.checkbox(&mut s.new_take, "Create New Take");
        ui.checkbox(&mut s.tail, "Add Effect Tail");
        egui::ComboBox::from_label("Gain").selected_text(s.gain.name()).show_ui(ui, |ui| {
            for g in Gain::ALL {
                ui.selectable_value(&mut s.gain, g, g.name());
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.menu_button("Action ⌄", |ui| {
                ui.checkbox(&mut s.preview_solo, "Preview enables Solo");
                ui.checkbox(&mut s.preview_cycle, "Preview enables Cycle");
            });
            if ui.button("Preview").clicked() {
                do_preview = true;
            }
            ui.add(egui::Slider::new(&mut s.preview_volume, 0.0..=1.0).show_value(false).text("Preview"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Apply").clicked() {
                    do_apply = true;
                }
            });
        });
        if targets(app).is_empty() {
            ui.label(egui::RichText::new("Select audio regions or make a marquee selection.").small().color(look::text_dim()));
        }
    });
    s.open = open;
    let fx = s.sets[s.b_on as usize].clone();
    app.sbp = s;
    if do_preview {
        // the processed selection, played on its own at the preview volume
        if let Some(&(t, r, a, b)) = targets(app).first() {
            let start = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map_or(a, |x| x.start);
            let mut wet = render(app, t, start, a, b, &fx, app.sbp.tail);
            let dry = render(app, t, start, a, b, &[], false);
            app.sbp.gain.apply(&mut wet, &dry);
            for x in wet.iter_mut() {
                *x *= app.sbp.preview_volume;
            }
            if app.sbp.preview_cycle {
                app.set_cycle((a, b));
            }
            let path = std::env::temp_dir().join("bramble-studio-sbp-preview.wav");
            let f = lg_audio::AudioFile::new(crate::audio::RATE, 2, wet);
            if f.write(&path).is_ok() {
                app.preview_file(&path);
            }
        }
    }
    if do_apply {
        let (split, take, tail, gain) = (app.sbp.split, app.sbp.new_take, app.sbp.tail, app.sbp.gain);
        if apply(app, fx.clone(), split, take, tail, gain) {
            app.sbp.last = Some((fx, split, take, tail, gain));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gains() {
        let dry = vec![0.5f32; 100];
        let mut w = vec![2.0f32; 100];
        Gain::OverloadProtection.apply(&mut w, &dry);
        assert!((w[0] - 1.0).abs() < 1e-5);
        let mut w = vec![0.2f32; 100];
        Gain::OverloadProtection.apply(&mut w, &dry);
        assert_eq!(w[0], 0.2, "only above 0 dB");
        let mut w = vec![0.25f32; 100];
        Gain::LoudnessCompensation.apply(&mut w, &dry);
        assert!((w[0] - 0.5).abs() < 1e-5);
        let mut w = vec![0.1f32, -0.4];
        Gain::Normalize.apply(&mut w, &dry);
        assert!((w[1] + 0.999).abs() < 1e-4);
        let mut w = vec![3.0f32];
        Gain::NoChange.apply(&mut w, &dry);
        assert_eq!(w[0], 3.0);
    }

    #[test]
    fn processes_regions_and_marquee() {
        let mut app = crate::tm::test_app();
        let p = app.project.as_ref().unwrap();
        let t = p.tracks.iter().position(|t| t.kind != lg_core::TrackKind::Audio && t.regions.iter().any(|r| !r.notes.is_empty())).unwrap();
        let dir = std::env::temp_dir().join(format!("lg-sbp-{}", std::process::id()));
        app.project.as_mut().unwrap().audio_dir = Some(dir.clone());
        let o = crate::freeze::BipOpts { dest: crate::freeze::Dest::Other, ..Default::default() };
        let at = crate::freeze::bounce_track(&mut app, t, &o, "Src").unwrap();
        app.after_edit();
        app.load_engine();
        // the whole region, normalized
        app.selected_regions = vec![(at, 0)];
        app.selected_region = Some((at, 0));
        let before = app.project.as_ref().unwrap().tracks[at].regions[0].audio.clone().unwrap().file;
        assert!(apply(&mut app, vec![("Compressor".into(), Vec::new())], true, false, false, Gain::Normalize));
        let rg = app.project.as_ref().unwrap().tracks[at].regions[0].clone();
        let file = rg.audio.as_ref().unwrap().file.clone();
        assert_ne!(file, before);
        let f = lg_audio::AudioFile::read(&file).unwrap();
        let peak = f.data.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!((peak - 0.999).abs() < 0.01, "normalized: {peak}");
        // a marquee: split at its borders, the middle processed (three regions)
        let (s0, len) = (rg.start, rg.length);
        app.ta.regions.marquee = Some(crate::regionops::Marquee { from: s0 + len / 4, to: s0 + len / 2, tracks: vec![at] });
        app.selected_regions.clear();
        assert!(apply(&mut app, Vec::new(), true, false, false, Gain::NoChange));
        let n = app.project.as_ref().unwrap().tracks[at].regions.len();
        assert_eq!(n, 3, "split at the marquee borders");
        let _ = std::fs::remove_dir_all(dir);
    }
}
