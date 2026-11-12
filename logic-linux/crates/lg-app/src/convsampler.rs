//! Convert Regions to New Sampler Track (⌃E; the audio region shortcut menu's Convert ▸ Convert
//! to New Sampler Track…), as Logic's: a dialog with Create Zones From Regions / Create Zones
//! From Transient Markers, the Instrument Name and the Trigger Note Range (two pop-up menus, or
//! Learn: play the low and the high key), OK and Cancel. OK makes a new track whose Sampler has a
//! zone per region (in timeline order, on successive keys from the range's lowest note) or, for
//! one region and transient markers, a zone per slice from its first transient; and a MIDI
//! region of trigger notes at the regions' (or transients') positions, so it plays as before.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use egui::vec2;

use crate::app::App;
use crate::instwin;

pub const COMMANDS: &[&str] = &["Convert Regions to New Sampler Track…", "Convert to New Sampler Track…"];

/// One zone to make: a stretch of a file, where its trigger note goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub file: PathBuf,
    /// Frames in the file.
    pub start: usize,
    pub end: usize,
    /// The trigger note's position and length (project ticks).
    pub at: u32,
    pub len: u32,
}

#[derive(Clone)]
struct Dlg {
    regions: Vec<(usize, usize)>,
    transients: bool,
    name: String,
    lo: u8,
    hi: u8,
    /// Learn: 0 off, 1 waiting for the low key, 2 for the high key.
    learn: u8,
}

fn dlg_id() -> egui::Id {
    egui::Id::unique("convert-sampler-dialog")
}

/// The keys the pieces go on: successive keys from the range's lowest note, as many as fit.
pub fn assign_keys(n: usize, lo: u8, hi: u8) -> Vec<u8> {
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    (0..n).map(|k| lo as usize + k).take_while(|&k| k <= hi as usize).map(|k| k as u8).collect()
}

/// Slices of a region from its first transient: each runs to the next transient (the last to the
/// region's end). `trs` are (frame in the file, tick) pairs inside the region.
pub fn slices(file: &std::path::Path, end_frame: usize, end_tick: u32, trs: &[(usize, u32)]) -> Vec<Piece> {
    let mut v: Vec<(usize, u32)> = trs.to_vec();
    v.sort();
    v.dedup_by_key(|x| x.0);
    (0..v.len())
        .map(|i| {
            let (s, at) = v[i];
            let (e, to) = v.get(i + 1).copied().unwrap_or((end_frame, end_tick));
            Piece { file: file.to_path_buf(), start: s, end: e.max(s + 1), at, len: to.saturating_sub(at).max(1) }
        })
        .collect()
}

/// The audio regions the command works on: the selected ones, else the selected region.
fn targets(app: &App) -> Vec<(usize, usize)> {
    let Some(p) = &app.project else { return Vec::new() };
    let is_audio = |&(t, r): &(usize, usize)| p.tracks.get(t).and_then(|tr| tr.regions.get(r)).is_some_and(|g| g.audio.is_some());
    let mut v: Vec<(usize, usize)> = app.selected_regions.iter().copied().filter(is_audio).collect();
    if v.is_empty() {
        v.extend(app.selected_region.filter(is_audio));
    }
    // timeline order
    v.sort_by_key(|&(t, r)| (p.tracks[t].regions[r].start, t));
    v.dedup();
    v
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
    }
    let regions = targets(app);
    let Some(&(t, r)) = regions.first() else {
        app.tell("Select one or more audio regions to convert");
        return true;
    };
    let name = app.project.as_ref().map(|p| p.tracks[t].regions[r].name.clone()).unwrap_or_default();
    let single = regions.len() == 1;
    ctx.data_mut(|d| d.insert_temp(dlg_id(), Some(Dlg { regions, transients: single, name, lo: 36, hi: 96, learn: 0 })));
    true
}

/// A region's file, its offset and length in the file (frames) and its ticks.
fn span(app: &App, t: usize, r: usize) -> Option<(PathBuf, usize, usize, u32, u32, u32)> {
    let g = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?;
    let a = g.audio.as_ref()?;
    let rate = crate::projaudio::file_info(&a.file).map(|i| i.rate).unwrap_or(crate::audio::RATE);
    let off = app.audio_edit.regions.get(&(t, r)).map_or(0, |x| x.offset);
    let secs = app.seconds_at((g.start + g.length) as f64) - app.seconds_at(g.start as f64);
    let len = (secs.min(a.seconds.max(0.0)).max(0.0) * rate as f64).round() as usize;
    Some((a.file.clone(), off, len.max(1), g.start, g.length, rate))
}

/// The pieces for the dialog's choice.
fn pieces(app: &mut App, d: &Dlg) -> Vec<Piece> {
    if d.transients && d.regions.len() == 1 {
        let (t, r) = d.regions[0];
        if let Some((file, off, len, start, length, rate)) = span(app, t, r) {
            let s0 = app.seconds_at(start as f64);
            let trs: Vec<(usize, u32)> = crate::flex::transient_ticks(app, t, r)
                .into_iter()
                .filter(|&k| k >= start && k < start + length)
                .map(|k| (off + ((app.seconds_at(k as f64) - s0).max(0.0) * rate as f64).round() as usize, k))
                .collect();
            if !trs.is_empty() {
                return slices(&file, off + len, start + length, &trs);
            }
        }
    }
    d.regions.iter().filter_map(|&(t, r)| span(app, t, r)).map(|(file, off, len, start, length, _)| Piece { file, start: off, end: off + len, at: start, len: length.max(1) }).collect()
}

/// OK: the new track, its Sampler and the trigger region.
fn convert(app: &mut App, d: &Dlg) {
    let list = pieces(app, d);
    let keys = assign_keys(list.len(), d.lo, d.hi);
    if keys.is_empty() {
        app.tell("Nothing to convert");
        return;
    }
    let mut cache: HashMap<PathBuf, Arc<lg_synth::sampler::SampleData>> = HashMap::new();
    let mut doc = lg_synth::sampler_doc::SamplerDoc::new();
    doc.name = d.name.clone();
    for (p, &key) in list.iter().zip(&keys) {
        let data = match cache.get(&p.file) {
            Some(x) => x.clone(),
            None => {
                let Ok(a) = lg_synth::wav::read_audio(&p.file) else { continue };
                let x = Arc::new(lg_synth::sampler::SampleData { rate: a.rate as f32, ch: a.channels.into_iter().take(2).map(lg_synth::sampler::Pcm::F32).collect() });
                cache.insert(p.file.clone(), x.clone());
                x
            }
        };
        let zs = doc.add_loaded(vec![(p.file.clone(), data, None, Some(key))], lg_synth::sampler_doc::Drop::Key(key));
        if let Some(&zi) = zs.first() {
            let z = &mut doc.zones[zi];
            z.key_lo = key;
            z.key_hi = key;
            z.root = key;
            z.pitch = false;
            z.one_shot = true;
            z.start = p.start;
            z.end = p.end;
        }
    }
    app.checkpoint("Convert Regions to New Sampler Track");
    let Some(p) = app.project.as_mut() else { return };
    let mut tr = lg_core::Track::new(&d.name, lg_core::TrackKind::Instrument);
    tr.row = p.tracks.len() as u32 + 1;
    let at = list.iter().map(|x| x.at).min().unwrap_or(0);
    let end = list.iter().map(|x| x.at + x.len).max().unwrap_or(at + 1);
    let notes = list.iter().zip(&keys).map(|(x, &k)| lg_core::events::Note { channel: 0, pitch: k, velocity: 100, pos: x.at, len: x.len, muted: false }).collect();
    tr.regions.push(lg_core::Region { name: d.name.clone(), start: at, length: end.saturating_sub(at).max(1), notes, audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
    p.tracks.push(tr);
    let nt = p.tracks.len() - 1;
    app.programs.push(crate::app::program_for(&d.name, lg_core::TrackKind::Instrument));
    crate::trackarea::sync(app);
    let Some(nid) = instwin::track_id(app, nt) else { return };
    app.instrument_choice.insert(nt, "Sampler".into());
    app.inst.docs.insert(nid, doc);
    app.load_engine();
    instwin::bump(app, nt);
    app.selected_track = Some(nt);
    app.after_edit();
    app.tell(format!("Converted {} to the Sampler track “{}”", if list.len() == 1 { "1 zone".to_string() } else { format!("{} zones", keys.len()) }, d.name));
}

fn note_menu(ui: &mut egui::Ui, id: &str, v: &mut u8) {
    egui::ComboBox::from_id_salt(id).selected_text(crate::app::note_name(*v)).width(70.0).height(260.0).show_ui(ui, |ui| {
        for n in 0..=127u8 {
            ui.selectable_value(v, n, crate::app::note_name(n));
        }
    });
}

/// The dialog (drawn every frame while open).
pub fn dialog(app: &mut App, ctx: &egui::Context) {
    let Some(mut d) = ctx.data(|x| x.get_temp::<Option<Dlg>>(dlg_id())).flatten() else { return };
    // Learn: the next two keys played set the range
    if d.learn > 0 {
        if let Some(n) = app.inst.last_played.take() {
            if d.learn == 1 {
                d.lo = n;
                d.learn = 2;
            } else {
                d.hi = n.max(d.lo);
                d.learn = 0;
            }
        }
    }
    let mut done: Option<bool> = None;
    egui::Window::new("Convert Regions to New Sampler Track").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        ui.set_width(380.0);
        let single = d.regions.len() == 1;
        ui.radio_value(&mut d.transients, false, "Create Zones From Regions");
        ui.add_enabled_ui(single, |ui| {
            ui.radio_value(&mut d.transients, true, "Create Zones From Transient Markers");
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Instrument Name:");
            ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(220.0));
        });
        ui.horizontal(|ui| {
            ui.label("Trigger Note Range:");
            note_menu(ui, "conv-lo", &mut d.lo);
            ui.label("–");
            note_menu(ui, "conv-hi", &mut d.hi);
            let label = match d.learn {
                1 => "Play low key…",
                2 => "Play high key…",
                _ => "Learn",
            };
            if ui.add(egui::Button::selectable(d.learn > 0, label)).clicked() {
                d.learn = if d.learn > 0 { 0 } else { 1 };
                app.inst.last_played = None;
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("  OK  ").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(false);
                }
            });
        });
    });
    match done {
        Some(ok) => {
            ctx.data_mut(|x| x.insert_temp::<Option<Dlg>>(dlg_id(), None));
            if ok {
                if d.name.trim().is_empty() {
                    d.name = "Sampler".into();
                }
                convert(app, &d);
            }
        }
        None => {
            ctx.data_mut(|x| x.insert_temp(dlg_id(), Some(d)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_go_up_from_the_lowest_note_and_stop_at_the_top() {
        assert_eq!(assign_keys(3, 36, 96), vec![36, 37, 38]);
        assert_eq!(assign_keys(5, 60, 62), vec![60, 61, 62]);
        assert_eq!(assign_keys(2, 62, 60), vec![60, 61]);
    }

    #[test]
    fn slices_run_from_the_first_transient_to_the_next() {
        let f = std::path::Path::new("/x.wav");
        let s = slices(f, 1000, 4000, &[(600, 2400), (100, 400), (300, 1200)]);
        assert_eq!(s.len(), 3);
        assert_eq!((s[0].start, s[0].end, s[0].at, s[0].len), (100, 300, 400, 800));
        assert_eq!((s[2].start, s[2].end, s[2].at, s[2].len), (600, 1000, 2400, 1600));
    }
}
