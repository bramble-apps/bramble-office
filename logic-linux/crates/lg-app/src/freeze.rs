//! Freeze, Bounce in Place and exporting tracks and regions as audio files: all render a
//! track on its own, offline, through its instrument and (unless bypassed) its effects.
//!
//! Freeze: the header's Freeze button marks the track; the next play start renders its freeze
//! file (32-bit float; Track inspector ▸ Freeze Mode: Source Only without the effects, Pre
//! Fader with them), which then plays in place of the track until the button is clicked again.
//! Bounce Track / Regions in Place and Bounce and Replace All Tracks open Logic's dialogs
//! (24-bit files at the engine's rate). Export ▸ Track(s) / Regions as Audio Files open the
//! export dialog with the file name Pattern.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use egui::vec2;
use lg_core::trackmix::{build_name, civil, FreezeMode, NameCtx, NameElem, Normalize};
use lg_core::{Region, TrackKind};

use crate::app::App;
use crate::audio::{self, PlayTrack, RATE};
use crate::look;

pub const COMMANDS: &[&str] = &[
    "Track in Place…",
    "Bounce Track in Place…",
    "Regions in Place…",
    "And Replace All Tracks…",
    "Bounce and Replace All Tracks…",
    "All Tracks as Audio Files…",
    "Export All Tracks as Audio Files…",
    "Regions as Audio Files…",
    "Export Regions as Audio Files…",
    "Refresh Freeze Files",
];

#[derive(Clone)]
pub struct FreezeFile {
    pub samples: Arc<Vec<f32>>,
    pub mode: FreezeMode,
}

/// Where a bounce in place lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Dest {
    #[default]
    NewTrack,
    /// Tracks: Replace Track. Regions: Selected Track.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Source {
    Leave,
    #[default]
    Mute,
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FilePer {
    #[default]
    OneFile,
    Track,
    Region,
}

/// The Bounce in Place dialogs' choices (remembered for the next use).
#[derive(Clone, Debug, PartialEq)]
pub struct BipOpts {
    pub name: String,
    pub dest: Dest,
    pub file_per: FilePer,
    pub source: Source,
    pub multi_outputs: bool,
    pub as_additional_tracks: bool,
    pub bypass_fx: bool,
    pub tail_to_file: bool,
    pub tail_to_region: bool,
    pub automation: bool,
    pub normalize: Normalize,
}

impl Default for BipOpts {
    fn default() -> Self {
        BipOpts { name: String::new(), dest: Dest::NewTrack, file_per: FilePer::OneFile, source: Source::Mute, multi_outputs: false, as_additional_tracks: false, bypass_fx: false, tail_to_file: false, tail_to_region: false, automation: false, normalize: Normalize::OverloadProtection }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Range {
    #[default]
    TrimSilence,
    CycleOnly,
    ToProjectEnd,
}

/// The export dialog's choices.
#[derive(Clone, Debug, PartialEq)]
pub struct ExportOpts {
    pub aiff: bool,
    /// 16, 24, or 32 (float).
    pub bits: u16,
    pub range: Range,
    pub per_channel_strip: bool,
    pub bypass_fx: bool,
    pub tail: bool,
    pub automation: bool,
    pub tempo_info: bool,
    pub normalize: Normalize,
    pub add_to_browser: bool,
    pub pattern: Vec<NameElem>,
    pub custom: String,
    pub folder: Option<PathBuf>,
}

impl Default for ExportOpts {
    fn default() -> Self {
        ExportOpts { aiff: false, bits: 24, range: Range::TrimSilence, per_channel_strip: false, bypass_fx: false, tail: false, automation: false, tempo_info: true, normalize: Normalize::OverloadProtection, add_to_browser: false, pattern: vec![NameElem::TrackName], custom: String::new(), folder: None }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialog {
    Track,
    Regions,
    /// Bounce and Replace All Tracks: first whether to save, then the options.
    AllAsk,
    All,
    ExportTracks { all: bool },
    ExportRegions,
}

#[derive(Default)]
pub struct State {
    pub files: HashMap<u64, FreezeFile>,
    pub dialog: Option<Dialog>,
    pub track: BipOpts,
    pub regions: BipOpts,
    pub all: BipOpts,
    pub export: ExportOpts,
    pub export_regions: ExportOpts,
}

/// What one render plays: these tracks alone, their notes kept to the given tick ranges.
#[derive(Clone, Debug, Default)]
pub struct What {
    pub tracks: Vec<usize>,
    /// (track, from, to): only notes starting inside play. Empty: everything.
    pub only: Vec<(usize, u32, u32)>,
    pub from: u32,
    pub to: u32,
    pub bypass_fx: bool,
    /// With the fader, pan and their automation (else unity, centred).
    pub fader: bool,
    pub tail_secs: f32,
    pub until_silent: bool,
}

/// A prepared offline render (can run on another thread).
pub struct Job {
    engine: audio::Engine,
    from: u32,
    to: u32,
    tail_secs: f32,
    until_silent: bool,
}

impl Job {
    pub fn run(self) -> Vec<f32> {
        audio::render_offline(self.engine, self.from, self.to, self.tail_secs, self.until_silent)
    }
}

/// The engine's tracks prepared for `what`: the others muted, sends off (the strip alone).
pub fn prepare(mut tracks: Vec<PlayTrack>, what: &What) -> Vec<PlayTrack> {
    for (i, pt) in tracks.iter_mut().enumerate() {
        pt.solo = false;
        if !what.tracks.contains(&i) {
            pt.mute = true;
            continue;
        }
        pt.mute = false;
        pt.sends.clear();
        if !what.only.is_empty() {
            pt.notes.retain(|n| what.only.iter().any(|&(t, a, b)| t == i && n.0 >= a && n.0 < b));
            // (audio regions outside the ranges stay out too)
            pt.audio.retain(|(at, _)| what.only.iter().any(|&(t, a, b)| t == i && *at >= a && *at < b));
        }
        if what.bypass_fx {
            pt.inserts.clear();
            pt.eq = None;
        }
        if !what.fader {
            pt.volume = 1.0;
            pt.pan = 0.0;
            pt.auto_volume.clear();
            pt.auto_pan.clear();
        }
    }
    tracks
}

pub fn job(app: &App, what: &What) -> Job {
    let (tracks, sf, param_auto) = {
        let e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
        (e.tracks.clone(), e.soundfont.clone(), e.param_auto.clone())
    };
    let mut engine = audio::Engine::new();
    if let Some(p) = app.project.as_ref() {
        engine.tempo = p.tempo;
        engine.tempo_map = p.tempo_map.clone();
        engine.beats_per_bar = p.signature.0.max(1) as u32;
        engine.timeline = lg_core::timeline::Map::of(p);
    }
    engine.tracks = prepare(tracks, what);
    engine.param_auto = param_auto;
    // (the tracks' outputs: straight to the Stereo Out, without the Mixer's routing)
    engine.load_soundfont(sf.as_deref());
    Job { engine, from: what.from, to: what.to, tail_secs: what.tail_secs, until_silent: what.until_silent }
}

fn project_end(app: &App) -> u32 {
    app.project.as_ref().map_or(0, |p| p.tracks.iter().flat_map(|t| t.regions.iter()).map(|r| r.start + r.length).max().unwrap_or(0)).max(app.bar_ticks())
}

fn track_span(app: &App, t: usize) -> Option<(u32, u32)> {
    let tr = app.project.as_ref()?.tracks.get(t)?;
    let from = tr.regions.iter().map(|r| r.start).min()?;
    let to = tr.regions.iter().map(|r| r.start + r.length).max()?;
    Some((from, to))
}

// ---------------------------------------------------------------------------------------------
// freeze

fn freeze_dir(app: &App) -> Option<PathBuf> {
    let (alt, _) = app.file.as_ref()?;
    Some(lg_core::project::package_of(alt).join("Freeze Files"))
}

fn freeze_path(app: &App, id: u64, mode: FreezeMode) -> Option<PathBuf> {
    Some(freeze_dir(app)?.join(format!("Track {id} {}.wav", if mode == FreezeMode::SourceOnly { "source" } else { "pre-fader" })))
}

/// Freezes or unfreezes track `i`. Unfreezing drops its freeze file.
pub fn set_frozen(app: &mut App, i: usize, frozen: bool) {
    let Some(t) = app.project.as_ref().and_then(|p| p.tracks.get(i)) else { return };
    let (id, mode) = (t.extra.id, t.extra.set.freeze_mode);
    if let Some(s) = crate::tm::set_mut(app, i) {
        s.frozen = frozen;
    }
    if !frozen {
        app.tm.freeze.files.remove(&id);
        if let Some(p) = freeze_path(app, id, mode) {
            let _ = std::fs::remove_file(p);
        }
    } else {
        app.armed.remove(&i);
    }
    crate::mixengine::sync(app);
}

/// The frozen tracks whose freeze file is missing or was made in the other mode.
pub fn stale(app: &App) -> Vec<usize> {
    let Some(p) = app.project.as_ref() else { return Vec::new() };
    p.tracks.iter().enumerate().filter(|(_, t)| t.extra.set.frozen && app.tm.freeze.files.get(&t.extra.id).is_none_or(|f| f.mode != t.extra.set.freeze_mode)).map(|(i, _)| i).collect()
}

/// Renders track `i`'s freeze file now: the whole track with its instrument and automation,
/// before the fader; Pre Fader with its effects, Source Only without them.
pub fn render_freeze(app: &mut App, i: usize) {
    let Some(t) = app.project.as_ref().and_then(|p| p.tracks.get(i)) else { return };
    let (id, mode) = (t.extra.id, t.extra.set.freeze_mode);
    // (its own freeze file must not play into the render)
    app.tm.freeze.files.remove(&id);
    let what = What { tracks: vec![i], from: 0, to: project_end(app), bypass_fx: mode == FreezeMode::SourceOnly, fader: false, tail_secs: 4.0, until_silent: true, ..Default::default() };
    let samples = job(app, &what).run();
    if let Some(path) = freeze_path(app, id, mode) {
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        // freeze files: 32-bit floating point, so nothing clips
        let mut f = lg_audio::AudioFile::new(RATE, 2, samples.clone());
        f.bits = 32;
        f.float = true;
        let _ = f.write(&path);
    }
    app.tm.freeze.files.insert(id, FreezeFile { samples: Arc::new(samples), mode });
}

/// At a play start: the freeze files still to make are rendered, then playback goes on.
pub fn frame(app: &mut App, _ctx: &egui::Context) {
    if !app.playing() {
        return;
    }
    let todo = stale(app);
    if todo.is_empty() {
        return;
    }
    let at = app.position();
    app.stop();
    for i in todo {
        render_freeze(app, i);
    }
    crate::mixengine::sync(app);
    app.locate(at);
    app.play_stop();
    app.tell("Freeze files rendered");
}

/// After a project opens: the freeze files saved with it.
pub fn after_open(app: &mut App) {
    let Some(p) = app.project.as_ref() else { return };
    let frozen: Vec<(u64, FreezeMode)> = p.tracks.iter().filter(|t| t.extra.set.frozen).map(|t| (t.extra.id, t.extra.set.freeze_mode)).collect();
    for (id, mode) in frozen {
        if let Some(f) = freeze_path(app, id, mode).and_then(|p| lg_audio::AudioFile::read(&p).ok()) {
            app.tm.freeze.files.insert(id, FreezeFile { samples: Arc::new(f.stereo_48k()), mode });
        }
    }
}

// ---------------------------------------------------------------------------------------------
// bounce in place

fn audio_dir(app: &App) -> PathBuf {
    match (&app.file, app.project.as_ref().and_then(|p| p.audio_dir.clone())) {
        (_, Some(d)) => d,
        (Some((alt, _)), None) => lg_core::project::package_of(alt).join("Media/Audio Files"),
        _ => std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".local/share/bramble-studio/Bounces"),
    }
}

fn unique(dir: &std::path::Path, base: &str, ext: &str) -> PathBuf {
    let mut path = dir.join(format!("{base}.{ext}"));
    let mut k = 1;
    while path.exists() {
        path = dir.join(format!("{base} {k}.{ext}"));
        k += 1;
    }
    path
}

/// Writes a bounce in place file: 24-bit at the engine's rate.
fn write_bounce(app: &mut App, base: &str, samples: &[f32]) -> Option<(PathBuf, f64)> {
    let dir = audio_dir(app);
    let _ = std::fs::create_dir_all(&dir);
    let path = unique(&dir, base, "wav");
    let mut f = lg_audio::AudioFile::new(RATE, 2, samples.to_vec());
    f.bits = 24;
    if let Err(e) = f.write(&path) {
        app.tell(format!("Couldn't write the bounce: {e}"));
        return None;
    }
    Some((path, samples.len() as f64 / 2.0 / RATE as f64))
}

fn audio_region(name: &str, start: u32, length: u32, file: PathBuf, seconds: f64) -> Region {
    Region { name: name.into(), start, length, notes: Vec::new(), audio: Some(lg_core::project::AudioRef { file, seconds }), muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() }
}

fn ticks_of(app: &App, from: u32, frames: usize) -> u32 {
    // the length in ticks of `frames` rendered from `from` (at the tempo there)
    let secs = frames as f64 / RATE as f64;
    let bpm = app.project.as_ref().map_or(120.0, |p| p.tempo_map.iter().rev().find(|t| t.0 <= from).map_or(p.tempo, |t| t.1));
    (secs * bpm / 60.0 * lg_core::events::PPQ as f64).round() as u32
}

fn default_name(app: &App, regions: bool) -> String {
    let first = if regions {
        app.region_selection().first().and_then(|&(t, r)| app.project.as_ref()?.tracks.get(t)?.regions.get(r).map(|x| x.name.clone()))
    } else {
        app.selected_track.and_then(|t| app.project.as_ref()?.tracks.get(t)?.regions.first().map(|x| x.name.clone()))
    };
    format!("{}_bip", first.unwrap_or_else(|| "Untitled".into()).trim())
}

fn bounceable(app: &App, t: usize) -> bool {
    // (External MIDI tracks have no sound of their own to bounce)
    app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|x| x.extra.set.external.is_none() && x.extra.stack.is_none())
}

/// Bounce Track in Place for track `t` with `o`. Returns the track holding the bounce.
pub fn bounce_track(app: &mut App, t: usize, o: &BipOpts, name: &str) -> Option<usize> {
    if !bounceable(app, t) {
        return None;
    }
    let (from, to) = track_span(app, t)?;
    let what = What { tracks: vec![t], from, to, bypass_fx: o.bypass_fx, fader: o.automation, tail_secs: 6.0, until_silent: true, ..Default::default() };
    let mut samples = job(app, &what).run();
    o.normalize.apply(&mut samples);
    let (path, secs) = write_bounce(app, name, &samples)?;
    let length = ticks_of(app, from, samples.len() / 2).max(to - from);
    let region = audio_region(name, from, length, path, secs);
    let p = app.project.as_mut()?;
    let src = p.tracks[t].clone();
    // volume, pan (and their automation) stay on the strip unless they were rendered in
    let keep_auto: Vec<lg_core::project::AutoLane> = if o.automation { Vec::new() } else { src.automation.iter().filter(|l| l.plugin.is_none() && (l.param == crate::automation::VOLUME || l.param == crate::automation::PAN)).cloned().collect() };
    match o.dest {
        Dest::NewTrack => {
            let mut nt = lg_core::Track::new(&format!("{}_bip", src.name), TrackKind::Audio);
            nt.extra.depth = src.extra.depth;
            nt.regions.push(region);
            if !o.automation {
                nt.volume_db = src.volume_db;
                nt.pan = src.pan;
                nt.automation = keep_auto;
            }
            let at = lg_core::arrange::group(&p.tracks, t).end;
            p.tracks.insert(at, nt);
            app.programs.insert(at.min(app.programs.len()), 0);
            // the original is muted (its regions stay)
            p.tracks[t].mute = true;
            Some(at)
        }
        Dest::Other => {
            // Replace Track: the bounce in its place; the original regions and all automation
            // but volume, pan and sends are gone, the channel strip is reset
            let tr = &mut p.tracks[t];
            tr.kind = TrackKind::Audio;
            tr.regions = vec![region];
            tr.plugins.clear();
            tr.automation = keep_auto;
            tr.extra.midi_fx.clear();
            if o.automation {
                tr.volume_db = 0.0;
                tr.pan = 0.0;
            }
            app.eqs.remove(&t);
            app.instrument_choice.remove(&t);
            Some(t)
        }
    }
}

/// Bounce Regions in Place with `o`: the selected regions rendered into one file, one per
/// track or one per region.
pub fn bounce_regions(app: &mut App, o: &BipOpts) -> bool {
    let mut sel = app.region_selection();
    sel.retain(|&(t, _)| bounceable(app, t));
    sel.sort();
    if sel.is_empty() {
        return false;
    }
    let Some(p) = app.project.as_ref() else { return false };
    let span = |list: &[(usize, usize)]| -> (u32, u32) { (list.iter().map(|&(t, r)| p.tracks[t].regions[r].start).min().unwrap_or(0), list.iter().map(|&(t, r)| p.tracks[t].regions[r].start + p.tracks[t].regions[r].length).max().unwrap_or(0)) };
    // the groups: each becomes one file
    let groups: Vec<Vec<(usize, usize)>> = match o.file_per {
        FilePer::OneFile => vec![sel.clone()],
        FilePer::Track => {
            let mut ts: Vec<usize> = sel.iter().map(|x| x.0).collect();
            ts.dedup();
            ts.into_iter().map(|t| sel.iter().filter(|x| x.0 == t).copied().collect()).collect()
        }
        FilePer::Region => sel.iter().map(|x| vec![*x]).collect(),
    };
    let plans: Vec<(Vec<(usize, usize)>, What)> = groups
        .into_iter()
        .map(|g| {
            let (from, to) = span(&g);
            let mut tracks: Vec<usize> = g.iter().map(|x| x.0).collect();
            tracks.dedup();
            let only = g.iter().map(|&(t, r)| (t, p.tracks[t].regions[r].start, p.tracks[t].regions[r].start + p.tracks[t].regions[r].length)).collect();
            let tail = o.tail_to_file || o.tail_to_region;
            (g, What { tracks, only, from, to, bypass_fx: o.bypass_fx, fader: o.automation, tail_secs: if tail { 8.0 } else { 0.0 }, until_silent: true })
        })
        .collect();
    app.checkpoint("Bounce Regions in Place");
    let selected_track = app.selected_track;
    let mut new_rows: Vec<(usize, Region, lg_core::Track)> = Vec::new();
    for (k, (g, what)) in plans.iter().enumerate() {
        let mut samples = job(app, what).run();
        o.normalize.apply(&mut samples);
        let name = if plans.len() > 1 { format!("{} {}", o.name, k + 1) } else { o.name.clone() };
        let Some((path, secs)) = write_bounce(app, &name, &samples) else { return true };
        // the region covers the range; with Add Effects Tail to Region, the whole file
        let length = if o.tail_to_region { ticks_of(app, what.from, samples.len() / 2).max(what.to - what.from) } else { what.to - what.from };
        let src = app.project.as_ref().map(|p| p.tracks[g[0].0].clone()).unwrap_or_else(|| lg_core::Track::new("", TrackKind::Audio));
        new_rows.push((g[0].0, audio_region(&name, what.from, length, path, secs), src));
    }
    let Some(p) = app.project.as_mut() else { return false };
    // what happens to the sources
    let mut del = sel.clone();
    del.sort_by(|a, b| b.cmp(a));
    for (t, r) in del {
        match o.source {
            Source::Leave => {}
            Source::Mute => p.tracks[t].regions[r].muted = true,
            Source::Delete => {
                p.tracks[t].regions.remove(r);
            }
        }
    }
    match o.dest {
        Dest::Other => {
            // Selected Track: the bounces go on it (the selected regions there are replaced)
            let t = selected_track.unwrap_or(new_rows[0].0).min(p.tracks.len() - 1);
            if o.source == Source::Leave {
                let mine: Vec<usize> = sel.iter().filter(|x| x.0 == t).map(|x| x.1).collect();
                for r in mine.into_iter().rev() {
                    if r < p.tracks[t].regions.len() {
                        p.tracks[t].regions.remove(r);
                    }
                }
            }
            for (_, region, _) in new_rows {
                p.tracks[t].regions.push(region);
            }
            p.tracks[t].regions.sort_by_key(|r| r.start);
        }
        Dest::NewTrack => {
            // one new audio track under each source track (bottom first: indexes above stay)
            new_rows.sort_by(|a, b| b.0.cmp(&a.0));
            let mut made: HashMap<usize, usize> = HashMap::new();
            for (t, region, src) in new_rows {
                if let Some(&at) = made.get(&t) {
                    p.tracks[at].regions.push(region);
                    continue;
                }
                let mut nt = lg_core::Track::new(&format!("{}_bip", src.name), TrackKind::Audio);
                nt.extra.depth = src.extra.depth;
                if !o.automation {
                    nt.volume_db = src.volume_db;
                    nt.pan = src.pan;
                }
                nt.regions.push(region);
                let at = lg_core::arrange::group(&p.tracks, t).end;
                p.tracks.insert(at, nt);
                app.programs.insert(at.min(app.programs.len()), 0);
                made.insert(t, at);
            }
        }
    }
    app.selected_region = None;
    app.selected_regions.clear();
    app.after_edit();
    app.tell("Bounced in place");
    true
}

/// Bounce and Replace All Tracks: every audio, software instrument and Drummer track becomes
/// its bounce.
pub fn replace_all(app: &mut App, o: &BipOpts) -> usize {
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    app.checkpoint("Bounce and Replace All Tracks");
    let mut done = 0;
    for t in 0..n {
        let name = app.project.as_ref().map(|p| format!("{}_bip", p.tracks[t].name)).unwrap_or_default();
        let opts = BipOpts { dest: Dest::Other, ..o.clone() };
        if bounce_track(app, t, &opts, &name).is_some() {
            done += 1;
        }
    }
    app.after_edit();
    done
}

// ---------------------------------------------------------------------------------------------
// export

/// The WAV 'acid' chunk carrying the tempo (Include Tempo Information).
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

fn now() -> [u32; 6] {
    civil(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()))
}

fn pattern_with_custom(o: &ExportOpts) -> Vec<NameElem> {
    o.pattern.iter().map(|e| if let NameElem::Custom(_) = e { NameElem::Custom(o.custom.clone()) } else { e.clone() }).collect()
}

fn write_export(app: &App, o: &ExportOpts, dir: &std::path::Path, name: &str, mut samples: Vec<f32>, from: u32, to: u32) -> Option<PathBuf> {
    o.normalize.apply(&mut samples);
    let ext = if o.aiff { "aif" } else { "wav" };
    let path = unique(dir, if name.is_empty() { "Untitled" } else { name }, ext);
    let mut f = lg_audio::AudioFile::new(RATE, 2, samples);
    f.format = if o.aiff { lg_audio::Format::Aiff } else { lg_audio::Format::Wav };
    f.bits = o.bits;
    f.float = o.bits == 32;
    if o.tempo_info && !o.aiff {
        if let Some(p) = app.project.as_ref() {
            let beats = (to.saturating_sub(from)) / lg_core::events::PPQ;
            f.extra.push((*b"acid", acid_chunk(p.tempo, beats, p.signature)));
        }
    }
    f.write(&path).ok()?;
    Some(path)
}

/// Exports `tracks` as audio files into `dir`; the files written.
pub fn export_tracks(app: &mut App, tracks: &[usize], o: &ExportOpts, dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let end = project_end(app);
    let pattern = pattern_with_custom(o);
    for (k, &t) in tracks.iter().enumerate() {
        if !bounceable(app, t) {
            continue;
        }
        let (from, to) = match o.range {
            Range::CycleOnly => app.cycle,
            _ => (0, end),
        };
        let what = What { tracks: vec![t], from, to, bypass_fx: o.bypass_fx, fader: o.automation, tail_secs: if o.tail { 8.0 } else { 0.0 }, until_silent: true, ..Default::default() };
        let mut samples = job(app, &what).run();
        if o.range == Range::TrimSilence {
            let frames = lg_core::trackmix::sound_end(&samples, 1e-4);
            samples.truncate(frames * 2);
        }
        let Some(p) = app.project.as_ref() else { break };
        let tr = &p.tracks[t];
        let bar = app.bar_ticks().max(1);
        let ctx = NameCtx { project: p.name.clone(), alternative: tr.extra.alt_name.clone(), track: tr.name.clone(), track_number: t + 1, group: app.mix.group_of.get(&t).and_then(|g| app.mix.groups.get(*g)).map(|g| g.name.clone()).unwrap_or_default(), region: String::new(), bars: (from / bar + 1, to / bar + 1), seconds: (app.seconds_at(from as f64), app.seconds_at(to as f64)), increment: k + 1, now: now() };
        let name = build_name(&pattern, &ctx);
        if let Some(path) = write_export(app, o, dir, &name, samples, from, to) {
            out.push(path);
        }
    }
    if o.add_to_browser && !out.is_empty() {
        crate::projaudio::add_files(app, out.clone());
    }
    out
}

/// Exports the selected regions as audio files (each with its fades and its track's sound).
pub fn export_regions(app: &mut App, o: &ExportOpts, dir: &std::path::Path) -> Vec<PathBuf> {
    let mut sel = app.region_selection();
    sel.sort();
    let pattern = pattern_with_custom(o);
    let mut out = Vec::new();
    for (k, &(t, r)) in sel.iter().enumerate() {
        if !bounceable(app, t) {
            continue;
        }
        let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r).cloned()) else { continue };
        let (from, to) = (rg.start, rg.start + rg.length);
        let what = What { tracks: vec![t], only: vec![(t, from, to)], from, to, bypass_fx: o.bypass_fx, fader: o.automation, tail_secs: if o.tail { 8.0 } else { 0.0 }, until_silent: true };
        let samples = job(app, &what).run();
        let Some(p) = app.project.as_ref() else { break };
        let tr = &p.tracks[t];
        let ctx = NameCtx { project: p.name.clone(), alternative: tr.extra.alt_name.clone(), track: tr.name.clone(), track_number: t + 1, region: rg.name.clone(), increment: k + 1, now: now(), ..Default::default() };
        let name = build_name(&pattern, &ctx);
        if let Some(path) = write_export(app, o, dir, &name, samples, from, to) {
            out.push(path);
        }
    }
    if o.add_to_browser && !out.is_empty() {
        crate::projaudio::add_files(app, out.clone());
    }
    out
}

// ---------------------------------------------------------------------------------------------
// commands and dialogs

pub fn command(app: &mut App, _ctx: &egui::Context, name: &str) -> bool {
    match name {
        "Track in Place…" | "Bounce Track in Place…" => {
            if app.selected_track.is_none() {
                return false;
            }
            app.tm.freeze.track.name = default_name(app, false);
            app.tm.freeze.dialog = Some(Dialog::Track);
        }
        "Regions in Place…" => {
            if app.region_selection().is_empty() {
                return false;
            }
            app.tm.freeze.regions.name = default_name(app, true);
            app.tm.freeze.dialog = Some(Dialog::Regions);
        }
        "And Replace All Tracks…" | "Bounce and Replace All Tracks…" => app.tm.freeze.dialog = Some(if app.dirty && app.file.is_some() { Dialog::AllAsk } else { Dialog::All }),
        "All Tracks as Audio Files…" | "Export All Tracks as Audio Files…" => app.tm.freeze.dialog = Some(Dialog::ExportTracks { all: true }),
        "Regions as Audio Files…" | "Export Regions as Audio Files…" => {
            if app.region_selection().is_empty() {
                return false;
            }
            app.tm.freeze.export_regions.pattern = vec![NameElem::RegionName];
            app.tm.freeze.dialog = Some(Dialog::ExportRegions);
        }
        "Refresh Freeze Files" => {
            app.tm.freeze.files.clear();
            for i in stale(app) {
                render_freeze(app, i);
            }
            crate::mixengine::sync(app);
            app.tell("Freeze files refreshed");
        }
        _ => return false,
    }
    true
}

/// Opens the export dialog for the selected tracks (File ▸ Export ▸ 1 Track as Audio File…).
pub fn open_export_selected(app: &mut App) {
    app.tm.freeze.dialog = Some(Dialog::ExportTracks { all: false });
}

fn normalize_ui(ui: &mut egui::Ui, id: &str, n: &mut Normalize) {
    ui.horizontal(|ui| {
        ui.label("Normalize:");
        egui::ComboBox::from_id_salt(id).selected_text(n.name()).show_ui(ui, |ui| {
            for x in Normalize::ALL {
                ui.selectable_value(n, x, x.name());
            }
        });
    });
}

fn bip_ui(ui: &mut egui::Ui, o: &mut BipOpts, kind: Dialog) {
    if kind != Dialog::All {
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.add(egui::TextEdit::singleline(&mut o.name).desired_width(240.0));
        });
        ui.horizontal(|ui| {
            ui.label("Destination:");
            ui.selectable_value(&mut o.dest, Dest::NewTrack, "New Track");
            ui.selectable_value(&mut o.dest, Dest::Other, if kind == Dialog::Track { "Replace Track" } else { "Selected Track" });
        });
    }
    if kind == Dialog::Regions {
        ui.horizontal(|ui| {
            ui.label("Destination File:");
            ui.selectable_value(&mut o.file_per, FilePer::OneFile, "One File");
            ui.selectable_value(&mut o.file_per, FilePer::Track, "One File per Track");
            ui.selectable_value(&mut o.file_per, FilePer::Region, "One File per Region");
        });
        ui.add_enabled_ui(o.dest == Dest::NewTrack, |ui| {
            ui.horizontal(|ui| {
                ui.label("Source:");
                ui.selectable_value(&mut o.source, Source::Leave, "Leave");
                ui.selectable_value(&mut o.source, Source::Mute, "Mute");
                ui.selectable_value(&mut o.source, Source::Delete, "Delete");
            });
        });
    }
    ui.separator();
    if kind != Dialog::All {
        ui.checkbox(&mut o.multi_outputs, "Include Instrument Multi-Outputs");
        ui.add_enabled(o.multi_outputs, egui::Checkbox::new(&mut o.as_additional_tracks, "As Additional Tracks"));
    }
    ui.checkbox(&mut o.bypass_fx, "Bypass Effect Plug-ins");
    if kind == Dialog::Regions {
        ui.checkbox(&mut o.tail_to_file, "Add Effects Tail to File");
        ui.checkbox(&mut o.tail_to_region, "Add Effects Tail to Region");
    }
    ui.checkbox(&mut o.automation, "Include Volume/Pan Automation");
    normalize_ui(ui, "bip-normalize", &mut o.normalize);
}

fn export_ui(ui: &mut egui::Ui, o: &mut ExportOpts, regions: bool, example: &NameCtx) {
    ui.horizontal(|ui| {
        ui.label("File Type:");
        egui::ComboBox::from_id_salt("exp-type").selected_text(if o.aiff { "AIFF" } else { "WAVE" }).show_ui(ui, |ui| {
            ui.selectable_value(&mut o.aiff, false, "WAVE");
            ui.selectable_value(&mut o.aiff, true, "AIFF");
        });
        ui.label("Bit Depth:");
        egui::ComboBox::from_id_salt("exp-bits").selected_text(if o.bits == 32 { "32-bit Float".to_string() } else { format!("{}-bit", o.bits) }).show_ui(ui, |ui| {
            ui.selectable_value(&mut o.bits, 16, "16-bit");
            ui.selectable_value(&mut o.bits, 24, "24-bit");
            ui.selectable_value(&mut o.bits, 32, "32-bit Float");
        });
    });
    if !regions {
        ui.horizontal(|ui| {
            ui.label("Range:");
            egui::ComboBox::from_id_salt("exp-range").selected_text(match o.range { Range::TrimSilence => "Trim Silence at File End", Range::CycleOnly => "Export Cycle Range Only", Range::ToProjectEnd => "Extend File Length to Project End" }).show_ui(ui, |ui| {
                ui.selectable_value(&mut o.range, Range::TrimSilence, "Trim Silence at File End");
                ui.selectable_value(&mut o.range, Range::CycleOnly, "Export Cycle Range Only");
                ui.selectable_value(&mut o.range, Range::ToProjectEnd, "Extend File Length to Project End");
            });
        });
    }
    ui.horizontal(|ui| {
        ui.label("Multi-Output Software Instruments:");
        egui::ComboBox::from_id_salt("exp-multi").selected_text(if o.per_channel_strip { "One File per Channel Strip" } else { "One File per Track" }).show_ui(ui, |ui| {
            ui.selectable_value(&mut o.per_channel_strip, false, "One File per Track");
            ui.selectable_value(&mut o.per_channel_strip, true, "One File per Channel Strip");
        });
    });
    ui.checkbox(&mut o.bypass_fx, "Bypass Effect Plug-ins");
    ui.checkbox(&mut o.tail, "Include Audio Tail");
    ui.checkbox(&mut o.automation, "Include Volume/Pan Automation");
    ui.checkbox(&mut o.tempo_info, "Include Tempo Information");
    normalize_ui(ui, "exp-normalize", &mut o.normalize);
    ui.checkbox(&mut o.add_to_browser, "Add resulting files to Project Audio Browser");
    ui.separator();
    // the Pattern: the elements in use (click one to take it out), then those to add
    ui.label("Pattern:");
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        for (k, e) in o.pattern.iter().enumerate() {
            if ui.add(egui::Button::new(e.label()).fill(look::accent().gamma_multiply(0.5)).corner_radius(9.0)).on_hover_text("Click to remove").clicked() {
                remove = Some(k);
            }
        }
        if o.pattern.is_empty() {
            ui.label(egui::RichText::new("Add elements from below").color(look::text_dim()));
        }
    });
    if let Some(k) = remove {
        o.pattern.remove(k);
    }
    ui.label(egui::RichText::new("Filename elements (click to add):").color(look::text_dim()));
    ui.horizontal_wrapped(|ui| {
        for e in NameElem::choices(regions) {
            if ui.add(egui::Button::new(e.label()).corner_radius(9.0)).clicked() {
                o.pattern.push(e);
            }
        }
    });
    if o.pattern.iter().any(|e| matches!(e, NameElem::Custom(_))) {
        ui.horizontal(|ui| {
            ui.label("Custom:");
            ui.add(egui::TextEdit::singleline(&mut o.custom).desired_width(200.0));
        });
    }
    let name = build_name(&pattern_with_custom(o), example);
    ui.label(egui::RichText::new(format!("Example: {}.{}", if name.is_empty() { "Untitled" } else { &name }, if o.aiff { "aif" } else { "wav" })).color(look::text_dim()));
}

fn pick_folder(app: &App, o: &ExportOpts) -> Option<PathBuf> {
    if app.automated {
        return o.folder.clone();
    }
    kn_ui::files::FileDialog::new().set_title("Export").pick_folder()
}

pub fn windows(app: &mut App, ctx: &egui::Context) {
    let Some(kind) = app.tm.freeze.dialog else { return };
    let mut close = false;
    let mut go = false;
    let title = match kind {
        Dialog::Track => "Bounce Track In Place",
        Dialog::Regions => "Bounce Regions In Place",
        Dialog::AllAsk | Dialog::All => "Bounce Replace All Tracks",
        Dialog::ExportTracks { all: true } => "Export All Tracks as Audio Files",
        Dialog::ExportTracks { all: false } => "Export Track(s) as Audio Files",
        Dialog::ExportRegions => "Export Regions as Audio Files",
    };
    let mut save_first: Option<bool> = None;
    let example = {
        let t = app.selected_track.unwrap_or(0);
        let p = app.project.as_ref();
        let (rt, rr) = app.region_selection().first().copied().unwrap_or((t, 0));
        NameCtx { project: p.map(|p| p.name.clone()).unwrap_or_default(), track: p.and_then(|p| p.tracks.get(t)).map(|x| x.name.clone()).unwrap_or_default(), track_number: t + 1, region: p.and_then(|p| p.tracks.get(rt)?.regions.get(rr)).map(|x| x.name.clone()).unwrap_or_default(), bars: (1, 9), seconds: (0.0, 16.0), increment: 1, now: now(), ..Default::default() }
    };
    egui::Window::new(title).id(egui::Id::unique("tm-bounce-dialog")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
        ui.set_width(440.0);
        let st = &mut app.tm.freeze;
        match kind {
            Dialog::AllAsk => {
                ui.label("Bounce and Replace All Tracks changes every track.\nDo you want to save the project first?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    if ui.button("Don’t Save").clicked() {
                        save_first = Some(false);
                    }
                    if ui.add(egui::Button::new(egui::RichText::new("Save").color(egui::Color32::WHITE)).fill(look::accent())).clicked() {
                        save_first = Some(true);
                    }
                });
                return;
            }
            Dialog::Track => bip_ui(ui, &mut st.track, kind),
            Dialog::Regions => bip_ui(ui, &mut st.regions, kind),
            Dialog::All => bip_ui(ui, &mut st.all, kind),
            Dialog::ExportTracks { .. } => export_ui(ui, &mut st.export, false, &example),
            Dialog::ExportRegions => export_ui(ui, &mut st.export_regions, true, &example),
        }
        ui.separator();
        ui.horizontal(|ui| {
            if matches!(kind, Dialog::Track | Dialog::Regions | Dialog::All) && ui.button("Restore Defaults").clicked() {
                let name = match kind { Dialog::Track => st.track.name.clone(), Dialog::Regions => st.regions.name.clone(), _ => String::new() };
                let d = BipOpts { name, ..Default::default() };
                match kind {
                    Dialog::Track => st.track = d,
                    Dialog::Regions => st.regions = d,
                    _ => st.all = d,
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if matches!(kind, Dialog::ExportTracks { .. } | Dialog::ExportRegions) { "Export" } else { "OK" };
                if ui.add(egui::Button::new(egui::RichText::new(label).color(egui::Color32::WHITE)).fill(look::accent()).min_size(vec2(76.0, 24.0))).clicked() {
                    go = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
    });
    if let Some(save) = save_first {
        if save {
            app.save();
        }
        app.tm.freeze.dialog = Some(Dialog::All);
        return;
    }
    if go {
        app.tm.freeze.dialog = None;
        match kind {
            Dialog::Track => {
                let o = app.tm.freeze.track.clone();
                let tracks = crate::trackarea::selection(app);
                app.checkpoint("Bounce Track in Place");
                // bottom first, so the indexes above stay right
                let mut last = None;
                for (k, &t) in tracks.iter().enumerate().rev() {
                    let name = if tracks.len() > 1 { format!("{} {}", o.name, k + 1) } else { o.name.clone() };
                    last = bounce_track(app, t, &o, &name).or(last);
                }
                app.after_edit();
                match last {
                    Some(t) => {
                        app.selected_track = Some(t);
                        app.tell("Bounced in place");
                    }
                    None => app.tell("Nothing to bounce on the selected track"),
                }
            }
            Dialog::Regions => {
                let o = app.tm.freeze.regions.clone();
                bounce_regions(app, &o);
            }
            Dialog::All => {
                let o = app.tm.freeze.all.clone();
                let n = replace_all(app, &o);
                app.tell(format!("{n} tracks bounced and replaced"));
            }
            Dialog::ExportTracks { all } => {
                let o = app.tm.freeze.export.clone();
                if let Some(dir) = pick_folder(app, &o) {
                    let tracks: Vec<usize> = if all { (0..app.project.as_ref().map_or(0, |p| p.tracks.len())).collect() } else { crate::trackarea::selection(app) };
                    let n = export_tracks(app, &tracks, &o, &dir).len();
                    app.tell(format!("{n} audio files exported"));
                }
            }
            Dialog::ExportRegions => {
                let o = app.tm.freeze.export_regions.clone();
                if let Some(dir) = pick_folder(app, &o) {
                    let n = export_regions(app, &o, &dir).len();
                    app.tell(format!("{n} audio files exported"));
                }
            }
            Dialog::AllAsk => {}
        }
    } else if close {
        app.tm.freeze.dialog = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loud(s: &[f32]) -> bool {
        s.iter().any(|x| x.abs() > 1e-3)
    }

    fn inst_track(app: &App) -> usize {
        let p = app.project.as_ref().unwrap();
        p.tracks.iter().position(|t| t.kind != TrackKind::Audio && t.regions.iter().any(|r| !r.notes.is_empty())).expect("the demo song has a MIDI track")
    }

    #[test]
    fn a_frozen_track_plays_its_freeze_file() {
        let mut app = crate::tm::test_app();
        let t = inst_track(&app);
        set_frozen(&mut app, t, true);
        assert_eq!(stale(&app), vec![t]);
        render_freeze(&mut app, t);
        assert!(stale(&app).is_empty());
        let id = app.project.as_ref().unwrap().tracks[t].extra.id;
        assert!(loud(&app.tm.freeze.files[&id].samples), "the freeze file has the track's sound");
        crate::mixengine::sync(&app);
        assert!(app.engine.lock().unwrap().ext.tracks[t].frozen.is_some());
        // the other mode: the file is stale again
        crate::tm::set_mut(&mut app, t).unwrap().freeze_mode = FreezeMode::SourceOnly;
        assert_eq!(stale(&app), vec![t]);
        set_frozen(&mut app, t, false);
        assert!(app.tm.freeze.files.is_empty());
        crate::mixengine::sync(&app);
        assert!(app.engine.lock().unwrap().ext.tracks[t].frozen.is_none());
    }

    #[test]
    fn bounce_track_in_place_makes_an_audio_track_or_replaces() {
        let mut app = crate::tm::test_app();
        let t = inst_track(&app);
        let n = app.project.as_ref().unwrap().tracks.len();
        let dir = std::env::temp_dir().join(format!("lg-bip-{}", std::process::id()));
        app.project.as_mut().unwrap().audio_dir = Some(dir.clone());
        let o = BipOpts::default();
        let at = bounce_track(&mut app, t, &o, "Lead_bip").unwrap();
        app.after_edit();
        let p = app.project.as_ref().unwrap();
        assert_eq!(p.tracks.len(), n + 1);
        assert_eq!(p.tracks[at].kind, TrackKind::Audio);
        let file = p.tracks[at].regions[0].audio.as_ref().unwrap().file.clone();
        let f = lg_audio::AudioFile::read(&file).unwrap();
        assert_eq!((f.bits, f.rate), (24, RATE));
        assert!(loud(&f.data));
        assert!(p.tracks[t].mute, "the source track is muted");
        // Replace Track
        let o = BipOpts { dest: Dest::Other, ..Default::default() };
        app.project.as_mut().unwrap().tracks[t].mute = false;
        app.load_engine();
        let at = bounce_track(&mut app, t, &o, "Lead_bip").unwrap();
        assert_eq!(at, t);
        let p = app.project.as_ref().unwrap();
        assert_eq!((p.tracks[t].kind, p.tracks[t].regions.len()), (TrackKind::Audio, 1));
        assert!(p.tracks[t].plugins.is_empty(), "the channel strip is reset");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn exports_tracks_with_the_pattern_names() {
        let mut app = crate::tm::test_app();
        let t = inst_track(&app);
        let dir = std::env::temp_dir().join(format!("lg-export-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let o = ExportOpts { bits: 16, pattern: vec![NameElem::Custom(String::new()), NameElem::TrackNumber, NameElem::TrackName], custom: "Stem".into(), ..Default::default() };
        let files = export_tracks(&mut app, &[t], &o, &dir);
        assert_eq!(files.len(), 1);
        let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
        let track = app.project.as_ref().unwrap().tracks[t].name.clone();
        assert_eq!(name, format!("Stem_{:02}_{}.wav", t + 1, track));
        let f = lg_audio::AudioFile::read(&files[0]).unwrap();
        assert_eq!(f.bits, 16);
        assert!(f.extra.iter().any(|c| &c.0 == b"acid"), "the tempo is in the file");
        assert!(loud(&f.data));
        // trimmed: no long silence at the end
        let end = lg_core::trackmix::sound_end(&f.data, 1e-3);
        assert!(f.frames() - end < RATE as usize / 2);
        let o = ExportOpts { aiff: true, ..o };
        let files = export_tracks(&mut app, &[t], &o, &dir);
        assert!(files[0].extension().is_some_and(|e| e == "aif"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prepare_solos_the_strip() {
        let pt = |notes: Vec<(u32, u32, u8, u8)>| PlayTrack { channel: 0, program: 0, notes, audio: Vec::new(), volume: 0.5, pan: 0.3, mute: false, solo: true, eq: None, instrument: None, instrument_floats: Vec::new(), inserts: vec![("Compressor".into(), Vec::new())], sends: vec![audio::Send { bus: 0, gain: 1.0, pre: false }], auto_volume: vec![(0, 0.2)], auto_pan: Vec::new() };
        let tracks = vec![pt(vec![(0, 10, 60, 100), (500, 600, 62, 100)]), pt(vec![(0, 10, 64, 100)])];
        let what = What { tracks: vec![0], only: vec![(0, 400, 800)], bypass_fx: true, ..Default::default() };
        let out = prepare(tracks, &what);
        assert!(out[1].mute && !out[0].mute && !out[0].solo);
        assert_eq!(out[0].notes, vec![(500, 600, 62, 100)]);
        assert!(out[0].inserts.is_empty() && out[0].sends.is_empty());
        assert_eq!((out[0].volume, out[0].pan, out[0].auto_volume.len()), (1.0, 0.0, 0));
    }
}
