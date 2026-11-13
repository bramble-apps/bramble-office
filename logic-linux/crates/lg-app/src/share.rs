//! Sharing a project, as Logic's: File ▸ Bounce ▸ Project or Section (the Bounce window for a
//! stereo project: the PCM, MP3 and M4A destinations with their options, Start / End, Mode,
//! Bounce 2nd Cycle Pass, Include Audio Tail, Include Tempo Information, Normalize, the ID3
//! Settings), File ▸ Share ▸ Song to Music / Song to Voice Memos / Mail (Linux stand-ins: the
//! Music folder with its playlists, a Voice Memos folder, the mail program), File ▸ Export ▸
//! Selection as MIDI File, and File ▸ Import ▸ Audio File / MIDI File / Other.
//!
//! MP3 and M4A files are made from the uncompressed bounce by ffmpeg (LAME, AAC, ALAC): Studio
//! carries no encoder of its own.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::vec2;
use lg_core::trackmix::Normalize;

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Project or Section…",
    "Bounce Project or Section…",
    "Bounce…",
    "Song to Music…",
    "Song to Voice Memos…",
    "Mail…",
    "Selection as MIDI File…",
    "Export Selection as MIDI File…",
    "Audio File…",
    "Import Audio File…",
    "MIDI File…",
    "Import MIDI File…",
    "Other…",
    "Import Other…",
];

// ---- the Bounce window's choices -------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FileType {
    Aiff,
    #[default]
    Wave,
    Caf,
}

impl FileType {
    pub const ALL: [FileType; 3] = [FileType::Aiff, FileType::Wave, FileType::Caf];
    pub fn name(self) -> &'static str {
        match self {
            FileType::Aiff => "AIFF",
            FileType::Wave => "WAVE",
            FileType::Caf => "CAF",
        }
    }
    pub fn ext(self) -> &'static str {
        match self {
            FileType::Aiff => "aif",
            FileType::Wave => "wav",
            FileType::Caf => "caf",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Automatic,
    Offline,
    Realtime,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Automatic, Mode::Offline, Mode::Realtime];
    pub fn name(self) -> &'static str {
        match self {
            Mode::Automatic => "Automatic",
            Mode::Offline => "Offline",
            Mode::Realtime => "Realtime",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Dither {
    #[default]
    None,
    Powr1,
    Powr2,
    Powr3,
    Uv22hr,
}

impl Dither {
    pub const ALL: [Dither; 5] = [Dither::None, Dither::Powr1, Dither::Powr2, Dither::Powr3, Dither::Uv22hr];
    pub fn name(self) -> &'static str {
        match self {
            Dither::None => "None",
            Dither::Powr1 => "POWr #1 (Dithering)",
            Dither::Powr2 => "POWr #2 (Noise Shaping)",
            Dither::Powr3 => "POWr #3 (Noise Shaping)",
            Dither::Uv22hr => "UV22HR",
        }
    }
}

/// The destination whose options show on the right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Dest {
    #[default]
    Pcm,
    Mp3,
    M4a,
    Burn,
}

pub const RATES: [u32; 8] = [11_025, 22_050, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
pub const BITS: [(&str, u16); 4] = [("8 Bit", 8), ("16 Bit", 16), ("24 Bit", 24), ("32 Bit Float", 32)];
pub const MP3_RATES: [u32; 14] = [32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
pub const M4A_RATES: [u32; 15] = [16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 256, 320];
pub const QUALITY: [&str; 5] = ["Highest", "High", "Medium", "Low", "Lowest"];

/// The ID3 frames the MP3 options write (frame, ffmpeg's metadata key).
pub const ID3_FRAMES: [(&str, &str); 12] = [
    ("Title", "title"),
    ("Artist", "artist"),
    ("Album", "album"),
    ("Year", "date"),
    ("Track Number", "track"),
    ("Genre", "genre"),
    ("Comment", "comment"),
    ("Composer", "composer"),
    ("Tempo", "TBPM"),
    ("Initial Key", "TKEY"),
    ("Copyright", "copyright"),
    ("Encoded By", "encoded_by"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Opts {
    pub pcm: bool,
    pub mp3: bool,
    pub m4a: bool,
    pub shown: Dest,
    pub start: u32,
    pub end: u32,
    pub mode: Mode,
    pub second_pass: bool,
    pub tail: bool,
    pub tempo_info: bool,
    pub normalize: Normalize,
    // PCM
    pub file_type: FileType,
    pub bits: u16,
    pub rate: u32,
    pub split: bool,
    pub dither: Dither,
    pub add_to_project: bool,
    pub add_to_library: bool,
    // MP3
    pub mp3_mono: u32,
    pub mp3_stereo: u32,
    pub mp3_vbr: bool,
    pub mp3_quality: usize,
    pub mp3_best: bool,
    pub mp3_filter: bool,
    pub mp3_joint: bool,
    pub mp3_id3: bool,
    pub id3_defaults: bool,
    pub id3: Vec<String>,
    // M4A
    pub m4a_lossless: bool,
    pub m4a_rate: u32,
    pub m4a_vbr: bool,
    /// Burn: CD/DVD (cdburn.rs).
    pub burn: crate::cdburn::Burn,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            pcm: true,
            mp3: false,
            m4a: false,
            shown: Dest::Pcm,
            start: 0,
            end: 3840,
            mode: Mode::Automatic,
            second_pass: false,
            tail: true,
            tempo_info: true,
            normalize: Normalize::OverloadProtection,
            file_type: FileType::Wave,
            bits: 24,
            rate: 48_000,
            split: false,
            dither: Dither::None,
            add_to_project: false,
            add_to_library: false,
            mp3_mono: 80,
            mp3_stereo: 160,
            mp3_vbr: false,
            mp3_quality: 0,
            mp3_best: true,
            mp3_filter: true,
            mp3_joint: true,
            mp3_id3: true,
            id3_defaults: true,
            id3: vec![String::new(); ID3_FRAMES.len()],
            m4a_lossless: false,
            m4a_rate: 256,
            m4a_vbr: false,
            burn: Default::default(),
        }
    }
}

/// The Share dialogs (Song to Music, Song to Voice Memos, Mail).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShareKind {
    Music,
    VoiceMemos,
    Mail,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShareDlg {
    pub kind: ShareKind,
    pub title: String,
    pub artist: String,
    pub composer: String,
    pub album: String,
    pub playlist: String,
    pub quality: usize,
    /// Only the cycle area or the selected regions.
    pub part: bool,
    /// Mail: the project itself rather than a mixdown.
    pub project: bool,
    /// The project changed since it was saved: asked whether to save first.
    pub ask_save: bool,
}

/// Share quality: (menu name, what's written).
pub const SHARE_QUALITY: [&str; 5] = ["Low Quality (AAC 64 kbit/s)", "Medium Quality (AAC 128 kbit/s)", "High Quality (AAC 256 kbit/s)", "Apple Lossless", "Uncompressed AIFF"];

#[derive(Default)]
struct State {
    bounce: Option<Opts>,
    last: Option<Opts>,
    id3_open: bool,
    id3_editing: Option<usize>,
    share: Option<ShareDlg>,
    job: Option<Arc<Mutex<Job>>>,
    /// Audio and MIDI files dropped on the window this frame (imported at the end of it unless
    /// a pane took them).
    dropped: Vec<PathBuf>,
    taken: bool,
}

/// A bounce running on another thread.
#[derive(Default)]
pub struct Job {
    pub progress: f32,
    pub done: Option<Result<Vec<PathBuf>, String>>,
    /// What to do with the files on the main thread when it's done.
    then: After,
    cancel: bool,
}

#[derive(Default, Clone)]
enum After {
    #[default]
    Report,
    Bounce { add_to_project: bool, add_to_library: bool },
    Mail,
}

thread_local! {
    static S: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    S.with(|s| f(&mut s.borrow_mut()))
}

// ---- commands --------------------------------------------------------------------------------

pub fn command(app: &mut App, name: &str) -> bool {
    if !COMMANDS.contains(&name) {
        return false;
    }
    match name {
        "Project or Section…" | "Bounce Project or Section…" | "Bounce…" => {
            if crate::surround::active(app) {
                // (a surround or Spatial Audio project: the Bounce window with Surround Bounce)
                crate::surround_ui::open_bounce(app);
            } else {
                open_bounce(app);
            }
        }
        "Song to Music…" => open_share(app, ShareKind::Music),
        "Song to Voice Memos…" => open_share(app, ShareKind::VoiceMemos),
        "Mail…" => open_share(app, ShareKind::Mail),
        "Selection as MIDI File…" | "Export Selection as MIDI File…" => export_selection_midi(app),
        "Audio File…" | "Import Audio File…" => import_dialog(app, "Import Audio File", &[("Audio", &["wav", "wave", "aif", "aiff", "aifc", "caf", "mp3", "m4a", "flac"])]),
        "MIDI File…" | "Import MIDI File…" => import_dialog(app, "Import MIDI File", &[("MIDI", &["mid", "midi", "smf", "kar"])]),
        "Other…" | "Import Other…" => import_dialog(app, "Import", &[("Supported files", &["mid", "midi", "smf", "kar", "wav", "wave", "aif", "aiff", "aifc", "caf", "mp3", "m4a", "flac"])]),
        _ => return false,
    }
    true
}

pub fn enabled(app: &App, name: &str) -> Option<bool> {
    if !COMMANDS.contains(&name) {
        return None;
    }
    Some(match name {
        "Selection as MIDI File…" | "Export Selection as MIDI File…" => !app.region_selection().is_empty(),
        _ => app.project.is_some(),
    })
}

fn importable(p: &Path) -> bool {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    ["mid", "midi", "smf", "kar"].contains(&ext.as_str()) || (p.is_file() && crate::soundlib::is_audio(p))
}

/// Audio and MIDI files dragged from the file manager onto the window: imported (at the
/// playhead, on the bar) unless the Project Audio Browser under the pointer takes them.
pub fn takes_drop(app: &App, files: &[PathBuf]) -> bool {
    if app.project.is_none() || files.is_empty() || !files.iter().all(|f| importable(f)) {
        return false;
    }
    with(|s| {
        s.dropped = files.to_vec();
        s.taken = false;
    });
    true
}

/// The Project Audio Browser added this frame's dropped files itself.
pub fn took_drop() {
    with(|s| s.taken = true);
}

fn import_dialog(app: &mut App, title: &str, filters: &[(&str, &[&str])]) {
    if app.project.is_none() || app.automated {
        return;
    }
    let mut d = kn_ui::files::FileDialog::new().set_title(title);
    for (n, e) in filters {
        d = d.add_filter(*n, e);
    }
    let Some(path) = d.pick_file() else { return };
    import_path(app, &path);
}

/// Imports a file by its kind: MIDI and audio at the playhead (rounded to the bar), MusicXML as
/// new tracks.
pub fn import_path(app: &mut App, path: &Path) {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "musicxml" | "xml" => crate::scorexml::import(app, path),
        "mxl" | "fcpxml" => app.tell("Studio doesn't import that kind of file yet"),
        _ => app.import_file(path),
    }
}

// ---- the bounce range --------------------------------------------------------------------------

/// The bounce range: the cycle area when Cycle is on, else the selected regions, else the whole
/// project (to the end of its last region).
pub fn bounce_range(app: &App) -> (u32, u32) {
    let Some(p) = &app.project else { return (0, 3840) };
    if app.cycle_on() && app.cycle.1 > app.cycle.0 {
        return app.cycle;
    }
    let sel: Vec<(u32, u32)> = app.region_selection().iter().filter_map(|(t, r)| p.tracks.get(*t)?.regions.get(*r)).map(|r| (r.start, r.start + r.length)).collect();
    if !sel.is_empty() {
        return (sel.iter().map(|x| x.0).min().unwrap_or(0), sel.iter().map(|x| x.1).max().unwrap_or(3840));
    }
    (0, p.tracks.iter().flat_map(|t| t.regions.iter()).map(|r| r.start + r.length).max().unwrap_or(3840).max(app.bar_ticks()))
}

pub fn open_bounce(app: &mut App) {
    if app.project.is_none() {
        return;
    }
    let (start, end) = bounce_range(app);
    let mut o = with(|s| s.last.clone()).unwrap_or_default();
    o.start = start;
    o.end = end;
    o.second_pass &= app.cycle_on();
    with(|s| s.bounce = Some(o));
}

/// The plug-ins whose sound comes from outside Studio, which only a realtime bounce can capture.
fn needs_realtime(app: &App) -> bool {
    let outside = |n: &str| n == "I/O" || n == "External Instrument" || n.starts_with("I/O ");
    app.project.as_ref().is_some_and(|p| p.tracks.iter().any(|t| t.plugins.iter().any(|u| outside(&u.name))))
}

/// Automatic: offline unless something must be heard as it plays.
pub fn effective_mode(app: &App, m: Mode) -> Mode {
    match m {
        Mode::Automatic if needs_realtime(app) => Mode::Realtime,
        Mode::Automatic => Mode::Offline,
        m => m,
    }
}

/// Studio's folder for bounces (~/Music/Bramble Studio/Bounces), or the folder last chosen.
pub fn bounce_folder(app: &App) -> PathBuf {
    let v = app.logic_settings.0.get("Bounce / Folder").cloned().unwrap_or_default();
    if !v.is_empty() && Path::new(&v).is_dir() {
        return PathBuf::from(v);
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    home.join("Music/Bramble Studio/Bounces")
}

fn music_dir() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    if let Ok(o) = Command::new("xdg-user-dir").arg("MUSIC").output() {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !s.is_empty() && s != home.to_string_lossy() {
            return PathBuf::from(s);
        }
    }
    home.join("Music")
}

/// "Add to Music Library": Studio's folder in the Music folder.
pub fn library_dir() -> PathBuf {
    std::env::var_os("LG_MUSIC_LIBRARY").map(PathBuf::from).unwrap_or_else(|| music_dir().join("Bramble Studio"))
}

// ---- rendering ---------------------------------------------------------------------------------

/// Renders the range into (left, right) at the engine's rate. `second_pass`: the range plays
/// twice and the file is written from the second time through (the first pass's tails ring
/// into its start); `tail`: the instruments' release and effect tails after the end, until it
/// is quiet (at most 20 s); `realtime`: paced to the clock as it would play.
pub fn render(mut e: crate::audio::Engine, from: u32, to: u32, second_pass: bool, tail: bool, realtime: bool, job: Option<&Arc<Mutex<Job>>>) -> Option<(Vec<f32>, Vec<f32>)> {
    const B: usize = 256;
    let rate = crate::audio::RATE as f64;
    let mut buf = vec![0f32; 2 * B];
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let started = Instant::now();
    let mut made = 0usize;
    let span = to.saturating_sub(from).max(1) as f64;
    let passes = if second_pass { 2 } else { 1 };
    let pace = |made: &mut usize, frac: f32| -> bool {
        *made += B;
        if let Some(j) = job {
            let mut j = j.lock().unwrap_or_else(|e| e.into_inner());
            j.progress = frac;
            if j.cancel {
                return false;
            }
        }
        if realtime {
            let due = Duration::from_secs_f64(*made as f64 / rate);
            let el = started.elapsed();
            if due > el {
                std::thread::sleep(due - el);
            }
        }
        true
    };
    // (a bounce runs to `to`, past the Project End marker when asked)
    e.end = None;
    e.pos = from as f64;
    e.playing = true;
    for pass in 0..passes {
        e.pos = from as f64;
        while e.pos < to as f64 {
            e.render(&mut buf);
            if pass + 1 == passes {
                l.extend(buf.iter().step_by(2));
                r.extend(buf.iter().skip(1).step_by(2));
            }
            let frac = ((pass as f64 + (e.pos - from as f64) / span) / passes as f64) as f32;
            if !pace(&mut made, frac.min(0.99)) {
                return None;
            }
        }
    }
    // the end exactly (the last block runs past it)
    let exact = frames_between(&e, from, to);
    if !tail {
        l.truncate(exact);
        r.truncate(exact);
        return Some((l, r));
    }
    e.playing = false;
    e.all_off();
    let mut quiet = 0;
    for _ in 0..(20.0 * rate) as usize / B {
        e.render(&mut buf);
        l.extend(buf.iter().step_by(2));
        r.extend(buf.iter().skip(1).step_by(2));
        quiet = if buf.iter().all(|x| x.abs() < 1e-4) { quiet + 1 } else { 0 };
        if quiet * B > crate::audio::RATE as usize / 4 || !pace(&mut made, 0.99) {
            break;
        }
    }
    // (the quiet quarter second after the tail is cut, down to the end of the sound)
    let last = l.iter().zip(r.iter()).rposition(|(a, b)| a.abs() >= 1e-4 || b.abs() >= 1e-4).map_or(0, |k| k + 1);
    let keep = last.max(exact).min(l.len());
    l.truncate(keep);
    r.truncate(keep);
    Some((l, r))
}

/// Frames from tick `from` to `to` at the engine's tempo map.
fn frames_between(e: &crate::audio::Engine, from: u32, to: u32) -> usize {
    let m = if e.timeline.points.is_empty() { lg_core::timeline::Map::tempo(e.tempo) } else { e.timeline.clone() };
    let secs = m.secs_at(to as f64) - m.secs_at(from as f64);
    (secs * crate::audio::RATE as f64).round() as usize
}

// ---- writing -------------------------------------------------------------------------------------

/// Sample rate conversion from the engine's 48 kHz: a windowed-sinc low-pass first when going
/// down (no aliasing), then cubic interpolation.
pub fn convert_rate(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    let src: Vec<f32> = if to < from { lowpass(x, 0.45 * to as f64 / from as f64) } else { x.to_vec() };
    let frames = (x.len() as f64 * to as f64 / from as f64).round() as usize;
    lg_audio::stretch::resample(&src, 1, frames)
}

/// A 127-tap windowed-sinc low-pass at `cut` (fraction of the rate).
fn lowpass(x: &[f32], cut: f64) -> Vec<f32> {
    const N: isize = 63;
    let taps: Vec<f32> = (-N..=N)
        .map(|k| {
            let k = k as f64;
            let sinc = if k == 0.0 { 2.0 * cut } else { (2.0 * std::f64::consts::PI * cut * k).sin() / (std::f64::consts::PI * k) };
            let w = 0.42 + 0.5 * (std::f64::consts::PI * k / N as f64).cos() + 0.08 * (2.0 * std::f64::consts::PI * k / N as f64).cos();
            (sinc * w) as f32
        })
        .collect();
    let n = x.len() as isize;
    (0..n)
        .map(|i| {
            let mut s = 0.0f32;
            for (j, t) in taps.iter().enumerate() {
                let k = i + j as isize - N;
                if k >= 0 && k < n {
                    s += x[k as usize] * t;
                }
            }
            s
        })
        .collect()
}

/// A small reproducible noise source (xorshift), uniform in -0.5..0.5.
struct Noise(u64);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32 - 0.5
    }
}

/// The quantising scale the files use for `bits` (as lg-audio writes them).
fn scale_of(bits: u16) -> f32 {
    match bits {
        8 => 127.0,
        16 => 32767.0,
        24 => 8_388_607.0,
        _ => 0.0,
    }
}

/// Reduces a channel to `bits` with the chosen dither (None: plain rounding when the file is
/// written). POWr #1: triangular dither of one step; POWr #2: the same with first-order noise
/// shaping (the quantisation noise pushed up the spectrum); POWr #3: a steeper five-tap shaper
/// (least noise around 2–4 kHz, where the ear is most sensitive, the rest above 10 kHz); UV22HR:
/// a low-level high-frequency dither near the top of the band. Studio's own algorithms in the
/// spirit of Logic's (they're not Apple's).
pub fn dither(x: &mut [f32], bits: u16, d: Dither, seed: u64) {
    let scale = scale_of(bits);
    if d == Dither::None || scale == 0.0 {
        return;
    }
    let mut n = Noise(seed.max(1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    // error-feedback filter (applied to past quantisation errors)
    let h: &[f32] = match d {
        Dither::Powr2 => &[1.0],
        Dither::Powr3 => &[2.033, -2.165, 1.959, -1.590, 0.6149],
        _ => &[],
    };
    let mut err = [0f32; 5];
    let mut prev = 0.0f32;
    for s in x.iter_mut() {
        let v = *s * scale;
        let shaped = v - h.iter().zip(err.iter()).map(|(a, e)| a * e).sum::<f32>();
        let dn = match d {
            Dither::Uv22hr => {
                // high-passed noise: most of its energy near the Nyquist frequency
                let w = n.next();
                let hp = (w - prev) * 0.5;
                prev = w;
                hp
            }
            _ => n.next() + n.next(),
        };
        let q = (shaped + dn).round().clamp(-scale - 1.0, scale);
        err.rotate_right(1);
        err[0] = q - shaped;
        *s = q / scale;
    }
}

/// A CAF file (Core Audio Format): linear PCM, big-endian integers or 32-bit float.
pub fn caf_bytes(rate: u32, chans: &[Vec<f32>], bits: u16) -> Vec<u8> {
    let float = bits == 32;
    let c = chans.len().max(1);
    let bps = bits as usize / 8;
    let n = chans.iter().map(|x| x.len()).max().unwrap_or(0);
    let mut out = Vec::with_capacity(n * c * bps + 128);
    out.extend_from_slice(b"caff");
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(b"desc");
    out.extend_from_slice(&32i64.to_be_bytes());
    out.extend_from_slice(&(rate as f64).to_be_bytes());
    out.extend_from_slice(b"lpcm");
    out.extend_from_slice(&(if float { 1u32 } else { 0 }).to_be_bytes());
    out.extend_from_slice(&((bps * c) as u32).to_be_bytes());
    out.extend_from_slice(&1u32.to_be_bytes());
    out.extend_from_slice(&(c as u32).to_be_bytes());
    out.extend_from_slice(&(bits as u32).to_be_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&((4 + n * c * bps) as i64).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    for f in 0..n {
        for ch in chans {
            let s = ch.get(f).copied().unwrap_or(0.0);
            if float {
                out.extend_from_slice(&s.to_be_bytes());
                continue;
            }
            let v = (s.clamp(-1.0, 1.0) as f64 * scale_of(bits) as f64).round() as i32;
            let b = v.to_be_bytes();
            out.extend_from_slice(&b[4 - bps..]);
        }
    }
    out
}

/// The WAV 'acid' chunk carrying the tempo and the length in beats (Include Tempo Information).
fn acid_chunk(bpm: f64, beats: u32, sig: (u8, u8)) -> Vec<u8> {
    let mut b = Vec::with_capacity(24);
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&60u16.to_le_bytes());
    b.extend_from_slice(&0x8000u16.to_le_bytes());
    b.extend_from_slice(&0f32.to_le_bytes());
    b.extend_from_slice(&beats.to_le_bytes());
    b.extend_from_slice(&(sig.1 as u16).to_le_bytes());
    b.extend_from_slice(&(sig.0 as u16).to_le_bytes());
    b.extend_from_slice(&(bpm as f32).to_le_bytes());
    b
}

/// One uncompressed file of `chans` at `rate`.
fn write_pcm(path: &Path, chans: &[Vec<f32>], rate: u32, ft: FileType, bits: u16, tempo: Option<(f64, u32, (u8, u8))>) -> std::io::Result<()> {
    if ft == FileType::Caf {
        return std::fs::write(path, caf_bytes(rate, chans, bits));
    }
    let n = chans.iter().map(|c| c.len()).max().unwrap_or(0);
    let k = chans.len().max(1);
    let mut data = vec![0f32; n * k];
    for (c, buf) in chans.iter().enumerate() {
        for (f, x) in buf.iter().enumerate() {
            data[f * k + c] = *x;
        }
    }
    let mut af = lg_audio::AudioFile::new(rate, k, data);
    af.bits = bits;
    af.float = bits == 32;
    af.format = if ft == FileType::Aiff { lg_audio::Format::Aiff } else { lg_audio::Format::Wav };
    if let Some((bpm, beats, sig)) = tempo {
        af.extra.push((*b"acid", acid_chunk(bpm, beats, sig)));
    }
    af.write(path)
}

/// What a bounce writes, worked out on the main thread.
#[derive(Clone, Debug)]
pub struct Plan {
    pub opts: Opts,
    /// The chosen file without its extension ("…/Bounces/Output 1-2").
    pub base: PathBuf,
    /// The split files' channel identifiers (Bounce Extensions: ".L", ".R").
    pub exts: (String, String),
    pub tempo: Option<(f64, u32, (u8, u8))>,
    /// ID3 / M4A tags (ffmpeg key, value).
    pub tags: Vec<(String, String)>,
}

fn with_ext(base: &Path, tag: &str, ext: &str) -> PathBuf {
    let name = base.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    base.with_file_name(format!("{name}{tag}.{ext}"))
}

/// The files a plan writes, in order (for the report and Add to Project).
pub fn file_names(p: &Plan) -> Vec<PathBuf> {
    let o = &p.opts;
    let mut v = Vec::new();
    let tags: Vec<&str> = if o.split { vec![&p.exts.0, &p.exts.1] } else { vec![""] };
    if o.pcm {
        v.extend(tags.iter().map(|t| with_ext(&p.base, t, o.file_type.ext())));
    }
    if o.mp3 {
        v.extend(tags.iter().map(|t| with_ext(&p.base, t, "mp3")));
    }
    if o.m4a {
        v.extend(tags.iter().map(|t| with_ext(&p.base, t, "m4a")));
    }
    v
}

/// The ffmpeg arguments that encode `src` (an uncompressed file) to MP3 at `dst`.
pub fn mp3_args(o: &Opts, mono: bool, src: &Path, dst: &Path, tags: &[(String, String)]) -> Vec<String> {
    let mut a: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-y".into(), "-i".into(), src.to_string_lossy().into()];
    a.extend(["-c:a".into(), "libmp3lame".into()]);
    let kbps = if mono { o.mp3_mono } else { o.mp3_stereo };
    if o.mp3_vbr {
        // (the V level whose average is near the chosen rate)
        let v = match kbps {
            256.. => 0,
            224.. => 1,
            192.. => 2,
            160.. => 4,
            128.. => 5,
            96.. => 7,
            _ => 9,
        };
        a.extend(["-q:a".into(), v.to_string()]);
    } else {
        a.extend(["-b:a".into(), format!("{kbps}k")]);
    }
    // LAME's algorithm quality: 0 is the slowest and best
    let q = if o.mp3_best { 0 } else { [2, 3, 5, 7, 9][o.mp3_quality.min(4)] };
    a.extend(["-compression_level".into(), q.to_string()]);
    if !mono {
        a.extend(["-joint_stereo".into(), if o.mp3_joint { "1" } else { "0" }.into()]);
    }
    if o.mp3_filter {
        a.extend(["-af".into(), "highpass=f=10".into()]);
    }
    a.extend(["-ar".into(), "48000".into()]);
    if o.mp3_id3 {
        a.extend(["-id3v2_version".into(), "3".into()]);
        for (k, v) in tags {
            if !v.is_empty() {
                a.extend(["-metadata".into(), format!("{k}={v}")]);
            }
        }
    } else {
        a.extend(["-write_id3v1".into(), "0".into(), "-id3v2_version".into(), "0".into(), "-map_metadata".into(), "-1".into()]);
    }
    a.push(dst.to_string_lossy().into());
    a
}

/// The ffmpeg arguments for M4A (AAC or Apple Lossless).
pub fn m4a_args(o: &Opts, src: &Path, dst: &Path, tags: &[(String, String)]) -> Vec<String> {
    let mut a: Vec<String> = vec!["-hide_banner".into(), "-loglevel".into(), "error".into(), "-y".into(), "-i".into(), src.to_string_lossy().into(), "-vn".into()];
    if o.m4a_lossless {
        a.extend(["-c:a".into(), "alac".into()]);
    } else {
        a.extend(["-c:a".into(), "aac".into()]);
        if o.m4a_vbr {
            a.extend(["-q:a".into(), format!("{:.2}", (o.m4a_rate as f32 / 160.0).clamp(0.1, 2.0))]);
        } else {
            a.extend(["-b:a".into(), format!("{}k", o.m4a_rate)]);
        }
    }
    a.extend(["-ar".into(), "48000".into()]);
    for (k, v) in tags {
        if !v.is_empty() && !k.starts_with('T') {
            a.extend(["-metadata".into(), format!("{k}={v}")]);
        }
    }
    a.push(dst.to_string_lossy().into());
    a
}

fn ffmpeg(args: &[String]) -> Result<(), String> {
    let bin = std::env::var("LG_FFMPEG").unwrap_or_else(|_| "ffmpeg".into());
    let out = Command::new(&bin).args(args).output().map_err(|e| format!("MP3 and M4A need ffmpeg ({e})"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("ffmpeg failed").to_string())
    }
}

/// Writes every destination of a plan from the rendered mix (at the engine's rate).
pub fn write_all(p: &Plan, mix: (Vec<f32>, Vec<f32>)) -> Result<Vec<PathBuf>, String> {
    let o = &p.opts;
    let src_rate = crate::audio::RATE;
    let (mut l, mut r) = mix;
    // Normalize: over the whole mix, both channels by the same gain
    let peak = l.iter().chain(r.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
    let g = o.normalize.gain(peak);
    if g != 1.0 {
        l.iter_mut().chain(r.iter_mut()).for_each(|x| *x *= g);
    }
    if let Some(d) = p.base.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let chans_at = |rate: u32, bits: u16, d: Dither| -> Vec<Vec<f32>> {
        [&l, &r]
            .iter()
            .enumerate()
            .map(|(k, c)| {
                let mut x = convert_rate(c, src_rate, rate);
                if bits < 32 {
                    dither(&mut x, bits, d, k as u64 + 1);
                }
                x
            })
            .collect()
    };
    let groups = |chans: Vec<Vec<f32>>| -> Vec<(String, Vec<Vec<f32>>)> {
        if o.split {
            vec![(p.exts.0.clone(), vec![chans[0].clone()]), (p.exts.1.clone(), vec![chans[1].clone()])]
        } else {
            vec![(String::new(), chans)]
        }
    };
    let mut written = Vec::new();
    // the uncompressed files (also the encoders' source when they're at most 48 kHz)
    let mut source: Vec<(String, PathBuf)> = Vec::new();
    if o.pcm {
        let tempo = if o.tempo_info && o.file_type != FileType::Caf { p.tempo } else { None };
        for (tag, ch) in groups(chans_at(o.rate, o.bits, o.dither)) {
            let path = with_ext(&p.base, &tag, o.file_type.ext());
            write_pcm(&path, &ch, o.rate, o.file_type, o.bits, tempo).map_err(|e| e.to_string())?;
            if o.rate <= 48_000 && o.file_type != FileType::Caf {
                source.push((tag.clone(), path.clone()));
            }
            written.push(path);
        }
    }
    if o.mp3 || o.m4a {
        // (else a temporary uncompressed file at 48 kHz is the source)
        let mut temps = Vec::new();
        if source.is_empty() {
            let dir = std::env::temp_dir().join(format!("bramble-bounce-{}", std::process::id()));
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            for (tag, ch) in groups(chans_at(48_000, 24, Dither::None)) {
                let t = dir.join(format!("source{tag}.wav"));
                write_pcm(&t, &ch, 48_000, FileType::Wave, 24, None).map_err(|e| e.to_string())?;
                temps.push(t.clone());
                source.push((tag, t));
            }
        }
        for (tag, src) in &source {
            if o.mp3 {
                let dst = with_ext(&p.base, tag, "mp3");
                ffmpeg(&mp3_args(o, o.split, src, &dst, &p.tags))?;
                written.push(dst);
            }
            if o.m4a {
                let dst = with_ext(&p.base, tag, "m4a");
                ffmpeg(&m4a_args(o, src, &dst, &p.tags))?;
                written.push(dst);
            }
        }
        for t in temps {
            let _ = std::fs::remove_file(t);
        }
    }
    Ok(written)
}

/// The ID3 values "Use default values" fills in: the project's name, tempo and key (the
/// Signature track's first key, else C), the year and Studio as the encoder.
pub fn id3_defaults(app: &App) -> Vec<String> {
    let p = app.project.as_ref();
    let key = p.map(|p| p.key.trim().to_string()).filter(|k| !k.is_empty()).unwrap_or_else(|| "C".into());
    let year = 1970 + std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 31_556_952);
    ID3_FRAMES
        .iter()
        .map(|(f, _)| match *f {
            "Title" => p.map(|p| p.name.clone()).unwrap_or_default(),
            "Year" => year.to_string(),
            "Tempo" => p.map(|p| format!("{}", p.tempo.round() as i64)).unwrap_or_default(),
            "Initial Key" => key.clone(),
            "Encoded By" => "Bramble Studio".into(),
            _ => String::new(),
        })
        .collect()
}

fn plan(app: &App, o: &Opts, base: PathBuf) -> Plan {
    let ext = |c| crate::surround_ui::extension(app, c);
    let exts = (ext(lg_dsp::surround::Ch::L), ext(lg_dsp::surround::Ch::R));
    let p = app.project.as_ref();
    let tempo = p.map(|p| (p.tempo, o.end.saturating_sub(o.start) / lg_core::events::PPQ, p.signature));
    let values = if o.id3_defaults {
        let d = id3_defaults(app);
        o.id3.iter().zip(d).map(|(v, d)| if v.is_empty() { d } else { v.clone() }).collect()
    } else {
        o.id3.clone()
    };
    let tags = ID3_FRAMES.iter().zip(values).map(|((_, k), v)| (k.to_string(), v)).collect();
    Plan { opts: o.clone(), base, exts, tempo, tags }
}

/// Starts a bounce in the background; the window shows its progress and the report follows.
fn start_job(app: &mut App, p: Plan, then: After) {
    let Some(engine) = crate::surround_ui::offline(app) else { return };
    let realtime = effective_mode(app, p.opts.mode) == Mode::Realtime;
    let job = Arc::new(Mutex::new(Job { then, ..Default::default() }));
    with(|s| s.job = Some(job.clone()));
    app.tell(if realtime { "Bouncing in real time…" } else { "Bouncing…" });
    let o = p.opts.clone();
    std::thread::spawn(move || {
        let res = match render(engine, o.start, o.end.max(o.start + 1), o.second_pass, o.tail, realtime, Some(&job)) {
            Some(mix) => write_all(&p, mix),
            None => Err("Bounce cancelled".into()),
        };
        let mut j = job.lock().unwrap_or_else(|e| e.into_inner());
        j.progress = 1.0;
        j.done = Some(res);
    });
}

/// Runs a bounce now, on this thread (tests and scripts).
pub fn bounce_now(app: &App, o: &Opts, base: PathBuf) -> Result<Vec<PathBuf>, String> {
    let p = plan(app, o, base);
    let e = crate::surround_ui::offline(app).ok_or("no engine")?;
    let realtime = effective_mode(app, o.mode) == Mode::Realtime;
    let mix = render(e, o.start, o.end.max(o.start + 1), o.second_pass, o.tail, realtime, None).ok_or("cancelled")?;
    write_all(&p, mix)
}

/// What's done with finished files on the main thread.
fn finish(app: &mut App, then: After, res: Result<Vec<PathBuf>, String>) {
    let files = match res {
        Ok(f) => f,
        Err(e) => {
            app.tell(format!("Couldn't bounce: {e}"));
            return;
        }
    };
    let names = files.iter().map(|f| f.file_name().unwrap_or_default().to_string_lossy().to_string()).collect::<Vec<_>>().join(", ");
    match then {
        After::Report => app.tell(format!("Shared {names}")),
        After::Bounce { add_to_project, add_to_library } => {
            if add_to_project {
                // (the uncompressed, MP3 and M4A files alike go to the Project Audio Browser)
                crate::projaudio::add_files(app, files.clone());
            }
            if add_to_library {
                let dir = library_dir();
                let _ = std::fs::create_dir_all(&dir);
                for f in &files {
                    if let Some(n) = f.file_name() {
                        let _ = std::fs::copy(f, dir.join(n));
                    }
                }
            }
            app.tell(format!("Bounced {names}"));
            crate::cdburn::after_bounce(app, &files);
        }
        After::Mail => mail(app, &files),
    }
}

// ---- Share --------------------------------------------------------------------------------------

fn open_share(app: &mut App, kind: ShareKind) {
    let Some(p) = &app.project else { return };
    let part = app.cycle_on() || !app.region_selection().is_empty();
    // (Settings ▸ My Info fills the artist, composer, album and playlist)
    let me = |k: &str| app.logic_settings.get(&format!("My Info /  / {k}")).trim().to_string();
    let playlist = Some(me("Playlist:")).filter(|s| !s.is_empty()).unwrap_or_else(|| "Bramble Studio".into());
    let d = ShareDlg { kind, title: p.name.clone(), artist: me("Artist Name:"), composer: me("Composer Name:"), album: me("Album Name:"), playlist, quality: 2, part, project: false, ask_save: false };
    with(|s| s.share = Some(d));
}

/// Where Song to Music puts a song: the Music folder, under the artist and album.
pub fn music_path(d: &ShareDlg, ext: &str) -> PathBuf {
    let clean = |s: &str, or: &str| {
        let t: String = s.trim().chars().map(|c| if c == '/' || c == '\0' { '-' } else { c }).collect();
        if t.is_empty() { or.to_string() } else { t }
    };
    let root = match d.kind {
        ShareKind::VoiceMemos => music_dir().join("Voice Memos"),
        _ => library_dir().join(clean(&d.artist, "Unknown Artist")).join(clean(&d.album, "Unknown Album")),
    };
    root.join(format!("{}.{ext}", clean(&d.title, "Untitled")))
}

/// Adds a song to a playlist (an .m3u in the Music folder's Playlists).
pub fn add_to_playlist(list: &str, song: &Path) -> std::io::Result<PathBuf> {
    let dir = library_dir().join("Playlists");
    std::fs::create_dir_all(&dir)?;
    let f = dir.join(format!("{}.m3u", list.trim()));
    let mut text = std::fs::read_to_string(&f).unwrap_or_else(|_| "#EXTM3U\n".into());
    let line = song.to_string_lossy().to_string();
    if !text.lines().any(|l| l == line) {
        text += &format!("{line}\n");
    }
    std::fs::write(&f, text)?;
    Ok(f)
}

/// A share's bounce options from its Quality.
pub fn share_opts(q: usize) -> Opts {
    let mut o = Opts { pcm: false, m4a: true, tail: true, ..Default::default() };
    match q {
        0 => o.m4a_rate = 64,
        1 => o.m4a_rate = 128,
        3 => o.m4a_lossless = true,
        4 => {
            o.m4a = false;
            o.pcm = true;
            o.file_type = FileType::Aiff;
            o.bits = 16;
            o.rate = 44_100;
            o.dither = Dither::Powr2;
        }
        _ => o.m4a_rate = 256,
    }
    o
}

fn run_share(app: &mut App, d: ShareDlg) {
    if d.kind == ShareKind::Mail && d.project {
        mail_project(app);
        return;
    }
    let (start, end) = if d.part { bounce_range(app) } else { (0, project_end(app)) };
    let mut o = share_opts(d.quality);
    o.start = start;
    o.end = end;
    let ext = if o.pcm { "aif" } else { "m4a" };
    let file = match d.kind {
        ShareKind::Mail => std::env::temp_dir().join(format!("bramble-share-{}", std::process::id())).join(format!("{}.{ext}", d.title.trim())),
        _ => music_path(&d, ext),
    };
    let base = file.with_extension("");
    let mut p = plan(app, &o, base);
    p.tags = vec![("title".into(), d.title.clone()), ("artist".into(), d.artist.clone()), ("composer".into(), d.composer.clone()), ("album".into(), d.album.clone())];
    let then = match d.kind {
        ShareKind::Mail => After::Mail,
        _ => After::Report,
    };
    let playlist = (d.kind == ShareKind::Music && !d.playlist.trim().is_empty()).then(|| d.playlist.clone());
    if let Some(list) = playlist {
        let _ = add_to_playlist(&list, &file);
    }
    start_job(app, p, then);
}

/// The Project End marker (Logic's Bounce with nothing selected runs to it; projend.rs).
fn project_end(app: &App) -> u32 {
    crate::projend::end(app)
}

/// Opens the mail program with the files attached (xdg-email).
fn mail(app: &mut App, files: &[PathBuf]) {
    let mut c = Command::new("xdg-email");
    let subject = app.project.as_ref().map_or("Song".into(), |p| p.name.clone());
    c.arg("--subject").arg(subject);
    for f in files {
        c.arg("--attach").arg(f);
    }
    match c.spawn() {
        Ok(_) => app.tell("A new message with the attachment is open in your mail program"),
        Err(e) => app.tell(format!("Couldn't open your mail program: {e}")),
    }
}

/// Mail ▸ Project: the saved project package (all its assets) zipped, as a package is one file
/// on a Mac.
fn mail_project(app: &mut App) {
    let Some((path, _)) = app.file.clone() else {
        app.tell("Save the project first");
        return;
    };
    let dir = std::env::temp_dir().join(format!("bramble-share-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let zip = dir.join(format!("{name}.zip"));
    let _ = std::fs::remove_file(&zip);
    let ok = if path.is_dir() {
        Command::new("zip").arg("-qr").arg(&zip).arg(&name).current_dir(path.parent().unwrap_or(Path::new("."))).status().is_ok_and(|s| s.success())
    } else {
        std::fs::copy(&path, dir.join(&name)).is_ok()
    };
    if !ok {
        app.tell("Couldn't pack the project for mail");
        return;
    }
    let f = if path.is_dir() { zip } else { dir.join(&name) };
    mail(app, &[f]);
}

// ---- Export ▸ Selection as MIDI File -------------------------------------------------------------

/// The selected regions as a format 1 MIDI file (one track per track with selected regions).
pub fn selection_midi(app: &App) -> Option<Vec<u8>> {
    let p = app.project.as_ref()?;
    let sel = app.region_selection();
    if sel.is_empty() {
        return None;
    }
    let mut q = p.clone();
    for (t, tr) in q.tracks.iter_mut().enumerate() {
        let keep: Vec<usize> = sel.iter().filter(|(st, _)| *st == t).map(|x| x.1).collect();
        let mut k = 0;
        tr.regions.retain(|_| {
            k += 1;
            keep.contains(&(k - 1))
        });
    }
    q.tracks.retain(|t| !t.regions.is_empty());
    // (Settings ▸ General ▸ Project Handling: a single MIDI region as format 0)
    let single = sel.len() == 1 && app.logic_settings.get("General / Project Handling / Export MIDI File command saves single MIDI region as format 0.") != "0";
    Some(if single { lg_core::midi::write_format0(&q) } else { lg_core::midi::write(&q) })
}

fn export_selection_midi(app: &mut App) {
    let Some(bytes) = selection_midi(app) else {
        app.tell("Select the regions to export first");
        return;
    };
    let name = app.project.as_ref().map_or("Untitled".into(), |p| p.name.clone());
    let Some(path) = (if app.automated { std::env::var("LG_EXPORT_MIDI").ok().map(PathBuf::from) } else { kn_ui::files::FileDialog::new().set_title("Export Selection as MIDI File").set_file_name(format!("{name}.mid")).add_filter("MIDI", &["mid"]).save_file() }) else { return };
    match std::fs::write(&path, bytes) {
        Ok(()) => app.tell(format!("Exported {}", path.file_name().unwrap_or_default().to_string_lossy())),
        Err(e) => app.tell(format!("Couldn't export: {e}")),
    }
}

// ---- windows ----------------------------------------------------------------------------------------

fn bar_text(app: &App, t: u32) -> String {
    let bar = app.bar_ticks().max(1);
    let beat = lg_core::events::PPQ;
    let div = beat / 4;
    format!("{} {} {} {}", t / bar + 1, (t % bar) / beat + 1, (t % beat) / div + 1, (t % div) * 240 / div.max(1) + 1)
}

/// A grid's label, right-aligned against its control as Logic's are.
fn lab(ui: &mut egui::Ui, t: &str) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(t));
}

fn primary(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add(egui::Button::new(egui::RichText::new(label).color(egui::Color32::WHITE)).fill(look::accent()).min_size(vec2(76.0, 22.0))).clicked()
}

fn combo<T: Copy + PartialEq>(ui: &mut egui::Ui, id: &str, v: &mut T, items: &[(T, String)]) {
    let cur = items.iter().find(|x| x.0 == *v).map(|x| x.1.clone()).unwrap_or_default();
    egui::ComboBox::from_id_salt(id).width(170.0).selected_text(cur).show_ui(ui, |ui| {
        for (x, n) in items {
            ui.selectable_value(v, *x, n);
        }
    });
}

/// The Bounce window, the ID3 Settings, the Share dialogs and a running bounce's progress.
pub fn windows(app: &mut App, ctx: &egui::Context) {
    let (files, taken) = with(|s| (std::mem::take(&mut s.dropped), std::mem::take(&mut s.taken)));
    if !taken {
        for f in files {
            import_path(app, &f);
        }
    }
    bounce_window(app, ctx);
    id3_window(app, ctx);
    share_window(app, ctx);
    progress(app, ctx);
}

fn bounce_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut o) = with(|s| s.bounce.clone()) else { return };
    let mut close = false;
    let mut go = false;
    let cycle = app.cycle_on();
    let starts = (bar_text(app, o.start), bar_text(app, o.end));
    let bar = app.bar_ticks().max(1);
    egui::Window::new("Bounce “Output 1-2”").id(egui::Id::unique("share-bounce")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal_top(|ui| {
            // Destination: a framed list, each with its checkbox; the row clicked shows its options
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Destination").strong());
                egui::Frame::new().stroke(egui::Stroke::new(1.0, look::line())).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
                    ui.set_width(150.0);
                    for (d, label) in [(Dest::Pcm, "PCM"), (Dest::Mp3, "MP3"), (Dest::M4a, "M4A: AAC"), (Dest::Burn, "Burn: CD/DVD")] {
                        let sel = o.shown == d;
                        let bg = ui.painter().add(egui::Shape::Noop);
                        let r = ui.horizontal(|ui| {
                            let mut on = match d {
                                Dest::Pcm => o.pcm,
                                Dest::Mp3 => o.mp3,
                                Dest::M4a => o.m4a,
                                Dest::Burn => o.burn.on,
                            };
                            let c = ui.add(egui::Checkbox::without_text(&mut on));
                            if c.changed() {
                                match d {
                                    Dest::Pcm => o.pcm = on,
                                    Dest::Mp3 => o.mp3 = on,
                                    Dest::M4a => o.m4a = on,
                                    // (burning takes the uncompressed bounce, interleaved)
                                    Dest::Burn => {
                                        o.burn.on = on;
                                        if on {
                                            o.pcm = true;
                                            o.split = false;
                                        }
                                    }
                                }
                                o.shown = d;
                            }
                            let t = egui::RichText::new(label).color(if sel { egui::Color32::WHITE } else { look::text() });
                            let l = ui.add(egui::Label::new(t).sense(egui::Sense::click()));
                            if l.clicked() {
                                o.shown = d;
                            }
                        });
                        if sel {
                            // (the whole row, as Logic's selected destination)
                            let mut row = r.response.rect.expand2(vec2(2.0, 1.0));
                            row.max.x = ui.max_rect().max.x + 2.0;
                            ui.painter().set(bg, egui::Shape::rect_filled(row, 3.0, look::accent()));
                        }
                    }
                });
            });
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_min_width(330.0);
                match o.shown {
                    Dest::Pcm => {
                        pcm_options(ui, &mut o);
                        // (Split turns Burn off)
                        if o.split {
                            o.burn.on = false;
                        }
                    }
                    Dest::Mp3 => mp3_options(ui, &mut o),
                    Dest::M4a => m4a_options(ui, &mut o),
                    Dest::Burn => crate::cdburn::options(ui, &mut o.burn, o.bits),
                }
            });
        });
        ui.separator();
        egui::Grid::new("share-bounce-common").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
            lab(ui, "Start:");
            ui.add(egui::DragValue::new(&mut o.start).speed(bar as f64 / 8.0).custom_formatter(|_, _| starts.0.clone()));
            ui.end_row();
            lab(ui, "End:");
            ui.add(egui::DragValue::new(&mut o.end).speed(bar as f64 / 8.0).custom_formatter(|_, _| starts.1.clone()));
            ui.end_row();
            lab(ui, "Mode:");
            combo(ui, "share-mode", &mut o.mode, &Mode::ALL.map(|m| (m, m.name().to_string())));
            ui.end_row();
            ui.label("");
            ui.vertical(|ui| {
                ui.add_enabled(cycle, egui::Checkbox::new(&mut o.second_pass, "Bounce 2nd Cycle Pass"));
                ui.checkbox(&mut o.tail, "Include Audio Tail");
                ui.add_enabled(o.pcm, egui::Checkbox::new(&mut o.tempo_info, "Include Tempo Information"));
            });
            ui.end_row();
            lab(ui, "Normalize:");
            combo(ui, "share-normalize", &mut o.normalize, &Normalize::ALL.map(|n| (n, n.name().to_string())));
            ui.end_row();
        });
        if o.end <= o.start {
            o.end = o.start + bar;
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(o.pcm || o.mp3 || o.m4a, |ui| {
                    if primary(ui, if o.burn.on { "Bounce and Burn" } else { "OK" }) {
                        go = true;
                    }
                });
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
    });
    with(|s| {
        s.bounce = if close || go { None } else { Some(o.clone()) };
        s.last = Some(o.clone());
    });
    if go {
        let dir = bounce_folder(app);
        let _ = std::fs::create_dir_all(&dir);
        let ext = if o.pcm { o.file_type.ext() } else if o.mp3 { "mp3" } else { "m4a" };
        let chosen = if app.automated { std::env::var("LG_BOUNCE_PATH").ok().map(PathBuf::from) } else { kn_ui::files::FileDialog::new().set_title("Bounce Output 1-2").set_directory(&dir).set_can_create_directories(true).set_file_name(format!("Output 1-2.{ext}")).save_file() };
        let Some(chosen) = chosen else { return };
        if let Some(d) = chosen.parent() {
            app.logic_settings.0.insert("Bounce / Folder".into(), d.to_string_lossy().to_string());
            app.logic_settings.save();
        }
        crate::cdburn::arm(&o.burn, "Output 1-2");
        let p = plan(app, &o, chosen.with_extension(""));
        start_job(app, p, After::Bounce { add_to_project: o.add_to_project, add_to_library: o.add_to_library });
    }
}

fn pcm_options(ui: &mut egui::Ui, o: &mut Opts) {
    egui::Grid::new("share-pcm").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        lab(ui, "File Format:");
        combo(ui, "share-ft", &mut o.file_type, &FileType::ALL.map(|f| (f, f.name().to_string())));
        ui.end_row();
        lab(ui, "Resolution:");
        combo(ui, "share-bits", &mut o.bits, &BITS.map(|(n, b)| (b, n.to_string())));
        ui.end_row();
        lab(ui, "Sample Rate:");
        combo(ui, "share-rate", &mut o.rate, &RATES.map(|r| (r, if r % 1000 == 0 { format!("{} kHz", r / 1000) } else { format!("{:.3} kHz", r as f64 / 1000.0).trim_end_matches('0').to_string() })));
        ui.end_row();
        lab(ui, "File Type:");
        combo(ui, "share-split", &mut o.split, &[(false, "Interleaved".to_string()), (true, "Split".to_string())]);
        ui.end_row();
        lab(ui, "Dithering:");
        ui.add_enabled_ui(o.bits < 32, |ui| combo(ui, "share-dither", &mut o.dither, &Dither::ALL.map(|d| (d, d.name().to_string()))));
        ui.end_row();
        // (the checkboxes in the controls' column)
        ui.label("");
        ui.vertical(|ui| {
            ui.add_enabled(false, egui::Checkbox::new(&mut false, "Surround Bounce")).on_disabled_hover_text("The project is stereo (File ▸ Project Settings ▸ Audio sets a surround format)");
            ui.checkbox(&mut o.add_to_library, "Add to Music Library");
            ui.checkbox(&mut o.add_to_project, "Add to Project");
        });
        ui.end_row();
    });
}

fn mp3_options(ui: &mut egui::Ui, o: &mut Opts) {
    let rates = MP3_RATES.map(|r| (r, format!("{r} kbit/s")));
    egui::Grid::new("share-mp3").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        lab(ui, "Bit Rate (Mono):");
        combo(ui, "share-mp3-mono", &mut o.mp3_mono, &rates);
        ui.end_row();
        lab(ui, "Bit Rate (Stereo):");
        combo(ui, "share-mp3-stereo", &mut o.mp3_stereo, &rates);
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut o.mp3_vbr, "Use Variable Bit Rate Encoding (VBR)");
        ui.end_row();
        lab(ui, "Quality:");
        combo(ui, "share-mp3-q", &mut o.mp3_quality, &[0, 1, 2, 3, 4].map(|k| (k, QUALITY[k].to_string())));
        ui.end_row();
        ui.label("");
        ui.vertical(|ui| {
            ui.checkbox(&mut o.mp3_best, "Use best encoding");
            ui.checkbox(&mut o.mp3_filter, "Filter frequencies below 10 Hz");
        });
        ui.end_row();
        lab(ui, "Stereo Mode:");
        combo(ui, "share-mp3-st", &mut o.mp3_joint, &[(true, "Joint Stereo".to_string()), (false, "Normal Stereo".to_string())]);
        ui.end_row();
        ui.label("");
        ui.horizontal(|ui| {
            ui.checkbox(&mut o.mp3_id3, "Write ID3 Tags");
            if ui.add_enabled(o.mp3_id3, egui::Button::new("ID3 Settings…")).clicked() {
                with(|s| s.id3_open = true);
            }
        });
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut o.add_to_library, "Add to Music Library");
        ui.end_row();
    });
}

fn m4a_options(ui: &mut egui::Ui, o: &mut Opts) {
    egui::Grid::new("share-m4a").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
        lab(ui, "Encoding:");
        combo(ui, "share-m4a-enc", &mut o.m4a_lossless, &[(false, "AAC".to_string()), (true, "Apple Lossless".to_string())]);
        ui.end_row();
        lab(ui, "Bit Rate:");
        ui.add_enabled_ui(!o.m4a_lossless, |ui| combo(ui, "share-m4a-rate", &mut o.m4a_rate, &M4A_RATES.map(|r| (r, format!("{r} kbit/s")))));
        ui.end_row();
        ui.label("");
        ui.add_enabled(!o.m4a_lossless, egui::Checkbox::new(&mut o.m4a_vbr, "Encode with variable bit rate (VBR)"));
        ui.end_row();
        ui.label("");
        ui.checkbox(&mut o.add_to_library, "Add to Music Library");
        ui.end_row();
    });
}

fn id3_window(app: &mut App, ctx: &egui::Context) {
    if !with(|s| s.id3_open) {
        return;
    }
    let Some(mut o) = with(|s| s.bounce.clone()) else {
        with(|s| s.id3_open = false);
        return;
    };
    let defaults = id3_defaults(app);
    let mut editing = with(|s| s.id3_editing);
    let mut done = false;
    egui::Window::new("ID3 Settings").id(egui::Id::unique("share-id3")).collapsible(false).resizable(false).show(ctx, |ui| {
        egui::Grid::new("share-id3-grid").num_columns(2).striped(true).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
            ui.label(egui::RichText::new("ID3 Frame").strong());
            ui.label(egui::RichText::new("Content").strong());
            ui.end_row();
            for (k, (frame, _)) in ID3_FRAMES.iter().enumerate() {
                ui.label(*frame);
                if editing == Some(k) {
                    let r = ui.add(egui::TextEdit::singleline(&mut o.id3[k]).desired_width(220.0));
                    r.request_focus();
                    if r.lost_focus() {
                        editing = None;
                    }
                } else {
                    let shown = if o.id3[k].is_empty() && o.id3_defaults { defaults[k].clone() } else { o.id3[k].clone() };
                    let r = ui.add(egui::Label::new(if shown.is_empty() { " ".to_string() } else { shown }).sense(egui::Sense::click()));
                    if r.double_clicked() {
                        editing = Some(k);
                    }
                }
                ui.end_row();
            }
        });
        ui.checkbox(&mut o.id3_defaults, "Use default values");
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary(ui, "OK") {
                    done = true;
                }
            });
        });
    });
    with(|s| {
        s.id3_editing = editing;
        s.id3_open = !done;
        s.bounce = Some(o);
    });
}

fn share_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut d) = with(|s| s.share.clone()) else { return };
    let mut close = false;
    let mut go = false;
    let title = match d.kind {
        ShareKind::Music => "Share to Music",
        ShareKind::VoiceMemos => "Share to Voice Memos",
        ShareKind::Mail => "Mail",
    };
    let has_part = app.cycle_on() || !app.region_selection().is_empty();
    egui::Window::new(title).id(egui::Id::unique("share-song")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
        if d.ask_save {
            ui.label("The project has changes that aren't saved.\nDo you want to save it and share the saved project?");
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                if primary(ui, "Save and Share") {
                    app.save();
                    go = true;
                }
            });
            return;
        }
        if d.kind == ShareKind::Mail {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut d.project, true, "Share as Project");
                ui.selectable_value(&mut d.project, false, "Share as Song");
            });
            ui.separator();
        }
        let song = !d.project;
        egui::Grid::new("share-song-grid").num_columns(2).spacing(vec2(8.0, 6.0)).show(ui, |ui| {
            if song {
                lab(ui, "Title:");
                ui.add(egui::TextEdit::singleline(&mut d.title).desired_width(260.0));
                ui.end_row();
            }
            if d.kind == ShareKind::Music {
                for (n, v) in [("Artist:", &mut d.artist), ("Composer:", &mut d.composer), ("Album:", &mut d.album), ("Playlist:", &mut d.playlist)] {
                    ui.label(n);
                    ui.add(egui::TextEdit::singleline(v).desired_width(260.0));
                    ui.end_row();
                }
            }
            if song {
                lab(ui, "Quality:");
                combo(ui, "share-q", &mut d.quality, &[0, 1, 2, 3, 4].map(|k| (k, SHARE_QUALITY[k].to_string())));
                ui.end_row();
            }
        });
        if song {
            ui.add_enabled(has_part, egui::Checkbox::new(&mut d.part, "Share cycle area or selected regions only"));
            if d.kind == ShareKind::Music {
                ui.label(egui::RichText::new(format!("Adds the song to {}", library_dir().display())).small().color(look::text_dim()));
            }
        } else {
            ui.label(egui::RichText::new("The whole project package, with all its assets").small().color(look::text_dim()));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary(ui, "Share") {
                    go = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
    });
    if go && d.kind == ShareKind::Mail && d.project && app.dirty && !d.ask_save {
        d.ask_save = true;
        go = false;
    }
    with(|s| s.share = if close || go { None } else { Some(d.clone()) });
    if go {
        run_share(app, d);
    }
}

fn progress(app: &mut App, ctx: &egui::Context) {
    let Some(job) = with(|s| s.job.clone()) else { return };
    let (frac, done) = {
        let mut j = job.lock().unwrap_or_else(|e| e.into_inner());
        (j.progress, j.done.take().map(|d| (d, j.then.clone())))
    };
    if let Some((res, then)) = done {
        with(|s| s.job = None);
        finish(app, then, res);
        return;
    }
    let mut cancel = false;
    egui::Window::new("Bounce").id(egui::Id::unique("share-progress")).collapsible(false).resizable(false).title_bar(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        ui.label("Bouncing…");
        ui.add(egui::ProgressBar::new(frac).desired_width(260.0));
        if ui.button("Cancel").clicked() {
            cancel = true;
        }
    });
    if cancel {
        job.lock().unwrap_or_else(|e| e.into_inner()).cancel = true;
    }
    ctx.request_repaint_after(Duration::from_millis(100));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let ctx = egui::Context::default();
        let mut a = App::new(&eframe::CreationContext::_new_kittest(ctx), None);
        a.automated = true;
        a.set_project(crate::demo::song(), true);
        a.load_engine();
        a
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lg-share-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_range_is_the_cycle_else_the_selection_else_the_project() {
        let mut a = app();
        let whole = bounce_range(&a);
        assert_eq!(whole.0, 0);
        assert!(whole.1 > 3840);
        let r = a.project.as_ref().unwrap().tracks[0].regions[0].clone();
        a.selected_region = Some((0, 0));
        assert_eq!(bounce_range(&a), (r.start, r.start + r.length));
        a.cycle = (3840, 7680);
        a.engine.lock().unwrap().cycle_on = true;
        assert_eq!(bounce_range(&a), (3840, 7680));
    }

    #[test]
    fn a_bounce_writes_every_destination_with_one_name() {
        let a = app();
        let d = tmp("dest");
        let o = Opts { start: 0, end: 3840, mp3: true, m4a: true, file_type: FileType::Aiff, bits: 16, rate: 44_100, dither: Dither::Powr2, tail: false, normalize: Normalize::On, ..Default::default() };
        let files = match bounce_now(&a, &o, d.join("Output 1-2")) {
            Ok(f) => f,
            Err(e) if e.contains("ffmpeg") => {
                eprintln!("{e}: skipped");
                return;
            }
            Err(e) => panic!("{e}"),
        };
        let names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, ["Output 1-2.aif", "Output 1-2.mp3", "Output 1-2.m4a"]);
        let f = lg_audio::AudioFile::read(&files[0]).unwrap();
        assert_eq!((f.rate, f.bits, f.channels), (44_100, 16, 2));
        // one bar at the demo's tempo, exactly (no tail)
        let secs = lg_core::timeline::Map::of(a.project.as_ref().unwrap()).secs_at(3840.0);
        assert!((f.seconds() - secs).abs() < 0.01, "{} vs {secs}", f.seconds());
        // Normalize On: the peak at full scale
        let peak = f.data.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak > 0.97, "{peak}");
        for x in &files[1..] {
            assert!(std::fs::metadata(x).unwrap().len() > 1000);
        }
    }

    #[test]
    fn split_caf_and_tempo_information() {
        let a = app();
        let d = tmp("split");
        let o = Opts { start: 0, end: 1920, split: true, file_type: FileType::Wave, bits: 24, tail: false, ..Default::default() };
        let files = bounce_now(&a, &o, d.join("Mix")).unwrap();
        let names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, ["Mix.L.wav", "Mix.R.wav"]);
        let f = lg_audio::AudioFile::read(&files[0]).unwrap();
        assert_eq!(f.channels, 1);
        assert!(f.extra.iter().any(|(id, _)| id == b"acid"), "the tempo chunk");
        let caf = caf_bytes(48_000, &[vec![0.5, -0.5], vec![0.0, 1.0]], 16);
        assert_eq!(&caf[0..4], b"caff");
        assert_eq!(&caf[8..12], b"desc");
        // data: edit count then big-endian frames L R L R
        let data = &caf[caf.len() - 8..];
        assert_eq!(i16::from_be_bytes([data[0], data[1]]), 16384);
        assert_eq!(i16::from_be_bytes([data[6], data[7]]), 32767);
    }

    #[test]
    fn the_tail_and_the_second_cycle_pass() {
        let a = app();
        let e = || crate::surround_ui::offline(&a).unwrap();
        let (plain, _) = render(e(), 0, 3840, false, false, false, None).unwrap();
        let (tail, _) = render(e(), 0, 3840, false, true, false, None).unwrap();
        assert!(tail.len() >= plain.len());
        let (second, _) = render(e(), 0, 3840, true, false, false, None).unwrap();
        assert_eq!(second.len(), plain.len(), "the file is one pass long");
    }

    #[test]
    fn dither_quantises_to_the_grid_and_shapes_the_noise() {
        let x: Vec<f32> = (0..48_000).map(|k| 0.3 * (k as f32 * 0.01).sin()).collect();
        for d in [Dither::Powr1, Dither::Powr2, Dither::Powr3, Dither::Uv22hr] {
            let mut y = x.clone();
            dither(&mut y, 16, d, 1);
            for v in &y {
                let s = v * 32767.0;
                assert!((s - s.round()).abs() < 1e-2, "{d:?}");
            }
            let err: f32 = y.iter().zip(&x).map(|(a, b)| (a - b).abs()).sum::<f32>() / x.len() as f32;
            assert!(err < 4.0 / 32767.0, "{d:?}: {err}");
        }
        // noise shaping: less of the error at low frequencies than plain dither
        let low = |d: Dither| {
            let mut y = x.clone();
            dither(&mut y, 16, d, 3);
            let e: Vec<f32> = y.iter().zip(&x).map(|(a, b)| a - b).collect();
            // a crude low-pass: the error's running mean over 32 samples
            e.chunks(32).map(|c| c.iter().sum::<f32>().powi(2)).sum::<f32>()
        };
        assert!(low(Dither::Powr2) < low(Dither::Powr1));
        let mut same = x.clone();
        dither(&mut same, 16, Dither::None, 1);
        assert_eq!(same, x);
    }

    #[test]
    fn rates_convert_with_the_length_kept() {
        let x: Vec<f32> = (0..4800).map(|k| (k as f32 * 0.05).sin()).collect();
        assert_eq!(convert_rate(&x, 48_000, 44_100).len(), 4410);
        assert_eq!(convert_rate(&x, 48_000, 96_000).len(), 9600);
        assert_eq!(convert_rate(&x, 48_000, 11_025).len(), 1103);
        // a 20 kHz tone gone at 11.025 kHz (no aliasing)
        let hi: Vec<f32> = (0..4800).map(|k| (k as f32 * 2.0 * std::f32::consts::PI * 20_000.0 / 48_000.0).sin()).collect();
        let y = convert_rate(&hi, 48_000, 11_025);
        let rms = (y[100..1000].iter().map(|v| v * v).sum::<f32>() / 900.0).sqrt();
        assert!(rms < 0.05, "{rms}");
    }

    #[test]
    fn encoder_arguments_follow_the_options() {
        let o = Opts { mp3_stereo: 192, mp3_joint: false, mp3_filter: true, ..Default::default() };
        let a = mp3_args(&o, false, Path::new("a.wav"), Path::new("a.mp3"), &[("title".into(), "Song".into()), ("TKEY".into(), "C".into())]);
        let s = a.join(" ");
        assert!(s.contains("-b:a 192k") && s.contains("-joint_stereo 0") && s.contains("highpass=f=10") && s.contains("title=Song") && s.contains("TKEY=C"), "{s}");
        let v = mp3_args(&Opts { mp3_vbr: true, ..Default::default() }, true, Path::new("a"), Path::new("b"), &[]).join(" ");
        assert!(v.contains("-q:a") && !v.contains("-b:a"), "{v}");
        let m = m4a_args(&Opts { m4a_lossless: true, ..Default::default() }, Path::new("a"), Path::new("b"), &[]).join(" ");
        assert!(m.contains("alac") && !m.contains("-b:a"), "{m}");
        assert_eq!(share_opts(4).file_type, FileType::Aiff);
        assert!(share_opts(3).m4a_lossless);
    }

    #[test]
    fn id3_defaults_and_selection_midi() {
        let mut a = app();
        let d = id3_defaults(&a);
        let p = a.project.as_ref().unwrap();
        assert_eq!(d[0], p.name);
        assert_eq!(d[8], format!("{}", p.tempo.round() as i64));
        assert!(!d[9].is_empty(), "a key, C when the project has none");
        a.selected_region = None;
        a.selected_regions.clear();
        assert!(selection_midi(&a).is_none());
        a.selected_region = Some((0, 0));
        let bytes = selection_midi(&a).unwrap();
        let m = lg_core::midi::read(&bytes).unwrap();
        assert_eq!(m.tracks.len(), 1);
    }

    #[test]
    fn music_paths_and_playlists() {
        let d = tmp("music");
        // (only this test reads it)
        unsafe { std::env::set_var("LG_MUSIC_LIBRARY", &d) };
        let s = ShareDlg { kind: ShareKind::Music, title: "My Song".into(), artist: "Me".into(), composer: String::new(), album: String::new(), playlist: "Demos".into(), quality: 2, part: false, project: false, ask_save: false };
        let f = music_path(&s, "m4a");
        assert_eq!(f, d.join("Me").join("Unknown Album").join("My Song.m4a"));
        let list = add_to_playlist("Demos", &f).unwrap();
        add_to_playlist("Demos", &f).unwrap();
        let text = std::fs::read_to_string(list).unwrap();
        assert_eq!(text.matches("My Song.m4a").count(), 1);
    }
}
