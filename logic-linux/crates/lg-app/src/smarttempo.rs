//! Smart Tempo (Logic Pro 13): each audio file's analysis (tempo, beats, downbeats, meter,
//! Constant or Variable), its edits and their undo, and what it does to the project: Apply
//! Region Tempo to Project Tempo, Apply Project Tempo to Region and Downbeat, Set Downbeat to
//! Playhead Position, Tap Tempo. The Smart Tempo Editor (the audio editor's third tab,
//! audioedit.rs) draws it.
//!
//! Analysis runs on a thread (Logic: about two seconds, "Analyzing…"); the result is kept by
//! file, as Logic keeps it in the file's LGBM chunk (not written here yet).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use lg_audio::tempo::BeatMap;

use crate::app::App;

const PPQ_F: f64 = lg_core::events::PPQ as f64;

/// A file's analysis: the beat map as edited, as first analysed, and its undo.
#[derive(Clone, Debug)]
pub struct Analysis {
    pub map: BeatMap,
    pub original: BeatMap,
    pub undo: Vec<BeatMap>,
    pub redo: Vec<BeatMap>,
}

pub struct State {
    pub maps: HashMap<PathBuf, Analysis>,
    pending: Vec<(PathBuf, mpsc::Receiver<Option<BeatMap>>)>,
    /// The beat marker selected (clicked), and a range of beats (Shift-drag, Select ▸).
    pub selected: Option<usize>,
    pub range: Option<(usize, usize)>,
    /// Edit menu switches (Logic's defaults: Apply Changes Automatically ✓, Snap to Transients ✓).
    pub apply_auto: bool,
    pub snap_transients: bool,
    pub maintain_time: bool,
    /// View menu switches.
    pub scroll_in_play: bool,
    pub show_curve: bool,
    pub show_transients: bool,
    /// The orange scope button (on: the selected region; off: the track).
    pub region_scope: bool,
    /// Listen (the speaker button) and the editor's own click (M).
    pub listen: bool,
    pub click: bool,
    /// Tap Tempo: the taps' times (seconds, the app's clock).
    pub taps: Vec<f64>,
    /// The view: file seconds at the left edge, seconds per point; the file it was fitted to.
    pub view_start: f64,
    pub sec_per_px: f64,
    pub fitted_for: Option<PathBuf>,
    /// Hint mode's taps (sthints.rs).
    pub hint: crate::sthints::State,
}

impl Default for State {
    fn default() -> Self {
        State {
            maps: HashMap::new(),
            pending: Vec::new(),
            selected: None,
            range: None,
            apply_auto: true,
            snap_transients: true,
            maintain_time: false,
            scroll_in_play: false,
            show_curve: false,
            show_transients: false,
            region_scope: true,
            listen: false,
            click: false,
            taps: Vec::new(),
            view_start: 0.0,
            sec_per_px: 0.02,
            fitted_for: None,
            hint: Default::default(),
        }
    }
}

/// Where an audio region is: its track and index, file, start (ticks and seconds), length
/// (seconds) and offset into the file (seconds).
#[derive(Clone, Debug)]
pub struct RegionTime {
    pub track: usize,
    pub region: usize,
    pub file: PathBuf,
    pub start_tick: u32,
    pub start_sec: f64,
    pub len_sec: f64,
    pub offset_sec: f64,
}

/// The tick at a time (seconds from bar 1), through the tempo changes.
pub fn tick_at(app: &App, sec: f64) -> f64 {
    let Some(p) = &app.project else { return sec * 120.0 / 60.0 * PPQ_F };
    tick_at_tempo(p.tempo, &p.tempo_map, sec)
}

/// Seconds from bar 1 to a tick, from a tempo and its changes.
pub fn secs_at_tempo(tempo: f64, tempo_map: &[(u32, f64)], tick: f64) -> f64 {
    let mut secs = 0.0;
    let mut at = 0.0;
    let mut bpm = tempo;
    for &(t, b) in tempo_map {
        let t = t as f64;
        if t >= tick {
            break;
        }
        if t > at {
            secs += (t - at) / PPQ_F * 60.0 / bpm;
            at = t;
        }
        bpm = b;
    }
    secs + (tick - at).max(0.0) / PPQ_F * 60.0 / bpm
}

/// The tick at a time from a tempo and its changes (ticks, BPM).
pub fn tick_at_tempo(tempo: f64, tempo_map: &[(u32, f64)], sec: f64) -> f64 {
    let mut at_sec = 0.0;
    let mut at_tick = 0.0;
    let mut bpm = tempo;
    for &(t, b) in tempo_map {
        let t = t as f64;
        if t > at_tick {
            let s = at_sec + (t - at_tick) / PPQ_F * 60.0 / bpm;
            if s >= sec {
                break;
            }
            at_sec = s;
            at_tick = t;
        }
        bpm = b;
    }
    at_tick + (sec - at_sec) * bpm / 60.0 * PPQ_F
}

/// The selected audio region's times.
pub fn selected(app: &App) -> Option<RegionTime> {
    let (t, r) = app.selected_region?;
    region_time(app, t, r)
}

pub fn region_time(app: &App, t: usize, r: usize) -> Option<RegionTime> {
    let rg = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?;
    let a = rg.audio.as_ref()?;
    let start_sec = app.seconds_at(rg.start as f64);
    let len_sec = app.seconds_at((rg.start + rg.length) as f64) - start_sec;
    let extra = crate::audioedit::extras(app, t, r);
    let rate = crate::audioedit::rate_of(app, &a.file).unwrap_or(48_000) as f64;
    Some(RegionTime { track: t, region: r, file: a.file.clone(), start_tick: rg.start, start_sec, len_sec, offset_sec: extra.offset as f64 / rate })
}

pub fn analysing(app: &App, file: &Path) -> bool {
    app.audio_edit.st.pending.iter().any(|(p, _)| p == file)
}

/// Analyze Tempo: starts the analysis of a file (on a thread).
pub fn analyze(app: &mut App, file: &Path) {
    if analysing(app, file) {
        return;
    }
    let variable = app.audio_edit.st.maps.get(file).is_some_and(|a| a.map.variable);
    let (tx, rx) = mpsc::channel();
    let path = file.to_path_buf();
    std::thread::spawn(move || {
        let (mono, rate) = match lg_audio::AudioFile::read(&path) {
            Ok(a) => (a.mono(), a.rate),
            Err(_) => match crate::audio::load_audio(&path) {
                Some(s) => (s.chunks(2).map(|f| (f[0] + f.get(1).copied().unwrap_or(f[0])) / 2.0).collect(), crate::audio::RATE),
                None => (Vec::new(), 48_000),
            },
        };
        let _ = tx.send(lg_audio::tempo::analyze(&mono, rate, variable));
    });
    app.audio_edit.st.pending.push((file.to_path_buf(), rx));
}

/// Collects finished analyses (each frame).
pub fn poll(app: &mut App) {
    let mut done = Vec::new();
    app.audio_edit.st.pending.retain(|(p, rx)| match rx.try_recv() {
        Ok(m) => {
            done.push((p.clone(), m));
            false
        }
        Err(mpsc::TryRecvError::Empty) => true,
        Err(_) => false,
    });
    for (p, m) in done {
        match m {
            Some(map) => {
                app.audio_edit.st.maps.insert(p.clone(), Analysis { original: map.clone(), map, undo: Vec::new(), redo: Vec::new() });
                app.audio_edit.st.selected = None;
                app.audio_edit.st.range = None;
            }
            None => app.tell("Smart Tempo found no tempo in this recording"),
        }
    }
}

/// Changes a file's beat map (one undo step).
pub fn edit(app: &mut App, file: &Path, f: impl FnOnce(&mut BeatMap)) -> bool {
    let Some(a) = app.audio_edit.st.maps.get_mut(file) else { return false };
    let before = a.map.clone();
    f(&mut a.map);
    if a.map != before {
        a.undo.push(before);
        a.redo.clear();
    }
    crate::stmulti::propagate(app, file);
    true
}

/// The selected region's file's beat map.
pub fn current(app: &App) -> Option<(RegionTime, &Analysis)> {
    let rt = selected(app)?;
    let a = app.audio_edit.st.maps.get(&rt.file)?;
    Some((rt, a))
}

/// A file moment (seconds) on the project's timeline (ticks), through the region.
pub fn file_to_tick(app: &App, rt: &RegionTime, file_sec: f64) -> f64 {
    tick_at(app, rt.start_sec + file_sec - rt.offset_sec)
}

pub fn tick_to_file(app: &App, rt: &RegionTime, tick: f64) -> f64 {
    app.seconds_at(tick) - rt.start_sec + rt.offset_sec
}

/// Apply Region Tempo to Project Tempo: the project's tempo follows the region's beats (one
/// tempo when Constant, one per beat when Variable), its downbeat on the nearest bar line;
/// the region keeps playing in time with them.
pub fn apply_region_tempo(app: &mut App) -> bool {
    let Some((rt, a)) = current(app) else { return false };
    let map = a.map.clone();
    if map.beats.len() < 2 {
        return false;
    }
    let beat_t = lg_core::events::PPQ as f64 * 4.0 / map.beat_unit.max(1) as f64;
    let bar_t = beat_t * map.beats_per_bar.max(1) as f64;
    // the downbeat on the bar line nearest where it is now (a bar later when the region would
    // then start before the project)
    let d = map.downbeat.min(map.beats.len() - 1);
    let down_tick = file_to_tick(app, &rt, map.beats[d]);
    let mut bar_tick = (down_tick / bar_t).round().max(0.0) * bar_t;
    let down_in_region = map.beats[d] - rt.offset_sec;
    let first = map.beats.iter().position(|&b| b >= rt.offset_sec - 1e-6).unwrap_or(0);
    let last = map.beats.iter().rposition(|&b| b <= rt.offset_sec + rt.len_sec + 1e-6).unwrap_or(map.beats.len() - 1);
    let Some(old) = app.project.as_ref().map(|p| (p.tempo, p.tempo_map.clone())) else { return false };
    let mut plan = None;
    for _ in 0..8 {
        // the region's beats on the timeline (ticks), with their tempos
        let mut entries: Vec<(u32, f64)> = Vec::new();
        for k in first..last.max(first + 1).min(map.beats.len() - 1) {
            let tick = bar_tick + (k as f64 - d as f64) * beat_t;
            if tick < 0.0 {
                continue;
            }
            let bpm = 60.0 / (map.beats[k + 1] - map.beats[k]).max(1e-6) * (beat_t / PPQ_F);
            let bpm = (bpm * 10_000.0).round() / 10_000.0;
            if !map.variable && !entries.is_empty() {
                break;
            }
            if entries.last().is_none_or(|e| (e.1 - bpm).abs() > 1e-4) {
                entries.push((tick.round() as u32, bpm));
            }
        }
        if entries.is_empty() {
            return false;
        }
        let mut tm: Vec<(u32, f64)> = old.1.iter().copied().filter(|e| e.0 < entries[0].0).collect();
        // Constant (or nothing before it): one tempo from the start, Logic's result
        if tm.len() <= 1 {
            tm.clear();
            entries[0].0 = 0;
        }
        tm.extend(entries);
        tm.sort_by_key(|e| e.0);
        tm.dedup_by(|b, a| a.1 == b.1);
        if tm.first().is_none_or(|e| e.0 != 0) {
            tm.insert(0, (0, old.0));
        }
        let start_sec = secs_at_tempo(tm[0].1, &tm, bar_tick) - down_in_region;
        if start_sec >= -1e-6 {
            plan = Some((tm, start_sec));
            break;
        }
        bar_tick += bar_t;
    }
    let Some((tm, start_sec)) = plan else { return false };
    app.checkpoint("Apply Region Tempo to Project Tempo");
    let sig = (map.beats_per_bar, map.beat_unit);
    let Some(p) = app.project.as_mut() else { return false };
    p.tempo = tm[0].1;
    p.tempo_map = tm;
    p.signature = sig;
    // the region where its downbeat meets the bar line under the new tempos
    let new_start = tick_at(app, start_sec.max(0.0));
    let new_end = tick_at(app, start_sec.max(0.0) + rt.len_sec);
    if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(rt.track)).and_then(|t| t.regions.get_mut(rt.region)) {
        rg.start = new_start.round() as u32;
        rg.length = (new_end - new_start).round().max(1.0) as u32;
    }
    app.after_edit();
    app.tell(format!("Project tempo: {}", if map.variable { "the region's tempo curve".to_string() } else { format!("{:.2} BPM", app.project.as_ref().map_or(0.0, |p| p.tempo)) }));
    true
}

/// Apply Project Tempo to Region and Downbeat: the region's beats are the project's.
pub fn apply_project_tempo(app: &mut App) -> bool {
    let Some(rt) = selected(app) else { return false };
    let Some(p) = &app.project else { return false };
    let (sig, variable) = (p.signature, p.tempo_map.len() > 1);
    let beat_t = lg_core::events::PPQ as f64 * 4.0 / sig.1.max(1) as f64;
    let bar_t = beat_t * sig.0.max(1) as f64;
    let file_secs = file_seconds(app, &rt.file).unwrap_or(rt.offset_sec + rt.len_sec);
    // the project's beats through the file (the region's timeline extended to the file's ends)
    let t0 = file_to_tick(app, &rt, 0.0);
    let first_beat = (t0 / beat_t).ceil() * beat_t;
    let mut beats = Vec::new();
    let mut downbeat = None;
    let mut tick = first_beat;
    loop {
        let s = tick_to_file(app, &rt, tick);
        if s > file_secs {
            break;
        }
        if s >= 0.0 {
            if downbeat.is_none() && (tick / bar_t).fract().abs() < 1e-9 {
                downbeat = Some(beats.len());
            }
            beats.push(s);
        }
        tick += beat_t;
    }
    if beats.len() < 2 {
        return false;
    }
    let map = BeatMap { beats, downbeat: downbeat.unwrap_or(0), beats_per_bar: sig.0, beat_unit: sig.1, variable, hints: Default::default() };
    let file = rt.file.clone();
    if app.audio_edit.st.maps.contains_key(&file) {
        edit(app, &file, |m| *m = map);
    } else {
        app.audio_edit.st.maps.insert(file, Analysis { original: map.clone(), map, undo: Vec::new(), redo: Vec::new() });
    }
    true
}

fn file_seconds(app: &App, file: &Path) -> Option<f64> {
    crate::audioedit::loaded_seconds(app, file)
}

/// Set Downbeat to Playhead Position.
pub fn set_downbeat_to_playhead(app: &mut App) -> bool {
    let Some(rt) = selected(app) else { return false };
    let s = tick_to_file(app, &rt, app.position());
    let Some(i) = app.audio_edit.st.maps.get(&rt.file).and_then(|a| a.map.nearest_beat(s)) else { return false };
    edit(app, &rt.file, |m| m.set_downbeat(i))
}

/// Tap Tempo: each tap's time; from the second on, the beats' tempo follows the taps (and
/// with Downbeat, the tap nearest a beat marks the downbeat).
pub fn tap(app: &mut App, downbeat: bool) -> bool {
    let Some(rt) = selected(app) else { return false };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    let taps = &mut app.audio_edit.st.taps;
    if taps.last().is_some_and(|t| now - t > 2.5) {
        taps.clear();
    }
    taps.push(now);
    if downbeat {
        return set_downbeat_to_playhead(app);
    }
    let taps = app.audio_edit.st.taps.clone();
    if taps.len() < 2 {
        app.tell("Tap Tempo: keep tapping");
        return true;
    }
    let period = (taps[taps.len() - 1] - taps[0]) / (taps.len() - 1) as f64;
    let bpm = 60.0 / period.max(0.1);
    set_tempo(app, &rt.file, bpm);
    app.tell(format!("Tap Tempo: {bpm:.1} BPM"));
    true
}

/// The beats evenly at a tempo, the downbeat staying where it is (the TEMPO readout, Tap).
pub fn set_tempo(app: &mut App, file: &Path, bpm: f64) -> bool {
    let secs = file_seconds(app, file).unwrap_or(0.0);
    edit(app, file, |m| {
        let d = m.beats.get(m.downbeat).copied().unwrap_or(0.0);
        let n = BeatMap::constant(bpm.clamp(5.0, 990.0), d, secs.max(d), m.beats_per_bar);
        *m = BeatMap { variable: m.variable, beat_unit: m.beat_unit, hints: m.hints.clone(), ..n };
    })
}

/// The Smart Tempo Editor's commands (key commands, its Edit and View menus). False when the
/// name isn't one, or there's nothing to do it to.
pub fn command(app: &mut App, name: &str) -> bool {
    if crate::sthints::command(app, name) {
        return true;
    }
    let rt = selected(app);
    let file = rt.as_ref().map(|r| r.file.clone());
    let st = &mut app.audio_edit.st;
    match name {
        "Apply Changes Automatically" => st.apply_auto = !st.apply_auto,
        "Snap to Transients" | "Snap To Transients" => st.snap_transients = !st.snap_transients,
        "Maintain Time Position of All Regions" | "Maintain Time Positions of All Regions" => st.maintain_time = !st.maintain_time,
        "Show Tempo Curve Overlay" => st.show_curve = !st.show_curve,
        "Show Transients" => st.show_transients = !st.show_transients,
        "Toggle Metronome Click - Smart Tempo Editor" => {
            st.click = !st.click;
            crate::audioedit::toggle_click(app);
        }
        "Analyze Tempo" => {
            let Some(f) = file else { return false };
            analyze(app, &f);
        }
        "Apply Region Tempo to Project Tempo…" | "Apply Region Tempo to Project Tempo" => return apply_region_tempo(app),
        "Apply Project Tempo to Region and Downbeat" => return apply_project_tempo(app),
        "Set Downbeat to Playhead Position" => return set_downbeat_to_playhead(app),
        "Tap Tempo - Smart Tempo Editor" => return tap(app, false),
        "Tap Tempo (Downbeat)" => return tap(app, true),
        _ => {
            let Some(f) = file else { return false };
            if !app.audio_edit.st.maps.contains_key(&f) {
                return false;
            }
            return map_command(app, &f, name);
        }
    }
    true
}

/// Commands on an analysed file's beats.
fn map_command(app: &mut App, f: &Path, name: &str) -> bool {
    let n = app.audio_edit.st.maps.get(f).map_or(0, |a| a.map.beats.len());
    let range = app.audio_edit.st.range.or(app.audio_edit.st.selected.map(|s| (s, s))).unwrap_or((0, n.saturating_sub(1)));
    let (a, b) = (range.0.min(range.1), range.0.max(range.1));
    let rt = selected(app);
    let play_beat = rt.as_ref().and_then(|rt| {
        let s = tick_to_file(app, rt, app.position());
        app.audio_edit.st.maps.get(f).map(|x| x.map.beats_at(s))
    });
    match name {
        "Undo Edit Hint Marker" | "Undo" => {
            let Some(x) = app.audio_edit.st.maps.get_mut(f) else { return false };
            let Some(m) = x.undo.pop() else { return false };
            let cur = std::mem::replace(&mut x.map, m);
            x.redo.push(cur);
        }
        "Redo" => {
            let Some(x) = app.audio_edit.st.maps.get_mut(f) else { return false };
            let Some(m) = x.redo.pop() else { return false };
            let cur = std::mem::replace(&mut x.map, m);
            x.undo.push(cur);
        }
        "Half Tempo" => return edit(app, f, |m| m.halve()),
        "Double Tempo" => return edit(app, f, |m| m.double()),
        "Constant" => return edit(app, f, |m| m.make_constant()),
        "Variable" => return edit(app, f, |m| m.variable = true),
        "Set Average Tempo for Selection" => return edit(app, f, |m| m.even_out(a, b)),
        "Set Average Tempo within each Bar" => {
            return edit(app, f, |m| {
                let bars: Vec<usize> = (0..m.beats.len()).filter(|&i| m.bar_beat(i).1 == 1).collect();
                for w in bars.windows(2) {
                    m.even_out(w[0], w[1]);
                }
            });
        }
        "Extend First Tempo in Selection to Beginning" => {
            return edit(app, f, |m| {
                if a + 1 < m.beats.len() {
                    let p = m.beats[a + 1] - m.beats[a];
                    for k in (0..a).rev() {
                        m.beats[k] = m.beats[k + 1] - p;
                    }
                    m.beats.retain(|&x| x >= -1e-9);
                }
            });
        }
        "Extend Last Tempo in Selection to End" => {
            return edit(app, f, |m| {
                if b >= 1 && b < m.beats.len() {
                    let p = m.beats[b] - m.beats[b - 1];
                    for k in b + 1..m.beats.len() {
                        m.beats[k] = m.beats[k - 1] + p;
                    }
                }
            });
        }
        "Delete Hints" | "Clear Hints" | "Revert Changes" | "Revert to Original Recording Tempo" => {
            let Some(x) = app.audio_edit.st.maps.get(f) else { return false };
            let orig = x.original.clone();
            return edit(app, f, |m| *m = orig);
        }
        "Remove Tempo Information" => {
            app.audio_edit.st.maps.remove(f);
            app.audio_edit.st.selected = None;
            app.audio_edit.st.range = None;
        }
        "Convert all Beat Hints to Downbeat Hints" => return edit(app, f, |m| m.beats_per_bar = 1),
        "Convert all Downbeat Hints to Beat Hints" => return edit(app, f, |m| m.beats_per_bar = m.beats_per_bar.max(4)),
        "All" => app.audio_edit.st.range = Some((0, n.saturating_sub(1))),
        "All Following" => app.audio_edit.st.range = Some((app.audio_edit.st.selected.unwrap_or(0), n.saturating_sub(1))),
        "All Previous" => app.audio_edit.st.range = Some((0, app.audio_edit.st.selected.unwrap_or(0))),
        "Select from Start to Playhead" => app.audio_edit.st.range = Some((0, play_beat.unwrap_or(0.0).floor().max(0.0) as usize)),
        "Select from Playhead to End" => app.audio_edit.st.range = Some((play_beat.unwrap_or(0.0).ceil().max(0.0) as usize, n.saturating_sub(1))),
        "Trim Region To Selection" => {
            let (Some(rt), Some(x)) = (rt, app.audio_edit.st.maps.get(f)) else { return false };
            let (s, e) = (x.map.beats.get(a).copied().unwrap_or(0.0), x.map.beats.get(b).copied().unwrap_or(0.0));
            if e <= s {
                return false;
            }
            return crate::audioedit::set_region_span(app, rt.track, rt.region, s, e);
        }
        "Forward by Bar" | "Rewind by Bar" | "Forward by Beat" | "Rewind by Beat" | "Forward 8 Bars" | "Rewind 8 Bars" => {
            let (Some(rt), Some(pb)) = (rt, play_beat) else { return false };
            let bpb = app.audio_edit.st.maps.get(f).map_or(4, |x| x.map.beats_per_bar.max(1)) as f64;
            let step = match name {
                "Forward by Bar" => bpb,
                "Rewind by Bar" => -bpb,
                "Forward by Beat" => 1.0,
                "Rewind by Beat" => -1.0,
                "Forward 8 Bars" => 8.0 * bpb,
                _ => -8.0 * bpb,
            };
            let target = if step > 0.0 { (pb + 1e-6).floor() + step } else { (pb - 1e-6).ceil() + step };
            let Some(sec) = app.audio_edit.st.maps.get(f).map(|x| x.map.sec_at(target.max(0.0))) else { return false };
            let tick = file_to_tick(app, &rt, sec).max(0.0);
            app.locate(tick);
        }
        _ => return false,
    }
    true
}

/// The menu items Smart Tempo shows ticked.
pub fn checked(app: &App, label: &str) -> Option<bool> {
    let st = &app.audio_edit.st;
    Some(match label {
        "Apply Changes Automatically" => st.apply_auto,
        "Snap to Transients" => st.snap_transients,
        "Maintain Time Position of All Regions" => st.maintain_time,
        "Scroll in Play" => st.scroll_in_play,
        "Show Tempo Curve Overlay" => st.show_curve,
        "Show Transients" => st.show_transients,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_and_seconds_round_trip_through_tempo_changes() {
        let map = vec![(0, 120.0), (3840, 60.0), (7680, 90.0)];
        // bar 2 (3840 ticks) at 120: 2 s; bar 3 at 60: 4 s later
        assert!((secs_at_tempo(120.0, &map, 3840.0) - 2.0).abs() < 1e-9);
        assert!((secs_at_tempo(120.0, &map, 7680.0) - 6.0).abs() < 1e-9);
        for t in [0.0, 100.0, 3840.0, 5000.0, 7680.0, 20000.0] {
            let s = secs_at_tempo(120.0, &map, t);
            assert!((tick_at_tempo(120.0, &map, s) - t).abs() < 1e-6, "{t}");
        }
    }
}
