//! Flashback Capture (Logic Pro 13: Record ▸ Flashback Capture, ⇧R, the control bar's button):
//! the most recent performance made into a region although Studio wasn't recording.
//!
//! - MIDI: the notes and controller messages played while the project plays are kept (one set a
//!   cycle pass); played while stopped they are kept with their times. Capturing after playing
//!   makes one region of them on the focused instrument track (cycling: a take for each pass
//!   that got MIDI, as Overlapping MIDI Recordings ▸ Cycle On says). Capturing what was played
//!   while stopped (Flashback Capture MIDI in Stop Mode, on by default) places it from the
//!   playhead at the project tempo: events before a pause of 20 s are dropped, notes before a
//!   pause of 1.5 bars or more stay hidden before the region's start (drag its start to show
//!   them). With Stop Mode off, those notes go at the playhead, one tick long.
//! - Audio: while the project plays (not recording) with an audio track focused, its input is
//!   kept from the moment a signal comes in, up to the last minute. Capturing needs at least
//!   four seconds of playback; each playback becomes a region from where the signal was found
//!   (cycling: a take per pass, as Overlapping Audio Recordings ▸ Cycle On says).
//! - The button and the menu item are dimmed until there is something to capture.

use std::collections::HashMap;
use std::time::Instant;

use lg_core::events::{Control, Note, PPQ};
use lg_core::{Region, TrackKind};

use crate::app::App;
use crate::audio::RATE;

pub const COMMANDS: &[&str] = &["Flashback Capture", "Flashback Capture as Recording and Play", "Flashback Capture MIDI in Stop Mode"];

/// Stop mode: events before a pause this long (seconds) are dropped.
pub const DROP_PAUSE: f64 = 20.0;
/// Stop mode: notes before a pause this long (bars) are hidden before the region start.
pub const HIDE_PAUSE_BARS: f64 = 1.5;
/// Audio: the most kept (seconds), the least playback needed (seconds), the signal level.
pub const KEEP_SECS: usize = 60;
pub const MIN_PLAY_SECS: usize = 4;
const SIGNAL: f32 = 0.003;
const SETTING: &str = "Recording / Flashback Capture MIDI in Stop Mode";

/// A note or controller played while stopped (seconds since the app started keeping them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timed {
    pub t: f64,
    pub len: f64,
    pub status: u8,
    pub a: u8,
    pub b: u8,
}

#[derive(Default)]
pub struct State {
    /// Played while playing: (cycle pass, note) and (pass, controller at its song position).
    play: Vec<(u32, Note)>,
    play_cc: Vec<(u32, Control)>,
    /// Played while stopped since the last playback (notes' lengths set as they end).
    stopped: Vec<Timed>,
    held: HashMap<u8, usize>,
    clock: Option<Instant>,
    pass: u32,
    was_playing: bool,
    last_pos: f64,
    /// The cycle while the last playback ran (its passes become takes).
    cycle: Option<(u32, u32)>,
}

fn now(s: &mut State) -> f64 {
    s.clock.get_or_insert_with(Instant::now).elapsed().as_secs_f64()
}

/// Flashback Capture MIDI in Stop Mode (an app setting, on by default).
pub fn stop_mode(app: &App) -> bool {
    app.logic_settings.0.get(SETTING).is_none_or(|v| v != "0")
}

/// A note started (velocity > 0) or ended, from a keyboard or Musical Typing (live.rs).
pub fn note_in(app: &mut App, ch: u8, pitch: u8, velocity: u8) {
    if app.recording() {
        return;
    }
    if app.playing() {
        if velocity > 0 {
            app.fb.held.insert(pitch, usize::MAX);
            return;
        }
        // (live.rs has the note's start; the note is taken from app.played as it ends)
        if let Some(n) = app.played.last().copied().filter(|n| n.pitch == pitch) {
            let pass = app.fb.pass;
            app.fb.play.push((pass, Note { channel: ch, ..n }));
            if app.fb.play.len() > 20_000 {
                app.fb.play.remove(0);
            }
        }
        app.fb.held.remove(&pitch);
        return;
    }
    let t = now(&mut app.fb);
    if velocity > 0 {
        app.fb.stopped.push(Timed { t, len: 0.0, status: 0x90 | (ch & 15), a: pitch, b: velocity });
        let k = app.fb.stopped.len() - 1;
        app.fb.held.insert(pitch, k);
    } else if let Some(k) = app.fb.held.remove(&pitch) {
        if let Some(n) = app.fb.stopped.get_mut(k) {
            n.len = (t - n.t).max(0.0);
        }
    }
}

/// A controller message from a keyboard.
pub fn cc_in(app: &mut App, ch: u8, number: u8, value: u8) {
    if app.recording() {
        return;
    }
    if app.playing() {
        let pos = app.position().max(0.0) as u32;
        let pass = app.fb.pass;
        app.fb.play_cc.push((pass, Control { kind: 0xB, channel: ch & 15, pos, number, value }));
        return;
    }
    let t = now(&mut app.fb);
    app.fb.stopped.push(Timed { t, len: 0.0, status: 0xB0 | (ch & 15), a: number, b: value });
}

/// Each frame: a new playback starts a new performance; cycle passes are counted; audio is kept
/// while an audio track is focused.
pub fn update(app: &mut App) {
    let playing = app.playing();
    let pos = app.position();
    if playing && !app.fb.was_playing && !app.recording() {
        app.fb.play.clear();
        app.fb.play_cc.clear();
        app.fb.stopped.clear();
        app.fb.held.clear();
        app.fb.pass = 0;
        app.fb.cycle = app.cycle_on().then_some(app.cycle).filter(|c| c.1 > c.0);
    }
    if playing && app.fb.cycle.is_some() && pos + 1.0 < app.fb.last_pos {
        app.fb.pass += 1;
    }
    app.fb.was_playing = playing;
    app.fb.last_pos = pos;
    let want = wants_input(app);
    let armed = want && crate::recording::ensure_feed(app);
    let sel = app.selected_track.and_then(|t| app.rec.inputs.get(&t).copied()).unwrap_or_default();
    let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
    e.rec.flash.armed = armed;
    e.rec.flash.sel = sel;
}

fn focused_audio(app: &App) -> Option<usize> {
    let t = app.selected_track?;
    app.project.as_ref()?.tracks.get(t).filter(|tr| tr.kind == TrackKind::Audio).map(|_| t)
}

/// The input is wanted for audio Flashback Capture: playing, not recording, an audio track
/// focused.
pub fn wants_input(app: &App) -> bool {
    app.playing() && !app.recording() && focused_audio(app).is_some()
}

fn midi_ready(app: &App) -> bool {
    app.fb.stopped.iter().any(|e| e.status & 0xF0 == 0x90) || !app.fb.play.is_empty()
}

fn audio_ready(app: &App) -> bool {
    focused_audio(app).is_some() && app.engine.lock().is_ok_and(|e| e.rec.flash.ready())
}

/// Something to capture (the button and the menu item are dimmed until there is).
pub fn available(app: &App) -> bool {
    !app.recording() && (audio_ready(app) || midi_ready(app))
}

pub fn enabled(_app: &App, label: &str) -> Option<bool> {
    match label {
        "Flashback Capture" | "Flashback Capture as Recording" | "Flashback Capture as Recording and Play" | "Capture as Recording" => Some(available(_app)),
        _ => None,
    }
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Flashback Capture" | "Capture as Recording" => capture(app),
        "Flashback Capture as Recording and Play" => {
            capture(app);
            if !app.playing() {
                app.play_stop();
            }
        }
        "Flashback Capture MIDI in Stop Mode" => {
            let on = !stop_mode(app);
            app.logic_settings.0.insert(SETTING.into(), if on { "1" } else { "0" }.into());
            app.logic_settings.save();
        }
        _ => return false,
    }
    true
}

/// The control bar button's Control-click menu.
pub fn button_menu(app: &mut App, resp: &egui::Response) {
    resp.context_menu(|ui| {
        if ui.add_enabled(available(app), egui::Button::new("Flashback Capture")).clicked() {
            capture(app);
            ui.close();
        }
        ui.separator();
        let mut on = stop_mode(app);
        if ui.checkbox(&mut on, "Flashback Capture MIDI in Stop Mode").changed() {
            command(app, "Flashback Capture MIDI in Stop Mode");
        }
    });
}

/// The Record button's Control-click menu (Record Button Options, Quick Punch-In, Auto Input
/// Monitoring, Flashback Capture).
pub fn record_button_menu(app: &mut App, resp: &egui::Response) {
    resp.context_menu(|ui| {
        ui.set_min_width(220.0);
        for l in ["Record", "Record / Record Toggle", "Record / Record Repeat"] {
            let (on, _) = crate::recording::choice(app, "Record Button Options", 0, l, false);
            if ui.selectable_label(on, l).clicked() {
                crate::recording::choice(app, "Record Button Options", 0, l, true);
                ui.close();
            }
        }
        ui.separator();
        for l in ["Allow Quick Punch-In", "Auto Input Monitoring"] {
            let mut on = crate::recording::checked(app, l).unwrap_or(false);
            if ui.checkbox(&mut on, l).changed() {
                crate::recording::command(app, l);
            }
        }
        ui.separator();
        if ui.add_enabled(available(app), egui::Button::new("Flashback Capture")).clicked() {
            capture(app);
            ui.close();
        }
        ui.separator();
        if ui.button("Recording Settings…").clicked() {
            crate::recording::command(app, "Recording Settings…");
            ui.close();
        }
    });
}

/// Flashback Capture: the audio kept for the focused audio track, else the MIDI kept.
pub fn capture(app: &mut App) {
    if app.recording() || app.project.is_none() {
        return;
    }
    if let Some(t) = focused_audio(app) {
        if capture_audio(app, t) {
            return;
        }
    }
    capture_midi(app);
}

// ---------------------------------------------------------------------------------------------
// MIDI

/// Stop mode's notes and controllers as (ticks from the region start, length ticks, event):
/// `tps` ticks a second, `bar` ticks a bar. Events before a 20 s pause are dropped; the region
/// starts at the first note after the last pause of 1.5 bars or more (earlier ones come before
/// it, negative). `keep_time` false: every note at 0, one tick long.
pub fn stop_events(ev: &[Timed], tps: f64, bar: f64, keep_time: bool) -> Vec<(f64, f64, Timed)> {
    let mut ev: Vec<Timed> = ev.to_vec();
    ev.sort_by(|a, b| a.t.total_cmp(&b.t));
    if !keep_time {
        return ev.into_iter().filter(|e| e.status & 0xF0 == 0x90).map(|e| (0.0, 1.0, e)).collect();
    }
    // the pauses: from the end of everything before to the next event
    let mut end = f64::MIN;
    let (mut from, mut anchor) = (0, 0);
    for (k, e) in ev.iter().enumerate() {
        if k > 0 {
            let gap = e.t - end;
            if gap >= DROP_PAUSE {
                from = k;
                anchor = k;
            } else if gap * tps >= HIDE_PAUSE_BARS * bar {
                anchor = k;
            }
        }
        end = end.max(e.t + e.len);
    }
    let ev = &ev[from..];
    let anchor = anchor - from;
    // (the region starts at the first note from there)
    let t0 = ev[anchor..].iter().find(|e| e.status & 0xF0 == 0x90).or(ev.get(anchor)).map_or(0.0, |e| e.t);
    ev.iter().map(|e| ((e.t - t0) * tps, (e.len * tps).max(1.0), *e)).collect()
}

/// The captured notes' region range from `start` (whole bars, at least one).
fn bar_range(start: u32, end: u32, bar: u32, musical: bool) -> (u32, u32) {
    if !musical {
        return (start, end.max(start + 1));
    }
    (start, start + end.saturating_sub(start).div_ceil(bar).max(1) * bar)
}

fn capture_midi(app: &mut App) {
    let Some(t) = crate::recording::midi_target(app).or(app.selected_track) else { return };
    if app.project.as_ref().and_then(|p| p.tracks.get(t)).is_none_or(|tr| tr.kind == TrackKind::Audio) {
        return;
    }
    let bar = app.bar_ticks();
    let stopped_notes = app.fb.stopped.iter().any(|e| e.status & 0xF0 == 0x90);
    if stopped_notes {
        let ev = std::mem::take(&mut app.fb.stopped);
        app.fb.held.clear();
        let p = app.position().max(0.0);
        let tps = crate::recording::tick_at(app, app.seconds_at(p) + 1.0) - p;
        let placed = stop_events(&ev, tps.max(1.0), bar as f64, stop_mode(app));
        let start = p.round() as u32;
        let mut notes = Vec::new();
        let mut ccs = Vec::new();
        for (dt, len, e) in placed {
            let at = p + dt;
            if at < 0.0 {
                continue;
            }
            if e.status & 0xF0 == 0x90 {
                notes.push(Note { channel: e.status & 15, pitch: e.a, velocity: e.b, pos: at.round() as u32, len: len.round().max(1.0) as u32, muted: false });
            } else if at >= start as f64 {
                ccs.push(Control { kind: 0xB, channel: e.status & 15, pos: at.round() as u32 - start, number: e.a, value: e.b });
            }
        }
        let end = notes.iter().map(|n| n.pos + n.len).max().unwrap_or(start + 1);
        let range = bar_range(start, end, bar, app.rec.musical_grid);
        crate::audioedit::checkpoint(app, "Flashback Capture");
        crate::recording::place_midi(app, t, vec![notes], range, false);
        add_ccs(app, t, range.0, ccs);
        app.after_edit();
        return;
    }
    let play = std::mem::take(&mut app.fb.play);
    let ccs = std::mem::take(&mut app.fb.play_cc);
    if play.is_empty() {
        return;
    }
    let mut notes: Vec<Note> = play.iter().map(|p| p.1).collect();
    crate::scoreattr::auto_split(app, t, &mut notes);
    crate::audioedit::checkpoint(app, "Flashback Capture");
    match app.fb.cycle {
        // cycling: a take a pass that got MIDI
        Some(c) if play.iter().any(|p| p.0 != play[0].0) => {
            let mut passes: Vec<(u32, Vec<Note>)> = Vec::new();
            for (k, n) in play {
                match passes.iter_mut().find(|p| p.0 == k) {
                    Some(p) => p.1.push(n),
                    None => passes.push((k, vec![n])),
                }
            }
            crate::recording::place_midi(app, t, passes.into_iter().map(|p| p.1).collect(), c, true);
        }
        _ => {
            let first = notes.iter().map(|n| n.pos).min().unwrap_or(0);
            let end = notes.iter().map(|n| n.pos + n.len).max().unwrap_or(first + 1);
            let start = if app.rec.musical_grid { first / bar * bar } else { first };
            let range = bar_range(start, end, bar, app.rec.musical_grid);
            crate::recording::place_midi(app, t, vec![notes], range, false);
            let ccs = ccs.into_iter().filter(|c| c.1.pos >= range.0 && c.1.pos < range.1).map(|c| Control { pos: c.1.pos - range.0, ..c.1 }).collect();
            add_ccs(app, t, range.0, ccs);
        }
    }
    app.after_edit();
}

/// The controllers into the region just placed on `t` at `start`.
fn add_ccs(app: &mut App, t: usize, start: u32, ccs: Vec<Control>) {
    if ccs.is_empty() {
        return;
    }
    let sel = app.selected_region;
    if let Some(r) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|tr| {
        let k = sel.filter(|s| s.0 == t).map(|s| s.1)?;
        tr.regions.get_mut(k).filter(|r| r.start == start)
    }) {
        r.ext.events.extend(ccs);
        r.ext.events.sort_by_key(|c| c.pos);
    }
}

// ---------------------------------------------------------------------------------------------
// audio

/// One stretch of input kept: its playback and cycle pass, its first frame's position (ticks)
/// and the ticks a frame then, the stereo frames, and the frame after the last one with signal.
#[derive(Clone, Debug, Default)]
pub struct Span {
    pub play: u32,
    pub pass: u32,
    pub start: f64,
    pub tps: f64,
    pub data: Vec<f32>,
    pub sig_end: usize,
}

/// The input kept for audio Flashback Capture (in the engine: EngineRec.flash).
#[derive(Default)]
pub struct Buf {
    /// Set by the app: keep the input while playing.
    pub armed: bool,
    pub sel: crate::recording::InputSel,
    pub spans: Vec<Span>,
    play: u32,
    pass: u32,
    open: bool,
    /// Frames played by each playback.
    played: Vec<(u32, usize)>,
}

impl Buf {
    /// A block played from `from` (`tps` ticks a frame); `fresh`: playback just started.
    pub fn take(&mut self, from: f64, tps: f64, input: Option<&[f32]>, fresh: bool) {
        if fresh {
            self.play += 1;
            self.pass = 0;
            self.open = false;
        }
        if !self.armed {
            self.open = false;
            return;
        }
        let Some(inp) = input else { return };
        let n = inp.len() / 2;
        match self.played.last_mut() {
            Some(p) if p.0 == self.play => p.1 += n,
            _ => self.played.push((self.play, n)),
        }
        let loud = |k: usize| inp[2 * k].abs().max(inp[2 * k + 1].abs()) > SIGNAL;
        let k0 = if self.open {
            0
        } else {
            // (kept from the first frame with signal)
            let Some(k) = (0..n).find(|&k| loud(k)) else { return };
            self.spans.push(Span { play: self.play, pass: self.pass, start: from + k as f64 * tps, tps, data: Vec::new(), sig_end: 0 });
            self.open = true;
            k
        };
        let s = self.spans.last_mut().unwrap();
        let base = s.data.len() / 2;
        s.data.extend_from_slice(&inp[2 * k0..2 * n]);
        if let Some(k) = (k0..n).rev().find(|&k| loud(k)) {
            s.sig_end = base + (k - k0) + 1;
        }
        self.trim();
    }

    /// The cycle went round: the next frames are a new pass.
    pub fn wrapped(&mut self) {
        self.pass += 1;
        self.open = false;
    }

    /// Only the last minute is kept.
    fn trim(&mut self) {
        let keep = KEEP_SECS * RATE as usize;
        let mut total: usize = self.spans.iter().map(|s| s.data.len() / 2).sum();
        while total > keep && !self.spans.is_empty() {
            let s = &mut self.spans[0];
            let d = (total - keep).min(s.data.len() / 2);
            s.data.drain(..2 * d);
            s.start += d as f64 * s.tps;
            s.sig_end = s.sig_end.saturating_sub(d);
            total -= d;
            if s.data.is_empty() {
                self.spans.remove(0);
                if self.spans.is_empty() {
                    self.open = false;
                }
            }
        }
        let first = self.spans.first().map_or(self.play, |s| s.play);
        self.played.retain(|p| p.0 >= first);
    }

    fn long_enough(&self, play: u32) -> bool {
        self.played.iter().any(|p| p.0 == play && p.1 >= MIN_PLAY_SECS * RATE as usize)
    }

    /// Something to capture: signal from a playback of four seconds or more.
    pub fn ready(&self) -> bool {
        self.spans.iter().any(|s| s.sig_end > 0 && self.long_enough(s.play))
    }

    /// What can be captured (each span cut after its last signal), the buffer emptied.
    pub fn take_spans(&mut self) -> Vec<Span> {
        let spans = std::mem::take(&mut self.spans);
        self.open = false;
        let out = spans
            .into_iter()
            .filter(|s| s.sig_end > 0 && self.long_enough(s.play))
            .map(|mut s| {
                let end = (s.sig_end + RATE as usize / 10).min(s.data.len() / 2);
                s.data.truncate(2 * end);
                s
            })
            .collect();
        self.played.clear();
        out
    }
}

fn capture_audio(app: &mut App, t: usize) -> bool {
    let spans = {
        let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
        if !e.rec.flash.ready() {
            return false;
        }
        e.rec.flash.take_spans()
    };
    let sel = app.rec.inputs.get(&t).copied().unwrap_or_default();
    let name = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(String::new(), |tr| tr.name.clone());
    let dir = crate::recording::audio_dir(app);
    let _ = std::fs::create_dir_all(&dir);
    let bits = match app.logic_settings.get(crate::prefs::REC_BIT_DEPTH).as_str() {
        "16-bit" => 16,
        "32-bit Float" => 32,
        _ => 24,
    };
    // each playback: its regions (a take a pass when it cycled)
    let mut plays: Vec<(u32, Vec<Region>)> = Vec::new();
    for s in spans {
        let path = crate::recording::file_name(&dir, &name);
        let ch = sel.channels();
        let frames = s.data.len() / 2;
        let mut out = Vec::with_capacity(frames * ch);
        for f in s.data.chunks(2) {
            let (a, b) = sel.pick(f[0], f[1]);
            out.push(a);
            if ch == 2 {
                out.push(b);
            }
        }
        let ok = lg_audio::record::WavWriter::create(&path, RATE, ch, bits).and_then(|mut w| {
            w.write(&out)?;
            w.finish()
        });
        if let Err(e) = ok {
            app.tell(format!("Can't write {}: {e}", path.display()));
            continue;
        }
        let secs = frames as f64 / RATE as f64;
        let end = crate::recording::tick_at(app, app.seconds_at(s.start) + secs);
        let start = s.start.max(0.0).round() as u32;
        let length = ((end - s.start).round() as u32).max(1);
        app.rec.recorded_at.insert(path.clone(), s.start);
        let r = Region { name: name.clone(), start, length, notes: Vec::new(), audio: Some(lg_core::project::AudioRef { file: path, seconds: secs }), muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() };
        match plays.iter_mut().find(|p| p.0 == s.play) {
            Some(p) => p.1.push(r),
            None => plays.push((s.play, vec![r])),
        }
    }
    if plays.is_empty() {
        return false;
    }
    crate::audioedit::checkpoint(app, "Flashback Capture");
    let cycle = app.fb.cycle;
    for (_, regions) in plays {
        if regions.len() > 1 && cycle.is_some() {
            crate::recording::place_audio(app, t, regions, cycle);
        } else {
            for r in regions {
                crate::recording::place_audio(app, t, vec![r], None);
            }
        }
    }
    app.after_edit();
    let _ = PPQ;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(t: f64, len: f64, pitch: u8) -> Timed {
        Timed { t, len, status: 0x90, a: pitch, b: 100 }
    }

    #[test]
    fn stop_mode_keeps_time_hides_and_drops() {
        // 2 ticks a second for easy numbers: a bar of 8 ticks (1.5 bars = 12 ticks = 6 s)
        let ev = vec![n(0.0, 0.5, 60), n(25.0, 0.5, 61), n(26.0, 0.5, 62), n(40.0, 1.0, 64), n(41.0, 1.0, 65)];
        let out = stop_events(&ev, 2.0, 8.0, true);
        // the note before the 24.5 s pause is dropped
        assert_eq!(out.len(), 4);
        assert!(out.iter().all(|e| e.2.a != 60));
        // the region starts at the note after the 13.5 s (27 tick) pause; the earlier ones are
        // before it (hidden)
        let at: Vec<(u8, f64)> = out.iter().map(|e| (e.2.a, e.0)).collect();
        assert_eq!(at, vec![(61, -30.0), (62, -28.0), (64, 0.0), (65, 2.0)]);
        assert_eq!(out[2].1, 2.0);
        // short pauses: all kept from the first note
        let out = stop_events(&[n(1.0, 0.2, 60), n(2.0, 0.2, 62)], 2.0, 8.0, true);
        assert_eq!(out.iter().map(|e| e.0).collect::<Vec<_>>(), vec![0.0, 2.0]);
    }

    #[test]
    fn stop_mode_off_puts_notes_at_the_playhead() {
        let ev = vec![n(0.0, 0.5, 60), n(3.0, 0.5, 64), Timed { t: 1.0, len: 0.0, status: 0xB0, a: 1, b: 64 }];
        let out = stop_events(&ev, 2.0, 8.0, false);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|e| e.0 == 0.0 && e.1 == 1.0));
    }

    #[test]
    fn audio_buffer_waits_for_signal_and_needs_four_seconds() {
        let mut b = Buf { armed: true, ..Default::default() };
        let block = |v: f32, frames: usize| vec![v; frames * 2];
        let r = RATE as usize;
        // a second of silence, then signal for a second: kept from the signal
        b.take(0.0, 1.0, Some(&block(0.0, r)), true);
        assert!(b.spans.is_empty());
        b.take(r as f64, 1.0, Some(&block(0.2, r)), false);
        assert_eq!(b.spans.len(), 1);
        assert_eq!(b.spans[0].start, r as f64);
        // two seconds played: not yet
        assert!(!b.ready());
        b.take(2.0 * r as f64, 1.0, Some(&block(0.0, 2 * r)), false);
        assert!(b.ready());
        // a cycle pass: a new span; stopped and played again: another playback
        b.wrapped();
        b.take(0.0, 1.0, Some(&block(0.2, r / 2)), false);
        b.take(0.0, 1.0, Some(&block(0.2, r / 2)), true);
        let s = b.take_spans();
        // (the last playback was half a second: left out)
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].pass, s[1].pass), (0, 1));
        // cut a tenth of a second after the last signal
        assert_eq!(s[0].data.len() / 2, r + r / 10);
        assert!(b.spans.is_empty() && !b.ready());
    }

    #[test]
    fn audio_buffer_keeps_the_last_minute() {
        let mut b = Buf { armed: true, ..Default::default() };
        let r = RATE as usize;
        let sec = vec![0.1f32; r * 2];
        for k in 0..70 {
            b.take((k * r) as f64, 1.0, Some(&sec), k == 0);
        }
        let total: usize = b.spans.iter().map(|s| s.data.len() / 2).sum();
        assert_eq!(total, KEEP_SECS * r);
        assert_eq!(b.spans[0].start, (10 * r) as f64);
        // not armed: nothing kept
        let mut q = Buf::default();
        q.take(0.0, 1.0, Some(&sec), true);
        assert!(q.spans.is_empty());
    }
}
