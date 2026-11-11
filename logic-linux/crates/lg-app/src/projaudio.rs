//! The Project Audio Browser (Project Files ▸ Project in the right pane; Window ▸ Open Project
//! Audio, ⌘8, as a window), as Logic's: the project's audio files, each with the regions made
//! from it (disclosure arrow), the Icon column (missing file, follows the project tempo, has
//! tempo information), the Info column (sample rate, bit depth, format, size; a bar for the
//! file's length with its regions' parts coloured, or a waveform overview), the Bpm column,
//! Play / Cycle / volume, its local menus (Edit, View, Audio File) and its key commands. Files
//! and regions can be added, renamed, deleted, sorted and grouped; files backed up, copied or
//! converted, moved out of the project, optimized; region information exported and imported.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::TrackKind;

use crate::app::App;
use crate::look;

/// The key commands and menu items this file runs (Project Audio group, its local menus).
pub const COMMANDS: &[&str] = &[
    "Select Previous Audio File", "Select Next Audio File", "Show Audio File Details", "Hide Audio File Details", "Rename…", "Add Audio File…", "Add Region", "Delete Files", "Optimize Files…", "Backup Files", "Copy/Convert Files…", "Move Files out of Project", "Select Used", "Select Unused", "Show All Regions", "Hide All Regions", "Import Region Information", "Export Region Information", "Create Group…", "Delete Selected Audio File Groups",
    "Open Project Audio", "Open Project Audio…", "Show/Hide Project Audio", "Show/Hide All Files Browser",
];

/// Sorting (View ▸ Sort Files By).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    None,
    Name,
    Size,
    Length,
}

/// Grouping (View ▸ Group Files By).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Group {
    #[default]
    None,
    Location,
    Attributes,
    Selection,
}

/// A region kept in the project's audio pool, not placed on a track (Add Region, Import Region
/// Information).
#[derive(Clone, Debug, PartialEq)]
pub struct PoolRegion {
    pub file: PathBuf,
    pub name: String,
    /// Start and length in the file (seconds).
    pub start: f64,
    pub len: f64,
}

#[derive(Default)]
pub struct State {
    /// Files added (Add Audio File…) beyond those the regions use.
    pub added: Vec<PathBuf>,
    pub pool: Vec<PoolRegion>,
    /// Files shown with their regions.
    pub open: BTreeSet<PathBuf>,
    pub selected: BTreeSet<PathBuf>,
    /// A region selected: placed (track, region) or pool index.
    pub sel_region: Option<RegionRef>,
    pub details: bool,
    pub sort: Sort,
    pub group: Group,
    /// Selection groups (Create Group…): name, files.
    pub groups: Vec<(String, BTreeSet<PathBuf>)>,
    pub volume: f32,
    pub cycle: bool,
    pub window: bool,
    pub rename: Option<(RenameWhat, String)>,
    pub group_name: Option<String>,
    pub confirm_delete: bool,
    pub convert: Option<Convert>,
    /// The pane's rect (keys go to it under the pointer).
    pub rect: Option<Rect>,
    info_cache: HashMap<PathBuf, Option<FileInfo>>,
    overviews: HashMap<PathBuf, Option<Vec<(f32, f32)>>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RegionRef {
    Placed(usize, usize),
    Pool(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenameWhat {
    File(PathBuf),
    Region(RegionRef),
}

/// The Copy/Convert sheet's choices.
#[derive(Clone, Debug, PartialEq)]
pub struct Convert {
    pub rate: u32,
    pub bits: u16,
    pub move_refs: bool,
}

/// What a file's header says.
#[derive(Clone, Debug, PartialEq)]
pub struct FileInfo {
    pub rate: u32,
    pub bits: u16,
    pub channels: u16,
    pub bytes: u64,
    pub seconds: f64,
    pub format: String,
}

impl FileInfo {
    pub fn text(&self) -> String {
        let ch = match self.channels {
            1 => "Mono".to_string(),
            2 => "Stereo".to_string(),
            n => format!("{n} ch"),
        };
        let bits = if self.bits > 0 { format!("{} Bit", self.bits) } else { "—".into() };
        format!("{} Hz  {bits}  {ch}  {}  {}", self.rate, self.format, size_text(self.bytes))
    }
}

pub fn size_text(b: u64) -> String {
    if b >= 1 << 30 {
        format!("{:.1} GB", b as f64 / (1u64 << 30) as f64)
    } else if b >= 1 << 20 {
        format!("{:.1} MB", b as f64 / (1u64 << 20) as f64)
    } else {
        format!("{:.0} KB", (b as f64 / 1024.0).max(1.0))
    }
}

/// Reads a WAV / AIFF header (other formats: the decoder's rate and channels).
pub fn file_info(p: &Path) -> Option<FileInfo> {
    let bytes = std::fs::metadata(p).ok()?.len();
    let ext = p.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
    let mut head = vec![0u8; 4096];
    let n = {
        use std::io::Read;
        let mut f = std::fs::File::open(p).ok()?;
        f.read(&mut head).ok()?
    };
    head.truncate(n);
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WAVE" {
        let mut i = 12;
        let (mut rate, mut bits, mut ch, mut data) = (0u32, 0u16, 0u16, 0u64);
        while i + 8 <= head.len() {
            let id = &head[i..i + 4];
            let len = u32::from_le_bytes(head[i + 4..i + 8].try_into().ok()?) as usize;
            if id == b"fmt " && i + 24 <= head.len() {
                ch = u16::from_le_bytes(head[i + 10..i + 12].try_into().ok()?);
                rate = u32::from_le_bytes(head[i + 12..i + 16].try_into().ok()?);
                bits = u16::from_le_bytes(head[i + 22..i + 24].try_into().ok()?);
            }
            if id == b"data" {
                data = len as u64;
                break;
            }
            i += 8 + len + (len & 1);
        }
        let frame = (bits as u64 / 8).max(1) * ch.max(1) as u64;
        let seconds = if rate > 0 { data.min(bytes) as f64 / frame as f64 / rate as f64 } else { 0.0 };
        return Some(FileInfo { rate, bits, channels: ch, bytes, seconds, format: "WAVE".into() });
    }
    if head.len() >= 12 && &head[0..4] == b"FORM" && (&head[8..12] == b"AIFF" || &head[8..12] == b"AIFC") {
        let mut i = 12;
        while i + 8 <= head.len() {
            let id = &head[i..i + 4];
            let len = u32::from_be_bytes(head[i + 4..i + 8].try_into().ok()?) as usize;
            if id == b"COMM" && i + 26 <= head.len() {
                let ch = u16::from_be_bytes(head[i + 8..i + 10].try_into().ok()?);
                let frames = u32::from_be_bytes(head[i + 10..i + 14].try_into().ok()?);
                let bits = u16::from_be_bytes(head[i + 14..i + 16].try_into().ok()?);
                let rate = extended_to_f64(&head[i + 16..i + 26]) as u32;
                let seconds = if rate > 0 { frames as f64 / rate as f64 } else { 0.0 };
                return Some(FileInfo { rate, bits, channels: ch, bytes, seconds, format: "AIFF".into() });
            }
            i += 8 + len + (len & 1);
        }
    }
    use rodio::Source;
    let d = rodio::Decoder::try_from(std::fs::File::open(p).ok()?).ok()?;
    let seconds = d.total_duration().map(|t| t.as_secs_f64()).unwrap_or(0.0);
    Some(FileInfo { rate: d.sample_rate().get(), bits: 0, channels: d.channels().get(), bytes, seconds, format: ext })
}

/// AIFF's 80-bit sample rate.
fn extended_to_f64(b: &[u8]) -> f64 {
    let exp = (((b[0] as i32) & 0x7f) << 8 | b[1] as i32) - 16383;
    let mant = u64::from_be_bytes(b[2..10].try_into().unwrap_or([0; 8]));
    mant as f64 * 2f64.powi(exp - 63)
}

/// A file's tempo, as the Bpm column shows: from its name (loops), else none.
pub fn file_bpm(p: &Path) -> Option<f64> {
    crate::soundlib::bpm_in_name(&p.file_stem()?.to_string_lossy())
}

/// The project's audio files in order (first use), with the regions placed from each.
pub fn files(app: &App) -> Vec<(PathBuf, Vec<RegionRef>)> {
    let mut out: Vec<(PathBuf, Vec<RegionRef>)> = Vec::new();
    let add = |f: &Path, r: Option<RegionRef>, out: &mut Vec<(PathBuf, Vec<RegionRef>)>| {
        let i = match out.iter().position(|x| x.0 == f) {
            Some(i) => i,
            None => {
                out.push((f.to_path_buf(), Vec::new()));
                out.len() - 1
            }
        };
        if let Some(r) = r {
            out[i].1.push(r);
        }
    };
    if let Some(p) = &app.project {
        for (t, tr) in p.tracks.iter().enumerate() {
            for (r, rg) in tr.regions.iter().enumerate() {
                if let Some(a) = &rg.audio {
                    add(&a.file, Some(RegionRef::Placed(t, r)), &mut out);
                }
            }
        }
    }
    for (k, pr) in app.pa.pool.iter().enumerate() {
        add(&pr.file, Some(RegionRef::Pool(k)), &mut out);
    }
    for f in &app.pa.added {
        add(f, None, &mut out);
    }
    out
}

/// Files used by regions on tracks.
fn used(app: &App) -> BTreeSet<PathBuf> {
    files(app).into_iter().filter(|(_, r)| r.iter().any(|x| matches!(x, RegionRef::Placed(..)))).map(|x| x.0).collect()
}

/// A file's waveform overview (200 min / max pairs), made once.
fn overview(app: &mut App, p: &Path) -> Option<Vec<(f32, f32)>> {
    app.pa.overviews.entry(p.to_path_buf()).or_insert_with(|| {
        let s = crate::audio::load_audio(p)?;
        let frames = s.len() / 2;
        let n = 200usize;
        let per = (frames / n).max(1);
        Some((0..n.min(frames)).map(|k| {
            let seg = &s[k * per * 2..((k + 1) * per * 2).min(s.len())];
            let lo = seg.iter().copied().fold(0.0f32, f32::min);
            let hi = seg.iter().copied().fold(0.0f32, f32::max);
            (lo.max(-1.0), hi.min(1.0))
        }).collect())
    }).clone()
}

fn info(app: &mut App, p: &Path) -> Option<FileInfo> {
    app.pa.info_cache.entry(p.to_path_buf()).or_insert_with(|| file_info(p)).clone()
}

/// The files in the order shown (sorted, then grouped): (group heading, files).
fn arranged(app: &mut App) -> Vec<(String, Vec<(PathBuf, Vec<RegionRef>)>)> {
    let mut fs = files(app);
    match app.pa.sort {
        Sort::None => {}
        Sort::Name => fs.sort_by_key(|f| f.0.file_name().map(|n| n.to_string_lossy().to_lowercase())),
        Sort::Size => {
            let sizes: Vec<u64> = fs.iter().map(|f| std::fs::metadata(&f.0).map(|m| m.len()).unwrap_or(0)).collect();
            let mut idx: Vec<usize> = (0..fs.len()).collect();
            idx.sort_by_key(|&i| std::cmp::Reverse(sizes[i]));
            fs = idx.into_iter().map(|i| fs[i].clone()).collect();
        }
        Sort::Length => {
            let lens: Vec<f64> = fs.iter().map(|f| info(app, &f.0).map_or(0.0, |i| i.seconds)).collect();
            let mut idx: Vec<usize> = (0..fs.len()).collect();
            idx.sort_by(|&a, &b| lens[b].partial_cmp(&lens[a]).unwrap_or(std::cmp::Ordering::Equal));
            fs = idx.into_iter().map(|i| fs[i].clone()).collect();
        }
    }
    let key = |app: &mut App, f: &Path| -> String {
        match app.pa.group {
            Group::None => String::new(),
            Group::Location => f.parent().map(|d| d.display().to_string()).unwrap_or_default(),
            Group::Attributes => info(app, f).map(|i| format!("{} Hz {} Bit {}", i.rate, i.bits, if i.channels == 1 { "Mono" } else { "Stereo" })).unwrap_or_else(|| "Missing".into()),
            Group::Selection => app.pa.groups.iter().find(|g| g.1.contains(f)).map(|g| g.0.clone()).unwrap_or_else(|| "Not in a group".into()),
        }
    };
    let mut out: Vec<(String, Vec<(PathBuf, Vec<RegionRef>)>)> = Vec::new();
    for f in fs {
        let k = key(app, &f.0);
        match out.iter_mut().find(|g| g.0 == k) {
            Some(g) => g.1.push(f),
            None => out.push((k, vec![f])),
        }
    }
    out
}

/// A region's name, its start and length in the file (seconds).
fn region_span(app: &App, r: RegionRef) -> Option<(String, f64, f64)> {
    match r {
        RegionRef::Placed(t, k) => {
            let rg = app.project.as_ref()?.tracks.get(t)?.regions.get(k)?;
            let a = rg.audio.as_ref()?;
            let len = app.seconds_at((rg.start + rg.length) as f64) - app.seconds_at(rg.start as f64);
            let rate = info_rate(a.file.as_path()).unwrap_or(crate::audio::RATE) as f64;
            let off = app.audio_edit.regions.get(&(t, k)).map_or(0.0, |x| x.offset as f64 / rate);
            Some((rg.name.clone(), off, len.min(a.seconds.max(len))))
        }
        RegionRef::Pool(i) => app.pa.pool.get(i).map(|p| (p.name.clone(), p.start, p.len)),
    }
}

fn info_rate(p: &Path) -> Option<u32> {
    file_info(p).map(|i| i.rate)
}

fn missing(p: &Path) -> bool {
    !p.exists()
}

/// The key_command hook: the Project Audio commands no other area has (the rest run while the
/// pointer is over the browser, `key_press`).
pub fn global(app: &mut App, name: &str) -> bool {
    if matches!(name, "Select Previous Audio File" | "Select Next Audio File" | "Add Region" | "Rename…") || !COMMANDS.contains(&name) {
        return false;
    }
    command(app, name)
}

/// Runs a Project Audio command. True when it's this file's.
pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Open Project Audio" | "Open Project Audio…" => app.pa.window = true,
        "Show/Hide All Files Browser" => {
            app.right_pane = if app.right_pane == Some(crate::app::RightPane::Files) && app.pf_tab == 1 { None } else { Some(crate::app::RightPane::Files) };
            app.pf_tab = 1;
        }
        "Show/Hide Project Audio" => {
            app.right_pane = if app.right_pane == Some(crate::app::RightPane::Files) && app.pf_tab == 0 { None } else { Some(crate::app::RightPane::Files) };
            app.pf_tab = 0;
        }
        "Select Previous Audio File" | "Select Next Audio File" => {
            let fs: Vec<PathBuf> = arranged(app).into_iter().flat_map(|g| g.1.into_iter().map(|f| f.0)).collect();
            if fs.is_empty() {
                return true;
            }
            let cur = app.pa.selected.iter().next().and_then(|s| fs.iter().position(|f| f == s));
            let i = match (cur, name.contains("Next")) {
                (None, _) => 0,
                (Some(i), true) => (i + 1).min(fs.len() - 1),
                (Some(i), false) => i.saturating_sub(1),
            };
            app.pa.selected = [fs[i].clone()].into();
            app.pa.sel_region = None;
        }
        "Show Audio File Details" => app.pa.details = true,
        "Hide Audio File Details" => app.pa.details = false,
        "Show All Regions" => app.pa.open = files(app).into_iter().map(|f| f.0).collect(),
        "Hide All Regions" => app.pa.open.clear(),
        "Select Used" => app.pa.selected = used(app),
        "Select Unused" => {
            let u = used(app);
            app.pa.selected = files(app).into_iter().map(|f| f.0).filter(|f| !u.contains(f)).collect();
        }
        "Rename…" => {
            if let Some(r) = app.pa.sel_region {
                let n = region_span(app, r).map(|x| x.0).unwrap_or_default();
                app.pa.rename = Some((RenameWhat::Region(r), n));
            } else if let Some(f) = app.pa.selected.iter().next().cloned() {
                let n = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                app.pa.rename = Some((RenameWhat::File(f), n));
            }
        }
        "Add Audio File…" => {
            if app.automated {
                return true;
            }
            if let Some(fs) = kn_ui::files::FileDialog::new().set_title("Add Audio File").add_filter("Audio", crate::soundlib::AUDIO_EXT).pick_files() {
                add_files(app, fs);
            }
        }
        "Add Region" => {
            let Some(f) = app.pa.selected.iter().next().cloned() else { return true };
            let secs = file_info(&f).map_or(0.0, |i| i.seconds);
            let n = app.pa.pool.iter().filter(|p| p.file == f).count() + files(app).iter().find(|x| x.0 == f).map_or(0, |x| x.1.len());
            let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            app.pa.pool.push(PoolRegion { file: f.clone(), name: format!("{stem}.{}", n + 1), start: 0.0, len: secs });
            app.pa.open.insert(f);
            app.pa.sel_region = Some(RegionRef::Pool(app.pa.pool.len() - 1));
            app.dirty = true;
        }
        "Delete Files" => {
            if app.pa.sel_region.is_some() {
                delete_region(app);
            } else if !app.pa.selected.is_empty() {
                app.pa.confirm_delete = true;
            }
        }
        "Optimize Files…" => optimize(app),
        "Backup Files" => {
            let mut n = 0;
            for f in app.pa.selected.clone() {
                let mut b = f.clone().into_os_string();
                b.push(".bak");
                if std::fs::copy(&f, PathBuf::from(b)).is_ok() {
                    n += 1;
                }
            }
            app.tell(format!("Backed up {n} file{}", if n == 1 { "" } else { "s" }));
        }
        "Copy/Convert Files…" => {
            if !app.pa.selected.is_empty() {
                app.pa.convert = Some(Convert { rate: crate::audio::RATE, bits: 24, move_refs: true });
            }
        }
        "Move Files out of Project" => move_out(app),
        "Export Region Information" => export_regions(app),
        "Import Region Information" => import_regions(app),
        "Create Group…" => {
            if !app.pa.selected.is_empty() {
                app.pa.group_name = Some(format!("Group {}", app.pa.groups.len() + 1));
            }
        }
        "Delete Selected Audio File Groups" => {
            let sel = app.pa.selected.clone();
            app.pa.groups.retain(|g| g.1.is_disjoint(&sel));
        }
        _ => return false,
    }
    true
}

pub fn add_files(app: &mut App, fs: Vec<PathBuf>) {
    let have: BTreeSet<PathBuf> = files(app).into_iter().map(|f| f.0).collect();
    for f in fs {
        if !have.contains(&f) && !app.pa.added.contains(&f) {
            app.pa.added.push(f.clone());
        }
        app.pa.selected = [f].into();
    }
    app.dirty = true;
}

fn delete_region(app: &mut App) {
    match app.pa.sel_region.take() {
        Some(RegionRef::Placed(t, r)) => {
            app.checkpoint("Delete Region");
            if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
                if r < tr.regions.len() {
                    tr.regions.remove(r);
                }
            }
            app.after_edit();
        }
        Some(RegionRef::Pool(i)) => {
            if i < app.pa.pool.len() {
                app.pa.pool.remove(i);
            }
        }
        None => {}
    }
}

/// Delete Files: the files leave the project (their regions go, an undo step) and go to the
/// Trash.
fn delete_files(app: &mut App, to_trash: bool) {
    let sel = app.pa.selected.clone();
    app.checkpoint("Delete Files");
    if let Some(p) = app.project.as_mut() {
        for tr in p.tracks.iter_mut() {
            tr.regions.retain(|r| r.audio.as_ref().is_none_or(|a| !sel.contains(&a.file)));
        }
    }
    app.pa.pool.retain(|p| !sel.contains(&p.file));
    app.pa.added.retain(|f| !sel.contains(f));
    let mut trashed = 0;
    if to_trash {
        for f in &sel {
            if trash(f).is_ok() {
                trashed += 1;
            }
        }
    }
    app.pa.selected.clear();
    app.after_edit();
    app.tell(if to_trash { format!("{trashed} file{} moved to the Trash", if trashed == 1 { "" } else { "s" }) } else { "Removed from the project".to_string() });
}

/// Moves a file to the desktop's Trash (freedesktop: ~/.local/share/Trash).
pub fn trash(f: &Path) -> std::io::Result<()> {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))).ok_or(std::io::ErrorKind::NotFound)?;
    let dir = base.join("Trash");
    std::fs::create_dir_all(dir.join("files"))?;
    std::fs::create_dir_all(dir.join("info"))?;
    let name = f.file_name().ok_or(std::io::ErrorKind::InvalidInput)?.to_string_lossy().to_string();
    let mut dest = dir.join("files").join(&name);
    let mut k = 1;
    while dest.exists() {
        dest = dir.join("files").join(format!("{k} {name}"));
        k += 1;
    }
    let abs = std::fs::canonicalize(f)?;
    if std::fs::rename(f, &dest).is_err() {
        std::fs::copy(f, &dest)?;
        std::fs::remove_file(f)?;
    }
    let n = dest.file_name().unwrap_or_default().to_string_lossy().to_string();
    std::fs::write(dir.join("info").join(format!("{n}.trashinfo")), format!("[Trash Info]\nPath={}\nDeletionDate={}\n", abs.display(), "2026-01-01T00:00:00"))?;
    Ok(())
}

/// Every region (and the undo history's copies) pointing at `from` points at `to`.
fn repoint(app: &mut App, from: &Path, to: &Path) {
    let fix = |p: &mut lg_core::Project| {
        for tr in p.tracks.iter_mut() {
            for r in tr.regions.iter_mut() {
                if let Some(a) = r.audio.as_mut() {
                    if a.file == from {
                        a.file = to.to_path_buf();
                    }
                }
            }
        }
    };
    if let Some(p) = app.project.as_mut() {
        fix(p);
    }
    for (_, p, _) in app.undo.iter_mut().chain(app.redo.iter_mut()) {
        fix(p);
    }
    for r in app.audio_edit.regions.values_mut() {
        if r.file == from {
            r.file = to.to_path_buf();
        }
    }
    for pr in app.pa.pool.iter_mut() {
        if pr.file == from {
            pr.file = to.to_path_buf();
        }
    }
    for f in app.pa.added.iter_mut() {
        if f == from {
            *f = to.to_path_buf();
        }
    }
    for g in app.pa.groups.iter_mut() {
        if g.1.remove(from) {
            g.1.insert(to.to_path_buf());
        }
    }
    if app.pa.open.remove(from) {
        app.pa.open.insert(to.to_path_buf());
    }
    if app.pa.selected.remove(from) {
        app.pa.selected.insert(to.to_path_buf());
    }
    app.pa.info_cache.remove(from);
}

fn rename_done(app: &mut App, what: RenameWhat, name: String) {
    let name = name.trim().to_string();
    if name.is_empty() {
        return;
    }
    match what {
        RenameWhat::Region(RegionRef::Placed(t, r)) => {
            app.checkpoint("Rename Region");
            if let Some(rg) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)).and_then(|tr| tr.regions.get_mut(r)) {
                rg.name = name;
            }
            app.after_edit();
        }
        RenameWhat::Region(RegionRef::Pool(i)) => {
            if let Some(p) = app.pa.pool.get_mut(i) {
                p.name = name;
            }
        }
        RenameWhat::File(f) => {
            let ext = f.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
            let to = f.with_file_name(format!("{}{ext}", name.replace('/', "-")));
            if to == f {
                return;
            }
            if to.exists() {
                app.tell("A file with that name is already there");
                return;
            }
            match std::fs::rename(&f, &to) {
                Ok(()) => {
                    repoint(app, &f, &to);
                    app.dirty = true;
                    app.load_engine();
                }
                Err(e) => app.tell(format!("Couldn't rename the file: {e}")),
            }
        }
    }
}

/// Writes a WAV (16 or 24 bit PCM) at `rate` from channels at `src_rate`.
pub fn write_wav(path: &Path, channels: &[Vec<f32>], src_rate: u32, rate: u32, bits: u16) -> std::io::Result<()> {
    let ch = channels.len().max(1);
    let frames = channels.first().map_or(0, |c| c.len());
    let out_frames = (frames as f64 * rate as f64 / src_rate.max(1) as f64) as usize;
    let bps = (bits / 8) as usize;
    let data_len = out_frames * ch * bps;
    let mut b = Vec::with_capacity(44 + data_len);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&(ch as u16).to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&((rate as usize * ch * bps) as u32).to_le_bytes());
    b.extend_from_slice(&((ch * bps) as u16).to_le_bytes());
    b.extend_from_slice(&bits.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data_len as u32).to_le_bytes());
    for k in 0..out_frames {
        let src = k as f64 * src_rate as f64 / rate as f64;
        let i = src as usize;
        let t = (src - i as f64) as f32;
        for c in channels.iter().take(ch) {
            let a = c.get(i).copied().unwrap_or(0.0);
            let z = c.get(i + 1).copied().unwrap_or(a);
            let x = (a * (1.0 - t) + z * t).clamp(-1.0, 1.0);
            if bits == 24 {
                let v = (x * 8_388_607.0) as i32;
                b.extend_from_slice(&v.to_le_bytes()[..3]);
            } else {
                b.extend_from_slice(&((x * 32767.0) as i16).to_le_bytes());
            }
        }
    }
    std::fs::write(path, b)
}

fn convert(app: &mut App, c: Convert) {
    if app.automated {
        return;
    }
    let Some(dir) = kn_ui::files::FileDialog::new().set_title("Copy/Convert Files to").pick_folder() else { return };
    let mut n = 0;
    for f in app.pa.selected.clone() {
        let Ok(a) = lg_synth::wav::read_audio(&f) else { continue };
        let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let to = dir.join(format!("{stem}.wav"));
        if to == f {
            continue;
        }
        if write_wav(&to, &a.channels, a.rate, c.rate, c.bits).is_ok() {
            n += 1;
            if c.move_refs {
                // (the regions now play the copy: their offsets follow the new rate)
                let ratio = c.rate as f64 / a.rate.max(1) as f64;
                for (k, r) in app.audio_edit.regions.iter_mut() {
                    let _ = k;
                    if r.file == f {
                        r.offset = (r.offset as f64 * ratio) as usize;
                    }
                }
                repoint(app, &f, &to);
            }
        }
    }
    app.load_engine();
    app.tell(format!("Converted {n} file{}", if n == 1 { "" } else { "s" }));
}

fn move_out(app: &mut App) {
    if app.automated || app.pa.selected.is_empty() {
        return;
    }
    let Some(dir) = kn_ui::files::FileDialog::new().set_title("Move Files to").pick_folder() else { return };
    let mut n = 0;
    for f in app.pa.selected.clone() {
        let Some(name) = f.file_name() else { continue };
        let to = dir.join(name);
        if to.exists() {
            continue;
        }
        let ok = std::fs::rename(&f, &to).is_ok() || (std::fs::copy(&f, &to).is_ok() && std::fs::remove_file(&f).is_ok());
        if ok {
            repoint(app, &f, &to);
            n += 1;
        }
    }
    app.dirty = true;
    app.load_engine();
    app.tell(format!("Moved {n} file{}", if n == 1 { "" } else { "s" }));
}

/// Optimize Files: each selected file keeps only the parts its regions use (with a second's
/// margin), written as a new file next to it; the regions follow.
fn optimize(app: &mut App) {
    let sel = app.pa.selected.clone();
    let all = files(app);
    let mut n = 0;
    for (f, regions) in all.into_iter().filter(|x| sel.contains(&x.0)) {
        if regions.is_empty() {
            continue;
        }
        let Ok(a) = lg_synth::wav::read_audio(&f) else { continue };
        let rate = a.rate as f64;
        let spans: Vec<(f64, f64)> = regions.iter().filter_map(|r| region_span(app, *r).map(|x| (x.1, x.1 + x.2))).collect();
        let lo = spans.iter().map(|s| s.0).fold(f64::MAX, f64::min).max(0.0);
        let hi = spans.iter().map(|s| s.1).fold(0.0, f64::max);
        let (lo_f, hi_f) = ((((lo - 1.0).max(0.0)) * rate) as usize, (((hi + 1.0) * rate) as usize).min(a.frames()));
        if lo_f == 0 && hi_f >= a.frames() {
            continue;
        }
        let chans: Vec<Vec<f32>> = a.channels.iter().map(|c| c[lo_f.min(c.len())..hi_f.min(c.len())].to_vec()).collect();
        let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let to = f.with_file_name(format!("{stem} (optimized).wav"));
        if write_wav(&to, &chans, a.rate, a.rate, 24).is_err() {
            continue;
        }
        for r in app.audio_edit.regions.values_mut() {
            if r.file == f {
                r.offset = r.offset.saturating_sub(lo_f);
            }
        }
        for p in app.pa.pool.iter_mut() {
            if p.file == f {
                p.start = (p.start - lo_f as f64 / rate).max(0.0);
            }
        }
        // (placed regions without an offset start at the file's start: keep them so)
        let placed_no_offset: Vec<(usize, usize)> = regions.iter().filter_map(|r| if let RegionRef::Placed(t, k) = r { (!app.audio_edit.regions.contains_key(&(*t, *k))).then_some((*t, *k)) } else { None }).collect();
        if !placed_no_offset.is_empty() && lo_f > 0 {
            continue;
        }
        repoint(app, &f, &to);
        n += 1;
    }
    app.load_engine();
    app.tell(format!("Optimized {n} file{}", if n == 1 { "" } else { "s" }));
}

fn sidecar(f: &Path) -> PathBuf {
    let mut s = f.as_os_str().to_owned();
    s.push(".regions.txt");
    PathBuf::from(s)
}

/// Export Region Information: the selected files' regions (name, start, length) are kept with
/// the file (a "<file>.regions.txt" beside it), for other projects to import.
fn export_regions(app: &mut App) {
    let sel = app.pa.selected.clone();
    let mut n = 0;
    for (f, regions) in files(app).into_iter().filter(|x| sel.contains(&x.0)) {
        let body: String = regions.iter().filter_map(|r| region_span(app, *r)).map(|(name, s, l)| format!("{s:.6}\t{l:.6}\t{name}\n")).collect();
        if std::fs::write(sidecar(&f), format!("# Bramble Studio region information\n{body}")).is_ok() {
            n += 1;
        }
    }
    app.tell(format!("Exported the regions of {n} file{}", if n == 1 { "" } else { "s" }));
}

fn import_regions(app: &mut App) {
    let mut n = 0;
    for f in app.pa.selected.clone() {
        let Ok(t) = std::fs::read_to_string(sidecar(&f)) else { continue };
        for l in t.lines().filter(|l| !l.starts_with('#')) {
            let mut it = l.splitn(3, '\t');
            let (Some(s), Some(len), Some(name)) = (it.next().and_then(|x| x.parse().ok()), it.next().and_then(|x| x.parse().ok()), it.next()) else { continue };
            if !app.pa.pool.iter().any(|p| p.file == f && p.name == name) {
                app.pa.pool.push(PoolRegion { file: f.clone(), name: name.to_string(), start: s, len });
                n += 1;
            }
        }
        app.pa.open.insert(f);
    }
    app.tell(format!("Imported {n} region{}", if n == 1 { "" } else { "s" }));
}

/// Keys while the pointer is over the Project Audio Browser: its group's key commands.
pub fn key_press(app: &mut App, key: egui::Key, m: egui::Modifiers) -> bool {
    let over = app.ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| app.pa.rect.is_some_and(|r| r.contains(p))) && (app.pa.window || (app.right_pane == Some(crate::app::RightPane::Files) && app.pf_tab == 0));
    if !over {
        return false;
    }
    if app.pa.rename.is_some() {
        return false;
    }
    for b in crate::keymap::bindings().iter().filter(|b| b.area == "Project Audio") {
        if b.key == key && b.cmd == m.command && b.alt == m.alt && b.shift == m.shift {
            let c = b.command.clone();
            return command(app, &c);
        }
    }
    false
}

/// The pane (Project Files ▸ Project, or the window).
pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    app.pa.rect = Some(area);
    if app.pa.volume == 0.0 {
        app.pa.volume = 0.8;
    }
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    // ---- the local menus, then Play, Cycle and the volume
    let bar = Rect::from_min_size(area.min, vec2(area.width(), 28.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(8.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        local_menus(app, ui);
    });
    let tools = Rect::from_min_size(pos2(area.min.x, bar.max.y), vec2(area.width(), 30.0));
    let play = Rect::from_min_size(pos2(tools.min.x + 8.0, tools.min.y + 3.0), vec2(26.0, 24.0));
    let now = app.ctx.input(|i| i.time);
    let sel_id = selection_id(app);
    let playing = sel_id.as_ref().is_some_and(|(id, _, _)| app.preview.is_playing(id, now));
    let pr = ui.interact(play, ui.scope_id().with("pa-play"), Sense::click()).on_hover_text("Play the selected file or region");
    p.rect_filled(play, 5.0, if playing { look::accent() } else { look::group_bg() });
    crate::glyphs::paint(&p, play, if playing { crate::glyphs::ph_fill::STOP } else { crate::glyphs::ph_fill::PLAY }, 13.0, if playing { Color32::WHITE } else { look::text() });
    if pr.clicked() {
        if let Some((id, path, seg)) = sel_id.clone() {
            let (vol, cyc) = (app.pa.volume, app.pa.cycle);
            crate::preview::toggle(app, &id, &path, vol, seg, cyc);
        }
    }
    let cyc = Rect::from_min_size(pos2(play.max.x + 4.0, play.min.y), vec2(26.0, 24.0));
    let cr = ui.interact(cyc, ui.scope_id().with("pa-cycle"), Sense::click()).on_hover_text("Cycle: loop the playback");
    p.rect_filled(cyc, 5.0, if app.pa.cycle { look::cycle() } else { look::group_bg() });
    crate::glyphs::paint(&p, cyc, crate::glyphs::ph::REPEAT, 13.0, if app.pa.cycle { Color32::BLACK } else { look::text() });
    if cr.clicked() {
        app.pa.cycle = !app.pa.cycle;
    }
    let vol_r = Rect::from_min_max(pos2(cyc.max.x + 12.0, tools.min.y + 5.0), pos2((cyc.max.x + 140.0).min(tools.max.x - 8.0), tools.max.y - 5.0));
    crate::glyphs::paint(&p, Rect::from_min_size(vol_r.min - vec2(2.0, 0.0), vec2(14.0, vol_r.height())), crate::glyphs::ph::SPEAKER_HIGH, 11.0, look::text_dim());
    let mut v = app.pa.volume;
    if ui.put(Rect::from_min_max(vol_r.min + vec2(14.0, 0.0), vol_r.max), egui::Slider::new(&mut v, 0.0..=1.5).show_value(false)).on_hover_text("Preview volume").changed() {
        app.pa.volume = v;
        app.preview.set_volume(v);
    }
    p.hline(area.x_range(), tools.max.y, Stroke::new(1.0, look::line()));
    // ---- the column headers
    let hdr = Rect::from_min_size(pos2(area.min.x, tools.max.y + 1.0), vec2(area.width(), 20.0));
    p.rect_filled(hdr, 0.0, look::header_bg());
    let w = area.width();
    let (icon_x, name_x, info_x, bpm_x) = (area.min.x + 20.0, area.min.x + 56.0, area.min.x + w * 0.42, area.max.x - 44.0);
    for (x, t) in [(icon_x, "Icon"), (name_x, "Name"), (info_x, "Info"), (bpm_x, "Bpm")] {
        p.text(pos2(x, hdr.center().y), Align2::LEFT_CENTER, t, look::semibold(10.5), look::text_dim());
    }
    let list = Rect::from_min_max(pos2(area.min.x, hdr.max.y + 1.0), area.max);
    // ---- the rows
    let groups = arranged(app);
    if groups.iter().all(|g| g.1.is_empty()) {
        p.text(list.center(), Align2::CENTER_CENTER, "No audio files in the project\n(Audio File ▸ Add Audio File…, or drag files here)", look::regular(12.0), look::text_dim());
    }
    let mut rows: Vec<Row> = Vec::new();
    for (g, fs) in &groups {
        if app.pa.group != Group::None {
            rows.push(Row::Heading(g.clone()));
        }
        for (f, regions) in fs {
            rows.push(Row::File(f.clone(), regions.clone()));
            if app.pa.open.contains(f) {
                for r in regions {
                    rows.push(Row::Region(f.clone(), *r));
                }
            }
        }
    }
    let row_h = if app.pa.details { 36.0 } else { 22.0 };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    let mut clicked: Option<(Row, bool, bool)> = None;
    let mut dbl: Option<Row> = None;
    let mut drag_start: Option<Row> = None;
    egui::ScrollArea::vertical().id_salt("pa-list").auto_shrink([false, false]).show_rows(&mut child, row_h, rows.len(), |ui, range| {
        for k in range {
            let row = rows[k].clone();
            let (r, resp) = ui.allocate_exact_size(vec2(list.width(), row_h), Sense::click_and_drag());
            let pp = ui.painter().clone();
            match &row {
                Row::Heading(g) => {
                    pp.rect_filled(r, 0.0, look::header_bg());
                    pp.text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, g, look::semibold(11.5), look::text());
                }
                Row::File(f, regions) => {
                    let sel = app.pa.selected.contains(f) && app.pa.sel_region.is_none();
                    if sel {
                        pp.rect_filled(r, 0.0, look::accent().gamma_multiply(0.45));
                    } else if k % 2 == 1 {
                        pp.rect_filled(r, 0.0, look::lane_alt());
                    }
                    let open = app.pa.open.contains(f);
                    if !regions.is_empty() {
                        let tri = Rect::from_center_size(pos2(r.min.x + 10.0, r.center().y), vec2(14.0, 14.0));
                        crate::glyphs::paint(&pp, tri, if open { crate::glyphs::ph_fill::CARET_DOWN } else { crate::glyphs::ph_fill::CARET_RIGHT }, 10.0, look::text_dim());
                        if ui.interact(tri, ui.scope_id().with(("pa-tri", k)), Sense::click()).clicked() {
                            if open {
                                app.pa.open.remove(f);
                            } else {
                                app.pa.open.insert(f.clone());
                            }
                        }
                    }
                    // the Icon column: missing, follows the project tempo, has tempo information
                    let icon_r = Rect::from_center_size(pos2(icon_x + 6.0, r.center().y), vec2(14.0, 14.0));
                    let follows = regions.iter().any(|x| matches!(x, RegionRef::Placed(t, k) if app.audio_edit.regions.get(&(*t, *k)).is_some_and(|a| a.follow)));
                    let (glyph, col, tip) = if missing(f) {
                        (crate::glyphs::ph::WARNING, look::record_red(), "Missing file")
                    } else if follows {
                        (crate::glyphs::ph::METRONOME, look::accent(), "Follows the project tempo")
                    } else if file_bpm(f).is_some() {
                        (crate::glyphs::ph::CLOCK, look::text_dim(), "Has tempo information")
                    } else {
                        (crate::glyphs::ph::WAVEFORM, look::text_dim(), "Audio file")
                    };
                    crate::glyphs::paint(&pp, icon_r, glyph, 12.0, col);
                    ui.interact(icon_r, ui.scope_id().with(("pa-icon", k)), Sense::hover()).on_hover_text(tip);
                    let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    pp.with_clip_rect(Rect::from_min_max(r.min, pos2(info_x - 6.0, r.max.y))).text(pos2(name_x, r.center().y), Align2::LEFT_CENTER, &name, look::regular(12.0), look::text());
                    let fi = info(app, f);
                    let info_r = Rect::from_min_max(pos2(info_x, r.min.y + 2.0), pos2(bpm_x - 8.0, r.max.y - 2.0));
                    match &fi {
                        Some(i) => {
                            let secs = i.seconds.max(0.001);
                            let bar_r = if app.pa.details { Rect::from_min_max(pos2(info_r.min.x, info_r.min.y + 14.0), info_r.max) } else { info_r.shrink2(vec2(0.0, 4.0)) };
                            pp.rect_filled(bar_r, 2.0, look::lane_bg());
                            for rr in regions {
                                if let Some((_, s, l)) = region_span(app, *rr) {
                                    let a = bar_r.min.x + (s / secs).clamp(0.0, 1.0) as f32 * bar_r.width();
                                    let b = bar_r.min.x + ((s + l) / secs).clamp(0.0, 1.0) as f32 * bar_r.width();
                                    pp.rect_filled(Rect::from_min_max(pos2(a, bar_r.min.y), pos2(b.max(a + 1.0), bar_r.max.y)), 1.0, look::region_color(TrackKind::Audio).gamma_multiply(0.85));
                                }
                            }
                            if app.pa.details {
                                pp.with_clip_rect(info_r).text(info_r.left_top(), Align2::LEFT_TOP, i.text(), look::regular(10.5), look::text_dim());
                                // the waveform overview
                                if let Some(ov) = overview(app, f) {
                                    let n = ov.len().max(1);
                                    for (k, (lo, hi)) in ov.iter().enumerate() {
                                        let x = bar_r.min.x + k as f32 / n as f32 * bar_r.width();
                                        let c = bar_r.center().y;
                                        let h = bar_r.height() / 2.0;
                                        pp.vline(x, (c - hi * h)..=(c - lo * h), Stroke::new(1.0, look::text().gamma_multiply(0.6)));
                                    }
                                }
                            } else {
                                resp.clone().on_hover_text(i.text());
                            }
                        }
                        None => {
                            pp.text(info_r.left_center(), Align2::LEFT_CENTER, "File not found", look::regular(11.0), look::record_red());
                        }
                    }
                    if let Some(b) = file_bpm(f) {
                        pp.text(pos2(bpm_x, r.center().y), Align2::LEFT_CENTER, format!("{b:.0}"), look::regular(11.5), look::text());
                    }
                }
                Row::Region(f, rr) => {
                    let sel = app.pa.sel_region == Some(*rr);
                    if sel {
                        pp.rect_filled(r, 0.0, look::accent().gamma_multiply(0.45));
                    }
                    if let Some((name, s, l)) = region_span(app, *rr) {
                        let placed = matches!(rr, RegionRef::Placed(..));
                        pp.rect_filled(Rect::from_center_size(pos2(name_x + 4.0, r.center().y), vec2(8.0, 8.0)), 2.0, if placed { look::region_color(TrackKind::Audio) } else { look::text_dim() });
                        pp.with_clip_rect(Rect::from_min_max(r.min, pos2(info_x - 6.0, r.max.y))).text(pos2(name_x + 14.0, r.center().y), Align2::LEFT_CENTER, &name, look::regular(11.5), look::text());
                        let secs = info(app, f).map_or(1.0, |i| i.seconds.max(0.001));
                        let bar_r = Rect::from_min_max(pos2(info_x, r.min.y + 6.0), pos2(bpm_x - 8.0, r.max.y - 6.0));
                        pp.rect_stroke(bar_r, 1.0, Stroke::new(1.0, look::line()), egui::StrokeKind::Inside);
                        let a = bar_r.min.x + (s / secs).clamp(0.0, 1.0) as f32 * bar_r.width();
                        let b = bar_r.min.x + ((s + l) / secs).clamp(0.0, 1.0) as f32 * bar_r.width();
                        pp.rect_filled(Rect::from_min_max(pos2(a, bar_r.min.y), pos2(b.max(a + 1.0), bar_r.max.y)), 1.0, look::region_color(TrackKind::Audio));
                    }
                }
            }
            if resp.clicked() {
                let m = ui.input(|i| i.modifiers);
                clicked = Some((row.clone(), m.shift, m.command));
            }
            if resp.double_clicked() {
                dbl = Some(row.clone());
            }
            if resp.drag_started() {
                drag_start = Some(row.clone());
            }
            if !matches!(row, Row::Heading(_)) {
                resp.context_menu(|ui| {
                    if !matches!(row, Row::Heading(_)) && clicked.is_none() {
                        select_row(app, &row, false, false);
                    }
                    for c in ["Add Region", "Rename…", "Delete Files", "Backup Files", "Export Region Information", "Import Region Information"] {
                        if ui.button(c).clicked() {
                            command(app, c);
                            ui.close();
                        }
                    }
                    if ui.button("Show File in Finder").clicked() {
                        if let Row::File(f, _) | Row::Region(f, _) = &row {
                            reveal(f);
                        }
                        ui.close();
                    }
                });
            }
        }
    });
    if let Some((row, shift, cmd)) = clicked {
        select_row(app, &row, shift, cmd);
    }
    if let Some(row) = dbl {
        // a placed region: select it in the Tracks area; a file: show its regions
        match row {
            Row::Region(_, RegionRef::Placed(t, r)) => {
                app.selected_track = Some(t);
                app.selected_region = Some((t, r));
                app.selected_regions = vec![(t, r)];
            }
            Row::File(f, _) => {
                if !app.pa.open.remove(&f) {
                    app.pa.open.insert(f);
                }
            }
            _ => {}
        }
    }
    if let Some(row) = drag_start {
        let item = match &row {
            Row::File(f, _) => Some((f.clone(), None)),
            Row::Region(f, rr) => Some((f.clone(), region_span(app, *rr).map(|x| (x.1, x.2)))),
            _ => None,
        };
        if let Some((f, seg)) = item {
            crate::browser::start_file_drag(app, f, seg);
        }
    }
    // files dropped from the desktop onto the pane
    let dropped: Vec<PathBuf> = ui.ctx().input(|i| if i.pointer.hover_pos().is_some_and(|p| area.contains(p)) { i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| crate::soundlib::is_audio(p)).collect() } else { Vec::new() });
    if !dropped.is_empty() {
        add_files(app, dropped);
    }
    dialogs(app, ui.ctx());
}

#[derive(Clone, Debug, PartialEq)]
enum Row {
    Heading(String),
    File(PathBuf, Vec<RegionRef>),
    Region(PathBuf, RegionRef),
}

fn select_row(app: &mut App, row: &Row, shift: bool, cmd: bool) {
    match row {
        Row::File(f, _) => {
            app.pa.sel_region = None;
            if shift || cmd {
                if !app.pa.selected.remove(f) {
                    app.pa.selected.insert(f.clone());
                }
            } else {
                app.pa.selected = [f.clone()].into();
            }
        }
        Row::Region(f, r) => {
            app.pa.selected = [f.clone()].into();
            app.pa.sel_region = Some(*r);
        }
        Row::Heading(_) => {}
    }
}

/// What Play plays: the selected region (its part of the file) or file.
fn selection_id(app: &App) -> Option<(String, PathBuf, Option<(f64, f64)>)> {
    let f = app.pa.selected.iter().next()?.clone();
    if let Some(r) = app.pa.sel_region {
        let (_, s, l) = region_span(app, r)?;
        return Some((format!("{}#{s:.3}", f.display()), f, Some((s, l))));
    }
    Some((f.display().to_string(), f, None))
}

pub fn reveal(f: &Path) {
    if let Some(dir) = f.parent() {
        let _ = std::process::Command::new("xdg-open").arg(dir).stdin(std::process::Stdio::null()).spawn();
    }
}

/// The local menus: Edit, View, Audio File.
fn local_menus(app: &mut App, ui: &mut egui::Ui) {
    let mut run: Option<&str> = None;
    ui.menu_button("Edit", |ui| {
        ui.set_min_width(220.0);
        for (c, k) in [("Undo", "⌘Z"), ("Redo", "⇧⌘Z")] {
            if ui.add(egui::Button::new(c).shortcut_text(k)).clicked() {
                run = Some(c);
                ui.close();
            }
        }
        ui.separator();
        for (c, k) in [("Delete Files", "⌘⌫"), ("Select Used", ""), ("Select Unused", "⇧U")] {
            if ui.add(egui::Button::new(c).shortcut_text(k)).clicked() {
                run = Some(c);
                ui.close();
            }
        }
        if ui.add(egui::Button::new("Select All").shortcut_text("⌘A")).clicked() {
            app.pa.selected = files(app).into_iter().map(|f| f.0).collect();
            ui.close();
        }
        ui.separator();
        for (c, k) in [("Rename…", "⇧↩"), ("Create Group…", "⌃G"), ("Delete Selected Audio File Groups", "")] {
            if ui.add(egui::Button::new(c).shortcut_text(k)).clicked() {
                run = Some(c);
                ui.close();
            }
        }
    });
    ui.menu_button("View", |ui| {
        ui.set_min_width(220.0);
        let d = app.pa.details;
        if ui.add(egui::Button::selectable(d, "Show File Info")).clicked() {
            run = Some(if d { "Hide Audio File Details" } else { "Show Audio File Details" });
            ui.close();
        }
        ui.separator();
        for (c, k) in [("Show All Regions", "⌥↓"), ("Hide All Regions", "⌥↑")] {
            if ui.add(egui::Button::new(c).shortcut_text(k)).clicked() {
                run = Some(c);
                ui.close();
            }
        }
        ui.separator();
        ui.menu_button("Group Files By", |ui| {
            for (g, n) in [(Group::None, "None"), (Group::Location, "Location"), (Group::Attributes, "File Attributes"), (Group::Selection, "Selection")] {
                if ui.add(egui::Button::selectable(app.pa.group == g, n)).clicked() {
                    app.pa.group = g;
                    ui.close();
                }
            }
        });
        ui.menu_button("Sort Files By", |ui| {
            for (s, n) in [(Sort::None, "None"), (Sort::Name, "Name"), (Sort::Size, "Size"), (Sort::Length, "Length")] {
                if ui.add(egui::Button::selectable(app.pa.sort == s, n)).clicked() {
                    app.pa.sort = s;
                    ui.close();
                }
            }
        });
    });
    ui.menu_button("Audio File", |ui| {
        ui.set_min_width(240.0);
        for (c, k) in [("Add Audio File…", "⌃F"), ("Add Region", "⌃R"), ("", ""), ("Delete Files", "⌘⌫"), ("Optimize Files…", "⌃O"), ("Backup Files", "⌃B"), ("Copy/Convert Files…", "⌃K"), ("Move Files out of Project", ""), ("", ""), ("Import Region Information", "⌃I"), ("Export Region Information", "⌃E")] {
            if c.is_empty() {
                ui.separator();
                continue;
            }
            if ui.add(egui::Button::new(c).shortcut_text(k)).clicked() {
                run = Some(c);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Show File in Finder").clicked() {
            if let Some(f) = app.pa.selected.iter().next() {
                reveal(f);
            }
            ui.close();
        }
    });
    if let Some(c) = run {
        match c {
            "Undo" => app.undo(),
            "Redo" => app.redo(),
            _ => {
                command(app, c);
            }
        }
    }
}

/// The sheets: rename, a group's name, Delete Files, Copy/Convert.
fn dialogs(app: &mut App, ctx: &egui::Context) {
    if let Some((what, mut text)) = app.pa.rename.take() {
        let mut done = None;
        let mut keep = true;
        egui::Window::new("Rename").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            let te = ui.text_edit_singleline(&mut text);
            crate::widgets::focus_once(ui, &te);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
                if ui.button("OK").clicked() || (te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    done = Some(text.clone());
                }
            });
        });
        if let Some(n) = done {
            rename_done(app, what, n);
        } else if keep {
            app.pa.rename = Some((what, text));
        }
    }
    if let Some(mut name) = app.pa.group_name.take() {
        let mut keep = true;
        let mut ok = false;
        egui::Window::new("Create Group").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            let te = ui.text_edit_singleline(&mut name);
            crate::widgets::focus_once(ui, &te);
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
                ok = ui.button("OK").clicked() || (te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            });
        });
        if ok {
            let sel = app.pa.selected.clone();
            for g in app.pa.groups.iter_mut() {
                g.1.retain(|f| !sel.contains(f));
            }
            app.pa.groups.push((name, sel));
            app.pa.group = Group::Selection;
        } else if keep {
            app.pa.group_name = Some(name);
        }
    }
    if app.pa.confirm_delete {
        let n = app.pa.selected.len();
        egui::Window::new("Delete Files").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.label(format!("Delete {n} file{} from the project? Their regions are removed too.", if n == 1 { "" } else { "s" }));
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    app.pa.confirm_delete = false;
                }
                if ui.button("Remove from Project").clicked() {
                    app.pa.confirm_delete = false;
                    delete_files(app, false);
                }
                if ui.button("Move to Trash").clicked() {
                    app.pa.confirm_delete = false;
                    delete_files(app, true);
                }
            });
        });
    }
    if let Some(mut c) = app.pa.convert.take() {
        let mut keep = true;
        let mut go = false;
        egui::Window::new("Copy/Convert Files").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            egui::ComboBox::from_label("Sample Rate").selected_text(format!("{} Hz", c.rate)).show_ui(ui, |ui| {
                for r in [44_100u32, 48_000, 88_200, 96_000] {
                    ui.selectable_value(&mut c.rate, r, format!("{r} Hz"));
                }
            });
            egui::ComboBox::from_label("Bit Depth").selected_text(format!("{} Bit", c.bits)).show_ui(ui, |ui| {
                for b in [16u16, 24] {
                    ui.selectable_value(&mut c.bits, b, format!("{b} Bit"));
                }
            });
            ui.checkbox(&mut c.move_refs, "Change file references in regions");
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
                go = ui.button("Choose Folder…").clicked();
            });
        });
        if go {
            convert(app, c);
        } else if keep {
            app.pa.convert = Some(c);
        }
    }
}

/// Window ▸ Open Project Audio (⌘8).
pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.pa.window {
        return;
    }
    let mut open = true;
    egui::Window::new("Project Audio").open(&mut open).default_size(vec2(620.0, 420.0)).resizable(true).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().max(420.0), ui.available_height().max(300.0)), Sense::hover());
        show(app, ui, r);
    });
    if !open {
        app.pa.window = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_and_writer() {
        let dir = std::env::temp_dir().join(format!("lg-pa-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("t.wav");
        let one = vec![0.25f32; 4410];
        write_wav(&p, &[one.clone(), one], 44_100, 22_050, 24).unwrap();
        let i = file_info(&p).unwrap();
        assert_eq!((i.rate, i.bits, i.channels), (22_050, 24, 2));
        assert!((i.seconds - 0.1).abs() < 0.001, "{}", i.seconds);
        assert!(i.text().contains("22050 Hz") && i.text().contains("Stereo"));
        let a = lg_synth::wav::read_audio(&p).unwrap();
        assert_eq!(a.frames(), 2205);
        assert!((a.channels[0][100] - 0.25).abs() < 0.001);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sizes() {
        assert_eq!(size_text(2048), "2 KB");
        assert_eq!(size_text(3 << 20), "3.0 MB");
    }
}
