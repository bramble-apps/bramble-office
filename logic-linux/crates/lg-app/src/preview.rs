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
    /// The last audio file previewed, for the Browser's Preview Player (prevplayer.rs): its
    /// id, path, samples and peaks; where in it this preview started (seconds).
    pub wave: Option<Wave>,
    pub offset: f64,
}

pub struct Wave {
    pub id: String,
    pub samples: Arc<Vec<f32>>,
    pub peaks: Vec<f32>,
    pub looped: bool,
    pub gain: f32,
}

/// `n` peaks (0..1) across stereo samples.
pub fn peaks(samples: &[f32], n: usize) -> Vec<f32> {
    let frames = samples.len() / 2;
    if frames == 0 || n == 0 {
        return Vec::new();
    }
    (0..n)
        .map(|k| {
            let a = k * frames / n;
            let b = ((k + 1) * frames / n).max(a + 1).min(frames);
            samples[a * 2..b * 2].iter().fold(0f32, |m, x| m.max(x.abs())).min(1.0)
        })
        .collect()
}

/// Where the preview is now, as a fraction of the file (None: not playing it).
pub fn wave_position(app: &App, now: f64) -> Option<f32> {
    let w = app.preview.wave.as_ref()?;
    if !app.preview.is_playing(&w.id, now) {
        return None;
    }
    let len = w.samples.len() as f64 / 2.0 / crate::audio::RATE as f64;
    if len <= 0.0 {
        return None;
    }
    let t = app.preview.offset + now - app.preview.started;
    Some(((if w.looped { t.rem_euclid(len) } else { t.min(len) }) / len) as f32)
}

/// The Preview Player: play the last previewed file from `frac` (0..1) of its length.
pub fn play_wave_from(app: &mut App, frac: f32) {
    let Some(w) = app.preview.wave.as_ref() else { return };
    let (id, samples, looped, gain) = (w.id.clone(), w.samples.clone(), w.looped, w.gain);
    app.preview.stop();
    app.preview.set_volume(gain);
    let frames = samples.len() / 2;
    let at = ((frac.clamp(0.0, 1.0) as f64 * frames as f64) as usize).min(frames.saturating_sub(1)) * 2;
    let stop = Arc::new(AtomicBool::new(false));
    let len = samples.len();
    app.output.add_source(Source { samples, from: 0, to: len, at, looped, gain: app.preview.gain.clone(), stop: stop.clone() });
    let rate = crate::audio::RATE as f64;
    app.preview.stop = Some(stop);
    app.preview.playing = Some(id);
    app.preview.started = app.ctx.input(|i| i.time);
    app.preview.offset = at as f64 / 2.0 / rate;
    app.preview.seconds = (len - at) as f64 / 2.0 / rate;
    app.preview.looped = looped;
}

impl Preview {
    pub fn set_volume(&self, gain: f32) {
        self.gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn stop(&mut self) {
        if let Some(s) = self.stop.take() {
            s.store(true, Ordering::Relaxed);
        }
        if self.playing.take().is_some() {
            cancel_render();
        }
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
    let samples = Arc::new(samples);
    app.preview.wave = Some(Wave { id: id.to_string(), peaks: peaks(&samples, 400), samples: samples.clone(), looped, gain });
    app.preview.offset = from as f64 / 2.0 / rate;
    let src = Source { samples, from, to, at: from, looped, gain: app.preview.gain.clone(), stop: stop.clone() };
    app.output.add_source(src);
    app.preview.stop = Some(stop);
    app.preview.playing = Some(id.to_string());
    app.preview.started = now;
    app.preview.seconds = (to - from) as f64 / 2.0 / rate;
    app.preview.looped = looped;
}

/// A render for the preview thread: it's given a check that says whether to go on (false: a
/// newer preview replaced it, so it stops early).
pub type Render = Box<dyn FnOnce(&dyn Fn() -> bool) -> Vec<f32> + std::marker::Send>;

struct Job {
    generation: u64,
    render: Render,
    ready: Arc<std::sync::OnceLock<Vec<f32>>>,
}

/// The one thread that renders previews (patches' chords): one render at a time; a new
/// request replaces one still waiting and tells the one in progress to stop; a render that
/// panics only ends that preview.
struct Renderer {
    slot: std::sync::Mutex<Option<Job>>,
    wake: std::sync::Condvar,
    generation: std::sync::atomic::AtomicU64,
    busy: AtomicBool,
}

fn renderer() -> &'static Renderer {
    static R: std::sync::OnceLock<&'static Renderer> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        let r: &'static Renderer = Box::leak(Box::new(Renderer { slot: Default::default(), wake: Default::default(), generation: Default::default(), busy: AtomicBool::new(false) }));
        let _ = std::thread::Builder::new().name("preview-render".into()).spawn(move || loop {
            let job = {
                let mut slot = r.slot.lock().unwrap_or_else(|e| e.into_inner());
                loop {
                    if let Some(j) = slot.take() {
                        break j;
                    }
                    slot = r.wake.wait(slot).unwrap_or_else(|e| e.into_inner());
                }
            };
            let current = |g: u64| r.generation.load(Ordering::SeqCst) == g;
            if !current(job.generation) {
                continue;
            }
            r.busy.store(true, Ordering::SeqCst);
            let g = job.generation;
            let go = move || current(g);
            let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (job.render)(&go)));
            r.busy.store(false, Ordering::SeqCst);
            match out {
                Ok(s) if current(g) => {
                    let _ = job.ready.set(s);
                }
                Ok(_) => {}
                Err(_) => {
                    eprintln!("preview render failed (the app goes on)");
                    let _ = job.ready.set(Vec::new());
                }
            }
        });
        r
    })
}

/// Stops the preview render in progress or waiting (a newer preview, or Stop).
pub fn cancel_render() {
    renderer().generation.fetch_add(1, Ordering::SeqCst);
}

/// Whether the preview thread is rendering now (for tests).
pub fn rendering() -> bool {
    let r = renderer();
    r.busy.load(Ordering::SeqCst) || r.slot.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

/// Samples for `render` made on the preview thread (replacing any render before it): they
/// appear in the returned cell when ready.
pub fn submit_render(render: Render) -> Arc<std::sync::OnceLock<Vec<f32>>> {
    let r = renderer();
    let generation = r.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let ready = Arc::new(std::sync::OnceLock::new());
    *r.slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(Job { generation, render, ready: ready.clone() });
    r.wake.notify_one();
    ready
}

/// Samples made off the interface thread (`render`, e.g. a patch's chord), played once at
/// `gain` as soon as they're ready; silent until then. `seconds`: how long the speaker shows.
pub fn play_rendered(app: &mut App, id: &str, gain: f32, seconds: f64, render: Render) {
    app.preview.stop();
    app.preview.set_volume(gain);
    let ready = submit_render(render);
    let stop = Arc::new(AtomicBool::new(false));
    app.output.add_source(Pending { ready, at: 0, waited: 0, gain: app.preview.gain.clone(), stop: stop.clone() });
    app.preview.stop = Some(stop);
    app.preview.playing = Some(id.to_string());
    app.preview.started = app.ctx.input(|i| i.time);
    app.preview.seconds = seconds;
    app.preview.looped = false;
}

/// Samples still being made: silence until they're ready (15 s at most), then them.
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
            // (a first click also loads the patch onto the track, so the render can take a few
            // seconds while the instrument's samples load: keep waiting; a newer preview or Stop
            // ends this one through `stop`)
            return (self.waited < 30 * crate::audio::RATE as usize).then_some(0.0);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wait(cell: &std::sync::OnceLock<Vec<f32>>) -> Option<Vec<f32>> {
        let t = std::time::Instant::now();
        while cell.get().is_none() && t.elapsed().as_secs() < 10 {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        cell.get().cloned()
    }

    /// A newer render replaces older ones (they stop early, give nothing); one that panics only
    /// ends itself and the thread renders the next.
    #[test]
    fn newest_render_wins_and_a_panic_doesnt_stop_the_thread() {
        let first = submit_render(Box::new(|go: &dyn Fn() -> bool| {
            while go() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            vec![9.0]
        }));
        for k in 0..40 {
            submit_render(Box::new(move |_: &dyn Fn() -> bool| vec![k as f32]));
        }
        let boom = submit_render(Box::new(|_: &dyn Fn() -> bool| panic!("a bad patch")));
        assert_eq!(wait(&boom), Some(Vec::new()));
        let last = submit_render(Box::new(|_: &dyn Fn() -> bool| vec![1.0, 2.0]));
        assert_eq!(wait(&last), Some(vec![1.0, 2.0]));
        assert!(first.get().is_none(), "a replaced render still gave samples");
    }
}
