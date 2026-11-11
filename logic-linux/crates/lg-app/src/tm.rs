//! Tracks and mixing beyond the first draft, as Logic's guide describes them: the hub for
//! trackhdr.rs (header components, colours, icons, On/Off, Protect, Freeze, groove, search),
//! trackops.rs (Track ▸ Other, Reassign, Sort, drum replacement, alternatives), freeze.rs
//! (freeze files, Bounce in Place, exporting tracks and regions), mixext.rs (the Mixer's
//! channel mode, panners, solo modes, sends, peak displays, strip settings, I/O labels) and
//! mixengine.rs (the engine's side). State lives in the project (lg-core trackmix, so Undo
//! follows) and here; it is saved in Studio's sidecar next to the project's other extras.

use std::collections::HashMap;
use std::sync::Mutex;

use lg_core::trackmix::{StripSet, TrackSet};

use crate::app::App;
use crate::audio::PlayTrack;
use crate::mixer::AUX;

#[derive(Default)]
pub struct State {
    pub hdr: crate::trackhdr::Cfg,
    pub ui: crate::trackhdr::Ui,
    pub freeze: crate::freeze::State,
    /// Aux, VCA and output strips' settings by strip id (tracks keep theirs in the project).
    pub strips: HashMap<usize, StripSet>,
    pub mx: crate::mixext::State,
    pub ops: crate::trackops::State,
    pub nt: crate::newtrack::Details,
    /// Protected and frozen tracks' regions as they were locked: (signature, regions).
    guard: HashMap<u64, (u64, Vec<lg_core::Region>)>,
    /// External MIDI tracks' notes for the transport: (port, channel, notes).
    ext_notes: Mutex<Vec<(String, u8, Vec<(u32, u32, u8, u8)>)>>,
    ext_pos: f64,
    ext_on: Vec<(String, u8, u8, u32)>,
}

pub fn set(app: &App, t: usize) -> Option<&TrackSet> {
    app.project.as_ref()?.tracks.get(t).map(|x| &x.extra.set)
}

pub fn set_mut(app: &mut App, t: usize) -> Option<&mut TrackSet> {
    app.dirty = true;
    app.project.as_mut()?.tracks.get_mut(t).map(|x| &mut x.extra.set)
}

/// A strip's settings (a track's, or an aux / VCA / output strip's).
pub fn strip(app: &App, s: usize) -> StripSet {
    if s < AUX { set(app, s).map(|x| x.strip.clone()).unwrap_or_default() } else { app.tm.strips.get(&s).cloned().unwrap_or_default() }
}

pub fn edit_strip(app: &mut App, s: usize, f: impl FnOnce(&mut StripSet)) {
    app.dirty = true;
    if s < AUX {
        if let Some(x) = set_mut(app, s) {
            f(&mut x.strip);
        }
    } else {
        f(app.tm.strips.entry(s).or_default());
    }
    crate::mixengine::sync(app);
}

/// The track with this id.
pub fn by_id(app: &App, id: u64) -> Option<usize> {
    app.project.as_ref()?.tracks.iter().position(|t| t.extra.id == id)
}

/// The track whose channel strip track `t` plays through (itself unless it shares another's).
pub fn strip_owner(app: &App, t: usize) -> usize {
    set(app, t).and_then(|s| s.same_strip).and_then(|id| by_id(app, id)).unwrap_or(t)
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    crate::trackhdr::command(app, ctx, name) || crate::trackops::command(app, ctx, name) || crate::freeze::command(app, ctx, name) || crate::mixext::command(app, ctx, name) || crate::newtrack::command(app, ctx, name)
}

pub fn checked(app: &App, label: &str) -> Option<bool> {
    crate::trackhdr::checked(app, label).or_else(|| crate::mixext::checked(app, label))
}

pub fn enabled(app: &App, label: &str) -> Option<bool> {
    crate::trackops::enabled(app, label).or_else(|| crate::mixext::enabled(app, label))
}

/// The menus this area draws itself (submenus whose items share names with others).
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    crate::trackhdr::menu_ui(app, ui, it) || crate::trackops::menu_ui(app, ui, it)
}

fn signature(regions: &[lg_core::Region]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut add = |x: u64| {
        h ^= x;
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    add(regions.len() as u64);
    for r in regions {
        add(r.start as u64);
        add(r.length as u64);
        add(r.muted as u64);
        add(r.name.len() as u64);
        add(r.notes.len() as u64);
        for n in &r.notes {
            add(((n.pos as u64) << 24) ^ ((n.len as u64) << 8) ^ ((n.pitch as u64) << 1) ^ (n.velocity as u64) ^ ((n.muted as u64) << 40));
        }
    }
    h
}

/// Every frame: locked tracks keep their content, freezes render, external MIDI tracks play.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    // protected and frozen tracks: an edit that got through is taken back, with Logic's alert
    let mut restore: Vec<(usize, Vec<lg_core::Region>, bool)> = Vec::new();
    if let Some(p) = app.project.as_ref() {
        let mut seen = Vec::new();
        for (i, t) in p.tracks.iter().enumerate() {
            let s = &t.extra.set;
            if !(s.protected || s.frozen) {
                continue;
            }
            seen.push(t.extra.id);
            let sig = signature(&t.regions);
            match app.tm.guard.get(&t.extra.id) {
                Some((old, regions)) if *old != sig => restore.push((i, regions.clone(), s.frozen)),
                Some(_) => {}
                None => {
                    app.tm.guard.insert(t.extra.id, (sig, t.regions.clone()));
                }
            }
        }
        app.tm.guard.retain(|id, _| seen.contains(id));
    } else {
        app.tm.guard.clear();
    }
    if !restore.is_empty() && !app.recording() {
        let frozen = restore.iter().any(|r| r.2);
        for (i, regions, _) in restore {
            if let Some(t) = app.project.as_mut().and_then(|p| p.tracks.get_mut(i)) {
                t.regions = regions;
            }
        }
        app.selected_region = None;
        app.selected_regions.clear();
        app.after_edit();
        app.tm.ui.alert = Some(if frozen { "The track is frozen.\nUnfreeze it with its Freeze button to change its content.".into() } else { "The track is locked.\nTurn off its Protect button to change it.".into() });
    }
    crate::trackhdr::frame(app, ctx);
    crate::freeze::frame(app, ctx);
    crate::mixext::frame(app, ctx);
    external_midi(app);
}

/// External MIDI tracks: their notes go to their MIDI destination as the playhead passes.
fn external_midi(app: &mut App) {
    let playing = app.playing();
    let pos = app.position();
    let notes = app.tm.ext_notes.lock().map(|g| g.clone()).unwrap_or_default();
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    if !playing || pos < app.tm.ext_pos {
        for (port, ch, pitch, _) in app.tm.ext_on.drain(..) {
            out.push((port, vec![0x80 | (ch & 15), pitch, 0]));
        }
    }
    if playing {
        let from = app.tm.ext_pos.min(pos);
        for (port, ch, list) in &notes {
            let a = list.partition_point(|n| (n.0 as f64) < from);
            for n in list[a..].iter().take_while(|n| (n.0 as f64) < pos) {
                out.push((port.clone(), vec![0x90 | (ch & 15), n.2, n.3]));
                app.tm.ext_on.push((port.clone(), *ch, n.2, n.1));
            }
        }
        let mut still = Vec::new();
        for (port, ch, pitch, end) in app.tm.ext_on.drain(..) {
            if (end as f64) <= pos {
                out.push((port, vec![0x80 | (ch & 15), pitch, 0]));
            } else {
                still.push((port, ch, pitch, end));
            }
        }
        app.tm.ext_on = still;
    }
    app.tm.ext_pos = pos;
    for (port, bytes) in out {
        crate::trackops::midi_send(app, &port, &bytes);
    }
}

/// The engine's tracks after the stacks: tracks sharing a channel strip play through its
/// owner, tracks not assigned send nothing, external MIDI tracks leave the engine (their notes
/// go out by MIDI), tracks matching the groove track take its timing.
pub fn engine_tracks(app: &App, tracks: &mut [PlayTrack]) {
    let Some(p) = app.project.as_ref() else { return };
    let groove: Option<Vec<u32>> = p.tracks.iter().position(|t| t.extra.set.groove).and_then(|g| tracks.get(g)).map(|t| {
        let mut o: Vec<u32> = t.notes.iter().map(|n| n.0).collect();
        o.dedup();
        o
    });
    let mut ext = Vec::new();
    for i in 0..tracks.len().min(p.tracks.len()) {
        let s = &p.tracks[i].extra.set;
        if let (true, Some(on)) = (s.match_groove && !s.groove, groove.as_ref()) {
            // (within a sixteenth: farther notes aren't the groove's)
            lg_core::trackmix::match_groove(&mut tracks[i].notes, on, lg_core::events::PPQ / 8);
        }
        if s.assign != lg_core::trackmix::Assign::Strip {
            tracks[i].notes.clear();
            continue;
        }
        if let Some(e) = &s.external {
            let notes = std::mem::take(&mut tracks[i].notes);
            if !p.tracks[i].mute && !s.off {
                ext.push((e.port.clone(), e.channel.saturating_sub(1), notes));
            }
            continue;
        }
        if let Some(o) = s.same_strip.and_then(|id| p.tracks.iter().position(|t| t.extra.id == id)).filter(|o| *o != i && *o < tracks.len()) {
            let notes = std::mem::take(&mut tracks[i].notes);
            if !p.tracks[i].mute && !s.off {
                tracks[o].notes.extend(notes);
                tracks[o].notes.sort_by_key(|n| n.0);
            }
            tracks[i].mute = true;
        }
    }
    if let Ok(mut g) = app.tm.ext_notes.lock() {
        *g = ext;
    }
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    crate::trackhdr::windows(app, ctx);
    crate::trackops::windows(app, ctx);
    crate::freeze::windows(app, ctx);
    crate::mixext::windows(app, ctx);
}

// ---------------------------------------------------------------------------------------------
// the sidecar

fn path(app: &App) -> Option<std::path::PathBuf> {
    let side = crate::hosting::sidecar_path(&app.file.as_ref()?.0)?;
    Some(side.parent()?.parent()?.join("tracks-mix").join(side.file_name()?))
}

pub fn sidecar_text(app: &App) -> String {
    let mut out = String::from("# Bramble Studio: track header and channel strip settings\n");
    let Some(p) = app.project.as_ref() else { return out };
    for (i, t) in p.tracks.iter().enumerate() {
        let line = t.extra.set.to_line();
        if !line.is_empty() {
            out += &format!("track\t{i}\t{line}\n");
        }
    }
    let mut ids: Vec<&usize> = app.tm.strips.keys().collect();
    ids.sort();
    for s in ids {
        let pairs = app.tm.strips[s].to_pairs();
        if !pairs.is_empty() {
            let t = TrackSet { strip: app.tm.strips[s].clone(), ..Default::default() };
            out += &format!("strip\t{s}\t{}\n", t.to_line());
        }
    }
    let cfg = app.tm.hdr.to_text();
    if cfg != crate::trackhdr::Cfg::default().to_text() {
        out += &format!("header\t{cfg}\n");
    }
    out += &crate::mixext::sidecar_lines(app);
    out
}

pub fn apply_sidecar(app: &mut App, text: &str) -> bool {
    let mut any = false;
    for l in text.lines() {
        let mut it = l.splitn(3, '\t');
        match (it.next(), it.next(), it.next()) {
            (Some("track"), Some(i), Some(rest)) => {
                if let (Ok(i), Some(p)) = (i.parse::<usize>(), app.project.as_mut()) {
                    if let Some(t) = p.tracks.get_mut(i) {
                        t.extra.set = TrackSet::from_line(rest);
                        any = true;
                    }
                }
            }
            (Some("strip"), Some(s), Some(rest)) => {
                if let Ok(s) = s.parse::<usize>() {
                    app.tm.strips.insert(s, TrackSet::from_line(rest).strip);
                    any = true;
                }
            }
            (Some("header"), Some(a), b) => {
                let joined = match b { Some(b) => format!("{a}\t{b}"), None => a.to_string() };
                app.tm.hdr = crate::trackhdr::Cfg::from_text(&joined);
                any = true;
            }
            (Some(k), Some(a), b) => any |= crate::mixext::apply_sidecar_line(app, k, a, b.unwrap_or("")),
            _ => {}
        }
    }
    any
}

/// On Save: next to Studio's other extras (removed when there is nothing to keep).
pub fn save_sidecar(app: &App) {
    let Some(path) = path(app) else { return };
    let text = sidecar_text(app);
    if text.lines().all(|l| l.starts_with('#')) {
        let _ = std::fs::remove_file(&path);
        return;
    }
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(path, text);
}

pub fn after_open(app: &mut App) {
    app.tm.strips.clear();
    app.tm.guard.clear();
    app.tm.freeze.files.clear();
    app.tm.hdr = crate::trackhdr::Cfg::user_default();
    let Some(path) = path(app) else { return };
    let Ok(text) = std::fs::read_to_string(path) else { return };
    if apply_sidecar(app, &text) {
        crate::freeze::after_open(app);
        app.load_engine();
    }
}

/// An app on the demo song, for tests.
#[cfg(test)]
pub fn test_app() -> App {
    let ctx = egui::Context::default();
    let mut a = App::new(&eframe::CreationContext::_new_kittest(ctx), None);
    a.set_project(crate::demo::song(), true);
    a.after_edit();
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use lg_core::trackmix::{Assign, External};

    fn app3() -> App {
        let mut app = test_app();
        let p = app.project.as_mut().unwrap();
        p.tracks.truncate(0);
        for k in 0..3 {
            let mut t = lg_core::Track::new(&format!("T{k}"), lg_core::TrackKind::Instrument);
            t.extra.id = 10 + k as u64;
            let mut r = lg_core::Region { name: "r".into(), start: 0, length: 3840, notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
            r.notes.push(lg_core::events::Note { channel: 0, pitch: 60 + k as u8, velocity: 100, pos: 100 * k as u32, len: 200, muted: false });
            t.regions.push(r);
            p.tracks.push(t);
        }
        app.programs = vec![0; 3];
        app.after_edit();
        app
    }

    #[test]
    fn tracks_sharing_a_strip_play_through_its_owner() {
        let mut app = app3();
        set_mut(&mut app, 1).unwrap().same_strip = Some(10);
        app.load_engine();
        let e = app.engine.lock().unwrap();
        assert_eq!(e.tracks[0].notes.len(), 2, "the owner plays both tracks' notes");
        assert!(e.tracks[1].notes.is_empty());
        drop(e);
        assert_eq!(strip_owner(&app, 1), 0);
        assert_eq!(strip_owner(&app, 2), 2);
    }

    #[test]
    fn unassigned_and_external_tracks_leave_the_engine() {
        let mut app = app3();
        set_mut(&mut app, 0).unwrap().assign = Assign::NotAssigned;
        set_mut(&mut app, 2).unwrap().external = Some(External { port: "Synth".into(), channel: 3, ..Default::default() });
        app.load_engine();
        let e = app.engine.lock().unwrap();
        assert!(e.tracks[0].notes.is_empty() && e.tracks[2].notes.is_empty());
        assert!(e.ext.tracks[0].off);
        drop(e);
        let ext = app.tm.ext_notes.lock().unwrap();
        assert_eq!(ext.len(), 1);
        assert_eq!((ext[0].0.as_str(), ext[0].1, ext[0].2.len()), ("Synth", 2, 1));
    }

    #[test]
    fn the_groove_track_sets_the_others_timing() {
        let mut app = app3();
        set_mut(&mut app, 2).unwrap().groove = true;
        set_mut(&mut app, 1).unwrap().match_groove = true;
        app.load_engine();
        let e = app.engine.lock().unwrap();
        assert_eq!(e.tracks[1].notes[0].0, 200, "track 2's note at 100 moved onto the groove track's at 200");
        assert_eq!(e.tracks[0].notes[0].0, 0, "tracks not matching keep theirs");
    }

    #[test]
    fn protected_tracks_take_edits_back() {
        let mut app = app3();
        let ctx = egui::Context::default();
        set_mut(&mut app, 1).unwrap().protected = true;
        frame(&mut app, &ctx);
        app.project.as_mut().unwrap().tracks[1].regions[0].start = 960;
        app.project.as_mut().unwrap().tracks[0].regions[0].start = 960;
        frame(&mut app, &ctx);
        let p = app.project.as_ref().unwrap();
        assert_eq!(p.tracks[1].regions[0].start, 0, "the protected track's region is back");
        assert_eq!(p.tracks[0].regions[0].start, 960);
        assert!(app.tm.ui.alert.as_deref().is_some_and(|a| a.contains("locked")));
        // unprotected: edits stay
        set_mut(&mut app, 1).unwrap().protected = false;
        frame(&mut app, &ctx);
        app.project.as_mut().unwrap().tracks[1].regions[0].start = 480;
        frame(&mut app, &ctx);
        assert_eq!(app.project.as_ref().unwrap().tracks[1].regions[0].start, 480);
    }

    #[test]
    fn sidecar_round_trip() {
        let mut app = app3();
        set_mut(&mut app, 0).unwrap().protected = true;
        set_mut(&mut app, 2).unwrap().strip.solo_safe = true;
        app.tm.strips.entry(AUX).or_default().format = lg_core::trackmix::InputFormat::Mono;
        app.tm.hdr.color_bars = true;
        let text = sidecar_text(&app);
        let mut b = app3();
        assert!(apply_sidecar(&mut b, &text));
        assert!(set(&b, 0).unwrap().protected && set(&b, 2).unwrap().strip.solo_safe);
        assert_eq!(b.tm.strips[&AUX].format, lg_core::trackmix::InputFormat::Mono);
        assert!(b.tm.hdr.color_bars);
    }
}
