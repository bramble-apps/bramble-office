//! Movies (File ▸ Movie ▸): one movie per project, shown in a floating Movie window in sync with
//! the playhead (and moving the playhead from its slider) and as thumbnails on the global Movie
//! track; its audio imported to tracks or the project's mix exported into a copy of the movie;
//! scene cut markers found by cut detection. Video is decoded by the system's FFmpeg programs
//! (ffmpeg / ffprobe, run as separate processes: no libraries linked).
//!
//! Each project keeps its movie (path, start) beside it (~/.config/bramble-studio/movies).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Open Movie…", "Remove Movie", "Show/Hide Movie Window", "Show Movie Window", "Show/Hide Movie Track", "Show Movie Track", "Show/Hide Movie Track Only", "Movie",
    "Import Audio from Movie", "Export Audio to Movie…", "Create Movie Scene Cut Markers (Auto Range)", "Remove Movie Scene Cut Markers (Auto Range)", "Show Movie in Finder",
    "Move Movie Region to Playhead", "Move Movie Region (and SMPTE Position) to Playhead", "Move Movie Region (and SMPTE Position) to Project Start", "Reset Movie Region", "Convert to Scene Marker",
];

/// What ffprobe tells about a movie.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Info {
    pub duration: f64,
    pub fps: f64,
    pub w: u32,
    pub h: u32,
    /// The movie's own SMPTE start (its timecode tag), seconds.
    pub timecode: Option<f64>,
    /// Audio streams (their titles or codecs).
    pub audio: Vec<String>,
}

/// A decoded frame: its movie time, size and RGBA pixels.
#[derive(Clone)]
struct Frame {
    t: f64,
    w: usize,
    h: usize,
    rgba: Vec<u8>,
}

/// The decoder thread: asked for a movie time, it keeps an ffmpeg process streaming frames
/// while the times go forward, restarting it on jumps.
struct Decoder {
    tx: mpsc::Sender<(f64, bool)>,
    latest: Arc<Mutex<Option<Frame>>>,
}

pub struct State {
    pub path: Option<PathBuf>,
    pub info: Option<Info>,
    pub window: bool,
    pub track: bool,
    /// Movie time 0 plays at this project SMPTE time (seconds from SMPTE 0): Movie Start.
    pub start: Option<f64>,
    /// Scene markers: their SMPTE times (seconds); kept where they are when the tempo changes.
    pub scenes: Vec<f64>,
    scale: f32,
    keep_aspect: bool,
    full: bool,
    center: bool,
    dec: Option<Decoder>,
    tex: Option<egui::TextureHandle>,
    shown_t: f64,
    thumbs: BTreeMap<i64, Option<egui::TextureHandle>>,
    thumb_tx: Option<mpsc::Sender<(i64, f64)>>,
    thumb_rx: Option<mpsc::Receiver<(i64, Frame)>>,
    busy: Option<String>,
    job: Option<mpsc::Receiver<Job>>,
    dlg: Option<Dlg>,
    ffmpeg: Option<bool>,
}

impl Default for State {
    fn default() -> State {
        State { path: None, info: None, window: false, track: false, start: None, scenes: Vec::new(), scale: 1.0, keep_aspect: true, full: false, center: false, dec: None, tex: None, shown_t: -1.0, thumbs: BTreeMap::new(), thumb_tx: None, thumb_rx: None, busy: None, job: None, dlg: None, ffmpeg: None }
    }
}

enum Job {
    Imported(Vec<PathBuf>),
    Exported(PathBuf),
    Scenes(Vec<f64>),
    Failed(String),
}

#[derive(Clone)]
enum Dlg {
    /// Import Audio from Movie: the streams and "On separate tracks".
    Import(bool),
    /// Export Audio to Movie: format, sample rate, bit depth, the movie's own tracks kept.
    Export { aac: bool, rate: u32, bits: u32, keep: Vec<bool> },
    /// Removing the movie: its scene markers too?
    Remove,
}

fn have_ffmpeg(s: &mut State) -> bool {
    *s.ffmpeg.get_or_insert_with(|| Command::new("ffprobe").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|x| x.success()))
}

/// ffprobe: duration, frame rate, size, timecode, audio streams.
pub fn probe(path: &Path) -> Option<Info> {
    let out = Command::new("ffprobe").args(["-v", "error", "-show_entries", "format=duration:format_tags=timecode:stream=codec_type,codec_name,width,height,r_frame_rate:stream_tags=timecode,title", "-of", "default=noprint_wrappers=0"]).arg(path).output().ok()?;
    parse_probe(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_probe(text: &str) -> Option<Info> {
    let mut info = Info::default();
    let mut kind = String::new();
    let mut codec = String::new();
    let mut title = String::new();
    let mut tc: Option<String> = None;
    let mut rate_text = String::new();
    for l in text.lines() {
        let l = l.trim();
        if l == "[STREAM]" {
            kind.clear();
            codec.clear();
            title.clear();
        } else if l == "[/STREAM]" {
            if kind == "audio" {
                info.audio.push(if title.is_empty() { format!("Audio {} ({codec})", info.audio.len() + 1) } else { title.clone() });
            }
        } else if let Some((k, v)) = l.split_once('=') {
            match k {
                "codec_type" => kind = v.to_string(),
                "codec_name" => codec = v.to_string(),
                "TAG:title" => title = v.to_string(),
                "width" if kind == "video" || info.w == 0 => info.w = v.parse().unwrap_or(info.w),
                "height" if kind == "video" || info.h == 0 => info.h = v.parse().unwrap_or(info.h),
                "r_frame_rate" if kind == "video" => rate_text = v.to_string(),
                "duration" => info.duration = v.parse().unwrap_or(info.duration),
                "TAG:timecode" => tc = Some(v.to_string()),
                _ => {}
            }
        }
    }
    if let Some((a, b)) = rate_text.split_once('/') {
        let (a, b): (f64, f64) = (a.parse().ok()?, b.parse().ok()?);
        if b > 0.0 {
            info.fps = a / b;
        }
    }
    if info.fps <= 0.0 {
        info.fps = 25.0;
    }
    let nominal = info.fps.round();
    info.timecode = tc.and_then(|t| {
        let f: Vec<f64> = t.split([':', ';', '.']).filter_map(|x| x.parse().ok()).collect();
        (f.len() == 4).then(|| f[0] * 3600.0 + f[1] * 60.0 + f[2] + f[3] / nominal)
    });
    (info.w > 0 && info.h > 0).then_some(info)
}

/// The decode size: at most 960 wide, even numbers.
fn decode_size(info: &Info) -> (usize, usize) {
    let w = (info.w as usize).min(960);
    let h = ((info.h as f64 * w as f64 / info.w.max(1) as f64).round() as usize).max(2);
    (w & !1, h & !1)
}

fn spawn_ffmpeg(path: &Path, t: f64, w: usize, h: usize, frames: Option<u32>) -> Option<Child> {
    let mut c = Command::new("ffmpeg");
    c.args(["-v", "error", "-nostdin", "-ss", &format!("{:.3}", t.max(0.0))]).arg("-i").arg(path).args(["-an", "-vf", &format!("scale={w}:{h}"), "-pix_fmt", "rgba", "-f", "rawvideo"]);
    if let Some(n) = frames {
        c.args(["-frames:v", &n.to_string()]);
    }
    c.arg("-").stdout(Stdio::piped()).stderr(Stdio::null()).stdin(Stdio::null());
    c.spawn().ok()
}

fn start_decoder(path: &Path, info: &Info) -> Decoder {
    let (tx, rx) = mpsc::channel::<(f64, bool)>();
    let latest = Arc::new(Mutex::new(None));
    let l2 = latest.clone();
    let path = path.to_path_buf();
    let fps = info.fps.max(1.0);
    let (w, h) = decode_size(info);
    std::thread::Builder::new().name("studio-movie".into()).spawn(move || {
        let size = w * h * 4;
        let mut child: Option<Child> = None;
        // the time of the next frame the running process gives
        let mut next_t = 0.0;
        loop {
            // the newest request (older ones are stale)
            let Ok(mut req) = rx.recv() else { break };
            while let Ok(r) = rx.try_recv() {
                req = r;
            }
            let (t, playing) = req;
            let behind = t < next_t - 0.5 / fps;
            let far = t > next_t + 1.0;
            if child.is_none() || behind || far || !playing {
                if let Some(mut c) = child.take() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                if !playing {
                    // stopped: the one frame
                    if let Some(mut c) = spawn_ffmpeg(&path, t, w, h, Some(1)) {
                        let mut buf = vec![0u8; size];
                        if c.stdout.as_mut().is_some_and(|o| o.read_exact(&mut buf).is_ok()) {
                            *l2.lock().unwrap() = Some(Frame { t, w, h, rgba: buf });
                        }
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                    next_t = f64::NEG_INFINITY;
                    continue;
                }
                child = spawn_ffmpeg(&path, t, w, h, None);
                next_t = t;
            }
            // read frames up to the time asked
            let Some(c) = child.as_mut() else { continue };
            let Some(out) = c.stdout.as_mut() else { continue };
            let mut buf = vec![0u8; size];
            let mut got = false;
            while next_t <= t + 0.5 / fps {
                if out.read_exact(&mut buf).is_err() {
                    child = None;
                    break;
                }
                got = true;
                next_t += 1.0 / fps;
            }
            if got {
                *l2.lock().unwrap() = Some(Frame { t: next_t - 1.0 / fps, w, h, rgba: buf });
            }
        }
        if let Some(mut c) = child {
            let _ = c.kill();
            let _ = c.wait();
        }
    }).ok();
    Decoder { tx, latest }
}

/// Thumbnails for the Movie track: one worker extracting small frames on request.
fn start_thumbs(s: &mut State, path: &Path, info: &Info) {
    let (tx, rx) = mpsc::channel::<(i64, f64)>();
    let (otx, orx) = mpsc::channel::<(i64, Frame)>();
    let path = path.to_path_buf();
    let h = 54usize;
    let w = (((info.w as f64 / info.h.max(1) as f64) * h as f64).round() as usize).max(2) & !1;
    std::thread::Builder::new().name("studio-thumbs".into()).spawn(move || {
        while let Ok((k, t)) = rx.recv() {
            let Some(mut c) = spawn_ffmpeg(&path, t, w, h, Some(1)) else { continue };
            let mut buf = vec![0u8; w * h * 4];
            if c.stdout.as_mut().is_some_and(|o| o.read_exact(&mut buf).is_ok()) {
                let _ = otx.send((k, Frame { t, w, h, rgba: buf }));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }).ok();
    s.thumb_tx = Some(tx);
    s.thumb_rx = Some(orx);
}

fn file_for(app: &App) -> Option<PathBuf> {
    let project = app.project.as_ref().map(|p| p.path.clone()).filter(|p| !app.demo && p.exists())?;
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    let id: String = project.canonicalize().unwrap_or(project).display().to_string().chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    Some(base.join("bramble-studio").join("movies").join(format!("{id}.txt")))
}

pub fn after_open(app: &mut App) {
    let old = std::mem::take(&mut app.hub.movie);
    app.hub.movie.ffmpeg = old.ffmpeg;
    let Some(t) = file_for(app).and_then(|f| std::fs::read_to_string(f).ok()) else { return };
    for l in t.lines() {
        if let Some(p) = l.strip_prefix("path\t") {
            let p = PathBuf::from(p);
            if p.exists() {
                open_path(app, &p, false);
            }
        } else if let Some(v) = l.strip_prefix("start\t") {
            app.hub.movie.start = v.parse().ok();
        } else if let Some(v) = l.strip_prefix("scenes\t") {
            app.hub.movie.scenes = v.split(',').filter_map(|x| x.parse().ok()).collect();
        } else if l == "track" {
            app.hub.movie.track = true;
        }
    }
}

pub fn save(app: &mut App) {
    if app.automated {
        return;
    }
    let Some(f) = file_for(app) else { return };
    let s = &app.hub.movie;
    let mut t = String::new();
    if let Some(p) = &s.path {
        t += &format!("path\t{}\n", p.display());
    }
    if let Some(st) = s.start {
        t += &format!("start\t{st}\n");
    }
    if !s.scenes.is_empty() {
        t += &format!("scenes\t{}\n", s.scenes.iter().map(|x| format!("{x:.3}")).collect::<Vec<_>>().join(","));
    }
    if s.track {
        t += "track\n";
    }
    if t.is_empty() {
        let _ = std::fs::remove_file(f);
        return;
    }
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, t);
}

/// The movie time at a song position (None before the movie starts or after it ends).
pub fn movie_time(app: &App, tick: f64) -> Option<f64> {
    let s = &app.hub.movie;
    let info = s.info.as_ref()?;
    let (bar1, _, _) = crate::projsettings::smpte(&app.proj);
    let at = app.seconds_at(tick) + bar1;
    let start = s.start.unwrap_or(bar1);
    let t = at - start;
    (t >= 0.0 && t <= info.duration).then_some(t)
}

/// The song position of a movie time.
pub fn tick_of(app: &App, t: f64) -> f64 {
    let (bar1, _, _) = crate::projsettings::smpte(&app.proj);
    let start = app.hub.movie.start.unwrap_or(bar1);
    let (map, tempo) = app.project.as_ref().map_or((Vec::new(), 120.0), |p| (p.tempo_map.clone(), p.tempo));
    crate::syncmidi::tick_at(&map, tempo, start + t - bar1)
}

fn open_path(app: &mut App, p: &Path, show: bool) {
    if !have_ffmpeg(&mut app.hub.movie) {
        app.tell("Movies need FFmpeg (the ffmpeg and ffprobe programs), which isn't installed");
        return;
    }
    let Some(info) = probe(p) else {
        app.tell(format!("“{}” isn't a movie Studio can read", p.file_name().unwrap_or_default().to_string_lossy()));
        return;
    };
    let s = &mut app.hub.movie;
    s.dec = Some(start_decoder(p, &info));
    s.thumbs.clear();
    start_thumbs(s, p, &info);
    s.path = Some(p.to_path_buf());
    s.info = Some(info);
    s.tex = None;
    s.shown_t = -1.0;
    if show {
        s.window = true;
    }
    app.dirty = true;
}

pub fn command(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    match name {
        "Open Movie…" => {
            // a movie open already: the Movie window comes back; open already: choose another
            if app.hub.movie.path.is_some() && !app.hub.movie.window {
                app.hub.movie.window = true;
                return true;
            }
            if app.automated {
                return true;
            }
            if let Some(p) = kn_ui::files::FileDialog::new().set_title("Open Movie").add_filter("Movies", &["mov", "mp4", "m4v", "mkv", "avi", "webm", "mxf", "MOV", "MP4"]).pick_file() {
                open_path(app, &p, true);
            }
        }
        "Remove Movie" => {
            if app.hub.movie.path.is_some() {
                if app.hub.movie.scenes.is_empty() {
                    remove(app, false);
                } else {
                    app.hub.movie.dlg = Some(Dlg::Remove);
                }
            }
        }
        "Show/Hide Movie Window" | "Show Movie Window" => {
            if app.hub.movie.path.is_some() {
                app.hub.movie.window = !app.hub.movie.window;
            }
        }
        "Show/Hide Movie Track" | "Show Movie Track" | "Movie" => {
            app.hub.movie.track = !app.hub.movie.track;
            if app.hub.movie.track {
                app.global_tracks = true;
            }
        }
        "Show/Hide Movie Track Only" => {
            app.hub.movie.track = true;
            app.global_tracks = true;
        }
        "Import Audio from Movie" => {
            if app.hub.movie.info.as_ref().is_some_and(|i| !i.audio.is_empty()) {
                app.hub.movie.dlg = Some(Dlg::Import(false));
            } else {
                app.tell("The movie has no audio");
            }
        }
        "Export Audio to Movie…" => {
            if let Some(i) = &app.hub.movie.info {
                app.hub.movie.dlg = Some(Dlg::Export { aac: true, rate: 48000, bits: 24, keep: vec![false; i.audio.len()] });
            }
        }
        "Create Movie Scene Cut Markers (Auto Range)" => scenes(app, "Auto Range"),
        "Remove Movie Scene Cut Markers (Auto Range)" => remove_scenes(app, "Auto Range"),
        "Show Movie in Finder" => {
            if let Some(d) = app.hub.movie.path.as_ref().and_then(|p| p.parent()) {
                let _ = Command::new("xdg-open").arg(d).spawn();
            }
        }
        "Move Movie Region to Playhead" | "Move Movie Region (and SMPTE Position) to Playhead" => {
            let (bar1, _, _) = crate::projsettings::smpte(&app.proj);
            let at = app.seconds_at(app.position()) + bar1;
            app.hub.movie.start = Some(at);
            app.dirty = true;
        }
        "Move Movie Region (and SMPTE Position) to Project Start" | "Reset Movie Region" => {
            app.hub.movie.start = None;
            app.dirty = true;
        }
        _ => {
            let _ = ctx;
            return false;
        }
    }
    true
}

pub fn checked(app: &App, label: &str) -> Option<bool> {
    match label {
        "Show Movie Track" | "Movie" => Some(app.hub.movie.track),
        "Show Movie Window" => Some(app.hub.movie.window),
        _ => None,
    }
}

fn remove(app: &mut App, scenes_too: bool) {
    if scenes_too {
        remove_scenes(app, "Entire Movie");
    }
    let s = &mut app.hub.movie;
    let ff = s.ffmpeg;
    *s = State::default();
    s.ffmpeg = ff;
    app.dirty = true;
}

/// The range a scene command works on, in movie seconds: Auto Range is the marquee, else the
/// cycle area, else the selected regions, else the whole movie.
fn range(app: &App, how: &str) -> Option<(f64, f64)> {
    let info = app.hub.movie.info.as_ref()?;
    let whole = (0.0, info.duration);
    let to_t = |tick: f64| movie_time(app, tick).unwrap_or(if tick <= 0.0 { 0.0 } else { info.duration });
    let cycle = || app.cycle_on().then(|| (to_t(app.cycle.0 as f64), to_t(app.cycle.1 as f64)));
    let regions = || {
        let (t, r) = app.selected_region?;
        let reg = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?;
        Some((to_t(reg.start as f64), to_t((reg.start + reg.length) as f64)))
    };
    match how {
        "Cycle Area" => cycle(),
        "Selected Regions" => regions(),
        "Entire Movie" => Some(whole),
        _ => cycle().or_else(regions).or(Some(whole)),
    }
}

/// Create Movie Scene Cut Markers: cut detection over the range (a fixed threshold).
fn scenes(app: &mut App, how: &str) {
    let (Some(path), Some((a, b))) = (app.hub.movie.path.clone(), range(app, how)) else { return };
    let (tx, rx) = mpsc::channel();
    app.hub.movie.job = Some(rx);
    app.hub.movie.busy = Some("Detecting scene cuts…".into());
    std::thread::spawn(move || {
        let out = Command::new("ffmpeg").args(["-v", "info", "-nostdin", "-ss", &format!("{a:.3}"), "-to", &format!("{b:.3}")]).arg("-i").arg(&path).args(["-an", "-vf", "select='gt(scene,0.4)',showinfo", "-f", "null", "-"]).output();
        let job = match out {
            Ok(o) => Job::Scenes(String::from_utf8_lossy(&o.stderr).lines().filter_map(|l| l.split("pts_time:").nth(1)?.split_whitespace().next()?.parse::<f64>().ok()).map(|t| t + a).collect()),
            Err(e) => Job::Failed(e.to_string()),
        };
        let _ = tx.send(job);
    });
}

fn remove_scenes(app: &mut App, how: &str) {
    let Some((a, b)) = range(app, how) else { return };
    let (bar1, _, _) = crate::projsettings::smpte(&app.proj);
    let start = app.hub.movie.start.unwrap_or(bar1);
    let gone: Vec<f64> = app.hub.movie.scenes.iter().copied().filter(|s| *s - start >= a - 1e-6 && *s - start <= b + 1e-6).collect();
    if gone.is_empty() {
        return;
    }
    app.checkpoint("Remove Scene Markers");
    let ticks: Vec<u32> = gone.iter().map(|s| tick_of(app, *s - start).round() as u32).collect();
    app.markers.retain(|m| !(m.1.starts_with("Scene ") && ticks.iter().any(|x| x.abs_diff(m.0) <= 1)));
    app.hub.movie.scenes.retain(|s| !gone.contains(s));
    app.dirty = true;
}

/// Scene markers stay at their SMPTE time when the tempo changes: they follow here.
fn follow_scenes(app: &mut App) {
    if app.hub.movie.scenes.is_empty() {
        return;
    }
    let (bar1, _, _) = crate::projsettings::smpte(&app.proj);
    let start = app.hub.movie.start.unwrap_or(bar1);
    let want: Vec<(u32, String)> = app.hub.movie.scenes.iter().enumerate().map(|(k, s)| (tick_of(app, *s - start).round() as u32, format!("Scene {}", k + 1))).collect();
    let p = app;
    let mut changed = false;
    for (tick, name) in want {
        match p.markers.iter_mut().find(|m| m.1 == name) {
            Some(m) if m.0 != tick => {
                m.0 = tick;
                changed = true;
            }
            _ => {}
        }
    }
    if changed {
        p.markers.sort_by_key(|m| m.0);
    }
}

pub fn frame(app: &mut App, ctx: &egui::Context) {
    // the decoder follows the playhead while the window shows
    let pos = app.position();
    let playing = app.playing();
    if app.hub.movie.window {
        if let Some(t) = movie_time(app, pos) {
            let s = &mut app.hub.movie;
            if (t - s.shown_t).abs() > 1e-4 {
                s.shown_t = t;
                if let Some(d) = &s.dec {
                    let _ = d.tx.send((t, playing));
                }
            }
            if playing {
                ctx.request_repaint_after(Duration::from_millis(15));
            }
        }
        let frame = app.hub.movie.dec.as_ref().and_then(|d| d.latest.lock().ok().and_then(|mut f| f.take()));
        if let Some(f) = frame {
            let img = egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.rgba);
            match &mut app.hub.movie.tex {
                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                None => app.hub.movie.tex = Some(ctx.load_texture("movie-frame", img, egui::TextureOptions::LINEAR)),
            }
            let _ = f.t;
        }
    }
    // thumbnails in
    if let Some(rx) = &app.hub.movie.thumb_rx {
        let got: Vec<(i64, Frame)> = rx.try_iter().collect();
        for (k, f) in got {
            let img = egui::ColorImage::from_rgba_unmultiplied([f.w, f.h], &f.rgba);
            let tex = ctx.load_texture(format!("movie-thumb-{k}"), img, egui::TextureOptions::LINEAR);
            app.hub.movie.thumbs.insert(k, Some(tex));
            ctx.request_repaint();
        }
    }
    // background jobs done
    let done = app.hub.movie.job.as_ref().and_then(|rx| rx.try_recv().ok());
    if let Some(job) = done {
        app.hub.movie.job = None;
        app.hub.movie.busy = None;
        match job {
            Job::Failed(e) => app.tell(e),
            Job::Exported(p) => app.tell(format!("Exported {}", p.file_name().unwrap_or_default().to_string_lossy())),
            Job::Imported(files) => {
                let at = tick_of(app, 0.0).max(0.0).round() as u32;
                let separate = files.len() > 1;
                for f in files {
                    if separate {
                        app.selected_track = None;
                    }
                    app.import_file(&f);
                    // (at the movie's start)
                    if let Some(t) = app.selected_track {
                        if let Some(r) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|tr| tr.regions.last_mut()) {
                            r.start = at;
                        }
                    }
                    app.after_edit();
                }
            }
            Job::Scenes(cuts) => {
                let (bar1, _, _) = crate::projsettings::smpte(&app.proj);
                let start = app.hub.movie.start.unwrap_or(bar1);
                app.checkpoint("Create Scene Markers");
                for c in cuts {
                    let abs = start + c;
                    if !app.hub.movie.scenes.iter().any(|x| (x - abs).abs() < 0.02) {
                        app.hub.movie.scenes.push(abs);
                    }
                }
                app.hub.movie.scenes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                // the markers, named in order
                app.markers.retain(|m| !m.1.starts_with("Scene "));
                let n = app.hub.movie.scenes.len();
                let ticks: Vec<(u32, f64)> = app.hub.movie.scenes.clone().iter().map(|s| (tick_of(app, s - start).round() as u32, s - start)).collect();
                for (k, (t, secs)) in ticks.into_iter().enumerate() {
                    // scene markers are SMPTE-locked (and can't be unlocked)
                    let mut m = lg_core::markers::Marker::new(t, format!("Scene {}", k + 1));
                    m.2.lock = Some(secs);
                    app.markers.push(m);
                }
                app.markers.sort_by_key(|m| m.0);
                app.dirty = true;
                app.tell(format!("{n} scene marker{}", if n == 1 { "" } else { "s" }));
            }
        }
    }
    follow_scenes(app);
}

/// The Movie window: the picture at the playhead, a position slider, size options.
pub fn window(app: &mut App, ctx: &egui::Context) {
    dialogs(app, ctx);
    if !app.hub.movie.window || app.hub.movie.info.is_none() {
        return;
    }
    let info = app.hub.movie.info.clone().unwrap();
    let name = app.hub.movie.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut open = true;
    let s = &app.hub.movie;
    let screen = ctx.content_rect();
    let base = vec2(info.w as f32, info.h as f32) * s.scale / ctx.pixels_per_point();
    let mut w = egui::Window::new(name).id(egui::Id::unique("movie-window")).open(&mut open).collapsible(false).resizable(true);
    if s.full {
        w = w.fixed_rect(screen);
    } else {
        w = w.default_size(base + vec2(0.0, 30.0));
        if s.center {
            w = w.anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0));
        }
    }
    w.show(ctx, |ui| {
        let avail = ui.available_size() - vec2(0.0, 28.0);
        let aspect = info.w as f32 / info.h.max(1) as f32;
        let size = if app.hub.movie.keep_aspect {
            let wv = avail.x.min(avail.y * aspect);
            vec2(wv, wv / aspect)
        } else {
            avail
        };
        let (r, resp) = ui.allocate_exact_size(vec2(avail.x.max(80.0), size.y.max(45.0)), Sense::click());
        let pic = Rect::from_center_size(r.center(), size);
        ui.painter().rect_filled(r, 0.0, Color32::BLACK);
        match (&app.hub.movie.tex, movie_time(app, app.position())) {
            (Some(t), Some(_)) => {
                ui.painter().image(t.id(), pic, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            _ => {
                ui.painter().text(r.center(), Align2::CENTER_CENTER, "Outside the movie", look::regular(12.0), Color32::from_gray(140));
            }
        }
        resp.context_menu(|ui| {
            let s = &mut app.hub.movie;
            for (l, f) in [("0.25 Size", 0.25), ("0.5 Size", 0.5), ("Original Size", 1.0), ("2x Size", 2.0), ("3x Size", 3.0)] {
                if ui.button(l).clicked() {
                    s.scale = f;
                    s.full = false;
                    ui.ctx().memory_mut(|m| m.reset_areas());
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(if s.full { "Exit Full Screen" } else { "Enter Full Screen" }).clicked() {
                s.full = !s.full;
                ui.close();
            }
            ui.checkbox(&mut s.keep_aspect, "Keep Aspect Ratio");
            ui.checkbox(&mut s.center, "Center");
        });
        // the position slider: the playhead chases it
        let t = movie_time(app, app.position()).unwrap_or(0.0);
        let mut v = t;
        let sl = ui.add(egui::Slider::new(&mut v, 0.0..=info.duration).show_value(false).trailing_fill(true));
        if sl.changed() {
            let tick = tick_of(app, v);
            app.locate(tick.max(0.0));
        }
    });
    if !open {
        app.hub.movie.window = false;
    }
}

/// The Movie track's height (0 when hidden).
pub fn track_height(app: &App) -> f32 {
    if app.hub.movie.track { 60.0 } else { 0.0 }
}

/// The global Movie track: thumbnails, the first left-aligned and the last right-aligned, each
/// one's left edge at its frame's place; how many depends on the zoom.
pub fn track_lane(app: &mut App, ui: &mut egui::Ui, row: Rect, lanes_x: f32) {
    let p = ui.painter_at(row);
    p.hline(row.x_range(), row.max.y, Stroke::new(1.0, look::line()));
    p.text(pos2(row.min.x + 14.0, row.min.y + 12.0), Align2::LEFT_CENTER, "Movie", look::semibold(12.0), look::text_dim());
    // the header's Movie menu
    let head = Rect::from_min_max(row.min, pos2(lanes_x, row.max.y));
    let hresp = ui.interact(head, ui.scope_id().with("movie-head"), Sense::click());
    let mut fired = None;
    hresp.context_menu(|ui| {
        for l in ["Open Movie…", "Remove Movie", "-", "Create Movie Scene Cut Markers (Auto Range)", "Import Audio from Movie", "Export Audio to Movie…"] {
            if l == "-" {
                ui.separator();
            } else if ui.button(if l.starts_with("Create Movie Scene") { "Create Marker Set from Scene Cuts" } else { l }).clicked() {
                fired = Some(l);
                ui.close();
            }
        }
    });
    let lane = Rect::from_min_max(pos2(lanes_x, row.min.y), row.max);
    let lresp = ui.interact(lane, ui.scope_id().with("movie-lane"), Sense::click());
    lresp.context_menu(|ui| {
        for l in ["Import Audio from Movie", "Export Audio to Movie…"] {
            if ui.button(l).clicked() {
                fired = Some(l);
                ui.close();
            }
        }
    });
    // the Pencil tool opens a movie
    if lresp.clicked() && app.tool == crate::edit::Tool::Pencil {
        fired = Some("Open Movie…");
    }
    if let Some(f) = fired {
        let ctx = ui.ctx().clone();
        command(app, &ctx, f);
    }
    let Some(info) = app.hub.movie.info.clone() else {
        let lp = ui.painter_at(lane);
        lp.text(pos2(lane.min.x + 8.0, lane.center().y), Align2::LEFT_CENTER, "No movie: Open Movie… (⌥⌘O)", look::regular(11.0), look::text_dim());
        return;
    };
    let lp = ui.painter_at(lane);
    let bar_t = app.bar_ticks() as f32;
    let (scroll, bw) = (app.scroll_x, app.bar_w);
    let x_of = |tick: f64| lanes_x + (tick as f32 / bar_t - scroll) * bw;
    let x0 = x_of(tick_of(app, 0.0));
    let x1 = x_of(tick_of(app, info.duration));
    let th = row.height() - 6.0;
    let tw = th * info.w as f32 / info.h.max(1) as f32;
    lp.rect_filled(Rect::from_min_max(pos2(x0, row.min.y + 3.0), pos2(x1, row.max.y - 3.0)), 2.0, Color32::from_gray(25));
    let n = (((x1 - x0) / tw).floor() as i64).max(1);
    let visible = |x: f32| x + tw >= lane.min.x && x <= lane.max.x;
    for k in 0..=n {
        // the last right-aligned
        let x = if k == n { x1 - tw } else { x0 + k as f32 * tw };
        if !visible(x) || (k == n && x < x0 + (n - 1) as f32 * tw + tw * 0.5) {
            continue;
        }
        // the frame at the thumbnail's left edge (the last: the movie's end)
        let tick = (x - lanes_x) / bw * bar_t + scroll * bar_t;
        let t = if k == n { (info.duration - 1.0 / info.fps).max(0.0) } else { movie_time(app, tick as f64).unwrap_or(0.0) };
        let key = (t * 4.0).round() as i64;
        let tr = Rect::from_min_size(pos2(x, row.min.y + 3.0), vec2(tw, th));
        match app.hub.movie.thumbs.get(&key) {
            Some(Some(tex)) => {
                lp.image(tex.id(), tr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            Some(None) => {
                lp.rect_filled(tr.shrink(1.0), 0.0, Color32::from_gray(45));
            }
            None => {
                app.hub.movie.thumbs.insert(key, None);
                if let Some(tx) = &app.hub.movie.thumb_tx {
                    let _ = tx.send((key, key as f64 / 4.0));
                }
            }
        }
        lp.vline(tr.min.x, tr.y_range(), Stroke::new(1.0, Color32::from_gray(10)));
    }
}

fn dialogs(app: &mut App, ctx: &egui::Context) {
    if let Some(b) = &app.hub.movie.busy {
        egui::Area::new(egui::Id::unique("movie-busy")).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -40.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(b);
                });
            });
        });
        ctx.request_repaint_after(Duration::from_millis(100));
    }
    let Some(d) = app.hub.movie.dlg.clone() else { return };
    let mut close = false;
    egui::Window::new("Movie").id(egui::Id::unique("movie-dlg")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, -60.0)).show(ctx, |ui| match d {
        Dlg::Remove => {
            ui.label("Remove the movie's scene markers too?");
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                if ui.button("Keep").clicked() {
                    remove(app, false);
                    close = true;
                }
                if ui.button("Remove").clicked() {
                    remove(app, true);
                    close = true;
                }
            });
        }
        Dlg::Import(mut separate) => {
            ui.label("Import Audio from Movie");
            let streams = app.hub.movie.info.as_ref().map(|i| i.audio.clone()).unwrap_or_default();
            for s in &streams {
                ui.label(format!("• {s}"));
            }
            ui.add_enabled(streams.len() > 1, egui::Checkbox::new(&mut separate, "On separate tracks"));
            app.hub.movie.dlg = Some(Dlg::Import(separate));
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                if ui.button("Import").clicked() {
                    import_audio(app, separate, streams.len());
                    close = true;
                }
            });
        }
        Dlg::Export { mut aac, mut rate, mut bits, mut keep } => {
            ui.label(if app.cycle_on() { "Export Audio to Movie: the cycle area's audio and movie" } else { "Export Audio to Movie: from the movie's start to its end" });
            egui::Grid::new("movie-export").num_columns(2).show(ui, |ui| {
                ui.label("Audio Format:");
                egui::ComboBox::from_id_salt("mx-fmt").selected_text(if aac { "AAC" } else { "PCM" }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut aac, true, "AAC");
                    ui.selectable_value(&mut aac, false, "PCM");
                });
                ui.end_row();
                ui.label("Sample Rate:");
                egui::ComboBox::from_id_salt("mx-rate").selected_text(format!("{} kHz", rate as f32 / 1000.0)).show_ui(ui, |ui| {
                    for r in [44100, 48000, 88200, 96000] {
                        ui.selectable_value(&mut rate, r, format!("{} kHz", r as f32 / 1000.0));
                    }
                });
                ui.end_row();
                ui.label("Bit Depth:");
                ui.add_enabled_ui(!aac, |ui| {
                    egui::ComboBox::from_id_salt("mx-bits").selected_text(format!("{bits}-bit")).show_ui(ui, |ui| {
                        for b in [16, 24] {
                            ui.selectable_value(&mut bits, b, format!("{b}-bit"));
                        }
                    });
                });
                ui.end_row();
            });
            let streams = app.hub.movie.info.as_ref().map(|i| i.audio.clone()).unwrap_or_default();
            if !streams.is_empty() {
                ui.label("Keep the movie's own audio tracks (Enable):");
                for (k, s) in streams.iter().enumerate() {
                    if let Some(x) = keep.get_mut(k) {
                        ui.checkbox(x, s);
                    }
                }
            }
            app.hub.movie.dlg = Some(Dlg::Export { aac, rate, bits, keep: keep.clone() });
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                if ui.button("Save…").clicked() {
                    export_audio(app, aac, rate, bits, &keep);
                    close = true;
                }
            });
        }
    });
    if close {
        app.hub.movie.dlg = None;
    }
}

/// Import Audio from Movie: each audio stream bounced to a WAV named after the movie (next to
/// the project's audio, else the movie), then placed at the movie's start.
fn import_audio(app: &mut App, separate: bool, n: usize) {
    let Some(path) = app.hub.movie.path.clone() else { return };
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let dir = crate::projsettings::recording_folder(&app.proj).or_else(|| app.project.as_ref().and_then(|p| p.audio_dir.clone())).or_else(|| path.parent().map(|p| p.to_path_buf())).unwrap_or_else(std::env::temp_dir);
    let (tx, rx) = mpsc::channel();
    app.hub.movie.job = Some(rx);
    app.hub.movie.busy = Some("Importing the movie's audio…".into());
    std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        let mut files = Vec::new();
        let streams: Vec<Option<usize>> = if separate { (0..n).map(Some).collect() } else { vec![None] };
        for s in streams {
            let name = match s {
                Some(k) if n > 1 => format!("{stem} {}.wav", k + 1),
                _ => format!("{stem}.wav"),
            };
            let out = dir.join(name);
            let mut c = Command::new("ffmpeg");
            c.args(["-v", "error", "-nostdin", "-y"]).arg("-i").arg(&path).arg("-vn");
            match s {
                Some(k) => {
                    c.args(["-map", &format!("0:a:{k}")]);
                }
                None if n > 1 => {
                    c.args(["-filter_complex", &format!("amix=inputs={n}:normalize=0")]);
                }
                None => {}
            }
            c.args(["-ac", "2", "-ar", &crate::audio::RATE.to_string(), "-c:a", "pcm_s24le"]).arg(&out);
            match c.status() {
                Ok(st) if st.success() => files.push(out),
                _ => {
                    let _ = tx.send(Job::Failed("Couldn't read the movie's audio".into()));
                    return;
                }
            }
        }
        let _ = tx.send(Job::Imported(files));
    });
}

/// Export Audio to Movie: the project's mix (the cycle area, or the movie's length) bounced and
/// written into a copy of the movie as its soundtrack (the picture copied as it is).
fn export_audio(app: &mut App, aac: bool, rate: u32, bits: u32, keep: &[bool]) {
    let (Some(path), Some(info)) = (app.hub.movie.path.clone(), app.hub.movie.info.clone()) else { return };
    let Some(p) = &app.project else { return };
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "mov".into());
    let Some(out) = (if app.automated { None } else { kn_ui::files::FileDialog::new().set_title("Export Audio to Movie").set_file_name(format!("{stem} (with project audio).{ext}")).save_file() }) else { return };
    let (t0, t1) = if app.cycle_on() { (movie_time(app, app.cycle.0 as f64).unwrap_or(0.0), movie_time(app, app.cycle.1 as f64).unwrap_or(info.duration)) } else { (0.0, info.duration) };
    let (from, to) = (tick_of(app, t0).max(0.0) as u32, tick_of(app, t1).max(0.0) as u32);
    let tracks = app.engine.lock().map(|e| e.tracks.clone()).unwrap_or_default();
    let sf = app.engine.lock().ok().and_then(|e| e.soundfont.clone());
    let (tempo, map) = (p.tempo, p.tempo_map.clone());
    let keep: Vec<usize> = keep.iter().enumerate().filter(|x| *x.1).map(|x| x.0).collect();
    let (tx, rx) = mpsc::channel();
    app.hub.movie.job = Some(rx);
    app.hub.movie.busy = Some("Exporting audio to the movie…".into());
    std::thread::spawn(move || {
        let samples = crate::audio::bounce(tracks, tempo, map, sf.as_deref(), from, to);
        let wav = std::env::temp_dir().join(format!("bramble-movie-mix-{}.wav", std::process::id()));
        if std::fs::write(&wav, crate::audio::wav_bytes(&samples)).is_err() {
            let _ = tx.send(Job::Failed("Couldn't write the mix".into()));
            return;
        }
        let mut c = Command::new("ffmpeg");
        c.args(["-v", "error", "-nostdin", "-y", "-ss", &format!("{t0:.3}"), "-to", &format!("{t1:.3}")]).arg("-i").arg(&path).arg("-i").arg(&wav).args(["-map", "0:v", "-map", "1:a"]);
        for k in &keep {
            c.args(["-map", &format!("0:a:{k}")]);
        }
        c.args(["-c:v", "copy"]);
        if aac {
            c.args(["-c:a", "aac", "-b:a", "256k"]);
        } else {
            c.args(["-c:a", if bits == 16 { "pcm_s16le" } else { "pcm_s24le" }]);
        }
        c.args(["-ar", &rate.to_string(), "-shortest"]).arg(&out);
        let job = match c.status() {
            Ok(s) if s.success() => Job::Exported(out),
            _ => Job::Failed("Couldn't write the movie".into()),
        };
        let _ = std::fs::remove_file(&wav);
        let _ = tx.send(job);
    });
}

/// File ▸ Movie and Navigate ▸ Other menus' submenus whose items share names (Create / Remove
/// Movie Scene Cut Markers ▸ Auto Range, Cycle Area…). True when drawn here.
pub fn menu_ui(app: &mut App, ui: &mut egui::Ui, label: &str) -> bool {
    let create = label == "Create Movie Scene Cut Markers";
    if !create && label != "Remove Movie Scene Cut Markers" {
        return false;
    }
    let on = app.hub.movie.info.is_some();
    ui.add_enabled_ui(on, |ui| {
        ui.menu_button(label, |ui| {
            for item in ["Auto Range", "-", "Marquee Selection", "Cycle Area", "Selected Regions", "-", "Entire Movie"] {
                if item == "-" {
                    ui.separator();
                    continue;
                }
                let enabled = match item {
                    "Marquee Selection" => false,
                    "Cycle Area" => app.cycle_on(),
                    "Selected Regions" => app.selected_region.is_some(),
                    _ => true,
                };
                if ui.add_enabled(enabled, egui::Button::new(item)).clicked() {
                    if create {
                        scenes(app, item);
                    } else {
                        remove_scenes(app, item);
                    }
                    ui.close();
                }
            }
            if !create {
                ui.separator();
                if ui.button("All Scene Markers").clicked() {
                    remove_scenes(app, "Entire Movie");
                    ui.close();
                }
            }
        });
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_text() {
        let t = "[STREAM]\ncodec_name=h264\ncodec_type=video\nwidth=1920\nheight=1080\nr_frame_rate=25/1\nTAG:timecode=01:00:00:00\n[/STREAM]\n[STREAM]\ncodec_name=aac\ncodec_type=audio\nr_frame_rate=0/0\n[/STREAM]\n[FORMAT]\nduration=12.5\n[/FORMAT]\n";
        let i = parse_probe(t).unwrap();
        assert_eq!((i.w, i.h), (1920, 1080));
        assert_eq!(i.fps, 25.0);
        assert_eq!(i.duration, 12.5);
        assert_eq!(i.timecode, Some(3600.0));
        assert_eq!(i.audio.len(), 1);
        assert_eq!(decode_size(&i), (960, 540));
    }
}
