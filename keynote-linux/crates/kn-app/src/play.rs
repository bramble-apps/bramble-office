//! Slideshow (Play) mode: full-screen, builds stepped in Build Order, transitions between slides.
//!
//! Each "state" of a slide is the number of build steps applied (see [`builds::state_at`]). A click
//! plays a *group* of steps (the clicked one plus the automatic ones after it) with per-object
//! animations; the layers and textures for the next group / transition are rendered while the
//! show is idle so animations only draw GPU meshes. Movies play through `ffmpeg`/`ffplay`
//! ([`media`]).

use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The show's clock: real time, or while exporting a movie a frame clock that only moves on
/// when a frame has been captured (so builds and transitions come out smooth at any speed).
static FRAME_CLOCK: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

fn now() -> Instant {
    FRAME_CLOCK.lock().ok().and_then(|c| *c).unwrap_or_else(Instant::now)
}

fn since(t: Instant) -> Duration {
    now().saturating_duration_since(t)
}

use egui::{Color32, Key, Rect, TextureHandle};
use kn_format::ObjId;
use kn_model::{DrawableKind, SlideScene};

use crate::app::App;
use crate::builds::{self, Group, Kind, Map};
use crate::media;
use crate::transitions::{self, ObjSet};

struct TransAnim {
    from: TextureHandle,
    to: TextureHandle,
    objs: Option<(Rc<ObjSet>, Rc<ObjSet>, Vec<(usize, usize)>)>,
    effect: String,
    dir: u32,
    color: Color32,
    duration: f32,
    /// Both slides have this dynamic background: it keeps playing under the transition (the
    /// slides' textures are then made without background).
    live: Option<kn_model::motionbg::MotionBg>,
    /// Otherwise the incoming slide's dynamic background, playing under it (moving with it)…
    to_live: Option<kn_model::motionbg::MotionBg>,
    /// …and the outgoing one's, frozen at its last frame.
    from_bg: Option<TextureHandle>,
}

/// Two slides show the same dynamic background (Keynote keeps it running across them).
fn same_motion(a: &kn_model::motionbg::MotionBg, b: &kn_model::motionbg::MotionBg) -> bool {
    a.looks_same(b)
}

enum Anim {
    Build { g: Rc<Group>, fired: Vec<bool> },
    Trans(TransAnim),
}

/// Textures and layers rendered ahead of time, for the current output width.
#[derive(Default)]
struct Cache {
    width: u32,
    states: HashMap<(usize, usize), TextureHandle>,
    groups: HashMap<(usize, usize), Rc<Group>>,
    objs: HashMap<(usize, usize), Rc<ObjSet>>,
    order: Vec<(u8, usize, usize)>,
}

impl Cache {
    fn touch(&mut self, kind: u8, i: usize, s: usize) {
        self.order.retain(|k| *k != (kind, i, s));
        self.order.push((kind, i, s));
        // keep memory bounded: drop the least recently used entries
        // (a slide with several builds plus the next one fit)
        while self.order.len() > 24 {
            let (k, i, s) = self.order.remove(0);
            match k {
                0 => {
                    self.states.remove(&(i, s));
                }
                1 => {
                    self.groups.remove(&(i, s));
                }
                _ => {
                    self.objs.remove(&(i, s));
                }
            }
        }
    }
}

pub struct Play {
    pub index: usize,
    pub step: usize,
    anim: Option<(Anim, Instant)>,
    shown_at: Instant,
    started_fullscreen: bool,
    /// Play > In Window: the show stays in the app window.
    in_window: bool,
    /// Play > Rehearse Slideshow: the presenter display only, timer from zero.
    rehearse: bool,
    show_hud: Option<Instant>,
    cache: Cache,
    players: Vec<media::Player>,
    /// Movie / web video controls on screen last frame (their clicks don't advance the show),
    /// and when the pointer last moved (they show for a while after).
    video_ctl: Vec<Rect>,
    pointer_moved: Instant,
    /// Web videos told to start whose stream was still being looked up.
    pending_web: std::collections::HashSet<ObjId>,
    /// Movies to start once the current transition has finished.
    autoplay_pending: bool,
    /// The slide shown before the current one (links to "Last slide viewed").
    last_viewed: usize,
    /// KN_CLICKS: simulated clicks (seconds after the show starts), for automated checks.
    clicks: Vec<f32>,
    started: Instant,
    /// KN_ANIM_AT: freeze the last animation at this time (automated checks).
    anim_cap: Option<f32>,
    /// Frames shown so far (the window takes a few frames to become full screen).
    frames: u32,
    presenter: Presenter,
    /// Input gathered in the second window last frame.
    child_cmd: Cmd,
    /// Data shared with the presenter window.
    shared: std::sync::Arc<std::sync::Mutex<WindowShared>>,
    /// KN_DEBUG: slowest frame of the running animation.
    worst_frame: (Option<Instant>, f32),
    /// Scenes of the slides in use (App::scene clones a scene on every call).
    scenes: HashMap<usize, Rc<SlideScene>>,
    /// File > Export To > Movie / GIF with builds and transitions: the show is being recorded.
    pub recording: Option<Recording>,
    /// Play > Record Slideshow: narration and navigation being recorded.
    pub narration: Option<Narration>,
    /// Play > Play Recorded Slideshow: the recording driving the show.
    pub replay: Option<Replay>,
    /// Background preparation of slide states and builds (started on the first frame).
    prep: Option<PrepWorker>,
}

/// What the preparation worker makes: slide states (with or without background) and build groups.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum PrepJob {
    State(usize, usize, bool),
    Group(usize, usize),
}

enum PrepDone {
    State(usize, usize, bool, TextureHandle),
    Group(usize, usize, Group),
}

/// Renders the show's slide states and build layers on a background thread (own renderer, a copy
/// of the document), so the slideshow never stops to draw: textures can be made off the UI
/// thread. Jobs are done newest-first in priority order; anything still missing when needed is
/// drawn on the spot as before.
struct PrepWorker {
    jobs: std::sync::Arc<(std::sync::Mutex<Vec<PrepJob>>, std::sync::Condvar)>,
    done: std::sync::Arc<std::sync::Mutex<Vec<PrepDone>>>,
    asked: std::collections::HashSet<PrepJob>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl PrepWorker {
    fn start(ctx: &egui::Context, file: kn_format::KeyFile, slides: Vec<kn_model::SlideInfo>, width: u32) -> PrepWorker {
        use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Condvar, Mutex};
        let jobs: Arc<(Mutex<Vec<PrepJob>>, Condvar)> = Arc::new((Mutex::new(Vec::new()), Condvar::new()));
        let done: Arc<Mutex<Vec<PrepDone>>> = Default::default();
        let stop = Arc::new(AtomicBool::new(false));
        let (j2, d2, s2, ctx) = (jobs.clone(), done.clone(), stop.clone(), ctx.clone());
        std::thread::spawn(move || {
            let doc = kn_model::Document::from_file(file);
            let mut r = kn_render::Renderer::new();
            let mut scenes: HashMap<usize, SlideScene> = HashMap::new();
            loop {
                let job = {
                    let (m, cv) = &*j2;
                    let mut g = m.lock().unwrap();
                    while g.is_empty() && !s2.load(Ordering::Relaxed) {
                        g = cv.wait_timeout(g, Duration::from_millis(200)).unwrap().0;
                    }
                    if s2.load(Ordering::Relaxed) {
                        return;
                    }
                    g.remove(0)
                };
                let (PrepJob::State(i, _, _) | PrepJob::Group(i, _)) = job;
                if !scenes.contains_key(&i) {
                    if let Some(sc) = doc.scene(&slides, i) {
                        scenes.insert(i, sc);
                    }
                }
                let Some(sc) = scenes.get(&i) else { continue };
                let t0 = Instant::now();
                let out = crate::textures::guarded("prepare", || match job {
                    PrepJob::State(i, s, bg) => {
                        let st = builds::state_at(sc, s);
                        let key = if bg { s } else { s | (1 << 40) };
                        PrepDone::State(i, s, bg, builds::state_texture(&ctx, &mut r, &doc, sc, &st, width, &|_| true, bg, &format!("play-{i}-{key}")))
                    }
                    PrepJob::Group(i, s) => PrepDone::Group(i, s, Group::prepare(&ctx, &mut r, &doc, sc, s, width)),
                });
                if std::env::var_os("KN_DEBUG").is_some() {
                    eprintln!("worker {job:?}: {:?}", t0.elapsed());
                }
                if let Some(o) = out {
                    d2.lock().unwrap().push(o);
                    ctx.request_repaint();
                }
            }
        });
        PrepWorker { jobs, done, asked: Default::default(), stop }
    }

    /// Ask for a job (once).
    fn ask(&mut self, job: PrepJob) {
        if self.asked.insert(job) {
            let (m, cv) = &*self.jobs;
            m.lock().unwrap().push(job);
            cv.notify_one();
        }
    }
}

impl Drop for PrepWorker {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.jobs.1.notify_all();
    }
}

/// Play > Record Slideshow in progress.
pub struct Narration {
    mic: Option<std::process::Child>,
    wav: std::path::PathBuf,
    started: Instant,
    events: Vec<kn_model::recording::NavEvent>,
    last: Option<(usize, usize)>,
    anim_since: Option<Instant>,
    was_anim: bool,
    pending_end: Option<(usize, usize)>,
}

/// Play > Play Recorded Slideshow in progress.
pub struct Replay {
    nav: Vec<kn_model::recording::NavEvent>,
    next: usize,
    duration: f64,
    started: Instant,
    _audio: Option<media::Player>,
}

/// Recording the show into numbered PNG frames on the frame clock.
pub struct Recording {
    pub dir: std::path::PathBuf,
    pub out: std::path::PathBuf,
    pub format: crate::export::Format,
    pub fps: f32,
    /// Seconds each slide / build stays up before the next click.
    pub hold: f32,
    /// Output width in pixels.
    pub width: u32,
    pub frame: u32,
    waiting: bool,
    /// When dropping blank frames (the first slide loading) began, in real time.
    skip_since: Option<Instant>,
    shot: std::sync::Arc<std::sync::Mutex<Option<std::sync::Arc<egui::ColorImage>>>>,
    last_busy: Instant,
}

impl Recording {
    pub fn new(dir: std::path::PathBuf, out: std::path::PathBuf, format: crate::export::Format, fps: f32, hold: f32, width: u32) -> Recording {
        if let Ok(mut c) = FRAME_CLOCK.lock() {
            *c = Some(Instant::now());
        }
        Recording { dir, out, format, fps, hold, width, frame: 0, waiting: false, skip_since: None, shot: Default::default(), last_busy: now() }
    }
}

fn stop_frame_clock() {
    if let Ok(mut c) = FRAME_CLOCK.lock() {
        *c = None;
    }
}

/// After drawing a recorded frame: ask for its screenshot; when it arrives, keep the slide part
/// as the next frame and move the clock on by one frame.
fn record_frame(rec: &mut Recording, ctx: &egui::Context, rect: Rect) {
    let got = rec.shot.lock().ok().and_then(|mut s| s.take());
    if let Some(img) = got {
        let ppp = ctx.pixels_per_point();
        let [w, h] = img.size;
        let (x0, y0) = (((rect.min.x * ppp).round() as usize).min(w), ((rect.min.y * ppp).round() as usize).min(h));
        let (x1, y1) = (((rect.max.x * ppp).round() as usize).min(w), ((rect.max.y * ppp).round() as usize).min(h));
        let (cw, ch) = (x1.saturating_sub(x0) as u32, y1.saturating_sub(y0) as u32);
        let mut out = image::RgbaImage::new(cw.max(1), ch.max(1));
        for y in 0..ch {
            for x in 0..cw {
                let c = img.pixels[(y0 + y as usize) * w + x0 + x as usize];
                out.put_pixel(x, y, image::Rgba(c.to_array()));
            }
        }
        // the first slide may still be loading: leading blank frames are dropped (the clock waits)
        // (up to 8 s of real time; a slide that really is blank then starts anyway)
        let started = *rec.skip_since.get_or_insert_with(Instant::now);
        let blank = rec.frame == 0 && started.elapsed().as_secs_f32() < 8.0 && out.pixels().all(|p| p == out.get_pixel(0, 0));
        if blank {
            rec.waiting = false;
            ctx.request_repaint();
            return;
        }
        let out = if cw != rec.width && cw > 0 {
            let nh = (ch as f32 * rec.width as f32 / cw as f32).round() as u32 / 2 * 2;
            image::imageops::resize(&out, rec.width, nh.max(2), image::imageops::FilterType::Triangle)
        } else {
            out
        };
        let _ = out.save(rec.dir.join(format!("f{:05}.png", rec.frame)));
        rec.frame += 1;
        if let Ok(mut c) = FRAME_CLOCK.lock() {
            if let Some(t) = c.as_mut() {
                *t += Duration::from_secs_f32(1.0 / rec.fps);
            }
        }
        rec.waiting = false;
    }
    if !rec.waiting {
        rec.waiting = true;
        let slot = rec.shot.clone();
        let cb = egui::ScreenshotCallback::new(move |img: std::sync::Arc<egui::ColorImage>| {
            if let Ok(mut s) = slot.lock() {
                *s = Some(img);
            }
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
    }
    ctx.request_repaint();
}

impl Play {
    pub fn new(index: usize, in_window: bool, rehearse: bool, presenter_window: bool) -> Play {
        let clicks = std::env::var("KN_CLICKS")
            .ok()
            .map(|s| s.split(',').filter_map(|x| x.trim().parse::<f32>().ok()).collect())
            .unwrap_or_default();
        Play {
            index,
            step: 0,
            anim: None,
            shown_at: now(),
            started_fullscreen: false,
            in_window,
            rehearse,
            show_hud: None,
            cache: Cache::default(),
            players: Vec::new(),
            video_ctl: Vec::new(),
            pointer_moved: now(),
            pending_web: Default::default(),
            autoplay_pending: true,
            clicks,
            started: now(),
            anim_cap: None,
            frames: 0,
            presenter: Presenter::new(rehearse, presenter_window),
            child_cmd: Cmd::default(),
            shared: Default::default(),
            scenes: HashMap::new(),
            worst_frame: (None, 0.0),
            last_viewed: index,
            recording: None,
            narration: None,
            replay: None,
            prep: None,
        }
    }

    /// Seconds into the running animation.
    fn anim_time(&self, start: Instant) -> f32 {
        let t = since(start).as_secs_f32();
        self.anim_cap.map(|c| t.min(c)).unwrap_or(t)
    }

    fn begin(&mut self, a: Anim) {
        self.anim_cap = if self.clicks.is_empty() { std::env::var("KN_ANIM_AT").ok().and_then(|s| s.parse().ok()) } else { None };
        self.anim = Some((a, now()));
    }
}

fn steps(scene: &SlideScene) -> usize {
    scene.build_order.len()
}

fn next_visible(app: &App, from: usize) -> Option<usize> {
    (from + 1..app.slides.len()).find(|i| !app.slides[*i].skipped)
}

fn prev_visible(app: &App, from: usize) -> Option<usize> {
    (0..from).rev().find(|i| !app.slides[*i].skipped)
}

// ------------------------------------------------------------------ prepared assets

fn scene_rc(app: &mut App, play: &mut Play, index: usize) -> Option<Rc<SlideScene>> {
    if let Some(s) = play.scenes.get(&index) {
        return Some(s.clone());
    }
    let s = Rc::new(app.scene(index)?);
    if play.scenes.len() > 8 {
        play.scenes.clear();
    }
    play.scenes.insert(index, s.clone());
    Some(s)
}

/// A slide state as the show draws it: with its background, except a dynamic one (that plays
/// live underneath).
fn state_tex(app: &mut App, play: &mut Play, ctx: &egui::Context, index: usize, step: usize) -> Option<TextureHandle> {
    let live = scene_rc(app, play, index).is_some_and(|s| s.motion.is_some());
    state_tex_bg(app, play, ctx, index, step, !live)
}

/// `with_bg` false: without the slide background (a dynamic background is drawn live under it).
fn state_tex_bg(app: &mut App, play: &mut Play, ctx: &egui::Context, index: usize, step: usize, with_bg: bool) -> Option<TextureHandle> {
    let key = if with_bg { step } else { step | (1 << 40) };
    if let Some(t) = play.cache.states.get(&(index, key)) {
        let t = t.clone();
        play.cache.touch(0, index, key);
        return Some(t);
    }
    let sc = scene_rc(app, play, index)?;
    let st = builds::state_at(&sc, step);
    let doc = app.doc.as_ref()?;
    let t0 = std::time::Instant::now();
    let t = builds::state_texture(ctx, &mut app.renderer, doc, &sc, &st, play.cache.width, &|_| true, with_bg, &format!("play-{index}-{key}"));
    if std::env::var_os("KN_DEBUG").is_some() {
        eprintln!("state texture {index}/{step}: {:?}", t0.elapsed());
    }
    play.cache.states.insert((index, key), t.clone());
    play.cache.touch(0, index, key);
    Some(t)
}

/// The objects above z-index `z` at a build step, without the background (drawn over a playing
/// movie).
fn state_tex_above(app: &mut App, play: &mut Play, ctx: &egui::Context, index: usize, step: usize, z: usize) -> Option<TextureHandle> {
    let key = step | (1 << 41) | (z << 42);
    if let Some(t) = play.cache.states.get(&(index, key)) {
        let t = t.clone();
        play.cache.touch(0, index, key);
        return Some(t);
    }
    let sc = scene_rc(app, play, index)?;
    let above: std::collections::HashSet<ObjId> = sc.drawables.iter().skip(z + 1).map(|d| d.id).collect();
    let st = builds::state_at(&sc, step);
    let doc = app.doc.as_ref()?;
    let t0 = std::time::Instant::now();
    let t = builds::state_texture(ctx, &mut app.renderer, doc, &sc, &st, play.cache.width, &|d| above.contains(&d.id), false, &format!("play-{index}-{key}"));
    if std::env::var_os("KN_DEBUG").is_some() {
        eprintln!("above texture {index}/{step}: {:?}", t0.elapsed());
    }
    play.cache.states.insert((index, key), t.clone());
    play.cache.touch(0, index, key);
    Some(t)
}

fn group(app: &mut App, play: &mut Play, ctx: &egui::Context, index: usize, step: usize) -> Option<Rc<Group>> {
    if let Some(g) = play.cache.groups.get(&(index, step)) {
        let g = g.clone();
        play.cache.touch(1, index, step);
        return Some(g);
    }
    let sc = scene_rc(app, play, index)?;
    let doc = app.doc.as_ref()?;
    let t0 = std::time::Instant::now();
    let g = Rc::new(Group::prepare(ctx, &mut app.renderer, doc, &sc, step, play.cache.width));
    if std::env::var_os("KN_DEBUG").is_some() {
        eprintln!("prepare group {index}/{step}: {:?}", t0.elapsed());
    }
    play.cache.groups.insert((index, step), g.clone());
    play.cache.touch(1, index, step);
    Some(g)
}

fn objset(app: &mut App, play: &mut Play, ctx: &egui::Context, index: usize, step: usize) -> Option<Rc<ObjSet>> {
    if let Some(o) = play.cache.objs.get(&(index, step)) {
        let o = o.clone();
        play.cache.touch(2, index, step);
        return Some(o);
    }
    let sc = scene_rc(app, play, index)?;
    let st = builds::state_at(&sc, step);
    let doc = app.doc.as_ref()?;
    let o = Rc::new(ObjSet::prepare(ctx, &mut app.renderer, doc, &sc, &st, play.cache.width));
    play.cache.objs.insert((index, step), o.clone());
    play.cache.touch(2, index, step);
    Some(o)
}

/// Render one missing asset that the next click will need. Returns true if it did some work.
/// The next missing asset of slide `i` from step `s` on (its states and build groups); returns
/// whether one was made (one per call: each can take a few hundred ms).
fn prepare_steps(app: &mut App, play: &mut Play, ctx: &egui::Context, i: usize, s: usize, n_steps: usize) -> bool {
    // with the background worker: ask for what's missing (a group tells where the next step is,
    // so the walk stops at the first group not made yet)
    if play.prep.is_some() {
        let bg = !scene_rc(app, play, i).is_some_and(|sc| sc.motion.is_some());
        let mut k = s;
        loop {
            let key = if bg { k } else { k | (1 << 40) };
            if !play.cache.states.contains_key(&(i, key)) {
                play.prep.as_mut().unwrap().ask(PrepJob::State(i, k, bg));
            }
            if k >= n_steps {
                return false;
            }
            match play.cache.groups.get(&(i, k)).map(|g| g.to) {
                Some(to) => k = to.max(k + 1),
                None => {
                    play.prep.as_mut().unwrap().ask(PrepJob::Group(i, k));
                    return false;
                }
            }
        }
    }
    let mut k = s;
    loop {
        if !play.cache.states.contains_key(&(i, k)) {
            return state_tex(app, play, ctx, i, k).is_some();
        }
        if k >= n_steps {
            return false;
        }
        if !play.cache.groups.contains_key(&(i, k)) {
            return group(app, play, ctx, i, k).is_some();
        }
        k = play.cache.groups.get(&(i, k)).map(|g| g.to).unwrap_or(k + 1).max(k + 1);
    }
}

fn prepare_next(app: &mut App, play: &mut Play, ctx: &egui::Context, scene: &SlideScene) -> bool {
    let (i, s) = (play.index, play.step);
    // everything this slide still needs, every step (so no build waits for rendering)
    if prepare_steps(app, play, ctx, i, s, steps(scene)) {
        return true;
    }
    // rendering on this thread stalls a playing movie: the next slide is prepared while none
    // plays (normally on the slide before a movie slide), else at the click
    // the next slide's dynamic background: its first frame rendered ahead
    if let Some(nb) = next_visible(app, i).and_then(|n| scene_rc(app, play, n)).and_then(|s| s.motion.clone()) {
        let t = since(app.motion_clock).as_secs_f32() + 0.5;
        app.motion_tex.prime(ctx, &nb, t);
    }
    // without the worker, rendering here would stall a playing movie: the next slide waits
    let has_movie = scene.drawables.iter().any(|d| matches!(d.kind, DrawableKind::Movie(_)));
    if play.prep.is_none() && (play.players.iter().any(|p| !p.finished()) || (has_movie && play.players.is_empty())) {
        return false;
    }
    let Some(n) = next_visible(app, i) else { return false };
    if play.prep.is_some() {
        // the next slide: all its states and builds, on the worker
        let n_steps = scene_rc(app, play, n).map(|sc| steps(&sc)).unwrap_or(0);
        prepare_steps(app, play, ctx, n, 0, n_steps);
    } else if !play.cache.states.contains_key(&(n, 0)) {
        return state_tex(app, play, ctx, n, 0).is_some();
    }
    if scene.transition.as_ref().is_some_and(|t| transitions::needs_objects(&t.effect)) {
        if !play.cache.objs.contains_key(&(i, s)) {
            return objset(app, play, ctx, i, s).is_some();
        }
        if !play.cache.objs.contains_key(&(n, 0)) {
            return objset(app, play, ctx, n, 0).is_some();
        }
    }
    if let Some(nsc) = scene_rc(app, play, n) {
        // copy the next slide's movies into the cache directory early
        if let Some(doc) = app.doc.as_ref() {
            for d in &nsc.drawables {
                if let DrawableKind::Movie(m) = &d.kind {
                    if let Some(id) = m.movie_data {
                        let _ = media::cache_file(doc, id);
                    }
                }
            }
        }
        // all of the next slide's steps
        let n_steps = steps(&nsc);
        if prepare_steps(app, play, ctx, n, 0, n_steps) {
            return true;
        }
    }
    false
}

// ------------------------------------------------------------------ movies

fn movie_px(map: &Map, ppp: f32, w: f32, h: f32) -> [u32; 2] {
    let (mut pw, mut ph) = (w * map.k * ppp, h * map.k * ppp);
    let cap = (1920.0 / pw.max(1.0)).min(1080.0 / ph.max(1.0)).min(1.0);
    pw *= cap;
    ph *= cap;
    [pw.round().max(2.0) as u32, ph.round().max(2.0) as u32]
}

fn start_movie(app: &App, play: &mut Play, ctx: &egui::Context, scene: &SlideScene, id: ObjId, map: &Map) {
    if play.players.iter().any(|p| p.drawable == id && !p.finished()) {
        return;
    }
    play.players.retain(|p| p.drawable != id);
    let Some(d) = scene.drawables.iter().find(|d| d.id == id) else { return };
    // a web video: its stream (once known) over its poster
    if let DrawableKind::Image(im) = &d.kind {
        let Some(w) = &im.web_video else { return };
        match media::web_stream(&w.url) {
            Some(stream) => {
                let px = movie_px(map, ctx.pixels_per_point(), d.geom.w, d.geom.h);
                let spec = media::Spec { path: std::path::PathBuf::from(stream), len: 0, start: 0.0, end: 0.0, volume: 1.0, looping: false, video: Some([(px[0] + 1) & !1, (px[1] + 1) & !1]) };
                let key = w.url.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)) | 1;
                play.players.push(media::Player::start(ctx, id, key, spec));
            }
            None => {
                // still being looked up: try again shortly
                play.pending_web.insert(id);
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }
        return;
    }
    let DrawableKind::Movie(m) = &d.kind else { return };
    if m.live {
        // Live Video: the camera, filling the object's frame
        let px = movie_px(map, ctx.pixels_per_point(), d.geom.w, d.geom.h);
        play.players.push(media::Player::camera(ctx, id, px[0], px[1]));
        return;
    }
    let (Some(data), Some(doc)) = (m.movie_data, app.doc.as_ref()) else { return };
    let Some((path, len)) = media::cache_file(doc, data) else { return };
    let px = movie_px(map, ctx.pixels_per_point(), d.geom.w, d.geom.h);
    let spec = media::Spec::new(path, len, m, Some(px));
    play.players.push(media::Player::start(ctx, id, data, spec));
}

/// Movies that start by themselves when the slide (state) appears: visible, and without a
/// "start movie" build of their own.
fn autoplay(app: &App, play: &mut Play, ctx: &egui::Context, scene: &SlideScene, map: &Map) {
    let st = builds::state_at(scene, play.step);
    let ids: Vec<ObjId> = scene
        .drawables
        .iter()
        .filter(|d| matches!(&d.kind, DrawableKind::Movie(m) if m.movie_data.is_some() || m.live))
        .filter(|d| !st.hidden.contains(&d.id))
        .filter(|d| !scene.builds.iter().any(|b| b.drawable == d.id && matches!(builds::kind_of(b), Kind::Media(true))))
        .map(|d| d.id)
        .collect();
    for id in ids {
        start_movie(app, play, ctx, scene, id, map);
    }
    // web videos: those without a "start" build play when the slide appears (Keynote:
    // "Load video on click" off); a started one whose stream wasn't known yet starts now
    let webs: Vec<ObjId> = scene
        .drawables
        .iter()
        .filter(|d| matches!(&d.kind, DrawableKind::Image(im) if im.web_video.is_some()))
        .filter(|d| !st.hidden.contains(&d.id))
        .filter(|d| play.pending_web.contains(&d.id) || !scene.builds.iter().any(|b| b.drawable == d.id && matches!(builds::kind_of(b), Kind::Media(true))))
        .map(|d| d.id)
        .collect();
    for id in webs {
        play.pending_web.remove(&id);
        start_movie(app, play, ctx, scene, id, map);
    }
}

/// Fire the movie starts/stops of a running build group up to time `t`.
#[allow(clippy::too_many_arguments)]
fn group_media(app: &App, play: &mut Play, ctx: &egui::Context, scene: &SlideScene, g: &Group, fired: &mut [bool], t: f32, map: &Map) {
    for (i, tr) in g.tracks.iter().enumerate() {
        if fired[i] || t < tr.start {
            continue;
        }
        fired[i] = true;
        let id = tr.build.drawable;
        let is_movie = scene.drawables.iter().any(|d| d.id == id && matches!(d.kind, DrawableKind::Movie(_)));
        match tr.kind {
            Kind::Media(true) => start_movie(app, play, ctx, scene, id, map),
            Kind::Media(false) | Kind::Out if is_movie => play.players.retain(|p| p.drawable != id),
            Kind::In if is_movie && !scene.builds.iter().any(|b| b.drawable == id && matches!(builds::kind_of(b), Kind::Media(true))) => {
                start_movie(app, play, ctx, scene, id, map)
            }
            _ => {}
        }
    }
}

/// Leaving the slide: keep only movies that play across into the next slide.
fn hand_over_movies(play: &mut Play, next: Option<&SlideScene>) {
    let keep: Vec<(u64, ObjId)> = next
        .map(|sc| {
            sc.drawables
                .iter()
                .filter_map(|d| match &d.kind {
                    DrawableKind::Movie(m) if m.plays_across_slides => m.movie_data.map(|id| (id, d.id)),
                    // the camera stays on from one live video slide to the next
                    DrawableKind::Movie(m) if m.live => Some((0, d.id)),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    play.players.retain_mut(|p| match keep.iter().find(|(data, _)| *data == p.data_id) {
        Some((_, id)) if !p.finished() => {
            p.drawable = *id;
            true
        }
        _ => false,
    });
}

fn video_frames(play: &mut Play, ctx: &egui::Context) -> HashMap<ObjId, egui::TextureId> {
    let mut out = HashMap::new();
    for p in &mut play.players {
        let id = p.drawable;
        if let Some(t) = p.texture(ctx) {
            out.insert(id, t.id());
        }
    }
    out
}

// ------------------------------------------------------------------ video controls

/// The video the keys act on: the newest one playing that can be moved through.
fn active_player(play: &Play) -> Option<&media::Player> {
    play.players.iter().rev().find(|p| p.controllable() && !p.finished())
}

/// Keynote's slideshow movie keys: K pause / play, J back and L forward 10 s, I start, O end.
fn video_keys(play: &mut Play, ctx: &egui::Context) {
    let (k, j, l, i, o) = ctx.input(|i| (i.key_pressed(Key::K), i.key_pressed(Key::J), i.key_pressed(Key::L), i.key_pressed(Key::I), i.key_pressed(Key::O)));
    if !(k || j || l || i || o) {
        return;
    }
    let Some(p) = active_player(play) else { return };
    if k {
        p.set_paused(!p.paused());
    }
    if j {
        p.seek(p.position() - 10.0);
    }
    if l {
        p.seek(p.position() + 10.0);
    }
    if i {
        p.seek(0.0);
    }
    if o {
        p.seek(p.length());
    }
    play.pointer_moved = now();
    ctx.request_repaint();
}

fn clock(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// A control bar over each playing movie / web video while the pointer moves over it (and
/// while it's paused): play / pause, a timeline to click or drag, the time.
fn video_controls(play: &mut Play, ui: &mut egui::Ui, map: &Map, scene: &SlideScene) {
    let ctx = ui.ctx().clone();
    let (pointer, moved) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.delta() != egui::Vec2::ZERO || i.pointer.any_pressed()));
    if moved {
        play.pointer_moved = now();
    }
    let fresh = since(play.pointer_moved) < Duration::from_millis(2500);
    play.video_ctl.clear();
    for p in &play.players {
        if !p.controllable() {
            continue;
        }
        let Some(d) = scene.drawables.iter().find(|d| d.id == p.drawable) else { continue };
        let g = d.geom;
        let vr = Rect::from_min_max(map.pt(egui::pos2(g.x, g.y)), map.pt(egui::pos2(g.x + g.w, g.y + g.h)));
        let over = pointer.is_some_and(|q| vr.contains(q));
        if !(p.paused() || (fresh && over)) {
            continue;
        }
        let h = 40.0f32.min(vr.height() * 0.4).max(28.0);
        let bar = Rect::from_min_max(egui::pos2(vr.min.x + 10.0, vr.max.y - h - 10.0), egui::pos2(vr.max.x - 10.0, vr.max.y - 10.0));
        if bar.width() < 120.0 {
            continue;
        }
        play.video_ctl.push(bar);
        let painter = ui.painter().clone();
        painter.rect_filled(bar, h / 2.0, Color32::from_black_alpha(170));
        // play / pause
        let btn = Rect::from_center_size(egui::pos2(bar.min.x + h / 2.0 + 4.0, bar.center().y), egui::vec2(h - 8.0, h - 8.0));
        let r = ui.interact(btn, ui.scope_id().with(("video-play", p.drawable)), egui::Sense::click());
        if r.clicked() {
            p.set_paused(!p.paused());
        }
        let c = btn.center();
        let s = btn.height() * 0.28;
        if p.paused() {
            painter.add(egui::Shape::convex_polygon(vec![egui::pos2(c.x - s * 0.8, c.y - s), egui::pos2(c.x + s, c.y), egui::pos2(c.x - s * 0.8, c.y + s)], Color32::WHITE, egui::Stroke::NONE));
        } else {
            for dx in [-s * 0.55, s * 0.55] {
                painter.rect_filled(Rect::from_center_size(egui::pos2(c.x + dx, c.y), egui::vec2(s * 0.6, s * 2.0)), 1.5, Color32::WHITE);
            }
        }
        // the time and the timeline
        let (pos, len) = (p.position(), p.length());
        let label = format!("{} / {}", clock(pos), clock(len));
        let font = egui::FontId::proportional((h * 0.36).clamp(11.0, 15.0));
        let tw = painter.layout_no_wrap(label.clone(), font.clone(), Color32::WHITE).size().x;
        painter.text(egui::pos2(bar.max.x - h / 2.0, bar.center().y), egui::Align2::RIGHT_CENTER, label, font, Color32::WHITE);
        let track = Rect::from_min_max(egui::pos2(btn.max.x + 12.0, bar.center().y - 10.0), egui::pos2(bar.max.x - h / 2.0 - tw - 14.0, bar.center().y + 10.0));
        if track.width() > 20.0 {
            let r = ui.interact(track, ui.scope_id().with(("video-seek", p.drawable)), egui::Sense::click_and_drag());
            let f = if let (true, Some(q)) = (r.dragged() || r.clicked(), r.interact_pointer_pos()) {
                // dragging shows where it will go; it jumps on release (or a click)
                let f = ((q.x - track.min.x) / track.width()).clamp(0.0, 1.0);
                if r.clicked() {
                    p.seek(f64::from(f) * len);
                }
                f
            } else if len > 0.0 {
                (pos / len).clamp(0.0, 1.0) as f32
            } else {
                0.0
            };
            if r.drag_stopped() {
                if let Some(q) = r.interact_pointer_pos() {
                    p.seek(f64::from(((q.x - track.min.x) / track.width()).clamp(0.0, 1.0)) * len);
                }
            }
            let y = track.center().y;
            painter.line_segment([egui::pos2(track.min.x, y), egui::pos2(track.max.x, y)], egui::Stroke::new(4.0, Color32::from_white_alpha(70)));
            let x = track.min.x + f * track.width();
            painter.line_segment([egui::pos2(track.min.x, y), egui::pos2(x, y)], egui::Stroke::new(4.0, Color32::WHITE));
            painter.circle_filled(egui::pos2(x, y), if r.hovered() || r.dragged() { 7.0 } else { 5.0 }, Color32::WHITE);
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

// ------------------------------------------------------------------ input

#[derive(Default, Clone, Copy)]
struct Cmd {
    fwd: bool,
    back: bool,
    esc: bool,
    home: bool,
    end: bool,
    /// X: swap the slideshow and the presenter display.
    swap: bool,
    notes_bigger: bool,
    notes_smaller: bool,
    reset_timer: bool,
}

impl Cmd {
    fn read(ctx: &egui::Context, clicks: bool) -> Cmd {
        ctx.input(|i| Cmd {
            fwd: (clicks && i.pointer.primary_clicked())
                || i.key_pressed(Key::Space)
                || i.key_pressed(Key::ArrowRight)
                || i.key_pressed(Key::ArrowDown)
                || i.key_pressed(Key::PageDown)
                || i.key_pressed(Key::N)
                || i.key_pressed(Key::Enter),
            back: (clicks && i.pointer.secondary_clicked())
                || i.key_pressed(Key::ArrowLeft)
                || i.key_pressed(Key::ArrowUp)
                || i.key_pressed(Key::PageUp)
                || i.key_pressed(Key::P)
                || i.key_pressed(Key::Backspace),
            esc: i.key_pressed(Key::Escape) || i.key_pressed(Key::Q),
            home: i.key_pressed(Key::Home),
            end: i.key_pressed(Key::End),
            swap: i.key_pressed(Key::X),
            notes_bigger: i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals),
            notes_smaller: i.key_pressed(Key::Minus),
            reset_timer: i.key_pressed(Key::R),
        })
    }

    fn merge(self, o: Cmd) -> Cmd {
        Cmd {
            fwd: self.fwd || o.fwd,
            back: self.back || o.back,
            esc: self.esc || o.esc,
            home: self.home || o.home,
            end: self.end || o.end,
            swap: self.swap || o.swap,
            notes_bigger: self.notes_bigger || o.notes_bigger,
            notes_smaller: self.notes_smaller || o.notes_smaller,
            reset_timer: self.reset_timer || o.reset_timer,
        }
    }
}

// ------------------------------------------------------------------ presenter display

/// Number of connected displays (Linux DRM connectors); 1 if unknown.
fn connected_displays() -> usize {
    let Ok(rd) = std::fs::read_dir("/sys/class/drm") else { return 1 };
    let n = rd
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains('-'))
        .filter(|e| std::fs::read_to_string(e.path().join("status")).is_ok_and(|s| s.trim() == "connected"))
        .count();
    n.max(1)
}

struct Presenter {
    /// Separate presenter window (second display present, or KN_PRESENTER=window).
    window: bool,
    /// The slideshow window shows the presenter display (X), the other window the slides.
    swapped: bool,
    notes_size: f32,
    timer: Instant,
    /// Local time offset from UTC in seconds (from `date +%z`, read in the background).
    utc_offset: std::sync::Arc<std::sync::Mutex<Option<i64>>>,
}

impl Presenter {
    fn new(rehearse: bool, in_window: bool) -> Presenter {
        let mode = std::env::var("KN_PRESENTER").ok();
        let window = match mode.as_deref() {
            Some("window") => true,
            _ if in_window => true,
            Some("off" | "swap") => false,
            _ => connected_displays() > 1,
        };
        let swapped = rehearse || mode.as_deref() == Some("swap");
        let window = window && !rehearse;
        let utc_offset = std::sync::Arc::new(std::sync::Mutex::new(None));
        let o = utc_offset.clone();
        std::thread::spawn(move || {
            let out = std::process::Command::new("date").arg("+%z").stdin(std::process::Stdio::null()).output();
            if let Ok(out) = out {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if s.len() == 5 {
                    let sign = if s.starts_with('-') { -1 } else { 1 };
                    let h: i64 = s[1..3].parse().unwrap_or(0);
                    let m: i64 = s[3..5].parse().unwrap_or(0);
                    if let Ok(mut g) = o.lock() {
                        *g = Some(sign * (h * 3600 + m * 60));
                    }
                }
            }
        });
        Presenter { window, swapped, notes_size: 22.0, timer: now(), utc_offset }
    }

    fn clock(&self) -> String {
        let off = self.utc_offset.lock().ok().and_then(|g| *g).unwrap_or(0);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) + off;
        let s = now.rem_euclid(86400);
        format!("{:02}:{:02}", s / 3600, (s / 60) % 60)
    }
}

fn hms(secs: u64) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
}

/// Everything the presenter display shows, as plain data (also sent to the presenter window).
#[derive(Clone, Default)]
struct PView {
    ready: bool,
    index: usize,
    count: usize,
    clock: String,
    elapsed: u64,
    notes: String,
    notes_size: f32,
    aspect: f32,
    next_label: &'static str,
    next: Option<TextureHandle>,
    end_of_show: bool,
    rehearse: bool,
    /// Current state and live movie frames over it (for the presenter window).
    cur: Option<TextureHandle>,
    movies: Vec<(egui::TextureId, kn_model::Drawable, builds::Xform)>,
    slide_w: f32,
    /// Play > Customize Presenter Display: [current, next, notes, clock, timer, ready].
    show: [bool; 6],
}

#[derive(Default)]
struct PActions {
    bigger: bool,
    smaller: bool,
    reset: bool,
}

fn presenter_view(app: &App, play: &Play, scene: &SlideScene, frames: &HashMap<ObjId, egui::TextureId>) -> PView {
    let has_next_build = play.step < steps(scene);
    let next = if has_next_build {
        let to = play.cache.groups.get(&(play.index, play.step)).map(|g| g.to).unwrap_or(play.step + 1);
        play.cache.states.get(&(play.index, to)).cloned()
    } else {
        next_visible(app, play.index).and_then(|i| play.cache.states.get(&(i, 0)).cloned())
    };
    let st = builds::state_at(scene, play.step);
    let movies = scene
        .drawables
        .iter()
        .filter(|d| !st.hidden.contains(&d.id))
        .filter_map(|d| frames.get(&d.id).map(|t| (*t, d.clone(), st.xform(d.id))))
        .collect();
    PView {
        ready: play.anim.is_none(),
        index: play.index,
        count: app.slides.len(),
        clock: play.presenter.clock(),
        elapsed: since(play.presenter.timer).as_secs(),
        notes: app.slides.get(play.index).and_then(|s| app.doc.as_ref().map(|d| d.notes(s.slide))).unwrap_or_default(),
        notes_size: play.presenter.notes_size,
        aspect: scene.size.h / scene.size.w.max(1.0),
        next_label: if has_next_build { "Next build" } else { "Next slide" },
        end_of_show: !has_next_build && next_visible(app, play.index).is_none(),
        rehearse: play.rehearse,
        next,
        cur: play.cache.states.get(&(play.index, play.step)).cloned(),
        movies,
        slide_w: scene.size.w,
        show: {
            let s = &app.settings;
            [s.pd_current, s.pd_next, s.pd_notes, s.pd_clock, s.pd_timer, s.pd_ready]
        },
    }
}

/// The presenter display: current slide, next slide or build, notes, timer, clock.
/// `draw_current` paints the current slide into the given rect.
fn presenter_ui(ui: &mut egui::Ui, v: &PView, draw_current: &mut dyn FnMut(&egui::Painter, Rect)) -> PActions {
    let mut act = PActions::default();
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, crate::theme::tint(Color32::from_gray(18)));
    let pad = 16.0;
    let top_h = 54.0;
    let p = ui.painter().clone();
    let [show_cur, show_next, show_notes, show_clock, show_timer, show_ready] = v.show;
    let (label, col) = if v.ready { ("Ready to advance", Color32::from_rgb(60, 190, 90)) } else { ("Wait…", Color32::from_rgb(220, 70, 60)) };
    let bar = Rect::from_min_size(full.min, egui::vec2(full.width(), top_h));
    if show_ready {
        p.rect_filled(Rect::from_min_size(bar.min, egui::vec2(full.width(), 6.0)), 0.0, col);
        p.text(bar.left_center() + egui::vec2(pad, 3.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(20.0), col);
    }
    let title = if v.rehearse { format!("Rehearse Slideshow · Slide {} of {}", v.index + 1, v.count) } else { format!("Slide {} of {}", v.index + 1, v.count) };
    crate::theme::centered(&p, bar.center() + egui::vec2(0.0, 3.0), title, egui::FontId::proportional(20.0), crate::theme::tint(Color32::from_gray(220)));
    p.text(
        bar.right_center() + egui::vec2(-pad, 3.0),
        egui::Align2::RIGHT_CENTER,
        match (show_clock, show_timer) {
            (true, true) => format!("{}    {}", v.clock, hms(v.elapsed)),
            (true, false) => v.clock.clone(),
            (false, true) => hms(v.elapsed),
            _ => String::new(),
        },
        egui::FontId::monospace(22.0),
        crate::theme::tint(Color32::from_gray(235)),
    );
    // slides: current on the left, next on the right
    let body = Rect::from_min_max(full.min + egui::vec2(pad, top_h + 8.0), full.max - egui::vec2(pad, pad));
    // the slides get more room without notes; one slide alone is centred
    let slides_h = if show_notes { body.height() * 0.58 } else { body.height() - 30.0 };
    let aspect = v.aspect.max(0.1);
    let (cur_share, next_share) = match (show_cur, show_next) {
        (true, true) => (0.6, 0.7),
        (true, false) => (1.0, 0.0),
        (false, true) => (0.0, 1.0),
        _ => (0.0, 0.0),
    };
    let cur_w = if show_cur { (body.width() * cur_share - if show_next { pad } else { 0.0 }).min(slides_h / aspect).max(10.0) } else { 0.0 };
    let cur = Rect::from_min_size(body.min + egui::vec2(if show_next { 0.0 } else { (body.width() - cur_w) / 2.0 }, 22.0), egui::vec2(cur_w, cur_w * aspect));
    let next_w = if show_next { (body.width() - cur_w - if show_cur { 2.0 * pad } else { 0.0 }).min(slides_h * next_share / aspect).max(10.0) } else { 0.0 };
    let next_x = if show_cur { body.max.x - next_w } else { body.center().x - next_w / 2.0 };
    let next = Rect::from_min_size(egui::pos2(next_x, body.min.y + 22.0), egui::vec2(next_w, next_w * aspect));
    let small = egui::FontId::proportional(15.0);
    let grey = crate::theme::tint(Color32::from_gray(170));
    if show_cur {
        p.text(cur.left_top() - egui::vec2(0.0, 4.0), egui::Align2::LEFT_BOTTOM, "Current", small.clone(), grey);
        p.rect_filled(cur, 0.0, Color32::BLACK);
        draw_current(&ui.painter_at(cur), cur);
    }
    if show_next {
        p.text(next.left_top() - egui::vec2(0.0, 4.0), egui::Align2::LEFT_BOTTOM, v.next_label, small, grey);
    }
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    if show_next { match &v.next {
        Some(t) => {
            p.image(t.id(), next, uv, Color32::WHITE);
        }
        None => {
            p.rect_filled(next, 0.0, Color32::BLACK);
            let msg = if v.end_of_show { "End of show" } else { "" };
            crate::theme::centered(&p, next.center(), msg, egui::FontId::proportional(18.0), grey);
        }
    } }
    if !show_notes {
        return act;
    }
    // notes
    let notes_top = if show_cur || show_next { cur.max.y.max(next.max.y) + 14.0 } else { body.min.y };
    let notes_rect = Rect::from_min_max(egui::pos2(body.min.x, notes_top), body.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(notes_rect));
    child.horizontal(|ui| {
        ui.label(egui::RichText::new("Presenter Notes").color(grey));
        act.smaller = ui.small_button("A−").clicked();
        act.bigger = ui.small_button("A+").clicked();
        act.reset = ui.small_button("Reset timer").clicked();
        ui.label(egui::RichText::new("X swap displays · +/− notes size · R reset timer").color(crate::theme::tint(Color32::from_gray(110))).size(12.0));
    });
    egui::ScrollArea::vertical().id_salt(("notes", v.index)).auto_shrink([false, false]).show(&mut child, |ui| {
        let text = if v.notes.trim().is_empty() { "No notes for this slide." } else { v.notes.as_str() };
        ui.label(egui::RichText::new(text).size(v.notes_size).color(crate::theme::tint(Color32::from_gray(235))));
    });
    act
}

fn apply_actions(play: &mut Play, a: &PActions) {
    if a.bigger {
        play.presenter.notes_size = (play.presenter.notes_size + 2.0).min(72.0);
    }
    if a.smaller {
        play.presenter.notes_size = (play.presenter.notes_size - 2.0).max(10.0);
    }
    if a.reset {
        play.presenter.timer = now();
    }
}

/// State shared with the presenter window (a deferred viewport, drawn by egui separately).
#[derive(Default)]
struct WindowShared {
    view: PView,
    cmd: Cmd,
    actions: PActions,
    open: bool,
}

fn show_presenter_window(ctx: &egui::Context, shared: &std::sync::Arc<std::sync::Mutex<WindowShared>>) {
    let sh = shared.clone();
    let builder = egui::ViewportBuilder::default().with_title("Presenter Display").with_inner_size([1280.0, 800.0]);
    ctx.show_viewport_deferred(egui::ViewportId::from_hash_of("kn-presenter"), builder, move |ui, _class| {
        let cctx = ui.ctx().clone();
        let view = match sh.lock() {
            Ok(g) => g.view.clone(),
            Err(_) => return,
        };
        let mut cmd = Cmd::read(&cctx, false);
        if cctx.input(|i| i.viewport().close_requested()) {
            cmd.esc = true;
        }
        let mut draw = |p: &egui::Painter, r: Rect| {
            if let Some(t) = &view.cur {
                p.image(t.id(), r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            let map = Map::new(r, view.slide_w);
            for (tex, d, x) in &view.movies {
                builds::draw_movie(p, &map, d, d.geom, x, *tex);
            }
        };
        let act = presenter_ui(ui, &view, &mut draw);
        if let Ok(mut g) = sh.lock() {
            g.cmd = g.cmd.merge(cmd);
            g.actions.bigger |= act.bigger;
            g.actions.smaller |= act.smaller;
            g.actions.reset |= act.reset;
        }
        cctx.request_repaint_after(Duration::from_millis(if view.movies.is_empty() { 250 } else { 30 }));
    });
}

// ------------------------------------------------------------------ frame

/// A dynamic background, animated, filling `rect`.
fn motion_texture(app: &mut App, ctx: &egui::Context, rect: Rect, mb: &kn_model::motionbg::MotionBg) -> TextureHandle {
    // rendered (on a worker thread) at about 40 % of the screen size and scaled up smoothly
    let w = ((rect.width() * ctx.pixels_per_point()) / 2.5).clamp(480.0, 1280.0) as u32;
    let h = (w as f32 * rect.height() / rect.width().max(1.0)).round().max(2.0) as u32;
    let t = since(app.motion_clock).as_secs_f32();
    ctx.request_repaint();
    app.motion_tex.get(ctx, mb, t, w, h)
}

fn draw_motion(app: &mut App, ctx: &egui::Context, painter: &egui::Painter, rect: Rect, mb: &kn_model::motionbg::MotionBg) {
    let tex = motion_texture(app, ctx, rect, mb);
    painter.image(tex.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    ctx.request_repaint();
}

/// Draw the show (running animation, or the current state with live movie frames) into `rect`.
fn draw_slides(app: &mut App, play: &mut Play, ctx: &egui::Context, painter: &egui::Painter, rect: Rect, scene: &SlideScene, frames: &HashMap<ObjId, egui::TextureId>) {
    let map = Map::new(rect, scene.size.w);
    match &play.anim {
        Some((Anim::Build { g, .. }, start)) => {
            if let Some(mb) = &scene.motion {
                draw_motion(app, ctx, painter, rect, mb);
            }
            let t = play.anim_time(*start);
            let sizes: HashMap<ObjId, egui::Vec2> = scene.drawables.iter().map(|d| (d.id, egui::vec2(d.geom.w, d.geom.h))).collect();
            let mut video = |id: ObjId| frames.get(&id).map(|t| (*t, sizes.get(&id).copied().unwrap_or_default()));
            g.draw(painter, &map, t, &mut video);
        }
        Some((Anim::Trans(tr), start)) => {
            if let Some(mb) = &tr.live {
                draw_motion(app, ctx, painter, rect, mb);
            }
            let p = play.anim_time(*start) / tr.duration;
            // each slide's own dynamic background travels with it
            let mut under = Vec::new();
            if let Some(mb) = &tr.to_live {
                let bg = motion_texture(app, ctx, rect, mb).id();
                under.push((tr.to.id(), bg));
                if let Some((_, ob, _)) = &tr.objs {
                    under.push((ob.base.id(), bg));
                }
            }
            if let Some(f) = &tr.from_bg {
                under.push((tr.from.id(), f.id()));
                if let Some((oa, _, _)) = &tr.objs {
                    under.push((oa.base.id(), f.id()));
                }
            }
            transitions::with_underlays(&under, || match &tr.objs {
                Some((oa, ob, m)) => transitions::draw_objects(painter, rect, scene.size.w, &tr.effect, p, oa, ob, m, tr.dir),
                None => transitions::draw(painter, rect, tr.from.id(), tr.to.id(), &tr.effect, p, tr.color, tr.dir),
            });
        }
        None => {
            let (i, s) = (play.index, play.step);
            let live = scene.motion.clone();
            if let Some(mb) = &live {
                draw_motion(app, ctx, painter, rect, mb);
            }
            if let Some(t) = state_tex(app, play, ctx, i, s) {
                painter.image(t.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            // live movie frames over their posters, then whatever lies above the lowest playing
            // movie again (text over a background video must stay on top of it)
            if !frames.is_empty() {
                let st = builds::state_at(scene, play.step);
                let mut lowest = None;
                for (z, d) in scene.drawables.iter().enumerate() {
                    if let Some(tex) = frames.get(&d.id) {
                        if !st.hidden.contains(&d.id) {
                            builds::draw_movie(painter, &map, d, d.geom, &st.xform(d.id), *tex);
                            lowest.get_or_insert(z);
                        }
                    }
                }
                if let Some(z) = lowest.filter(|z| z + 1 < scene.drawables.len()) {
                    if let Some(t) = state_tex_above(app, play, ctx, i, s, z) {
                        painter.image(t.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    }
                }
            }
        }
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let Some(mut play) = app.play.take() else { return };
    if !play.started_fullscreen {
        if std::env::var_os("KN_NOFS").is_none() && !play.in_window {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
        play.started_fullscreen = true;
        // start on a visible slide
        if app.slides.get(play.index).is_some_and(|s| s.skipped) {
            play.index = next_visible(app, play.index).or_else(|| prev_visible(app, play.index)).unwrap_or(0);
        }
    }
    let screen = ctx.content_rect();
    ui.painter_at(screen).rect_filled(screen, 0.0, Color32::BLACK);
    // don't render anything at the windowed size while the window goes full screen
    play.frames += 1;
    if std::env::var_os("KN_DEBUG").is_some() && play.frames % 30 == 0 {
        eprintln!("play frame {} fullscreen {:?}", play.frames, ctx.input(|i| i.viewport().fullscreen));
    }
    if !play.in_window && play.frames < 60 && ctx.input(|i| i.viewport().fullscreen) != Some(true) {
        ctx.request_repaint();
        app.play = Some(play);
        return;
    }

    let Some(mut scene) = ({ let i = play.index; scene_rc(app, &mut play, i) }) else {
        exit(app, &ctx, play.in_window, play.recording.take(), play.narration.take());
        return;
    };
    // where the slides go: this window, or the presenter window when swapped
    let slides_here = !play.presenter.swapped;
    let slide_screen = screen;
    let size = scene.size;
    // Settings > Slideshow > Scale slides up to fit display (off: never above 100 %)
    let max_k = if app.settings.scale_slides_up { f32::INFINITY } else { 1.0 / ctx.pixels_per_point() };
    let rect_on = move |scr: Rect| {
        let k = (scr.width() / size.w).min(scr.height() / size.h).min(max_k);
        Rect::from_center_size(scr.center(), egui::vec2(size.w * k, size.h * k))
    };
    let rect = rect_on(slide_screen);
    let width_px = (rect.width() * ctx.pixels_per_point()).round().max(16.0) as u32;
    if play.cache.width != width_px {
        play.cache = Cache { width: width_px, ..Default::default() };
        play.anim = None;
        play.prep = None;
    }
    let map = Map::new(rect, scene.size.w);

    // the preparation worker (restarted if the slide size on screen changes), and what it made
    if play.prep.is_none() {
        if let Some(doc) = app.doc.as_ref() {
            play.prep = Some(PrepWorker::start(&ctx, doc.file.clone(), app.slides.clone(), width_px));
        }
    }
    if let Some(w) = play.prep.as_ref() {
        let done: Vec<PrepDone> = std::mem::take(&mut *w.done.lock().unwrap());
        for d in done {
            match d {
                PrepDone::State(i, s, bg, tex) => {
                    let key = if bg { s } else { s | (1 << 40) };
                    play.cache.states.entry((i, key)).or_insert(tex);
                    play.cache.touch(0, i, key);
                }
                PrepDone::Group(i, s, g) => {
                    play.cache.groups.entry((i, s)).or_insert_with(|| Rc::new(g));
                    play.cache.touch(1, i, s);
                }
            }
        }
    }

    // ------------------------------------------------------------------ input
    let mut cmd = Cmd::read(&ctx, slides_here);
    // clicks on a video's controls are for the video, not the show
    let on_ctl = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| play.video_ctl.iter().any(|r| r.contains(p)));
    if on_ctl {
        cmd.fwd = false;
        cmd.back = false;
    }
    video_keys(&mut play, &ctx);
    // KN_PAUSE_AT=<seconds>: pause the playing video then (tests)
    if let Some(at) = std::env::var("KN_PAUSE_AT").ok().and_then(|s| s.parse::<f32>().ok()) {
        if since(play.started).as_secs_f32() >= at {
            if let Some(p) = active_player(&play).filter(|p| !p.paused()) {
                static ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                if !ONCE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    p.set_paused(true);
                }
            }
        } else {
            ctx.request_repaint();
        }
    }
    cmd = cmd.merge(std::mem::take(&mut play.child_cmd));
    if play.clicks.first().is_some_and(|t| since(play.started).as_secs_f32() >= *t) {
        play.clicks.remove(0);
        cmd.fwd = true;
    }
    if !play.clicks.is_empty() {
        ctx.request_repaint();
    }
    // recording: click when nothing has moved for the hold time
    if let (Some(_), Ok(until)) = (play.recording.as_ref(), std::env::var("KN_RECORD_UNTIL").unwrap_or_default().parse::<usize>()) {
        if play.index + 1 > until {
            exit(app, &ctx, play.in_window, play.recording.take(), play.narration.take());
            return;
        }
    }
    if let Some(rec) = play.recording.as_mut() {
        if play.anim.is_some() {
            rec.last_busy = now();
        } else if since(rec.last_busy).as_secs_f32() >= rec.hold && since(play.shown_at).as_secs_f32() >= rec.hold {
            cmd.fwd = true;
            rec.last_busy = now();
        }
        cmd.back = false;
    }
    // Play Recorded Slideshow: the recording's navigation drives the show (keys other than Esc
    // are ignored, as in Keynote's playback)
    if let Some(rp) = play.replay.as_mut() {
        use kn_model::recording::Phase;
        (cmd.fwd, cmd.back, cmd.home, cmd.end) = (false, false, false, false);
        let t = since(rp.started).as_secs_f64();
        let mut snap: Option<(usize, usize)> = None;
        while let Some(e) = rp.nav.get(rp.next).filter(|e| e.time <= t).cloned() {
            let Some(ti) = app.slides.iter().position(|s| s.node == e.slide_node) else {
                rp.next += 1;
                continue;
            };
            let te = e.event as usize;
            if (ti, te) == (play.index, play.step) {
                rp.next += 1;
                continue;
            }
            if play.anim.is_some() {
                break; // let the running animation finish first
            }
            rp.next += 1;
            let next_slide = te == 0 && play.step >= steps(&scene) && next_visible(app, play.index) == Some(ti);
            if e.phase == Phase::Start && ((ti == play.index && te > play.step) || next_slide) {
                cmd.fwd = true;
                break;
            }
            snap = Some((ti, te));
        }
        if let Some((ti, te)) = snap {
            play.index = ti;
            play.step = te;
            play.autoplay_pending = true;
        }
        if rp.next >= rp.nav.len() && t >= rp.duration && play.anim.is_none() {
            exit(app, &ctx, play.in_window, play.recording.take(), play.narration.take());
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(15));
    }
    let (mut fwd, back, home, end) = (cmd.fwd, cmd.back, cmd.home, cmd.end);
    // Links Only: clicks and keys don't advance; only hyperlinks move through the show
    if app.doc.as_ref().is_some_and(|d| d.show_settings().mode == kn_model::slides::ShowMode::LinksOnly) {
        fwd = false;
    }
    // hyperlinks: a click on a linked object follows the link instead of advancing
    let mut link_jump: Option<usize> = None;
    if slides_here {
        if let Some(pos) = ctx.pointer_hover_pos() {
            // the pointer: hidden while still for 2 s, or (Settings > Slideshow) only shown on
            // slides with links or movies
            let link_here = crate::links::link_at(&scene, rect, pos).is_some();
            let has_movies = scene.drawables.iter().any(|d| matches!(d.kind, kn_model::DrawableKind::Movie(_)));
            let idle = ctx.input(|i| i.pointer.time_since_last_movement()) > 2.0;
            if !link_here && (idle || (!app.settings.show_pointer_always && !has_movies)) {
                ctx.set_cursor_icon(egui::CursorIcon::None);
            }
            if !idle {
                ctx.request_repaint_after(std::time::Duration::from_millis(2100));
            }
            if let Some(url) = crate::links::link_at(&scene, rect, pos) {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                if ctx.input(|i| i.pointer.primary_clicked()) {
                    fwd = false;
                    let n = app.slides.len();
                    match crate::links::action(&url) {
                        crate::links::LinkAction::Open(u) => crate::links::open_url(&u),
                        crate::links::LinkAction::Next => fwd = true,
                        crate::links::LinkAction::Previous => link_jump = Some(play.index.saturating_sub(1)),
                        crate::links::LinkAction::First => link_jump = Some(0),
                        crate::links::LinkAction::Last => link_jump = Some(n.saturating_sub(1)),
                        crate::links::LinkAction::Slide(i) => link_jump = Some(i.min(n.saturating_sub(1))),
                        crate::links::LinkAction::SlideId(u) => link_jump = app.doc.as_ref().and_then(|d| d.slide_for_link_uuid(&u)),
                        crate::links::LinkAction::LastViewed => link_jump = Some(play.last_viewed.min(n.saturating_sub(1))),
                        crate::links::LinkAction::Exit => {
                            exit(app, &ctx, play.in_window, play.recording.take(), play.narration.take());
                            return;
                        }
                    }
                }
            }
        }
    }
    if let Some(i) = link_jump {
        play.last_viewed = play.index;
        play.index = i;
        play.step = 0;
        play.anim = None;
    }
    if cmd.esc {
        exit(app, &ctx, play.in_window, play.recording.take(), play.narration.take());
        return;
    }
    if cmd.swap {
        play.presenter.swapped = !play.presenter.swapped;
    }
    if cmd.notes_bigger {
        play.presenter.notes_size = (play.presenter.notes_size + 2.0).min(72.0);
    }
    if cmd.notes_smaller {
        play.presenter.notes_size = (play.presenter.notes_size - 2.0).max(10.0);
    }
    if cmd.reset_timer {
        play.presenter.timer = now();
    }
    if ctx.input(|i| i.pointer.delta().length() > 0.0) {
        play.show_hud = Some(now());
    }

    // a finished animation settles into its end state; a click during one jumps to its end
    let mut finished = false;
    if let Some((a, start)) = &play.anim {
        let t = play.anim_time(*start);
        let done = match a {
            Anim::Build { g, .. } => t >= g.total,
            Anim::Trans(tr) => t >= tr.duration,
        };
        if (done && play.anim_cap.is_none()) || fwd {
            finished = true;
            if !done {
                fwd = false;
            }
        }
    }
    if finished && std::env::var_os("KN_DEBUG").is_some() {
        eprintln!("animation done: slowest frame {:.1} ms", play.worst_frame.1 * 1000.0);
    }
    if play.anim.is_some() && std::env::var_os("KN_DEBUG").is_some() {
        let now = now();
        if let Some(prev) = play.worst_frame.0 {
            play.worst_frame.1 = play.worst_frame.1.max((now - prev).as_secs_f32());
        }
        play.worst_frame.0 = Some(now);
    } else {
        play.worst_frame = (None, 0.0);
    }
    if finished {
        if let Some((a, _)) = play.anim.take() {
            match a {
                Anim::Build { g, mut fired } => {
                    group_media(app, &mut play, &ctx, &scene, &g, &mut fired, g.total + 1.0, &map);
                    play.step = g.to;
                }
                Anim::Trans(_) => play.autoplay_pending = true,
            }
        }
        play.shown_at = now();
    }

    // automatic advance: builds that start after the transition, and automatic transitions
    let show = app.doc.as_ref().map(|d| d.show_settings());
    let self_playing = show.is_some_and(|s| s.mode == kn_model::slides::ShowMode::SelfPlaying);
    let mut auto = false;
    if play.anim.is_none() {
        let since = since(play.shown_at).as_secs_f64();
        if play.step < steps(&scene) {
            if play.step == 0 && scene.build_order[0].automatic {
                auto = true;
            } else if self_playing && since >= show.map(|s| s.build_delay).unwrap_or(2.0) {
                // Self-Playing: every build advances by itself after the build delay
                auto = true;
            }
        } else if let Some(t) = scene.transition.as_ref().filter(|t| t.automatic) {
            if since >= t.delay {
                auto = true;
            }
        } else if self_playing && since >= show.map(|s| s.transition_delay).unwrap_or(5.0) {
            auto = true;
        }
        if auto {
            ctx.request_repaint();
        } else if self_playing {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    if play.anim.is_none() && play.autoplay_pending {
        play.autoplay_pending = false;
        autoplay(app, &mut play, &ctx, &scene, &map);
    }

    if (fwd || auto) && play.anim.is_none() {
        if play.step < steps(&scene) {
            if let Some(g) = { let (i, s) = (play.index, play.step); group(app, &mut play, &ctx, i, s) } {
                let fired = vec![false; g.tracks.len()];
                play.begin(Anim::Build { g, fired });
            }
        } else if let Some(n) = next_visible(app, play.index) {
            let tr = scene.transition.clone();
            let next_motion = scene_rc(app, &mut play, n).and_then(|sc| sc.motion.clone());
            // a dynamic background plays under the transition: the next slide's (or this one's);
            // slides with one are drawn without it
            let live = match (&scene.motion, &next_motion) {
                (Some(a), Some(b)) if same_motion(a, b) => Some(a.clone()),
                _ => None,
            };
            let to_live = next_motion.clone().filter(|_| live.is_none());
            let from_bg = scene.motion.as_ref().filter(|_| live.is_none()).and_then(|a| app.motion_tex.freeze(&ctx, a));
            let from = { let (i, s) = (play.index, play.step); state_tex(app, &mut play, &ctx, i, s) };
            let objs = match &tr {
                Some(t) if transitions::needs_objects(&t.effect) => {
                    let a = { let (i, s) = (play.index, play.step); objset(app, &mut play, &ctx, i, s) };
                    let b = objset(app, &mut play, &ctx, n, 0);
                    a.zip(b).map(|(a, b)| {
                        let m = transitions::matches(&a, &b);
                        (a, b, m)
                    })
                }
                _ => None,
            };
            let next_scene = scene_rc(app, &mut play, n);
            hand_over_movies(&mut play, next_scene.as_deref());
            play.index = n;
            play.step = 0;
            // a movie that plays on from the last slide is already started: Keynote skips its
            // Start Movie click
            if let Some(sc) = next_scene.as_deref() {
                while let Some(b) = sc.build_order.get(play.step).and_then(|st| sc.builds.iter().find(|b| b.id == st.build)) {
                    if !(b.effect.ends_with("movie-start") && play.players.iter().any(|p| p.drawable == b.drawable)) {
                        break;
                    }
                    play.step += 1;
                }
            }
            let to = { let s = play.step; state_tex(app, &mut play, &ctx, n, s) };
            play.autoplay_pending = true;
            if let (Some(tr), Some(from), Some(to)) = (tr, from, to) {
                let c = tr.color.map(|c| Color32::from_rgb((c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8)).unwrap_or(Color32::BLACK);
                play.begin(Anim::Trans(TransAnim { from, to, objs, effect: tr.effect, dir: tr.direction, color: c, duration: tr.duration.clamp(0.05, 20.0) as f32, live, to_live, from_bg }));
                play.autoplay_pending = false;
            }
            if let Some(sc) = next_scene {
                scene = sc;
            }
        } else if show.is_some_and(|s| s.looping) {
            // Loop slideshow: back to the first slide (Esc exits)
            if let Some(first) = (0..app.slides.len()).find(|i| !app.slides[*i].skipped) {
                play.index = first;
                play.step = 0;
                play.players.clear();
                play.autoplay_pending = true;
                if let Some(sc) = scene_rc(app, &mut play, first) {
                    scene = sc;
                }
            }
        } else if !auto {
            // past the last slide: Keynote shows black, then exits on the next click
            exit(app, &ctx, play.in_window, play.recording.take(), play.narration.take());
            return;
        }
        play.shown_at = now();
    } else if back {
        // Keynote goes back at once, even mid-animation: a running build or transition is cut
        // short; the previous slide is shown fully built (its automatic builds don't replay)
        play.anim = None;
        play.players.clear();
        if play.step > 0 {
            play.step = 0;
            play.autoplay_pending = true;
        } else if let Some(p) = prev_visible(app, play.index) {
            play.index = p;
            let sc = scene_rc(app, &mut play, p);
            play.step = sc.as_deref().map(steps).unwrap_or(0);
            if let Some(sc) = sc {
                scene = sc;
            }
            play.autoplay_pending = false;
        }
        play.shown_at = now();
    } else if home || end {
        let idx = if home { (0..app.slides.len()).find(|i| !app.slides[*i].skipped) } else { (0..app.slides.len()).rev().find(|i| !app.slides[*i].skipped) };
        play.index = idx.unwrap_or(0);
        play.step = 0;
        play.anim = None;
        play.players.clear();
        play.autoplay_pending = true;
        if let Some(sc) = { let i = play.index; scene_rc(app, &mut play, i) } {
            scene = sc;
        }
    }

    // movie starts/stops of a running build group
    if let Some((Anim::Build { g, fired }, start)) = &mut play.anim {
        let (g, mut f, t) = (g.clone(), std::mem::take(fired), since(*start).as_secs_f32());
        let t = play.anim_cap.map(|c| t.min(c)).unwrap_or(t);
        group_media(app, &mut play, &ctx, &scene, &g, &mut f, t, &map);
        if let Some((Anim::Build { fired, .. }, _)) = &mut play.anim {
            *fired = f;
        }
    }

    // ------------------------------------------------------------------ draw
    let frames = video_frames(&mut play, &ctx);
    let idle = play.anim.is_none();
    if play.anim.is_some() {
        ctx.request_repaint();
    }
    if slides_here {
        draw_slides(app, &mut play, &ctx, &ui.painter_at(rect), rect, &scene, &frames);
        video_controls(&mut play, ui, &map, &scene);
    } else {
        let view = presenter_view(app, &play, &scene, &frames);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(screen));
        let act = presenter_ui(&mut child, &view, &mut |p: &egui::Painter, r: Rect| draw_slides(app, &mut play, &ctx, p, r, &scene, &frames));
        apply_actions(&mut play, &act);
        ctx.request_repaint_after(Duration::from_millis(500));
    }
    // the presenter window (second display)
    if play.presenter.window {
        let view = presenter_view(app, &play, &scene, &frames);
        let (cmd, act) = match play.shared.lock() {
            Ok(mut g) => {
                g.view = view;
                g.open = true;
                (std::mem::take(&mut g.cmd), std::mem::take(&mut g.actions))
            }
            Err(_) => (Cmd::default(), PActions::default()),
        };
        play.child_cmd = cmd;
        apply_actions(&mut play, &act);
        show_presenter_window(&ctx, &play.shared);
        ctx.request_repaint_after(Duration::from_millis(250));
    }
    if play.players.iter().any(|p| !p.finished()) {
        ctx.request_repaint_after(Duration::from_millis(15));
    }
    // warm the cache for what comes next while idle (one asset per frame)
    if (idle || play.prep.is_some()) && prepare_next(app, &mut play, &ctx, &scene) {
        ctx.request_repaint();
    }
    // keep ticking for automatic advances
    if scene.transition.as_ref().is_some_and(|t| t.automatic) || scene.build_order.iter().any(|b| b.automatic) {
        ctx.request_repaint_after(Duration::from_millis(30));
    }
    if let Some(h) = play.show_hud {
        if since(h) < Duration::from_secs(2) && slides_here && play.recording.is_none() {
            let n = app.slides.len();
            let hint = if play.presenter.window { "" } else { "   X presenter display" };
            ui.painter_at(screen).text(
                screen.right_bottom() + egui::vec2(-20.0, -16.0),
                egui::Align2::RIGHT_BOTTOM,
                format!("{} / {}   Esc to exit{hint}", play.index + 1, n),
                egui::FontId::proportional(14.0),
                Color32::from_white_alpha(160),
            );
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
    if let Some(rec) = play.recording.as_mut() {
        record_frame(rec, &ctx, rect);
    }
    if let Some(n) = play.narration.as_mut() {
        log_navigation(app, n, play.index, play.step, play.anim.is_some());
    }
    app.cur = play.index;
    app.play = Some(play);
}

/// Record Slideshow: note where the show went, Keynote style. The first slide is a "None" event at
/// 0 s; every later move is "Start" when its animation began (or now) and "End" once it settles.
fn log_navigation(app: &App, n: &mut Narration, index: usize, step: usize, anim: bool) {
    use kn_model::recording::{NavEvent, Phase};
    let node = |i: usize| app.slides.get(i).map(|s| s.node).unwrap_or(0);
    let at = |t: Instant| t.saturating_duration_since(n.started).as_secs_f64();
    if anim && !n.was_anim {
        n.anim_since = Some(Instant::now());
    }
    n.was_anim = anim;
    let state = (index, step);
    if n.last != Some(state) {
        if n.last.is_none() {
            n.events.push(NavEvent { time: 0.0, slide_node: node(index), event: step as u32, phase: Phase::None });
        } else {
            let t0 = n.anim_since.take().unwrap_or_else(Instant::now);
            n.events.push(NavEvent { time: at(t0), slide_node: node(index), event: step as u32, phase: Phase::Start });
            if anim {
                n.pending_end = Some(state);
            } else {
                n.events.push(NavEvent { time: at(Instant::now()), slide_node: node(index), event: step as u32, phase: Phase::End });
            }
        }
        n.last = Some(state);
    } else if !anim {
        if let Some((i, s)) = n.pending_end.take() {
            n.events.push(NavEvent { time: at(Instant::now()), slide_node: node(i), event: s as u32, phase: Phase::End });
        }
    }
}

/// Start Play > Record Slideshow from slide `from`: the microphone is recorded (PipeWire's
/// pw-record, else ALSA's arecord) while the show runs.
pub fn start_narration(app: &mut App, from: usize) {
    let dir = std::env::temp_dir().join(format!("kn-narration-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let wav = dir.join("narration.wav");
    let _ = std::fs::remove_file(&wav);
    let null = || std::process::Stdio::null();
    // KN_NO_MIC (automated checks): timing only, silent narration
    let mic = if std::env::var_os("KN_NO_MIC").is_some() { None } else { std::process::Command::new("pw-record")
        .args(["--rate", "48000", "--channels", "2"])
        .arg(&wav)
        .stdin(null())
        .stdout(null())
        .stderr(null())
        .spawn()
        .or_else(|_| std::process::Command::new("arecord").args(["-f", "dat"]).arg(&wav).stdin(null()).stdout(null()).stderr(null()).spawn())
        .ok() };
    if mic.is_none() && std::env::var_os("KN_NO_MIC").is_none() {
        app.notify("No microphone recorder found (pw-record or arecord): recording the timing only");
    }
    app.start_play(from, false);
    if let Some(p) = app.play.as_mut() {
        p.narration = Some(Narration { mic, wav, started: Instant::now(), events: Vec::new(), last: None, anim_since: None, was_anim: false, pending_end: None });
    }
}

/// Play > Clear Recording…: Keynote's confirmation, then the recording goes.
pub fn clear_recording_prompt(app: &mut App, ctx: &egui::Context) {
    if !app.confirm_clear_recording {
        return;
    }
    let (mut clear, mut cancel) = (false, false);
    egui::Window::new("Clear Recording").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).title_bar(false).show(ctx, |ui| {
        ui.label(crate::theme::bold("Are you sure you want to clear the recording?"));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            cancel = ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(Key::Escape));
            clear = ui.button("Clear").clicked();
        });
    });
    if cancel {
        app.confirm_clear_recording = false;
    }
    if clear {
        app.confirm_clear_recording = false;
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Clear Recording");
            doc.clear_recording(&mut app.hist);
            doc.end(&mut app.hist);
        }
        app.notify("Recording cleared");
    }
}

/// Start Play > Play Recorded Slideshow.
pub fn start_replay(app: &mut App, ctx: &egui::Context) {
    let Some(rec) = app.doc.as_ref().and_then(|d| d.recording()) else { return };
    let first = rec.nav.first().and_then(|e| app.slides.iter().position(|s| s.node == e.slide_node)).unwrap_or(0);
    let audio = rec.audio.and_then(|id| {
        let (path, len) = media::cache_file(app.doc.as_ref()?, id)?;
        let spec = media::Spec { path, len, start: 0.0, end: 0.0, volume: 1.0, looping: false, video: None };
        Some(media::Player::start(ctx, 0, id, spec))
    });
    app.start_play(first, false);
    if let Some(p) = app.play.as_mut() {
        p.replay = Some(Replay { nav: rec.nav, next: 0, duration: rec.duration, started: Instant::now(), _audio: audio });
    }
}

/// Record Slideshow is over: stop the microphone, encode the narration (AAC in a .mov, as
/// Keynote) and store the recording.
fn finish_narration(app: &mut App, mut n: Narration) {
    let duration = n.started.elapsed().as_secs_f64();
    if let Some(mut mic) = n.mic.take() {
        // SIGINT lets the recorder finish the WAV header
        let _ = std::process::Command::new("kill").args(["-INT", &mic.id().to_string()]).status();
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_secs(3) {
            if let Ok(Some(_)) = mic.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = mic.kill();
        let _ = mic.wait();
    }
    if n.events.len() < 1 || duration < 0.5 {
        app.notify("The recording was too short to keep");
        return;
    }
    let mov = n.wav.with_extension("mov");
    let _ = std::fs::remove_file(&mov);
    let mut c = std::process::Command::new("ffmpeg");
    c.args(["-y", "-loglevel", "error"]);
    if n.wav.metadata().is_ok_and(|m| m.len() > 64) {
        c.arg("-i").arg(&n.wav);
    } else {
        // no microphone: silent narration of the show's length (Keynote's muted recordings too)
        c.args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo", "-t", &format!("{duration:.3}")]);
    }
    c.args(["-c:a", "aac", "-b:a", "128k", "-ar", "48000", "-ac", "2", "-f", "mov"]).arg(&mov);
    let ok = c.stdin(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
    let bytes = if ok { std::fs::read(&mov).ok() } else { None };
    let _ = std::fs::remove_dir_all(n.wav.parent().unwrap_or(&n.wav));
    let Some(bytes) = bytes else {
        app.notify("Couldn't save the recording (ffmpeg is needed)");
        return;
    };
    let Some(doc) = app.doc.as_mut() else { return };
    doc.begin(&mut app.hist, "Record Slideshow");
    let r = doc.set_recording(&mut app.hist, &n.events, bytes, duration);
    doc.end(&mut app.hist);
    if std::env::var_os("KN_DEBUG").is_some() {
        eprintln!("recorded {duration:.2} s: {:?}", n.events);
    }
    // KN_SAVE_AS (automated checks): write the document after recording
    if let (Ok(out), Some(doc)) = (std::env::var("KN_SAVE_AS"), app.doc.as_mut()) {
        let _ = doc.save_as(&out);
    }
    match r {
        Ok(()) => app.notify(format!("Recorded {:.0} s; Play > Play Recorded Slideshow plays it back", duration)),
        Err(e) => app.notify(format!("Couldn't save the recording: {e}")),
    }
}

fn exit(app: &mut App, ctx: &egui::Context, in_window: bool, rec: Option<Recording>, narration: Option<Narration>) {
    if let Some(n) = narration {
        finish_narration(app, n);
    }
    if !in_window {
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
    }
    if let Some(r) = rec {
        stop_frame_clock();
        crate::export::finish_recording(app, r.dir, r.out, r.format, r.fps, r.frame);
    }
    app.play = None;
    let c = app.cur;
    app.go_to(c);
}
