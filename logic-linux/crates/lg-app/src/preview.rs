//! Previews for the Browsers (the Browser, Project Audio, All Files): a file (or part of one)
//! played over the song at the preview volume, once or looped (Project Audio's Cycle), stopped
//! by a second click; MIDI files play their notes through the selected track's sound.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use crate::app::App;

/// The preview playing now.
#[derive(Default)]
pub struct Preview {
    /// What plays (its id: a path, or a path and region).
    pub playing: Option<String>,
    stop: Option<Arc<AtomicBool>>,
    gain: Arc<AtomicU32>,
    /// When it started and how long it is (seconds), for the playing mark.
    pub started: f64,
    pub seconds: f64,
    pub looped: bool,
}

impl Preview {
    pub fn set_volume(&self, gain: f32) {
        self.gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn stop(&mut self) {
        if let Some(s) = self.stop.take() {
            s.store(true, Ordering::Relaxed);
        }
        self.playing = None;
    }

    pub fn is_playing(&self, id: &str, now: f64) -> bool {
        self.playing.as_deref() == Some(id) && (self.looped || now - self.started < self.seconds + 0.1)
    }
}

/// Samples played with the preview's gain until stopped.
struct Source {
    samples: Arc<Vec<f32>>,
    from: usize,
    to: usize,
    at: usize,
    looped: bool,
    gain: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
}

impl Iterator for Source {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.stop.load(Ordering::Relaxed) {
            return None;
        }
        if self.at >= self.to {
            if !self.looped || self.to <= self.from {
                return None;
            }
            self.at = self.from;
        }
        let x = self.samples.get(self.at).copied().unwrap_or(0.0);
        self.at += 1;
        Some(x * f32::from_bits(self.gain.load(Ordering::Relaxed)))
    }
}

impl rodio::Source for Source {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(crate::audio::RATE).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// Plays `path` (from `from_s` for `len_s` seconds; None: to the end) at `gain`, looped or
/// once. Playing the same id again stops it (the play button's second click).
pub fn toggle(app: &mut App, id: &str, path: &Path, gain: f32, segment: Option<(f64, f64)>, looped: bool) {
    let now = app.ctx.input(|i| i.time);
    if app.preview.is_playing(id, now) {
        app.preview.stop();
        return;
    }
    app.preview.stop();
    app.preview.set_volume(gain);
    if crate::soundlib::is_midi(path) {
        midi(app, id, path);
        return;
    }
    let Some(samples) = crate::audio::load_audio(path) else {
        app.tell("That file couldn't be played");
        return;
    };
    let rate = crate::audio::RATE as f64;
    let (from, to) = match segment {
        Some((a, l)) => (((a * rate) as usize * 2).min(samples.len()), (((a + l) * rate) as usize * 2).min(samples.len())),
        None => (0, samples.len()),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let src = Source { samples: Arc::new(samples), from, to, at: from, looped, gain: app.preview.gain.clone(), stop: stop.clone() };
    app.output.add_source(src);
    app.preview.stop = Some(stop);
    app.preview.playing = Some(id.to_string());
    app.preview.started = now;
    app.preview.seconds = (to - from) as f64 / 2.0 / rate;
    app.preview.looped = looped;
}

/// Samples made off the interface thread (`render`, e.g. a patch's chord), played once at
/// `gain` as soon as they're ready; silent until then. `seconds`: how long the speaker shows.
pub fn play_rendered(app: &mut App, id: &str, gain: f32, seconds: f64, render: impl FnOnce() -> Vec<f32> + std::marker::Send + 'static) {
    app.preview.stop();
    app.preview.set_volume(gain);
    let ready = Arc::new(std::sync::OnceLock::new());
    let r2 = ready.clone();
    std::thread::spawn(move || {
        let _ = r2.set(render());
    });
    let stop = Arc::new(AtomicBool::new(false));
    app.output.add_source(Pending { ready, at: 0, waited: 0, gain: app.preview.gain.clone(), stop: stop.clone() });
    app.preview.stop = Some(stop);
    app.preview.playing = Some(id.to_string());
    app.preview.started = app.ctx.input(|i| i.time);
    app.preview.seconds = seconds;
    app.preview.looped = false;
}

/// Samples still being made: silence until they're ready (two seconds at most), then them.
struct Pending {
    ready: Arc<std::sync::OnceLock<Vec<f32>>>,
    at: usize,
    waited: usize,
    gain: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
}

impl Iterator for Pending {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.stop.load(Ordering::Relaxed) {
            return None;
        }
        let Some(s) = self.ready.get() else {
            self.waited += 1;
            return (self.waited < 4 * crate::audio::RATE as usize).then_some(0.0);
        };
        let x = *s.get(self.at)?;
        self.at += 1;
        Some(x * f32::from_bits(self.gain.load(Ordering::Relaxed)))
    }
}

impl rodio::Source for Pending {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(crate::audio::RATE).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// A MIDI file's notes through the selected track's sound, at the project tempo.
fn midi(app: &mut App, id: &str, path: &Path) {
    let Some(m) = std::fs::read(path).ok().and_then(|d| lg_core::midi::read(&d)) else {
        app.tell("That MIDI file couldn't be read");
        return;
    };
    let t = app.selected_track.unwrap_or(0);
    let tempo = m.tempo.unwrap_or(app.tempo());
    let mut ev: Vec<(u32, u8, u8)> = Vec::new();
    for (_, _, notes) in &m.tracks {
        for n in notes {
            ev.push((n.pos, n.pitch, n.velocity));
            ev.push((n.pos + n.len.max(1), n.pitch, 0));
        }
    }
    ev.sort_by_key(|e| (e.0, e.2));
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let engine = app.engine.clone();
    let gain = app.preview.gain.clone();
    let secs_per_tick = 60.0 / tempo / lg_core::events::PPQ as f64;
    let total = ev.last().map_or(0.0, |e| e.0 as f64 * secs_per_tick);
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        for (tick, pitch, vel) in ev {
            let at = std::time::Duration::from_secs_f64(tick as f64 * secs_per_tick);
            while start.elapsed() < at {
                if s2.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let g = f32::from_bits(gain.load(Ordering::Relaxed));
            let v = if vel == 0 || s2.load(Ordering::Relaxed) { 0 } else { ((vel as f32 * g.min(1.5)) as u8).clamp(1, 127) };
            if let Ok(mut e) = engine.lock() {
                e.live_note(t, pitch, v);
            }
        }
    });
    app.preview.stop = Some(stop);
    app.preview.playing = Some(id.to_string());
    app.preview.started = app.ctx.input(|i| i.time);
    app.preview.seconds = total;
    app.preview.looped = false;
}
