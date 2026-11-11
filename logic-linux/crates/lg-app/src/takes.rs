//! Take folders in the tracks area (Logic Pro 13): a folder over its pieces (what plays: the
//! active take, or a comp of sections from several), its header with the disclosure triangle and
//! the take folder menu; opened, its take lanes under it, where clicking a take plays it whole
//! and dragging over one (Quick Swipe Comping) puts that part of it into the comp. The take
//! commands (Flatten, Flatten and Merge, Unpack, Export / Move to New Track, rename, delete,
//! next / previous take) and Pack Take Folder.
//!
//! The model is lg-core's `takes`; the folder's pieces are ordinary regions on the track (so they
//! play, draw and save as regions do), made again from the folder whenever it changes (`sync`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::takes::{Active, Section, TakeFolder};
use lg_core::{Region, TrackKind};

use crate::app::App;
use crate::look;

const TAKE_H: f32 = 34.0;

pub struct Ui {
    /// Quick Swipe Comping (⌥Q): dragging over a take lane comps; Snap Quick Swipe Comping.
    pub quick_swipe: bool,
    pub snap: bool,
    /// The folder the take commands act on (the last one clicked).
    pub focus: Option<usize>,
    /// The take or comp being renamed (⇧T): folder, the text so far.
    pub renaming: Option<(usize, String)>,
    swipe: Option<Swipe>,
    /// Waveforms of the takes' files: a peak per PEAK frames, the file's rate.
    peaks: HashMap<PathBuf, Arc<(Vec<f32>, u32)>>,
}

impl Default for Ui {
    fn default() -> Self {
        Ui { quick_swipe: true, snap: false, focus: None, renaming: None, swipe: None, peaks: HashMap::new() }
    }
}

struct Swipe {
    folder: usize,
    take: usize,
    from_tick: f32,
    before: TakeFolder,
}

const PEAK: usize = 256;

fn folders(app: &App) -> &[TakeFolder] {
    app.project.as_ref().map_or(&[], |p| p.takes.as_slice())
}

fn folder_mut(app: &mut App, fi: usize) -> Option<&mut TakeFolder> {
    app.project.as_mut()?.takes.get_mut(fi)
}

/// The region is one of folder `f`'s pieces.
fn owned_by(f: &TakeFolder, r: &Region) -> bool {
    if !f.overlaps(r.start, r.start + r.length) {
        return false;
    }
    match &r.audio {
        Some(a) => f.takes.iter().any(|t| t.audio.as_ref().is_some_and(|x| x.file == a.file)),
        None => f.is_midi() && r.start >= f.start && r.start + r.length <= f.end(),
    }
}

/// Region `r` on track `t` belongs to a take folder.
pub fn owned(app: &App, t: usize, r: usize) -> bool {
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)) else { return false };
    folders(app).iter().any(|f| f.track == t && owned_by(f, rg))
}

fn folder_of(app: &App, t: usize, r: usize) -> Option<usize> {
    let rg = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?;
    folders(app).iter().position(|f| f.track == t && owned_by(f, rg))
}

/// A take folder on `t` overlapping [a, b).
pub fn folder_at(app: &App, t: usize, a: u32, b: u32) -> Option<usize> {
    folders(app).iter().position(|f| f.track == t && f.overlaps(a, b))
}

/// The folder the take commands act on: the selected region's, else the one last clicked, else
/// the selected track's under the playhead.
fn focused(app: &App) -> Option<usize> {
    if let Some(f) = app.selected_region.and_then(|(t, r)| folder_of(app, t, r)) {
        return Some(f);
    }
    if let Some(f) = app.rec.takes.focus.filter(|&f| f < folders(app).len()) {
        return Some(f);
    }
    let t = app.selected_track?;
    let pos = app.position() as u32;
    folders(app).iter().position(|f| f.track == t && f.start <= pos && pos < f.end())
}

/// The file's sample rate (WAV / AIFF header), else Studio's.
fn file_rate(path: &Path) -> u32 {
    use std::io::Read;
    let mut b = vec![0u8; 4096];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut b)).unwrap_or(0);
    let b = &b[..n];
    if b.len() > 12 && &b[..4] == b"RIFF" {
        let mut i = 12;
        while i + 8 <= b.len() {
            let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
            if &b[i..i + 4] == b"fmt " && i + 16 <= b.len() {
                return u32::from_le_bytes([b[i + 12], b[i + 13], b[i + 14], b[i + 15]]);
            }
            i += 8 + len + len % 2;
        }
    } else if b.len() > 12 && &b[..4] == b"FORM" {
        let mut i = 12;
        while i + 8 <= b.len() {
            let len = u32::from_be_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
            if &b[i..i + 4] == b"COMM" && i + 26 <= b.len() {
                // 80-bit extended
                let e = &b[i + 16..i + 26];
                let exp = (((e[0] as i32) & 0x7f) << 8 | e[1] as i32) - 16383;
                let mant = u64::from_be_bytes([e[2], e[3], e[4], e[5], e[6], e[7], e[8], e[9]]);
                return (mant as f64 * 2f64.powi(exp - 63)).round() as u32;
            }
            i += 8 + len + len % 2;
        }
    }
    crate::audio::RATE
}

/// Removes regions `idx` from track `t`, the audio editors' per-region settings following the
/// regions that stay.
pub fn remove_regions(app: &mut App, t: usize, mut idx: Vec<usize>) {
    idx.sort();
    idx.dedup();
    if idx.is_empty() {
        return;
    }
    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
        for &i in idx.iter().rev() {
            if i < tr.regions.len() {
                tr.regions.remove(i);
            }
        }
    }
    let old = std::mem::take(&mut app.audio_edit.regions);
    for ((tt, rr), v) in old {
        if tt != t {
            app.audio_edit.regions.insert((tt, rr), v);
        } else if !idx.contains(&rr) {
            let shift = idx.iter().filter(|&&i| i < rr).count();
            app.audio_edit.regions.insert((tt, rr - shift), v);
        }
    }
    let fix = |s: (usize, usize)| -> Option<(usize, usize)> {
        if s.0 != t {
            return Some(s);
        }
        (!idx.contains(&s.1)).then(|| (s.0, s.1 - idx.iter().filter(|&&i| i < s.1).count()))
    };
    app.selected_region = app.selected_region.and_then(fix);
    app.selected_regions = app.selected_regions.iter().filter_map(|&s| fix(s)).collect();
}

/// Adds a region to `t`, starting `offset` frames into its file; its index.
fn add_region(app: &mut App, t: usize, r: Region, offset: usize) -> Option<usize> {
    let file = r.audio.as_ref().map(|a| a.file.clone());
    let tr = app.project.as_mut()?.tracks.get_mut(t)?;
    tr.regions.push(r);
    let i = tr.regions.len() - 1;
    // (a stale entry for this index from a region gone earlier)
    app.audio_edit.regions.remove(&(t, i));
    if let (Some(file), true) = (file, offset > 0) {
        app.audio_edit.regions.insert((t, i), crate::audioedit::RegionAudio { file, offset, ..Default::default() });
    }
    Some(i)
}

/// A piece of the folder as a region, and its offset into its file (frames).
fn piece_region(app: &App, f: &TakeFolder, s: Section) -> (Region, usize) {
    let name = format!("{}: {}", f.name, f.active_name());
    let take = &f.takes[s.take];
    if take.audio.is_none() {
        return (f.midi_region(s, &name), 0);
    }
    let mut r = take.clone();
    r.name = name;
    r.start = s.start;
    r.length = s.end - s.start;
    r.source = None;
    r.muted = false;
    let rate = take.audio.as_ref().map_or(crate::audio::RATE, |a| file_rate(&a.file));
    let off = ((app.seconds_at(s.start as f64) - app.seconds_at(take.start as f64)).max(0.0) * rate as f64).round() as usize;
    (r, off)
}

/// The folder's pieces made again from what it plays.
pub fn sync(app: &mut App, fi: usize) {
    let Some(f) = folders(app).get(fi).cloned() else { return };
    let t = f.track;
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return };
    let mine: Vec<usize> = tr.regions.iter().enumerate().filter(|(_, r)| owned_by(&f, r)).map(|(i, _)| i).collect();
    remove_regions(app, t, mine);
    let mut first = None;
    for s in f.pieces() {
        let (r, off) = piece_region(app, &f, s);
        let i = add_region(app, t, r, off);
        first = first.or(i);
    }
    if let Some(i) = first {
        app.selected_region = Some((t, i));
        app.selected_track = Some(t);
    }
    app.rec.takes.focus = Some(fi);
}

/// A region as a take: the whole of its file from where it starts in it (a trimmed region's
/// take begins earlier), and the part of the track it showed.
fn as_take(app: &App, t: usize, r: usize) -> Option<(Region, (u32, u32))> {
    let rg = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?.clone();
    let shown = (rg.start, rg.start + rg.length);
    let mut take = rg;
    take.muted = false;
    if let Some(a) = &take.audio {
        let off = crate::audioedit::extras(app, t, r).offset;
        if off > 0 {
            let secs = app.seconds_at(take.start as f64) - off as f64 / file_rate(&a.file) as f64;
            let s = crate::recording::tick_at(app, secs.max(0.0)).round() as u32;
            take.length += take.start - s.min(take.start);
            take.start = s.min(take.start);
        }
    }
    Some((take, shown))
}

/// A new take folder on `t` from its regions `absorb` (they become its first takes) and the
/// new `takes`.
pub fn make_folder(app: &mut App, t: usize, absorb: Vec<usize>, takes: Vec<Region>) {
    let name = app.project.as_ref().and_then(|p| p.tracks.get(t)).map_or(String::new(), |tr| tr.name.clone());
    let mut old: Vec<(Region, (u32, u32))> = absorb.iter().filter_map(|&r| as_take(app, t, r)).collect();
    old.sort_by_key(|x| x.1 .0);
    remove_regions(app, t, absorb);
    let mut all: Vec<Region> = Vec::new();
    let mut shown: Option<(u32, u32)> = None;
    for (r, s) in old {
        shown = Some(shown.map_or(s, |x| (x.0.min(s.0), x.1.max(s.1))));
        all.push(r);
    }
    let first_new = all.len();
    all.extend(takes);
    let mut it = all.into_iter();
    let Some(first) = it.next() else { return };
    let mut f = TakeFolder::new(t, &name, first);
    // (an old trimmed region: the folder shows what it showed)
    if let Some((a, b)) = shown.filter(|_| first_new > 0) {
        f.start = a;
        f.length = b - a;
        if first_new == 1 {
            f.comps.clear();
        }
    }
    for r in it {
        f.add_take(r);
    }
    let Some(p) = app.project.as_mut() else { return };
    p.takes.push(f);
    let fi = p.takes.len() - 1;
    sync(app, fi);
}

/// New takes in folder `fi` (a recording over it).
pub fn add_takes(app: &mut App, fi: usize, takes: Vec<Region>) {
    let Some(f) = folder_mut(app, fi) else { return };
    for r in takes {
        f.add_take(r);
    }
    sync(app, fi);
}

/// Replace mode, audio: what the recording covers taken out of the regions on `t` (cut at its
/// edges: a region running past it keeps its end, from later in its file).
pub fn cut_out(app: &mut App, t: usize, a: u32, b: u32) {
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return };
    let over: Vec<usize> = tr.regions.iter().enumerate().filter(|(ri, r)| r.start < b && r.start + r.length > a && !owned(app, t, *ri)).map(|(i, _)| i).collect();
    let mut gone = Vec::new();
    let mut add = Vec::new();
    for i in over {
        let rg = app.project.as_ref().unwrap().tracks[t].regions[i].clone();
        let end = rg.start + rg.length;
        let x = crate::audioedit::extras(app, t, i);
        if end > b {
            let mut right = rg.clone();
            right.source = None;
            right.origin = rg.source.or(rg.origin);
            right.start = b;
            right.length = end - b;
            let rate = rg.audio.as_ref().map_or(crate::audio::RATE, |au| file_rate(&au.file)) as f64;
            let extra = ((app.seconds_at(b as f64) - app.seconds_at(rg.start as f64)) * rate).round() as usize;
            add.push((right, x.offset + extra));
        }
        if rg.start < a {
            if let Some(r) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)?.regions.get_mut(i)) {
                r.length = a - r.start;
            }
        } else {
            gone.push(i);
        }
    }
    remove_regions(app, t, gone);
    for (r, off) in add {
        add_region(app, t, r, off);
    }
}

// =============================================================================================
// commands

/// The take commands run here (menus.rs enables them; tool/checklist.py counts them).
pub const COMMANDS: &[&str] = &[
    "Pack Take Folder",
    "Un/disclose Take Folder",
    "Flatten Take Folder/Cell",
    "Flatten and Merge Take Folder/Cell",
    "Create New Empty MIDI Take",
    "Duplicate Active MIDI Take",
    "Rename Take or Comp",
    "Delete Take or Comp",
    "Export Active Take or Comp to New Track",
    "Move Active Take or Comp to New Track",
    "Toggle Take Folder Quick Swipe Comping Mode",
    "Create New Comp",
    "Select Previous Take or Comp",
    "Select Next Take or Comp",
    "Trim to Active Comp Sections",
    "Toggle Snap Quick Swipe Comping",
    "Snap Quick Swipe Comping",
    "Unpack Take Folder to Independent Tracks or Unpack Folder/Take Cell to New Tracks",
    "Unpack Take Folder to Independent Tracks (mute inactive) or Unpack Folder/Take Cell to Existing Tracks",
];

pub fn checked(app: &App, label: &str) -> Option<bool> {
    Some(match label {
        "Snap Quick Swipe Comping" | "Toggle Snap Quick Swipe Comping" => app.rec.takes.snap,
        "Toggle Take Folder Quick Swipe Comping Mode" => app.rec.takes.quick_swipe,
        _ => return None,
    })
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Snap Quick Swipe Comping" | "Toggle Snap Quick Swipe Comping" => app.rec.takes.snap = !app.rec.takes.snap,
        "Toggle Take Folder Quick Swipe Comping Mode" => app.rec.takes.quick_swipe = !app.rec.takes.quick_swipe,
        "Pack Take Folder" => pack(app),
        _ => {
            let Some(fi) = focused(app) else {
                // (the folder commands with a track folder's half: theirs)
                return matches!(name, "Un/disclose Take Folder" | "Flatten Take Folder/Cell" | "Flatten and Merge Take Folder/Cell" | "Create New Empty MIDI Take" | "Duplicate Active MIDI Take" | "Rename Take or Comp" | "Delete Take or Comp" | "Export Active Take or Comp to New Track" | "Move Active Take or Comp to New Track" | "Create New Comp" | "Select Previous Take or Comp" | "Select Next Take or Comp" | "Trim to Active Comp Sections");
            };
            return folder_command(app, fi, name);
        }
    }
    true
}

/// A command on folder `fi`; false when it isn't one.
fn folder_command(app: &mut App, fi: usize, name: &str) -> bool {
    let midi = folders(app).get(fi).is_some_and(|f| f.is_midi());
    match name {
        "Un/disclose Take Folder" => {
            if let Some(f) = folder_mut(app, fi) {
                f.expanded = !f.expanded;
            }
        }
        "Rename Take or Comp" => {
            let n = folders(app)[fi].active_name();
            app.rec.takes.renaming = Some((fi, n));
        }
        "Select Previous Take or Comp" | "Select Next Take or Comp" => edit(app, fi, "Select Take", |f| f.step(name.contains("Next"))),
        "Create New Comp" => edit(app, fi, "Create New Comp", |f| {
            f.new_comp();
        }),
        "Trim to Active Comp Sections" => edit(app, fi, "Trim to Active Comp Sections", |f| f.trim_to_active()),
        "Create New Empty MIDI Take" | "Duplicate Active MIDI Take" if midi => edit(app, fi, name, |f| {
            if name.starts_with("Create") {
                f.empty_take();
            } else {
                f.duplicate_take();
            }
        }),
        "Create New Empty MIDI Take" | "Duplicate Active MIDI Take" => {}
        "Delete Take or Comp" => {
            crate::audioedit::checkpoint(app, "Delete Take");
            let left = folder_mut(app, fi).is_some_and(|f| f.delete_active());
            if left {
                sync(app, fi);
            } else {
                drop_folder(app, fi, true);
            }
            app.after_edit();
        }
        "Flatten Take Folder/Cell" => {
            crate::audioedit::checkpoint(app, "Flatten Take Folder");
            drop_folder(app, fi, false);
            app.after_edit();
        }
        "Flatten and Merge Take Folder/Cell" => flatten_merge(app, fi),
        "Export Active Take or Comp to New Track" | "Move Active Take or Comp to New Track" => {
            let mv = name.starts_with("Move");
            crate::audioedit::checkpoint(app, if mv { "Move Take to New Track" } else { "Export Take to New Track" });
            let f = folders(app)[fi].clone();
            if let Some(nt) = crate::recording::new_track_like(app, f.track) {
                for s in f.pieces() {
                    let (mut r, off) = piece_region(app, &f, s);
                    r.name = f.active_name();
                    add_region(app, nt, r, off);
                }
                if mv {
                    let left = folder_mut(app, fi).is_some_and(|f| f.delete_active());
                    if left { sync(app, fi) } else { drop_folder(app, fi, true) }
                }
                app.selected_track = Some(nt);
            }
            app.after_edit();
        }
        "Unpack Take Folder to Independent Tracks or Unpack Folder/Take Cell to New Tracks" | "Unpack Take Folder to Independent Tracks (mute inactive) or Unpack Folder/Take Cell to Existing Tracks" => {
            let mute = name.contains("mute inactive");
            crate::audioedit::checkpoint(app, "Unpack Take Folder");
            let f = folders(app)[fi].clone();
            let playing: Vec<usize> = f.pieces().iter().map(|s| s.take).collect();
            drop_folder(app, fi, true);
            for (k, take) in f.takes.iter().enumerate() {
                let Some(nt) = crate::recording::new_track_like(app, f.track) else { break };
                let mut r = take.clone();
                r.muted = mute && !playing.contains(&k);
                add_region(app, nt, r, 0);
            }
            app.after_edit();
        }
        _ => return false,
    }
    true
}

/// A change to folder `fi`: one undo step, the pieces made again.
fn edit(app: &mut App, fi: usize, what: &str, f: impl FnOnce(&mut TakeFolder)) {
    crate::audioedit::checkpoint(app, what);
    if let Some(x) = folder_mut(app, fi) {
        f(x);
    }
    sync(app, fi);
    app.after_edit();
}

/// The folder goes: its pieces stay as plain regions (Flatten), or go too.
fn drop_folder(app: &mut App, fi: usize, with_pieces: bool) {
    let Some(f) = folders(app).get(fi).cloned() else { return };
    if with_pieces {
        let mine: Vec<usize> = app.project.as_ref().map(|p| p.tracks[f.track].regions.iter().enumerate().filter(|(_, r)| owned_by(&f, r)).map(|(i, _)| i).collect()).unwrap_or_default();
        remove_regions(app, f.track, mine);
    }
    if let Some(p) = app.project.as_mut() {
        p.takes.remove(fi);
    }
    app.rec.takes.focus = None;
    app.rec.takes.renaming = None;
}

/// Flatten and Merge: the comp as one region (audio: one new file of the pieces, crossfaded at
/// the joins; MIDI: one region of their notes).
fn flatten_merge(app: &mut App, fi: usize) {
    let f = folders(app)[fi].clone();
    let pieces = f.pieces();
    let (Some(a), Some(b)) = (pieces.iter().map(|s| s.start).min(), pieces.iter().map(|s| s.end).max()) else { return };
    let name = f.name.clone();
    let merged = if f.is_midi() {
        let mut r = f.midi_region(Section { start: a, end: b, take: pieces[0].take }, &name);
        r.notes = pieces.iter().flat_map(|s| f.midi_region(*s, &name).notes).collect();
        r.notes.sort_by_key(|n| n.pos);
        Some((r, 0))
    } else {
        let mut files: HashMap<PathBuf, lg_audio::AudioFile> = HashMap::new();
        for s in &pieces {
            if let Some(au) = &f.takes[s.take].audio {
                if !files.contains_key(&au.file) {
                    match lg_audio::AudioFile::read(&au.file) {
                        Ok(x) => {
                            files.insert(au.file.clone(), x);
                        }
                        Err(e) => {
                            app.tell(format!("Can't read {}: {e}", au.file.display()));
                            return;
                        }
                    }
                }
            }
        }
        let t0 = app.seconds_at(a as f64);
        let rate = files.values().next().map_or(crate::audio::RATE, |x| x.rate) as f64;
        let mut parts = Vec::new();
        for s in &pieces {
            let Some(au) = &f.takes[s.take].audio else { continue };
            let src = &files[&au.file];
            let (_, off) = piece_region(app, &f, *s);
            let frames = ((app.seconds_at(s.end as f64) - app.seconds_at(s.start as f64)) * rate).round() as usize;
            let at = ((app.seconds_at(s.start as f64) - t0) * rate).round() as usize;
            parts.push(lg_audio::record::Piece { src, from: off, frames, at });
        }
        let total = ((app.seconds_at(b as f64) - t0) * rate).round() as usize;
        // (Logic's comp crossfades: a few milliseconds)
        let joined = lg_audio::record::join(&parts, total, (rate * 0.005) as usize);
        let dir = crate::recording::audio_dir(app);
        let _ = std::fs::create_dir_all(&dir);
        let path = (1..).map(|n| dir.join(format!("{} Comp#{n:02}.wav", name.replace(['/', '\\', ':'], "-")))).find(|p| !p.exists()).unwrap();
        if let Err(e) = joined.write(&path) {
            app.tell(format!("Can't write {}: {e}", path.display()));
            return;
        }
        let secs = joined.seconds();
        Some((Region { name: name.clone(), start: a, length: b - a, notes: Vec::new(), audio: Some(lg_core::project::AudioRef { file: path, seconds: secs }), muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() }, 0))
    };
    let Some((r, off)) = merged else { return };
    crate::audioedit::checkpoint(app, "Flatten and Merge Take Folder");
    drop_folder(app, fi, true);
    if let Some(i) = add_region(app, f.track, r, off) {
        app.selected_region = Some((f.track, i));
    }
    app.after_edit();
}

/// Pack Take Folder: the selected regions (of one track) as the takes of a new folder.
fn pack(app: &mut App) {
    let sel = app.region_selection();
    let Some(&(t, _)) = sel.first() else { return };
    let idx: Vec<usize> = sel.iter().filter(|s| s.0 == t && !owned(app, t, s.1)).map(|s| s.1).collect();
    if idx.is_empty() {
        return;
    }
    crate::audioedit::checkpoint(app, "Pack Take Folder");
    make_folder(app, t, idx, Vec::new());
    app.after_edit();
}

// =============================================================================================
// drawing

/// A file's waveform (cached): a peak per PEAK frames, its rate.
fn peaks(app: &mut App, path: &Path) -> Option<Arc<(Vec<f32>, u32)>> {
    if let Some(p) = app.rec.takes.peaks.get(path) {
        return Some(p.clone());
    }
    let a = lg_audio::AudioFile::read(path).ok()?;
    let ch = a.channels.max(1);
    let v: Vec<f32> = a.data.chunks(PEAK * ch).map(|c| c.iter().fold(0f32, |m, x| m.max(x.abs()))).collect();
    let p = Arc::new((v, a.rate));
    app.rec.takes.peaks.insert(path.to_path_buf(), p.clone());
    Some(p)
}

fn badge(f: &TakeFolder) -> String {
    match f.active {
        Active::Take(k) => f.takes.get(k).and_then(|t| t.name.strip_prefix("Take ").map(str::to_string)).unwrap_or_else(|| (k + 1).to_string()),
        Active::Comp(c) => f.comps.get(c).and_then(|c| c.name.strip_prefix("Comp ").map(str::to_string)).unwrap_or_else(|| "C".into()),
    }
}

/// The take folders on track `track`'s lane: the folder's frame and header over its pieces;
/// opened, its take lanes below.
pub fn lane(app: &mut App, ui: &mut egui::Ui, lane: Rect, track: usize, x0: f32, scroll_ticks: f32, ppt: f32) {
    let x = move |t: f32| x0 + (t - scroll_ticks) * ppt;
    let to_tick = move |x: f32| (x - x0) / ppt + scroll_ticks;
    let n = folders(app).len();
    for fi in 0..n {
        let Some(f) = folders(app).get(fi).cloned() else { break };
        if f.track != track {
            continue;
        }
        let (fx0, fx1) = (x(f.start as f32), x(f.end() as f32));
        if fx1 < lane.min.x || fx0 > lane.max.x {
            if f.expanded {
                take_lanes(app, ui, lane, fi, &f, x, to_tick);
            }
            continue;
        }
        let kind = app.project.as_ref().and_then(|p| p.tracks.get(track)).map_or(TrackKind::Audio, |t| t.kind);
        let body = look::region_color(kind);
        let fr = Rect::from_min_max(pos2(fx0, lane.min.y + 3.0), pos2(fx1.max(fx0 + 30.0), lane.max.y - 3.0));
        let p = ui.painter_at(lane);
        p.rect_stroke(fr, 4.0, Stroke::new(1.2, look::mix(body, Color32::WHITE, 0.6)), egui::StrokeKind::Inside);
        let head = Rect::from_min_size(fr.min, vec2(fr.width(), 15.0));
        p.rect_filled(head, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, look::mix(body, Color32::BLACK, 0.25));
        // the disclosure triangle, the number badge (the take folder menu), the name
        let tri = Rect::from_min_size(head.min + vec2(2.0, 0.0), vec2(14.0, 15.0));
        let c = tri.center();
        let pts = if f.expanded { vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)] } else { vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)] };
        p.add(egui::Shape::convex_polygon(pts, Color32::WHITE, Stroke::NONE));
        let tresp = ui.interact(tri, ui.scope_id().with(("take-tri", fi)), Sense::click()).on_hover_text("Open or close the take folder (⌥F)");
        if tresp.clicked() {
            if let Some(f) = folder_mut(app, fi) {
                f.expanded = !f.expanded;
            }
            app.rec.takes.focus = Some(fi);
        }
        let b = badge(&f);
        let br = Rect::from_min_size(pos2(tri.max.x + 1.0, head.min.y + 1.5), vec2(8.0 + 7.0 * b.len() as f32, 12.0));
        p.rect_filled(br, 3.0, Color32::from_gray(0xf0));
        p.text(br.center(), Align2::CENTER_CENTER, &b, look::semibold(9.5), Color32::from_gray(0x20));
        let bresp = ui.interact(br, ui.scope_id().with(("take-badge", fi)), Sense::click()).on_hover_text("Take folder menu");
        let title = format!("{}: {}", f.name, f.active_name());
        let renaming = app.rec.takes.renaming.as_ref().is_some_and(|r| r.0 == fi);
        if renaming {
            let er = Rect::from_min_max(pos2(br.max.x + 4.0, head.min.y), pos2(head.max.x.max(br.max.x + 130.0), head.max.y + 2.0));
            let mut text = app.rec.takes.renaming.as_ref().map(|r| r.1.clone()).unwrap_or_default();
            let te = ui.put(er, egui::TextEdit::singleline(&mut text).font(look::regular(10.5)));
            crate::widgets::focus_once(ui, &te);
            if let Some(r) = app.rec.takes.renaming.as_mut() {
                r.1 = text.clone();
            }
            if te.lost_focus() {
                app.rec.takes.renaming = None;
                if !text.trim().is_empty() {
                    edit(app, fi, "Rename Take", |f| f.rename_active(text.trim()));
                }
            }
        } else if head.width() > 50.0 {
            p.with_clip_rect(head).text(pos2(br.max.x + 4.0, head.center().y), Align2::LEFT_CENTER, &title, look::semibold(10.5), Color32::WHITE);
        }
        egui::Popup::menu(&bresp).show(|ui| folder_menu(app, ui, fi));
        let hresp = ui.interact(Rect::from_min_max(pos2(br.max.x, head.min.y), head.max), ui.scope_id().with(("take-head", fi)), Sense::click());
        if hresp.clicked() || bresp.clicked() || hresp.secondary_clicked() {
            app.rec.takes.focus = Some(fi);
            app.selected_track = Some(track);
            if let Some(r) = app.project.as_ref().and_then(|p| p.tracks.get(track)).and_then(|tr| tr.regions.iter().position(|r| owned_by(&f, r))) {
                app.selected_region = Some((track, r));
            }
        }
        if hresp.double_clicked() {
            if let Some(f) = folder_mut(app, fi) {
                f.expanded = !f.expanded;
            }
        }
        egui::Popup::context_menu(&hresp).show(|ui| folder_menu(app, ui, fi));
        if f.expanded {
            take_lanes(app, ui, lane, fi, &f, x, to_tick);
        }
    }
}

/// The opened folder's take lanes, under its track (over the tracks below, as a drop-down).
fn take_lanes(app: &mut App, ui: &mut egui::Ui, lane: Rect, fi: usize, f: &TakeFolder, x: impl Fn(f32) -> f32, to_tick: impl Fn(f32) -> f32) {
    let n = f.takes.len();
    let area = Rect::from_min_size(pos2(lane.min.x, lane.max.y), vec2(lane.width(), n as f32 * TAKE_H));
    let kind = app.project.as_ref().and_then(|p| p.tracks.get(f.track)).map_or(TrackKind::Audio, |t| t.kind);
    let body = look::region_color(kind);
    let pieces = f.pieces();
    let bar = app.bar_ticks() as f32;
    let beat = bar / app.project.as_ref().map_or(4, |p| p.signature.0.max(1) as u32) as f32;
    let snap_on = app.rec.takes.snap;
    egui::Area::new(ui.scope_id().with(("take-lanes", fi)))
        .order(egui::Order::Middle)
        .fixed_pos(area.min)
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(area);
            let (_, _) = ui.allocate_exact_size(area.size(), Sense::hover());
            let p = ui.painter_at(area);
            p.rect_filled(area, 0.0, look::lane_bg());
            for (k, take) in f.takes.iter().enumerate() {
                let row = Rect::from_min_size(pos2(area.min.x, area.min.y + k as f32 * TAKE_H), vec2(area.width(), TAKE_H));
                p.hline(row.x_range(), row.max.y, Stroke::new(1.0, look::grid_line()));
                let tr = Rect::from_min_max(pos2(x(take.start as f32), row.min.y + 2.0), pos2(x((take.start + take.length) as f32), row.max.y - 2.0));
                p.rect_filled(tr, 3.0, body.gamma_multiply(0.28));
                // the parts of it that play: bright
                for s in pieces.iter().filter(|s| s.take == k) {
                    let r = Rect::from_min_max(pos2(x(s.start as f32), tr.min.y), pos2(x(s.end as f32), tr.max.y));
                    p.rect_filled(r, 3.0, body.gamma_multiply(0.9));
                }
                // (light on the dim parts, dark on the parts that play)
                let ink = look::mix(body, Color32::WHITE, 0.35);
                if let Some(au) = &take.audio {
                    if let Some(pk) = peaks(app, &au.file) {
                        let (v, rate) = (&pk.0, pk.1 as f64);
                        let t0 = app.seconds_at(take.start as f64);
                        let mid = tr.center().y + 4.0;
                        let h = (tr.height() - 12.0) * 0.5;
                        let (lo, hi) = (tr.min.x.max(area.min.x) as i32, tr.max.x.min(area.max.x) as i32);
                        for px in lo..hi {
                            let tick = to_tick(px as f32);
                            let i = ((app.seconds_at(tick as f64) - t0) * rate / PEAK as f64) as usize;
                            let pkv = v.get(i).copied().unwrap_or(0.0).min(1.0);
                            p.vline(px as f32, mid - pkv * h..=mid + pkv * h, Stroke::new(1.0, ink));
                        }
                    }
                } else {
                    for nt in &take.notes {
                        let y = tr.max.y - 2.0 - (nt.pitch as f32 - 24.0).clamp(0.0, 84.0) / 84.0 * (tr.height() - 14.0);
                        p.line_segment([pos2(x(nt.pos as f32), y), pos2(x((nt.pos + nt.len) as f32).max(x(nt.pos as f32) + 1.5), y)], Stroke::new(1.4, ink));
                    }
                }
                let active = f.active == Active::Take(k);
                p.with_clip_rect(tr).text(pos2(tr.min.x + 4.0, tr.min.y + 1.0), Align2::LEFT_TOP, &take.name, if active { look::semibold(10.5) } else { look::regular(10.5) }, look::text());
                // click: the take plays whole; drag (Quick Swipe Comping): that part into the comp
                let resp = ui.interact(tr, ui.scope_id().with(("take-lane", fi, k)), Sense::click_and_drag());
                let snap = |t: f32| if snap_on { ((t / beat).round() * beat).max(0.0) } else { t.max(0.0) };
                if resp.clicked() {
                    edit(app, fi, "Select Take", |f| f.select(Active::Take(k)));
                }
                if resp.drag_started() && app.rec.takes.quick_swipe {
                    let from_tick = ui.input(|i| i.pointer.press_origin()).map_or(0.0, |pp| snap(to_tick(pp.x)));
                    crate::audioedit::checkpoint(app, "Quick Swipe Comping");
                    app.rec.takes.swipe = Some(Swipe { folder: fi, take: k, from_tick, before: f.clone() });
                }
                if let (true, Some(pp)) = (resp.dragged(), resp.interact_pointer_pos()) {
                    if let Some(sw) = app.rec.takes.swipe.as_ref().filter(|s| s.folder == fi && s.take == k) {
                        let mut nf = sw.before.clone();
                        nf.swipe(k, sw.from_tick as u32, snap(to_tick(pp.x)) as u32);
                        if let Some(x) = folder_mut(app, fi) {
                            *x = nf;
                        }
                    }
                }
                if resp.drag_stopped() && app.rec.takes.swipe.take().is_some() {
                    sync(app, fi);
                    app.after_edit();
                }
                egui::Popup::context_menu(&resp).show(|ui| folder_menu(app, ui, fi));
            }
            p.rect_stroke(area, 0.0, Stroke::new(1.0, look::line()), egui::StrokeKind::Inside);
        });
}

/// The take folder menu (its number badge, a right-click on it): the takes and comps (the one
/// playing ticked), then the take commands.
fn folder_menu(app: &mut App, ui: &mut egui::Ui, fi: usize) {
    let Some(f) = folders(app).get(fi).cloned() else { return };
    ui.set_min_width(260.0);
    let mut pick = None;
    for (k, c) in f.comps.iter().enumerate() {
        if ui.add(egui::Button::selectable(f.active == Active::Comp(k), &c.name)).clicked() {
            pick = Some(Active::Comp(k));
        }
    }
    if !f.comps.is_empty() {
        ui.separator();
    }
    for (k, t) in f.takes.iter().enumerate() {
        if ui.add(egui::Button::selectable(f.active == Active::Take(k), &t.name)).clicked() {
            pick = Some(Active::Take(k));
        }
    }
    if let Some(a) = pick {
        edit(app, fi, "Select Take", |f| f.select(a));
        ui.close();
        return;
    }
    ui.separator();
    let midi = f.is_midi();
    let items: &[(&str, &str, bool)] = &[
        ("Create New Comp", "Create New Comp", true),
        ("Rename Take or Comp", "Rename…", true),
        ("Delete Take or Comp", "Delete", true),
        ("", "", true),
        ("Flatten Take Folder/Cell", "Flatten", true),
        ("Flatten and Merge Take Folder/Cell", "Flatten and Merge", true),
        ("Export Active Take or Comp to New Track", "Export Active Comp to New Track", true),
        ("Move Active Take or Comp to New Track", "Move Active Comp to New Track", true),
        ("Unpack Take Folder to Independent Tracks or Unpack Folder/Take Cell to New Tracks", "Unpack to New Tracks", true),
        ("Unpack Take Folder to Independent Tracks (mute inactive) or Unpack Folder/Take Cell to Existing Tracks", "Unpack to New Tracks (mute inactive)", true),
        ("Trim to Active Comp Sections", "Trim to Active Comp Sections", true),
        ("", "", true),
        ("Create New Empty MIDI Take", "Create New Empty MIDI Take", midi),
        ("Duplicate Active MIDI Take", "Duplicate Active MIDI Take", midi),
        ("", "", true),
        ("Toggle Take Folder Quick Swipe Comping Mode", "Quick Swipe Comping", true),
        ("Toggle Snap Quick Swipe Comping", "Snap Quick Swipe Comping", true),
    ];
    for &(cmd, label, on) in items {
        if cmd.is_empty() {
            ui.separator();
            continue;
        }
        let tick = checked(app, cmd).unwrap_or(false);
        let shown = if tick { format!("✓ {label}") } else { label.to_string() };
        if ui.add_enabled(on, egui::Button::new(shown)).clicked() {
            app.rec.takes.focus = Some(fi);
            match cmd {
                "Toggle Take Folder Quick Swipe Comping Mode" | "Toggle Snap Quick Swipe Comping" => {
                    command(app, cmd);
                }
                _ => {
                    folder_command(app, fi, cmd);
                }
            }
            ui.close();
        }
    }
}

/// Folders whose track is gone go (a deleted track).
pub fn tidy(app: &mut App) {
    let Some(p) = app.project.as_mut() else { return };
    let n = p.tracks.len();
    p.takes.retain(|f| f.track < n);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_rate_of_wav_and_aiff_files() {
        let d = std::env::temp_dir().join(format!("lg-takes-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let w = d.join("a.wav");
        lg_audio::AudioFile::new(44_100, 1, vec![0.0; 10]).write(&w).unwrap();
        assert_eq!(file_rate(&w), 44_100);
        let a = d.join("a.aif");
        let mut f = lg_audio::AudioFile::new(96_000, 2, vec![0.0; 10]);
        f.format = lg_audio::Format::Aiff;
        f.write(&a).unwrap();
        assert_eq!(file_rate(&a), 96_000);
    }
}
