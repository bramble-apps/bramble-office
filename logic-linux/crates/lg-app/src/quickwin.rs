//! The Quick Sampler window, as Logic's: the mode buttons (Classic, One Shot, Slice, Recorder),
//! the waveform display with the sample start and end markers, the loop markers (Classic with
//! Loop on) and the slice markers (Slice: Option-click adds one, drag moves it, double-click
//! removes it; Slice Sensitivity finds them again), a file dropped on it (or Load Audio File…)
//! loads it; then the Pitch, Filter and Amp controls. "Open in Sampler" turns the sound into a
//! Sampler group (Logic: a Quick Sampler sound opened in Sampler becomes a group of zones).

use std::path::PathBuf;
use std::sync::Arc;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_synth::quick::{QuickSampler, MODES};
use lg_synth::wav::Audio;

use crate::app::App;
use crate::instwin;
use crate::look;

#[derive(Clone, Default)]
pub struct QuickDoc {
    pub file: Option<PathBuf>,
    pub audio: Option<Arc<Audio>>,
    pub slices: Option<Vec<usize>>,
}

#[derive(Default)]
pub struct Ui {
    drag: Option<(u8, usize)>,
}

pub fn apply(d: &QuickDoc, q: &mut QuickSampler) {
    if let Some(a) = &d.audio {
        q.set_audio((**a).clone());
    }
    q.set_slices(d.slices.clone());
}

/// The waveform (peaks per pixel) with the start / end marked, and optional slice / loop lines.
pub fn draw_wave(ui: &egui::Ui, r: Rect, a: &Audio, start: f32, end: f32, extra: Option<(&[usize], Option<(f32, f32)>)>) {
    let p = ui.painter_at(r);
    p.rect_filled(r, 4.0, look::lane_bg());
    let n = a.frames().max(1);
    let w = r.width().max(1.0) as usize;
    let c = a.channels.first().map(|v| v.as_slice()).unwrap_or(&[]);
    for x in 0..w {
        let i0 = n * x / w;
        let i1 = (n * (x + 1) / w).max(i0 + 1).min(n);
        let step = ((i1 - i0) / 48).max(1);
        let (mut lo, mut hi) = (0.0f32, 0.0f32);
        for i in (i0..i1).step_by(step) {
            let s = c.get(i).copied().unwrap_or(0.0);
            lo = lo.min(s);
            hi = hi.max(s);
        }
        let xx = r.min.x + x as f32;
        p.vline(xx, (r.center().y - hi * r.height() * 0.48)..=(r.center().y - lo * r.height() * 0.48 + 0.5), Stroke::new(1.0, look::region_color(lg_core::TrackKind::Audio)));
    }
    let xf = |f: f32| r.min.x + f * r.width();
    p.rect_filled(Rect::from_min_max(r.min, pos2(xf(start), r.max.y)), 0.0, Color32::from_black_alpha(110));
    p.rect_filled(Rect::from_min_max(pos2(xf(end), r.min.y), r.max), 0.0, Color32::from_black_alpha(110));
    let blue = Color32::from_rgb(0x3c, 0x9c, 0xf0);
    p.vline(xf(start), r.y_range(), Stroke::new(1.5, blue));
    p.vline(xf(end), r.y_range(), Stroke::new(1.5, blue));
    if let Some((slices, lp)) = extra {
        for &s in slices {
            p.vline(xf(s as f32 / n as f32), r.y_range(), Stroke::new(1.0, Color32::from_rgb(0xf0, 0x80, 0x30)));
        }
        if let Some((ls, le)) = lp {
            let y = Color32::from_rgb(0xf5, 0xc5, 0x18);
            p.rect_filled(Rect::from_min_max(pos2(xf(ls), r.min.y), pos2(xf(le), r.max.y)), 0.0, y.gamma_multiply(0.15));
            p.vline(xf(ls), r.y_range(), Stroke::new(1.5, y));
            p.vline(xf(le), r.y_range(), Stroke::new(1.5, y));
        }
    }
}

fn load(app: &mut App, t: usize, path: &std::path::Path) {
    match lg_synth::wav::read_audio(path) {
        Ok(a) => {
            let Some(id) = instwin::track_id(app, t) else { return };
            instwin::checkpoint(app, t, "Load Audio File");
            app.inst.quick.insert(id, QuickDoc { file: Some(path.to_path_buf()), audio: Some(Arc::new(a)), slices: None });
            instwin::bump(app, t);
        }
        Err(e) => app.tell(format!("Couldn't load {}: {e}", path.display())),
    }
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some(id) = instwin::track_id(app, t) else { return };
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let get = |pid: u32| params.iter().position(|x| x.id == pid).map(|i| vals[i]).unwrap_or(0.0);
    let doc = app.inst.quick.get(&id).cloned().unwrap_or_default();
    // the sample the instrument plays (also one from a project or a patch)
    let shown = doc.audio.clone().or_else(|| instwin::with_inst(app, t, |q: &mut QuickSampler| q.audio().cloned().map(Arc::new)).flatten());
    let slices = instwin::with_inst(app, t, |q: &mut QuickSampler| q.slices.clone()).unwrap_or_default();
    let track = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    let dropped = std::mem::take(&mut app.inst.dropped);
    let mut set: Vec<(u32, f32)> = Vec::new();
    let mut new_slices: Option<Option<Vec<usize>>> = None;
    let mut load_dialog = false;
    let mut to_sampler = false;
    let mut open = true;
    let mut st = std::mem::take(&mut app.inst.quick_ui);
    let resp = egui::Window::new(format!("{track} — Quick Sampler")).id(egui::Id::unique(("quick-window", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(760.0, 520.0)).show(ctx, |ui| {
        let mode = get(0).round() as usize;
        ui.horizontal(|ui| {
            for (k, m) in MODES.iter().enumerate() {
                if ui.add(egui::Button::selectable(mode == k, *m)).clicked() {
                    set.push((0, k as f32));
                }
            }
            ui.add_enabled(false, egui::Button::new("Flex")).on_disabled_hover_text("Flex (the sample's speed kept at every pitch) isn't in Studio's Quick Sampler yet");
            if ui.button("Load Audio File…").clicked() {
                load_dialog = true;
            }
            if ui.add_enabled(shown.is_some(), egui::Button::new("Open in Sampler")).on_hover_text("Replaces Quick Sampler with Sampler: the sound becomes a group of zones").clicked() {
                to_sampler = true;
            }
            if let Some(f) = &doc.file {
                ui.label(egui::RichText::new(f.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()).color(look::text_dim()));
            }
        });
        if mode == 3 {
            ui.label(egui::RichText::new("Recorder: Studio doesn't record into Quick Sampler yet; record on an audio track and drag the file here.").color(look::text_dim()));
        }
        let (wr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 170.0), Sense::hover());
        match &shown {
            Some(a) => {
                let n = a.frames().max(1);
                let (s, e) = (get(2), get(3));
                let lp = (mode == 0 && get(4) >= 0.5).then(|| (get(5), get(6)));
                let sl: &[usize] = if mode == 2 { &slices } else { &[] };
                draw_wave(ui, wr, a, s, e, Some((sl, lp)));
                let resp = ui.interact(wr, ui.scope_id().with("qs-wave"), Sense::click_and_drag());
                let fx = |x: f32| ((x - wr.min.x) / wr.width()).clamp(0.0, 1.0);
                let xf = |f: f32| wr.min.x + f * wr.width();
                let m = ui.input(|i| i.modifiers);
                // markers: 0 start, 1 end, 2 loop start, 3 loop end, 4 a slice
                let mut marks: Vec<(u8, usize, f32)> = vec![(0, 0, s), (1, 0, e)];
                if let Some((ls, le)) = lp {
                    marks.push((2, 0, ls));
                    marks.push((3, 0, le));
                }
                for (i, &x) in sl.iter().enumerate() {
                    marks.push((4, i, x as f32 / n as f32));
                }
                let near = |x: f32| marks.iter().filter(|m| (xf(m.2) - x).abs() < 5.0).min_by(|a, b| (xf(a.2) - x).abs().partial_cmp(&(xf(b.2) - x).abs()).unwrap_or(std::cmp::Ordering::Equal)).map(|m| (m.0, m.1));
                if let Some(pp) = resp.hover_pos()
                    && near(pp.x).is_some()
                {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                if resp.drag_started()
                    && let Some(pp) = resp.interact_pointer_pos()
                {
                    st.drag = near(pp.x - resp.drag_delta().x);
                }
                if resp.dragged()
                    && let (Some((kind, i)), Some(pp)) = (st.drag, resp.interact_pointer_pos())
                {
                    let f = fx(pp.x);
                    match kind {
                        0 => set.push((2, f.min(e - 0.001))),
                        1 => set.push((3, f.max(s + 0.001))),
                        2 => set.push((5, f)),
                        3 => set.push((6, f)),
                        _ => {
                            let mut v = slices.clone();
                            if let Some(x) = v.get_mut(i) {
                                *x = (f * n as f32) as usize;
                            }
                            new_slices = Some(Some(v));
                        }
                    }
                }
                if resp.drag_stopped() {
                    st.drag = None;
                }
                if mode == 2
                    && let Some(pp) = resp.interact_pointer_pos()
                {
                    if resp.clicked() && m.alt {
                        let mut v = slices.clone();
                        v.push((fx(pp.x) * n as f32) as usize);
                        v.sort_unstable();
                        new_slices = Some(Some(v));
                    } else if resp.double_clicked()
                        && let Some((4, i)) = near(pp.x)
                    {
                        let mut v = slices.clone();
                        v.remove(i);
                        new_slices = Some(Some(v));
                    }
                }
                if mode == 2 {
                    ui.label(egui::RichText::new(format!("{} slices from {} (Option-click adds, double-click removes)", sl.len(), lg_synth::sampler_doc::note_name(36))).size(11.0).color(look::text_dim()));
                }
            }
            None => {
                ui.painter().rect_filled(wr, 4.0, look::lane_bg());
                ui.painter().text(wr.center(), Align2::CENTER_CENTER, "Drag an audio file here", look::regular(14.0), look::text_dim());
            }
        }
        ui.add_space(6.0);
        // the controls: Pitch, Filter, Amp, the mode's own
        let groups: &[(&str, &[u32])] = &[("Pitch", &[1, 16, 15, 17, 18]), ("Filter", &[11, 12]), ("Amp", &[7, 8, 9, 10, 13]), ("Mode", &[4, 14])];
        ui.horizontal_top(|ui| {
            for (title, ids) in groups {
                egui::Frame::new().fill(look::group_bg()).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(*title).strong());
                        for &pid in *ids {
                            let Some(prm) = params.iter().find(|x| x.id == pid) else { continue };
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(prm.name).size(11.0).color(look::text_dim()));
                                let mut v = get(pid);
                                match prm.unit {
                                    lg_dsp::Unit::OnOff => {
                                        let mut on = v >= 0.5;
                                        if ui.checkbox(&mut on, "").changed() {
                                            set.push((pid, if on { 1.0 } else { 0.0 }));
                                        }
                                    }
                                    _ => {
                                        let note = pid == 1;
                                        if ui.add(egui::DragValue::new(&mut v).range(prm.min..=prm.max).speed(if note { 0.2 } else { (prm.max - prm.min) / 300.0 }).custom_formatter(|x, _| if note { lg_synth::sampler_doc::note_name(x.round() as u8) } else { format!("{x:.2}") })).changed() {
                                            set.push((pid, if note { v.round() } else { v }));
                                            if pid == 14 {
                                                // Slice Sensitivity finds the slices again
                                                new_slices = Some(None);
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    });
                });
            }
        });
    });
    app.inst.rect = resp.map(|r| r.response.rect);
    app.inst.quick_ui = st;
    if let Some(f) = dropped.first() {
        load(app, t, f);
    }
    if load_dialog
        && let Some(p) = kn_ui::files::FileDialog::new().set_title("Load Audio File").add_filter("Audio", &["wav", "aif", "aiff", "caf", "flac", "WAV", "AIF", "AIFF"]).pick_file()
    {
        load(app, t, &p);
    }
    if !set.is_empty() {
        instwin::checkpoint_once(app, ctx, t, "Quick Sampler Setting");
        instwin::set_params(app, t, &set);
    }
    if let Some(s) = new_slices {
        instwin::checkpoint_once(app, ctx, t, "Move Slice");
        let d = app.inst.quick.entry(id).or_default();
        if d.audio.is_none() {
            d.audio = shown.clone();
        }
        d.slices = s.clone();
        instwin::with_inst(app, t, |q: &mut QuickSampler| q.set_slices(s));
    }
    if to_sampler && let Some(a) = shown {
        // the sound as a Sampler group (its zones: one, or one per slice)
        let mut doc = lg_synth::sampler_doc::SamplerDoc::new();
        let data = Arc::new(lg_synth::sampler::SampleData { rate: a.rate as f32, ch: a.channels.iter().take(2).map(|c| lg_synth::sampler::Pcm::F32(c.clone())).collect() });
        let path = app.inst.quick.get(&id).and_then(|d| d.file.clone()).unwrap_or_default();
        let root = get(1).round() as u8;
        let mode = get(0).round() as usize;
        let n = a.frames();
        if mode == 2 && !slices.is_empty() {
            let zs = doc.add_loaded(vec![(path.clone(), data.clone(), None, Some(36))], lg_synth::sampler_doc::Drop::Key(36));
            if let Some(&z) = zs.first() {
                let end = (get(3) * n as f32) as usize;
                for (k, &s0) in slices.iter().enumerate() {
                    let zi = if k == 0 { z } else {
                        let c = doc.zones[z].clone();
                        doc.zones.push(c);
                        doc.zones.len() - 1
                    };
                    let zz = &mut doc.zones[zi];
                    zz.start = s0;
                    zz.end = slices.get(k + 1).copied().unwrap_or(end).max(s0 + 1);
                    let key = (36 + k).min(127) as u8;
                    zz.key_lo = key;
                    zz.key_hi = key;
                    zz.root = key;
                    zz.one_shot = true;
                }
            }
        } else {
            let zs = doc.add_loaded(vec![(path, data, None, Some(root))], lg_synth::sampler_doc::Drop::Key(root));
            if let Some(&z) = zs.first() {
                let zz = &mut doc.zones[z];
                zz.key_lo = 0;
                zz.key_hi = 127;
                zz.start = (get(2) * n as f32) as usize;
                zz.end = (get(3) * n as f32) as usize;
                zz.one_shot = mode == 1;
                zz.loop_on = get(4) >= 0.5;
                zz.loop_start = (get(5) * n as f32) as usize;
                zz.loop_end = (get(6) * n as f32) as usize;
                zz.reverse = get(17) >= 0.5;
                zz.tune = get(16);
                zz.pitch = get(15) >= 0.5;
            }
        }
        doc.groups.retain(|_| true);
        app.instrument_choice.insert(t, "Sampler".into());
        app.inst.rev.remove(&id);
        app.load_engine();
        app.inst.docs.insert(id, doc);
        instwin::bump(app, t);
    }
    if !open {
        app.plugin_window = None;
        app.inst.rect = None;
    }
}
