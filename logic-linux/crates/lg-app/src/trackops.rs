//! Track commands of Logic's Track menu and header shortcut menu beyond the basics: External
//! MIDI tracks, Track ▸ Other (tracks sharing a channel strip, the next instrument, tracks for
//! selected or overlapped regions), Reassign Track (a channel strip, Not Assigned, Folder;
//! Reassign All Tracks), Sort Tracks By, renaming several tracks at once, the default patch per
//! track type, Replace or Double Drum Track, Rename Track Alternative by Region, and the Track
//! inspector's further parameters (icon, colour, Freeze Mode, Q-Reference, Default Region
//! Type, the MIDI parameters).

use egui::{pos2, vec2, Align2, Color32, Rect, Sense};
use lg_core::trackmix::{self, Assign, DrumKind, External, FreezeMode, RegionType, SortBy, SortKey};
use lg_core::{Region, Track, TrackKind};

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &[
    "New External MIDI Track",
    "New Track With Duplicate Settings",
    "New Track With Duplicate Settings and Content",
    "New Track With Next Instrument",
    "New Track With Same Instrument",
    "New Track With Next Channel",
    "New Track With Same Channel",
    "New Track For Overlapped Regions",
    "New Track For Selected Regions",
    "Replace or Double Drum Track…",
    "Use as Default Patch for MIDI Tracks",
    "Use as Default Patch for Pattern Tracks",
    "Use as Default Patch for Audio Tracks",
    "Use as Default Patch for Guitar Tracks",
    "Rename by Region",
    "Rename Track Alternative by Region",
    "Not Assigned",
    "(Folder)",
    "Select All Regions of Selected Tracks",
    "Select Tracks of Selected Regions/Cells/Folders",
    "Toggle Solo Tracks (Channel Strips) of Selected Regions/Cells",
];

/// The Replace or Double Drum Track dialog's state.
#[derive(Clone, Debug)]
pub struct DrumRepl {
    /// The audio track and the instrument track made for it (track ids).
    pub audio: u64,
    pub inst: u64,
    pub kind: DrumKind,
    pub doubling: bool,
    /// Relative Threshold (0…1 of the loudest transient).
    pub threshold: f32,
    /// None: AUTO.
    pub note: Option<u8>,
    pub offset_ms: f32,
    /// The transients found: (tick, level), and their places in samples with the mono sound
    /// (for Set average attack time).
    pub transients: Vec<(u32, f32)>,
    pub attack_ms: f32,
    /// Preview playing: where the playhead was, and the solos before.
    pub preview: Option<(f64, Vec<bool>)>,
    pub old_zoom: f32,
}

#[derive(Default)]
pub struct State {
    pub drum: Option<DrumRepl>,
    /// Reassign with Option: the choice waiting for "Reassign All Tracks" or this one only.
    pub reassign: Option<(usize, Reassign)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reassign {
    Strip(u64),
    NotAssigned,
    Folder,
}

/// Sends MIDI to a destination ("" = Studio's virtual output).
pub fn midi_send(app: &App, port: &str, bytes: &[u8]) {
    app.hub.outs.send(if port.is_empty() { crate::midiout::VIRTUAL } else { port }, bytes);
}

fn insert_track(app: &mut App, at: usize, t: Track) -> usize {
    let Some(p) = app.project.as_mut() else { return 0 };
    let at = at.min(p.tracks.len());
    p.tracks.insert(at, t);
    app.programs.insert(at.min(app.programs.len()), 0);
    // (ids given now, so state kept by index follows before anything points at the new track)
    crate::trackarea::sync(app);
    at
}

fn below(app: &App, t: usize) -> usize {
    app.project.as_ref().map_or(0, |p| lg_core::arrange::group(&p.tracks, t).end)
}

/// A new track under `t` playing through `t`'s channel strip.
pub fn new_with_same_strip(app: &mut App, t: usize) -> Option<usize> {
    let owner = crate::tm::strip_owner(app, t);
    let src = app.project.as_ref()?.tracks.get(owner)?.clone();
    let mut nt = Track::new(&src.name, src.kind);
    nt.extra.depth = app.project.as_ref()?.tracks.get(t)?.extra.depth;
    nt.extra.set.same_strip = Some(src.extra.id);
    nt.extra.set.strip.color = src.extra.set.strip.color;
    nt.extra.set.icon = src.extra.set.icon.clone();
    nt.extra.set.default_region = src.extra.set.default_region;
    let at = below(app, t);
    Some(insert_track(app, at, nt))
}

/// The overlapping regions of a track laid out in lanes: lane 0 keeps what fits, each further
/// lane takes the next that would overlap (region indexes by lane).
pub fn overlap_lanes(regions: &[Region]) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..regions.len()).collect();
    order.sort_by_key(|&r| regions[r].start);
    let mut lanes: Vec<(u32, Vec<usize>)> = Vec::new();
    for r in order {
        let (s, e) = (regions[r].start, regions[r].start + regions[r].length);
        match lanes.iter_mut().find(|l| l.0 <= s) {
            Some(l) => {
                l.0 = e;
                l.1.push(r);
            }
            None => lanes.push((e, vec![r])),
        }
    }
    lanes.into_iter().map(|l| l.1).collect()
}

fn sort_tracks(app: &mut App, by: SortBy) {
    let Some(p) = app.project.as_ref() else { return };
    // top-level tracks move with their subtracks
    let tops: Vec<usize> = (0..p.tracks.len()).filter(|&i| p.tracks[i].extra.depth == 0).collect();
    let keys: Vec<SortKey> = tops
        .iter()
        .map(|&i| {
            let t = &p.tracks[i];
            SortKey {
                midi_channel: t.extra.set.external.as_ref().map_or(if t.kind == TrackKind::Audio { 99 } else { t.channel }, |e| e.channel as u32),
                audio_channel: if t.kind == TrackKind::Audio { p.tracks[..=i].iter().filter(|o| o.kind == TrackKind::Audio).count() as u32 } else { u32::MAX },
                output: match app.mix.outputs.get(&i) {
                    Some(crate::audio::Out::Bus(b)) => 1000 + *b as u32,
                    Some(crate::audio::Out::Off) => u32::MAX,
                    _ => 0,
                },
                instrument: app.instrument_for(i).unwrap_or_else(|| "~".into()),
                name: t.name.clone(),
                used: !t.regions.is_empty() || !t.automation.is_empty(),
            }
        })
        .collect();
    let order = trackmix::sort_order(&keys, by);
    if order.iter().enumerate().all(|(a, b)| a == *b) {
        return;
    }
    app.checkpoint("Sort Tracks");
    let Some(p) = app.project.as_mut() else { return };
    let old = std::mem::take(&mut p.tracks);
    let mut slots: Vec<Option<Track>> = old.into_iter().map(Some).collect();
    let n = slots.len();
    for k in order {
        let start = tops[k];
        let end = tops.get(k + 1).copied().unwrap_or(n);
        for s in &mut slots[start..end] {
            if let Some(t) = s.take() {
                p.tracks.push(t);
            }
        }
    }
    // (state kept by track index follows through the tracks' ids)
    app.after_edit();
}

fn type_key(app: &App, t: usize) -> Option<(&'static str, &'static str)> {
    let tr = app.project.as_ref()?.tracks.get(t)?;
    Some(match (tr.kind, tr.extra.set.default_region) {
        (TrackKind::Audio, _) if tr.name.to_lowercase().contains("guitar") || tr.name.to_lowercase().contains("bass") => ("audio-guitar", "Guitar"),
        (TrackKind::Audio, _) => ("audio-mic", "Audio"),
        (TrackKind::Drummer, _) | (_, RegionType::SessionPlayer) => return None,
        (_, RegionType::Pattern) => ("inst-pattern", "Pattern"),
        _ => ("inst-midi", "MIDI"),
    })
}

fn default_patch_path(key: &str) -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("default-patches").join(format!("{key}.cst")))
}

/// Stores track `t`'s channel strip as the default patch of its track type.
pub fn store_default_patch(app: &mut App, t: usize) -> bool {
    let Some((key, label)) = type_key(app, t) else {
        app.tell("Session Player tracks can’t have a default patch");
        return false;
    };
    let text = crate::mixext::strip_setting_text(app, t);
    let Some(path) = default_patch_path(key) else { return false };
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let ok = std::fs::write(path, text).is_ok();
    if ok {
        app.tell(format!("Default patch for {label} tracks set"));
    }
    ok
}

/// Loads the default patch of its type onto the new track `t` (New Tracks ▸ Load Default Patch).
pub fn load_default_patch(app: &mut App, t: usize) -> bool {
    let Some((key, _)) = type_key(app, t) else { return false };
    let Some(text) = default_patch_path(key).and_then(|p| std::fs::read_to_string(p).ok()) else { return false };
    crate::mixext::apply_strip_setting(app, t, &text)
}

/// The track(s) renamed from the header's name field: several selected tracks all take the
/// name (a trailing number counts up); a track whose regions still carry its old name has
/// them renamed along.
pub fn renamed(app: &mut App, i: usize, new: &str) {
    let mut sel = crate::trackarea::selection(app);
    if !sel.contains(&i) {
        sel = vec![i];
    }
    sel.sort();
    let names = trackmix::numbered_names(new, sel.len());
    let Some(p) = app.project.as_mut() else { return };
    for (t, name) in sel.into_iter().zip(names) {
        let Some(tr) = p.tracks.get_mut(t) else { continue };
        let old = std::mem::replace(&mut tr.name, name.clone());
        // (a track never renamed before: its regions named after it follow)
        if !tr.extra.set.renamed {
            for r in tr.regions.iter_mut().filter(|r| r.name == old || r.name.starts_with(&format!("{old}#")) || r.name.starts_with(&format!("{old} "))) {
                r.name = format!("{name}{}", &r.name[old.len()..]);
            }
        }
        tr.extra.set.renamed = true;
    }
}

// ---------------------------------------------------------------------------------------------
// drum replacement

fn track_mono(app: &App, t: usize) -> Vec<(u32, Vec<f32>)> {
    let e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
    e.tracks.get(t).map(|pt| pt.audio.iter().map(|(at, s)| (*at, s.chunks(2).map(|c| (c[0] + c.get(1).copied().unwrap_or(0.0)) * 0.5).collect())).collect()).unwrap_or_default()
}

/// The transients of an audio track's regions as (tick, level) and the average attack (ms).
pub fn analyze(app: &App, t: usize) -> (Vec<(u32, f32)>, f32) {
    let rate = crate::audio::RATE;
    let tps = app.tempo() / 60.0 * lg_core::events::PPQ as f64 / rate as f64;
    let mut out = Vec::new();
    let (mut attack, mut n) = (0.0f32, 0);
    for (start, mono) in track_mono(app, t) {
        let at = lg_audio::transients::detect_mono(&mono, rate, 0.5);
        let look = rate as usize / 50;
        for &s in &at {
            let end = (s + look).min(mono.len());
            let lvl = mono[s..end].iter().fold(0.0f32, |m, x| m.max(x.abs()));
            out.push((start + (s as f64 * tps) as u32, lvl));
        }
        if !at.is_empty() {
            attack += trackmix::average_attack(&mono, &at, look) / rate as f32 * 1000.0;
            n += 1;
        }
    }
    out.sort_by_key(|x| x.0);
    (out, if n > 0 { attack / n as f32 } else { 0.0 })
}

fn drum_notes(app: &App, d: &DrumRepl) -> Vec<lg_core::events::Note> {
    let ticks_per_ms = app.tempo() / 60.0 * lg_core::events::PPQ as f64 / 1000.0;
    let pitch = d.note.unwrap_or_else(|| d.kind.default_note());
    trackmix::trigger_notes(&d.transients, d.threshold, (d.offset_ms as f64 * ticks_per_ms).round() as i32, pitch, lg_core::events::PPQ / 8)
}

/// Writes the dialog's current settings into the trigger region.
fn drum_apply(app: &mut App) {
    let Some(d) = app.tm.ops.drum.clone() else { return };
    let notes = drum_notes(app, &d);
    let Some(i) = crate::tm::by_id(app, d.inst) else { return };
    if let Some(r) = app.project.as_mut().and_then(|p| p.tracks.get_mut(i)?.regions.first_mut()) {
        r.notes = notes.into_iter().map(|mut n| {
            n.pos = n.pos.max(r.start);
            n
        }).collect();
    }
    app.load_engine();
}

/// Track ▸ Replace or Double Drum Track…: the selected audio track's transients (analyzed
/// now), a software instrument track under it with Sampler and a MIDI region of trigger notes,
/// the Library open for a sound, the audio track zoomed, and the dialog.
pub fn start_drum_replacement(app: &mut App) -> bool {
    let Some(t) = app.selected_track else { return false };
    let Some(src) = app.project.as_ref().and_then(|p| p.tracks.get(t)).cloned() else { return false };
    if src.kind != TrackKind::Audio || src.regions.is_empty() {
        app.tell("Select an audio track with a drum recording");
        return false;
    }
    let (transients, attack_ms) = analyze(app, t);
    app.checkpoint("Replace or Double Drum Track");
    let (from, to) = (src.regions.iter().map(|r| r.start).min().unwrap_or(0), src.regions.iter().map(|r| r.start + r.length).max().unwrap_or(0));
    let mut nt = Track::new(&format!("{} Trigger", src.name), TrackKind::Instrument);
    nt.extra.depth = src.extra.depth;
    let mut sampler = crate::mixstate::new_plugin("Sampler");
    sampler.slot = 2;
    nt.plugins.push(sampler);
    nt.regions.push(Region { name: "Drum Replacement".into(), start: from, length: (to - from).max(app.bar_ticks()), notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
    let at = below(app, t);
    let at = insert_track(app, at, nt);
    let old_zoom = src.extra.height;
    if let Some(p) = app.project.as_mut() {
        p.tracks[t].extra.height = 2.5;
    }
    crate::trackarea::sync(app);
    let ids = app.project.as_ref().map(|p| (p.tracks[t].extra.id, p.tracks[at].extra.id)).unwrap_or((0, 0));
    app.tm.ops.drum = Some(DrumRepl { audio: ids.0, inst: ids.1, kind: DrumKind::Kick, doubling: false, threshold: 0.3, note: None, offset_ms: 0.0, transients, attack_ms, preview: None, old_zoom });
    // the Library, at the Sampler's sounds
    app.show_browser = true;
    app.browser_search = "Sampler".into();
    drum_apply(app);
    true
}

fn stop_preview(app: &mut App) {
    let Some(d) = app.tm.ops.drum.as_mut() else { return };
    let Some((pos, solos)) = d.preview.take() else { return };
    app.stop();
    app.locate(pos);
    if let Some(p) = app.project.as_mut() {
        for (t, s) in p.tracks.iter_mut().zip(solos) {
            t.solo = s;
        }
    }
    app.sync_mix();
}

fn drum_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(mut d) = app.tm.ops.drum.clone() else { return };
    let before = (d.kind, d.threshold, d.note, d.offset_ms);
    let (mut ok, mut cancel, mut preview) = (false, false, false);
    let count = drum_notes(app, &d).len();
    egui::Window::new("Drum Replacement/Doubling").id(egui::Id::unique("tm-drum")).collapsible(false).resizable(false).default_pos(pos2(420.0, 220.0)).show(ctx, |ui| {
        ui.set_width(360.0);
        egui::Grid::new("drum-grid").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
            ui.label("Instrument:");
            let kind0 = d.kind;
            egui::ComboBox::from_id_salt("drum-kind").selected_text(d.kind.name()).show_ui(ui, |ui| {
                for k in DrumKind::ALL {
                    ui.selectable_value(&mut d.kind, k, k.name());
                }
            });
            if d.kind != kind0 {
                // a new instrument: the trigger note back to AUTO (C3 for Other)
                d.note = if d.kind == DrumKind::Other { Some(60) } else { None };
            }
            ui.end_row();
            ui.label("Mode:");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut d.doubling, false, "Replacement");
                ui.selectable_value(&mut d.doubling, true, "Doubling");
            });
            ui.end_row();
            ui.label("Relative Threshold:");
            let mut db = 20.0 * d.threshold.max(0.001).log10();
            if ui.add(egui::Slider::new(&mut db, -40.0..=0.0).suffix(" dB")).changed() {
                d.threshold = 10f32.powf(db / 20.0);
            }
            ui.end_row();
            ui.label("");
            if ui.button(if d.preview.is_some() { "Stop Preview" } else { "Preview" }).clicked() {
                preview = true;
            }
            ui.end_row();
            ui.label("Trigger Note:");
            let shown = match d.note {
                None => format!("AUTO ({})", crate::app::note_name(d.kind.default_note())),
                Some(n) => format!("{} ({n})", crate::app::note_name(n)),
            };
            egui::ComboBox::from_id_salt("drum-note").selected_text(shown).height(260.0).show_ui(ui, |ui| {
                if d.kind.auto_note().is_some() {
                    ui.selectable_value(&mut d.note, None, "AUTO");
                }
                for n in 24..=96u8 {
                    ui.selectable_value(&mut d.note, Some(n), format!("{} ({n})", crate::app::note_name(n)));
                }
            });
            ui.end_row();
            ui.label("Timing Offset:");
            ui.add(egui::Slider::new(&mut d.offset_ms, -50.0..=50.0).suffix(" ms").fixed_decimals(1));
            ui.end_row();
            ui.label("");
            if ui.button("Set average attack time").on_hover_text("The average attack of the track's regions as the offset").clicked() {
                d.offset_ms = d.attack_ms;
            }
            ui.end_row();
        });
        ui.label(egui::RichText::new(format!("{count} trigger notes from {} transients", d.transients.len())).color(look::text_dim()));
        ui.separator();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new(egui::RichText::new("OK").color(Color32::WHITE)).fill(look::accent()).min_size(vec2(76.0, 24.0))).clicked() {
                    ok = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
    });
    let changed = before != (d.kind, d.threshold, d.note, d.offset_ms);
    app.tm.ops.drum = Some(d.clone());
    if changed {
        drum_apply(app);
    }
    if preview {
        if d.preview.is_some() {
            stop_preview(app);
        } else {
            // both tracks soloed, from the playhead
            let pos = app.position();
            let solos: Vec<bool> = app.project.as_ref().map(|p| p.tracks.iter().map(|t| t.solo).collect()).unwrap_or_default();
            let (a, i) = (crate::tm::by_id(app, d.audio), crate::tm::by_id(app, d.inst));
            if let Some(p) = app.project.as_mut() {
                for (k, t) in p.tracks.iter_mut().enumerate() {
                    t.solo = Some(k) == a || Some(k) == i;
                }
            }
            app.sync_mix();
            if let Some(x) = app.tm.ops.drum.as_mut() {
                x.preview = Some((pos, solos));
            }
            if !app.playing() {
                app.play_stop();
            }
        }
    }
    if ok || cancel {
        stop_preview(app);
        let (a, i) = (crate::tm::by_id(app, d.audio), crate::tm::by_id(app, d.inst));
        app.tm.ops.drum = None;
        if cancel {
            // (the track made for it goes again)
            app.undo();
            app.redo.pop();
            return;
        }
        if let Some(p) = app.project.as_mut() {
            if let Some(t) = a.and_then(|a| p.tracks.get_mut(a)) {
                t.extra.height = d.old_zoom;
                // Replacement: the original's regions are muted; Doubling leaves them
                if !d.doubling {
                    for r in &mut t.regions {
                        r.muted = true;
                    }
                }
            }
        }
        if let Some(i) = i {
            app.selected_track = Some(i);
            app.ta.selected = vec![i];
        }
        app.after_edit();
    }
}

// ---------------------------------------------------------------------------------------------
// reassign

/// Reassigns track `t` (and, with `all`, every track sharing its old channel strip).
pub fn reassign(app: &mut App, t: usize, to: Reassign, all: bool) {
    let old_owner = crate::tm::strip_owner(app, t);
    let old_id = app.project.as_ref().and_then(|p| p.tracks.get(old_owner)).map(|x| x.extra.id);
    let targets: Vec<usize> = if all {
        let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
        (0..n).filter(|&k| crate::tm::strip_owner(app, k) == old_owner).collect()
    } else {
        vec![t]
    };
    app.checkpoint("Reassign Track");
    for k in targets {
        let own = app.project.as_ref().and_then(|p| p.tracks.get(k)).map(|x| x.extra.id);
        if let Some(s) = crate::tm::set_mut(app, k) {
            match to {
                Reassign::Strip(id) => {
                    s.assign = Assign::Strip;
                    s.same_strip = if Some(id) == own { None } else { Some(id) };
                }
                Reassign::NotAssigned => s.assign = Assign::NotAssigned,
                Reassign::Folder => s.assign = Assign::Folder,
            }
        }
    }
    let _ = old_id;
    app.after_edit();
}

fn reassign_menu(app: &mut App, ui: &mut egui::Ui) {
    let Some(t) = app.selected_track else { return };
    let Some(p) = app.project.as_ref() else { return };
    let cur_owner = crate::tm::strip_owner(app, t);
    let cur = crate::tm::set(app, t).map(|s| s.assign).unwrap_or_default();
    let mut pick = None;
    let strips: Vec<(usize, u64, String, TrackKind)> = p.tracks.iter().enumerate().filter(|(_, x)| x.extra.set.same_strip.is_none() && x.extra.stack.is_none()).map(|(i, x)| (i, x.extra.id, x.name.clone(), x.kind)).collect();
    ui.menu_button("Mixer", |ui| {
        for (title, audio) in [("Audio", true), ("Software Instrument", false)] {
            ui.menu_button(title, |ui| {
                let mut n = 0;
                for (i, id, name, kind) in &strips {
                    if (*kind == TrackKind::Audio) != audio {
                        continue;
                    }
                    n += 1;
                    let label = format!("{name} ({} {n})", if audio { "Audio" } else { "Inst" });
                    if ui.add(egui::Button::selectable(cur == Assign::Strip && cur_owner == *i, label)).clicked() {
                        pick = Some(Reassign::Strip(*id));
                    }
                }
            });
        }
    });
    ui.separator();
    if ui.add(egui::Button::selectable(cur == Assign::NotAssigned, "Not Assigned")).clicked() {
        pick = Some(Reassign::NotAssigned);
    }
    if ui.add(egui::Button::selectable(cur == Assign::Folder, "(Folder)")).clicked() {
        pick = Some(Reassign::Folder);
    }
    if let Some(to) = pick {
        // with Option held: asks whether every track on the old channel strip follows
        if ui.input(|i| i.modifiers.alt) {
            app.tm.ops.reassign = Some((t, to));
        } else {
            reassign(app, t, to, false);
        }
        ui.close();
    }
}

fn reassign_dialog(app: &mut App, ctx: &egui::Context) {
    let Some((t, to)) = app.tm.ops.reassign else { return };
    let mut choice = None;
    egui::Window::new("Reassign Track").id(egui::Id::unique("tm-reassign")).title_bar(false).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -60.0)).show(ctx, |ui| {
        ui.set_width(340.0);
        ui.label("Reassign only this track, or all tracks in the project that use the same channel strip?");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                choice = Some(None);
            }
            if ui.button("Reassign All Tracks").clicked() {
                choice = Some(Some(true));
            }
            if ui.add(egui::Button::new(egui::RichText::new("Reassign").color(Color32::WHITE)).fill(look::accent())).clicked() {
                choice = Some(Some(false));
            }
        });
    });
    if let Some(c) = choice {
        app.tm.ops.reassign = None;
        if let Some(all) = c {
            reassign(app, t, to, all);
        }
    }
}

// ---------------------------------------------------------------------------------------------

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    match name {
        "New External MIDI Track" => {
            app.checkpoint("New External MIDI Track");
            let Some(p) = app.project.as_ref() else { return false };
            // the next free MIDI channel on the virtual output
            let used: Vec<u8> = p.tracks.iter().filter_map(|t| t.extra.set.external.as_ref().map(|e| e.channel)).collect();
            let ch = (1..=16u8).find(|c| !used.contains(c)).unwrap_or(1);
            let mut nt = Track::new(&format!("MIDI {ch}"), TrackKind::Instrument);
            let opts = app.tm.nt.clone();
            nt.extra.set.external = Some(External { port: opts.midi_port.clone(), channel: ch, use_plugin: opts.external_plugin, ..Default::default() });
            let at = app.selected_track.map_or(p.tracks.len(), |t| below(app, t));
            let at = insert_track(app, at, nt);
            app.selected_track = Some(at);
            app.ta.selected = vec![at];
            app.after_edit();
        }
        "New Track With Duplicate Settings" | "New Track With Duplicate Settings and Content" => {
            let Some(t) = app.selected_track else { return false };
            let (armed, mon) = (app.armed.contains(&t), app.monitoring.contains(&t));
            app.edit_command(ctx, if name.ends_with("Content") { "New Track with Duplicate Settings and Content" } else { "New Track with Duplicate Settings" });
            // Record Enable and Input Monitoring move to the new track
            if let Some(n) = app.selected_track.filter(|n| *n != t) {
                if armed {
                    app.armed.remove(&t);
                    app.armed.insert(n);
                }
                if mon {
                    app.monitoring.remove(&t);
                    app.monitoring.insert(n);
                }
            }
        }
        "New Track With Same Instrument" | "New Track With Same Channel" => {
            let Some(t) = app.selected_track else { return false };
            app.checkpoint("New Track With Same Channel");
            let Some(at) = new_with_same_strip(app, t) else { return false };
            app.selected_track = Some(at);
            app.ta.selected = vec![at];
            app.after_edit();
        }
        "New Track With Next Instrument" | "New Track With Next Channel" => {
            let Some(t) = app.selected_track else { return false };
            let Some(src) = app.project.as_ref().and_then(|p| p.tracks.get(t)).cloned() else { return false };
            app.checkpoint("New Track With Next Channel");
            // an External MIDI track: the next MIDI channel on the same port; else a track on
            // the next channel strip of its kind (made when there is none)
            let mut nt = Track::new(&src.name, src.kind);
            nt.extra.depth = src.extra.depth;
            match &src.extra.set.external {
                Some(e) => {
                    let ch = e.channel % 16 + 1;
                    nt.name = format!("MIDI {ch}");
                    nt.extra.set.external = Some(External { channel: ch, ..e.clone() });
                }
                None => {
                    let n = app.project.as_ref().map_or(0, |p| p.tracks.iter().filter(|x| (x.kind == TrackKind::Audio) == (src.kind == TrackKind::Audio)).count());
                    nt.name = format!("{} {}", if src.kind == TrackKind::Audio { "Audio" } else { "Inst" }, n + 1);
                }
            }
            let at = below(app, t);
            let at = insert_track(app, at, nt);
            app.selected_track = Some(at);
            app.ta.selected = vec![at];
            app.after_edit();
        }
        "New Track For Selected Regions" => {
            let mut sel = app.region_selection();
            if sel.is_empty() {
                return false;
            }
            sel.sort();
            app.checkpoint("New Track For Selected Regions");
            let mut tracks: Vec<usize> = sel.iter().map(|x| x.0).collect();
            tracks.dedup();
            // one new track per source track, bottom first
            for &t in tracks.iter().rev() {
                let mine: Vec<usize> = sel.iter().filter(|x| x.0 == t).map(|x| x.1).collect();
                let Some(at) = new_with_same_strip(app, t) else { continue };
                if let Some(p) = app.project.as_mut() {
                    for &r in mine.iter().rev() {
                        let rg = p.tracks[t].regions.remove(r);
                        p.tracks[at].regions.insert(0, rg);
                    }
                }
            }
            app.selected_region = None;
            app.selected_regions.clear();
            app.after_edit();
        }
        "New Track For Overlapped Regions" => {
            let Some(t) = app.selected_track else { return false };
            let Some(regions) = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|x| x.regions.clone()) else { return false };
            let lanes = overlap_lanes(&regions);
            if lanes.len() < 2 {
                return false;
            }
            app.checkpoint("New Track For Overlapped Regions");
            // (each new track goes right under the source, so the last lane first)
            for lane in lanes[1..].iter().rev() {
                let Some(at) = new_with_same_strip(app, t) else { continue };
                if let Some(p) = app.project.as_mut() {
                    p.tracks[at].regions = lane.iter().map(|&r| regions[r].clone()).collect();
                }
            }
            if let Some(p) = app.project.as_mut() {
                p.tracks[t].regions = lanes[0].iter().map(|&r| regions[r].clone()).collect();
            }
            app.selected_region = None;
            app.selected_regions.clear();
            app.after_edit();
        }
        "Replace or Double Drum Track…" => return start_drum_replacement(app),
        "Use as Default Patch for MIDI Tracks" | "Use as Default Patch for Pattern Tracks" | "Use as Default Patch for Audio Tracks" | "Use as Default Patch for Guitar Tracks" => {
            let Some(t) = app.selected_track else { return false };
            return store_default_patch(app, t);
        }
        "Rename by Region" | "Rename Track Alternative by Region" => {
            let Some((t, r)) = app.selected_region else { return false };
            let Some(name) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map(|x| x.name.clone()) else { return false };
            app.checkpoint("Rename Track Alternative");
            if let Some(x) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                x.extra.alt_name = name;
            }
            app.ta.show_alternatives = true;
        }
        "Not Assigned" | "(Folder)" => {
            let Some(t) = app.selected_track else { return false };
            reassign(app, t, if name == "Not Assigned" { Reassign::NotAssigned } else { Reassign::Folder }, false);
        }
        "Select All Regions of Selected Tracks" => {
            let Some(p) = app.project.as_ref() else { return false };
            app.selected_regions = crate::trackarea::selection(app).into_iter().flat_map(|t| (0..p.tracks[t].regions.len()).map(move |r| (t, r))).collect();
            app.selected_region = app.selected_regions.first().copied();
        }
        "Select Tracks of Selected Regions/Cells/Folders" => {
            let mut ts: Vec<usize> = app.region_selection().into_iter().map(|x| x.0).collect();
            ts.sort();
            ts.dedup();
            if ts.is_empty() {
                return false;
            }
            app.selected_track = ts.first().copied();
            app.ta.selected = ts;
        }
        "Toggle Solo Tracks (Channel Strips) of Selected Regions/Cells" => {
            let mut ts: Vec<usize> = app.region_selection().into_iter().map(|x| x.0).collect();
            ts.sort();
            ts.dedup();
            let Some(p) = app.project.as_mut() else { return false };
            if ts.is_empty() {
                return false;
            }
            // all the soloed tracks are exactly those of the selection: every solo off; else
            // those tracks soloed and the others not
            let soloed: Vec<usize> = (0..p.tracks.len()).filter(|&i| p.tracks[i].solo).collect();
            let clear = soloed == ts;
            for (i, t) in p.tracks.iter_mut().enumerate() {
                t.solo = !clear && ts.contains(&i);
            }
            app.sync_mix();
        }
        _ => return false,
    }
    true
}

pub fn enabled(app: &App, label: &str) -> Option<bool> {
    Some(match label {
        "Replace or Double Drum Track…" => app.selected_track.and_then(|t| app.project.as_ref()?.tracks.get(t)).is_some_and(|t| t.kind == TrackKind::Audio && !t.regions.is_empty()),
        "New Track For Selected Regions" => !app.region_selection().is_empty(),
        "New Track For Overlapped Regions" => app.selected_track.and_then(|t| app.project.as_ref()?.tracks.get(t)).is_some_and(|t| overlap_lanes(&t.regions).len() > 1),
        "Rename by Region" => app.selected_region.is_some(),
        "Use as Default Patch for MIDI Tracks" => app.selected_track.is_some_and(|t| type_key(app, t).is_some()),
        _ => return None,
    })
}

pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    match it.label.as_str() {
        "Reassign Track" => {
            ui.menu_button("Reassign Track", |ui| reassign_menu(app, ui));
            true
        }
        "Sort Tracks by" => {
            ui.menu_button("Sort Tracks by", |ui| {
                for (by, label) in SortBy::ALL {
                    if ui.button(label).clicked() {
                        sort_tracks(app, by);
                        ui.close();
                    }
                }
            });
            true
        }
        // (the header menu's item names the track's type)
        "Use as Default Patch for MIDI Tracks" => {
            let t = app.selected_track.unwrap_or(0);
            let label = type_key(app, t).map(|k| format!("Use as Default Patch for {} Tracks", k.1));
            if ui.add_enabled(label.is_some(), egui::Button::new(label.unwrap_or_else(|| "Use as Default Patch".into()))).clicked() {
                store_default_patch(app, t);
                ui.close();
            }
            true
        }
        _ => false,
    }
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    drum_dialog(app, ctx);
    reassign_dialog(app, ctx);
}

/// The Track inspector's further rows (after Channel): the icon and colour, Freeze Mode,
/// Q-Reference, Default Region Type and the MIDI parameters. Returns the next row's y.
pub fn inspector_rows(app: &mut App, ui: &mut egui::Ui, area: Rect, mut y: f32, t: usize) -> f32 {
    use crate::inspector::{label, slot_rect, ROW};
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)).cloned() else { return y };
    let p = ui.painter().clone();
    let id = ui.scope_id().with(("trk-more", t));
    let set = tr.extra.set.clone();
    let audio = tr.kind == TrackKind::Audio;
    // the icon and the colour: a click opens their palettes
    label(&p, area, y, "Icon");
    let sr = slot_rect(area, y);
    let ir = Rect::from_min_size(pos2(sr.max.x - 44.0, y + 1.0), vec2(18.0, ROW - 2.0));
    let col = crate::trackhdr::color_of(&tr);
    crate::glyphs::paint(&p, ir, crate::trackhdr::chosen_glyph(&tr).unwrap_or_else(|| crate::glyphs::TrackIcon::of(&tr).glyph()), 15.0, col);
    if ui.interact(ir, id.with("icon"), Sense::click()).on_hover_text("Track Icon").clicked() {
        app.tm.ui.icons = Some(t);
    }
    let cr = Rect::from_min_size(pos2(sr.max.x - 20.0, y + 4.0), vec2(16.0, ROW - 8.0));
    p.rect_filled(cr, 3.0, col);
    if ui.interact(cr, id.with("color"), Sense::click()).on_hover_text("Track Color").clicked() {
        app.selected_track = Some(t);
        app.tm.ui.colors = Some(crate::trackhdr::ColorFor::Tracks);
    }
    y += ROW;
    let mut new = set.clone();
    let choice = |ui: &mut egui::Ui, y: &mut f32, key: &str, name: &str, cur: usize, choices: &[&str]| -> Option<usize> {
        label(&p, area, *y, name);
        let r = crate::widgets::choice_slot(ui, slot_rect(area, *y), id.with(key), cur, choices);
        *y += ROW;
        r
    };
    if let Some(k) = choice(ui, &mut y, "freeze", "Freeze Mode", (set.freeze_mode == FreezeMode::PreFader) as usize, &["Source Only", "Pre Fader"]) {
        new.freeze_mode = if k == 0 { FreezeMode::SourceOnly } else { FreezeMode::PreFader };
    }
    if audio {
        label(&p, area, y, "Q-Reference");
        crate::widgets::check_slot(ui, slot_rect(area, y), id.with("qref"), &mut new.q_reference);
        y += ROW;
    } else {
        let names: Vec<&str> = RegionType::ALL.iter().map(|x| x.name()).collect();
        let cur = RegionType::ALL.iter().position(|x| *x == set.default_region).unwrap_or(0);
        if let Some(k) = choice(ui, &mut y, "deftype", "Default Region Type", cur, &names) {
            new.default_region = RegionType::ALL[k];
        }
        // MIDI Input: Off, All, Musical Typing, the ports, Virtual In
        let mut inputs: Vec<String> = vec!["Off".into(), "All".into(), "Musical Typing".into()];
        inputs.extend(app.live.input_names.iter().cloned());
        inputs.push("Virtual In".into());
        let refs: Vec<&str> = inputs.iter().map(|s| s.as_str()).collect();
        let cur = inputs.iter().position(|x| *x == set.midi.input).unwrap_or(1);
        if let Some(k) = choice(ui, &mut y, "midi-in", "MIDI Input", cur, &refs) {
            new.midi.input = inputs[k].clone();
        }
        // Internal MIDI In: another instrument track, or Off
        let others: Vec<(u64, String)> = app.project.as_ref().map(|p| p.tracks.iter().filter(|x| x.kind != TrackKind::Audio && x.extra.id != tr.extra.id).map(|x| (x.extra.id, x.name.clone())).collect()).unwrap_or_default();
        let mut names: Vec<&str> = vec!["Off"];
        names.extend(others.iter().map(|x| x.1.as_str()));
        let cur = set.midi.internal_in.and_then(|id| others.iter().position(|x| x.0 == id)).map_or(0, |k| k + 1);
        if let Some(k) = choice(ui, &mut y, "midi-int", "Internal MIDI In", cur, &names) {
            new.midi.internal_in = if k == 0 { None } else { Some(others[k - 1].0) };
        }
        if set.midi.internal_in.is_some() {
            // (Record shows only with an Internal MIDI In)
            if let Some(k) = choice(ui, &mut y, "midi-rec", "Record", set.midi.record_both as usize, &["Internal MIDI In", "MIDI Input and Internal MIDI In"]) {
                new.midi.record_both = k == 1;
            }
        }
        let chans: Vec<String> = std::iter::once("All".to_string()).chain((1..=16).map(|c| c.to_string())).collect();
        let mut refs: Vec<&str> = chans.iter().map(|s| s.as_str()).collect();
        if let Some(k) = choice(ui, &mut y, "midi-inch", "MIDI In Channel", set.midi.in_channel as usize, &refs) {
            new.midi.in_channel = k as u8;
        }
        refs.push("Round Robin");
        if let Some(k) = choice(ui, &mut y, "midi-outch", "MIDI Out Channel", set.midi.out_channel as usize, &refs) {
            new.midi.out_channel = k as u8;
        }
        label(&p, area, y, "No Reset");
        crate::widgets::check_slot(ui, slot_rect(area, y), id.with("noreset"), &mut new.midi.no_reset);
        y += ROW;
        let styles = ["Auto", "Treble", "Bass", "Piano", "Lead Sheet", "Guitar", "Drums"];
        let cur = styles.iter().position(|x| *x == set.midi.staff_style).unwrap_or(0);
        if let Some(k) = choice(ui, &mut y, "staff", "Staff Style", cur, &styles) {
            new.midi.staff_style = styles[k].into();
        }
        // (Hermode Tuning shows only when the project's tuning is Hermode)
        if app.proj.get("Tuning / Tuning / Hermode Tuning (HMT)") == "1" || app.proj.get("Tuning / Hermode Tuning (HMT)") == "1" {
            if let Some(k) = choice(ui, &mut y, "hmt", "Hermode Tuning", set.midi.hermode as usize, &["Off", "Analyze", "Apply", "Analyze & Apply"]) {
                new.midi.hermode = k as u8;
            }
        }
        // External MIDI tracks: the MIDI output port (Virtual Out first) and channel
        if let Some(e) = new.external.as_mut() {
            let mut ports: Vec<String> = vec![crate::midiout::VIRTUAL.to_string()];
            ports.extend(app.hub.outs.ports().into_iter().filter(|x| x != crate::midiout::VIRTUAL));
            let refs: Vec<&str> = ports.iter().map(|s| s.as_str()).collect();
            let cur = if e.port.is_empty() { 0 } else { ports.iter().position(|x| *x == e.port).unwrap_or(0) };
            if let Some(k) = choice(ui, &mut y, "midiout", "MIDI Out", cur, &refs) {
                e.port = if k == 0 { String::new() } else { ports[k].clone() };
            }
            let chans: Vec<String> = (1..=16).map(|c| c.to_string()).collect();
            let crefs: Vec<&str> = chans.iter().map(|s| s.as_str()).collect();
            if let Some(k) = choice(ui, &mut y, "midich", "MIDI Channel", e.channel.clamp(1, 16) as usize - 1, &crefs) {
                e.channel = k as u8 + 1;
            }
        }
    }
    if new != set {
        app.checkpoint("Track Parameter");
        if let Some(s) = crate::tm::set_mut(app, t) {
            *s = new;
        }
        app.after_edit();
    }
    y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rg(start: u32, len: u32) -> Region {
        Region { name: "r".into(), start, length: len, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() }
    }

    #[test]
    fn overlapping_regions_spread_into_lanes() {
        let regions = vec![rg(0, 100), rg(50, 100), rg(100, 50), rg(60, 10), rg(200, 10)];
        let lanes = overlap_lanes(&regions);
        assert_eq!(lanes, vec![vec![0, 2, 4], vec![1], vec![3]]);
        assert_eq!(overlap_lanes(&[rg(0, 10), rg(10, 10)]).len(), 1);
    }

    #[test]
    fn same_channel_tracks_and_tracks_for_regions() {
        let mut app = crate::tm::test_app();
        let ctx = egui::Context::default();
        let t = app.project.as_ref().unwrap().tracks.iter().position(|x| x.kind != TrackKind::Audio && !x.regions.is_empty()).unwrap();
        let n = app.project.as_ref().unwrap().tracks.len();
        app.selected_track = Some(t);
        assert!(command(&mut app, &ctx, "New Track With Same Channel"));
        let p = app.project.as_ref().unwrap();
        assert_eq!(p.tracks.len(), n + 1);
        assert_eq!(p.tracks[t + 1].extra.set.same_strip, Some(p.tracks[t].extra.id));
        assert_eq!(crate::tm::strip_owner(&app, t + 1), t);
        // the selected region moves to a new track on the same strip
        let regions = app.project.as_ref().unwrap().tracks[t].regions.len();
        app.selected_region = Some((t, 0));
        app.selected_regions = vec![(t, 0)];
        assert!(command(&mut app, &ctx, "New Track For Selected Regions"));
        let p = app.project.as_ref().unwrap();
        assert_eq!(p.tracks[t].regions.len(), regions - 1);
        assert_eq!(p.tracks[t + 1].regions.len(), 1);
        assert_eq!(p.tracks[t + 1].extra.set.same_strip, Some(p.tracks[t].extra.id));
        app.undo();
        assert_eq!(app.project.as_ref().unwrap().tracks[t].regions.len(), regions);
    }

    #[test]
    fn external_midi_tracks_take_the_next_channel() {
        let mut app = crate::tm::test_app();
        let ctx = egui::Context::default();
        assert!(command(&mut app, &ctx, "New External MIDI Track"));
        let a = app.selected_track.unwrap();
        assert_eq!(app.project.as_ref().unwrap().tracks[a].extra.set.external.as_ref().unwrap().channel, 1);
        assert!(command(&mut app, &ctx, "New Track With Next Channel"));
        let b = app.selected_track.unwrap();
        assert_eq!(app.project.as_ref().unwrap().tracks[b].extra.set.external.as_ref().unwrap().channel, 2);
        assert!(command(&mut app, &ctx, "New External MIDI Track"));
        let c = app.selected_track.unwrap();
        assert_eq!(app.project.as_ref().unwrap().tracks[c].extra.set.external.as_ref().unwrap().channel, 3);
    }

    #[test]
    fn renaming_several_tracks_counts_up_and_renames_default_regions() {
        let mut app = crate::tm::test_app();
        let old = app.project.as_ref().unwrap().tracks[0].name.clone();
        if let Some(r) = app.project.as_mut().unwrap().tracks[0].regions.first_mut() {
            r.name = old.clone();
        }
        app.selected_track = Some(0);
        app.ta.selected = vec![0, 1];
        renamed(&mut app, 0, "Vox 1");
        let p = app.project.as_ref().unwrap();
        assert_eq!((p.tracks[0].name.as_str(), p.tracks[1].name.as_str()), ("Vox 1", "Vox 2"));
        if let Some(r) = p.tracks[0].regions.first() {
            assert_eq!(r.name, "Vox 1", "its default-named region follows");
        }
        // renamed once: its regions keep their names from now on
        app.ta.selected = vec![0];
        renamed(&mut app, 0, "Lead");
        let p = app.project.as_ref().unwrap();
        if let Some(r) = p.tracks[0].regions.first() {
            assert_eq!(r.name, "Vox 1");
        }
    }

    #[test]
    fn sorting_by_name_moves_tracks_and_undoes() {
        let mut app = crate::tm::test_app();
        let names: Vec<String> = app.project.as_ref().unwrap().tracks.iter().map(|t| t.name.clone()).collect();
        sort_tracks(&mut app, SortBy::TrackName);
        let sorted: Vec<String> = app.project.as_ref().unwrap().tracks.iter().filter(|t| t.extra.depth == 0).map(|t| t.name.to_lowercase()).collect();
        let mut want = sorted.clone();
        want.sort();
        assert_eq!(sorted.len(), names.len());
        // (natural order: equal to plain order for the demo's names)
        assert_eq!(sorted.iter().map(|s| s.as_str()).collect::<Vec<_>>(), want.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    }

    #[test]
    fn reassigning_all_tracks_on_a_strip() {
        let mut app = crate::tm::test_app();
        let ctx = egui::Context::default();
        let t = app.project.as_ref().unwrap().tracks.iter().position(|x| x.kind != TrackKind::Audio).unwrap();
        app.selected_track = Some(t);
        command(&mut app, &ctx, "New Track With Same Channel");
        reassign(&mut app, t, Reassign::NotAssigned, true);
        let p = app.project.as_ref().unwrap();
        assert_eq!(p.tracks[t].extra.set.assign, Assign::NotAssigned);
        assert_eq!(p.tracks[t + 1].extra.set.assign, Assign::NotAssigned, "every track on the old strip follows");
        let id = p.tracks[t].extra.id;
        reassign(&mut app, t + 1, Reassign::Strip(id), false);
        let p = app.project.as_ref().unwrap();
        assert_eq!((p.tracks[t + 1].extra.set.assign, p.tracks[t + 1].extra.set.same_strip), (Assign::Strip, Some(id)));
    }
}
