//! Normalize Cell Gain… (Functions ▸ Normalize Region/Cell Gain…, ⌃⌥G, with audio cells selected
//! in the Live Loops grid): the Normalize Gain dialog sets the selected audio cells' gain so they
//! reach a Target Level — by Peak (dB) or Loudness (integrated LUFS, BS.1770) — processing all
//! cells equally, equally per track, or each individually. One undo step.

use std::collections::BTreeMap;

use egui::{vec2, Align2};

use crate::app::App;
use crate::look;

/// (The command is the Tracks area's "Normalize Region/Cell Gain…", run here for cells.)
pub const COMMANDS: &[&str] = &[];

pub const PROCESSES: [&str; 3] = ["All Cells Equally", "Cells Equally per Track", "Individually"];
pub const METHODS: [&str; 2] = ["Peak", "Loudness"];

/// The dialog's settings (kept while the project is open).
#[derive(Clone, Debug)]
pub struct Dialog {
    pub open: bool,
    pub process: usize,
    pub method: usize,
    pub peak_db: f32,
    pub lufs: f32,
}

impl Default for Dialog {
    fn default() -> Self {
        Dialog { open: false, process: 2, method: 0, peak_db: 0.0, lufs: -16.0 }
    }
}

/// The selected audio cells (grid focused).
pub fn selection(app: &App) -> Vec<(usize, usize)> {
    let s = &app.live_loops;
    if s.view == crate::liveloops::View::TracksOnly || !s.focused {
        return Vec::new();
    }
    s.selected.iter().copied().filter(|k| s.cells.get(k).is_some_and(|c| c.audio.is_some())).collect()
}

/// Whether ⌃⌥G (and the Functions item) means the cells.
pub fn wants(app: &App) -> bool {
    !selection(app).is_empty()
}

pub fn open(app: &mut App) -> bool {
    if !wants(app) {
        return false;
    }
    app.live_loops.norm.open = true;
    true
}

/// The level of `samples` (interleaved stereo at RATE): peak dBFS, or integrated LUFS.
pub fn level(samples: &[f32], method: usize) -> f32 {
    if method == 0 {
        let p = samples.iter().fold(0f32, |m, v| m.max(v.abs()));
        return if p > 1e-9 { 20.0 * p.log10() } else { -120.0 };
    }
    let mut l = lg_fx::meters::Loudness::new(crate::audio::RATE as f32);
    for f in samples.chunks_exact(2) {
        l.push(f[0], f[1]);
    }
    l.integrated()
}

/// The gain (dB, to 0.1) each cell gets: `levels` of every cell and its track, grouped by
/// `process` (0 all equally, 1 equally per track, 2 individually); `joint` measures a group.
pub fn gains(levels: &BTreeMap<(usize, usize), f32>, process: usize, target: f32, joint: &dyn Fn(&[(usize, usize)]) -> f32) -> BTreeMap<(usize, usize), f32> {
    let keys: Vec<(usize, usize)> = levels.keys().copied().collect();
    let groups: Vec<Vec<(usize, usize)>> = match process {
        0 => vec![keys],
        1 => {
            let mut by: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
            for k in keys {
                by.entry(k.0).or_default().push(k);
            }
            by.into_values().collect()
        }
        _ => keys.into_iter().map(|k| vec![k]).collect(),
    };
    let mut out = BTreeMap::new();
    for g in groups {
        let lv = if g.len() == 1 { levels[&g[0]] } else { joint(&g) };
        if lv <= -119.0 {
            continue;
        }
        let db = ((target - lv) * 10.0).round() / 10.0;
        for k in g {
            out.insert(k, db.clamp(-96.0, 30.0));
        }
    }
    out
}

/// Apply: the selected audio cells normalized as the dialog says.
pub fn apply(app: &mut App) -> bool {
    let sel = selection(app);
    if sel.is_empty() {
        return false;
    }
    let d = app.live_loops.norm.clone();
    let mut audio: BTreeMap<(usize, usize), std::sync::Arc<Vec<f32>>> = BTreeMap::new();
    for k in &sel {
        let Some(path) = app.live_loops.cells.get(k).and_then(|c| c.audio.clone()) else { continue };
        let s = crate::liveloops::samples(app, &path).or_else(|| crate::audio::load_audio(&path).map(std::sync::Arc::new));
        if let Some(s) = s {
            audio.insert(*k, s);
        }
    }
    let levels: BTreeMap<(usize, usize), f32> = audio.iter().map(|(k, s)| (*k, level(s, d.method))).collect();
    let joint = |g: &[(usize, usize)]| {
        let all: Vec<f32> = g.iter().filter_map(|k| audio.get(k)).flat_map(|s| s.iter().copied()).collect();
        level(&all, d.method)
    };
    let target = if d.method == 0 { d.peak_db } else { d.lufs };
    let g = gains(&levels, d.process, target, &joint);
    crate::cellinsp::undoable(app, "Normalize Cell Gain", |app| {
        for (k, db) in g {
            if let Some(c) = app.live_loops.cells.get_mut(&k) {
                c.gain_db = db;
            }
        }
        app.dirty = true;
        true
    })
}

/// The Normalize Gain dialog.
pub fn dialog(app: &mut App, ctx: &egui::Context) {
    if !app.live_loops.norm.open {
        return;
    }
    let mut d = app.live_loops.norm.clone();
    let mut go = false;
    let mut open = true;
    egui::Window::new("Normalize Gain").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -80.0)).open(&mut open).show(ctx, |ui| {
        egui::Grid::new("cell-norm").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
            ui.label("Process:");
            egui::ComboBox::from_id_salt("cn-process").width(190.0).selected_text(PROCESSES[d.process]).show_ui(ui, |ui| {
                for (i, p) in PROCESSES.iter().enumerate() {
                    ui.selectable_value(&mut d.process, i, *p);
                }
            });
            ui.end_row();
            ui.label("Method:");
            egui::ComboBox::from_id_salt("cn-method").width(190.0).selected_text(METHODS[d.method]).show_ui(ui, |ui| {
                for (i, m) in METHODS.iter().enumerate() {
                    ui.selectable_value(&mut d.method, i, *m);
                }
            });
            ui.end_row();
            ui.label("Target Level:");
            if d.method == 0 {
                ui.add(egui::DragValue::new(&mut d.peak_db).range(-60.0..=0.0).speed(0.1).fixed_decimals(1).suffix(" dB"));
            } else {
                ui.add(egui::DragValue::new(&mut d.lufs).range(-60.0..=0.0).speed(0.1).fixed_decimals(1).suffix(" LUFS"));
            }
            ui.end_row();
        });
        ui.label(egui::RichText::new("Sets the selected audio cells' gain.").color(look::text_dim()));
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Apply").clicked() {
                go = true;
            }
            if ui.button("Cancel").clicked() {
                d.open = false;
            }
        });
    });
    if !open || go {
        d.open = false;
    }
    app.live_loops.norm = d;
    if go {
        apply(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_and_gains() {
        let half: Vec<f32> = (0..9600).map(|i| if i % 4 < 2 { 0.5 } else { -0.5 }).collect();
        assert!((level(&half, 0) - (-6.02)).abs() < 0.01);
        // a steady 1 kHz tone at -20 dBFS (each channel) reads about -20 + 3 LUFS (two channels)
        let tone: Vec<f32> = (0..48000 * 3).flat_map(|i| {
            let v = 0.1 * 2f32.sqrt() * (i as f32 * std::f32::consts::TAU * 1000.0 / 48000.0).sin();
            [v, v]
        }).collect();
        let l = level(&tone, 1);
        assert!((l - (-17.0)).abs() < 0.6, "{l}");
        let mut lv = BTreeMap::new();
        lv.insert((0, 0), -6.0);
        lv.insert((0, 1), -12.0);
        lv.insert((1, 0), -3.0);
        let joint = |g: &[(usize, usize)]| g.iter().map(|k| lv[k]).fold(-120.0f32, f32::max);
        let ind = gains(&lv, 2, 0.0, &joint);
        assert_eq!((ind[&(0, 0)], ind[&(0, 1)], ind[&(1, 0)]), (6.0, 12.0, 3.0));
        let per = gains(&lv, 1, 0.0, &joint);
        assert_eq!((per[&(0, 0)], per[&(0, 1)], per[&(1, 0)]), (6.0, 6.0, 3.0));
        let all = gains(&lv, 0, -1.0, &joint);
        assert!(all.values().all(|g| *g == 2.0));
    }
}
