//! The audio editors (Logic Pro 13): with an audio region selected, the bottom editor's tabs
//! are Track | File | Smart Tempo (research: interactions/UI-ANIMATIONS.txt "AUDIO EDITORS",
//! screenshots ui-audio-track-editor.png, ui-audio-file-editor.png, ui-smart-tempo-editor.png).
//!
//! - Track (Audio Track Editor): the region on the bar grid (ruler in bar / beat /
//!   division), the -100..100 % amplitude scale, the waveform; drag the lower edges to trim,
//!   the top corners for the fades (Control-Shift-drag in a fade for its curve), the body to
//!   move; Automation shows the region's gain line; Flex shows transients, dragged into flex
//!   markers (Flex Time). Pointer / Command-click tools as Logic's pop-ups; Snap.
//! - File (Audio File Editor): the whole file: overview strip with the visible area's box,
//!   sample-accurate ruler (Samples, Min:Sec:Ms, SMPTE, Bars/Beats), drag to select (Snap Edits
//!   to Zero Crossings), the destructive Functions with backup and undo, transients, the
//!   Anchor, Region and S. Loop lanes, prelisten.
//! - Smart Tempo (Smart Tempo Editor): Analyze, then the beat markers to drag, BAR / BEAT,
//!   TEMPO and SIGNATURE readouts, x2 and /2, Tempo: Constant / Variable, the overview; the
//!   project follows through Apply Region Tempo to Project Tempo (smarttempo.rs).
//!
//! What Studio keeps of an audio region beyond lg-core's Region (its offset into the file,
//! fades, gain, anchor, flex markers) is here by (track, region) until lg-core carries it.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Align2, Color32, Painter, Pos2, Rect, Sense, Stroke};
use lg_audio::fade::RegionFades;
use lg_audio::peaks::Overview;
use lg_audio::AudioFile;

use crate::app::App;
use crate::look;

const TABS_H: f32 = 36.0;
const LOCAL_H: f32 = 40.0;
const AMP_W: f32 = 64.0;
const FILE_AMP_W: f32 = 50.0;
const PPQ_F: f64 = lg_core::events::PPQ as f64;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Track,
    File,
    SmartTempo,
}

/// The Audio File Editor's ruler (its View menu).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Ruler {
    #[default]
    Samples,
    MinSec,
    Smpte,
    BarsBeats,
}

/// What Studio keeps of an audio region beyond lg-core's Region.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegionAudio {
    /// The file it was made for (a region index now on another file is a different region).
    pub file: PathBuf,
    /// Where the region starts in the file (frames).
    pub offset: usize,
    pub fades: RegionFades,
    /// Region gain (dB).
    pub gain_db: f32,
    /// The anchor: frames from the region's start in the file.
    pub anchor: usize,
    /// Flex markers: (source frame, destination frame), both from the region's start.
    pub flex: Vec<(usize, usize)>,
    /// Reverse (the region plays its audio backwards; the file is left as it is).
    pub reverse: bool,
    /// Flex Pitch: the region's notes with their edits (frames of the file; empty: unedited).
    pub pitch: Vec<lg_audio::flexpitch::Note>,
    /// Flex & Follow: the file's Smart Tempo beats follow the project tempo (flex.rs).
    pub follow: bool,
}

/// A decoded file: its samples at its own rate, the overview, its transients.
pub struct Loaded {
    pub audio: AudioFile,
    pub overview: Overview,
    pub transients: Option<Vec<usize>>,
    stamp: Option<std::time::SystemTime>,
    /// WAV / AIFF (written back by the destructive Functions); MP3, FLAC… only shown.
    pub editable: bool,
}

/// The Track editor's drags.
#[derive(Clone, Copy, Debug)]
enum TDrag {
    TrimStart { start: u32, length: u32, offset: usize },
    TrimEnd,
    FadeIn,
    FadeOut,
    Curve { out: bool, orig: i8, y0: f32 },
    Move { from: f64, start: u32 },
    Gain { orig: f32, y0: f32 },
    Marquee { from: f64 },
    Flex { src: usize, index: Option<usize> },
    Zoom { from: f64 },
}

/// The File editor's drags.
#[derive(Clone, Copy, Debug)]
enum FDrag {
    Select { from: usize },
    Overview,
    Anchor,
    RegionStart { offset: usize, start: u32 },
    RegionEnd,
    Transient { index: usize },
    Scroll { frame: f64, x: f32 },
}

/// The open sheets: Change Gain, Function Settings, Time and Pitch Machine.
#[derive(Clone, Debug)]
enum Dialog {
    ChangeGain { percent: f32, db: f32, relative: bool },
    Settings,
    TimePitch { tempo_pct: f32, cents: f32 },
}

pub struct AudioEdit {
    pub tab: Tab,
    files: HashMap<PathBuf, Loaded>,
    pub regions: HashMap<(usize, usize), RegionAudio>,
    snaps: Vec<(usize, HashMap<(usize, usize), RegionAudio>)>,
    redo_snaps: Vec<(usize, HashMap<(usize, usize), RegionAudio>)>,
    /// The region the views were last fitted to, the editor's width then, and whether it was
    /// zoomed since (else a new width fits again).
    fitted: Option<(usize, usize)>,
    fit_w: f32,
    zoomed: bool,
    // Audio Track Editor
    pub t_bar_w: f32,
    pub t_scroll: f64,
    pub tool: crate::edit::Tool,
    pub cmd_tool: crate::edit::Tool,
    pub snap_on: bool,
    pub snap_mode: usize,
    pub show_automation: bool,
    pub show_flex: bool,
    pub flex_mode: usize,
    pub catch: bool,
    pub wave_zoom: f32,
    t_drag: Option<TDrag>,
    /// The Marquee tool's range (ticks).
    pub marquee: Option<(f64, f64)>,
    // Audio File Editor
    pub f_start: f64,
    pub f_per_px: f64,
    pub sel: Option<(usize, usize)>,
    pub ruler: Ruler,
    pub amp_percent: bool,
    pub transient_mode: bool,
    pub sensitivity: f32,
    pub zero_cross: bool,
    pub lock_anchor: bool,
    pub sample_loop: Option<(usize, usize)>,
    pub file_tool: usize,
    pub prelisten_cycle: bool,
    pub prelisten_vol: f32,
    pub f_scroll_in_play: bool,
    pub f_amp_zoom: f32,
    f_drag: Option<FDrag>,
    file_undo: Vec<(PathBuf, AudioFile, String)>,
    file_redo: Vec<(PathBuf, AudioFile, String)>,
    dialog: Option<Dialog>,
    /// Function Settings: Normalize to (dB), the fade curve, Search Silence's threshold (dB).
    pub normalize_db: f32,
    pub fade_curve: i8,
    pub silence_db: f32,
    pub clipboard: Option<AudioFile>,
    /// Smart Tempo.
    pub st: crate::smarttempo::State,
    ctx: Option<egui::Context>,
}

impl Default for AudioEdit {
    fn default() -> Self {
        // (snapshots: LG_AUDIO_TAB=track|file|tempo)
        let tab = match std::env::var("LG_AUDIO_TAB").as_deref() {
            Ok("file") => Tab::File,
            Ok("tempo") => Tab::SmartTempo,
            _ => Tab::Track,
        };
        AudioEdit {
            tab,
            files: HashMap::new(),
            regions: HashMap::new(),
            snaps: Vec::new(),
            redo_snaps: Vec::new(),
            fitted: None,
            fit_w: 0.0,
            zoomed: false,
            t_bar_w: 240.0,
            t_scroll: 0.0,
            tool: crate::edit::Tool::Pointer,
            cmd_tool: crate::edit::Tool::Marquee,
            snap_on: true,
            snap_mode: 0,
            show_automation: false,
            show_flex: false,
            flex_mode: 1,
            catch: false,
            wave_zoom: 1.0,
            t_drag: None,
            marquee: None,
            f_start: 0.0,
            f_per_px: 100.0,
            sel: None,
            ruler: Ruler::Samples,
            amp_percent: true,
            transient_mode: false,
            sensitivity: 0.5,
            zero_cross: true,
            lock_anchor: true,
            sample_loop: None,
            file_tool: 0,
            prelisten_cycle: false,
            prelisten_vol: 0.75,
            f_scroll_in_play: false,
            f_amp_zoom: 1.0,
            f_drag: None,
            file_undo: Vec::new(),
            file_redo: Vec::new(),
            dialog: None,
            normalize_db: 0.0,
            fade_curve: 0,
            silence_db: -60.0,
            clipboard: None,
            st: Default::default(),
            ctx: None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// colours (measured on the screenshots, 2x)

fn pick(d: u32, l: u32) -> Color32 {
    kn_ui::theme::rgb(if kn_ui::theme::is_light() { l } else { d })
}
fn editor_bg() -> Color32 {
    pick(0x262626, 0xececee)
}
fn ruler_bg() -> Color32 {
    pick(0x303031, 0xdedee0)
}
fn amp_bg() -> Color32 {
    pick(0x141414, 0xe2e2e4)
}
fn track_body() -> Color32 {
    pick(0x2c405d, 0xc3d6ef)
}
fn track_header() -> Color32 {
    Color32::from_rgb(0xcf, 0xe3, 0xfd)
}
fn track_wave() -> Color32 {
    pick(0x9cbde8, 0x3b6cb0)
}
fn file_bg() -> Color32 {
    pick(0x373737, 0xf2f2f2)
}
fn file_wave() -> Color32 {
    pick(0xf4f4f4, 0x2e2e2e)
}
fn overview_bg() -> Color32 {
    pick(0x464545, 0xd2d2d4)
}
fn file_ruler_bg() -> Color32 {
    pick(0x141514, 0xe0e0e2)
}
fn lane_bg() -> Color32 {
    pick(0x1b1b1c, 0xe6e6e8)
}
fn lane_label_bg() -> Color32 {
    pick(0x262627, 0xdadadc)
}
fn region_lane() -> Color32 {
    Color32::from_rgb(0x52, 0x7c, 0xb9)
}
fn st_bg() -> Color32 {
    pick(0x19232f, 0xdde6f2)
}
fn st_wave() -> Color32 {
    pick(0x3f6190, 0x6286bd)
}
fn st_grid() -> Color32 {
    pick(0x2a3a50, 0xb6c6dc)
}
fn st_downbeat() -> Color32 {
    pick(0x4e6788, 0x8ea6c6)
}
fn orange() -> Color32 {
    Color32::from_rgb(0xe0, 0x84, 0x30)
}
fn scale_text() -> Color32 {
    pick(0xb4b4b6, 0x505052)
}

// ---------------------------------------------------------------------------------------------
// region data

/// The extras kept for a region (defaults: the file from its start, no fades).
pub fn extras(app: &App, t: usize, r: usize) -> RegionAudio {
    let file = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)?.audio.as_ref().map(|a| a.file.clone())).unwrap_or_default();
    app.audio_edit.regions.get(&(t, r)).filter(|x| x.file == file).cloned().unwrap_or(RegionAudio { file, ..Default::default() })
}

pub fn set_extras(app: &mut App, t: usize, r: usize, x: RegionAudio) {
    app.audio_edit.regions.insert((t, r), x);
}

/// Before an edit of the project or the extras: one undo step for both.
pub fn checkpoint(app: &mut App, what: &str) {
    let before = app.audio_edit.regions.clone();
    app.checkpoint(what);
    let n = app.undo.len();
    app.audio_edit.snaps.push((n, before));
    app.audio_edit.redo_snaps.clear();
}

/// The extras follow the app's Undo / Redo.
fn sync_undo(app: &mut App) {
    let n = app.undo.len();
    if app.redo.is_empty() {
        app.audio_edit.redo_snaps.clear();
    }
    while app.audio_edit.snaps.last().is_some_and(|s| s.0 > n) {
        let (k, before) = app.audio_edit.snaps.pop().unwrap();
        let now = std::mem::replace(&mut app.audio_edit.regions, before);
        app.audio_edit.redo_snaps.push((k, now));
    }
    while app.audio_edit.redo_snaps.last().is_some_and(|s| s.0 <= n) {
        let (k, after) = app.audio_edit.redo_snaps.pop().unwrap();
        let now = std::mem::replace(&mut app.audio_edit.regions, after);
        app.audio_edit.snaps.push((k, now));
    }
}

/// The file decoded (cached; read again when it changed on disk).
pub(crate) fn load<'a>(app: &'a mut App, path: &Path) -> Option<&'a Loaded> {
    let stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let fresh = app.audio_edit.files.get(path).is_some_and(|l| l.stamp == stamp);
    if !fresh {
        let (audio, editable) = match AudioFile::read(path) {
            Ok(a) => (a, true),
            Err(_) => (AudioFile::new(crate::audio::RATE, 2, crate::audio::load_audio(path)?), false),
        };
        let overview = Overview::new(&audio.data, audio.channels);
        app.audio_edit.files.insert(path.to_path_buf(), Loaded { audio, overview, transients: None, stamp, editable });
    }
    app.audio_edit.files.get(path)
}

/// A file's sample rate (as decoded).
pub fn rate_of(app: &App, path: &Path) -> Option<u32> {
    app.audio_edit.files.get(path).map(|l| l.audio.rate)
}

pub fn loaded_seconds(app: &App, path: &Path) -> Option<f64> {
    app.audio_edit.files.get(path).map(|l| l.audio.seconds())
}

fn transients(app: &mut App, path: &Path) -> Vec<usize> {
    let sens = app.audio_edit.sensitivity;
    let Some(l) = app.audio_edit.files.get_mut(path) else { return Vec::new() };
    if l.transients.is_none() {
        l.transients = Some(lg_audio::transients::detect(&l.audio.data, l.audio.channels, l.audio.rate, sens));
    }
    l.transients.clone().unwrap_or_default()
}

/// The selected region if it's an audio region: (track, region, the region, its file).
fn selected(app: &App) -> Option<(usize, usize, lg_core::Region, PathBuf)> {
    let (t, r) = app.selected_region?;
    let rg = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?.clone();
    let f = rg.audio.as_ref()?.file.clone();
    Some((t, r, rg, f))
}

/// True when the selected region is an audio region (the bottom editor shows these tabs).
pub fn is_audio_region(app: &App) -> bool {
    selected(app).is_some()
}

/// The region's audio span in its file (frames): start, end.
fn region_frames(app: &App, rg: &lg_core::Region, x: &RegionAudio, rate: u32) -> (usize, usize) {
    let secs = app.seconds_at((rg.start + rg.length) as f64) - app.seconds_at(rg.start as f64);
    let src = flex_src(&x.flex, secs * rate as f64);
    (x.offset, x.offset + src.max(0.0) as usize)
}

/// A flex-warped region's source frame for an output frame (both from the region's start).
fn flex_src(flex: &[(usize, usize)], dst: f64) -> f64 {
    let mut prev = (0.0, 0.0);
    for &(s, d) in flex {
        let (s, d) = (s as f64, d as f64);
        if dst <= d {
            return if d > prev.1 { prev.0 + (dst - prev.1) * (s - prev.0) / (d - prev.1) } else { s };
        }
        prev = (s, d);
    }
    prev.0 + (dst - prev.1)
}

fn flex_dst(flex: &[(usize, usize)], src: f64) -> f64 {
    let mut prev = (0.0, 0.0);
    for &(s, d) in flex {
        let (s, d) = (s as f64, d as f64);
        if src <= s {
            return if s > prev.0 { prev.1 + (src - prev.0) * (d - prev.1) / (s - prev.0) } else { d };
        }
        prev = (s, d);
    }
    prev.1 + (src - prev.0)
}

/// Sets a region's span to file seconds `s..e` (Trim Region To Selection; the region keeps its
/// place in time for the audio it plays).
pub fn set_region_span(app: &mut App, t: usize, r: usize, s: f64, e: f64) -> bool {
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return false };
    let Some(file) = rg.audio.as_ref().map(|a| a.file.clone()) else { return false };
    let Some(rate) = load(app, &file).map(|l| l.audio.rate) else { return false };
    let mut x = extras(app, t, r);
    let old_off = x.offset as f64 / rate as f64;
    let start_sec = app.seconds_at(rg.start as f64) + (s - old_off);
    let start = crate::smarttempo::tick_at(app, start_sec.max(0.0));
    let end = crate::smarttempo::tick_at(app, start_sec.max(0.0) + (e - s));
    checkpoint(app, "Trim Region");
    x.offset = (s.max(0.0) * rate as f64) as usize;
    set_extras(app, t, r, x);
    if let Some(g) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
        g.start = start.round() as u32;
        g.length = (end - start).round().max(1.0) as u32;
    }
    app.after_edit();
    true
}

// ---------------------------------------------------------------------------------------------
// playback

/// How the engine plays an audio region (the file's part, gain, fades, flex), made on the
/// engine's loading thread: `Plan::render` gives stereo samples at the engine's rate.
#[derive(Clone, Debug)]
pub struct Plan {
    pub file: PathBuf,
    pub offset: usize,
    /// The region's length in seconds (as it plays).
    pub seconds: f64,
    pub gain_db: f32,
    pub fades: RegionFades,
    pub flex: Vec<(usize, usize)>,
    pub reverse: bool,
    /// The track's Flex mode, and Flex Pitch's edited notes (frames of the file).
    pub mode: lg_audio::flex::Mode,
    pub notes: Vec<lg_audio::flexpitch::Note>,
}

impl Plan {
    pub fn render(&self) -> Option<Vec<f32>> {
        crate::flex::cached(self, || self.render_now())
    }

    fn render_now(&self) -> Option<Vec<f32>> {
        let a = match AudioFile::read(&self.file) {
            Ok(a) => a,
            Err(_) => AudioFile::new(crate::audio::RATE, 2, crate::audio::load_audio(&self.file)?),
        };
        let rate = a.rate as f64;
        let out_frames = (self.seconds * rate).round() as usize;
        let src_frames = flex_src(&self.flex, out_frames as f64).ceil() as usize;
        let mut part = a.slice(self.offset..self.offset + src_frames);
        if !self.notes.is_empty() {
            // Flex Pitch: the notes' edits on this part (its own pitch track)
            let c = part.channels.max(1);
            let mono: Vec<f32> = part.data.chunks(c).map(|f| f.iter().sum::<f32>() / c as f32).collect();
            let tr = lg_audio::flexpitch::track(&mono, part.rate);
            let notes: Vec<lg_audio::flexpitch::Note> = self.notes.iter().filter(|n| n.end > self.offset).map(|n| {
                let mut n = n.clone();
                n.start = n.start.saturating_sub(self.offset);
                n.end -= self.offset;
                n
            }).collect();
            part.data = lg_audio::flexpitch::render(&part.data, c, &tr, &notes);
        }
        if !self.flex.is_empty() {
            let trs = match self.mode {
                lg_audio::flex::Mode::Slicing | lg_audio::flex::Mode::Rhythmic | lg_audio::flex::Mode::Polyphonic => lg_audio::transients::detect(&part.data, part.channels, part.rate, 0.5),
                _ => Vec::new(),
            };
            part.data = lg_audio::flex::render(&part.data, part.channels, part.rate, &self.flex, out_frames, self.mode, &trs);
        }
        part.data.resize(out_frames.min(part.data.len() / part.channels) * part.channels, 0.0);
        if self.reverse {
            let c = part.channels.max(1);
            let frames: Vec<Vec<f32>> = part.data.chunks(c).rev().map(|f| f.to_vec()).collect();
            part.data = frames.concat();
        }
        let g = 10f32.powf(self.gain_db / 20.0);
        if g != 1.0 {
            part.data.iter_mut().for_each(|s| *s *= g);
        }
        self.fades.apply(&mut part.data, part.channels, part.rate);
        Some(part.stereo_48k())
    }
}

/// Every audio region the engine plays: (track, start tick, how).
pub fn playback(app: &App) -> Vec<(usize, u32, Plan)> {
    let Some(p) = &app.project else { return Vec::new() };
    let mut out = Vec::new();
    for (t, tr) in p.tracks.iter().enumerate() {
        for (r, rg) in tr.regions.iter().enumerate() {
            let Some(a) = &rg.audio else { continue };
            if rg.muted {
                continue;
            }
            let x = extras(app, t, r);
            let seconds = app.seconds_at((rg.start + rg.length) as f64) - app.seconds_at(rg.start as f64);
            let (mode, flex, notes) = crate::flex::plan_for(app, t, rg, &x);
            out.push((t, rg.start, Plan { file: a.file.clone(), offset: x.offset, seconds, gain_db: x.gain_db, fades: x.fades, flex, reverse: x.reverse, mode, notes }));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// the editor

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    app.audio_edit.ctx = Some(ui.ctx().clone());
    sync_undo(app);
    crate::smarttempo::poll(app);
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, editor_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    let Some((t, r, rg, file)) = selected(app) else {
        p.text(area.center(), Align2::CENTER_CENTER, "Select an audio region", look::regular(14.0), look::text_dim());
        return;
    };
    // the tabs: Track | File | Smart Tempo
    let tabs = Rect::from_min_size(area.min, vec2(area.width(), TABS_H));
    let names = ["Track", "File", "Smart Tempo"];
    let tw = 110.0;
    let tx0 = tabs.center().x - tw * 1.5;
    let pill = Rect::from_min_size(pos2(tx0, tabs.min.y + 6.0), vec2(tw * 3.0, 24.0));
    p.rect_filled(pill, 7.0, look::group_bg());
    let cur = app.audio_edit.tab as usize;
    for (i, n) in names.iter().enumerate() {
        let r = Rect::from_min_size(pos2(tx0 + i as f32 * tw, pill.min.y), vec2(tw, 24.0));
        if i == cur {
            p.rect_filled(r.shrink(1.0), 6.0, look::accent());
        } else if i > 0 && i - 1 != cur {
            p.vline(r.min.x, r.y_range().shrink(6.0), Stroke::new(1.0, look::line()));
        }
        p.text(r.center(), Align2::CENTER_CENTER, *n, look::regular(13.0), if i == cur { Color32::WHITE } else { look::text() });
        if ui.interact(r, ui.scope_id().with(("aetab", i)), Sense::click()).clicked() {
            app.audio_edit.tab = [Tab::Track, Tab::File, Tab::SmartTempo][i];
        }
    }
    let body = Rect::from_min_max(pos2(area.min.x, tabs.max.y), area.max);
    let Some(rate) = load(app, &file).map(|l| l.audio.rate) else {
        p.text(body.center(), Align2::CENTER_CENTER, format!("Can't read {}", file.display()), look::regular(13.0), look::text_dim());
        return;
    };
    let _ = rate;
    // a new region, or the editor resized while not zoomed: the views fit again
    if app.audio_edit.fitted != Some((t, r)) || (!app.audio_edit.zoomed && (body.width() - app.audio_edit.fit_w).abs() > 1.0) {
        app.audio_edit.fitted = None;
        app.audio_edit.st.fitted_for = None;
        app.audio_edit.zoomed = false;
        app.audio_edit.fit_w = body.width();
    }
    match app.audio_edit.tab {
        Tab::Track => track_editor(app, ui, body, t, r, &rg, &file),
        Tab::File => file_editor(app, ui, body, t, r, &rg, &file),
        Tab::SmartTempo => tempo_editor(app, ui, body, t, r, &rg, &file),
    }
    dialogs(app, ui.ctx(), &file);
    app.audio_edit.fitted = Some((t, r));
}

// ---------------------------------------------------------------------------------------------
// small controls

/// A local-bar button: a rounded capsule with a drawn glyph (accent when on).
fn icon_button(ui: &mut egui::Ui, r: Rect, id: egui::Id, on: bool, tip: &str, glyph: impl FnOnce(&Painter, Rect, Color32)) -> egui::Response {
    let resp = ui.interact(r, id, Sense::click()).on_hover_text(tip);
    let p = ui.painter();
    let fill = if on { look::accent() } else if resp.hovered() { look::button_on() } else { look::group_bg() };
    p.rect_filled(r, 6.0, fill);
    glyph(p, r, if on { Color32::WHITE } else { look::text() });
    resp
}

fn chevron(p: &Painter, at: Pos2, c: Color32) {
    p.line_segment([at + vec2(-3.5, -1.5), at + vec2(0.0, 2.0)], Stroke::new(1.3, c));
    p.line_segment([at + vec2(0.0, 2.0), at + vec2(3.5, -1.5)], Stroke::new(1.3, c));
}

fn updown(p: &Painter, at: Pos2, c: Color32) {
    for s in [-1.0f32, 1.0] {
        p.line_segment([at + vec2(-3.0, s * 1.5), at + vec2(0.0, s * 4.5)], Stroke::new(1.2, c));
        p.line_segment([at + vec2(0.0, s * 4.5), at + vec2(3.0, s * 1.5)], Stroke::new(1.2, c));
    }
}

fn arrow_glyph(p: &Painter, c: Pos2, col: Color32) {
    let pts = [c + vec2(-3.0, -7.0), c + vec2(-3.0, 5.0), c + vec2(0.0, 2.0), c + vec2(2.5, 7.0), c + vec2(4.0, 6.0), c + vec2(1.5, 1.0), c + vec2(5.5, 1.0)];
    p.add(egui::Shape::convex_polygon(pts.to_vec(), col, Stroke::NONE));
}

fn tool_glyph(p: &Painter, c: Pos2, tool: crate::edit::Tool, col: Color32) {
    use crate::edit::Tool;
    let s = Stroke::new(1.5, col);
    match tool {
        Tool::Pointer => arrow_glyph(p, c, col),
        Tool::Marquee => {
            p.hline(c.x - 6.0..=c.x + 6.0, c.y, s);
            p.vline(c.x, c.y - 6.0..=c.y + 6.0, s);
        }
        Tool::Pencil => {
            p.line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)], Stroke::new(2.5, col));
        }
        Tool::Eraser => {
            p.rect_stroke(Rect::from_center_size(c, vec2(10.0, 7.0)), 1.5, s, egui::StrokeKind::Middle);
        }
        Tool::Scissors => {
            p.circle_stroke(c + vec2(-3.0, 4.0), 2.2, s);
            p.circle_stroke(c + vec2(3.0, 4.0), 2.2, s);
            p.line_segment([c + vec2(-2.0, 2.0), c + vec2(3.0, -6.0)], s);
            p.line_segment([c + vec2(2.0, 2.0), c + vec2(-3.0, -6.0)], s);
        }
        Tool::Fade => {
            p.line_segment([c + vec2(-6.0, 5.0), c + vec2(6.0, -5.0)], s);
            p.hline(c.x - 6.0..=c.x + 6.0, c.y + 5.0, s);
        }
        Tool::Flex => flex_glyph(p, c, col),
        Tool::Zoom => {
            p.circle_stroke(c + vec2(-1.0, -1.0), 4.5, s);
            p.line_segment([c + vec2(2.5, 2.5), c + vec2(6.0, 6.0)], Stroke::new(2.0, col));
        }
        Tool::Mute => {
            p.text(c, Align2::CENTER_CENTER, "M", look::semibold(11.0), col);
        }
        Tool::Solo => {
            p.text(c, Align2::CENTER_CENTER, "S", look::semibold(11.0), col);
        }
        Tool::Text => {
            p.text(c, Align2::CENTER_CENTER, "T", look::semibold(11.0), col);
        }
        Tool::Glue => {
            p.rect_stroke(Rect::from_center_size(c, vec2(6.0, 11.0)), 2.0, s, egui::StrokeKind::Middle);
        }
        _ => arrow_glyph(p, c, col),
    }
}

/// Logic's Flex icon (an hourglass on its side).
fn flex_glyph(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.4, col);
    p.line_segment([c + vec2(-7.0, -6.0), c + vec2(7.0, 6.0)], s);
    p.line_segment([c + vec2(-7.0, 6.0), c + vec2(7.0, -6.0)], s);
    p.vline(c.x - 7.0, c.y - 6.0..=c.y + 6.0, s);
    p.vline(c.x + 7.0, c.y - 6.0..=c.y + 6.0, s);
}

fn automation_glyph(p: &Painter, c: Pos2, col: Color32) {
    let pts = vec![c + vec2(-8.0, 5.0), c + vec2(-3.0, 5.0), c + vec2(1.0, -4.0), c + vec2(8.0, -4.0)];
    p.add(egui::Shape::line(pts, Stroke::new(1.5, col)));
    p.circle_filled(c + vec2(-3.0, 5.0), 1.8, col);
    p.circle_filled(c + vec2(1.0, -4.0), 1.8, col);
}

fn catch_glyph(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.4, col);
    p.vline(c.x, c.y - 7.0..=c.y + 7.0, s);
    for d in [-1.0f32, 1.0] {
        p.line_segment([c + vec2(d * 2.0, 0.0), c + vec2(d * 8.0, 0.0)], s);
        p.line_segment([c + vec2(d * 8.0, 0.0), c + vec2(d * 5.0, -3.0)], s);
        p.line_segment([c + vec2(d * 8.0, 0.0), c + vec2(d * 5.0, 3.0)], s);
    }
}

fn speaker_glyph(p: &Painter, c: Pos2, col: Color32, waves: usize) {
    let pts = vec![c + vec2(-7.0, -2.5), c + vec2(-4.0, -2.5), c + vec2(0.0, -6.0), c + vec2(0.0, 6.0), c + vec2(-4.0, 2.5), c + vec2(-7.0, 2.5)];
    p.add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
    for w in 0..waves {
        let rr = 4.0 + w as f32 * 3.0;
        let arc: Vec<Pos2> = (0..=8).map(|k| {
            let a = -0.8 + 1.6 * k as f32 / 8.0;
            c + vec2(a.cos() * rr, a.sin() * rr)
        }).collect();
        p.add(egui::Shape::line(arc, Stroke::new(1.3, col)));
    }
}

fn loop_glyph(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.5, col);
    let r = Rect::from_center_size(c, vec2(15.0, 9.0));
    p.rect_stroke(r, 4.0, s, egui::StrokeKind::Middle);
    p.line_segment([pos2(r.max.x - 5.0, r.min.y - 3.0), pos2(r.max.x - 2.0, r.min.y)], s);
    p.line_segment([pos2(r.min.x + 5.0, r.max.y + 3.0), pos2(r.min.x + 2.0, r.max.y)], s);
}

fn wave_glyph(p: &Painter, c: Pos2, col: Color32) {
    for (i, h) in [3.0, 6.0, 9.0, 5.0, 8.0, 4.0, 2.0].iter().enumerate() {
        p.vline(c.x - 9.0 + i as f32 * 3.0, c.y - h..=c.y + h, Stroke::new(1.3, col));
    }
}

fn fit_glyph(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.4, col);
    p.vline(c.x - 8.0, c.y - 6.0..=c.y + 6.0, s);
    p.vline(c.x + 8.0, c.y - 6.0..=c.y + 6.0, s);
    p.hline(c.x - 6.0..=c.x + 6.0, c.y, s);
    for d in [-1.0f32, 1.0] {
        p.line_segment([c + vec2(d * 6.0, 0.0), c + vec2(d * 3.5, -2.5)], s);
        p.line_segment([c + vec2(d * 6.0, 0.0), c + vec2(d * 3.5, 2.5)], s);
    }
}

fn hand_glyph(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.3, col);
    p.rect_stroke(Rect::from_min_size(c + vec2(-5.0, -1.0), vec2(10.0, 8.0)), 3.0, s, egui::StrokeKind::Middle);
    for i in 0..4 {
        let x = c.x - 3.8 + i as f32 * 2.6;
        p.vline(x, c.y - 6.0 + (i as f32 - 1.5).abs()..=c.y, s);
    }
}

fn metronome_glyph(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.4, col);
    let pts = vec![c + vec2(-6.0, 7.0), c + vec2(-2.5, -7.0), c + vec2(2.5, -7.0), c + vec2(6.0, 7.0), c + vec2(-6.0, 7.0)];
    p.add(egui::Shape::line(pts, s));
    p.line_segment([c + vec2(0.0, 4.0), c + vec2(5.0, -4.0)], s);
}

fn power_glyph(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.5, col);
    let arc: Vec<Pos2> = (0..=14).map(|k| {
        let a = -std::f32::consts::FRAC_PI_2 + 0.6 + (2.0 * std::f32::consts::PI - 1.2) * k as f32 / 14.0;
        c + vec2(a.cos() * 6.0, a.sin() * 6.0)
    }).collect();
    p.add(egui::Shape::line(arc, s));
    p.vline(c.x, c.y - 7.5..=c.y - 1.0, s);
}

/// A small horizontal slider with a glyph before it (Logic's zoom and volume sliders).
fn mini_slider(ui: &mut egui::Ui, r: Rect, id: egui::Id, v: &mut f32, glyph: &str) -> bool {
    let p = ui.painter().clone();
    p.text(pos2(r.min.x + 8.0, r.center().y), Align2::CENTER_CENTER, glyph, look::regular(13.0), look::text());
    let track = Rect::from_min_max(pos2(r.min.x + 22.0, r.center().y - 2.0), pos2(r.max.x - 4.0, r.center().y + 2.0));
    let resp = ui.interact(track.expand2(vec2(6.0, 8.0)), id, Sense::click_and_drag());
    let mut changed = false;
    if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
        let nv = ((pp.x - track.min.x) / track.width()).clamp(0.0, 1.0);
        changed = nv != *v;
        *v = nv;
    }
    p.rect_filled(track, 2.0, look::lcd_bg());
    p.rect_filled(Rect::from_min_max(track.min, pos2(track.min.x + track.width() * *v, track.max.y)), 2.0, look::text_dim());
    p.circle_filled(pos2(track.min.x + track.width() * *v, track.center().y), 7.0, Color32::from_gray(0xe8));
    changed
}

/// A tool pop-up: the tool's glyph and a chevron; the menu lists the tools.
fn tool_popup(ui: &mut egui::Ui, r: Rect, id: egui::Id, tool: crate::edit::Tool, tools: &[crate::edit::Tool], tip: &str) -> Option<crate::edit::Tool> {
    let resp = icon_button(ui, r, id, false, tip, |p, r, c| {
        tool_glyph(p, pos2(r.min.x + 13.0, r.center().y), tool, c);
        chevron(p, pos2(r.max.x - 9.0, r.center().y), c);
    });
    let mut out = None;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(180.0);
        for &t in tools {
            if ui.add(egui::Button::selectable(t == tool, t.name())).clicked() {
                out = Some(t);
                ui.close();
            }
        }
    });
    out
}

fn tick_label(tick: f64, sig: (u8, u8), level: u8) -> String {
    let t = tick.max(0.0) as u32;
    let beat_t = lg_core::events::PPQ * 4 / sig.1.max(1) as u32;
    let bar_t = beat_t * sig.0.max(1) as u32;
    let div_t = lg_core::events::PPQ / 4;
    let bar = t / bar_t + 1;
    let beat = (t % bar_t) / beat_t + 1;
    let div = (t % beat_t) / div_t + 1;
    match level {
        0 => format!("{bar}"),
        1 => format!("{bar} {beat}"),
        _ => format!("{bar} {beat} {div}"),
    }
}

/// Labels on a vertical amplitude scale: -100..100 % (or sample values) every `step`.
fn amp_scale(p: &Painter, col: Rect, lane: Rect, step: f32, percent: bool, zoom: f32) {
    let mut v = -100.0;
    while v <= 100.0 + 0.01 {
        let y = lane.center().y - v / 100.0 * lane.height() / 2.0 * zoom;
        if y >= lane.min.y - 0.5 && y <= lane.max.y + 0.5 {
            let label = if percent { format!("{v:.0}") } else { format!("{:.0}", v / 100.0 * 32768.0) };
            p.text(pos2(col.max.x - 9.0, y), Align2::RIGHT_CENTER, label, look::regular(10.5), scale_text());
            p.hline(col.max.x - 6.0..=col.max.x - 2.0, y, Stroke::new(1.0, scale_text()));
        }
        v += step;
    }
}

// ---------------------------------------------------------------------------------------------
// Audio Track Editor

const SNAPS: [&str; 7] = ["Smart", "Bar", "Beat", "Division", "Ticks", "Frames", "Samples"];
const TRACK_TOOLS: [crate::edit::Tool; 12] = {
    use crate::edit::Tool::*;
    [Pointer, Pencil, Eraser, Text, Scissors, Glue, Solo, Mute, Zoom, Fade, Marquee, Flex]
};

fn snap_tick(app: &App, tick: f64, px_per_tick: f64) -> f64 {
    let ae = &app.audio_edit;
    if !ae.snap_on {
        return tick;
    }
    let bar = app.bar_ticks() as f64;
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let beat = PPQ_F * 4.0 / sig.1.max(1) as f64;
    let grid = match SNAPS[ae.snap_mode.min(SNAPS.len() - 1)] {
        "Bar" => bar,
        "Beat" => beat,
        "Division" => PPQ_F / 4.0,
        "Ticks" => 1.0,
        "Frames" => PPQ_F * app.tempo() / 60.0 / 30.0,
        "Samples" => return tick,
        // Smart: the finest grid line at least 8 points apart
        _ => {
            let mut g = bar;
            for c in [beat, PPQ_F / 4.0, PPQ_F / 16.0, 1.0] {
                if c * px_per_tick >= 8.0 {
                    g = c;
                }
            }
            g
        }
    };
    (tick / grid).round() * grid
}

fn track_editor(app: &mut App, ui: &mut egui::Ui, body: Rect, t: usize, r: usize, rg: &lg_core::Region, file: &Path) {
    let p = ui.painter_at(body);
    let x = extras(app, t, r);
    let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
    let bar_t = app.bar_ticks() as f64;
    // fit the region when it's new to the editor
    let main = Rect::from_min_max(pos2(body.min.x + AMP_W, body.min.y + LOCAL_H), body.max);
    if app.audio_edit.fitted != Some((t, r)) {
        let w = main.width().max(100.0) as f64;
        let span = (rg.length as f64).max(bar_t / 4.0) * 1.08;
        app.audio_edit.t_bar_w = (w / span * bar_t) as f32;
        app.audio_edit.t_scroll = (rg.start as f64 - span * 0.01).max(0.0);
    }
    // ---- the local bar
    let local = Rect::from_min_size(body.min, vec2(body.width(), LOCAL_H));
    ui.scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_size(local.min + vec2(10.0, 0.0), vec2(320.0, LOCAL_H))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        crate::menus::local_bar(app, ui, &[("Edit", "audiotrack-Edit"), ("Functions", "audiotrack-Functions"), ("View", "audiotrack-View")]);
    });
    let mut bx = local.min.x + 300.0;
    let cy = local.center().y;
    let b = |x: f32, w: f32| Rect::from_min_size(pos2(x, cy - 12.0), vec2(w, 24.0));
    let ae_id = ui.scope_id().with("ate");
    if icon_button(ui, b(bx, 36.0), ae_id.with("auto"), app.audio_edit.show_automation, "Show/Hide Automation", |p, r, c| automation_glyph(p, r.center(), c)).clicked() {
        app.audio_edit.show_automation = !app.audio_edit.show_automation;
    }
    bx += 38.0;
    if icon_button(ui, b(bx, 36.0), ae_id.with("flex"), app.audio_edit.show_flex, "Show/Hide Flex", |p, r, c| flex_glyph(p, r.center(), c)).clicked() {
        app.audio_edit.show_flex = !app.audio_edit.show_flex;
    }
    bx += 44.0;
    if app.audio_edit.show_flex {
        // the track's Flex mode (flex.rs), then Follow Tempo / Pitch Correction
        let fr = b(bx, 128.0);
        let names: Vec<&str> = lg_audio::flex::Mode::ALL.iter().map(|m| m.name()).collect();
        let cur = crate::flex::track(app, t).mode;
        p.rect_filled(fr, 6.0, look::group_bg());
        let k = lg_audio::flex::Mode::ALL.iter().position(|m| *m == cur).unwrap_or(0);
        if let Some(k) = crate::widgets::choice_slot(ui, fr, ae_id.with("fmode"), k, &names) {
            crate::flex::set_mode(app, t, lg_audio::flex::Mode::ALL[k]);
        }
        bx += 136.0;
        // (what room the tools and the right-hand controls leave)
        bx = crate::flex::editor_bar(app, ui, pos2(bx, cy), local.width() - 980.0, t, r) + 6.0;
    }
    if icon_button(ui, b(bx, 36.0), ae_id.with("catch"), app.audio_edit.catch, "Catch Playhead", |p, r, c| catch_glyph(p, r.center(), c)).clicked() {
        app.audio_edit.catch = !app.audio_edit.catch;
    }
    // the tools, centred
    let tcx = local.center().x.max(bx + 140.0);
    if let Some(tl) = tool_popup(ui, b(tcx - 64.0, 56.0), ae_id.with("tool"), app.audio_edit.tool, &TRACK_TOOLS, "Left-click Tool") {
        app.audio_edit.tool = tl;
    }
    if let Some(tl) = tool_popup(ui, b(tcx + 0.0, 56.0), ae_id.with("cmdtool"), app.audio_edit.cmd_tool, &TRACK_TOOLS, "Command-click Tool") {
        app.audio_edit.cmd_tool = tl;
    }
    // Snap, the waveform zoom, fit, the horizontal zoom (at the right)
    let mut rx = local.max.x - 10.0;
    let zr = Rect::from_min_size(pos2(rx - 96.0, cy - 12.0), vec2(96.0, 24.0));
    let mut zv = ((app.audio_edit.t_bar_w / 20.0).ln() / (4000.0f32 / 20.0).ln()).clamp(0.0, 1.0);
    if mini_slider(ui, zr, ae_id.with("hz"), &mut zv, "↔") {
        app.audio_edit.zoomed = true;
        let centre = app.audio_edit.t_scroll + main.width() as f64 / app.audio_edit.t_bar_w as f64 * bar_t / 2.0;
        app.audio_edit.t_bar_w = 20.0 * (4000.0f32 / 20.0).powf(zv);
        app.audio_edit.t_scroll = (centre - main.width() as f64 / app.audio_edit.t_bar_w as f64 * bar_t / 2.0).max(0.0);
    }
    rx = zr.min.x - 8.0;
    if icon_button(ui, b(rx - 36.0, 36.0), ae_id.with("fit"), false, "Zoom to Fit Region", |p, r, c| fit_glyph(p, r.center(), c)).clicked() {
        app.audio_edit.fitted = None;
    }
    rx -= 40.0;
    let wz = icon_button(ui, b(rx - 36.0, 36.0), ae_id.with("wz"), app.audio_edit.wave_zoom > 1.0, "Waveform Zoom (drag up or down; click: 100 %)", |p, r, c| wave_glyph(p, r.center(), c));
    if wz.dragged() {
        app.audio_edit.wave_zoom = (app.audio_edit.wave_zoom * (1.0 - wz.drag_delta().y * 0.02)).clamp(1.0, 32.0);
    } else if wz.clicked() {
        app.audio_edit.wave_zoom = 1.0;
    }
    rx -= 44.0;
    // Snap: ⏻ [Smart ⇕] (left out when the editor is too narrow for it)
    if rx - 230.0 > tcx + 70.0 {
        let sr = b(rx - 120.0, 120.0);
        let sresp = ui.interact(sr, ae_id.with("snapm"), Sense::click());
        p.rect_filled(sr, 6.0, if sresp.hovered() { look::button_on() } else { look::group_bg() });
        p.text(pos2(sr.min.x + 10.0, cy), Align2::LEFT_CENTER, SNAPS[app.audio_edit.snap_mode.min(SNAPS.len() - 1)], look::regular(13.0), look::text());
        updown(&p, pos2(sr.max.x - 11.0, cy), look::text());
        egui::Popup::menu(&sresp).show(|ui| {
            for (k, n) in SNAPS.iter().enumerate() {
                if ui.add(egui::Button::selectable(k == app.audio_edit.snap_mode, *n)).clicked() {
                    app.audio_edit.snap_mode = k;
                    ui.close();
                }
            }
        });
        rx -= 124.0;
        if icon_button(ui, b(rx - 32.0, 32.0), ae_id.with("snap"), app.audio_edit.snap_on, "Snap", |p, r, c| power_glyph(p, r.center(), c)).clicked() {
            app.audio_edit.snap_on = !app.audio_edit.snap_on;
        }
        p.text(pos2(rx - 38.0, cy), Align2::RIGHT_CENTER, "Snap:", look::regular(13.0), look::text());
    }
    p.hline(local.x_range(), local.max.y, Stroke::new(1.0, look::line()));

    // ---- ruler, region lane, amplitude scale
    let ruler = Rect::from_min_max(pos2(main.min.x, main.min.y), pos2(main.max.x, main.min.y + 22.0));
    let header = Rect::from_min_max(pos2(main.min.x, ruler.max.y + 1.0), pos2(main.max.x, ruler.max.y + 23.0));
    let wave = Rect::from_min_max(pos2(main.min.x, header.max.y), main.max);
    let amp = Rect::from_min_max(pos2(body.min.x, main.min.y), pos2(main.min.x, body.max.y));
    p.rect_filled(amp, 0.0, amp_bg());
    p.rect_filled(ruler, 0.0, ruler_bg());
    p.rect_filled(Rect::from_min_max(header.min, wave.max), 0.0, look::lane_bg());
    let (bw, scroll) = (app.audio_edit.t_bar_w as f64, app.audio_edit.t_scroll);
    let px_per_tick = bw / bar_t;
    let x_of = move |tick: f64| main.min.x + ((tick - scroll) * px_per_tick) as f32;
    let tick_of = move |x: f32| scroll + (x - main.min.x) as f64 / px_per_tick;
    // scrolling and zoom (wheel: time; ⌘ + wheel or a pinch: zoom)
    let hover = ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| main.contains(h));
    if hover {
        let (sd, zoom, cmd) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.command));
        let at = ui.input(|i| i.pointer.hover_pos()).map_or(main.center().x, |h| h.x);
        let z = if cmd && sd.y != 0.0 { 1.0 + sd.y * 0.003 } else { zoom };
        if (z - 1.0).abs() > 0.001 {
            app.audio_edit.zoomed = true;
            let anchor = tick_of(at);
            app.audio_edit.t_bar_w = (app.audio_edit.t_bar_w * z).clamp(20.0, 4000.0);
            app.audio_edit.t_scroll = (anchor - (at - main.min.x) as f64 / (app.audio_edit.t_bar_w as f64 / bar_t)).max(0.0);
        } else {
            let d = if sd.x != 0.0 { sd.x } else { sd.y };
            app.audio_edit.t_scroll = (app.audio_edit.t_scroll - d as f64 / px_per_tick).max(0.0);
        }
    }
    // the grid and the ruler's labels (bar / beat / division as there's room)
    let beat_t = PPQ_F * 4.0 / sig.1.max(1) as f64;
    let div_t = PPQ_F / 2.0;
    let rp = ui.painter_at(ruler);
    let wp = ui.painter_at(Rect::from_min_max(header.min, wave.max));
    let first = (tick_of(main.min.x) / div_t).floor().max(0.0) as i64;
    let last = (tick_of(main.max.x) / div_t).ceil() as i64;
    let beat_px = beat_t * px_per_tick;
    for k in first..=last {
        let tick = k as f64 * div_t;
        let x = x_of(tick);
        let is_bar = (tick / bar_t).fract().abs() < 1e-9;
        let is_beat = (tick / beat_t).fract().abs() < 1e-9;
        let (level, h) = if is_bar { (0, 22.0) } else if is_beat { (1, 9.0) } else { (2, 5.0) };
        if level == 2 && div_t * px_per_tick < 6.0 || level == 1 && beat_px < 6.0 {
            continue;
        }
        rp.vline(x, ruler.max.y - h..=ruler.max.y, Stroke::new(1.0, if is_bar { scale_text() } else { look::text_dim() }));
        let show = match level {
            0 => true,
            1 => beat_px > 46.0,
            _ => div_t * px_per_tick > 46.0,
        };
        if show && level == 0 || show && bw > 200.0 {
            rp.text(pos2(x + 3.0, ruler.min.y + 8.0), Align2::LEFT_CENTER, tick_label(tick, sig, level), if level == 0 { look::semibold(12.5) } else { look::regular(12.0) }, look::text());
        }
        if level < 2 || div_t * px_per_tick > 14.0 {
            wp.vline(x, header.min.y..=wave.max.y, Stroke::new(1.0, if is_bar { look::line() } else { look::grid_line() }));
        }
    }
    let ruler_resp = ui.interact(ruler, ui.scope_id().with("aterul"), Sense::click());
    crate::menus::context_menu(app, &ruler_resp, "audiotrack-editor-ruler");
    if ruler_resp.clicked() {
        if let Some(pp) = ruler_resp.interact_pointer_pos() {
            let tk = tick_of(pp.x).max(0.0);
            app.locate(tk);
        }
    }
    // ---- the region
    let rx0 = x_of(rg.start as f64);
    let rx1 = x_of((rg.start + rg.length) as f64);
    let reg = Rect::from_min_max(pos2(rx0, header.min.y), pos2(rx1, wave.max.y));
    let head = Rect::from_min_max(pos2(rx0, header.min.y), pos2(rx1, header.max.y));
    let rbody = Rect::from_min_max(pos2(rx0, header.max.y), pos2(rx1, wave.max.y));
    let muted = rg.muted;
    wp.rect_filled(rbody, 0.0, if muted { Color32::from_gray(0x50) } else { track_body() });
    wp.rect_filled(head, 0.0, if muted { Color32::from_gray(0xa0) } else { track_header() });
    let name = if x.gain_db.abs() > 0.01 { format!("{}   {:+.1} dB", rg.name, x.gain_db) } else { rg.name.clone() };
    wp.with_clip_rect(head.intersect(wave.union(header))).text(pos2(head.min.x + 22.0, head.center().y), Align2::LEFT_CENTER, name, look::regular(12.5), Color32::from_rgb(0x1c, 0x2c, 0x44));
    // the region's play glyph (▷) as Logic's header
    let tri = head.min + vec2(9.0, head.height() / 2.0);
    wp.add(egui::Shape::convex_polygon(vec![tri + vec2(-3.0, -4.5), tri + vec2(4.0, 0.0), tri + vec2(-3.0, 4.5)], Color32::TRANSPARENT, Stroke::new(1.0, Color32::from_rgb(0x1c, 0x2c, 0x44))));
    // the waveform
    let lane = rbody.shrink2(vec2(0.0, 1.0));
    let mid = lane.center().y;
    let half = lane.height() / 2.0;
    let g = 10f32.powf(x.gain_db / 20.0) * app.audio_edit.wave_zoom;
    let region_secs = app.seconds_at((rg.start + rg.length) as f64) - app.seconds_at(rg.start as f64);
    let start_sec = app.seconds_at(rg.start as f64);
    if let Some(l) = app.audio_edit.files.get(file) {
        let rate = l.audio.rate as f64;
        let from = rx0.max(wave.min.x).floor() as i32;
        let to = rx1.min(wave.max.x).ceil() as i32;
        let wc = if muted { Color32::from_gray(0x90) } else { track_wave() };
        for px in from..to {
            let s0 = app.seconds_at(tick_of(px as f32)) - start_sec;
            let s1 = app.seconds_at(tick_of(px as f32 + 1.0)) - start_sec;
            if s1 < 0.0 || s0 > region_secs {
                continue;
            }
            let f0 = x.offset as f64 + flex_src(&x.flex, s0.max(0.0) * rate);
            let f1 = x.offset as f64 + flex_src(&x.flex, s1.min(region_secs) * rate);
            let (mn, mx) = l.overview.range(&l.audio.data, None, f0, f1.max(f0 + 1.0));
            // fades as heard (the waveform follows the fade)
            let fg = x.fades.gain_at(s0.max(0.0), region_secs);
            let (a, b) = ((mx * g * fg).clamp(-1.0, 1.0), (mn * g * fg).clamp(-1.0, 1.0));
            wp.vline(px as f32 + 0.5, (mid - a * half)..=(mid - b * half).max(mid - a * half + 1.0), Stroke::new(1.0, wc));
        }
    }
    wp.hline(lane.x_range(), mid, Stroke::new(1.0, Color32::from_white_alpha(200)));
    // the fades: darker outside the curve, the curve drawn
    let (tempo0, tmap) = app.project.as_ref().map_or((120.0, Vec::new()), |p| (p.tempo, p.tempo_map.clone()));
    let sec_x = move |s: f64| x_of(crate::smarttempo::tick_at_tempo(tempo0, &tmap, start_sec + s));
    let fin = x.fades.in_ms as f64 / 1000.0;
    let fout = x.fades.out_ms as f64 / 1000.0;
    let shade = Color32::from_black_alpha(110);
    for (on, out) in [(fin > 0.0, false), (fout > 0.0, true)] {
        if !on {
            continue;
        }
        let n = 40;
        let mut curve = Vec::with_capacity(n + 1);
        for k in 0..=n {
            let f = k as f64 / n as f64;
            let (s, gv) = if out {
                (region_secs - fout + f * fout, x.fades.gain_at(region_secs - fout + f * fout, region_secs))
            } else {
                (f * fin, x.fades.gain_at(f * fin, region_secs))
            };
            curve.push(pos2(sec_x(s), rbody.max.y - gv * rbody.height()));
        }
        for w in curve.windows(2) {
            let q = vec![w[0], w[1], pos2(w[1].x, rbody.min.y), pos2(w[0].x, rbody.min.y)];
            wp.add(egui::Shape::convex_polygon(q, shade, Stroke::NONE));
        }
        wp.add(egui::Shape::line(curve, Stroke::new(1.5, Color32::from_white_alpha(230))));
    }
    // automation: the region's gain line (drag up or down)
    let gain_y = |db: f32| rbody.min.y + (1.0 - ((db + 30.0) / 36.0).clamp(0.0, 1.0)) * rbody.height();
    if app.audio_edit.show_automation {
        let y = gain_y(x.gain_db);
        wp.hline(rbody.x_range(), y, Stroke::new(2.0, Color32::from_rgb(0xe8, 0xc8, 0x3a)));
        wp.text(pos2(rbody.min.x + 6.0, y - 8.0), Align2::LEFT_CENTER, format!("Gain {:+.1} dB", x.gain_db), look::regular(11.0), Color32::from_rgb(0xe8, 0xc8, 0x3a));
    }
    let rate_of_file = rate_of(app, file).unwrap_or(48_000) as f64;
    // flex: the transients (grey) and the flex markers (orange)
    let mut flex_hits: Vec<(f32, usize, Option<usize>)> = Vec::new();
    if app.audio_edit.show_flex {
        let trs = transients(app, file);
        if let Some(rate) = rate_of(app, file) {
            let (a, bnd) = region_frames(app, rg, &x, rate);
            for f in trs.into_iter().filter(|f| *f >= a && *f < bnd) {
                let src = f - x.offset;
                let dst = flex_dst(&x.flex, src as f64);
                let px = sec_x(dst / rate as f64);
                if x.flex.iter().any(|m| m.0 == src) {
                    continue;
                }
                wp.vline(px, rbody.min.y..=rbody.max.y, Stroke::new(1.0, Color32::from_white_alpha(70)));
                flex_hits.push((px, src, None));
            }
            for (k, m) in x.flex.iter().enumerate() {
                let px = sec_x(m.1 as f64 / rate as f64);
                wp.vline(px, rbody.min.y..=rbody.max.y, Stroke::new(1.5, orange()));
                wp.add(egui::Shape::convex_polygon(vec![pos2(px - 5.0, rbody.min.y), pos2(px + 5.0, rbody.min.y), pos2(px, rbody.min.y + 6.0)], orange(), Stroke::NONE));
                flex_hits.push((px, m.0, Some(k)));
            }
        }
    }
    // the marquee
    if let Some((a, bq)) = app.audio_edit.marquee {
        let mr = Rect::from_min_max(pos2(x_of(a.min(bq)), header.min.y), pos2(x_of(a.max(bq)), wave.max.y));
        wp.rect_filled(mr, 0.0, Color32::from_white_alpha(40));
        wp.rect_stroke(mr, 0.0, Stroke::new(1.0, Color32::from_white_alpha(160)), egui::StrokeKind::Inside);
    }
    // ---- the region's gestures
    let area_resp = ui.interact(Rect::from_min_max(header.min, wave.max), ui.scope_id().with("atebody"), Sense::click_and_drag());
    // Flex Pitch: the notes, over the region (their own gestures)
    let dst_x = |d: f64| sec_x(d / rate_of_file);
    crate::flex::pitch_view(app, ui, rbody, t, r, &dst_x);
    let on_region = area_resp.hover_pos().or(area_resp.interact_pointer_pos()).is_some_and(|h| reg.contains(h));
    if on_region {
        crate::menus::context_menu(app, &area_resp, "region-audio");
    }
    let cmd = ui.input(|i| i.modifiers.command);
    let tool = if cmd { app.audio_edit.cmd_tool } else { app.audio_edit.tool };
    let hover = area_resp.hover_pos();
    // what's under the pointer
    let zone = |h: Pos2| -> &'static str {
        if !reg.expand2(vec2(5.0, 0.0)).contains(h) {
            return "";
        }
        let fin_x = sec_x(fin);
        let fout_x = sec_x(region_secs - fout);
        if app.audio_edit.show_flex && flex_hits.iter().any(|f| (f.0 - h.x).abs() < 4.0) && h.y > rbody.min.y {
            return "flex";
        }
        if app.audio_edit.show_automation && (h.y - gain_y(x.gain_db)).abs() < 5.0 {
            return "gain";
        }
        let top = h.y < head.max.y + 8.0;
        if top && ((h.x - rx0).abs() < 12.0 || (fin > 0.0 && (h.x - fin_x).abs() < 6.0)) {
            return "fadein";
        }
        if top && ((h.x - rx1).abs() < 12.0 || (fout > 0.0 && (h.x - fout_x).abs() < 6.0)) {
            return "fadeout";
        }
        if (h.x - rx0).abs() < 6.0 {
            return "start";
        }
        if (h.x - rx1).abs() < 6.0 {
            return "end";
        }
        let shift_ctrl = ui.input(|i| i.modifiers.shift && (i.modifiers.ctrl || i.modifiers.command));
        if shift_ctrl && fin > 0.0 && h.x < fin_x {
            return "curvein";
        }
        if shift_ctrl && fout > 0.0 && h.x > fout_x {
            return "curveout";
        }
        "body"
    };
    if let Some(h) = hover {
        if tool == crate::edit::Tool::Pointer {
            let z = zone(h);
            let icon = match z {
                "start" | "end" | "fadein" | "fadeout" | "flex" => Some(egui::CursorIcon::ResizeHorizontal),
                "gain" | "curvein" | "curveout" => Some(egui::CursorIcon::ResizeVertical),
                _ => None,
            };
            if let Some(i) = icon {
                ui.ctx().set_cursor_icon(i);
            }
            // the fade handles, shown at the top corners while the pointer is near
            if z == "fadein" || z == "fadeout" {
                let hx = if z == "fadein" { sec_x(fin) } else { sec_x(region_secs - fout) };
                wp.circle_filled(pos2(hx, head.max.y), 4.5, Color32::WHITE);
                wp.circle_stroke(pos2(hx, head.max.y), 4.5, Stroke::new(1.0, Color32::from_gray(0x30)));
            }
        } else if reg.contains(h) {
            ui.ctx().set_cursor_icon(match tool {
                crate::edit::Tool::Scissors | crate::edit::Tool::Marquee => egui::CursorIcon::Crosshair,
                crate::edit::Tool::Zoom => egui::CursorIcon::ZoomIn,
                _ => egui::CursorIcon::PointingHand,
            });
        }
    }
    let rate = rate_of(app, file).unwrap_or(48_000);
    if area_resp.drag_started() {
        // where the press was (the drag starts a few points later)
        if let Some(pp) = ui.input(|i| i.pointer.press_origin()).or(area_resp.interact_pointer_pos()) {
            let z = zone(pp);
            let tk = tick_of(pp.x);
            let d = match tool {
                crate::edit::Tool::Pointer => match z {
                    "start" => Some(TDrag::TrimStart { start: rg.start, length: rg.length, offset: x.offset }),
                    "end" => Some(TDrag::TrimEnd),
                    "fadein" => Some(TDrag::FadeIn),
                    "fadeout" => Some(TDrag::FadeOut),
                    "curvein" => Some(TDrag::Curve { out: false, orig: x.fades.in_curve, y0: pp.y }),
                    "curveout" => Some(TDrag::Curve { out: true, orig: x.fades.out_curve, y0: pp.y }),
                    "gain" => Some(TDrag::Gain { orig: x.gain_db, y0: pp.y }),
                    "flex" => flex_hits.iter().min_by(|a, b| (a.0 - pp.x).abs().total_cmp(&(b.0 - pp.x).abs())).map(|f| {
                        TDrag::Flex { src: f.1, index: f.2 }
                    }),
                    "body" => Some(TDrag::Move { from: tk, start: rg.start }),
                    _ => None,
                },
                crate::edit::Tool::Fade if z != "" => Some(if pp.x < reg.center().x { TDrag::FadeIn } else { TDrag::FadeOut }),
                crate::edit::Tool::Marquee => Some(TDrag::Marquee { from: snap_tick(app, tk, px_per_tick) }),
                crate::edit::Tool::Zoom => Some(TDrag::Zoom { from: tk }),
                crate::edit::Tool::Flex if z != "" => {
                    let src = ((app.seconds_at(tk) - start_sec).max(0.0) * rate as f64) as usize;
                    Some(TDrag::Flex { src: flex_src(&x.flex, src as f64) as usize, index: None })
                }
                _ => None,
            };
            if let Some(d) = d {
                if !matches!(d, TDrag::Marquee { .. } | TDrag::Zoom { .. }) {
                    checkpoint(app, match d {
                        TDrag::TrimStart { .. } | TDrag::TrimEnd => "Resize Region",
                        TDrag::FadeIn | TDrag::FadeOut => "Region Fade Length",
                        TDrag::Curve { .. } => "Region Fade Curve",
                        TDrag::Gain { .. } => "Region Gain",
                        TDrag::Flex { .. } => "Flex Marker",
                        _ => "Move Region",
                    });
                }
                app.audio_edit.t_drag = Some(d);
            }
        }
    }
    if let (true, Some(pp), Some(d)) = (area_resp.dragged(), area_resp.interact_pointer_pos(), app.audio_edit.t_drag) {
        let free = ui.input(|i| i.modifiers.shift);
        let snapped = if free { tick_of(pp.x) } else { snap_tick(app, tick_of(pp.x), px_per_tick) };
        let mut nx = extras(app, t, r);
        let file_frames = load(app, file).map_or(0, |l| l.audio.frames());
        match d {
            TDrag::TrimStart { start, length, offset } => {
                let end = (start + length) as f64;
                let start_s = app.seconds_at(start as f64);
                // not before the file's start, not past the region's end
                let min_tick = crate::smarttempo::tick_at(app, (start_s - offset as f64 / rate as f64).max(0.0));
                let nt = snapped.clamp(min_tick, end - 1.0);
                let dsec = app.seconds_at(nt) - start_s;
                nx.offset = (offset as f64 + dsec * rate as f64).round().max(0.0) as usize;
                if let Some(g) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                    g.start = nt.round() as u32;
                    g.length = (end - nt).round().max(1.0) as u32;
                }
                set_extras(app, t, r, nx);
                app.help_tag = Some(vec![("Position:".into(), crate::app::bbdt(nt as u32, sig)), ("Length:".into(), crate::app::bbdt_len((end - nt) as u32, sig))]);
            }
            TDrag::TrimEnd => {
                let start = rg.start as f64;
                // up to the file's end
                let max_sec = start_sec + (file_frames.saturating_sub(x.offset)) as f64 / rate as f64;
                let max_tick = crate::smarttempo::tick_at(app, max_sec);
                let ne = snapped.clamp(start + 1.0, max_tick.max(start + 1.0));
                if let Some(g) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                    g.length = (ne - start).round().max(1.0) as u32;
                }
                app.help_tag = Some(vec![("Length:".into(), crate::app::bbdt_len((ne - start) as u32, sig))]);
            }
            TDrag::FadeIn => {
                let s = (app.seconds_at(tick_of(pp.x)) - start_sec).clamp(0.0, region_secs);
                nx.fades.in_ms = (s * 1000.0).round().min(u16::MAX as f64) as u16;
                app.help_tag = Some(vec![("Fade In:".into(), format!("{} ms", nx.fades.in_ms))]);
                set_extras(app, t, r, nx);
            }
            TDrag::FadeOut => {
                let s = (start_sec + region_secs - app.seconds_at(tick_of(pp.x))).clamp(0.0, region_secs);
                nx.fades.out_ms = (s * 1000.0).round().min(u16::MAX as f64) as u16;
                app.help_tag = Some(vec![("Fade Out:".into(), format!("{} ms", nx.fades.out_ms))]);
                set_extras(app, t, r, nx);
            }
            TDrag::Curve { out, orig, y0 } => {
                let c = (orig as f32 + (pp.y - y0) * 0.8).round().clamp(-99.0, 99.0) as i8;
                if out {
                    nx.fades.out_curve = c;
                } else {
                    nx.fades.in_curve = c;
                }
                app.help_tag = Some(vec![("Curve:".into(), format!("{c}"))]);
                set_extras(app, t, r, nx);
            }
            TDrag::Gain { orig, y0 } => {
                let db = (orig - (pp.y - y0) * 36.0 / rbody.height().max(1.0)).clamp(-30.0, 6.0);
                nx.gain_db = (db * 10.0).round() / 10.0;
                app.help_tag = Some(vec![("Gain:".into(), format!("{:+.1} dB", nx.gain_db))]);
                set_extras(app, t, r, nx);
            }
            TDrag::Move { from, start } => {
                let nt = if free { start as f64 + tick_of(pp.x) - from } else { snap_tick(app, start as f64 + tick_of(pp.x) - from, px_per_tick) };
                if let Some(g) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                    g.start = nt.max(0.0).round() as u32;
                }
                app.help_tag = Some(vec![("Position:".into(), crate::app::bbdt(nt.max(0.0) as u32, sig))]);
            }
            TDrag::Marquee { from } => {
                app.audio_edit.marquee = Some((from, snapped));
                let (a, bq) = (from.min(snapped), from.max(snapped));
                app.help_tag = Some(vec![("Marquee:".into(), crate::app::bbdt(a as u32, sig)), ("Length:".into(), crate::app::bbdt_len((bq - a) as u32, sig))]);
            }
            TDrag::Zoom { from } => app.audio_edit.marquee = Some((from, tick_of(pp.x))),
            TDrag::Flex { src, index, .. } => {
                let dst_sec = (app.seconds_at(tick_of(pp.x)) - start_sec).clamp(0.0, region_secs);
                let dst = (dst_sec * rate as f64) as usize;
                // between the neighbouring markers
                let mut flex = nx.flex.clone();
                if let Some(k) = index.filter(|k| *k < flex.len()) {
                    flex.remove(k);
                }
                let lo = flex.iter().filter(|m| m.0 < src).map(|m| m.1 + 1).max().unwrap_or(1);
                let hi = flex.iter().filter(|m| m.0 > src).map(|m| m.1.saturating_sub(1)).min().unwrap_or(usize::MAX);
                flex.push((src, dst.clamp(lo, hi.max(lo))));
                flex.sort_by_key(|m| m.0);
                let k = flex.iter().position(|m| m.0 == src);
                nx.flex = flex;
                set_extras(app, t, r, nx);
                app.audio_edit.t_drag = Some(TDrag::Flex { src, index: k });
            }
        }
    }
    if area_resp.drag_stopped() {
        if let Some(d) = app.audio_edit.t_drag.take() {
            match d {
                TDrag::Zoom { from } => {
                    if let Some((a, bq)) = app.audio_edit.marquee.take() {
                        let _ = from;
                        let (a, bq) = (a.min(bq), a.max(bq));
                        if bq - a > 10.0 {
                            app.audio_edit.zoomed = true;
                            app.audio_edit.t_scroll = a;
                            app.audio_edit.t_bar_w = ((main.width() as f64 / (bq - a) * bar_t) as f32).clamp(20.0, 4000.0);
                        }
                    }
                }
                TDrag::Marquee { .. } => {}
                _ => app.after_edit(),
            }
        }
    }
    if area_resp.clicked() {
        if let Some(pp) = area_resp.interact_pointer_pos() {
            let tk = tick_of(pp.x);
            match tool {
                crate::edit::Tool::Scissors if reg.contains(pp) => split(app, t, r, snap_tick(app, tk, px_per_tick)),
                crate::edit::Tool::Eraser if reg.contains(pp) => {
                    checkpoint(app, "Delete Region");
                    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                        if r < tr.regions.len() {
                            tr.regions.remove(r);
                        }
                    }
                    app.audio_edit.regions.remove(&(t, r));
                    app.after_edit();
                }
                crate::edit::Tool::Mute if reg.contains(pp) => {
                    checkpoint(app, "Mute Region");
                    if let Some(g) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                        g.muted = !g.muted;
                    }
                    app.after_edit();
                }
                crate::edit::Tool::Zoom => {
                    app.audio_edit.zoomed = true;
                    app.audio_edit.t_bar_w = (app.audio_edit.t_bar_w * 0.5).max(20.0);
                }
                _ => {
                    app.audio_edit.marquee = None;
                    if !reg.contains(pp) {
                        app.locate(snap_tick(app, tk, px_per_tick).max(0.0));
                    }
                }
            }
        }
    }
    if area_resp.double_clicked() && app.audio_edit.show_flex {
        // a double-click on a flex marker removes it
        if let Some(pp) = area_resp.interact_pointer_pos() {
            let hit = flex_hits.iter().find(|f| f.2.is_some() && (f.0 - pp.x).abs() < 4.0).and_then(|f| f.2);
            if let Some(k) = hit {
                checkpoint(app, "Delete Flex Marker");
                let mut nx = extras(app, t, r);
                if k < nx.flex.len() {
                    nx.flex.remove(k);
                }
                set_extras(app, t, r, nx);
                app.after_edit();
            }
        }
    }
    // the playhead
    let ph = x_of(app.position());
    if app.audio_edit.catch && app.playing() && (ph > main.max.x || ph < main.min.x) {
        app.audio_edit.t_scroll = app.position();
    }
    if main.x_range().contains(ph) {
        ui.painter_at(main).vline(ph, main.y_range(), Stroke::new(1.2, look::playhead()));
    }
    // the amplitude scale
    amp_scale(&p, amp, lane, 25.0, true, 1.0);
    p.vline(amp.max.x, amp.y_range(), Stroke::new(1.0, look::line()));
}

/// Splits an audio region at a tick (Scissors): the right part continues in the file.
fn split(app: &mut App, t: usize, r: usize, tick: f64) {
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).cloned() else { return };
    let tick = tick.round() as u32;
    if tick <= rg.start || tick >= rg.start + rg.length {
        return;
    }
    let file = rg.audio.as_ref().map(|a| a.file.clone()).unwrap_or_default();
    let rate = rate_of(app, &file).unwrap_or(48_000) as f64;
    let x = extras(app, t, r);
    checkpoint(app, "Split Region");
    let dsec = app.seconds_at(tick as f64) - app.seconds_at(rg.start as f64);
    let mut right = rg.clone();
    right.start = tick;
    right.length = rg.start + rg.length - tick;
    right.source = None;
    right.origin = rg.source.clone().or(rg.origin.clone());
    let mut rx = x.clone();
    rx.offset = x.offset + flex_src(&x.flex, dsec * rate) as usize;
    rx.fades.in_ms = 0;
    // the markers after the cut, from the right part's start
    let (cs, cd) = (flex_src(&x.flex, dsec * rate), dsec * rate);
    rx.flex = x.flex.iter().filter(|m| m.0 as f64 > cs + 1.0).map(|m| ((m.0 as f64 - cs).round() as usize, (m.1 as f64 - cd).round().max(1.0) as usize)).collect();
    let mut lx = x;
    lx.fades.out_ms = 0;
    let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return };
    if let Some(g) = tr.regions.get_mut(r) {
        g.length = tick - rg.start;
    }
    tr.regions.push(right);
    let ri = tr.regions.len() - 1;
    set_extras(app, t, r, lx);
    set_extras(app, t, ri, rx);
    app.after_edit();
}

// ---------------------------------------------------------------------------------------------
// Audio File Editor

fn fmt_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn frame_text(app: &App, frame: usize, rate: u32, len: bool) -> String {
    let secs = frame as f64 / rate.max(1) as f64;
    match app.audio_edit.ruler {
        Ruler::Samples => format!("{frame}"),
        Ruler::MinSec => {
            let ms = (secs * 1000.0).round() as u64;
            format!("{}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
        }
        Ruler::Smpte => {
            let fr = (secs * 30.0) as u64;
            format!("{:02}:{:02}:{:02}:{:02}", fr / 108_000, fr / 1800 % 60, fr / 30 % 60, fr % 30)
        }
        Ruler::BarsBeats => {
            let sig = app.project.as_ref().map_or((4, 4), |p| p.signature);
            let ticks = secs * app.tempo() / 60.0 * PPQ_F;
            if len { crate::app::bbdt_len(ticks as u32, sig) } else { crate::app::bbdt(ticks as u32, sig) }
        }
    }
}

fn file_editor(app: &mut App, ui: &mut egui::Ui, body: Rect, t: usize, r: usize, rg: &lg_core::Region, file: &Path) {
    let p = ui.painter_at(body);
    let x = extras(app, t, r);
    let (frames, channels, rate) = match app.audio_edit.files.get(file) {
        Some(l) => (l.audio.frames(), l.audio.channels, l.audio.rate),
        None => return,
    };
    let (reg_a, reg_b) = region_frames(app, rg, &x, rate);
    let wave_w = (body.width() - FILE_AMP_W).max(50.0) as f64;
    if app.audio_edit.fitted != Some((t, r)) {
        app.audio_edit.f_per_px = (frames.max(1) as f64 / wave_w).max(0.01);
        app.audio_edit.f_start = 0.0;
        app.audio_edit.sel = None;
    }
    // ---- the local bar
    let local = Rect::from_min_size(body.min, vec2(body.width(), LOCAL_H));
    ui.scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_size(local.min + vec2(10.0, 0.0), vec2(420.0, LOCAL_H))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        crate::menus::local_bar(app, ui, &[("Audio File", "audiofile-AudioFile"), ("Edit", "audiofile-Edit"), ("Functions", "audiofile-Functions"), ("View", "audiofile-View")]);
    });
    let cy = local.center().y;
    let b = |x: f32, w: f32| Rect::from_min_size(pos2(x, cy - 12.0), vec2(w, 24.0));
    let id = ui.scope_id().with("afe");
    let mut bx = local.min.x + 352.0;
    if icon_button(ui, b(bx, 36.0), id.with("trm"), app.audio_edit.transient_mode, "Transient Editing Mode", |p, r, c| {
        let m = r.center();
        p.vline(m.x, m.y - 7.0..=m.y + 7.0, Stroke::new(1.4, c));
        p.line_segment([m + vec2(-3.0, -3.0), m + vec2(-6.0, 0.0)], Stroke::new(1.4, c));
        p.line_segment([m + vec2(-6.0, 0.0), m + vec2(-3.0, 3.0)], Stroke::new(1.4, c));
        p.line_segment([m + vec2(3.0, -3.0), m + vec2(6.0, 0.0)], Stroke::new(1.4, c));
        p.line_segment([m + vec2(6.0, 0.0), m + vec2(3.0, 3.0)], Stroke::new(1.4, c));
    }).clicked() {
        app.audio_edit.transient_mode = !app.audio_edit.transient_mode;
    }
    bx += 40.0;
    if icon_button(ui, b(bx, 36.0), id.with("catch"), app.audio_edit.catch, "Catch Playhead", |p, r, c| catch_glyph(p, r.center(), c)).clicked() {
        app.audio_edit.catch = !app.audio_edit.catch;
    }
    // the tools and prelisten group, centred where there's room
    let gx = (local.center().x - 250.0).max(bx + 50.0);
    let tcx = gx + 120.0;
    {
        use crate::edit::Tool;
        let tools = [Tool::Pointer, Tool::Pencil, Tool::Zoom];
        let cur = [Tool::Pointer, Tool::Pencil, Tool::Zoom][app.audio_edit.file_tool.min(2)];
        if let Some(tl) = tool_popup(ui, b(tcx - 120.0, 56.0), id.with("tool"), cur, &tools, "Left-click Tool") {
            app.audio_edit.file_tool = tools.iter().position(|x| *x == tl).unwrap_or(0);
        }
        let hr = icon_button(ui, b(tcx - 60.0, 56.0), id.with("hand"), false, "Command-click Tool: Hand (drag to scroll)", |p, r, c| {
            hand_glyph(p, pos2(r.min.x + 14.0, r.center().y), c);
            chevron(p, pos2(r.max.x - 9.0, r.center().y), c);
        });
        let _ = hr;
    }
    if icon_button(ui, b(tcx + 4.0, 36.0), id.with("pre"), false, "Prelisten (the selection, else the region)", |p, r, c| speaker_glyph(p, r.center() + vec2(2.0, 0.0), c, 1)).clicked() {
        prelisten(app, file);
    }
    if icon_button(ui, b(tcx + 44.0, 36.0), id.with("cyc"), app.audio_edit.prelisten_cycle, "Cycle (prelisten repeats)", |p, r, c| loop_glyph(p, r.center(), c)).clicked() {
        app.audio_edit.prelisten_cycle = !app.audio_edit.prelisten_cycle;
    }
    let vr = b(tcx + 86.0, 110.0);
    let mut vol = app.audio_edit.prelisten_vol;
    if mini_slider(ui, vr, id.with("vol"), &mut vol, "") {
        app.audio_edit.prelisten_vol = vol;
    }
    speaker_glyph(&p, pos2(vr.min.x + 8.0, cy), look::text(), 0);
    speaker_glyph(&p, pos2(vr.max.x + 10.0, cy), look::text(), 2);
    // the selection's start and length (else the region's) in the ruler's units
    let (s0, slen) = match app.audio_edit.sel {
        Some((a, b2)) => (a.min(b2), a.abs_diff(b2)),
        None => (reg_a, reg_b - reg_a),
    };
    let room = (local.max.x - 200.0) - (vr.max.x + 30.0);
    let rw = room.min(300.0);
    let rr = Rect::from_min_size(pos2(local.max.x - 200.0 - rw, cy - 13.0), vec2(rw, 26.0));
    if rw >= 150.0 {
        p.rect_filled(rr, 6.0, look::lcd_bg());
        p.vline(rr.center().x, rr.y_range().shrink(4.0), Stroke::new(1.0, look::line()));
        p.text(pos2(rr.center().x - 10.0, cy), Align2::RIGHT_CENTER, frame_text(app, s0, rate, false), look::regular(14.0), look::lcd_text());
        p.text(pos2(rr.max.x - 10.0, cy), Align2::RIGHT_CENTER, frame_text(app, slen, rate, true), look::regular(14.0), look::lcd_text());
    }
    // vertical (amplitude) and horizontal zoom
    let hz = Rect::from_min_size(pos2(local.max.x - 96.0, cy - 12.0), vec2(90.0, 24.0));
    let vz = Rect::from_min_size(pos2(local.max.x - 190.0, cy - 12.0), vec2(90.0, 24.0));
    let mut vzv = ((app.audio_edit.f_amp_zoom.ln()) / 32f32.ln()).clamp(0.0, 1.0);
    if mini_slider(ui, vz, id.with("vz"), &mut vzv, "↕") {
        app.audio_edit.f_amp_zoom = 32f32.powf(vzv);
    }
    let fit = frames.max(1) as f64 / wave_w;
    let lo = 0.05f64;
    let mut hzv = (1.0 - ((app.audio_edit.f_per_px / lo).ln() / (fit / lo).ln().max(1e-6))).clamp(0.0, 1.0) as f32;
    if mini_slider(ui, hz, id.with("hz"), &mut hzv, "↔") {
        app.audio_edit.zoomed = true;
        let centre = app.audio_edit.f_start + app.audio_edit.f_per_px * wave_w / 2.0;
        app.audio_edit.f_per_px = lo * (fit / lo).powf(1.0 - hzv as f64);
        app.audio_edit.f_start = (centre - app.audio_edit.f_per_px * wave_w / 2.0).max(0.0);
    }
    p.hline(local.x_range(), local.max.y, Stroke::new(1.0, look::line()));

    // ---- geometry: overview, ruler, waveform lanes, Anchor / Region / S. Loop lanes
    let ov = Rect::from_min_max(pos2(body.min.x, local.max.y + 1.0), pos2(body.max.x, local.max.y + 27.0));
    let ruler = Rect::from_min_max(pos2(body.min.x + FILE_AMP_W, ov.max.y + 1.0), pos2(body.max.x, ov.max.y + 26.0));
    let lanes_h = 16.0;
    let lanes = Rect::from_min_max(pos2(body.min.x, body.max.y - 3.0 * lanes_h), body.max);
    let wave = Rect::from_min_max(pos2(body.min.x + FILE_AMP_W, ruler.max.y), pos2(body.max.x, lanes.min.y));
    let amp = Rect::from_min_max(pos2(body.min.x, ruler.min.y), pos2(wave.min.x, lanes.min.y));
    p.rect_filled(ov, 0.0, overview_bg());
    p.rect_filled(Rect::from_min_max(pos2(body.min.x, ruler.min.y), ruler.max), 0.0, file_ruler_bg());
    p.rect_filled(wave, 0.0, file_bg());
    p.rect_filled(amp, 0.0, amp_bg());
    // scroll and zoom
    let hover = ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| wave.contains(h) || ruler.contains(h));
    let wave_w = wave.width() as f64;
    if hover {
        let (sd, zoom, cmd) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.command));
        let at = ui.input(|i| i.pointer.hover_pos()).map_or(wave.center().x, |h| h.x);
        let z = if cmd && sd.y != 0.0 { 1.0 + sd.y * 0.003 } else { zoom };
        if (z - 1.0).abs() > 0.001 {
            app.audio_edit.zoomed = true;
            let anchor = app.audio_edit.f_start + (at - wave.min.x) as f64 * app.audio_edit.f_per_px;
            app.audio_edit.f_per_px = (app.audio_edit.f_per_px / z as f64).clamp(lo, fit.max(lo));
            app.audio_edit.f_start = (anchor - (at - wave.min.x) as f64 * app.audio_edit.f_per_px).max(0.0);
        } else {
            let d = if sd.x != 0.0 { sd.x } else { sd.y };
            app.audio_edit.f_start = (app.audio_edit.f_start - d as f64 * app.audio_edit.f_per_px).clamp(0.0, (frames as f64 - wave_w * app.audio_edit.f_per_px).max(0.0));
        }
    }
    let (fs, fpp) = (app.audio_edit.f_start, app.audio_edit.f_per_px);
    let x_of = move |f: f64| wave.min.x + ((f - fs) / fpp) as f32;
    let f_of = move |x: f32| (fs + (x - wave.min.x) as f64 * fpp).max(0.0);
    // the overview: the whole file, the visible area boxed
    if let Some(l) = app.audio_edit.files.get(file) {
        let n = ov.width() as usize;
        let per = frames as f64 / n.max(1) as f64;
        let cols = l.overview.columns(&l.audio.data, None, 0.0, per, n);
        let mid = ov.center().y;
        for (k, (mn, mx)) in cols.iter().enumerate() {
            let xk = ov.min.x + k as f32 + 0.5;
            let (a, bb) = (mid - mx * ov.height() * 0.45, mid - mn * ov.height() * 0.45);
            p.vline(xk, a..=bb.max(a + 1.0), Stroke::new(1.0, file_wave()));
        }
        let vis = Rect::from_min_max(pos2(ov.min.x + (fs / per) as f32, ov.min.y + 1.0), pos2(ov.min.x + ((fs + wave_w * fpp) / per) as f32, ov.max.y - 1.0));
        p.rect_filled(vis, 0.0, Color32::from_white_alpha(28));
        p.rect_stroke(vis, 0.0, Stroke::new(1.0, Color32::from_gray(0xc0)), egui::StrokeKind::Inside);
    }
    let ov_resp = ui.interact(ov, id.with("ov"), Sense::click_and_drag());
    if ov_resp.drag_started() || ov_resp.clicked() {
        app.audio_edit.f_drag = Some(FDrag::Overview);
    }
    if let (true, Some(pp)) = (ov_resp.dragged() || ov_resp.clicked(), ov_resp.interact_pointer_pos()) {
        let per = frames as f64 / ov.width().max(1.0) as f64;
        let centre = (pp.x - ov.min.x) as f64 * per;
        app.audio_edit.f_start = (centre - wave_w * fpp / 2.0).clamp(0.0, (frames as f64 - wave_w * fpp).max(0.0));
    }
    // the ruler: nice steps at least 90 points apart
    let rp = ui.painter_at(ruler);
    let unit = match app.audio_edit.ruler {
        Ruler::Samples => 1.0,
        _ => rate as f64 / 1000.0,
    };
    let want = 90.0 * fpp / unit;
    let mag = 10f64.powf(want.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= want).unwrap_or(10.0 * mag) * unit;
    let mut f = (fs / step).floor() * step;
    while x_of(f) < ruler.max.x {
        let xk = x_of(f);
        if xk >= ruler.min.x - 1.0 {
            rp.vline(xk, ruler.min.y + 12.0..=ruler.max.y, Stroke::new(1.0, scale_text()));
            let label = match app.audio_edit.ruler {
                Ruler::Samples => fmt_thousands(f.round() as u64),
                _ => frame_text(app, f.round() as usize, rate, false),
            };
            rp.text(pos2(xk, ruler.min.y + 7.0), Align2::CENTER_CENTER, label, look::regular(11.5), look::text());
            for k in 1..5 {
                let xs = x_of(f + step * k as f64 / 5.0);
                rp.vline(xs, ruler.max.y - 4.0..=ruler.max.y, Stroke::new(1.0, look::text_dim()));
            }
            ui.painter_at(wave).vline(xk, wave.y_range(), Stroke::new(1.0, pick(0x444444, 0xdddddd)));
        }
        f += step;
    }
    // the region's start bracket in the ruler
    let rsx = x_of(reg_a as f64);
    if ruler.x_range().contains(rsx) {
        rp.line_segment([pos2(rsx, ruler.max.y - 10.0), pos2(rsx, ruler.max.y)], Stroke::new(1.5, Color32::from_gray(0xd8)));
        rp.line_segment([pos2(rsx, ruler.max.y - 10.0), pos2(rsx + 4.0, ruler.max.y - 13.0)], Stroke::new(1.5, Color32::from_gray(0xd8)));
    }
    // the waveform: a lane per channel
    let wp = ui.painter_at(wave);
    // outside the region: darker
    let dim = Color32::from_black_alpha(60);
    wp.rect_filled(Rect::from_min_max(wave.min, pos2(x_of(reg_a as f64).max(wave.min.x), wave.max.y)), 0.0, dim);
    wp.rect_filled(Rect::from_min_max(pos2(x_of(reg_b as f64).min(wave.max.x), wave.min.y), wave.max), 0.0, dim);
    // the selection
    if let Some((a, b2)) = app.audio_edit.sel {
        let sr = Rect::from_min_max(pos2(x_of(a.min(b2) as f64), wave.min.y), pos2(x_of(a.max(b2) as f64).max(x_of(a.min(b2) as f64) + 1.0), wave.max.y));
        wp.rect_filled(sr, 0.0, pick(0x6a6a6c, 0xc4d6f0));
    }
    let lane_h = wave.height() / channels.max(1) as f32;
    let zoom = app.audio_edit.f_amp_zoom;
    if let Some(l) = app.audio_edit.files.get(file) {
        let n = wave.width().ceil() as usize;
        for ch in 0..channels {
            let lane = Rect::from_min_size(pos2(wave.min.x, wave.min.y + ch as f32 * lane_h), vec2(wave.width(), lane_h));
            let mid = lane.center().y;
            let half = lane.height() / 2.0 - 2.0;
            if fpp < 2.0 {
                // close up: the samples joined (sample & hold off)
                let mut pts = Vec::new();
                let f0 = fs.floor() as usize;
                let f1 = (fs + wave_w * fpp).ceil() as usize + 1;
                for fr in f0..f1.min(frames) {
                    let v = (l.audio.at(fr, ch) * zoom).clamp(-1.0, 1.0);
                    pts.push(pos2(x_of(fr as f64), mid - v * half));
                }
                if fpp < 0.2 {
                    for q in &pts {
                        wp.circle_filled(*q, 2.0, file_wave());
                    }
                }
                wp.add(egui::Shape::line(pts, Stroke::new(1.2, file_wave())));
            } else {
                let cols = l.overview.columns(&l.audio.data, Some(ch), fs, fpp, n);
                for (k, (mn, mx)) in cols.iter().enumerate() {
                    let xk = wave.min.x + k as f32 + 0.5;
                    let a = mid - (mx * zoom).clamp(-1.0, 1.0) * half;
                    let bb = mid - (mn * zoom).clamp(-1.0, 1.0) * half;
                    wp.vline(xk, a..=bb.max(a + 1.0), Stroke::new(1.0, file_wave()));
                }
            }
            wp.hline(lane.x_range(), mid, Stroke::new(1.0, file_wave()));
            if ch > 0 {
                wp.hline(lane.x_range(), lane.min.y, Stroke::new(1.0, look::line()));
            }
            let steps = if lane.height() > 220.0 { 20.0 } else if lane.height() > 110.0 { 50.0 } else { 100.0 };
            amp_scale(&p, amp, Rect::from_min_max(pos2(amp.min.x, lane.min.y + 2.0), pos2(amp.max.x, lane.max.y - 2.0)), steps, app.audio_edit.amp_percent, zoom);
        }
    }
    // transients (Transient Editing Mode)
    let mut tr_hits: Vec<(f32, usize)> = Vec::new();
    if app.audio_edit.transient_mode {
        let trs = transients(app, file);
        for (k, fr) in trs.iter().enumerate() {
            let xk = x_of(*fr as f64);
            if wave.x_range().contains(xk) {
                wp.vline(xk, wave.y_range(), Stroke::new(1.0, Color32::from_rgb(0x9a, 0x9a, 0xff)));
                wp.add(egui::Shape::convex_polygon(vec![pos2(xk - 4.0, wave.min.y), pos2(xk + 4.0, wave.min.y), pos2(xk, wave.min.y + 6.0)], Color32::from_rgb(0x9a, 0x9a, 0xff), Stroke::NONE));
                tr_hits.push((xk, k));
            }
        }
    }
    // the playhead (where the project's playhead is in the file)
    let ph_sec = app.seconds_at(app.position()) - app.seconds_at(rg.start as f64);
    let ph_f = x.offset as f64 + ph_sec * rate as f64;
    if ph_sec >= 0.0 && wave.x_range().contains(x_of(ph_f)) {
        wp.vline(x_of(ph_f), wave.y_range(), Stroke::new(1.2, look::playhead()));
    }
    if (app.audio_edit.catch || app.audio_edit.f_scroll_in_play) && app.playing() && (x_of(ph_f) > wave.max.x || x_of(ph_f) < wave.min.x) && ph_sec >= 0.0 {
        app.audio_edit.f_start = ph_f;
    }
    // ---- the lanes: Anchor, Region, S. Loop
    let lane_rect = |k: usize| Rect::from_min_size(pos2(lanes.min.x, lanes.min.y + k as f32 * lanes_h), vec2(lanes.width(), lanes_h));
    for (k, name) in ["Anchor", "Region", "S. Loop"].iter().enumerate() {
        let lr = lane_rect(k);
        let label = Rect::from_min_max(lr.min, pos2(wave.min.x, lr.max.y));
        p.rect_filled(lr, 0.0, lane_bg());
        p.rect_filled(label, 0.0, lane_label_bg());
        p.text(pos2(label.min.x + 4.0, lr.center().y), Align2::LEFT_CENTER, *name, look::regular(11.0), look::text());
        p.hline(lr.x_range(), lr.min.y, Stroke::new(1.0, look::line()));
    }
    let lp = ui.painter_at(Rect::from_min_max(pos2(wave.min.x, lanes.min.y), lanes.max));
    // the anchor (a small flag at the region's anchor)
    let ax = x_of((x.offset + x.anchor) as f64);
    let al = lane_rect(0);
    lp.vline(ax, al.y_range(), Stroke::new(1.5, look::text()));
    lp.add(egui::Shape::convex_polygon(vec![pos2(ax, al.min.y + 2.0), pos2(ax + 7.0, al.min.y + 6.0), pos2(ax, al.min.y + 10.0)], look::text(), Stroke::NONE));
    // the region
    let rl = lane_rect(1);
    let rbar = Rect::from_min_max(pos2(x_of(reg_a as f64), rl.min.y + 1.0), pos2(x_of(reg_b as f64), rl.max.y));
    lp.rect_filled(rbar, 0.0, region_lane());
    lp.with_clip_rect(rbar.intersect(lanes)).text(pos2(rbar.min.x.max(wave.min.x) + 4.0, rl.center().y), Align2::LEFT_CENTER, &rg.name, look::semibold(12.0), Color32::WHITE);
    // the sample loop
    if let Some((a, b2)) = app.audio_edit.sample_loop {
        let sl = lane_rect(2);
        let lr = Rect::from_min_max(pos2(x_of(a as f64), sl.min.y + 1.0), pos2(x_of(b2 as f64), sl.max.y));
        lp.rect_filled(lr, 0.0, Color32::from_rgb(0xc8, 0x9a, 0x2a));
    }
    // ---- gestures
    let lanes_resp = ui.interact(Rect::from_min_max(pos2(wave.min.x, lanes.min.y), lanes.max), id.with("lanes"), Sense::click_and_drag());
    if let Some(h) = lanes_resp.hover_pos() {
        if (h.x - ax).abs() < 5.0 && al.y_range().contains(h.y) || rl.y_range().contains(h.y) && ((h.x - rbar.min.x).abs() < 5.0 || (h.x - rbar.max.x).abs() < 5.0) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
    if lanes_resp.drag_started() {
        // where the press was (the drag starts a few points later)
        if let Some(pp) = ui.input(|i| i.pointer.press_origin()).or(lanes_resp.interact_pointer_pos()) {
            let d = if al.y_range().contains(pp.y) {
                Some(FDrag::Anchor)
            } else if rl.y_range().contains(pp.y) && (pp.x - rbar.min.x).abs() < 8.0 {
                Some(FDrag::RegionStart { offset: x.offset, start: rg.start })
            } else if rl.y_range().contains(pp.y) && (pp.x - rbar.max.x).abs() < 8.0 {
                Some(FDrag::RegionEnd)
            } else {
                None
            };
            if let Some(d) = d {
                checkpoint(app, if matches!(d, FDrag::Anchor) { "Move Anchor" } else { "Resize Region" });
                app.audio_edit.f_drag = Some(d);
            }
        }
    }
    let wave_resp = ui.interact(wave, id.with("wave"), Sense::click_and_drag());
    let cmd = ui.input(|i| i.modifiers.command);
    if let Some(h) = wave_resp.hover_pos() {
        if app.audio_edit.transient_mode && tr_hits.iter().any(|t| (t.0 - h.x).abs() < 4.0) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        } else if cmd {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }
    let snap_zero = |app: &App, fr: usize| -> usize {
        if !app.audio_edit.zero_cross {
            return fr;
        }
        app.audio_edit.files.get(file).map_or(fr, |l| lg_audio::ops::zero_crossing(&l.audio, fr, (fpp * 6.0).max(32.0) as usize))
    };
    if wave_resp.drag_started() {
        // where the press was (the drag starts a few points later)
        if let Some(pp) = ui.input(|i| i.pointer.press_origin()).or(wave_resp.interact_pointer_pos()) {
            let hit = tr_hits.iter().find(|t| (t.0 - pp.x).abs() < 4.0).map(|t| t.1);
            app.audio_edit.f_drag = Some(if cmd {
                FDrag::Scroll { frame: fs, x: pp.x }
            } else if let Some(k) = hit {
                FDrag::Transient { index: k }
            } else {
                let fr = snap_zero(app, f_of(pp.x).round() as usize).min(frames);
                let shift = ui.input(|i| i.modifiers.shift);
                let from = match (shift, app.audio_edit.sel) {
                    (true, Some((a, b2))) => if fr < (a + b2) / 2 { a.max(b2) } else { a.min(b2) },
                    _ => fr,
                };
                FDrag::Select { from }
            });
        }
    }
    if let Some(d) = app.audio_edit.f_drag {
        let pp = wave_resp.interact_pointer_pos().or(lanes_resp.interact_pointer_pos());
        if let (true, Some(pp)) = (wave_resp.dragged() || lanes_resp.dragged(), pp) {
            let fr = f_of(pp.x).round().clamp(0.0, frames as f64) as usize;
            match d {
                FDrag::Select { from } => {
                    let fr = snap_zero(app, fr);
                    app.audio_edit.sel = Some((from.min(fr), from.max(fr)));
                    app.help_tag = Some(vec![("Start:".into(), frame_text(app, from.min(fr), rate, false)), ("Length:".into(), frame_text(app, from.abs_diff(fr), rate, true))]);
                    // drag past the edges: scroll
                    if pp.x > wave.max.x - 4.0 || pp.x < wave.min.x + 4.0 {
                        let dir = if pp.x > wave.max.x - 4.0 { 1.0 } else { -1.0 };
                        app.audio_edit.f_start = (app.audio_edit.f_start + dir * 8.0 * fpp).clamp(0.0, (frames as f64 - wave_w * fpp).max(0.0));
                    }
                }
                FDrag::Scroll { frame, x: x0 } => {
                    app.audio_edit.f_start = (frame - (pp.x - x0) as f64 * fpp).clamp(0.0, (frames as f64 - wave_w * fpp).max(0.0));
                }
                FDrag::Anchor => {
                    let mut nx = extras(app, t, r);
                    nx.anchor = fr.saturating_sub(nx.offset).min(reg_b - reg_a);
                    set_extras(app, t, r, nx);
                }
                FDrag::RegionStart { offset, start } => {
                    let fr = snap_zero(app, fr).min(reg_b.saturating_sub(1));
                    let mut nx = extras(app, t, r);
                    nx.offset = fr;
                    set_extras(app, t, r, nx);
                    // the region's audio stays where it plays: its start moves with the offset
                    let dsec = (fr as f64 - offset as f64) / rate as f64;
                    let end_tick = (rg.start + rg.length) as f64;
                    let ns = crate::smarttempo::tick_at(app, app.seconds_at(start as f64) + dsec).max(0.0);
                    if let Some(g) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                        g.start = ns.round() as u32;
                        g.length = (end_tick - ns).round().max(1.0) as u32;
                    }
                }
                FDrag::RegionEnd => {
                    let fr = snap_zero(app, fr).max(reg_a + 1);
                    let secs = (fr - x.offset) as f64 / rate as f64;
                    let s0 = app.seconds_at(rg.start as f64);
                    let end = crate::smarttempo::tick_at(app, s0 + secs);
                    if let Some(g) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(r)) {
                        g.length = (end - g.start as f64).round().max(1.0) as u32;
                    }
                }
                FDrag::Transient { index } => {
                    if let Some(l) = app.audio_edit.files.get_mut(file) {
                        if let Some(tr) = l.transients.as_mut() {
                            if index < tr.len() {
                                tr[index] = fr;
                            }
                        }
                    }
                }
                FDrag::Overview => {}
            }
        }
        if wave_resp.drag_stopped() || lanes_resp.drag_stopped() || ov_resp.drag_stopped() {
            app.audio_edit.f_drag = None;
            if matches!(d, FDrag::RegionStart { .. } | FDrag::RegionEnd) {
                app.after_edit();
            }
            if let (FDrag::Transient { .. }, Some(l)) = (d, app.audio_edit.files.get_mut(file)) {
                if let Some(tr) = l.transients.as_mut() {
                    tr.sort_unstable();
                }
            }
        }
    }
    if wave_resp.clicked() {
        if let Some(pp) = wave_resp.interact_pointer_pos() {
            match app.audio_edit.file_tool {
                2 => {
                    app.audio_edit.zoomed = true;
                    let anchor = f_of(pp.x);
                    let k = if ui.input(|i| i.modifiers.alt) { 2.0 } else { 0.5 };
                    app.audio_edit.f_per_px = (fpp * k).clamp(lo, fit.max(lo));
                    app.audio_edit.f_start = (anchor - (pp.x - wave.min.x) as f64 * app.audio_edit.f_per_px).max(0.0);
                }
                _ => {
                    app.audio_edit.sel = None;
                    // the playhead to where the file plays on the timeline
                    let fr = f_of(pp.x);
                    let s = app.seconds_at(rg.start as f64) + (fr - x.offset as f64) / rate as f64;
                    if s >= 0.0 {
                        let tk = crate::smarttempo::tick_at(app, s);
                        app.locate(tk);
                    }
                }
            }
        }
    }
    if wave_resp.double_clicked() && app.audio_edit.transient_mode {
        // a double-click adds a transient, or removes the one under the pointer
        if let Some(pp) = wave_resp.interact_pointer_pos() {
            let hit = tr_hits.iter().find(|t| (t.0 - pp.x).abs() < 4.0).map(|t| t.1);
            let fr = f_of(pp.x) as usize;
            if let Some(tr) = app.audio_edit.files.get_mut(file).and_then(|l| l.transients.as_mut()) {
                match hit {
                    Some(k) => {
                        tr.remove(k);
                    }
                    None => {
                        tr.push(fr);
                        tr.sort_unstable();
                    }
                }
            }
        }
    }
    p.vline(amp.max.x, amp.y_range(), Stroke::new(1.0, look::line()));
}

/// The selection, else the region (frames).
fn sel_or_region(app: &mut App, file: &Path) -> Option<Range<usize>> {
    if let Some((a, b)) = app.audio_edit.sel.filter(|s| s.0 != s.1) {
        return Some(a.min(b)..a.max(b));
    }
    let (t, r, rg, _) = selected(app)?;
    let x = extras(app, t, r);
    let rate = load(app, file)?.audio.rate;
    let (a, b) = region_frames(app, &rg, &x, rate);
    Some(a..b)
}

/// Prelisten: the selection (else the region) played once (or repeated with Cycle on).
fn prelisten(app: &mut App, file: &Path) {
    let Some(range) = sel_or_region(app, file) else { return };
    let Some(l) = load(app, file) else { return };
    let mut part = l.audio.slice(range);
    let vol = app.audio_edit.prelisten_vol * app.audio_edit.prelisten_vol * 1.6;
    part.data.iter_mut().for_each(|s| *s *= vol);
    let mut st = part.stereo_48k();
    if app.audio_edit.prelisten_cycle && !st.is_empty() {
        let once = st.clone();
        while st.len() < 48_000 * 2 * 30 {
            st.extend_from_slice(&once);
        }
    }
    let mut out = AudioFile::new(48_000, 2, st);
    out.float = true;
    let tmp = std::env::temp_dir().join(format!("bramble-prelisten-{}.wav", std::process::id()));
    if out.write(&tmp).is_ok() {
        app.output.preview(&tmp);
    }
}

/// A destructive Function on the selection (else the region): the backup made once, an undo
/// step, the file written, the engine reloaded.
fn destructive(app: &mut App, file: &Path, what: &str, f: impl FnOnce(&mut AudioFile, Range<usize>)) -> bool {
    let Some(range) = sel_or_region(app, file) else { return false };
    let Some(l) = load(app, file) else { return false };
    if !l.editable {
        app.tell("Only WAV and AIFF files can be changed here");
        return true;
    }
    let before = l.audio.clone();
    if !lg_audio::ops::has_backup(file) {
        if let Err(e) = lg_audio::ops::create_backup(file) {
            app.tell(format!("Couldn't make a backup: {e}"));
            return true;
        }
    }
    let mut after = before.clone();
    f(&mut after, range.clone());
    if let Err(e) = after.write(file) {
        app.tell(format!("Couldn't write {}: {e}", file.display()));
        return true;
    }
    app.audio_edit.file_undo.push((file.to_path_buf(), before, what.to_string()));
    if app.audio_edit.file_undo.len() > 10 {
        app.audio_edit.file_undo.remove(0);
    }
    app.audio_edit.file_redo.clear();
    // Trim: the regions on this file now start earlier in it
    if what == "Trim" {
        for (k, x) in app.audio_edit.regions.iter_mut() {
            let _ = k;
            if x.file == file {
                x.offset = x.offset.saturating_sub(range.start);
            }
        }
        app.audio_edit.sel = Some((0, range.end - range.start));
    }
    app.audio_edit.files.remove(file);
    let _ = load(app, file);
    app.load_engine();
    app.tell(what.to_string());
    true
}

fn file_undo(app: &mut App, redo: bool) -> bool {
    let stack = if redo { &mut app.audio_edit.file_redo } else { &mut app.audio_edit.file_undo };
    let Some((path, audio, what)) = stack.pop() else { return false };
    let current = match AudioFile::read(&path) {
        Ok(a) => a,
        Err(_) => return false,
    };
    if audio.write(&path).is_err() {
        return false;
    }
    let other = if redo { &mut app.audio_edit.file_undo } else { &mut app.audio_edit.file_redo };
    other.push((path.clone(), current, what.clone()));
    app.audio_edit.files.remove(&path);
    let _ = load(app, &path);
    app.load_engine();
    app.tell(format!("{} {what}", if redo { "Redo" } else { "Undo" }));
    true
}

// ---------------------------------------------------------------------------------------------
// Smart Tempo Editor

fn tempo_editor(app: &mut App, ui: &mut egui::Ui, body: Rect, t: usize, r: usize, rg: &lg_core::Region, file: &Path) {
    let p = ui.painter_at(body);
    let id = ui.scope_id().with("ste");
    if std::env::var_os("LG_AUDIO_ANALYZE").is_some() && !app.audio_edit.st.maps.contains_key(file) && !crate::smarttempo::analysing(app, file) && app.audio_edit.fitted != Some((t, r)) {
        crate::smarttempo::analyze(app, file);
    }
    let Some(analysis) = app.audio_edit.st.maps.get(file).cloned() else {
        // before analysis: Logic's words and the Analyze button
        let busy = crate::smarttempo::analysing(app, file);
        let lines = ["The audio file for the selected region does not contain tempo, downbeat, or time signature information.", "Click \"Analyze\" to start Smart Tempo detection and open the Smart Tempo Editor."];
        for (k, l) in lines.iter().enumerate() {
            p.text(body.center() - vec2(0.0, 36.0 - k as f32 * 18.0), Align2::CENTER_CENTER, *l, look::regular(13.0), look::text_dim());
        }
        let br = Rect::from_center_size(body.center() + vec2(0.0, 22.0), vec2(96.0, 26.0));
        let label = if busy { "Analyzing…" } else { "Analyze" };
        if ui.put(br, egui::Button::new(label).corner_radius(6.0)).clicked() && !busy {
            crate::smarttempo::analyze(app, file);
        }
        if busy {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        return;
    };
    let map = analysis.map.clone();
    let (frames, rate) = app.audio_edit.files.get(file).map_or((0, 48_000), |l| (l.audio.frames(), l.audio.rate));
    let secs = frames as f64 / rate as f64;
    let x = extras(app, t, r);
    let (reg_a, reg_b) = region_frames(app, rg, &x, rate);
    let (ra, rb) = (reg_a as f64 / rate as f64, reg_b as f64 / rate as f64);
    // ---- the local bar: Edit / View, scope, catch, the readouts, Tempo mode, click, listen, zoom
    let local = Rect::from_min_size(body.min, vec2(body.width(), LOCAL_H));
    ui.scope_builder(egui::UiBuilder::new().max_rect(Rect::from_min_size(local.min + vec2(10.0, 0.0), vec2(200.0, LOCAL_H))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        crate::menus::local_bar(app, ui, &[("Edit", "smarttempo-Edit"), ("View", "smarttempo-View")]);
    });
    let cy = local.center().y;
    let b = |x: f32, w: f32| Rect::from_min_size(pos2(x, cy - 12.0), vec2(w, 24.0));
    let scope = app.audio_edit.st.region_scope;
    let sr = b(local.min.x + 152.0, 36.0);
    let resp = ui.interact(sr, id.with("scope"), Sense::click()).on_hover_text(if scope { "Edit the selected region" } else { "Edit the track" });
    p.rect_filled(sr, 6.0, if scope { orange() } else { look::group_bg() });
    {
        let c = sr.center();
        let col = Color32::WHITE;
        p.vline(c.x - 5.0, c.y - 6.0..=c.y + 6.0, Stroke::new(1.3, col));
        p.vline(c.x + 5.0, c.y - 6.0..=c.y + 6.0, Stroke::new(1.3, col));
        p.hline(c.x - 3.0..=c.x + 3.0, c.y, Stroke::new(1.3, col));
        p.line_segment([c + vec2(-3.0, 0.0), c + vec2(-1.0, -2.0)], Stroke::new(1.3, col));
        p.line_segment([c + vec2(-3.0, 0.0), c + vec2(-1.0, 2.0)], Stroke::new(1.3, col));
        p.line_segment([c + vec2(3.0, 0.0), c + vec2(1.0, -2.0)], Stroke::new(1.3, col));
        p.line_segment([c + vec2(3.0, 0.0), c + vec2(1.0, 2.0)], Stroke::new(1.3, col));
    }
    if resp.clicked() {
        app.audio_edit.st.region_scope = !scope;
    }
    if icon_button(ui, b(local.min.x + 192.0, 36.0), id.with("catch"), app.audio_edit.catch, "Catch Playhead", |p, r, c| catch_glyph(p, r.center(), c)).clicked() {
        app.audio_edit.catch = !app.audio_edit.catch;
    }
    // the readouts: BAR BEAT | TEMPO | x2 /2 | SIGNATURE
    let sel_beat = app.audio_edit.st.selected.filter(|s| *s < map.beats.len());
    let beat_i = sel_beat.unwrap_or_else(|| map.beats.iter().position(|&s| s >= ra - 1e-6).unwrap_or(0));
    let (bar, beat) = map.bar_beat(beat_i);
    let tempo = if map.variable { map.tempo_at(map.beats.get(beat_i).copied().unwrap_or(0.0)) } else { map.tempo() };
    let lcd = Rect::from_center_size(pos2(local.center().x - 30.0, cy), vec2(386.0, 34.0));
    p.rect_filled(lcd, 7.0, look::lcd_bg());
    let cols = [lcd.min.x, lcd.min.x + 122.0, lcd.min.x + 212.0, lcd.min.x + 256.0, lcd.max.x];
    for cx in &cols[1..4] {
        p.vline(*cx, lcd.y_range().shrink(6.0), Stroke::new(1.0, look::line()));
    }
    let big = look::regular(20.0);
    let small = look::regular(9.0);
    let bb = Rect::from_min_max(pos2(cols[0], lcd.min.y), pos2(cols[1], lcd.max.y));
    p.text(pos2(bb.center().x - 18.0, lcd.min.y + 13.0), Align2::RIGHT_CENTER, format!("{bar}."), big.clone(), look::lcd_text());
    p.text(pos2(bb.center().x + 26.0, lcd.min.y + 13.0), Align2::RIGHT_CENTER, format!("{beat}."), big.clone(), look::lcd_text());
    p.text(pos2(bb.center().x - 22.0, lcd.max.y - 6.0), Align2::CENTER_CENTER, "BAR", small.clone(), look::lcd_dim());
    p.text(pos2(bb.center().x + 18.0, lcd.max.y - 6.0), Align2::CENTER_CENTER, "BEAT", small.clone(), look::lcd_dim());
    let tr = Rect::from_min_max(pos2(cols[1], lcd.min.y), pos2(cols[2], lcd.max.y));
    p.text(pos2(tr.center().x, lcd.min.y + 13.0), Align2::CENTER_CENTER, format!("{tempo:.1}"), big.clone(), if sel_beat.is_some() || map.variable { look::lcd_text() } else { look::lcd_dim() });
    p.text(pos2(tr.center().x, lcd.max.y - 6.0), Align2::CENTER_CENTER, "TEMPO", small.clone(), look::lcd_dim());
    // drag the tempo up or down
    let tresp = ui.interact(tr, id.with("tempo"), Sense::click_and_drag()).on_hover_text("Drag up or down to change the tempo");
    if tresp.hovered() || tresp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if tresp.drag_started() {
        app.audio_edit.st.taps.clear();
    }
    if tresp.dragged() && tresp.drag_delta().y != 0.0 {
        let fine = ui.input(|i| i.modifiers.shift);
        let nb = (tempo - tresp.drag_delta().y as f64 * if fine { 0.05 } else { 0.5 }).clamp(20.0, 400.0);
        crate::smarttempo::set_tempo(app, file, (nb * 10.0).round() / 10.0);
    }
    let xr = Rect::from_min_max(pos2(cols[2], lcd.min.y), pos2(cols[3], lcd.max.y));
    let x2 = Rect::from_min_max(xr.min, pos2(xr.max.x, xr.center().y));
    let h2 = Rect::from_min_max(pos2(xr.min.x, xr.center().y), xr.max);
    p.text(x2.center() + vec2(0.0, 1.0), Align2::CENTER_CENTER, "x2", look::regular(13.0), look::lcd_text());
    p.text(h2.center() - vec2(0.0, 1.0), Align2::CENTER_CENTER, "/2", look::regular(13.0), look::lcd_text());
    if ui.interact(x2, id.with("x2"), Sense::click()).on_hover_text("Double Tempo").clicked() {
        crate::smarttempo::edit(app, file, |m| m.double());
    }
    if ui.interact(h2, id.with("h2"), Sense::click()).on_hover_text("Half Tempo").clicked() {
        crate::smarttempo::edit(app, file, |m| m.halve());
    }
    let sg = Rect::from_min_max(pos2(cols[3], lcd.min.y), pos2(cols[4], lcd.max.y));
    p.text(pos2(sg.center().x, lcd.min.y + 13.0), Align2::CENTER_CENTER, format!("{}/{}", map.beats_per_bar, map.beat_unit), big, look::lcd_text());
    p.text(pos2(sg.center().x, lcd.max.y - 6.0), Align2::CENTER_CENTER, "SIGNATURE", small, look::lcd_dim());
    let sresp = ui.interact(sg, id.with("sig"), Sense::click());
    egui::Popup::menu(&sresp).show(|ui| {
        for (n, d) in [(2u8, 4u8), (3, 4), (4, 4), (5, 4), (6, 4), (7, 4), (6, 8), (7, 8), (9, 8), (12, 8)] {
            if ui.add(egui::Button::selectable(map.beats_per_bar == n && map.beat_unit == d, format!("{n}/{d}"))).clicked() {
                crate::smarttempo::edit(app, file, |m| {
                    m.beats_per_bar = n;
                    m.beat_unit = d;
                });
                ui.close();
            }
        }
    });
    // Tempo: Constant | Variable
    let mx = lcd.max.x + 12.0;
    p.text(pos2(mx, cy), Align2::LEFT_CENTER, "Tempo:", look::regular(13.0), look::text());
    let mr = b(mx + 50.0, 118.0);
    p.rect_filled(mr, 6.0, look::group_bg());
    let modes: Vec<&str> = crate::menus::context("smart-tempo-tempo-mode-popup").iter().filter(|i| !i.separator).map(|i| i.label.as_str()).collect();
    let modes = if modes.is_empty() { vec!["Constant", "Variable"] } else { modes };
    let mresp = ui.interact(mr, id.with("mode"), Sense::click());
    p.text(pos2(mr.min.x + 10.0, cy), Align2::LEFT_CENTER, if map.variable { "Variable" } else { "Constant" }, look::regular(13.0), look::text());
    updown(&p, pos2(mr.max.x - 11.0, cy), look::text());
    egui::Popup::menu(&mresp).show(|ui| {
        for m in &modes {
            let on = (*m == "Variable") == map.variable;
            if ui.add(egui::Button::selectable(on, *m)).clicked() {
                let name = m.to_string();
                crate::smarttempo::edit(app, file, |bm| if name == "Variable" { bm.variable = true } else { bm.make_constant() });
                ui.close();
            }
        }
    });
    // at the right: the click, listen, zoom
    let hz = Rect::from_min_size(pos2(local.max.x - 100.0, cy - 12.0), vec2(94.0, 24.0));
    let mut zv = (1.0 - (app.audio_edit.st.sec_per_px / (secs / 600.0).max(1e-4)).ln() / 200f64.ln()).clamp(0.0, 1.0) as f32;
    if mini_slider(ui, hz, id.with("hz"), &mut zv, "↔") {
        app.audio_edit.zoomed = true;
        let w = (body.width() - AMP_W) as f64;
        let centre = app.audio_edit.st.view_start + app.audio_edit.st.sec_per_px * w / 2.0;
        app.audio_edit.st.sec_per_px = (secs / 600.0).max(1e-4) * 200f64.powf(1.0 - zv as f64);
        app.audio_edit.st.view_start = (centre - app.audio_edit.st.sec_per_px * w / 2.0).max(0.0);
    }
    if icon_button(ui, b(hz.min.x - 44.0, 36.0), id.with("listen"), app.audio_edit.st.listen, "Listen", |p, r, c| speaker_glyph(p, r.center() + vec2(2.0, 0.0), c, 1)).clicked() {
        app.audio_edit.st.listen = !app.audio_edit.st.listen;
        if app.audio_edit.st.listen {
            prelisten(app, file);
            app.audio_edit.st.listen = false;
        }
    }
    if icon_button(ui, b(hz.min.x - 84.0, 36.0), id.with("click"), app.audio_edit.st.click, "Metronome Click", |p, r, c| metronome_glyph(p, r.center(), c)).clicked() {
        crate::smarttempo::command(app, "Toggle Metronome Click - Smart Tempo Editor");
    }
    p.hline(local.x_range(), local.max.y, Stroke::new(1.0, look::line()));

    // ---- the overview strip, ruler, region header, main view
    let ov = Rect::from_min_max(pos2(body.min.x, local.max.y + 1.0), pos2(body.max.x, local.max.y + 46.0));
    let ruler = Rect::from_min_max(pos2(body.min.x + AMP_W, ov.max.y + 3.0), pos2(body.max.x, ov.max.y + 38.0));
    let header = Rect::from_min_max(pos2(ruler.min.x, ruler.max.y + 1.0), pos2(body.max.x, ruler.max.y + 20.0));
    let main = Rect::from_min_max(pos2(ruler.min.x, header.max.y), body.max);
    let amp = Rect::from_min_max(pos2(body.min.x, ruler.min.y), pos2(ruler.min.x, body.max.y));
    p.rect_filled(Rect::from_min_max(ov.min, pos2(ov.max.x, ov.max.y + 2.0)), 0.0, editor_bg());
    p.rect_filled(ov.shrink2(vec2(1.0, 2.0)), 0.0, st_bg());
    p.rect_filled(Rect::from_min_max(pos2(body.min.x, ruler.min.y), ruler.max), 0.0, amp_bg());
    p.rect_filled(Rect::from_min_max(header.min, main.max), 0.0, st_bg());
    p.rect_filled(amp, 0.0, amp_bg());
    let w = main.width() as f64;
    if app.audio_edit.st.fitted_for.as_deref() != Some(file) || app.audio_edit.fitted != Some((t, r)) {
        let span = (rb - ra).max(0.5);
        app.audio_edit.st.sec_per_px = span * 1.02 / w;
        app.audio_edit.st.view_start = (ra - span * 0.01).max(0.0);
        app.audio_edit.st.fitted_for = Some(file.to_path_buf());
    }
    let hover = ui.input(|i| i.pointer.hover_pos()).is_some_and(|h| main.contains(h) || ruler.contains(h) || header.contains(h));
    if hover {
        let (sd, zoom, cmd) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.command));
        let at = ui.input(|i| i.pointer.hover_pos()).map_or(main.center().x, |h| h.x);
        let z = if cmd && sd.y != 0.0 { 1.0 + sd.y * 0.003 } else { zoom };
        let st = &mut app.audio_edit.st;
        if (z - 1.0).abs() > 0.001 {
            app.audio_edit.zoomed = true;
            let anchor = st.view_start + (at - main.min.x) as f64 * st.sec_per_px;
            st.sec_per_px = (st.sec_per_px / z as f64).clamp(1e-4, (secs / w).max(1e-4) * 2.0);
            st.view_start = (anchor - (at - main.min.x) as f64 * st.sec_per_px).max(0.0);
        } else {
            let d = if sd.x != 0.0 { sd.x } else { sd.y };
            st.view_start = (st.view_start - d as f64 * st.sec_per_px).clamp(0.0, secs.max(0.0));
        }
    }
    let (vs, spp) = (app.audio_edit.st.view_start, app.audio_edit.st.sec_per_px);
    let x_of = move |s: f64| main.min.x + ((s - vs) / spp) as f32;
    let s_of = move |x: f32| vs + (x - main.min.x) as f64 * spp;
    // overview: the file's waveform as blocks, bar lines, the visible box
    if let Some(l) = app.audio_edit.files.get(file) {
        let inner = ov.shrink2(vec2(1.0, 4.0));
        let n = inner.width() as usize;
        let per = frames as f64 / n.max(1) as f64;
        let cols = l.overview.columns(&l.audio.data, None, 0.0, per, n);
        let mid = inner.center().y;
        for (bs, _) in map.bars() {
            let xk = inner.min.x + (bs * rate as f64 / per) as f32;
            p.vline(xk, inner.y_range(), Stroke::new(1.0, st_grid()));
        }
        for (k, (mn, mx)) in cols.iter().enumerate() {
            let a = mid - mx.abs().max(mn.abs()) * inner.height() * 0.5;
            let bb = mid + mx.abs().max(mn.abs()) * inner.height() * 0.5;
            if bb - a > 0.5 {
                p.vline(inner.min.x + k as f32 + 0.5, a..=bb, Stroke::new(1.0, st_wave()));
            }
        }
        let vis = Rect::from_min_max(pos2(inner.min.x + (vs * rate as f64 / per) as f32, ov.min.y + 2.0), pos2(inner.min.x + ((vs + w * spp) * rate as f64 / per) as f32, ov.max.y - 2.0));
        p.rect_stroke(vis, 2.0, Stroke::new(1.0, Color32::from_gray(0x90)), egui::StrokeKind::Inside);
        let ovr = ui.interact(ov, id.with("ov"), Sense::click_and_drag());
        if let (true, Some(pp)) = (ovr.dragged() || ovr.clicked(), ovr.interact_pointer_pos()) {
            let c = (pp.x - inner.min.x) as f64 * per / rate as f64;
            app.audio_edit.st.view_start = (c - w * spp / 2.0).clamp(0.0, secs.max(0.0));
        }
    }
    // ruler: bar numbers, the signature under bar 1, the beat markers
    let rp = ui.painter_at(ruler);
    let mp = ui.painter_at(Rect::from_min_max(header.min, main.max));
    let first_bar_shown = map.bars().into_iter().find(|b| b.1 >= 1).map(|b| b.0);
    for (k, &bs) in map.beats.iter().enumerate() {
        let xk = x_of(bs);
        if xk < main.min.x - 2.0 || xk > main.max.x + 2.0 {
            continue;
        }
        let (bar, beat) = map.bar_beat(k);
        let down = beat == 1;
        let selected_b = sel_beat == Some(k) || app.audio_edit.st.range.is_some_and(|(a, b2)| k >= a.min(b2) && k <= a.max(b2));
        mp.vline(xk, header.max.y..=main.max.y, Stroke::new(1.0, if down { st_downbeat() } else { st_grid() }));
        let col = if selected_b { orange() } else if down { look::text() } else { look::text_dim() };
        rp.vline(xk, if down { ruler.min.y + 2.0 } else { ruler.max.y - 9.0 }..=ruler.max.y, Stroke::new(1.0, col));
        if down && (bar >= 1 || bars_dense(spp, &map)) {
            rp.text(pos2(xk + 4.0, ruler.min.y + 10.0), Align2::LEFT_CENTER, format!("{bar}"), look::regular(13.0), look::text());
        }
        if Some(bs) == first_bar_shown {
            rp.text(pos2(xk + 4.0, ruler.min.y + 26.0), Align2::LEFT_CENTER, format!("{}/{}", map.beats_per_bar, map.beat_unit), look::regular(13.0), look::text());
        }
        // the marker's handle
        let hy = ruler.max.y - 1.0;
        let tri = vec![pos2(xk - 4.0, hy - 6.0), pos2(xk + 4.0, hy - 6.0), pos2(xk, hy)];
        if selected_b || down {
            rp.add(egui::Shape::convex_polygon(tri, col, Stroke::NONE));
        }
    }
    // the tempo curve overlay
    if app.audio_edit.st.show_curve && map.beats.len() > 2 {
        let lo = map.curve().iter().map(|c| c.1).fold(f64::MAX, f64::min) - 5.0;
        let hi = map.curve().iter().map(|c| c.1).fold(f64::MIN, f64::max) + 5.0;
        let pts: Vec<Pos2> = map.curve().iter().map(|(s, bpm)| pos2(x_of(*s), main.max.y - ((bpm - lo) / (hi - lo)) as f32 * main.height())).collect();
        mp.add(egui::Shape::line(pts, Stroke::new(1.5, orange())));
    }
    // the region header and the waveform
    let hx0 = x_of(ra).max(header.min.x);
    let hx1 = x_of(rb).min(header.max.x);
    if hx1 > hx0 {
        let hr = Rect::from_min_max(pos2(hx0, header.min.y + 1.0), pos2(hx1, header.max.y));
        mp.rect_filled(hr, 2.0, st_wave());
        mp.with_clip_rect(hr).text(pos2(hr.min.x + 6.0, hr.center().y), Align2::LEFT_CENTER, &rg.name, look::regular(13.0), Color32::WHITE);
    }
    if let Some(l) = app.audio_edit.files.get(file) {
        let mid = main.center().y;
        let half = main.height() / 2.0 - 3.0;
        let n = main.width().ceil() as usize;
        let cols = l.overview.columns(&l.audio.data, None, vs * rate as f64, spp * rate as f64, n);
        for (k, (mn, mx)) in cols.iter().enumerate() {
            let s = s_of(main.min.x + k as f32);
            if s < 0.0 || s > secs {
                continue;
            }
            let inside = s >= ra && s <= rb;
            let a = mid - mx.clamp(-1.0, 1.0) * half;
            let bb = mid - mn.clamp(-1.0, 1.0) * half;
            mp.vline(main.min.x + k as f32 + 0.5, a..=bb.max(a + 1.0), Stroke::new(1.0, if inside { st_wave() } else { st_wave().gamma_multiply(0.45) }));
        }
        mp.hline(main.x_range(), mid, Stroke::new(1.0, st_grid()));
    }
    // transients (View ▸ Show Transients)
    let mut trs = Vec::new();
    if app.audio_edit.st.show_transients || app.audio_edit.st.snap_transients {
        trs = transients(app, file).into_iter().map(|f| f as f64 / rate as f64).collect::<Vec<_>>();
    }
    if app.audio_edit.st.show_transients {
        for &s in &trs {
            let xk = x_of(s);
            if main.x_range().contains(xk) {
                mp.vline(xk, main.min.y..=main.min.y + 10.0, Stroke::new(1.0, look::text_dim()));
            }
        }
    }
    // the playhead
    let rt = crate::smarttempo::region_time(app, t, r);
    if let Some(rt) = &rt {
        let ph = x_of(crate::smarttempo::tick_to_file(app, rt, app.position()));
        if main.x_range().contains(ph) {
            mp.vline(ph, header.min.y..=main.max.y, Stroke::new(1.2, look::playhead()));
        }
    }
    // ---- gestures: the beat markers (click selects, drag moves, double-click: the downbeat),
    // Shift-drag selects beats, a click elsewhere places the playhead
    let gest = Rect::from_min_max(ruler.min, main.max);
    let resp = ui.interact(gest, id.with("main"), Sense::click_and_drag());
    let near = |px: f32| -> Option<usize> {
        let k = map.nearest_beat(s_of(px))?;
        ((x_of(map.beats[k]) - px).abs() < 5.0).then_some(k)
    };
    if let Some(h) = resp.hover_pos() {
        if near(h.x).is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
    let drag_key = id.with("drag");
    if resp.drag_started() {
        // where the press was (the drag starts a few points later)
        if let Some(pp) = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos()) {
            let shift = ui.input(|i| i.modifiers.shift);
            if let (false, Some(k)) = (shift, near(pp.x)) {
                app.audio_edit.st.selected = Some(k);
                app.audio_edit.st.range = None;
                crate::smarttempo::edit(app, file, |_| {});
                // one undo step for the whole drag
                if let Some(a) = app.audio_edit.st.maps.get_mut(file) {
                    a.undo.push(a.map.clone());
                    a.redo.clear();
                }
                ui.data_mut(|d| d.insert_temp(drag_key, (k as i64, 0.0f64)));
            } else {
                let k = map.nearest_beat(s_of(pp.x)).unwrap_or(0);
                ui.data_mut(|d| d.insert_temp(drag_key, (-1i64 - k as i64, 0.0f64)));
            }
        }
    }
    if let (true, Some(pp)) = (resp.dragged(), resp.interact_pointer_pos()) {
        let (k, _) = ui.data(|d| d.get_temp::<(i64, f64)>(drag_key)).unwrap_or((i64::MIN, 0.0));
        if k >= 0 {
            let mut s = s_of(pp.x).clamp(0.0, secs);
            if app.audio_edit.st.snap_transients && !ui.input(|i| i.modifiers.shift) {
                if let Some(&tsec) = trs.iter().min_by(|a, b| (*a - s).abs().total_cmp(&(*b - s).abs())) {
                    if ((x_of(tsec) - pp.x).abs()) < 6.0 {
                        s = tsec;
                    }
                }
            }
            if let Some(a) = app.audio_edit.st.maps.get_mut(file) {
                a.map.move_beat(k as usize, s);
                let (bar, beat) = a.map.bar_beat(k as usize);
                let tempo = a.map.tempo_at(s);
                app.help_tag = Some(vec![("Beat:".into(), format!("{bar}. {beat}.")), ("Tempo:".into(), format!("{tempo:.2}"))]);
            }
        } else if k != i64::MIN {
            let from = (-1 - k) as usize;
            let to = map.nearest_beat(s_of(pp.x)).unwrap_or(from);
            app.audio_edit.st.range = Some((from.min(to), from.max(to)));
            app.audio_edit.st.selected = None;
        }
    }
    if resp.drag_stopped() {
        ui.data_mut(|d| d.remove::<(i64, f64)>(drag_key));
        // the edit undone if nothing moved
        if let Some(a) = app.audio_edit.st.maps.get_mut(file) {
            if a.undo.last() == Some(&a.map) {
                a.undo.pop();
            }
        }
    }
    if resp.double_clicked() {
        if let Some(k) = resp.interact_pointer_pos().and_then(|pp| near(pp.x)) {
            crate::smarttempo::edit(app, file, |m| m.set_downbeat(k));
        }
    } else if resp.clicked() {
        if let Some(pp) = resp.interact_pointer_pos() {
            match near(pp.x) {
                Some(k) => {
                    app.audio_edit.st.selected = Some(k);
                    app.audio_edit.st.range = None;
                }
                None => {
                    app.audio_edit.st.selected = None;
                    app.audio_edit.st.range = None;
                    if let Some(rt) = &rt {
                        let tk = crate::smarttempo::file_to_tick(app, rt, s_of(pp.x));
                        if tk >= 0.0 {
                            app.locate(tk);
                        }
                    }
                }
            }
        }
    }
    let ctx_resp = resp.clone();
    ctx_resp.context_menu(|ui| {
        ui.set_min_width(230.0);
        for item in crate::menus::local("smarttempo-Edit").iter().filter(|i| !i.separator && i.children.is_empty()) {
            if ui.button(&item.label).clicked() {
                command(app, &item.label);
                ui.close();
            }
        }
    });
    amp_scale(&p, amp, Rect::from_min_max(pos2(amp.min.x, main.min.y + 3.0), pos2(amp.max.x, main.max.y - 3.0)), 25.0, true, 1.0);
    p.vline(amp.max.x, amp.y_range(), Stroke::new(1.0, look::line()));
    let _ = x;
}

fn bars_dense(spp: f64, map: &lg_audio::tempo::BeatMap) -> bool {
    let bar_secs = 60.0 / map.tempo() * map.beats_per_bar as f64;
    bar_secs / spp > 30.0
}

// ---------------------------------------------------------------------------------------------
// sheets

fn dialogs(app: &mut App, ctx: &egui::Context, file: &Path) {
    let Some(d) = app.audio_edit.dialog.clone() else { return };
    let mut open = true;
    let mut close = false;
    match d {
        Dialog::ChangeGain { mut percent, mut db, mut relative } => {
            let peak = sel_or_region(app, file).and_then(|r| app.audio_edit.files.get(file).map(|l| lg_audio::ops::peak(&l.audio, r))).unwrap_or(0.0);
            egui::Window::new("Change Gain").collapsible(false).resizable(false).open(&mut open).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut relative, true, "Change Relative");
                    ui.radio_value(&mut relative, false, "Change Absolute");
                });
                ui.add_space(4.0);
                egui::Grid::new("cg").num_columns(3).show(ui, |ui| {
                    ui.label("Change:");
                    if ui.add(egui::DragValue::new(&mut percent).range(1.0..=1000.0).speed(0.5).suffix(" %")).changed() {
                        db = lg_audio::ops::percent_to_db(percent);
                    }
                    if ui.add(egui::DragValue::new(&mut db).range(-40.0..=20.0).speed(0.05).suffix(" dB")).changed() {
                        percent = 100.0 * 10f32.powf(db / 20.0);
                    }
                    ui.end_row();
                    ui.label("Maximum Level:");
                    ui.label(format!("{:.1} %", peak * 100.0));
                    ui.label(format!("{:.1} dB", 20.0 * peak.max(1e-6).log10()));
                    ui.end_row();
                    ui.label("Result:");
                    let res = if relative { peak * 10f32.powf(db / 20.0) } else { 10f32.powf(db / 20.0) };
                    ui.label(format!("{:.1} %", res * 100.0));
                    ui.label(if res > 1.0 { "clips" } else { "" });
                    ui.end_row();
                });
                if ui.button("Search Maximum").clicked() {
                    db = -20.0 * peak.max(1e-6).log10();
                    percent = 100.0 * 10f32.powf(db / 20.0);
                    relative = true;
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("Change").clicked() {
                        let gain_db = if relative { db } else { db - 20.0 * peak.max(1e-6).log10() };
                        destructive(app, file, "Change Gain", |a, r| lg_audio::ops::change_gain(a, r, gain_db));
                        close = true;
                    }
                });
            });
            if !close {
                app.audio_edit.dialog = Some(Dialog::ChangeGain { percent, db, relative });
            }
        }
        Dialog::Settings => {
            let (mut nd, mut fc, mut sd) = (app.audio_edit.normalize_db, app.audio_edit.fade_curve as i32, app.audio_edit.silence_db);
            egui::Window::new("Function Settings").collapsible(false).resizable(false).open(&mut open).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                egui::Grid::new("fs").num_columns(2).show(ui, |ui| {
                    ui.label("Normalize to:");
                    ui.add(egui::DragValue::new(&mut nd).range(-40.0..=0.0).speed(0.1).suffix(" dB"));
                    ui.end_row();
                    ui.label("Fade Curve:");
                    ui.add(egui::Slider::new(&mut fc, -99..=99));
                    ui.end_row();
                    ui.label("Silence Threshold:");
                    ui.add(egui::DragValue::new(&mut sd).range(-96.0..=-20.0).speed(0.5).suffix(" dB"));
                    ui.end_row();
                });
                if ui.button("OK").clicked() {
                    close = true;
                }
            });
            app.audio_edit.normalize_db = nd;
            app.audio_edit.fade_curve = fc as i8;
            app.audio_edit.silence_db = sd;
        }
        Dialog::TimePitch { mut tempo_pct, mut cents } => {
            egui::Window::new("Time and Pitch Machine").collapsible(false).resizable(false).open(&mut open).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                egui::Grid::new("tpm").num_columns(2).show(ui, |ui| {
                    ui.label("Tempo Change:");
                    ui.add(egui::DragValue::new(&mut tempo_pct).range(-75.0..=300.0).speed(0.1).suffix(" %"));
                    ui.end_row();
                    ui.label("Transposition:");
                    ui.add(egui::DragValue::new(&mut cents).range(-2400.0..=2400.0).speed(1.0).suffix(" cents"));
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("Process and Paste").clicked() {
                        let (tp, c) = (tempo_pct as f64, cents as f64);
                        destructive(app, file, "Time and Pitch Machine", |a, r| {
                            let ch = a.channels;
                            let part = a.data[r.start * ch..r.end * ch].to_vec();
                            let mut out = if c.abs() > 0.01 { lg_audio::stretch::pitch_shift(&part, ch, a.rate, c) } else { part };
                            if tp.abs() > 0.001 {
                                out = lg_audio::stretch::stretch(&out, ch, a.rate, 1.0 / (1.0 + tp / 100.0));
                            }
                            a.data.splice(r.start * ch..r.end * ch, out);
                        });
                        close = true;
                    }
                });
            });
            if !close {
                app.audio_edit.dialog = Some(Dialog::TimePitch { tempo_pct, cents });
            }
        }
    }
    if close || !open {
        app.audio_edit.dialog = None;
    }
}

// ---------------------------------------------------------------------------------------------
// commands

/// Commands that also mean something elsewhere (Undo, Copy…): ours only while the pointer is
/// over the editor (or its local menus).
const GENERIC: &[&str] = &["Undo", "Redo", "Cut", "Copy", "Paste", "Delete", "Select All"];

/// Every command name `command` runs (for the menus' enabling).
pub const COMMANDS: &[&str] = &[
    // Audio File: the local menu and the key commands
    "Create Backup", "Revert to Backup", "Save A Copy of Audio File As…", "Save Selection As…", "Detect Transients", "Detect Transients of Audio File", "Update File Information", "Refresh Overview",
    "Select All Previous Samples", "Select All Following Samples", "Region -> Selection", "Selection -> Region", "Create New Region", "Sample Loop -> Selection", "Selection -> Sample Loop", "Write Sample Loop to Audio File",
    "Selection Start", "Selection End", "Region Start", "Region End", "Region Anchor", "Previous Transient", "Next Transient",
    "Go to Region Start", "Go to Region End", "Go to Region Anchor", "Go to Previous Transient", "Go to Next Transient",
    "Selection Start To Previous Transient", "Selection Start To Next Transients", "Selection End To Previous Transient", "Selection End To Next Transient",
    "Set Selection Start To Previous Transient", "Set Selection Start To Next Transients", "Set Selection End To Previous Transient", "Set Selection End To Next Transient",
    "Selection Start and End to Previous Transient", "Selection Start and End to Next Transient", "Selection Start and End to Previous Transient and Play", "Selection Start and End to Next Transient and Play",
    "Region Anchor to Previous Transient", "Region Anchor to Next Transient", "Set Region Anchor to Previous Transient", "Set Region Anchor to Next Transient", "Set Region Anchor to Playhead", "Set Region Anchor to Region Start",
    "Snap Edits to Zero Crossings", "Toggle Snap Edits to Zero Crossings", "Lock Position in Track when moving Anchor",
    "Normalize", "Change Gain…", "Fade-In", "Fade-Out", "Silence", "Invert", "Reverse", "Trim", "Remove DC Offset", "Function Settings…", "Time and Pitch Machine…", "Adjust Tempo by Selection and Locators", "Search Peak", "Search Silence",
    "Samples", "Min:Sec:Ms", "SMPTE Time", "Bars/Beats", "Transient Editing Mode", "Toggle Transient Editing Mode", "Amplitude Percentage", "Amplitude Sample Value", "Scroll in Play", "Show File in Finder", "Show Files in Finder",
    "Increase Number of Transients", "Decrease Number of Transients",
    "Play/Stop All", "Play/Stop Region", "Play/Stop Region to Anchor", "Play/Stop Region from Anchor",
    "Open Audio File Editor…", "Show/Hide Audio File Editor",
    // Smart Tempo
    "Undo Edit Hint Marker", "Apply Region Tempo to Project Tempo…", "Apply Region Tempo to Project Tempo", "Apply Project Tempo to Region and Downbeat", "Maintain Time Position of All Regions", "Maintain Time Positions of All Regions",
    "Apply Changes Automatically", "Snap to Transients", "Snap To Transients", "Analyze Tempo", "Delete Hints", "Clear Hints", "Remove Tempo Information", "Revert to Original Recording Tempo", "Revert Changes",
    "All", "All Following", "All Previous", "Set Downbeat to Playhead Position", "Convert all Beat Hints to Downbeat Hints", "Convert all Downbeat Hints to Beat Hints",
    "Show Tempo Curve Overlay", "Show Transients", "Tap Tempo - Smart Tempo Editor", "Tap Tempo (Downbeat)", "Toggle Metronome Click - Smart Tempo Editor",
    "Scroll Left", "Scroll Right", "Select from Start to Playhead", "Select from Playhead to End", "Zoom All", "Center around Playhead", "Half Tempo", "Double Tempo", "Set Average Tempo for Selection", "Set Average Tempo within each Bar",
    "Extend First Tempo in Selection to Beginning", "Extend Last Tempo in Selection to End", "Trim Region To Selection", "Forward by Bar", "Rewind by Bar", "Forward by Beat", "Rewind by Beat", "Forward 8 Bars", "Rewind 8 Bars",
    // Project Audio (what applies to the selected region's file)
    "Select Previous Audio File", "Select Next Audio File", "Add Region",
    // the Audio Track Editor
    "Show/Hide Flex", "Show/Hide Automation",
    // while over the editor
    "Undo", "Redo", "Cut", "Copy", "Paste", "Delete", "Select All",
];

fn in_editor(app: &App) -> bool {
    app.editor_rect.contains(app.pointer) || app.audio_edit.ctx.as_ref().is_some_and(|c| c.input(|i| i.pointer.hover_pos()).is_some_and(|h| app.editor_rect.contains(h)))
}

/// Runs a command of the audio editors by its Logic name (key commands of the groups "Audio File
/// Editor", "Smart Tempo Editor", "Project Audio", "Windows Showing Audio Files", and the local
/// menus). False when it isn't one of theirs or doesn't apply now.
pub fn command(app: &mut App, name: &str) -> bool {
    // Undo, Copy… in the Track tab are the app's (the project's)
    if GENERIC.contains(&name) && (!in_editor(app) || app.audio_edit.tab == Tab::Track) {
        return false;
    }
    // the editor-wide ones
    match name {
        "Open Audio File Editor…" | "Show/Hide Audio File Editor" => {
            if !is_audio_region(app) {
                return false;
            }
            let showing = app.bottom == crate::app::Bottom::PianoRoll && app.audio_edit.tab == Tab::File;
            if name.starts_with("Show/Hide") && showing {
                app.bottom = crate::app::Bottom::None;
            } else {
                app.bottom = crate::app::Bottom::PianoRoll;
                app.audio_edit.tab = Tab::File;
            }
            return true;
        }
        "Show/Hide Flex" => {
            app.audio_edit.show_flex = !app.audio_edit.show_flex;
            return true;
        }
        "Show/Hide Automation" if is_audio_region(app) && in_editor(app) => {
            app.audio_edit.show_automation = !app.audio_edit.show_automation;
            return true;
        }
        "Toggle Snap Edits to Zero Crossings" | "Snap Edits to Zero Crossings" => {
            app.audio_edit.zero_cross = !app.audio_edit.zero_cross;
            return true;
        }
        "Lock Position in Track when moving Anchor" => {
            app.audio_edit.lock_anchor = !app.audio_edit.lock_anchor;
            return true;
        }
        "Select Previous Audio File" | "Select Next Audio File" => return next_audio_region(app, name.contains("Next")),
        _ => {}
    }
    let Some((t, r, rg, file)) = selected(app) else { return false };
    // Smart Tempo's names: in its tab (a few apply from anywhere)
    const ANYWHERE: &[&str] = &["Analyze Tempo", "Apply Region Tempo to Project Tempo…", "Apply Region Tempo to Project Tempo", "Apply Project Tempo to Region and Downbeat", "Set Downbeat to Playhead Position"];
    if app.audio_edit.tab == Tab::SmartTempo || ANYWHERE.contains(&name) {
        if match name {
            "Scroll Left" | "Scroll Right" => {
                let w = 600.0 * app.audio_edit.st.sec_per_px;
                let d = if name == "Scroll Left" { -w / 4.0 } else { w / 4.0 };
                app.audio_edit.st.view_start = (app.audio_edit.st.view_start + d).max(0.0);
                true
            }
            "Zoom All" => {
                app.audio_edit.st.fitted_for = None;
                true
            }
            "Center around Playhead" => {
                if let Some(rt) = crate::smarttempo::region_time(app, t, r) {
                    let s = crate::smarttempo::tick_to_file(app, &rt, app.position());
                    app.audio_edit.st.view_start = (s - 300.0 * app.audio_edit.st.sec_per_px).max(0.0);
                }
                true
            }
            _ => false,
        } {
            return true;
        }
        if app.audio_edit.tab == Tab::SmartTempo && GENERIC.contains(&name) {
            return crate::smarttempo::command(app, name);
        }
        if crate::smarttempo::command(app, name) {
            return true;
        }
    }
    // the Audio File Editor's names: in its tab (Project Audio's from anywhere)
    if app.audio_edit.tab != Tab::File && !["Detect Transients of Audio File", "Add Region", "Show Files in Finder"].contains(&name) {
        return false;
    }
    let Some(l) = load(app, &file) else { return false };
    let (frames, rate) = (l.audio.frames(), l.audio.rate);
    let x = extras(app, t, r);
    let (reg_a, reg_b) = region_frames(app, &rg, &x, rate);
    let sel = app.audio_edit.sel.map(|(a, b)| (a.min(b), a.max(b)));
    let play_frame = {
        let s = app.seconds_at(app.position()) - app.seconds_at(rg.start as f64);
        (x.offset as f64 + s * rate as f64).clamp(0.0, frames as f64) as usize
    };
    let mut trs = || transients_of(app, &file);
    let prev_tr = |trs: &[usize], f: usize| trs.iter().rev().find(|&&x| x < f).copied();
    let next_tr = |trs: &[usize], f: usize| trs.iter().find(|&&x| x > f).copied();
    let locate_frame = |app: &mut App, f: usize| {
        let s = app.seconds_at(rg.start as f64) + (f as f64 - x.offset as f64) / rate as f64;
        let tk = crate::smarttempo::tick_at(app, s.max(0.0));
        app.locate(tk);
        // and into view
        let w = 600.0 * app.audio_edit.f_per_px;
        if (f as f64) < app.audio_edit.f_start || f as f64 > app.audio_edit.f_start + w {
            app.audio_edit.f_start = (f as f64 - w / 4.0).max(0.0);
        }
    };
    match name {
        // ---- Audio File
        "Create Backup" => match lg_audio::ops::create_backup(&file) {
            Ok(_) => app.tell("Backup created"),
            Err(e) => app.tell(format!("Couldn't make a backup: {e}")),
        },
        "Revert to Backup" => {
            if !lg_audio::ops::has_backup(&file) {
                app.tell("There's no backup of this file");
                return true;
            }
            if let Ok(cur) = AudioFile::read(&file) {
                app.audio_edit.file_undo.push((file.clone(), cur, "Revert to Backup".into()));
            }
            match lg_audio::ops::revert_to_backup(&file) {
                Ok(()) => {
                    app.audio_edit.files.remove(&file);
                    app.load_engine();
                    app.tell("Reverted to the backup");
                }
                Err(e) => app.tell(format!("Couldn't revert: {e}")),
            }
        }
        "Save A Copy of Audio File As…" | "Save Selection As…" => {
            let whole = name.starts_with("Save A Copy");
            let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("audio").to_string();
            let ext = file.extension().and_then(|s| s.to_str()).unwrap_or("wav").to_string();
            let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
            let mut k = 1;
            let path = loop {
                let p = dir.join(format!("{stem}{}#{k:02}.{ext}", if whole { "" } else { " selection" }));
                if !p.exists() {
                    break p;
                }
                k += 1;
            };
            let Some(l) = app.audio_edit.files.get(&file) else { return false };
            let mut out = if whole { l.audio.clone() } else { l.audio.slice(sel.map_or(reg_a..reg_b, |s| s.0..s.1)) };
            out.format = lg_audio::Format::of(&path);
            match out.write(&path) {
                Ok(()) => app.tell(format!("Saved {}", path.display())),
                Err(e) => app.tell(format!("Couldn't save: {e}")),
            }
        }
        "Detect Transients" | "Detect Transients of Audio File" => {
            if let Some(l) = app.audio_edit.files.get_mut(&file) {
                l.transients = None;
            }
            app.audio_edit.transient_mode = true;
            let n = transients_of(app, &file).len();
            app.tell(format!("{n} transients"));
        }
        "Update File Information" | "Refresh Overview" => {
            app.audio_edit.files.remove(&file);
            let _ = load(app, &file);
        }
        // ---- Edit
        "Undo" => return file_undo(app, false),
        "Redo" => return file_undo(app, true),
        "Copy" | "Cut" => {
            let Some((a, b)) = sel.filter(|s| s.0 != s.1) else { return false };
            app.audio_edit.clipboard = app.audio_edit.files.get(&file).map(|l| l.audio.slice(a..b));
            if name == "Cut" {
                destructive(app, &file, "Cut", |au, r| {
                    let c = au.channels;
                    au.data.drain(r.start * c..r.end * c);
                });
                app.audio_edit.sel = Some((a, a));
            }
        }
        "Paste" => {
            let Some(clip) = app.audio_edit.clipboard.clone() else { return false };
            let at = sel.map_or(play_frame, |s| s.0);
            let ok = destructive(app, &file, "Paste", |au, r| {
                let _ = r;
                let c = au.channels;
                let data: Vec<f32> = (0..clip.frames()).flat_map(|f| (0..c).map(move |ch| (f, ch))).map(|(f, ch)| clip.at(f, ch)).collect();
                let at = at.min(au.frames());
                au.data.splice(at * c..at * c, data);
            });
            if ok {
                app.audio_edit.sel = Some((at, at + clip.frames()));
            }
        }
        "Delete" => {
            let Some((a, b)) = sel.filter(|s| s.0 != s.1) else { return false };
            destructive(app, &file, "Delete", |au, r| {
                let c = au.channels;
                au.data.drain(r.start * c..r.end * c);
            });
            app.audio_edit.sel = Some((a, a.min(b)));
        }
        "Select All" => app.audio_edit.sel = Some((0, frames)),
        "Select All Previous Samples" => app.audio_edit.sel = Some((0, sel.map_or(play_frame, |s| s.1))),
        "Select All Following Samples" => app.audio_edit.sel = Some((sel.map_or(play_frame, |s| s.0), frames)),
        "Region -> Selection" => app.audio_edit.sel = Some((reg_a, reg_b)),
        "Selection -> Region" => {
            let Some((a, b)) = sel.filter(|s| s.0 != s.1) else { return false };
            return set_region_span(app, t, r, a as f64 / rate as f64, b as f64 / rate as f64);
        }
        "Create New Region" | "Add Region" => {
            let (a, b) = sel.filter(|s| s.0 != s.1).unwrap_or((reg_a, reg_b));
            checkpoint(app, "Create Region");
            let start_sec = app.seconds_at(rg.start as f64) + (a as f64 - x.offset as f64) / rate as f64;
            let st = crate::smarttempo::tick_at(app, start_sec.max(0.0));
            let en = crate::smarttempo::tick_at(app, start_sec.max(0.0) + (b - a) as f64 / rate as f64);
            let mut nr = rg.clone();
            nr.name = format!("{}.1", rg.name);
            nr.start = st.round() as u32;
            nr.length = (en - st).round().max(1.0) as u32;
            nr.source = None;
            nr.origin = rg.source.clone().or(rg.origin.clone());
            let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return false };
            tr.regions.push(nr);
            let ri = tr.regions.len() - 1;
            set_extras(app, t, ri, RegionAudio { file: file.clone(), offset: a, ..Default::default() });
            app.selected_region = Some((t, ri));
            app.after_edit();
        }
        "Sample Loop -> Selection" => {
            let Some(l) = app.audio_edit.sample_loop else { return false };
            app.audio_edit.sel = Some(l);
        }
        "Selection -> Sample Loop" => {
            let Some(s) = sel.filter(|s| s.0 != s.1) else { return false };
            app.audio_edit.sample_loop = Some(s);
        }
        "Write Sample Loop to Audio File" => {
            let Some((a, b)) = app.audio_edit.sample_loop else { return false };
            // a RIFF 'smpl' chunk (one forward loop)
            let mut smpl = vec![0u8; 36];
            smpl[12..16].copy_from_slice(&(1_000_000_000u32 / rate.max(1)).to_le_bytes());
            smpl[16..20].copy_from_slice(&60u32.to_le_bytes());
            smpl[28..32].copy_from_slice(&1u32.to_le_bytes());
            let mut lp = vec![0u8; 24];
            lp[8..12].copy_from_slice(&(a as u32).to_le_bytes());
            lp[12..16].copy_from_slice(&(b.saturating_sub(1) as u32).to_le_bytes());
            smpl.extend(lp);
            destructive(app, &file, "Write Sample Loop", move |au, _| {
                au.extra.retain(|c| &c.0 != b"smpl");
                au.extra.push((*b"smpl", smpl));
            });
        }
        // ---- Go to / Set
        "Selection Start" => locate_frame(app, sel.map_or(reg_a, |s| s.0)),
        "Selection End" => locate_frame(app, sel.map_or(reg_b, |s| s.1)),
        "Region Start" | "Go to Region Start" => locate_frame(app, reg_a),
        "Region End" | "Go to Region End" => locate_frame(app, reg_b),
        "Region Anchor" | "Go to Region Anchor" => locate_frame(app, x.offset + x.anchor),
        "Previous Transient" | "Go to Previous Transient" => {
            let Some(f) = prev_tr(&trs(), play_frame) else { return false };
            locate_frame(app, f);
        }
        "Next Transient" | "Go to Next Transient" => {
            let Some(f) = next_tr(&trs(), play_frame) else { return false };
            locate_frame(app, f);
        }
        "Selection Start To Previous Transient" | "Set Selection Start To Previous Transient" | "Selection Start To Next Transients" | "Set Selection Start To Next Transients" => {
            let (a, b) = sel.unwrap_or((play_frame, play_frame));
            let f = if name.contains("Previous") { prev_tr(&trs(), a) } else { next_tr(&trs(), a) };
            let Some(f) = f else { return false };
            app.audio_edit.sel = Some((f.min(b), f.max(b)));
        }
        "Selection End To Previous Transient" | "Set Selection End To Previous Transient" | "Selection End To Next Transient" | "Set Selection End To Next Transient" => {
            let (a, b) = sel.unwrap_or((play_frame, play_frame));
            let f = if name.contains("Previous") { prev_tr(&trs(), b) } else { next_tr(&trs(), b) };
            let Some(f) = f else { return false };
            app.audio_edit.sel = Some((a.min(f), a.max(f)));
        }
        "Selection Start and End to Previous Transient" | "Selection Start and End to Next Transient" | "Selection Start and End to Previous Transient and Play" | "Selection Start and End to Next Transient and Play" => {
            // the selection becomes the span between two transients, a step back or on
            let trs = trs();
            let (a, _) = sel.unwrap_or((play_frame, play_frame));
            let s = if name.contains("Previous") { prev_tr(&trs, a) } else { next_tr(&trs, a) };
            let Some(s) = s else { return false };
            let e = next_tr(&trs, s).unwrap_or(frames);
            app.audio_edit.sel = Some((s, e));
            if name.ends_with("and Play") {
                prelisten(app, &file);
            }
        }
        "Region Anchor to Previous Transient" | "Set Region Anchor to Previous Transient" | "Region Anchor to Next Transient" | "Set Region Anchor to Next Transient" | "Set Region Anchor to Playhead" | "Set Region Anchor to Region Start" => {
            let cur = x.offset + x.anchor;
            let f = match name {
                n if n.contains("Previous") => prev_tr(&trs(), cur),
                n if n.contains("Next") => next_tr(&trs(), cur),
                n if n.contains("Playhead") => Some(play_frame),
                _ => Some(reg_a),
            };
            let Some(f) = f else { return false };
            checkpoint(app, "Move Anchor");
            let mut nx = extras(app, t, r);
            nx.anchor = f.clamp(reg_a, reg_b) - reg_a;
            set_extras(app, t, r, nx);
        }
        // ---- Functions (destructive)
        "Normalize" => {
            let db = app.audio_edit.normalize_db;
            destructive(app, &file, "Normalize", |a, r| {
                lg_audio::ops::normalize(a, r, db);
            });
        }
        "Change Gain…" => app.audio_edit.dialog = Some(Dialog::ChangeGain { percent: 100.0, db: 0.0, relative: true }),
        "Fade-In" => {
            let c = app.audio_edit.fade_curve;
            destructive(app, &file, "Fade-In", |a, r| lg_audio::ops::fade_in(a, r, c));
        }
        "Fade-Out" => {
            let c = app.audio_edit.fade_curve;
            destructive(app, &file, "Fade-Out", |a, r| lg_audio::ops::fade_out(a, r, c));
        }
        "Silence" => {
            destructive(app, &file, "Silence", lg_audio::ops::silence);
        }
        "Invert" => {
            destructive(app, &file, "Invert", lg_audio::ops::invert);
        }
        "Reverse" => {
            destructive(app, &file, "Reverse", lg_audio::ops::reverse);
        }
        "Trim" => {
            if sel.is_none_or(|s| s.0 == s.1) {
                app.tell("Select what to keep first");
                return true;
            }
            destructive(app, &file, "Trim", lg_audio::ops::trim);
        }
        "Remove DC Offset" => {
            destructive(app, &file, "Remove DC Offset", lg_audio::ops::remove_dc);
        }
        "Function Settings…" => app.audio_edit.dialog = Some(Dialog::Settings),
        "Time and Pitch Machine…" => app.audio_edit.dialog = Some(Dialog::TimePitch { tempo_pct: 0.0, cents: 0.0 }),
        "Adjust Tempo by Selection and Locators" => {
            // the project's tempo so the selection lasts as long as the cycle (locators)
            let Some((a, b)) = sel.filter(|s| s.0 != s.1) else { return false };
            let (c0, c1) = app.cycle;
            if c1 <= c0 {
                return false;
            }
            let secs = (b - a) as f64 / rate as f64;
            let beats = (c1 - c0) as f64 / PPQ_F;
            let bpm = beats * 60.0 / secs;
            checkpoint(app, "Adjust Tempo");
            app.set_tempo(bpm);
            app.after_edit();
            app.tell(format!("Tempo {bpm:.2} BPM"));
        }
        "Search Peak" => {
            let range = sel.filter(|s| s.0 != s.1).map_or(reg_a..reg_b, |s| s.0..s.1);
            let Some(f) = app.audio_edit.files.get(&file).and_then(|l| lg_audio::ops::search_peak(&l.audio, range)) else { return false };
            app.audio_edit.sel = Some((f, f));
            locate_frame(app, f);
        }
        "Search Silence" => {
            let from = sel.map_or(reg_a, |s| s.1);
            let th = 10f32.powf(app.audio_edit.silence_db / 20.0);
            let Some(s) = app.audio_edit.files.get(&file).and_then(|l| lg_audio::ops::search_silence(&l.audio, from, th, (rate / 20) as usize)) else {
                app.tell("No silence found");
                return true;
            };
            app.audio_edit.sel = Some((s.start, s.end));
            locate_frame(app, s.start);
        }
        // ---- View
        "Samples" => app.audio_edit.ruler = Ruler::Samples,
        "Min:Sec:Ms" => app.audio_edit.ruler = Ruler::MinSec,
        "SMPTE Time" => app.audio_edit.ruler = Ruler::Smpte,
        "Bars/Beats" => app.audio_edit.ruler = Ruler::BarsBeats,
        "Transient Editing Mode" | "Toggle Transient Editing Mode" => app.audio_edit.transient_mode = !app.audio_edit.transient_mode,
        "Increase Number of Transients" | "Decrease Number of Transients" => {
            let d = if name.starts_with("Increase") { 0.1 } else { -0.1 };
            app.audio_edit.sensitivity = (app.audio_edit.sensitivity + d).clamp(0.0, 1.0);
            if let Some(l) = app.audio_edit.files.get_mut(&file) {
                l.transients = None;
            }
            app.audio_edit.transient_mode = true;
        }
        "Amplitude Percentage" => app.audio_edit.amp_percent = true,
        "Amplitude Sample Value" => app.audio_edit.amp_percent = false,
        "Scroll in Play" => {
            if app.audio_edit.tab == Tab::SmartTempo {
                app.audio_edit.st.scroll_in_play = !app.audio_edit.st.scroll_in_play;
            } else {
                app.audio_edit.f_scroll_in_play = !app.audio_edit.f_scroll_in_play;
            }
        }
        "Show File in Finder" | "Show Files in Finder" => {
            let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
            // (never from checks and snapshots: it opens a window on the desktop)
            if std::env::var_os("LG_SNAP").is_some() {
                app.tell(format!("Show in Files: {}", dir.display()));
                return true;
            }
            let _ = std::process::Command::new("xdg-open").arg(dir).stdin(std::process::Stdio::null()).spawn();
        }
        // ---- playing
        "Play/Stop All" | "Play/Stop Region" | "Play/Stop Region to Anchor" | "Play/Stop Region from Anchor" => {
            let range = match name {
                "Play/Stop All" => 0..frames,
                "Play/Stop Region to Anchor" => reg_a..x.offset + x.anchor,
                "Play/Stop Region from Anchor" => x.offset + x.anchor..reg_b,
                _ => reg_a..reg_b,
            };
            let keep = app.audio_edit.sel;
            app.audio_edit.sel = Some((range.start, range.end));
            prelisten(app, &file);
            app.audio_edit.sel = keep;
        }
        _ => return false,
    }
    true
}

pub fn transients_of(app: &mut App, file: &Path) -> Vec<usize> {
    transients(app, file)
}

/// Select Previous / Next Audio File: the previous or next audio region in the project.
fn next_audio_region(app: &mut App, forward: bool) -> bool {
    let Some(p) = &app.project else { return false };
    let all: Vec<(usize, usize)> = p.tracks.iter().enumerate().flat_map(|(t, tr)| tr.regions.iter().enumerate().filter(|(_, r)| r.audio.is_some()).map(move |(r, _)| (t, r))).collect();
    if all.is_empty() {
        return false;
    }
    let cur = app.selected_region.and_then(|s| all.iter().position(|x| *x == s));
    let k = match (cur, forward) {
        (Some(i), true) => (i + 1).min(all.len() - 1),
        (Some(i), false) => i.saturating_sub(1),
        (None, _) => 0,
    };
    app.selected_region = Some(all[k]);
    app.selected_track = Some(all[k].0);
    true
}

/// The menu items shown ticked (the local menus' ✓ items), None when not ours.
pub fn checked(app: &App, label: &str) -> Option<bool> {
    if !is_audio_region(app) || !in_editor(app) {
        return None;
    }
    let ae = &app.audio_edit;
    if ae.tab == Tab::SmartTempo {
        if let Some(c) = crate::smarttempo::checked(app, label) {
            return Some(c);
        }
    }
    Some(match label {
        "Samples" => ae.ruler == Ruler::Samples,
        "Min:Sec:Ms" => ae.ruler == Ruler::MinSec,
        "SMPTE Time" => ae.ruler == Ruler::Smpte,
        "Bars/Beats" => ae.ruler == Ruler::BarsBeats,
        "Amplitude Percentage" => ae.amp_percent,
        "Amplitude Sample Value" => !ae.amp_percent,
        "Transient Editing Mode" => ae.transient_mode,
        "Snap Edits to Zero Crossings" => ae.zero_cross,
        "Lock Position in Track when moving Anchor" => ae.lock_anchor,
        "Scroll in Play" => ae.f_scroll_in_play,
        _ => return None,
    })
}

/// A key press while the pointer is over the audio editor: Logic's bindings for the tab's area
/// ("Audio File Editor", "Smart Tempo Editor", "Windows Showing Audio Files") run here first.
/// True when one ran.
pub fn key_press(app: &mut App, key: egui::Key, m: egui::Modifiers) -> bool {
    if !is_audio_region(app) {
        return false;
    }
    // ⌘Z / ⇧⌘Z in the File tab: the file's own undo (Logic's Audio File Editor keeps one)
    if app.audio_edit.tab == Tab::File && m.command && key == egui::Key::Z && !app.audio_edit.file_undo.is_empty() | m.shift {
        return file_undo(app, m.shift);
    }
    let areas: &[&str] = match app.audio_edit.tab {
        Tab::File => &["Audio File Editor", "Windows Showing Audio Files"],
        Tab::SmartTempo => &["Smart Tempo Editor", "Windows Showing Audio Files"],
        Tab::Track => &["Windows Showing Audio Files"],
    };
    // the tab's own area first (its ⌃ keys before the window-wide ⌘ ones: ⌃⇧R is Reverse
    // here, ⇧⌘R Show File in Finder), ⌘ before ⌃ within an area
    for area in areas {
        for ctrl in [false, true] {
            let names: Vec<String> = crate::keymap::bindings().iter().filter(|b| b.area == *area && b.ctrl == ctrl && b.key == key && b.cmd == m.command && b.alt == m.alt && b.shift == m.shift).map(|b| b.command.clone()).collect();
            for n in names {
                if command(app, &n) {
                    return true;
                }
            }
        }
    }
    false
}

/// Toggles Logic's metronome (the Smart Tempo Editor's M) through the app.
pub fn toggle_click(app: &mut App) {
    if let Some(ctx) = app.audio_edit.ctx.clone() {
        app.key_command(&ctx, "Toggle Metronome Click");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flex_maps_both_ways() {
        let flex = vec![(1000, 1500), (3000, 3000)];
        assert_eq!(flex_src(&flex, 750.0), 500.0);
        assert_eq!(flex_dst(&flex, 500.0), 750.0);
        assert_eq!(flex_src(&flex, 2250.0), 2000.0);
        assert_eq!(flex_src(&flex, 4000.0), 4000.0);
        for d in [0.0, 100.0, 1499.0, 2000.0, 5000.0] {
            assert!((flex_dst(&flex, flex_src(&flex, d)) - d).abs() < 1e-6);
        }
        assert_eq!(flex_src(&[], 123.0), 123.0);
    }

    #[test]
    fn labels() {
        assert_eq!(fmt_thousands(1_920_000), "1,920,000");
        assert_eq!(fmt_thousands(50), "50");
        assert_eq!(tick_label(0.0, (4, 4), 0), "1");
        assert_eq!(tick_label(960.0 * 4.0 + 960.0 + 480.0, (4, 4), 2), "2 2 3");
        assert_eq!(tick_label(960.0, (4, 4), 1), "1 2");
    }

    #[test]
    fn every_command_listed() {
        // the local menus' items Studio runs are in COMMANDS (menus enable by it)
        for menu in ["audiofile-AudioFile", "audiofile-Edit", "audiofile-Functions", "audiofile-View", "smarttempo-Edit", "smarttempo-View"] {
            fn walk(items: &[crate::menus::Item], out: &mut Vec<String>) {
                for i in items {
                    if i.children.is_empty() && !i.separator {
                        out.push(i.label.clone());
                    }
                    walk(&i.children, out);
                }
            }
            let mut labels = Vec::new();
            walk(crate::menus::local(menu), &mut labels);
            assert!(!labels.is_empty(), "{menu}");
            for l in labels {
                if l == "Show as Sample & Hold" || l.starts_with("Can’t") {
                    continue;
                }
                assert!(COMMANDS.contains(&l.as_str()), "{menu}: {l}");
            }
        }
    }

    #[test]
    fn plans_render_cut_gain_and_fades() {
        let dir = std::env::temp_dir().join(format!("lg-ae-plan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        let mut a = AudioFile::new(48_000, 1, vec![0.5; 48_000]);
        a.bits = 16;
        a.write(&path).unwrap();
        let plan = Plan { file: path.clone(), offset: 12_000, seconds: 0.5, gain_db: -6.0206, fades: RegionFades { in_ms: 100, ..Default::default() }, flex: Vec::new(), reverse: false, mode: Default::default(), notes: Vec::new() };
        let out = plan.render().unwrap();
        assert_eq!(out.len(), 48_000);
        assert_eq!(out[0], 0.0);
        assert!((out[2 * 10_000] - 0.25).abs() < 0.002, "{}", out[2 * 10_000]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
