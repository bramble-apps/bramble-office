//! The sound engine: plays the song's notes and audio regions from the playhead, in time with
//! the tempo. Instruments are a General MIDI SoundFont played natively (rustysynth, the
//! SoundFont the settings name, or the system's), or a small built-in synth when there's none.
//! The engine runs in rodio's mixer thread; the interface reads the playhead from it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lg_core::events::PPQ;

/// The output rate the engine renders at (rodio's mixer converts to the device's).
pub const RATE: u32 = 48_000;
const BLOCK: usize = 64;

/// One track as the engine plays it.
#[derive(Clone, Debug)]
pub struct PlayTrack {
    /// MIDI channel (9 = drums).
    pub channel: u8,
    pub program: u8,
    /// (start, end, pitch, velocity), sorted by start; song ticks.
    pub notes: Vec<(u32, u32, u8, u8)>,
    /// Audio regions: start tick and stereo samples at RATE.
    pub audio: Vec<(u32, Arc<Vec<f32>>)>,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
}

pub struct Engine {
    pub playing: bool,
    /// The playhead in ticks from bar 1.
    pub pos: f64,
    pub tempo: f64,
    /// Tempo changes (ticks, BPM); empty: `tempo` throughout.
    pub tempo_map: Vec<(u32, f64)>,
    pub tracks: Vec<PlayTrack>,
    pub cycle: Option<(u32, u32)>,
    pub cycle_on: bool,
    /// The metronome click (K): on every beat, higher on the bar's first (Logic's Klopfgeist).
    pub metronome: bool,
    /// Beats per bar, for the click's accent.
    pub beats_per_bar: u32,
    click: Option<(f32, f32)>,
    pub master: f32,
    /// Peak levels per track (for the meters), decaying.
    pub meters: Vec<f32>,
    pub master_meter: f32,
    synth: Option<rustysynth::Synthesizer>,
    voices: Vec<Voice>,
    sounding: Vec<(usize, u8, u8, u32)>,
    pub soundfont: Option<PathBuf>,
    pub error: Option<String>,
}

#[derive(Clone, Copy)]
struct Voice {
    track: usize,
    pitch: u8,
    phase: f32,
    freq: f32,
    amp: f32,
    env: f32,
    releasing: bool,
    drum: bool,
}

impl Engine {
    pub fn new() -> Engine {
        Engine { playing: false, pos: 0.0, tempo: 120.0, tempo_map: Vec::new(), tracks: Vec::new(), cycle: None, cycle_on: false, metronome: false, beats_per_bar: 4, click: None, master: 1.0, meters: Vec::new(), master_meter: 0.0, synth: None, voices: Vec::new(), sounding: Vec::new(), soundfont: None, error: None }
    }

    /// Loads a SoundFont for the instruments (None: the system's General MIDI one, if any).
    pub fn load_soundfont(&mut self, path: Option<&Path>) {
        let found = path.map(Path::to_path_buf).or_else(system_soundfont);
        self.synth = None;
        self.soundfont = None;
        let Some(p) = found else { return };
        match std::fs::File::open(&p).map_err(|e| e.to_string()).and_then(|mut f| rustysynth::SoundFont::new(&mut f).map_err(|e| format!("{e:?}"))) {
            Ok(sf) => {
                let settings = rustysynth::SynthesizerSettings::new(RATE as i32);
                match rustysynth::Synthesizer::new(&Arc::new(sf), &settings) {
                    Ok(s) => {
                        self.synth = Some(s);
                        self.soundfont = Some(p);
                    }
                    Err(e) => self.error = Some(format!("SoundFont: {e:?}")),
                }
            }
            Err(e) => self.error = Some(format!("SoundFont {}: {e}", p.display())),
        }
        self.program_changes();
    }

    /// Each track's sound on its channel.
    pub fn program_changes(&mut self) {
        let progs: Vec<(u8, u8)> = self.tracks.iter().map(|t| (t.channel, t.program)).collect();
        if let Some(s) = &mut self.synth {
            for (ch, prog) in progs {
                s.process_midi_message(ch as i32, 0xC0, prog as i32, 0);
            }
        }
    }

    pub fn has_soundfont(&self) -> bool {
        self.synth.is_some()
    }

    /// Stops every sounding note (stop, jump, cycle).
    pub fn all_off(&mut self) {
        if let Some(s) = &mut self.synth {
            s.note_off_all(false);
        }
        self.sounding.clear();
        for v in &mut self.voices {
            v.releasing = true;
        }
    }

    pub fn locate(&mut self, ticks: f64) {
        self.all_off();
        self.pos = ticks.max(0.0);
    }

    fn audible(&self, i: usize) -> bool {
        let any_solo = self.tracks.iter().any(|t| t.solo);
        let t = &self.tracks[i];
        !t.mute && (!any_solo || t.solo)
    }

    /// Renders one block of stereo frames into `out` (interleaved).
    fn render(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        out.fill(0.0);
        if self.meters.len() != self.tracks.len() {
            self.meters = vec![0.0; self.tracks.len()];
        }
        let bpm = self.tempo_map.iter().rev().find(|t| t.0 as f64 <= self.pos).map_or(self.tempo, |t| t.1);
        let tps = bpm / 60.0 * PPQ as f64 / RATE as f64;
        if self.playing {
            let from = self.pos;
            let to = from + tps * frames as f64;
            // notes ending in this block
            let mut ended = Vec::new();
            self.sounding.retain(|&(t, ch, pitch, end)| {
                if (end as f64) < to {
                    ended.push((t, ch, pitch));
                    false
                } else {
                    true
                }
            });
            for (t, ch, pitch) in ended {
                if let Some(s) = &mut self.synth {
                    s.note_off(ch as i32, pitch as i32);
                }
                for v in self.voices.iter_mut().filter(|v| v.track == t && v.pitch == pitch) {
                    v.releasing = true;
                }
            }
            // notes starting in this block
            for i in 0..self.tracks.len() {
                if !self.audible(i) {
                    continue;
                }
                let t = &self.tracks[i];
                let (ch, drum) = (t.channel, t.channel == 9);
                let starts: Vec<(u32, u32, u8, u8)> = t.notes.iter().filter(|n| (n.0 as f64) >= from && (n.0 as f64) < to).copied().collect();
                let gain = t.volume;
                for (s, e, pitch, vel) in starts {
                    let _ = s;
                    let vel = ((vel as f32) * gain.min(1.5)).clamp(1.0, 127.0) as u8;
                    if let Some(sy) = &mut self.synth {
                        sy.note_on(ch as i32, pitch as i32, vel as i32);
                    } else {
                        self.voices.push(Voice { track: i, pitch, phase: 0.0, freq: 440.0 * 2f32.powf((pitch as f32 - 69.0) / 12.0), amp: vel as f32 / 127.0 * 0.25, env: 0.0, releasing: false, drum });
                    }
                    self.sounding.push((i, ch, pitch, e));
                }
            }
            // the click: a beat line crossed in this block
            if self.metronome {
                let beat = PPQ as f64;
                let next = (from / beat).ceil() * beat;
                if next < to {
                    let n = (next / beat).round() as u32;
                    let accent = n % self.beats_per_bar.max(1) == 0;
                    self.click = Some((if accent { 1760.0 } else { 1320.0 }, 0.0));
                }
            }
            self.pos = to;
            if let (true, Some((a, b))) = (self.cycle_on, self.cycle) {
                if b > a && self.pos >= b as f64 {
                    self.all_off();
                    self.pos = a as f64;
                }
            }
        }
        // instruments
        if let Some(s) = &mut self.synth {
            let mut l = vec![0f32; frames];
            let mut r = vec![0f32; frames];
            s.render(&mut l, &mut r);
            for k in 0..frames {
                out[2 * k] += l[k];
                out[2 * k + 1] += r[k];
            }
        } else {
            for v in &mut self.voices {
                for k in 0..frames {
                    v.env = if v.releasing { v.env * 0.9992 } else { (v.env + 0.01).min(1.0) };
                    let x = if v.drum {
                        // a short noisy click per drum note
                        let n = ((v.phase * 12.9898).sin() * 43758.547).fract() * 2.0 - 1.0;
                        v.env *= 0.9985;
                        n
                    } else {
                        // a soft saw
                        let s = 2.0 * v.phase - 1.0;
                        s * 0.6 + (v.phase * std::f32::consts::TAU).sin() * 0.4
                    };
                    v.phase = (v.phase + v.freq / RATE as f32).fract();
                    let y = x * v.amp * v.env;
                    out[2 * k] += y;
                    out[2 * k + 1] += y;
                }
            }
            self.voices.retain(|v| v.env > 0.0005 || !v.releasing);
        }
        // the metronome's click: a short decaying sine
        if let Some((freq, ref mut t)) = self.click {
            for k in 0..frames {
                let env = (-*t * 60.0).exp();
                let y = (std::f32::consts::TAU * freq * *t).sin() * env * 0.35;
                out[2 * k] += y;
                out[2 * k + 1] += y;
                *t += 1.0 / RATE as f32;
            }
            if *t > 0.08 {
                self.click = None;
            }
        }
        // audio regions
        if self.playing {
            let start_tick = self.pos - tps * frames as f64;
            let ticks_per_frame = tps;
            for i in 0..self.tracks.len() {
                if !self.audible(i) {
                    continue;
                }
                let gain = self.tracks[i].volume;
                let mut peak = 0f32;
                for (at, samples) in &self.tracks[i].audio {
                    for k in 0..frames {
                        let tick = start_tick + k as f64 * ticks_per_frame - *at as f64;
                        if tick < 0.0 {
                            continue;
                        }
                        let frame = (tick / ticks_per_frame) as usize;
                        if 2 * frame + 1 >= samples.len() {
                            break;
                        }
                        let (l, r) = (samples[2 * frame] * gain, samples[2 * frame + 1] * gain);
                        out[2 * k] += l;
                        out[2 * k + 1] += r;
                        peak = peak.max(l.abs()).max(r.abs());
                    }
                }
                if let Some(m) = self.meters.get_mut(i) {
                    *m = m.max(peak);
                }
            }
        }
        // the meters: tracks with notes sounding light up by velocity (the synth mixes them)
        for &(t, ..) in &self.sounding {
            if let Some(m) = self.meters.get_mut(t) {
                *m = m.max(0.5);
            }
        }
        let mut peak = 0f32;
        for x in out.iter_mut() {
            *x = (*x * self.master).clamp(-1.0, 1.0);
            peak = peak.max(x.abs());
        }
        self.master_meter = self.master_meter.max(peak);
        let fall = 0.985f32.powf(frames as f32 / 64.0);
        for m in &mut self.meters {
            *m *= fall;
        }
        self.master_meter *= fall;
    }
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

/// Bounce (File > Bounce > Project or Section…): renders `tracks` offline from `from` to `to`
/// (ticks) plus a two-second tail, with the given SoundFont; stereo samples at RATE.
pub fn bounce(tracks: Vec<PlayTrack>, tempo: f64, tempo_map: Vec<(u32, f64)>, soundfont: Option<&Path>, from: u32, to: u32) -> Vec<f32> {
    let mut e = Engine::new();
    e.tempo = tempo;
    e.tempo_map = tempo_map;
    e.tracks = tracks;
    e.load_soundfont(soundfont);
    e.pos = from as f64;
    e.playing = true;
    let mut out = Vec::new();
    let mut buf = vec![0f32; 2 * BLOCK];
    let tps = tempo / 60.0 * PPQ as f64 / RATE as f64;
    let tail = (2.0 * RATE as f64) as usize / BLOCK;
    while e.pos < to as f64 {
        e.render(&mut buf);
        out.extend_from_slice(&buf);
    }
    e.playing = false;
    e.all_off();
    let _ = tps;
    for _ in 0..tail {
        e.render(&mut buf);
        out.extend_from_slice(&buf);
    }
    out
}

/// A 16-bit stereo WAV file of `samples` at RATE.
pub fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for x in samples {
        b.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    b
}

/// The system's General MIDI SoundFont (Linux distributions install one with FluidSynth / TiMidity).
pub fn system_soundfont() -> Option<PathBuf> {
    let candidates = ["/usr/share/sounds/sf2/default-GM.sf2", "/usr/share/sounds/sf2/FluidR3_GM.sf2", "/usr/share/soundfonts/default.sf2", "/usr/share/soundfonts/FluidR3_GM.sf2", "/usr/share/sounds/sf2/TimGM6mb.sf2", "/usr/share/sounds/sf3/default-GM.sf3"];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

/// rodio's side: pulls blocks from the engine.
struct Stream {
    engine: Arc<Mutex<Engine>>,
    buf: Vec<f32>,
    at: usize,
}

impl Iterator for Stream {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.at >= self.buf.len() {
            let mut e = self.engine.lock().unwrap_or_else(|e| e.into_inner());
            e.render(&mut self.buf);
            self.at = 0;
        }
        let x = self.buf[self.at];
        self.at += 1;
        Some(x)
    }
}

impl rodio::Source for Stream {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(RATE).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// The audio output: opened once; the engine plays through it for the app's life.
pub struct Output {
    pub engine: Arc<Mutex<Engine>>,
    _sink: Option<rodio::MixerDeviceSink>,
    pub error: Option<String>,
}

impl Output {
    pub fn open(engine: Arc<Mutex<Engine>>) -> Output {
        // (tests and snapshots: no sound device)
        if std::env::var_os("LG_SNAP").is_some() || std::env::var_os("LG_NOAUDIO").is_some() {
            return Output { engine, _sink: None, error: None };
        }
        match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(mut s) => {
                s.log_on_drop(false);
                s.mixer().add(Stream { engine: engine.clone(), buf: vec![0.0; BLOCK * 2], at: BLOCK * 2 });
                Output { engine, _sink: Some(s), error: None }
            }
            Err(e) => Output { engine, _sink: None, error: Some(e.to_string()) },
        }
    }
}

/// A WAV / AIFF / MP3 / FLAC file as stereo samples at RATE (linear resampling).
pub fn load_audio(path: &Path) -> Option<Vec<f32>> {
    use rodio::Source;
    let f = std::fs::File::open(path).ok()?;
    let d = rodio::Decoder::try_from(f).ok()?;
    let ch = d.channels().get() as usize;
    let rate = d.sample_rate().get() as f64;
    let raw: Vec<f32> = d.collect();
    let frames = raw.len() / ch.max(1);
    let out_frames = (frames as f64 * RATE as f64 / rate) as usize;
    let mut out = Vec::with_capacity(out_frames * 2);
    for k in 0..out_frames {
        let src = k as f64 * rate / RATE as f64;
        let i = src as usize;
        let t = (src - i as f64) as f32;
        let get = |f: usize, c: usize| raw.get(f * ch + c.min(ch - 1)).copied().unwrap_or(0.0);
        for c in 0..2 {
            out.push(get(i, c) * (1.0 - t) + get(i + 1, c) * t);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounces_the_demo_with_the_soundfont() {
        let Some(sf) = system_soundfont() else { return };
        let p = crate::demo::song();
        let tracks: Vec<PlayTrack> = p.tracks.iter().enumerate().map(|(i, t)| PlayTrack {
            channel: if t.kind == lg_core::TrackKind::Drummer { 9 } else { i as u8 },
            program: crate::app::program_for(&t.name, t.kind),
            notes: t.regions.iter().flat_map(|r| r.notes.iter().map(|n| (n.pos, n.pos + n.len, n.pitch, n.velocity))).collect(),
            audio: Vec::new(),
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
        }).collect();
        // two bars at 104 BPM: about 4.6 s, plus the tail
        let out = bounce(tracks, p.tempo, Vec::new(), Some(&sf), 0, 2 * 3840);
        let secs = out.len() as f64 / 2.0 / RATE as f64;
        assert!((secs - (2.0 * 4.0 * 60.0 / 104.0 + 2.0)).abs() < 0.1, "{secs} s");
        let rms = (out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32).sqrt();
        assert!(rms > 0.01, "the bounce has sound: rms {rms}");
        let w = wav_bytes(&out[..2 * RATE as usize]);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(w.len(), 44 + 2 * RATE as usize * 2);
    }

    #[test]
    fn plays_notes_in_time() {
        let mut e = Engine::new();
        e.tempo = 120.0;
        e.tracks = vec![PlayTrack { channel: 0, program: 0, notes: vec![(0, 480, 60, 100), (960, 1440, 64, 100)], audio: Vec::new(), volume: 1.0, pan: 0.0, mute: false, solo: false }];
        e.playing = true;
        let mut buf = vec![0f32; 2 * BLOCK];
        // half a second at 120 BPM = one beat = 960 ticks
        let blocks = (RATE as usize / 2) / BLOCK;
        let mut loud = 0f32;
        for _ in 0..blocks {
            e.render(&mut buf);
            loud = loud.max(buf.iter().fold(0f32, |a, x| a.max(x.abs())));
        }
        assert!((e.pos - 960.0).abs() < 10.0, "a beat played: {}", e.pos);
        assert!(loud > 0.01, "something sounded");
        // muted: silence
        e.locate(0.0);
        e.voices.clear();
        e.tracks[0].mute = true;
        let mut quiet = 0f32;
        for _ in 0..blocks {
            e.render(&mut buf);
            quiet = quiet.max(buf.iter().fold(0f32, |a, x| a.max(x.abs())));
        }
        assert!(quiet < 0.001, "muted track is silent ({quiet})");
    }
}
