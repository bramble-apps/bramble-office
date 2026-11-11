//! Recording (Logic's Record menu, the transport's Record, Count-in, Autopunch, Replace, cycle
//! recording): audio from the system input onto record-enabled audio tracks, written as WAV
//! files into the project's Audio Files as it comes in; MIDI from the keyboards (live.rs) onto
//! the instrument track. Overlapping and cycle recordings follow the Record menu's choices
//! (take folders, merge, new tracks…), Replace its replace choice.
//!
//! The engine side (`EngineRec`, in the audio thread): the input for each block (the sound
//! card's input, or LG_TEST_INPUT=<wav> for checks: never the microphone headless), input
//! monitoring into the tracks, the count-in before bar 1, and the capture (one pass per cycle,
//! only between the punch locators with Autopunch). The app side (`Recording`): the options, the
//! session (files being written, their waveforms), and placing what was recorded.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::events::{Note, PPQ};
use lg_core::{Region, TrackKind};

use crate::app::App;
use crate::audio::RATE;
use crate::look;

/// What an audio track records from (Logic's input slot): the first or second input, or both
/// as a stereo pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InputSel {
    #[default]
    Input1,
    Input2,
    Stereo,
}

impl InputSel {
    pub const ALL: [InputSel; 3] = [InputSel::Input1, InputSel::Input2, InputSel::Stereo];
    pub fn label(self) -> &'static str {
        match self {
            InputSel::Input1 => "Input 1",
            InputSel::Input2 => "Input 2",
            InputSel::Stereo => "Input 1-2",
        }
    }
    pub fn channels(self) -> usize {
        if self == InputSel::Stereo { 2 } else { 1 }
    }
    /// A stereo input frame as this input hears it (a mono input on both sides).
    fn pick(self, l: f32, r: f32) -> (f32, f32) {
        match self {
            InputSel::Input1 => (l, l),
            InputSel::Input2 => (r, r),
            InputSel::Stereo => (l, r),
        }
    }
}

// =============================================================================================
// the engine side

/// Where the input comes from: nothing, the sound card (filled by its own thread), or a file
/// played in as if it were the input (LG_TEST_INPUT; stereo at RATE).
#[derive(Default)]
pub enum Feed {
    #[default]
    None,
    Device(Arc<Mutex<VecDeque<f32>>>),
    File { samples: Arc<Vec<f32>>, at: usize },
}

/// One pass of a recording (a cycle repeat is a new one): its first frame's position (ticks)
/// and the stereo frames not yet taken by the app.
#[derive(Default, Debug)]
pub struct Pass {
    pub start: f64,
    pub data: Vec<f32>,
    pub frames: usize,
}

#[derive(Default)]
pub struct EngineRec {
    pub feed: Feed,
    /// Capturing the input (audio tracks record-enabled and recording).
    pub capturing: bool,
    /// Where recording begins (ticks): nothing before it is kept (the count-in).
    pub from: f64,
    /// Autopunch: only this range is recorded.
    pub punch: Option<(f64, f64)>,
    /// The count-in before bar 1: ticks left while the playhead waits at the start.
    pub hold: f64,
    /// The count-in clicks up to here even with the metronome off.
    pub click_until: Option<f64>,
    pub passes: Vec<Pass>,
    new_pass: bool,
    /// Tracks hearing their input (input monitoring), with the input each hears.
    pub monitored: Vec<(usize, InputSel)>,
    /// The input's peak level (decaying), for the meters.
    pub level: f32,
    /// The Tuner is open: the input is read, its last samples kept (mono) for it.
    pub tuner: bool,
    pub tap: VecDeque<f32>,
}

impl EngineRec {
    /// The input for a block of `frames` (stereo, interleaved), when something needs it.
    pub fn input(&mut self, frames: usize) -> Option<Vec<f32>> {
        let need = self.capturing || !self.monitored.is_empty() || self.tuner;
        let out = match &mut self.feed {
            Feed::None => return None,
            Feed::Device(q) => {
                let mut q = q.lock().unwrap_or_else(|e| e.into_inner());
                if !need {
                    q.clear();
                    return None;
                }
                let mut out = vec![0f32; frames * 2];
                let n = (frames * 2).min(q.len());
                for (o, x) in out.iter_mut().zip(q.drain(..n)) {
                    *o = x;
                }
                // (the input's clock ahead of the output's: the oldest dropped)
                if q.len() > RATE as usize {
                    let extra = q.len() - frames * 8;
                    q.drain(..extra);
                }
                out
            }
            Feed::File { samples, at } => {
                if !need {
                    return None;
                }
                let mut out = vec![0f32; frames * 2];
                let end = (*at + frames * 2).min(samples.len());
                if *at < end {
                    out[..end - *at].copy_from_slice(&samples[*at..end]);
                }
                *at += frames * 2;
                out
            }
        };
        let peak = out.iter().fold(0f32, |m, x| m.max(x.abs()));
        self.level = (self.level * 0.97).max(peak);
        if self.tuner {
            self.tap.extend(out.chunks(2).map(|f| (f[0] + f.get(1).copied().unwrap_or(f[0])) * 0.5));
            let extra = self.tap.len().saturating_sub(8192);
            self.tap.drain(..extra);
        }
        Some(out)
    }

    /// The count-in before bar 1 (the playhead waits at 0 while it clicks): true while it lasts.
    pub fn hold(&mut self, frames: usize, tps: f64, beats_per_bar: u32, click: &mut Option<(f32, f32)>) -> bool {
        if self.hold <= 0.0 {
            return false;
        }
        let beat = PPQ as f64;
        let v0 = -self.hold;
        let v1 = v0 + tps * frames as f64;
        let next = (v0 / beat).ceil() * beat;
        if next < v1 && next < -0.5 {
            let n = (next / beat).round() as i64;
            *click = Some((if n.rem_euclid(beats_per_bar.max(1) as i64) == 0 { 1760.0 } else { 1320.0 }, 0.0));
        }
        self.hold = (self.hold - tps * frames as f64).max(0.0);
        true
    }

    /// The count-in clicks before recording begins, the metronome on or not.
    pub fn click_forced(&self, at: f64) -> bool {
        self.click_until.is_some_and(|u| at < u - 0.5)
    }

    /// The block from `from` (ticks, `tps` a frame) taken into the current pass.
    pub fn capture(&mut self, from: f64, tps: f64, input: Option<&[f32]>) {
        let (true, Some(inp)) = (self.capturing, input) else { return };
        for k in 0..inp.len() / 2 {
            let t = from + k as f64 * tps;
            if t < self.from - 1e-6 {
                continue;
            }
            if let Some((a, b)) = self.punch {
                if t < a || t >= b {
                    if t >= b {
                        self.new_pass = true;
                    }
                    continue;
                }
            }
            if self.new_pass || self.passes.is_empty() {
                self.passes.push(Pass { start: t, data: Vec::new(), frames: 0 });
                self.new_pass = false;
            }
            let p = self.passes.last_mut().unwrap();
            p.data.push(inp[2 * k]);
            p.data.push(inp[2 * k + 1]);
            p.frames += 1;
        }
    }

    /// The cycle went round: the next frames are a new pass.
    pub fn wrapped(&mut self) {
        if self.capturing {
            self.new_pass = true;
        }
    }

    /// Input monitoring: the track's input into its signal. True when it heard any.
    pub fn monitor(&self, track: usize, input: Option<&[f32]>, l: &mut [f32], r: &mut [f32]) -> bool {
        let (Some(inp), Some(&(_, sel))) = (input, self.monitored.iter().find(|m| m.0 == track)) else { return false };
        for k in 0..l.len().min(inp.len() / 2) {
            let (a, b) = sel.pick(inp[2 * k], inp[2 * k + 1]);
            l[k] += a;
            r[k] += b;
        }
        true
    }

    pub fn start(&mut self, from: f64, punch: Option<(f64, f64)>) {
        self.capturing = true;
        self.from = from;
        self.punch = punch;
        self.passes.clear();
        self.new_pass = true;
    }

    pub fn stop(&mut self) {
        self.capturing = false;
        self.hold = 0.0;
        self.click_until = None;
    }
}

/// The sound card's input: its stream (kept open while needed) feeding the engine, as stereo
/// at RATE.
pub struct Device {
    _stream: rodio::cpal::Stream,
    pub queue: Arc<Mutex<VecDeque<f32>>>,
    pub name: String,
}

fn open_device(wanted: Option<&str>) -> Result<Device, String> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use rodio::cpal::{self, SampleFormat};
    let host = cpal::default_host();
    let dev = match wanted {
        Some(n) => host.input_devices().map_err(|e| e.to_string())?.find(|d| d.description().is_ok_and(|x| x.name() == n)),
        None => host.default_input_device(),
    }
    .ok_or_else(|| "No audio input found".to_string())?;
    let name = dev.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "Input".into());
    let cfg = dev.default_input_config().map_err(|e| e.to_string())?;
    let (ch, rate) = (cfg.channels() as usize, cfg.sample_rate() as f64);
    let queue = Arc::new(Mutex::new(VecDeque::new()));
    let config = cfg.config();
    let stream = match cfg.sample_format() {
        SampleFormat::F32 => build::<f32>(&dev, config, ch, rate, queue.clone()),
        SampleFormat::I16 => build::<i16>(&dev, config, ch, rate, queue.clone()),
        SampleFormat::I32 => build::<i32>(&dev, config, ch, rate, queue.clone()),
        SampleFormat::U16 => build::<u16>(&dev, config, ch, rate, queue.clone()),
        f => return Err(format!("Input format {f:?} isn't supported")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Device { _stream: stream, queue, name })
}

fn build<T>(dev: &rodio::cpal::Device, config: rodio::cpal::StreamConfig, ch: usize, rate: f64, q: Arc<Mutex<VecDeque<f32>>>) -> Result<rodio::cpal::Stream, String>
where
    T: rodio::cpal::SizedSample,
    f32: rodio::cpal::FromSample<T>,
{
    use rodio::cpal::traits::DeviceTrait;
    let mut rs = Resampler::new(rate);
    let mut buf = Vec::new();
    dev.build_input_stream::<T, _, _>(
        config,
        move |data: &[T], _| {
            buf.clear();
            for f in data.chunks(ch.max(1)) {
                let l: f32 = f[0].to_sample();
                let r: f32 = if f.len() > 1 { f[1].to_sample() } else { l };
                rs.push(l, r, &mut buf);
            }
            let mut q = q.lock().unwrap_or_else(|e| e.into_inner());
            q.extend(buf.iter().copied());
        },
        |e| eprintln!("audio input: {e}"),
        None,
    )
    .map_err(|e| e.to_string())
}

/// Linear resampling of a stream of stereo frames to RATE.
struct Resampler {
    step: f64,
    t: f64,
    prev: (f32, f32),
}

impl Resampler {
    fn new(rate: f64) -> Resampler {
        Resampler { step: rate / RATE as f64, t: 0.0, prev: (0.0, 0.0) }
    }
    fn push(&mut self, l: f32, r: f32, out: &mut Vec<f32>) {
        while self.t <= 1.0 {
            let k = self.t as f32;
            out.push(self.prev.0 + (l - self.prev.0) * k);
            out.push(self.prev.1 + (r - self.prev.1) * k);
            self.t += self.step;
        }
        self.t -= 1.0;
        self.prev = (l, r);
    }
}

// =============================================================================================
// the app side

/// Record ▸ Count-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountIn {
    Bars(u32),
    Beats(u32),
}

/// Record ▸ Record Button Options.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Button {
    #[default]
    Record,
    Toggle,
    Repeat,
}

/// Record ▸ Overlapping MIDI / Audio Recordings: the choices (Logic's own names).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlap {
    TakeFolder,
    Merge,
    Overlap,
    OverlapMergeSelected,
    NewTrack,
    MergeCurrentOnly,
    NewTracksAndMute,
}

/// Record ▸ Overlapping MIDI Recordings ▸ Replace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplaceMode {
    RegionErase,
    RegionPunch,
    ContentErase,
    ContentPunch,
}

/// One audio track recording: its input, the files of its passes.
struct TrackRec {
    track: usize,
    sel: InputSel,
    name: String,
    files: Vec<PassFile>,
}

struct PassFile {
    start: f64,
    writer: Option<lg_audio::record::WavWriter>,
    path: PathBuf,
    frames: u64,
    /// The waveform so far: the peak of each PEAK frames.
    peaks: Vec<f32>,
    acc: (f32, usize),
}

const PEAK: usize = 256;

pub struct Session {
    /// Where recording begins (ticks) and where playback began (Discard returns there).
    pub from: f64,
    pub play_from: f64,
    pub cycle: Option<(u32, u32)>,
    pub punch: Option<(u32, u32)>,
    audio: Vec<TrackRec>,
    /// The MIDI track recorded onto (None: no instrument track), where each cycle pass's notes
    /// begin in `App::recorded`.
    pub midi: Option<usize>,
    midi_marks: Vec<usize>,
    last_pos: f64,
}

pub struct Recording {
    pub count_in: CountIn,
    pub button: Button,
    pub midi_cycle_off: Overlap,
    pub midi_cycle_on: Overlap,
    pub midi_replace: ReplaceMode,
    pub audio_cycle_off: Overlap,
    pub audio_cycle_on: Overlap,
    pub quick_punch: bool,
    pub auto_monitor: bool,
    pub musical_grid: bool,
    /// The punch locators (ticks).
    pub punch: (u32, u32),
    /// Each audio track's input (Input 1 when not set).
    pub inputs: HashMap<usize, InputSel>,
    pub session: Option<Session>,
    device: Option<Device>,
    device_failed: Option<String>,
    /// Where each file recorded here began (ticks), for Move to Recorded Position.
    pub recorded_at: HashMap<PathBuf, f64>,
    /// The regions the last recording made (track, start), for Record Repeat.
    last: Vec<(usize, u32)>,
    /// Takes: their waveforms (by file), Quick Swipe Comping on, its snapping, the folder in
    /// focus, a take or comp being renamed, a swipe being made.
    pub takes: crate::takes::Ui,
    drag: Option<PunchDrag>,
}

#[derive(Clone, Copy)]
enum PunchDrag {
    In,
    Out,
    Move { from_tick: f32, start: (u32, u32) },
}

impl Default for Recording {
    fn default() -> Self {
        Recording {
            count_in: CountIn::Bars(1),
            button: Button::Record,
            midi_cycle_off: Overlap::Merge,
            midi_cycle_on: Overlap::Merge,
            midi_replace: ReplaceMode::RegionErase,
            audio_cycle_off: Overlap::TakeFolder,
            audio_cycle_on: Overlap::TakeFolder,
            quick_punch: true,
            auto_monitor: true,
            musical_grid: true,
            punch: (4 * 3840, 6 * 3840),
            inputs: HashMap::new(),
            session: None,
            device: None,
            device_failed: None,
            recorded_at: HashMap::new(),
            last: Vec::new(),
            takes: Default::default(),
            drag: None,
        }
    }
}

/// Runs without a sound device (snapshots, scripted checks).
fn headless() -> bool {
    std::env::var_os("LG_SNAP").is_some() || std::env::var_os("LG_NOAUDIO").is_some()
}

fn test_input() -> Option<PathBuf> {
    std::env::var_os("LG_TEST_INPUT").map(PathBuf::from)
}

/// Seconds from bar 1 → ticks, through the tempo changes (seconds_at's inverse).
pub fn tick_at(app: &App, secs: f64) -> f64 {
    let ppq = PPQ as f64;
    let Some(p) = &app.project else { return secs * 2.0 * ppq };
    let (mut at, mut s, mut bpm) = (0.0, 0.0, p.tempo);
    for &(t, b) in &p.tempo_map {
        let t = t as f64;
        if t > at {
            let seg = (t - at) / ppq * 60.0 / bpm;
            if s + seg >= secs {
                break;
            }
            s += seg;
            at = t;
        }
        bpm = b;
    }
    at + (secs - s) * bpm / 60.0 * ppq
}

/// The project's Audio Files folder (Logic's Media/Audio Files in the package); unsaved
/// projects record into Studio's own folder. LG_RECORD_DIR overrides it (checks).
pub fn audio_dir(app: &App) -> PathBuf {
    if let Some(d) = std::env::var_os("LG_RECORD_DIR") {
        return PathBuf::from(d);
    }
    let p = app.project.as_ref();
    if let Some(path) = p.map(|p| p.path.clone()).filter(|p| p.is_dir() && !app.demo) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        return if ext == "logicx" || ext == "band" { path.join("Media").join("Audio Files") } else { path.join("Audio Files") };
    }
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))).unwrap_or_else(std::env::temp_dir);
    let name = p.map_or("Untitled".to_string(), |p| p.name.clone());
    base.join("bramble-studio").join("Recordings").join(name.replace('/', "-")).join("Audio Files")
}

/// "Audio 1#01.wav", the next number free in `dir`.
fn file_name(dir: &Path, track: &str) -> PathBuf {
    let base = track.replace(['/', '\\', ':'], "-");
    (1..).map(|n| dir.join(format!("{base}#{n:02}.wav"))).find(|p| !p.exists()).unwrap()
}

impl App {
    fn engine_lock(&self) -> std::sync::MutexGuard<'_, crate::audio::Engine> {
        self.engine.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub fn count_in_ticks(app: &App) -> f64 {
    let bar = app.bar_ticks() as f64;
    let beat = bar / app.project.as_ref().map_or(4, |p| p.signature.0.max(1) as u32) as f64;
    match app.rec.count_in {
        CountIn::Bars(n) => n as f64 * bar,
        CountIn::Beats(n) => n as f64 * beat,
    }
}

/// The audio tracks a recording goes onto: the record-enabled ones, else (Logic's Auto Record
/// Enable: the focused track) the selected one when it's audio and nothing is enabled.
fn audio_targets(app: &App) -> Vec<usize> {
    let Some(p) = &app.project else { return Vec::new() };
    let mut v: Vec<usize> = app.armed.iter().copied().filter(|&t| p.tracks.get(t).is_some_and(|tr| tr.kind == TrackKind::Audio)).collect();
    v.sort();
    if app.armed.is_empty() {
        if let Some(t) = app.selected_track.filter(|&t| p.tracks.get(t).is_some_and(|tr| tr.kind == TrackKind::Audio)) {
            v.push(t);
        }
    }
    v
}

fn midi_target(app: &App) -> Option<usize> {
    let p = app.project.as_ref()?;
    let ok = |t: usize| p.tracks.get(t).is_some_and(|tr| tr.kind != TrackKind::Audio);
    let armed: Vec<usize> = { let mut a: Vec<usize> = app.armed.iter().copied().filter(|&t| ok(t)).collect(); a.sort(); a };
    armed.first().copied().or_else(|| app.armed.is_empty().then_some(app.selected_track).flatten().filter(|&t| ok(t)))
}

/// The input feed set up for the engine: the test file, else the sound card (never headless).
pub(crate) fn ensure_feed(app: &mut App) -> bool {
    if let Some(path) = test_input() {
        let has = matches!(app.engine_lock().rec.feed, Feed::File { .. });
        if !has {
            let Some(samples) = crate::audio::load_audio(&path) else {
                app.tell(format!("LG_TEST_INPUT: can't read {}", path.display()));
                return false;
            };
            app.engine_lock().rec.feed = Feed::File { samples: Arc::new(samples), at: 0 };
        }
        return true;
    }
    if headless() {
        return false;
    }
    if app.rec.device.is_none() && app.rec.device_failed.is_none() {
        let wanted = app.logic_settings.0.get("Audio / Devices / Input Device:").cloned().filter(|n| n != "System Default");
        match open_device(wanted.as_deref()) {
            Ok(d) => {
                app.engine_lock().rec.feed = Feed::Device(d.queue.clone());
                app.rec.device = Some(d);
            }
            Err(e) => {
                app.tell(format!("Audio input: {e}"));
                app.rec.device_failed = Some(e);
            }
        }
    }
    app.rec.device.is_some()
}

/// The Record button / key (R): Logic's Record Button Options.
pub fn record_pressed(app: &mut App, name: &str) {
    let recording = app.recording() || app.rec.session.is_some();
    if !recording {
        start(app);
        return;
    }
    let mode = match name {
        "Record Toggle" | "Record / Record Toggle" => Button::Toggle,
        "Record Repeat" | "Record / Record Repeat" => Button::Repeat,
        _ => app.rec.button,
    };
    match mode {
        Button::Repeat => repeat(app),
        // Record while recording: punches out, playback goes on (Logic's Record Toggle and the
        // default button alike)
        _ => finish(app),
    }
}

pub fn start(app: &mut App) {
    if app.project.is_none() {
        return;
    }
    let playing = app.playing();
    if playing && !app.rec.quick_punch {
        app.tell("Allow Quick Punch-In is off");
        return;
    }
    let pos = app.position();
    let cycle = app.cycle_on().then_some(app.cycle).filter(|c| c.1 > c.0);
    let from = match cycle {
        Some((a, b)) if !playing && (pos < a as f64 || pos >= b as f64) => a as f64,
        _ => pos,
    };
    let punch = app.autopunch.then_some(app.rec.punch).filter(|p| p.1 > p.0);
    let audio = audio_targets(app);
    let midi = midi_target(app);
    if audio.is_empty() && midi.is_none() {
        app.tell("No track is record-enabled");
        return;
    }
    if !audio.is_empty() && !ensure_feed(app) {
        app.tell("No audio input: recording MIDI only");
    }
    let have_input = !matches!(app.engine_lock().rec.feed, Feed::None);
    let names: Vec<String> = audio.iter().map(|&t| app.project.as_ref().map_or(String::new(), |p| p.tracks[t].name.clone())).collect();
    let audio: Vec<TrackRec> = if have_input { audio.iter().zip(names).map(|(&t, name)| TrackRec { track: t, sel: app.rec.inputs.get(&t).copied().unwrap_or_default(), name, files: Vec::new() }).collect() } else { Vec::new() };
    app.record_from = Some(from);
    app.recorded.clear();
    let count = if !playing && app.count_in { count_in_ticks(app) } else { 0.0 };
    {
        let mut e = app.engine_lock();
        if !audio.is_empty() {
            e.rec.start(from, punch.map(|(a, b)| (a as f64, b as f64)));
        }
        if !playing {
            if count > 0.0 {
                let pre = from - count;
                e.locate(pre.max(0.0));
                e.rec.hold = (-pre).max(0.0);
                e.rec.click_until = Some(from);
            } else {
                e.locate(from);
            }
            e.playing = true;
        }
    }
    app.rec.session = Some(Session { from, play_from: pos, cycle, punch, audio, midi, midi_marks: vec![0], last_pos: from });
}

/// Record Repeat: the last recording thrown away, recording again from where it began.
fn repeat(app: &mut App) {
    let from = app.rec.session.as_ref().map(|s| s.play_from).or(app.record_from);
    discard_session(app);
    app.stop();
    if let Some(f) = from {
        app.locate(f);
    }
    start(app);
}

/// Each frame: the input device opened while a track monitors or records (closed after), the
/// engine driven offline when there's no sound device but a test input, what was captured
/// written to the files, the cycle passes of a MIDI recording, a stop ending the recording.
pub fn update(app: &mut App) {
    crate::takes::tidy(app);
    // input monitoring: the I buttons, and (Auto Input Monitoring) record-enabled audio tracks
    // while recording
    let recording_audio = app.rec.session.as_ref().is_some_and(|s| !s.audio.is_empty());
    let mut mon: Vec<(usize, InputSel)> = app.monitoring.iter().map(|&t| (t, app.rec.inputs.get(&t).copied().unwrap_or_default())).collect();
    if recording_audio && app.rec.auto_monitor {
        for t in app.rec.session.as_ref().map(|s| s.audio.iter().map(|a| (a.track, a.sel)).collect::<Vec<_>>()).unwrap_or_default() {
            if !mon.iter().any(|m| m.0 == t.0) {
                mon.push(t);
            }
        }
    }
    let is_audio = |t: usize| app.project.as_ref().is_some_and(|p| p.tracks.get(t).is_some_and(|tr| tr.kind == TrackKind::Audio));
    mon.retain(|m| is_audio(m.0));
    mon.sort_by_key(|m| m.0);
    if !mon.is_empty() {
        ensure_feed(app);
    }
    let needs_device = !mon.is_empty() || recording_audio;
    {
        let mut e = app.engine_lock();
        if e.rec.monitored != mon {
            e.rec.monitored = mon;
        }
    }
    if !needs_device && app.rec.device.is_some() {
        app.rec.device = None;
        app.engine_lock().rec.feed = Feed::None;
    }
    if !needs_device {
        app.rec.device_failed = None;
    }
    let Some(_) = app.rec.session.as_ref() else { return };
    // headless with a test input: the engine runs here, a tenth of a second a frame
    if headless() && test_input().is_some() {
        let step = std::env::var("LG_TEST_STEP").ok().and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.1);
        let mut buf = vec![0f32; 128];
        let blocks = (step * RATE as f64 / 64.0).round() as usize;
        let mut e = app.engine_lock();
        for _ in 0..blocks {
            if !e.playing {
                break;
            }
            e.render(&mut buf);
        }
    }
    drain(app);
    let (pos, playing, counting) = {
        let e = app.engine_lock();
        (e.pos, e.playing, e.rec.hold > 0.0 || e.rec.click_until.is_some_and(|u| e.pos < u - 0.5))
    };
    let n = app.recorded.len();
    if let Some(s) = app.rec.session.as_mut() {
        if s.cycle.is_some() && pos + 1.0 < s.last_pos && !counting {
            s.midi_marks.push(n);
        }
        s.last_pos = pos;
    }
    if !playing {
        finish(app);
    } else {
        app.ctx.request_repaint();
    }
}

/// What the engine captured since the last frame, into each recording track's files.
fn drain(app: &mut App) {
    let passes: Vec<(usize, f64, Vec<f32>)> = {
        let mut e = app.engine_lock();
        e.rec.passes.iter_mut().enumerate().filter(|(_, p)| !p.data.is_empty()).map(|(i, p)| (i, p.start, std::mem::take(&mut p.data))).collect()
    };
    if passes.is_empty() {
        return;
    }
    let dir = audio_dir(app);
    let bits = match app.logic_settings.get("Recording / main / Bit Depth:").as_str() {
        "16-bit" => 16,
        "32-bit Float" => 32,
        _ => 24,
    };
    let Some(s) = app.rec.session.as_mut() else { return };
    let mut failed = None;
    for tr in &mut s.audio {
        for (pi, start, data) in &passes {
            while tr.files.len() <= *pi {
                let _ = std::fs::create_dir_all(&dir);
                let path = file_name(&dir, &tr.name);
                let writer = match lg_audio::record::WavWriter::create(&path, RATE, tr.sel.channels(), bits) {
                    Ok(w) => Some(w),
                    Err(e) => {
                        failed = Some(format!("Can't write {}: {e}", path.display()));
                        None
                    }
                };
                tr.files.push(PassFile { start: *start, writer, path, frames: 0, peaks: Vec::new(), acc: (0.0, 0) });
            }
            let f = &mut tr.files[*pi];
            let ch = tr.sel.channels();
            let mut out = Vec::with_capacity(data.len() / 2 * ch);
            for fr in data.chunks(2) {
                let (a, b) = tr.sel.pick(fr[0], fr[1]);
                out.push(a);
                if ch == 2 {
                    out.push(b);
                }
                f.acc.0 = f.acc.0.max(a.abs()).max(b.abs());
                f.acc.1 += 1;
                if f.acc.1 == PEAK {
                    f.peaks.push(f.acc.0);
                    f.acc = (0.0, 0);
                }
            }
            f.frames += (data.len() / 2) as u64;
            if let Some(w) = f.writer.as_mut() {
                if let Err(e) = w.write(&out) {
                    failed = Some(format!("Can't write {}: {e}", f.path.display()));
                    f.writer = None;
                }
            }
        }
    }
    if let Some(m) = failed {
        app.tell(m);
    }
}

/// Throws the recording away (its files deleted): Discard, Record Repeat.
fn discard_session(app: &mut App) {
    {
        let mut e = app.engine_lock();
        e.rec.stop();
        e.rec.passes.clear();
    }
    if let Some(s) = app.rec.session.take() {
        for tr in s.audio {
            for f in tr.files {
                drop(f.writer);
                let _ = std::fs::remove_file(&f.path);
            }
        }
    }
    app.recorded.clear();
    app.record_from = None;
}

/// Discard Recording and Return to Last Play Position (⌘.).
pub fn discard(app: &mut App) {
    let back = app.rec.session.as_ref().map(|s| s.play_from).or(app.record_from);
    discard_session(app);
    app.stop();
    if let Some(b) = back {
        app.locate(b);
    }
}

/// Recording ends (Stop, Record again): the files closed, what was recorded placed on the
/// tracks as the Record menu's choices say, one undo step.
pub fn finish(app: &mut App) {
    let Some(mut s) = app.rec.session.take() else {
        // (MIDI recording begun some other way)
        if let Some(from) = app.record_from.take() {
            let notes = std::mem::take(&mut app.recorded);
            midi_take(app, from as u32, notes, "Record");
        }
        return;
    };
    app.engine_lock().rec.stop();
    // the session back for the last of the audio
    app.rec.session = Some(s);
    drain(app);
    s = app.rec.session.take().unwrap();
    app.engine_lock().rec.passes.clear();
    let notes = std::mem::take(&mut app.recorded);
    app.record_from = None;
    let tempo_ok = app.project.is_some();
    if !tempo_ok {
        return;
    }
    // the audio: each pass a region on its file
    let mut audio: Vec<(usize, Vec<Region>)> = Vec::new();
    for tr in std::mem::take(&mut s.audio) {
        let mut regions = Vec::new();
        for f in tr.files {
            if let Some(w) = f.writer {
                let _ = w.finish();
            }
            if f.frames < 64 {
                let _ = std::fs::remove_file(&f.path);
                continue;
            }
            let secs = f.frames as f64 / RATE as f64;
            let start_s = app.seconds_at(f.start);
            let mut end = tick_at(app, start_s + secs);
            // (a cycle pass: no longer than the cycle)
            if let Some((_, b)) = s.cycle.filter(|_| s.punch.is_none()) {
                end = end.min(b as f64).max(f.start + 1.0);
            }
            let start = f.start.round() as u32;
            let length = ((end - f.start).round() as u32).max(1);
            app.rec.recorded_at.insert(f.path.clone(), f.start);
            regions.push(Region { name: tr.name.clone(), start, length, notes: Vec::new(), audio: Some(lg_core::project::AudioRef { file: f.path.clone(), seconds: secs }), muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
        }
        if !regions.is_empty() {
            audio.push((tr.track, regions));
        }
    }
    // the MIDI: the notes of each pass (from where recording began, inside the punch range)
    let mut midi_passes: Vec<Vec<Note>> = Vec::new();
    if s.midi.is_some() {
        let mut marks = s.midi_marks.clone();
        marks.push(notes.len());
        for w in marks.windows(2) {
            let part: Vec<Note> = notes[w[0].min(notes.len())..w[1].min(notes.len())].iter().copied().filter(|n| n.pos as f64 >= s.from - 1.0 && s.punch.is_none_or(|(a, b)| n.pos >= a && n.pos < b)).collect();
            if !part.is_empty() {
                midi_passes.push(part);
            }
        }
    }
    if audio.is_empty() && midi_passes.is_empty() {
        return;
    }
    crate::audioedit::checkpoint(app, "Record");
    app.rec.last.clear();
    for (t, regions) in audio {
        place_audio(app, t, regions, s.cycle);
    }
    if let (Some(t), false) = (s.midi, midi_passes.is_empty()) {
        let range = midi_range(app, &s, &midi_passes);
        place_midi(app, t, midi_passes, range, s.cycle.is_some());
    }
    app.after_edit();
}

/// A MIDI recording's region range: the cycle, the punch range, else from the bar recording
/// began to the bar after the last note (Logic).
fn midi_range(app: &App, s: &Session, passes: &[Vec<Note>]) -> (u32, u32) {
    if let Some(p) = s.punch {
        return p;
    }
    if let Some(c) = s.cycle {
        return c;
    }
    let first = passes.iter().flatten().map(|n| n.pos).min().unwrap_or(s.from as u32);
    let end = passes.iter().flatten().map(|n| n.pos + n.len).max().unwrap_or(first + 1);
    // Use Musical Grid off: the region from where recording began to the last note's end
    if !app.rec.musical_grid {
        let start = (s.from as u32).min(first);
        return (start, end.max(start + 1));
    }
    let bar = app.bar_ticks();
    let start = ((s.from as u32).min(first) / bar) * bar;
    (start, start + end.saturating_sub(start).div_ceil(bar).max(1) * bar)
}

/// Capture as Recording and MIDI recordings begun outside a session: the notes as a region.
pub fn midi_take(app: &mut App, from: u32, notes: Vec<Note>, what: &str) {
    if notes.is_empty() {
        return;
    }
    let Some(t) = midi_target(app).or(app.selected_track) else { return };
    if app.project.as_ref().and_then(|p| p.tracks.get(t)).is_none_or(|tr| tr.kind == TrackKind::Audio) {
        return;
    }
    let bar = app.bar_ticks();
    let start = (from.min(notes.iter().map(|n| n.pos).min().unwrap_or(from)) / bar) * bar;
    let end = notes.iter().map(|n| n.pos + n.len).max().unwrap_or(start + bar);
    let range = (start, start + end.saturating_sub(start).div_ceil(bar).max(1) * bar);
    crate::audioedit::checkpoint(app, what);
    place_midi(app, t, vec![notes], range, false);
    app.after_edit();
}

fn region_of(name: &str, (a, b): (u32, u32), notes: Vec<Note>) -> Region {
    Region { name: name.to_string(), start: a, length: b - a, notes, audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() }
}

/// A new track like `t` (its kind, sound and instrument), at the end of the list.
pub fn new_track_like(app: &mut App, t: usize) -> Option<usize> {
    let p = app.project.as_mut()?;
    let src = p.tracks.get(t)?.clone();
    let row = p.tracks.len() as u32 + 1;
    p.tracks.push(lg_core::Track { name: src.name.clone(), kind: src.kind, channel: 0, row, regions: Vec::new(), volume_db: src.volume_db, pan: src.pan, mute: false, solo: false, plugins: src.plugins.clone(), automation: Vec::new(), extra: Default::default() });
    let n = p.tracks.len() - 1;
    let prog = app.programs.get(t).copied().unwrap_or(0);
    while app.programs.len() < n {
        app.programs.push(0);
    }
    app.programs.push(prog);
    if let Some(c) = app.instrument_choice.get(&t).cloned() {
        app.instrument_choice.insert(n, c);
    }
    if let Some(i) = app.rec.inputs.get(&t).copied() {
        app.rec.inputs.insert(n, i);
    }
    Some(n)
}

fn push_region(app: &mut App, t: usize, r: Region) {
    app.rec.last.push((t, r.start));
    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
        tr.regions.push(r);
        app.selected_region = Some((t, tr.regions.len() - 1));
        app.selected_track = Some(t);
    }
}

/// Indexes of the regions on `t` overlapping [a, b) (not a take folder's).
fn overlapping(app: &App, t: usize, a: u32, b: u32) -> Vec<usize> {
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return Vec::new() };
    tr.regions.iter().enumerate().filter(|(ri, r)| r.start < b && r.start + r.length > a && !crate::takes::owned(app, t, *ri)).map(|(i, _)| i).collect()
}

/// The MIDI recording's passes placed on track `t` over `range`.
fn place_midi(app: &mut App, t: usize, passes: Vec<Vec<Note>>, range: (u32, u32), cycle: bool) {
    let name = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(String::new(), |tr| tr.name.clone());
    let (a, b) = range;
    let all: Vec<Note> = passes.iter().flatten().copied().collect();
    // a take folder there: each pass a new take
    if let Some(fi) = crate::takes::folder_at(app, t, a, b) {
        crate::takes::add_takes(app, fi, passes.into_iter().map(|n| region_of(&name, range, n)).collect());
        return;
    }
    if app.replace_mode {
        let first = all.iter().map(|n| n.pos).min().unwrap_or(a);
        let last = all.iter().map(|n| n.pos + n.len).max().unwrap_or(b);
        replace_midi(app, t, (a, b), (first, last));
        push_region(app, t, region_of(&name, range, all));
        return;
    }
    let mode = if cycle && passes.len() > 1 { app.rec.midi_cycle_on } else { app.rec.midi_cycle_off };
    let over = overlapping(app, t, a, b);
    match mode {
        Overlap::TakeFolder if cycle && passes.len() > 1 || !over.is_empty() => {
            let takes = passes.into_iter().map(|n| region_of(&name, range, n)).collect();
            crate::takes::make_folder(app, t, over, takes);
        }
        Overlap::NewTrack | Overlap::NewTracksAndMute if cycle && passes.len() > 1 => {
            let n = passes.len();
            for (k, notes) in passes.into_iter().enumerate() {
                let tt = if k == 0 { t } else { new_track_like(app, t).unwrap_or(t) };
                let mut r = region_of(&name, range, notes);
                r.muted = mode == Overlap::NewTracksAndMute && k + 1 < n;
                push_region(app, tt, r);
            }
        }
        Overlap::NewTrack if !over.is_empty() => {
            let tt = new_track_like(app, t).unwrap_or(t);
            push_region(app, tt, region_of(&name, range, all));
        }
        Overlap::Merge if !over.is_empty() => merge_into(app, t, over[0], all, range),
        Overlap::OverlapMergeSelected => {
            match app.selected_regions.iter().find(|s| s.0 == t && over.contains(&s.1)).copied() {
                Some((_, r)) => merge_into(app, t, r, all, range),
                None => push_region(app, t, region_of(&name, range, all)),
            }
        }
        _ => push_region(app, t, region_of(&name, range, all)),
    }
}

fn merge_into(app: &mut App, t: usize, r: usize, notes: Vec<Note>, (a, b): (u32, u32)) {
    let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) else { return };
    let end = (rg.start + rg.length).max(b);
    rg.start = rg.start.min(a);
    rg.length = end - rg.start;
    rg.notes.extend(notes);
    rg.notes.sort_by_key(|n| n.pos);
    app.selected_region = Some((t, r));
}

/// Replace mode, MIDI (Record ▸ Overlapping MIDI Recordings ▸ Replace): Region Erase removes
/// the regions the recording overlaps, Region Punch cuts them at the recording's edges, Content
/// Erase removes their notes in the recording's range, Content Punch only between the first and
/// last note recorded.
fn replace_midi(app: &mut App, t: usize, (a, b): (u32, u32), (first, last): (u32, u32)) {
    let mode = app.rec.midi_replace;
    let over = overlapping(app, t, a, b);
    let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return };
    let mut keep: Vec<Region> = Vec::new();
    let mut gone = Vec::new();
    for &i in over.iter().rev() {
        let r = tr.regions[i].clone();
        match mode {
            ReplaceMode::RegionErase => {
                gone.push(i);
            }
            ReplaceMode::RegionPunch => {
                gone.push(i);
                let end = r.start + r.length;
                if r.start < a {
                    let mut left = r.clone();
                    left.length = a - r.start;
                    left.notes.retain(|n| n.pos < a);
                    keep.push(left);
                }
                if end > b {
                    let mut right = r.clone();
                    right.source = None;
                    right.origin = r.source.or(r.origin);
                    right.start = b;
                    right.length = end - b;
                    right.notes.retain(|n| n.pos >= b);
                    keep.push(right);
                }
            }
            ReplaceMode::ContentErase | ReplaceMode::ContentPunch => {
                let (x, y) = if mode == ReplaceMode::ContentErase { (a, b) } else { (first, last) };
                tr.regions[i].notes.retain(|n| n.pos < x || n.pos >= y);
            }
        }
    }
    gone.sort();
    for i in gone.into_iter().rev() {
        tr.regions.remove(i);
    }
    tr.regions.extend(keep);
}

/// The audio recording's passes placed on track `t`.
fn place_audio(app: &mut App, t: usize, regions: Vec<Region>, cycle: Option<(u32, u32)>) {
    let a = regions.iter().map(|r| r.start).min().unwrap_or(0);
    let b = regions.iter().map(|r| r.start + r.length).max().unwrap_or(a);
    if let Some(fi) = crate::takes::folder_at(app, t, a, b) {
        crate::takes::add_takes(app, fi, regions);
        return;
    }
    if app.replace_mode {
        crate::takes::cut_out(app, t, a, b);
        let last = regions.into_iter().last();
        if let Some(r) = last {
            push_region(app, t, r);
        }
        return;
    }
    let several = cycle.is_some() && regions.len() > 1;
    let mode = if several { app.rec.audio_cycle_on } else { app.rec.audio_cycle_off };
    let over = overlapping(app, t, a, b);
    match mode {
        Overlap::NewTracksAndMute if several => {
            let n = regions.len();
            for (k, mut r) in regions.into_iter().enumerate() {
                let tt = if k == 0 { t } else { new_track_like(app, t).unwrap_or(t) };
                r.muted = k + 1 < n;
                push_region(app, tt, r);
            }
        }
        Overlap::NewTrack if !over.is_empty() => {
            let tt = new_track_like(app, t).unwrap_or(t);
            for r in regions {
                push_region(app, tt, r);
            }
        }
        _ if several || !over.is_empty() => crate::takes::make_folder(app, t, over, regions),
        _ => {
            for r in regions {
                push_region(app, t, r);
            }
        }
    }
}

// =============================================================================================
// commands

/// Menu items and key commands run here (menus.rs enables them).
pub const COMMANDS: &[&str] = &[
    "Use Musical Grid",
    "Allow Quick Punch-In",
    "Auto Input Monitoring",
    "Recording Settings…",
    "Record Toggle",
    "Record Repeat",
    "Record / Record Repeat",
    "Set Punch In Locator by Playhead",
    "Set Punch In Locator by Rounded Playhead",
    "Set Punch Out Locator by Playhead",
    "Set Punch Out Locator Point by Rounded Playhead",
    "Set Punch Locators by Regions/Events/Marquee",
    "Set Rounded Punch Locators by Regions/Events",
    "Set Punch Locators by Cycle Locators",
    "Set Rounded Locators and Record",
    "Set Rounded Locators and Cycle Record",
    "Record off for all",
    "Toggle Allow Quick Punch-in",
    "Toggle Auto Input Monitoring",
    "Toggle Track Record Enable",
    "Toggle Channel Strip Record Enable",
    "Toggle Channel Strip Input Monitoring",
    "Open Recording Settings…",
    "Move Region to Recorded Position",
    "Move to Recorded Position",
    "To Recorded Position",
];

/// The Record menu's choices drawn by `menu_ui` (for tool/checklist.py; the labels other menus
/// also use, such as "Merge" or "1 Bar", left out so they aren't counted for those).
pub const CHOICES: &[&str] = &["None", "5 Bars", "2/4", "3/4", "4/4", "5/4", "6/4", "7/4", "8/4", "9/4", "Record / Record Repeat", "Create Take Folder", "Overlap/Merge Selected Regions", "Create Track", "Merge Current Recording Only", "Create Tracks", "Create Tracks and Mute", "Region Erase", "Region Punch", "Content Erase", "Content Punch", "Create New Track", "Create Tracks And Mute", "Count-in", "Record Button Options", "Overlapping MIDI Recordings", "Overlapping Audio Recordings"];

pub fn checked(app: &App, label: &str) -> Option<bool> {
    Some(match label {
        "Use Musical Grid" => app.rec.musical_grid,
        "Allow Quick Punch-In" => app.rec.quick_punch,
        "Auto Input Monitoring" => app.rec.auto_monitor,
        _ => return crate::takes::checked(app, label),
    })
}

/// Runs a recording command; false when it isn't one.
pub fn command(app: &mut App, name: &str) -> bool {
    let bar = app.bar_ticks();
    let pos = app.position();
    match name {
        "Record Toggle" | "Record Repeat" | "Record / Record Repeat" => record_pressed(app, name),
        "Use Musical Grid" => app.rec.musical_grid = !app.rec.musical_grid,
        "Allow Quick Punch-In" | "Toggle Allow Quick Punch-in" => app.rec.quick_punch = !app.rec.quick_punch,
        "Auto Input Monitoring" | "Toggle Auto Input Monitoring" => app.rec.auto_monitor = !app.rec.auto_monitor,
        "Recording Settings…" | "Open Recording Settings…" => {
            app.settings_tab = "Recording".into();
            app.settings_sub.clear();
            app.show_settings = true;
        }
        "Set Punch In Locator by Playhead" | "Set Punch In Locator by Rounded Playhead" => {
            let at = if name.contains("Rounded") { ((pos / bar as f64).round() as u32) * bar } else { pos as u32 };
            let out = app.rec.punch.1.max(at + 1);
            set_punch(app, (at, out));
        }
        "Set Punch Out Locator by Playhead" | "Set Punch Out Locator Point by Rounded Playhead" => {
            let at = if name.contains("Rounded") { ((pos / bar as f64).round() as u32) * bar } else { pos as u32 };
            let inn = app.rec.punch.0.min(at.saturating_sub(1));
            set_punch(app, (inn, at.max(inn + 1)));
        }
        "Set Punch Locators by Regions/Events/Marquee" | "Set Rounded Punch Locators by Regions/Events" => {
            let Some(r) = selection_range(app) else { return true };
            let r = if name.contains("Rounded") { (r.0 / bar * bar, r.1.div_ceil(bar) * bar) } else { r };
            set_punch(app, r);
        }
        "Set Punch Locators by Cycle Locators" => {
            let c = app.cycle;
            set_punch(app, c);
        }
        "Set Rounded Locators and Record" | "Set Rounded Locators and Cycle Record" => {
            if let Some((a, b)) = selection_range(app) {
                app.set_cycle((a / bar * bar, b.div_ceil(bar) * bar));
            }
            let want_cycle = name.contains("Cycle");
            if app.cycle_on() != want_cycle {
                app.toggle_cycle();
            }
            let c = app.cycle;
            app.locate(c.0 as f64);
            start(app);
        }
        "Record off for all" => app.armed.clear(),
        "Toggle Track Record Enable" | "Toggle Channel Strip Record Enable" => {
            let Some(t) = app.selected_track else { return true };
            if !app.armed.remove(&t) {
                app.armed.insert(t);
            }
        }
        "Toggle Channel Strip Input Monitoring" => {
            let Some(t) = app.selected_track else { return true };
            if !app.monitoring.remove(&t) {
                app.monitoring.insert(t);
            }
        }
        "Move Region to Recorded Position" | "Move to Recorded Position" | "To Recorded Position" => move_to_recorded(app),
        _ => return crate::takes::command(app, name),
    }
    true
}

pub fn set_punch(app: &mut App, (a, b): (u32, u32)) {
    if b > a {
        app.rec.punch = (a, b);
        if !app.autopunch {
            app.autopunch = true;
        }
    }
}

fn selection_range(app: &App) -> Option<(u32, u32)> {
    let p = app.project.as_ref()?;
    let sel = app.region_selection();
    let rs: Vec<&Region> = sel.iter().filter_map(|&(t, r)| p.tracks.get(t)?.regions.get(r)).collect();
    Some((rs.iter().map(|r| r.start).min()?, rs.iter().map(|r| r.start + r.length).max()?))
}

/// Move Region to Recorded Position: the selected regions recorded here back where their files
/// began when recorded (their offset into the file kept).
fn move_to_recorded(app: &mut App) {
    let sel = app.region_selection();
    let mut moves = Vec::new();
    for (t, r) in sel {
        let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)) else { continue };
        let Some(at) = rg.audio.as_ref().and_then(|a| app.rec.recorded_at.get(&a.file)) else { continue };
        let off = crate::audioedit::extras(app, t, r).offset as f64 / RATE as f64;
        let start = tick_at(app, app.seconds_at(*at) + off).round() as u32;
        if start != rg.start {
            moves.push((t, r, start));
        }
    }
    if moves.is_empty() {
        return;
    }
    crate::audioedit::checkpoint(app, "Move to Recorded Position");
    for (t, r, start) in moves {
        if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
            rg.start = start;
        }
    }
    app.after_edit();
}

/// The Record menu's submenus with choices (Count-in, Record Button Options, Overlapping MIDI /
/// Audio Recordings), drawn here: their items share names, so each runs by its place. True
/// when `it` was one of them.
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    let which = match it.label.as_str() {
        "Count-in" | "Record Button Options" | "Overlapping MIDI Recordings" | "Overlapping Audio Recordings" => it.label.clone(),
        _ => return false,
    };
    ui.menu_button(&it.label, |ui| {
        ui.set_min_width(230.0);
        let mut part = 0;
        for c in &it.children {
            if c.separator {
                ui.separator();
                part += 1;
                continue;
            }
            // headings ("Cycle Off", "Cycle On", "Replace") are shown greyed, as Logic's
            if matches!(c.label.as_str(), "Cycle Off" | "Cycle On" | "Replace") {
                ui.add_enabled(false, egui::Button::new(&c.label));
                continue;
            }
            let (on, can) = choice(app, &which, part, &c.label, false);
            let b = egui::Button::new(if on { format!("✓ {}", c.label) } else { format!("   {}", c.label) });
            if ui.add_enabled(can, b).clicked() {
                choice(app, &which, part, &c.label, true);
                ui.close();
            }
        }
    });
    true
}

/// A choice in one of the Record submenus: (checked, available); `set` picks it.
fn choice(app: &mut App, menu: &str, part: usize, label: &str, set: bool) -> (bool, bool) {
    match menu {
        "Count-in" => {
            let v = if label == "None" {
                None
            } else if let Some(n) = label.strip_suffix(" Bar").or_else(|| label.strip_suffix(" Bars")) {
                n.parse().ok().map(CountIn::Bars)
            } else if let Some(n) = label.strip_suffix("/4") {
                n.parse().ok().map(CountIn::Beats)
            } else {
                return (false, false);
            };
            if set {
                match v {
                    None => app.count_in = false,
                    Some(c) => {
                        app.rec.count_in = c;
                        app.count_in = true;
                    }
                }
            }
            (if app.count_in { v == Some(app.rec.count_in) } else { v.is_none() }, true)
        }
        "Record Button Options" => {
            let b = match label {
                "Record" => Button::Record,
                "Record / Record Toggle" => Button::Toggle,
                "Record / Record Repeat" => Button::Repeat,
                _ => return (false, false),
            };
            if set {
                app.rec.button = b;
            }
            (app.rec.button == b, true)
        }
        "Overlapping MIDI Recordings" => {
            if part == 2 {
                let m = match label {
                    "Region Erase" => ReplaceMode::RegionErase,
                    "Region Punch" => ReplaceMode::RegionPunch,
                    "Content Erase" => ReplaceMode::ContentErase,
                    "Content Punch" => ReplaceMode::ContentPunch,
                    _ => return (false, false),
                };
                if set {
                    app.rec.midi_replace = m;
                }
                return (app.rec.midi_replace == m, true);
            }
            let o = match label {
                "Create Take Folder" => Overlap::TakeFolder,
                "Merge" => Overlap::Merge,
                "Overlap" => Overlap::Overlap,
                "Overlap/Merge Selected Regions" => Overlap::OverlapMergeSelected,
                "Create Track" | "Create Tracks" => Overlap::NewTrack,
                "Merge Current Recording Only" => Overlap::MergeCurrentOnly,
                "Create Tracks and Mute" => Overlap::NewTracksAndMute,
                // (track alternatives aren't built yet)
                _ => return (false, false),
            };
            let slot = if part == 0 { &mut app.rec.midi_cycle_off } else { &mut app.rec.midi_cycle_on };
            if set {
                *slot = o;
            }
            (*slot == o, true)
        }
        "Overlapping Audio Recordings" => {
            let o = match label {
                "Create Take Folder" => Overlap::TakeFolder,
                "Create New Track" => Overlap::NewTrack,
                "Create Tracks And Mute" => Overlap::NewTracksAndMute,
                _ => return (false, false),
            };
            let slot = if part == 0 { &mut app.rec.audio_cycle_off } else { &mut app.rec.audio_cycle_on };
            if set {
                *slot = o;
            }
            (*slot == o, true)
        }
        _ => (false, false),
    }
}

// =============================================================================================
// the tracks area: the punch locators in the ruler, the recording as it grows

/// Autopunch's red strip in the ruler (between the cycle area and the bar marks): drag its
/// ends to set punch in / out, its middle to move it.
pub fn ruler(app: &mut App, ui: &mut egui::Ui, ruler: Rect, x0: f32, scroll_ticks: f32, ppt: f32) {
    if !app.autopunch {
        return;
    }
    let x = |t: f32| x0 + (t - scroll_ticks) * ppt;
    let to_tick = |x: f32| (x - x0) / ppt + scroll_ticks;
    let (a, b) = app.rec.punch;
    let band = Rect::from_min_max(pos2(x(a as f32), ruler.min.y + 17.0), pos2(x(b as f32), ruler.min.y + 23.0));
    let p = ui.painter_at(ruler);
    p.rect_filled(band, 2.0, look::record_red().gamma_multiply(0.85));
    let edge = |cx: f32| Rect::from_center_size(pos2(cx, band.center().y), vec2(8.0, 10.0));
    let ids = [("punch-in", edge(band.min.x)), ("punch-out", edge(band.max.x)), ("punch-mid", band.shrink2(vec2(4.0, 0.0)))];
    let beat = app.bar_ticks() as f32 / app.project.as_ref().map_or(4, |p| p.signature.0.max(1) as u32) as f32;
    let snap = |t: f32| ((t / beat).round() * beat).max(0.0) as u32;
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    for (i, (id, r)) in ids.into_iter().enumerate() {
        if r.width() <= 0.0 {
            continue;
        }
        let resp = ui.interact(r, ui.scope_id().with(id), Sense::drag()).on_hover_text("Autopunch: punch in / out locators");
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(if i == 2 { egui::CursorIcon::Grab } else { egui::CursorIcon::ResizeHorizontal });
        }
        if resp.drag_started() {
            let from_tick = resp.interact_pointer_pos().map_or(0.0, |pp| to_tick(pp.x));
            app.rec.drag = Some(match i {
                0 => PunchDrag::In,
                1 => PunchDrag::Out,
                _ => PunchDrag::Move { from_tick, start: app.rec.punch },
            });
        }
        if let (true, Some(pp)) = (resp.dragged(), resp.interact_pointer_pos()) {
            let t = to_tick(pp.x);
            let (a, b) = app.rec.punch;
            let np = match app.rec.drag {
                Some(PunchDrag::In) => (snap(t).min(b.saturating_sub(1)), b),
                Some(PunchDrag::Out) => (a, snap(t).max(a + 1)),
                Some(PunchDrag::Move { from_tick, start }) => {
                    let s = snap(start.0 as f32 + t - from_tick);
                    (s, s + (start.1 - start.0))
                }
                None => (a, b),
            };
            app.rec.punch = np;
            app.help_tag = Some(vec![("Autopunch".into(), String::new()), ("In:".into(), crate::app::bbdt(np.0, sig)), ("Out:".into(), crate::app::bbdt(np.1, sig))]);
        }
        if resp.drag_stopped() {
            app.rec.drag = None;
        }
    }
}

/// On a track's lane: what's being recorded onto it, red, growing with its waveform (audio) or
/// notes (MIDI); then its take folders.
pub fn lane(app: &mut App, ui: &mut egui::Ui, lane: Rect, track: usize, x0: f32, scroll_ticks: f32, ppt: f32) {
    crate::takes::lane(app, ui, lane, track, x0, scroll_ticks, ppt);
    let Some(s) = app.rec.session.as_ref() else { return };
    let x = |t: f64| x0 + (t as f32 - scroll_ticks) * ppt;
    let p = ui.painter_at(lane);
    let red = look::record_red();
    let pos = app.position();
    let rect = |a: f64, b: f64| Rect::from_min_max(pos2(x(a), lane.min.y + 3.0), pos2(x(b).max(x(a) + 2.0), lane.max.y - 3.0));
    let header = |p: &egui::Painter, r: Rect, name: &str| {
        p.rect_filled(r, 4.0, red.gamma_multiply(0.55));
        let h = Rect::from_min_size(r.min, vec2(r.width(), 15.0));
        p.rect_filled(h, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, red);
        p.with_clip_rect(h).text(pos2(h.min.x + 4.0, h.center().y), Align2::LEFT_CENTER, name, look::semibold(10.5), Color32::WHITE);
    };
    if let Some(tr) = s.audio.iter().find(|a| a.track == track) {
        for f in &tr.files {
            let secs = f.frames as f64 / RATE as f64;
            let end = tick_at(app, app.seconds_at(f.start) + secs);
            let r = rect(f.start, end);
            header(&p, r, &tr.name);
            let inner = Rect::from_min_max(pos2(r.min.x + 1.0, r.min.y + 16.0), pos2(r.max.x - 1.0, r.max.y - 2.0));
            if f.peaks.is_empty() || inner.width() < 1.0 {
                continue;
            }
            let mid = inner.center().y;
            let cols = inner.width() as usize;
            for k in 0..cols {
                let i0 = k * f.peaks.len() / cols;
                let i1 = ((k + 1) * f.peaks.len() / cols).max(i0 + 1).min(f.peaks.len());
                let pk = f.peaks[i0..i1].iter().fold(0f32, |m, v| m.max(*v)).min(1.0);
                let h = pk * inner.height() * 0.5;
                p.vline(inner.min.x + k as f32, mid - h..=mid + h, Stroke::new(1.0, Color32::from_rgb(0x40, 0x08, 0x08)));
            }
        }
    }
    if s.midi == Some(track) && pos >= s.from {
        let from = s.cycle.map_or(s.from, |c| c.0 as f64).max(s.punch.map_or(0.0, |p| p.0 as f64));
        let r = rect(from, pos.max(from + 1.0));
        let name = app.project.as_ref().and_then(|p| p.tracks.get(track)).map_or(String::new(), |t| t.name.clone());
        header(&p, r, &name);
        let inner = Rect::from_min_max(pos2(r.min.x, r.min.y + 16.0), pos2(r.max.x, r.max.y - 2.0));
        let held: Vec<(f64, u8)> = app.live.held.iter().map(|(&pitch, &(st, _))| (st, pitch)).collect();
        for (st, en, pitch) in app.recorded.iter().map(|n| (n.pos as f64, (n.pos + n.len) as f64, n.pitch)).chain(held.into_iter().map(|(st, pitch)| (st, pos, pitch))) {
            let y = inner.max.y - (pitch as f32 - 24.0).clamp(0.0, 84.0) / 84.0 * inner.height();
            p.line_segment([pos2(x(st), y), pos2(x(en).max(x(st) + 1.5), y)], Stroke::new(1.6, Color32::WHITE));
        }
    }
}

/// The input slot on an audio channel strip (Mixer): its input; a click picks another.
pub fn input_slot(app: &mut App, ui: &mut egui::Ui, r: Rect, track: usize) {
    let resp = ui.interact(r, ui.scope_id().with(("input-slot", track)), Sense::click()).on_hover_text("Input");
    egui::Popup::menu(&resp).show(|ui| {
        let cur = app.rec.inputs.get(&track).copied().unwrap_or_default();
        for s in InputSel::ALL {
            if ui.add(egui::Button::selectable(cur == s, s.label())).clicked() {
                app.rec.inputs.insert(track, s);
                app.dirty = true;
                ui.close();
            }
        }
    });
}

pub fn input_label(app: &App, track: usize) -> &'static str {
    app.rec.inputs.get(&track).copied().unwrap_or_default().label()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(n: usize) -> EngineRec {
        let samples: Vec<f32> = (0..n).flat_map(|i| [i as f32, -(i as f32)]).collect();
        EngineRec { feed: Feed::File { samples: Arc::new(samples), at: 0 }, ..Default::default() }
    }

    #[test]
    fn captures_from_where_recording_begins() {
        let mut e = feed(1000);
        e.start(100.0, None);
        // a block of 64 frames from tick 80, 1 tick a frame: frames from tick 100 on
        let inp = e.input(64);
        e.capture(80.0, 1.0, inp.as_deref());
        assert_eq!(e.passes.len(), 1);
        assert_eq!(e.passes[0].start, 100.0);
        assert_eq!(e.passes[0].frames, 44);
        assert_eq!(e.passes[0].data[0], 20.0);
    }

    #[test]
    fn punch_records_only_between_the_locators_and_cycles_make_passes() {
        let mut e = feed(10_000);
        e.start(0.0, Some((10.0, 20.0)));
        let inp = e.input(64);
        e.capture(0.0, 1.0, inp.as_deref());
        assert_eq!(e.passes.len(), 1);
        assert_eq!((e.passes[0].start, e.passes[0].frames), (10.0, 10));
        // the cycle goes round: a second pass, again only the punch range
        e.wrapped();
        let inp = e.input(64);
        e.capture(0.0, 1.0, inp.as_deref());
        assert_eq!(e.passes.len(), 2);
        assert_eq!((e.passes[1].start, e.passes[1].frames), (10.0, 10));
        assert_eq!(e.passes[1].data[0], 74.0);
    }

    #[test]
    fn count_in_before_bar_one_holds_and_clicks_each_beat() {
        let mut e = EngineRec { hold: 4.0 * PPQ as f64, ..Default::default() };
        // 1 beat (PPQ ticks) every 10 blocks
        let tps = PPQ as f64 / 640.0;
        let mut clicks = Vec::new();
        let mut blocks = 0;
        loop {
            let mut click = None;
            if !e.hold(64, tps, 4, &mut click) {
                break;
            }
            if let Some((f, _)) = click {
                clicks.push((blocks, f));
            }
            blocks += 1;
        }
        // four beats of count-in: 40 blocks, a click on each (the first one accented: bar 0)
        assert_eq!(blocks, 40);
        assert_eq!(clicks, vec![(0, 1760.0), (10, 1320.0), (20, 1320.0), (30, 1320.0)]);
    }

    #[test]
    fn the_count_in_clicks_until_recording_begins() {
        let e = EngineRec { click_until: Some(3840.0), ..Default::default() };
        assert!(e.click_forced(0.0));
        assert!(e.click_forced(3839.0));
        assert!(!e.click_forced(3840.0));
    }

    #[test]
    fn monitoring_hears_the_chosen_input() {
        let mut e = feed(10);
        e.monitored = vec![(2, InputSel::Input2)];
        let inp = e.input(4).unwrap();
        let (mut l, mut r) = (vec![0f32; 4], vec![0f32; 4]);
        assert!(!e.monitor(1, Some(&inp), &mut l, &mut r));
        assert!(e.monitor(2, Some(&inp), &mut l, &mut r));
        assert_eq!(l, vec![0.0, -1.0, -2.0, -3.0]);
        assert_eq!(l, r);
    }

    #[test]
    fn nothing_is_read_while_nothing_listens() {
        let mut e = feed(10);
        assert!(e.input(4).is_none());
        if let Feed::File { at, .. } = e.feed {
            assert_eq!(at, 0);
        }
    }

    /// The engine records in time: a cycle recording from bar 2 after a bar of count-in makes a
    /// pass per cycle, each from the left locator, each as long as the cycle.
    #[test]
    fn the_engine_records_cycle_passes() {
        let mut e = crate::audio::Engine::new();
        e.tempo = 120.0;
        e.tempo_map = vec![(0, 120.0)];
        let samples: Vec<f32> = (0..400_000).flat_map(|i| [(i as f32 * 0.01).sin() * 0.5, 0.0]).collect();
        e.rec.feed = Feed::File { samples: Arc::new(samples), at: 0 };
        e.cycle = Some((3840, 7680));
        e.cycle_on = true;
        e.rec.start(3840.0, None);
        e.rec.click_until = Some(3840.0);
        e.locate(0.0);
        e.playing = true;
        let mut buf = vec![0f32; 128];
        // 120 bpm: a bar is 2 s; a bar of count-in, then two times round the cycle
        for _ in 0..(6 * RATE as usize / 64) {
            e.render(&mut buf);
        }
        assert_eq!(e.rec.passes.len(), 2);
        for p in &e.rec.passes {
            assert!((p.start - 3840.0).abs() < 2.0, "{}", p.start);
            assert!((p.frames as i64 - 2 * RATE as i64).abs() <= 64, "{}", p.frames);
        }
    }

    /// Recording from bar 1 with a count-in: the playhead waits at 0 while the count-in clicks.
    #[test]
    fn the_count_in_holds_the_playhead_at_bar_one() {
        let mut e = crate::audio::Engine::new();
        e.tempo = 120.0;
        e.tempo_map = vec![(0, 120.0)];
        e.rec.feed = Feed::File { samples: Arc::new(vec![0.25; 2 * 4 * RATE as usize]), at: 0 };
        e.rec.start(0.0, None);
        e.rec.hold = 3840.0;
        e.playing = true;
        let mut buf = vec![0f32; 128];
        let mut clicked = 0;
        for _ in 0..(RATE as usize / 64) {
            e.render(&mut buf);
            if buf.iter().any(|x| x.abs() > 0.01) {
                clicked += 1;
            }
        }
        // a second in: still counting in, nothing recorded, and the click heard
        assert_eq!(e.pos, 0.0);
        assert!(e.rec.passes.is_empty());
        assert!(clicked > 0);
        for _ in 0..(2 * RATE as usize / 64) {
            e.render(&mut buf);
        }
        assert!(e.pos > 0.0);
        assert_eq!(e.rec.passes.len(), 1);
        assert_eq!(e.rec.passes[0].start, 0.0);
    }

    #[test]
    fn resampling_keeps_the_rate() {
        let mut rs = Resampler::new(44_100.0);
        let mut out = Vec::new();
        for i in 0..44_100 {
            rs.push(i as f32, 0.0, &mut out);
        }
        let frames = out.len() / 2;
        assert!((frames as i64 - RATE as i64).abs() <= 2, "{frames}");
    }
}
