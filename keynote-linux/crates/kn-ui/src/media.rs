//! Movie and audio playback in the slideshow through the system `ffmpeg` / `ffplay` binaries.
//!
//! The movie bytes are copied once into `$XDG_CACHE_HOME/keydeck/movies/`. A player owns a
//! background thread that runs `ffmpeg ... -f rawvideo -pix_fmt rgba -` and keeps the latest
//! decoded frame (paced by the wall clock) in a shared slot, plus an `ffplay -nodisp` child for
//! the sound. The UI thread only uploads the newest frame to a texture; it never waits on a child.
//! Dropping a player kills its children (the worker thread reaps them).

use std::collections::HashSet;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use egui::{ColorImage, TextureHandle, TextureOptions};
use kn_format::ObjId;
use kn_model::{Document, MovieData};

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("keydeck").join("movies")
}

fn fnv(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h ^= u64::from(*b);
        *h = h.wrapping_mul(0x100_0000_01b3);
    }
}

/// Files being written by a background thread right now.
fn writing() -> &'static Mutex<HashSet<PathBuf>> {
    static W: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    W.get_or_init(Default::default)
}

/// Where the movie with `data_id` lives on disk; starts copying it there (in the background)
/// if it isn't cached yet. Returns the path and the expected size.
pub fn cache_file(doc: &Document, data_id: u64) -> Option<(PathBuf, u64)> {
    let bytes = doc.data(data_id)?;
    let name = doc.data_name(data_id).unwrap_or("movie");
    let base = name.rsplit('/').next().unwrap_or(name);
    let clean: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' }).collect();
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let n = bytes.len();
    fnv(&mut h, &bytes[..n.min(65536)]);
    fnv(&mut h, &bytes[n.saturating_sub(65536)..]);
    fnv(&mut h, &(n as u64).to_le_bytes());
    let path = cache_dir().join(format!("{h:016x}-{clean}"));
    let len = n as u64;
    if std::fs::metadata(&path).is_ok_and(|m| m.len() == len) {
        return Some((path, len));
    }
    let mut w = writing().lock().ok()?;
    if w.insert(path.clone()) {
        let data = bytes.to_vec();
        let p = path.clone();
        std::thread::spawn(move || {
            let tmp = p.with_extension("part");
            let ok = std::fs::create_dir_all(cache_dir()).is_ok() && std::fs::write(&tmp, &data).is_ok() && std::fs::rename(&tmp, &p).is_ok();
            if !ok {
                eprintln!("couldn't cache movie at {}", p.display());
            }
            if let Ok(mut w) = writing().lock() {
                w.remove(&p);
            }
        });
    }
    Some((path, len))
}

/// What to play.
#[derive(Clone, Debug)]
pub struct Spec {
    pub path: PathBuf,
    pub len: u64,
    pub start: f32,
    /// End time in seconds (0 = to the end of the file).
    pub end: f32,
    /// 0..1
    pub volume: f32,
    pub looping: bool,
    /// Decoded frame size in pixels (None = sound only).
    pub video: Option<[u32; 2]>,
}

impl Spec {
    pub fn new(path: PathBuf, len: u64, m: &MovieData, video: Option<[u32; 2]>) -> Spec {
        Spec {
            path,
            len,
            start: m.start.max(0.0),
            end: if m.end > m.start + 0.01 { m.end } else { 0.0 },
            volume: m.volume.clamp(0.0, 1.0),
            looping: m.loop_mode != 0,
            video: if m.audio_only { None } else { video.map(|[w, h]| [(w.max(2) + 1) & !1, (h.max(2) + 1) & !1]) },
        }
    }
}

struct Frame {
    seq: u64,
    size: [usize; 2],
    rgba: Vec<u8>,
}

#[derive(Default)]
struct Shared {
    stop: AtomicBool,
    done: AtomicBool,
    frame: Mutex<Option<Frame>>,
    children: Mutex<Vec<Child>>,
    /// Slideshow controls: paused, where to jump (seconds), where it is and how long it is
    /// (milliseconds; length 0 = unknown).
    paused: AtomicBool,
    seek: Mutex<Option<f64>>,
    pos_ms: std::sync::atomic::AtomicU64,
    len_ms: std::sync::atomic::AtomicU64,
}

impl Shared {
    fn kill_all(&self) {
        if let Ok(mut ch) = self.children.lock() {
            for c in ch.iter_mut() {
                let _ = c.kill();
            }
        }
    }

    /// Kill and reap the children (worker thread only: `wait` after `kill` returns at once).
    fn reap(&self) {
        let list: Vec<Child> = self.children.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default();
        for mut c in list {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Freeze (SIGSTOP) or continue (SIGCONT) the decoder and sound processes.
    fn signal(&self, stop: bool) {
        if let Ok(ch) = self.children.lock() {
            for c in ch.iter() {
                let _ = Command::new("kill").arg(if stop { "-STOP" } else { "-CONT" }).arg(c.id().to_string()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
            }
        }
    }

    fn take_seek(&self) -> Option<f64> {
        self.seek.lock().ok().and_then(|mut s| s.take())
    }

    /// While paused: wait (frozen) until resumed, stopped or asked to jump. Returns how long it
    /// was paused.
    fn hold(&self) -> Duration {
        if !self.paused.load(Ordering::Relaxed) {
            return Duration::ZERO;
        }
        let t = Instant::now();
        self.signal(true);
        while self.paused.load(Ordering::Relaxed) && !self.stopped() && self.seek.lock().is_ok_and(|s| s.is_none()) {
            std::thread::sleep(Duration::from_millis(20));
        }
        self.signal(false);
        t.elapsed()
    }
}

/// One playing movie (or sound).
pub struct Player {
    pub drawable: ObjId,
    pub data_id: u64,
    shared: Arc<Shared>,
    tex: Option<TextureHandle>,
    seq: u64,
}

impl Player {
    pub fn start(ctx: &egui::Context, drawable: ObjId, data_id: u64, spec: Spec) -> Player {
        let shared = Arc::new(Shared::default());
        let sh = shared.clone();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            worker(&sh, &spec, &ctx2);
            sh.reap();
            sh.done.store(true, Ordering::Relaxed);
        });
        Player { drawable, data_id, shared, tex: None, seq: 0 }
    }

    /// Texture with the newest decoded frame (uploads it if it changed). None before the first frame.
    pub fn texture(&mut self, ctx: &egui::Context) -> Option<&TextureHandle> {
        let f = self.shared.frame.lock().ok().and_then(|mut f| f.take());
        if let Some(f) = f {
            if f.seq != self.seq {
                self.seq = f.seq;
                let img = ColorImage::from_rgba_unmultiplied(f.size, &f.rgba);
                match &mut self.tex {
                    Some(t) if t.size() == f.size => t.set(img, TextureOptions::LINEAR),
                    _ => self.tex = Some(ctx.load_texture(format!("movie-{}", self.drawable), img, TextureOptions::LINEAR)),
                }
            }
        }
        self.tex.as_ref()
    }

    /// Live video: the default camera (Video4Linux /dev/video0, through ffmpeg) at `w` x `h`,
    /// filling the frame (cropped to its aspect).
    pub fn camera(ctx: &egui::Context, drawable: ObjId, w: u32, h: u32) -> Player {
        let shared = Arc::new(Shared::default());
        let sh = shared.clone();
        let ctx2 = ctx.clone();
        let (w, h) = ((w.max(2) + 1) & !1, (h.max(2) + 1) & !1);
        std::thread::spawn(move || {
            camera_worker(&sh, w, h, &ctx2);
            sh.reap();
            sh.done.store(true, Ordering::Relaxed);
        });
        Player { drawable, data_id: 0, shared, tex: None, seq: 0 }
    }

    pub fn finished(&self) -> bool {
        self.shared.done.load(Ordering::Relaxed)
    }

    pub fn paused(&self) -> bool {
        self.shared.paused.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, on: bool) {
        self.shared.paused.store(on, Ordering::Relaxed);
    }

    /// Jump to `secs` from the movie's start (clamped to its length).
    pub fn seek(&self, secs: f64) {
        let len = self.length();
        let t = if len > 0.0 { secs.clamp(0.0, (len - 0.05).max(0.0)) } else { secs.max(0.0) };
        if let Ok(mut s) = self.shared.seek.lock() {
            *s = Some(t);
        }
        self.shared.pos_ms.store((t * 1000.0) as u64, Ordering::Relaxed);
    }

    /// Where it is playing (seconds).
    pub fn position(&self) -> f64 {
        self.shared.pos_ms.load(Ordering::Relaxed) as f64 / 1000.0
    }

    /// How long it is (seconds; 0 when unknown).
    pub fn length(&self) -> f64 {
        self.shared.len_ms.load(Ordering::Relaxed) as f64 / 1000.0
    }

    /// A camera or movie that can be paused and moved through (not live video).
    pub fn controllable(&self) -> bool {
        self.length() > 0.0
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.kill_all();
    }
}

/// (frames per second, has video, has audio) from ffprobe.
fn probe(path: &PathBuf) -> (f64, bool, bool) {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "stream=codec_type,avg_frame_rate", "-of", "csv=p=0"])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else { return (30.0, true, true) };
    let text = String::from_utf8_lossy(&out.stdout);
    let (mut fps, mut v, mut a) = (0.0, false, false);
    for line in text.lines() {
        let mut it = line.split(',');
        match (it.next().map(str::trim), it.next().map(str::trim)) {
            (Some("video"), r) => {
                v = true;
                if let Some((n, d)) = r.and_then(|r| r.split_once('/')) {
                    let (n, d) = (n.parse::<f64>().unwrap_or(0.0), d.parse::<f64>().unwrap_or(0.0));
                    if d > 0.0 && fps == 0.0 {
                        fps = n / d;
                    }
                }
            }
            (Some("audio"), _) => a = true,
            _ => {}
        }
    }
    if !(1.0..=240.0).contains(&fps) {
        fps = 30.0;
    }
    (fps, v, a)
}

fn worker(sh: &Shared, spec: &Spec, ctx: &egui::Context) {
    // wait for the cache copy (written by another thread); a stream (web video) has none
    let t0 = Instant::now();
    while spec.len > 0 && !std::fs::metadata(&spec.path).is_ok_and(|m| m.len() == spec.len) {
        if sh.stopped() || t0.elapsed() > Duration::from_secs(60) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let (fps, has_video, has_audio) = probe(&spec.path);
    if sh.stopped() {
        return;
    }
    let total = duration(&spec.path).unwrap_or(0.0);
    let end = if spec.end > spec.start { f64::from(spec.end) } else { total };
    sh.len_ms.store((end.max(0.0) * 1000.0) as u64, Ordering::Relaxed);
    // keep decoding cheap: at most 60 fps
    let fps = if fps > 60.0 { fps / (fps / 60.0).ceil() } else { fps };
    let video = spec.video.filter(|_| has_video);
    let audio = has_audio && spec.volume > 0.001;
    if video.is_none() && !audio {
        return;
    }
    let mut seq = 0u64;
    // where this pass starts (a jump restarts the pass there)
    let mut offset = f64::from(spec.start);
    'pass: loop {
        if sh.stopped() {
            return;
        }
        let dur = (spec.end > spec.start).then(|| (f64::from(spec.end) - offset).max(0.01));
        sh.pos_ms.store((offset * 1000.0) as u64, Ordering::Relaxed);
        // a jump asked for: kill this pass's processes and start again at `t`
        macro_rules! jump {
            ($t:expr) => {{
                sh.reap();
                offset = $t;
                continue 'pass;
            }};
        }
        if audio {
            let mut c = Command::new("ffplay");
            c.args(["-nodisp", "-autoexit", "-loglevel", "quiet", "-volume", &format!("{}", (spec.volume * 100.0).round() as u32)]);
            if offset > 0.0 {
                c.args(["-ss", &format!("{:.3}", offset)]);
            }
            if let Some(d) = dur {
                c.args(["-t", &format!("{d:.3}")]);
            }
            c.arg(&spec.path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
            match c.spawn() {
                Ok(child) => {
                    if let Ok(mut ch) = sh.children.lock() {
                        ch.push(child);
                    }
                }
                Err(e) => eprintln!("couldn't start ffplay: {e}"),
            }
        }
        if let Some([w, h]) = video {
            let mut c = Command::new("ffmpeg");
            c.args(["-nostdin", "-loglevel", "error", "-threads", "0"]);
            if offset > 0.0 {
                c.args(["-ss", &format!("{:.3}", offset)]);
            }
            if let Some(d) = dur {
                c.args(["-t", &format!("{d:.3}")]);
            }
            c.arg("-i").arg(&spec.path);
            c.args(["-an", "-vf", &format!("fps={fps:.4},scale={w}:{h}:flags=bilinear"), "-f", "rawvideo", "-pix_fmt", "rgba", "-"]);
            c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
            let mut child = match c.spawn() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("couldn't start ffmpeg: {e}");
                    return;
                }
            };
            let mut out = child.stdout.take();
            if let Ok(mut ch) = sh.children.lock() {
                ch.push(child);
            }
            if sh.stopped() {
                return;
            }
            let frame_len = (w * h * 4) as usize;
            let mut buf = vec![0u8; frame_len];
            let mut start: Option<Instant> = None;
            let mut n = 0u64;
            while let Some(o) = out.as_mut() {
                if let Some(t) = sh.take_seek() {
                    jump!(t);
                }
                let held = sh.hold();
                if let Some(t) = sh.take_seek() {
                    jump!(t);
                }
                if let Some(s) = start.as_mut() {
                    *s += held;
                }
                if o.read_exact(&mut buf).is_err() || sh.stopped() {
                    break;
                }
                sh.pos_ms.store(((offset + n as f64 / fps) * 1000.0) as u64, Ordering::Relaxed);
                // pace by the wall clock (the pipe's back-pressure keeps ffmpeg just ahead)
                let s = *start.get_or_insert_with(Instant::now);
                let due = s + Duration::from_secs_f64(n as f64 / fps);
                let now = Instant::now();
                if due > now {
                    std::thread::sleep(due - now);
                }
                n += 1;
                seq += 1;
                if let Ok(mut f) = sh.frame.lock() {
                    let rgba = match f.take() {
                        Some(mut old) if old.rgba.len() == frame_len => {
                            old.rgba.copy_from_slice(&buf);
                            old.rgba
                        }
                        _ => buf.clone(),
                    };
                    *f = Some(Frame { seq, size: [w as usize, h as usize], rgba });
                }
                ctx.request_repaint();
            }
        } else {
            // sound only: wait for ffplay to finish
            let mut began = Instant::now();
            loop {
                if sh.stopped() {
                    return;
                }
                if let Some(t) = sh.take_seek() {
                    jump!(t);
                }
                began += sh.hold();
                if let Some(t) = sh.take_seek() {
                    jump!(t);
                }
                sh.pos_ms.store(((offset + began.elapsed().as_secs_f64()) * 1000.0) as u64, Ordering::Relaxed);
                let running = sh
                    .children
                    .lock()
                    .map(|mut ch| ch.iter_mut().any(|c| matches!(c.try_wait(), Ok(None))))
                    .unwrap_or(false);
                if !running {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        // finished one pass: wait for the sound to end too, then reap
        while !sh.stopped() && sh.children.lock().map(|mut ch| ch.iter_mut().any(|c| matches!(c.try_wait(), Ok(None)))).unwrap_or(false) {
            std::thread::sleep(Duration::from_millis(20));
        }
        sh.reap();
        if let Some(t) = sh.take_seek() {
            offset = t;
            continue;
        }
        if !spec.looping || sh.stopped() {
            // at the end, still there to be moved back through (until the slide changes)
            if sh.stopped() || total <= 0.0 {
                return;
            }
            sh.pos_ms.store((end * 1000.0) as u64, Ordering::Relaxed);
            loop {
                if sh.stopped() {
                    return;
                }
                if let Some(t) = sh.take_seek() {
                    offset = t;
                    sh.paused.store(false, Ordering::Relaxed);
                    continue 'pass;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        }
        offset = f64::from(spec.start);
    }
}

static CAMERA: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The camera live video shows (KN_CAMERA, else the chosen one, else the first).
pub fn camera_device() -> String {
    if let Ok(d) = std::env::var("KN_CAMERA") {
        return d;
    }
    if let Some(d) = CAMERA.lock().ok().and_then(|c| c.clone()).filter(|d| !d.is_empty()) {
        return d;
    }
    cameras().into_iter().next().map(|(d, _)| d).unwrap_or_else(|| "/dev/video0".into())
}

pub fn set_camera_device(dev: &str) {
    if let Ok(mut c) = CAMERA.lock() {
        *c = Some(dev.to_string());
    }
}

/// Cameras (video capture devices) with their names, from /sys/class/video4linux. A camera
/// often has several nodes (metadata, IR): only the first node of each named device is kept.
pub fn cameras() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let Ok(rd) = std::fs::read_dir("/sys/class/video4linux") else { return out };
    let mut nodes: Vec<(u32, String, String)> = rd
        .flatten()
        .filter_map(|e| {
            let node = e.file_name().to_string_lossy().to_string();
            let n: u32 = node.strip_prefix("video")?.parse().ok()?;
            let name = std::fs::read_to_string(e.path().join("name")).ok()?.trim().to_string();
            // index 0 is the capture node of a device (others are metadata)
            let index = std::fs::read_to_string(e.path().join("index")).ok().and_then(|i| i.trim().parse::<u32>().ok()).unwrap_or(0);
            (index == 0).then_some((n, format!("/dev/{node}"), name))
        })
        .collect();
    nodes.sort();
    for (_, dev, name) in nodes {
        if !out.iter().any(|(_, n)| *n == name) {
            out.push((dev, name));
        }
    }
    out
}

fn camera_worker(sh: &Shared, w: u32, h: u32, ctx: &egui::Context) {
    let device = camera_device();
    let mut c = Command::new("ffmpeg");
    c.args(["-nostdin", "-loglevel", "error", "-f", "v4l2", "-i", &device]);
    c.args(["-an", "-vf", &format!("scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h}"), "-f", "rawvideo", "-pix_fmt", "rgba", "-"]);
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let Ok(mut child) = c.spawn() else { return };
    let mut out = child.stdout.take();
    if let Ok(mut ch) = sh.children.lock() {
        ch.push(child);
    }
    let frame_len = (w * h * 4) as usize;
    let mut buf = vec![0u8; frame_len];
    let mut seq = 0u64;
    while let Some(o) = out.as_mut() {
        if o.read_exact(&mut buf).is_err() || sh.stopped() {
            break;
        }
        seq += 1;
        if let Ok(mut f) = sh.frame.lock() {
            *f = Some(Frame { seq, size: [w as usize, h as usize], rgba: buf.clone() });
        }
        ctx.request_repaint();
    }
}

/// Length of a movie file in seconds (ffprobe), if known.
pub fn duration(path: &std::path::Path) -> Option<f64> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// One frame of a movie at `t` seconds, as JPEG (Format > Movie > Poster Frame).
pub fn frame_jpeg(path: &std::path::Path, t: f64) -> Option<Vec<u8>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-ss", &format!("{t:.3}"), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-f", "image2", "-c:v", "mjpeg", "-q:v", "3", "-"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout)
}

/// Picture size of a movie's video stream (ffprobe).
pub fn video_size(path: &std::path::Path) -> Option<(f32, f32)> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0"])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let t = String::from_utf8_lossy(&out.stdout);
    let (w, h) = t.trim().split_once(',')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

pub const MOVIE_EXT: &[&str] = &["mp4", "m4v", "mov", "webm", "mkv", "avi"];
pub const AUDIO_EXT: &[&str] = &["m4a", "mp3", "wav", "aac", "aif", "aiff", "flac", "ogg", "opus"];
pub const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg"];

/// What a picked file becomes on the slide.
pub enum Picked {
    Image(String, Vec<u8>),
    Movie(kn_model::insert::NewMovie),
}

/// Read a picture, movie or sound and work out what Keynote stores with it (size, length,
/// poster still). Runs on a background thread.
pub fn prepare(path: &std::path::Path) -> Result<Picked, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "media".into());
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    if IMAGE_EXT.contains(&ext.as_str()) {
        return Ok(Picked::Image(name, bytes));
    }
    let audio_only = AUDIO_EXT.contains(&ext.as_str()) || video_size(path).is_none();
    if !audio_only && !MOVIE_EXT.contains(&ext.as_str()) {
        return Err(format!("\u{201c}{name}\u{201d} isn\u{2019}t a picture, movie or sound Keydeck can use"));
    }
    let duration = duration(path).unwrap_or(0.0) as f32;
    let (natural, poster) = if audio_only { (None, None) } else { (video_size(path), frame_jpeg(path, (duration as f64 * 0.1).min(1.0))) };
    Ok(Picked::Movie(kn_model::insert::NewMovie { bytes, name, poster, natural, duration, audio_only }))
}

/// Insert > Web Video: the playable stream of a YouTube / Vimeo page, looked up once in the
/// background (yt-dlp) and cached. None until it is known (or when it can't be played).
/// The yt-dlp to use: Keydeck's own copy (Help > Update Video Support), a user install, then
/// the system's.
pub fn yt_dlp() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    [home.join(".local/share/keydeck/bin/yt-dlp"), home.join(".local/bin/yt-dlp")].into_iter().find(|p| p.is_file()).unwrap_or_else(|| "yt-dlp".into())
}

/// Why the last web video couldn't play (taken once, to tell the user).
static WEB_ERROR: Mutex<Option<String>> = Mutex::new(None);

pub fn take_web_error() -> Option<String> {
    WEB_ERROR.lock().ok()?.take()
}

/// Keydeck's own JavaScript runtime for yt-dlp (YouTube needs one to unlock its formats).
fn deno() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    [home.join(".local/share/keydeck/bin/deno"), home.join(".deno/bin/deno")].into_iter().find(|p| p.is_file())
}

/// A YouTube / Vimeo page's video as a local file, downloaded in the background with yt-dlp
/// into ~/.cache/keydeck/web (YouTube serves its streams to yt-dlp only, as separate picture
/// and sound that it merges): None until it's there. Kept, so the next play is instant. A
/// failed download is tried again after 30 s.
pub fn web_stream(page: &str) -> Option<String> {
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    let dir = std::env::var_os("XDG_CACHE_HOME").map(std::path::PathBuf::from).unwrap_or_else(|| home.join(".cache")).join("keydeck").join("web");
    let mut h = std::collections::hash_map::DefaultHasher::new();
    page.hash(&mut h);
    let file = dir.join(format!("{:016x}.mp4", h.finish()));
    if file.is_file() {
        return Some(file.to_string_lossy().to_string());
    }
    // page -> when a failed download ended (None while one runs)
    static TRIES: Mutex<Option<HashMap<String, Option<std::time::Instant>>>> = Mutex::new(None);
    let mut g = TRIES.lock().ok()?;
    let map = g.get_or_insert_with(HashMap::new);
    match map.get(page) {
        Some(None) => return None,
        Some(Some(t)) if t.elapsed().as_secs() < 30 => return None,
        _ => {}
    }
    map.insert(page.to_string(), None);
    let page = page.to_string();
    std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        let part = file.with_extension("download.mp4");
        let mut cmd = Command::new(yt_dlp());
        if let Some(d) = deno() {
            cmd.arg("--js-runtimes").arg(format!("deno:{}", d.display()));
        }
        // H.264 picture up to 1080p with AAC sound (easy to decode), else whatever plays
        cmd.args(["--no-playlist", "--no-warnings", "-q", "-f", "bv*[height<=1080][vcodec^=avc1]+ba[ext=m4a]/bv*[height<=1080]+ba/b", "--merge-output-format", "mp4", "--force-overwrites", "-o"])
            .arg(&part)
            .arg(&page)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        let out = cmd.output();
        let ok = out.as_ref().is_ok_and(|o| o.status.success()) && part.is_file() && std::fs::rename(&part, &file).is_ok();
        if !ok {
            let _ = std::fs::remove_file(&part);
            let why = match &out {
                Err(_) => "Web videos need yt-dlp, which isn't installed. Choose Help > Update Video Support.".to_string(),
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr);
                    eprintln!("web video: couldn't get {page}: {}", err.lines().last().unwrap_or(""));
                    // (an out-of-date yt-dlp also says "Unable to download ... HTTP Error 400")
                    let offline = ["Could not resolve", "Temporary failure in name resolution", "Network is unreachable", "No route to host", "timed out", "Connection refused"];
                    if offline.iter().any(|m| err.contains(m)) {
                        "This web video can't play: there's no internet connection.".to_string()
                    } else if err.contains("unavailable") || err.contains("Private video") || err.contains("removed") {
                        "This web video is no longer available.".to_string()
                    } else {
                        "This web video can't play: video support is out of date. Choose Help > Update Video Support.".to_string()
                    }
                }
            };
            if let Ok(mut e) = WEB_ERROR.lock() {
                *e = Some(why);
            }
        }
        if let Ok(mut g) = TRIES.lock() {
            if let Some(m) = g.as_mut() {
                m.insert(page, (!ok).then(std::time::Instant::now));
            }
        }
    });
    None
}

/// Help > Update Video Support: the latest yt-dlp from its official releases into Keydeck's
/// own folder (the system's copy is often too old for YouTube). Runs in the background;
/// the result goes to `done`.
/// Mark a downloaded tool runnable (Unix permissions; nothing to do elsewhere).
fn make_executable(p: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755))
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        Ok(())
    }
}

pub fn update_yt_dlp(done: std::sync::mpsc::Sender<Result<String, String>>) {
    std::thread::spawn(move || {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
        let dir = home.join(".local/share/keydeck/bin");
        let r = (|| -> Result<String, String> {
            // (the downloads below are the Linux builds)
            if !cfg!(target_os = "linux") {
                return Err("Video support can only be updated automatically on Linux".into());
            }
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let part = dir.join("yt-dlp.part");
            let ok = Command::new("curl")
                .args(["-fsSL", "--max-time", "300", "-o"])
                .arg(&part)
                .arg("https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp")
                .status()
                .map_err(|_| "curl isn't installed".to_string())?
                .success();
            if !ok {
                let _ = std::fs::remove_file(&part);
                return Err("Couldn't download yt-dlp (no internet connection?)".into());
            }
            make_executable(&part).map_err(|e| e.to_string())?;
            let bin = dir.join("yt-dlp");
            std::fs::rename(&part, &bin).map_err(|e| e.to_string())?;
            // YouTube also needs a JavaScript runtime: Deno, from its official releases
            if deno().is_none() {
                let zip = dir.join("deno.zip");
                let got = Command::new("curl")
                    .args(["-fsSL", "--max-time", "600", "-o"])
                    .arg(&zip)
                    .arg("https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip")
                    .status()
                    .is_ok_and(|s| s.success());
                if got {
                    let _ = Command::new("unzip").arg("-o").arg("-q").arg(&zip).arg("-d").arg(&dir).status();
                    let _ = make_executable(&dir.join("deno"));
                }
                let _ = std::fs::remove_file(&zip);
            }
            let v = Command::new(&bin).arg("--version").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
            Ok(format!("Video support updated (yt-dlp {v})"))
        })();
        let _ = done.send(r);
    });
}

/// Web videos (Insert > Web Video): a YouTube / Vimeo page's title, author and poster.
pub mod web {
    /// A fetched web video: page URL, title, author and poster picture (PNG / JPEG bytes).
    pub type Fetched = Result<(String, String, String, Vec<u8>), String>;

    /// Run a command and return its stdout, if it succeeds.
    fn run(cmd: &str, args: &[&str]) -> Option<Vec<u8>> {
        let o = std::process::Command::new(cmd).args(args).stdin(std::process::Stdio::null()).output().ok()?;
        o.status.success().then_some(o.stdout)
    }

    fn json_str(v: &str, key: &str) -> String {
        let pat = format!("\"{key}\"");
        let Some(i) = v.find(&pat) else { return String::new() };
        let rest = v[i + pat.len()..].trim_start().trim_start_matches(':').trim_start();
        let Some(rest) = rest.strip_prefix('"') else { return String::new() };
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('u') => {
                        let hex: String = chars.by_ref().take(4).collect();
                        if let Some(c) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                            out.push(c);
                        }
                    }
                    Some(c) => out.push(c),
                    None => break,
                },
                c => out.push(c),
            }
        }
        out
    }

    /// A plain dark poster (with a play triangle) for when the video's picture can't be fetched.
    fn fallback_poster() -> Vec<u8> {
        let (w, h) = (1280u32, 720u32);
        let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([40, 40, 46, 255]));
        let (cx, cy, r) = (w as f32 / 2.0, h as f32 / 2.0, 90.0f32);
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                // play triangle pointing right inside a circle
                if dx * dx + dy * dy < r * r {
                    let inside = dx > -r * 0.35 && dx < r * 0.5 && dy.abs() < (r * 0.5 - dx) * 0.6;
                    img.put_pixel(x, y, if inside { image::Rgba([255, 255, 255, 255]) } else { image::Rgba([90, 90, 98, 255]) });
                }
            }
        }
        let mut out = std::io::Cursor::new(Vec::new());
        let _ = img.write_to(&mut out, image::ImageFormat::Png);
        out.into_inner()
    }

    /// Title, author and poster of a video page via its oEmbed description (YouTube / Vimeo).
    pub fn fetch(url: String) -> Fetched {
        let oembed = if url.contains("vimeo.com") {
            format!("https://vimeo.com/api/oembed.json?url={url}")
        } else {
            format!("https://www.youtube.com/oembed?format=json&url={url}")
        };
        let meta = run("curl", &["-fsL", "--max-time", "15", &oembed]).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default();
        let (title, author) = (json_str(&meta, "title"), json_str(&meta, "author_name"));
        // the largest YouTube still when there is one, else the oEmbed thumbnail
        let id = url.split("v=").nth(1).or_else(|| url.split("youtu.be/").nth(1)).map(|s| s.split(['&', '?', '#']).next().unwrap_or(s).to_string());
        let mut tries: Vec<String> = id.iter().map(|i| format!("https://img.youtube.com/vi/{i}/maxresdefault.jpg")).collect();
        let thumb = json_str(&meta, "thumbnail_url");
        if !thumb.is_empty() {
            tries.push(thumb);
        }
        let poster = tries.iter().find_map(|t| run("curl", &["-fsL", "--max-time", "20", t]).filter(|b| kn_format::objects::image_size(b).is_some())).unwrap_or_else(fallback_poster);
        Ok((url, title, author, poster))
    }

    /// A YouTube or Vimeo video's page (what Keynote accepts).
    pub fn video_link(u: &str) -> bool {
        let u = u.to_lowercase();
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            return false;
        }
        (u.contains("youtube.com/watch?") && u.contains("v=")) || u.contains("youtu.be/") || u.contains("youtube.com/shorts/") || u.contains("youtube.com/embed/") || (u.contains("vimeo.com/") && u.rsplit('/').next().is_some_and(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit())))
    }
}
