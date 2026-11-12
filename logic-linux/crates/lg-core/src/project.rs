//! A Logic project read from its package: <Name>.logicx/ (or a project folder holding one).
//!
//! - Resources/ProjectInformation.plist: VariantNames, the project's alternatives (folders
//!   under Alternatives/, not always 000).
//! - Alternatives/<NNN>/MetaData.plist: tempo, sample rate, key, signature, track count, audio
//!   files (plain plist).
//! - Alternatives/<NNN>/ProjectData: the song (chunks.rs, events.rs).
//!
//! How the song hangs together (decoded from Logic 13 sample projects):
//! - MSeq / Trak / EvSq chunks with the same id are one sequence: a folder or a region.
//!   An MSeq's name is at payload offset 16 (u16 length + text).
//! - The song's top folder is sequence 0x40000 (named after the project). Its event list
//!   places each region (kind 0x20; audio regions 0x24) at a position, on a track row, on a
//!   channel object (Envi chunk id >> 16, whose name is the track's name).
//! - A region's notes are in its own event list, at song positions.
//! - The global event list (EvSq id 0, the first one) holds the time and key signatures; the
//!   tempo is in the Song chunk (BPM × 10000 at 0x92 from its start).
//! - Audio: AuRg chunks are the audio regions (name), AuFl the files (UTF-16 name); the files
//!   are in Media/Audio Files of the package, or Audio Files of a project folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::chunks::{self, Chunk};
use crate::events::{self, Note};
use crate::plugin::{self, Plugin};

#[derive(Clone, Debug, Default)]
pub struct Project {
    pub name: String,
    /// The package (or project folder) read.
    pub path: PathBuf,
    pub tempo: f64,
    /// Tempo changes: (ticks from bar 1, BPM), first at 0 (the global tempo list's 0x60 events).
    pub tempo_map: Vec<(u32, f64)>,
    pub signature: (u8, u8),
    pub key: String,
    pub sample_rate: u32,
    pub tracks: Vec<Track>,
    /// Plug-ins found in the channel strips (display name, settings).
    pub plugins: Vec<Plugin>,
    /// Where the project's audio files are.
    pub audio_dir: Option<PathBuf>,
    /// Things read in a way still being checked (shown in the app's project info).
    pub notes: Vec<String>,
    /// The mixer: channel strips, sends, auxes, outputs, groups (lg-core mixer).
    pub mixer: crate::mixer::Mixer,
    /// Take folders made by recording here (not read from Logic's file yet).
    pub takes: Vec<crate::takes::TakeFolder>,
    /// The Transposition global track: (ticks from bar 1, semitones), each holding until the
    /// next (MIDI regions on tracks that take transposition play shifted).
    pub transposition: Vec<(u32, i8)>,
    /// The Score editor's attributes, staff styles and symbols (lg-core score).
    pub score: crate::score::Score,
    /// Markers (the Marker track): (ticks from bar 1, name).
    pub markers: Vec<crate::markers::Marker>,
    /// Arrangement markers: (ticks from bar 1, length in ticks, name).
    pub arrangement: Vec<(u32, u32, String)>,
    /// Tempo curves and sets, signature and key changes, beat mappings (timeline.rs).
    pub global: crate::timeline::Global,
    /// Groove templates (Make Groove Template; groove.rs).
    pub grooves: Vec<crate::groove::Groove>,
    /// The Project End marker, ticks from bar 1 (None: Logic's default, bar 129; projend.rs).
    pub end: Option<u32>,
    /// The Project Start marker, ticks from bar 1: 0, or earlier (bars 0, -1 …) for upbeats.
    pub start: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    Instrument,
    Drummer,
    Audio,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub name: String,
    pub kind: TrackKind,
    /// The channel object (Envi id >> 16).
    pub channel: u32,
    pub row: u32,
    pub regions: Vec<Region>,
    pub volume_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Track automation (Logic's "*Automation" sequences): per parameter (7 = volume, 10 = pan,
    /// Logic's numbering), points at song ticks with Logic's 0-127 value.
    pub automation: Vec<AutoLane>,
    /// The track's channel strip: its instrument and Audio FX, in slot order (read from the
    /// project; empty for tracks made here).
    pub plugins: Vec<Plugin>,
    /// The Tracks area's own state for the track: zoom, hidden, track stacks and folders,
    /// track alternatives (arrange.rs).
    pub extra: crate::arrange::TrackExtra,
}

/// One automation lane: a parameter and its points (song ticks, 0-127).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AutoLane {
    pub param: u8,
    pub points: Vec<(u32, u8)>,
    /// Curved segments: (the segment's first point's tick, curve -99..99; 0 or missing =
    /// straight), as the Automation Curve tool bends them.
    pub curves: Vec<(u32, i8)>,
    /// A plug-in parameter's lane (the channel strip's slot, the parameter's id) in place of
    /// Logic's channel parameter `param`.
    pub plugin: Option<(u32, u32)>,
    /// The parameter's automation switched off (the On/Off button on the parameter pop-up):
    /// kept, drawn gray, not played.
    pub muted: bool,
    /// A Relative curve (Trim / Relative modes): offsets the absolute lane of the same
    /// parameter by its value minus 64.
    pub relative: bool,
    /// Stepped segments: the first point's tick of each segment that holds its value up to
    /// the next point (the Automation Curve tool's Option-click).
    pub steps: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Region {
    pub name: String,
    /// Start and length in ticks from bar 1 (960 per quarter note).
    pub start: u32,
    pub length: u32,
    /// Notes at song positions (ticks from bar 1).
    pub notes: Vec<Note>,
    /// An audio region's file (absolute path) and how long it is in seconds.
    pub audio: Option<AudioRef>,
    pub muted: bool,
    /// Where the region came from in the file it was read from (None: made here), for Save.
    pub source: Option<Source>,
    /// A region made here from one that was read (a copy, a paste, Repeat): Save writes it as a
    /// new sequence modelled on that one.
    pub origin: Option<Source>,
    /// The Region inspector's playback parameters (Logic's: applied as it plays, the notes kept).
    pub props: RegionProps,
    /// MIDI events other than notes, aliases and clones (arrange.rs).
    pub ext: crate::regions::RegionExt,
}

/// Logic's MIDI region parameters (Region inspector): played through, never written into the
/// notes until "Apply All Parameters Permanently".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionProps {
    /// Loop: repeat to the next region on the track (or the project end).
    pub looped: bool,
    /// Quantize grid in ticks (0: off); Q-Swing 0..100 (50 = straight).
    pub quantize: u32,
    pub swing: u8,
    /// Semitones; Fine Tune in cents (instruments that take pitch bend).
    pub transpose: i8,
    pub fine_tune: i8,
    /// Velocity offset (-99..99) and Dynamics (percent, 100 = as played; Fixed = 0).
    pub velocity: i8,
    pub dynamics: u16,
    /// Gate Time percent (100 = as played; 0 = Fixed).
    pub gate_time: u16,
    /// Delay in ticks (negative: early).
    pub delay: i32,
    /// Where the loop repetitions end when the loop was dragged out at the region's upper
    /// right corner (ticks from bar 1; 0: up to the next region or the project's end).
    pub loop_to: u32,
    /// Audio regions: fade-in and fade-out (length in ms, curve -99..99), as Logic saves them
    /// (format-analysis §8), and where the region starts in its file (samples, AuRg @0x06).
    pub fade_in: (u16, i8),
    pub fade_out: (u16, i8),
    pub audio_offset: u32,
    /// Q-Strength (percent of the way to the grid), Q-Range (ticks: 0 all notes, positive only
    /// notes farther off, negative only notes nearer), Q-Flam (ticks between chord notes).
    pub q_strength: u8,
    pub q_range: i16,
    pub q_flam: i16,
    /// Clip Length: notes end at the region's right border.
    pub clip_length: bool,
    /// Pitch Source: 0 Off, 1 Key Signature (MIDI regions follow key changes).
    pub pitch_source: u8,
    /// Lock Time Position: the region does not move.
    pub locked: bool,
    /// The region's own colour (palette index; None: its track's).
    pub color: Option<u8>,
    /// Delay shown (and typed) in milliseconds rather than ticks.
    pub delay_ms: bool,
    /// A caller's mark, carried through edits and copies (not a Logic parameter).
    pub tag: u32,
    /// Q-Velocity / Q-Length: percent of a groove template's velocities / lengths taken in
    /// (negative: the difference exaggerated).
    pub q_velocity: i16,
    pub q_length: i16,
}

/// Gate Time "Legato": every note lasts to the next one.
pub const GATE_LEGATO: u16 = u16::MAX;

impl Default for RegionProps {
    fn default() -> Self {
        RegionProps { looped: false, quantize: 0, swing: 50, transpose: 0, fine_tune: 0, velocity: 0, dynamics: 100, gate_time: 100, delay: 0, loop_to: 0, fade_in: (0, 0), fade_out: (0, 0), audio_offset: 0, q_strength: 100, q_range: 0, q_flam: 0, clip_length: false, pitch_source: 0, locked: false, color: None, delay_ms: false, tag: 0, q_velocity: 0, q_length: 0 }
    }
}

impl RegionProps {
    /// A note as the region plays it.
    pub fn apply(&self, n: &crate::events::Note, region_start: u32) -> crate::events::Note {
        let mut out = *n;
        if let Some((q, swing)) = crate::groove::grid(self.quantize, self.swing) {
            let rel = n.pos as i64 - region_start as i64;
            let k = (rel as f64 / q).round() as i64;
            let mut grid = (k as f64 * q).round() as i64;
            // Q-Swing: every other grid point later (or earlier) by up to half a step
            if k % 2 == 1 {
                grid += (q * (swing as f64 - 50.0) / 100.0) as i64;
            }
            // Q-Range: 0 every note; positive only notes farther off than it; negative only
            // notes within it. Q-Strength: how far toward the grid.
            let off = region_start as i64 + grid - n.pos as i64;
            let r = self.q_range as i64;
            let take = r == 0 || (r > 0 && off.abs() > r) || (r < 0 && off.abs() <= -r);
            if take {
                let moved = (off as f64 * self.q_strength.min(100) as f64 / 100.0).round() as i64;
                out.pos = (n.pos as i64 + moved).max(0) as u32;
            }
        }
        out.pos = (out.pos as i64 + self.delay as i64).max(0) as u32;
        out.pitch = (n.pitch as i16 + self.transpose as i16).clamp(0, 127) as u8;
        let v = if self.dynamics == 0 { 64.0 } else { 64.0 + (n.velocity as f32 - 64.0) * self.dynamics as f32 / 100.0 };
        out.velocity = (v + self.velocity as f32).round().clamp(1.0, 127.0) as u8;
        out.len = if self.gate_time == 0 { 120 } else if self.gate_time == GATE_LEGATO { n.len } else { ((n.len as u64 * self.gate_time as u64) / 100).max(1) as u32 };
        out
    }
}

/// A region's place in the original ProjectData: the folder whose event list places it, its
/// position there as read (song ticks, Logic's own count), and its sequence (MSeq id).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Source {
    pub folder: u32,
    pub pos: u32,
    pub seq: Option<u32>,
    pub audio_region: u32,
}

#[derive(Clone, Debug)]
pub struct AudioRef {
    pub file: PathBuf,
    pub seconds: f64,
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    NotAProject(String),
    Data(chunks::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::NotAProject(s) => write!(f, "{s}"),
            Error::Data(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<chunks::Error> for Error {
    fn from(e: chunks::Error) -> Self {
        Error::Data(e)
    }
}

/// Channel objects that aren't tracks (the environment's fixed parts).
const NOT_TRACKS: &[&str] = &["Not Assigned", "MIDI Click", "(Folder)", "Sequencer Input", "Physical Input", "Input View", "Input Notes", "Preview", "Click", "Stereo Out", "Master"];

/// The package inside a project folder (the "folder" save: <X>/<X>.logicx), or the path itself.
pub fn package_of(path: &Path) -> PathBuf {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("logicx")) {
        return path.to_path_buf();
    }
    if let Ok(rd) = std::fs::read_dir(path) {
        for e in rd.flatten() {
            if e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("logicx")) {
                return e.path();
            }
        }
    }
    path.to_path_buf()
}

/// The alternatives of a package, in VariantNames order (or the folders found), each with its
/// name.
pub fn alternatives(pkg: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let info = pkg.join("Resources/ProjectInformation.plist");
    if let Ok(plist::Value::Dictionary(d)) = plist::Value::from_file(&info) {
        if let Some(plist::Value::Dictionary(v)) = d.get("VariantNames") {
            let mut keys: Vec<(&String, &plist::Value)> = v.iter().collect();
            keys.sort_by_key(|(k, _)| k.parse::<u32>().unwrap_or(u32::MAX));
            for (k, name) in keys {
                let dir = pkg.join(format!("Alternatives/{:03}", k.parse::<u32>().unwrap_or(0)));
                if dir.join("ProjectData").exists() {
                    out.push((name.as_string().unwrap_or("").to_string(), dir));
                }
            }
        }
    }
    if out.is_empty() {
        if let Ok(rd) = std::fs::read_dir(pkg.join("Alternatives")) {
            let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.join("ProjectData").exists()).collect();
            dirs.sort();
            out = dirs.into_iter().map(|d| (d.file_name().unwrap_or_default().to_string_lossy().into_owned(), d)).collect();
        }
    }
    out
}

/// GarageBand's .band: `projectData` is an NSKeyedArchiver plist whose "DfDocument logic model"
/// holds a Logic ProjectData (NS.data); `Output/metadata.plist` has the tempo, key and title.
pub fn open_band(pkg: &Path) -> Result<Project, Error> {
    let archive = plist::Value::from_file(pkg.join("projectData")).map_err(|e| Error::NotAProject(format!("{}: {e}", pkg.display())))?;
    let data = band_logic_model(&archive).ok_or_else(|| Error::NotAProject(format!("{} has no Logic model in its projectData", pkg.display())))?;
    let mut p = read_project_data(&data)?;
    p.path = pkg.to_path_buf();
    p.name = pkg.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if let Ok(plist::Value::Dictionary(m)) = plist::Value::from_file(pkg.join("Output/metadata.plist")) {
        let get = |k: &str| m.get(&format!("com_apple_garageband_metadata_{k}"));
        if let Some(t) = get("songTempo").and_then(|v| v.as_real().or_else(|| v.as_signed_integer().map(|i| i as f64))) {
            if t > 0.0 {
                p.tempo = t;
                if p.tempo_map.len() == 1 {
                    p.tempo_map[0].1 = t;
                }
            }
        }
        if let (Some(k), Some(g)) = (get("songKey").and_then(|v| v.as_string()), get("songGender").and_then(|v| v.as_string())) {
            p.key = format!("{k} {g}");
        }
    }
    p.audio_dir = Some(pkg.join("Media")).filter(|d| d.is_dir());
    attach_audio(&mut p);
    if p.sample_rate == 0 {
        p.sample_rate = 44100;
    }
    Ok(p)
}

/// The Logic ProjectData bytes inside a GarageBand projectData archive.
pub fn band_logic_model(archive: &plist::Value) -> Option<Vec<u8>> {
    let d = archive.as_dictionary()?;
    let objects = d.get("$objects")?.as_array()?;
    // (references: plist UIDs in binary archives, {CF$UID: n} dictionaries in XML ones)
    let resolve = |v: &plist::Value| -> Option<plist::Value> {
        match v {
            plist::Value::Uid(u) => objects.get(u.get() as usize).cloned(),
            plist::Value::Dictionary(d) if d.len() == 1 && d.contains_key("CF$UID") => objects.get(d.get("CF$UID")?.as_unsigned_integer()? as usize).cloned(),
            other => Some(other.clone()),
        }
    };
    let top = d.get("$top")?.as_dictionary()?;
    let model = resolve(top.get("DfDocument logic model")?)?;
    let bytes = match &model {
        plist::Value::Data(b) => b.clone(),
        plist::Value::Dictionary(m) => match resolve(m.get("NS.data")?)? {
            plist::Value::Data(b) => b,
            _ => return None,
        },
        _ => return None,
    };
    (bytes.len() > 24 && bytes[..4] == chunks::MAGIC).then_some(bytes)
}

/// Audio regions' files: by name in the project's audio folder, with their lengths.
pub(crate) fn attach_audio(p: &mut Project) {
    let Some(dir) = p.audio_dir.clone() else { return };
    let tempo = p.tempo;
    for t in &mut p.tracks {
        for r in &mut t.regions {
            if let Some(a) = &mut r.audio {
                let name = a.file.file_name().unwrap_or_default().to_os_string();
                let direct = dir.join(&name);
                a.file = if direct.exists() {
                    direct
                } else {
                    // (GarageBand keeps recordings in Media/ and Media/Recording/)
                    find_file(&dir, &name).unwrap_or(direct)
                };
                // (the region's own length from the project; else the whole file)
                if a.seconds <= 0.0 {
                    a.seconds = wav_seconds(&a.file).unwrap_or(a.seconds);
                }
                if a.seconds > 0.0 && tempo > 0.0 {
                    r.length = (a.seconds * tempo / 60.0 * events::PPQ as f64).round() as u32;
                }
            }
        }
    }
}

fn find_file(dir: &Path, name: &std::ffi::OsStr) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_file(&p, name) {
                return Some(f);
            }
        } else if p.file_name() == Some(name) {
            return Some(p);
        }
    }
    None
}

/// Reads a project: a .logicx package, a project folder holding one (the alternative that was
/// open when it was saved: ProjectInformation.plist ActiveVariant, else the first), or a
/// GarageBand .band.
pub fn open(path: &Path) -> Result<Project, Error> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("band")) || path.join("projectData").is_file() {
        return open_band(path);
    }
    let pkg = package_of(path);
    let Some(alt) = crate::package::active(&pkg).map(|a| a.dir) else {
        return Err(Error::NotAProject(format!("{} has no project data (Alternatives/…/ProjectData)", pkg.display())));
    };
    let data = std::fs::read(alt.join("ProjectData"))?;
    open_alternative(path, &alt, &data)
}

/// Reads one alternative of a package (its folder), from `data` (its ProjectData, or a backup
/// of it).
pub fn open_alternative(path: &Path, alt: &Path, data: &[u8]) -> Result<Project, Error> {
    let pkg = package_of(path);
    let mut p = read_project_data(data)?;
    p.path = path.to_path_buf();
    p.name = pkg.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    // the summary plist: tempo and the rest as Logic shows them
    if let Ok(plist::Value::Dictionary(m)) = plist::Value::from_file(alt.join("MetaData.plist")) {
        if let Some(t) = m.get("BeatsPerMinute").and_then(|v| v.as_real().or_else(|| v.as_signed_integer().map(|i| i as f64))) {
            if p.tempo <= 0.0 {
                p.tempo = t;
            }
        }
        if let Some(r) = m.get("SampleRate").and_then(|v| v.as_signed_integer()) {
            p.sample_rate = r as u32;
        }
        if let (Some(k), Some(g)) = (m.get("SongKey").and_then(|v| v.as_string()), m.get("SongGenderKey").and_then(|v| v.as_string())) {
            p.key = format!("{k} {g}");
        }
    }
    // where the audio files are
    let folder = path.join("Audio Files");
    let media = pkg.join("Media/Audio Files");
    p.audio_dir = [media, folder].into_iter().find(|d| d.is_dir());
    attach_audio(&mut p);
    if p.sample_rate == 0 {
        p.sample_rate = 48000;
    }
    Ok(p)
}

/// Reads the song from ProjectData bytes.
pub fn read_project_data(data: &[u8]) -> Result<Project, Error> {
    let chunks = chunks::walk(data)?;
    let mut p = Project { tempo: 0.0, signature: (4, 4), key: "C major".into(), ..Default::default() };
    // (undo history: whole mini projects inside USEl chunks — not part of the song)
    let live: Vec<&Chunk> = chunks.iter().filter(|c| !c.is("USEl") && !c.is("UStk")).collect();
    // tempo: the Song chunk
    if let Some(song) = live.iter().find(|c| c.is("Song")) {
        if let Some(t) = song.u32_at(0x92).filter(|t| *t > 0) {
            p.tempo = t as f64 / 10000.0;
        }
    }
    p.end = crate::projend::read(data);
    // the global events: signatures, key
    if let Some(global) = live.iter().find(|c| c.is("EvSq") && c.id == 0) {
        // (the song's own: the first of each; later ones are changes mid-song)
        let recs = events::records(global.payload);
        if let Some(s) = recs.iter().find_map(events::signature) {
            p.signature = s;
        }
        if let Some(k) = recs.iter().find_map(events::key) {
            p.key = events::key_string(k);
        }
    }
    // the tempo list: the second global event list (EvSq id 0), kind 0x60, BPM × 10000 in the
    // second unit
    if let Some(tempos) = live.iter().filter(|c| c.is("EvSq") && c.id == 0).nth(1) {
        for r in events::records(tempos.payload) {
            if r.kind == 0x60 && r.units() >= 2 {
                let bpm = r.u32_at(16) as f64 / 10000.0;
                if (5.0..=990.0).contains(&bpm) {
                    p.tempo_map.push((r.pos.saturating_sub(events::EVENT_BAR_ONE), bpm));
                }
            }
        }
        p.tempo_map.sort_by_key(|t| t.0);
        // (events count bar 1 from 38400: E23's change at bar 7 sits at 61440)
        if let Some(first) = p.tempo_map.first_mut() {
            first.0 = 0;
        }
        p.tempo_map.dedup_by(|b, a| a.1 == b.1);
    }
    // sequences by id: name and events (the last MSeq / EvSq of an id wins: earlier ones are
    // older copies kept in some files)
    let mut names: BTreeMap<u32, String> = BTreeMap::new();
    let mut evs: BTreeMap<u32, &[u8]> = BTreeMap::new();
    for c in &live {
        if c.is("MSeq") {
            if let Some(n) = chunks::short_string(c.payload, 16) {
                names.insert(c.id, n);
            }
        } else if c.is("EvSq") && c.id != 0 {
            evs.insert(c.id, c.payload);
        }
    }
    // channel objects: name by id >> 16
    // (Envi payload: the name at 158 as u16 length + text; byte 157 = 0x1e on aux and output
    // channels)
    let mut channels: BTreeMap<u32, String> = BTreeMap::new();
    let mut aux: std::collections::HashSet<u32> = std::collections::HashSet::new();
    for c in live.iter().filter(|c| c.is("Envi")) {
        if let Some(n) = chunks::short_string(c.payload, 158).filter(|n| !n.is_empty()) {
            channels.insert(c.id >> 16, n);
        }
        if c.payload.get(157) == Some(&0x1e) {
            aux.insert(c.id >> 16);
        }
    }
    // drummer regions (a GenM generator memento travels with them)
    let has_generator = live.iter().any(|c| c.is("GenM"));
    // audio regions and files, in file order
    let audio_regions: Vec<String> = live.iter().filter(|c| c.is("AuRg")).filter_map(|c| audio_region_name(c.payload)).collect();
    let audio_files: Vec<String> = live.iter().filter(|c| c.is("AuFl")).filter_map(|c| audio_file_name(c.payload)).collect();
    // (a placement names its file by the AuFl / AuRg header id >> 16 and its region by the
    // AuRg's number among that file's, header u32 @12 >> 16)
    let file_by_id: BTreeMap<u32, String> = live.iter().filter(|c| c.is("AuFl")).filter_map(|c| Some((c.id >> 16, audio_file_name(c.payload)?))).collect();
    let rate_by_id: BTreeMap<u32, u32> = live.iter().filter(|c| c.is("AuFl")).filter_map(|c| Some((c.id >> 16, audio_file_format(c.payload)?.1))).collect();
    // (name, offset and length in samples: AuRg payload @0x06 and @0x16, format-analysis §25)
    let region_by_ref: BTreeMap<(u32, u32), (String, u32, u32)> = live
        .iter()
        .filter(|c| c.is("AuRg"))
        .filter_map(|c| Some(((c.id >> 16, c.u32_at(12)? >> 16), (audio_region_name(c.payload)?, u32::from_le_bytes(c.payload.get(6..10)?.try_into().ok()?), u32::from_le_bytes(c.payload.get(0x16..0x1A)?.try_into().ok()?)))))
        .collect();
    // the regions, from the top folder down (folders and track stacks hold more placements)
    let mut placed: Vec<(events::Placement, Option<u32>, u32)> = Vec::new();
    let mut stack = vec![0x40000u32];
    let mut seen = std::collections::HashSet::new();
    while let Some(folder) = stack.pop() {
        if !seen.insert(folder) {
            continue;
        }
        let Some(ev) = evs.get(&folder) else { continue };
        for r in events::records(ev) {
            let Some(pl) = events::placement(&r) else { continue };
            let seq = pl.sequence.map(|s| s << 16);
            // a placed folder (its events place more): look inside
            if let Some(s) = seq {
                if evs.get(&s).is_some_and(|e| events::records(e).iter().any(|x| events::placement(x).is_some())) {
                    stack.push(s);
                    continue;
                }
            }
            placed.push((pl, seq, folder));
        }
    }
    let mut by_row: BTreeMap<u32, Track> = BTreeMap::new();
    for (pl, seq, folder) in placed {
        let channel_name = channels.get(&pl.channel).cloned().unwrap_or_else(|| format!("Track {}", pl.row));
        let mut region = Region { name: String::new(), start: pl.pos.saturating_sub(events::BAR_ONE), length: 0, notes: Vec::new(), audio: None, muted: pl.muted, source: Some(Source { folder, pos: pl.pos, seq, audio_region: pl.audio_region }), origin: None, props: Default::default(), ext: Default::default() };
        let mut kind = TrackKind::Instrument;
        if pl.audio {
            kind = TrackKind::Audio;
            let found = region_by_ref.get(&(pl.audio_file, pl.audio_index));
            region.name = found.map(|f| f.0.clone()).or_else(|| audio_regions.first().cloned()).unwrap_or_else(|| "Audio".into());
            let file = file_by_id.get(&pl.audio_file).or(audio_files.first()).cloned().unwrap_or_default();
            // (the region's own length when the AuRg gives one; 3 marks a take folder's empty head)
            let rate = rate_by_id.get(&pl.audio_file).copied().filter(|r| *r > 0).unwrap_or(48000) as f64;
            let seconds = found.map(|f| f.2).filter(|l| *l > 3).map_or(0.0, |l| l as f64 / rate);
            region.audio = Some(AudioRef { file: PathBuf::from(file), seconds });
            region.props.audio_offset = found.map_or(0, |f| f.1);
            region.props.fade_out = (pl.fade_out.0, pl.fade_out.1);
            region.props.fade_in = (pl.fade_in.0, pl.fade_in.1);
        } else if let Some(s) = seq {
            region.props.looped = pl.looped;
            region.props.velocity = pl.velocity;
            region.props.transpose = pl.transpose;
            region.name = names.get(&s).cloned().unwrap_or_default();
            if let Some(ev) = evs.get(&s) {
                let recs = events::records(ev);
                region.notes = recs.iter().filter_map(events::note).map(|mut n| {
                    n.pos = n.pos.saturating_sub(events::BAR_ONE);
                    n
                }).collect();
                // controllers, pitch bend, pressure, program changes: from the region's start
                let at = pl.pos;
                region.ext.events = recs.iter().filter_map(events::control).map(|mut c| {
                    c.pos = c.pos.saturating_sub(at);
                    c
                }).collect();
            }
            if has_generator && region.name.starts_with("Drummer") {
                kind = TrackKind::Drummer;
            }
        }
        if let Some(a) = region.audio.as_ref().filter(|a| a.seconds > 0.0 && p.tempo > 0.0) {
            region.length = (a.seconds * p.tempo / 60.0 * events::PPQ as f64).round() as u32;
        }
        // length: to the last note's end, whole bars, at least one bar (regions don't store a
        // length when it hasn't been changed — still being checked)
        if region.length == 0 {
            let bar = bar_ticks(p.signature);
            let end = region.notes.iter().map(|n| n.pos + n.len).max().unwrap_or(region.start);
            let len = end.saturating_sub(region.start).max(1);
            region.length = len.div_ceil(bar) * bar;
        }
        let t = by_row.entry(pl.row).or_insert_with(|| Track { name: channel_name, kind, channel: pl.channel, row: pl.row, regions: Vec::new(), volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new(), automation: Vec::new(), extra: Default::default() });
        if kind != TrackKind::Instrument {
            t.kind = kind;
        }
        t.regions.push(region);
    }
    // tracks without regions: the remaining channel objects that are tracks (instrument and
    // audio channels after the environment's fixed objects)
    let used: std::collections::HashSet<u32> = by_row.values().map(|t| t.channel).collect();
    let mut next_row = by_row.keys().max().copied().unwrap_or(0) + 1;
    for (id, name) in &channels {
        if used.contains(id) || aux.contains(id) || name.contains('/') || NOT_TRACKS.contains(&name.as_str()) || name.starts_with("Aux ") || name.starts_with("Bus ") || name.starts_with("Sum ") || name.starts_with("Output") || *id < 0x58 {
            continue;
        }
        let kind = if name.starts_with("Audio ") { TrackKind::Audio } else { TrackKind::Instrument };
        by_row.insert(next_row, Track { name: name.clone(), kind, channel: *id, row: next_row, regions: Vec::new(), volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new(), automation: Vec::new(), extra: Default::default() });
        next_row += 1;
    }
    p.tracks = by_row.into_values().collect();
    for t in &mut p.tracks {
        t.regions.sort_by_key(|r| r.start);
    }
    p.plugins = live
        .iter()
        .filter(|c| c.is("AuCU"))
        .filter_map(|c| {
            let mut u = plugin::read_unit(c.payload)?;
            u.channel = c.u32_at(12).unwrap_or(0) >> 16;
            u.slot = c.u32_at(16).unwrap_or(0) >> 16;
            Some(u)
        })
        .collect();
    // each instrument track's strip: the channels with an instrument (slot 2; Klopfgeist is the
    // metronome's) in channel order line up with the instrument tracks in channel-object order
    let mut strips: BTreeMap<u32, Vec<Plugin>> = BTreeMap::new();
    for u in &p.plugins {
        strips.entry(u.channel).or_default().push(u.clone());
    }
    let instrument_strips: Vec<Vec<Plugin>> = strips
        .into_values()
        .filter(|v| v.iter().any(|u| u.slot == 2 && u.name != "Klopfgeist"))
        .map(|mut v| {
            v.sort_by_key(|u| u.slot);
            v
        })
        .collect();
    let mut order: Vec<usize> = (0..p.tracks.len()).filter(|&i| p.tracks[i].kind != TrackKind::Audio).collect();
    order.sort_by_key(|&i| p.tracks[i].channel);
    order.dedup_by_key(|i| p.tracks[*i].channel);
    for (k, i) in order.into_iter().enumerate() {
        if let Some(strip) = instrument_strips.get(k) {
            let ch = p.tracks[i].channel;
            for t in p.tracks.iter_mut().filter(|t| t.channel == ch) {
                t.plugins = strip.clone();
            }
        }
    }
    crate::mixer::read(&live, &mut p);
    crate::writeback::read_markers(&live, &mut p);
    // track automation: the "Track Automation Root Folder" places each track's "*Automation"
    // sequence under its channel; that sequence's EvSq holds the points (0x5n records)
    let folder = live.iter().filter(|c| c.is("MSeq") && chunks::short_string(c.payload, 16).as_deref() == Some("Track Automation Root Folder")).map(|c| c.id).last();
    if let Some(f) = folder {
        if let Some(fe) = live.iter().filter(|c| c.is("EvSq") && c.id == f).last() {
            for pl in events::records(fe.payload).iter().filter_map(events::placement) {
                let Some(seq) = pl.sequence.map(|s| s << 16) else { continue };
                let Some(ev) = live.iter().filter(|c| c.is("EvSq") && c.id == seq).last() else { continue };
                let mut lanes: BTreeMap<u8, Vec<(u32, u8)>> = BTreeMap::new();
                for r in events::records(ev.payload) {
                    if let Some((param, value)) = events::automation(&r) {
                        lanes.entry(param).or_default().push((r.pos.saturating_sub(events::BAR_ONE), value));
                    }
                }
                for t in p.tracks.iter_mut().filter(|t| t.channel == pl.channel) {
                    t.automation = lanes.iter().map(|(param, pts)| AutoLane { param: *param, points: pts.clone(), ..Default::default() }).collect();
                }
            }
        }
    }
    if p.tempo <= 0.0 {
        p.tempo = p.tempo_map.first().map_or(120.0, |t| t.1);
    }
    // the map starts at the song's start with the song tempo
    if p.tempo_map.first().is_none_or(|t| t.0 > 0) {
        p.tempo_map.insert(0, (0, p.tempo));
    }
    p.tempo_map[0].0 = 0;
    Ok(p)
}

impl Project {
    /// The tempo at a position (ticks from bar 1).
    pub fn tempo_at(&self, tick: u32) -> f64 {
        if !self.global.tempo_curves.is_empty() {
            return crate::timeline::Map::of(self).bpm_at(tick as f64);
        }
        self.tempo_map.iter().rev().find(|t| t.0 <= tick).map_or(self.tempo, |t| t.1)
    }
}

/// Ticks in a bar of this signature.
pub fn bar_ticks(sig: (u8, u8)) -> u32 {
    let (n, d) = (sig.0.max(1) as u32, sig.1.max(1) as u32);
    n * events::PPQ * 4 / d
}

/// An AuRg's name: u16 length + ASCII, after the region's flags (found by scanning for the
/// first length-prefixed run of printable text).
fn audio_region_name(payload: &[u8]) -> Option<String> {
    // (S05: at 76)
    for off in 0..payload.len().saturating_sub(4).min(160) {
        let n = u16::from_le_bytes([payload[off], payload[off + 1]]) as usize;
        if (2..=200).contains(&n) {
            if let Some(s) = payload.get(off + 2..off + 2 + n) {
                if s.iter().all(|b| (0x20..0x7f).contains(b)) {
                    return Some(String::from_utf8_lossy(s).into_owned());
                }
            }
        }
    }
    None
}

/// An AuFl's file name: u16 length (in characters) + UTF-16LE, near the start.
pub fn audio_file_name(payload: &[u8]) -> Option<String> {
    for off in 0..payload.len().saturating_sub(4).min(64) {
        let n = u16::from_le_bytes([payload[off], payload[off + 1]]) as usize;
        if !(2..=255).contains(&n) {
            continue;
        }
        let Some(s) = payload.get(off + 2..off + 2 + 2 * n) else { continue };
        let units: Vec<u16> = s.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        if units.iter().all(|&u| (0x20..0xfffe).contains(&u) && u != 0x7f) {
            let name = String::from_utf16_lossy(&units);
            if name.contains('.') {
                return Some(name);
            }
        }
    }
    None
}

/// The audio format an AuFl records after its name (format-analysis, S05/R01): u16 name length
/// + UTF-16 name at 8, then from the name's end +456 the type ("EVAW" = WAVE), +464 the data
/// offset in the file, +468 frames, +476 sample rate, +480 channels, +482 bits.
/// (frames, rate, channels, bits)
pub fn audio_file_format(payload: &[u8]) -> Option<(u32, u32, u16, u16)> {
    let n = u16::from_le_bytes(payload.get(8..10)?.try_into().ok()?) as usize;
    let e = 10 + 2 * n;
    let u32_at = |o: usize| payload.get(e + o..e + o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let u16_at = |o: usize| payload.get(e + o..e + o + 2).map(|b| u16::from_le_bytes(b.try_into().unwrap()));
    Some((u32_at(468)?, u32_at(476)?, u16_at(480)?, u16_at(482)?))
}

/// A WAV or AIFF file's length in seconds (from its header).
pub fn wav_seconds(path: &Path) -> Option<f64> {
    let b = std::fs::read(path).ok()?;
    if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WAVE" {
        let mut p = 12;
        let (mut rate, mut block) = (0u32, 0u16);
        while p + 8 <= b.len() {
            let id = &b[p..p + 4];
            let size = u32::from_le_bytes(b[p + 4..p + 8].try_into().ok()?) as usize;
            if id == b"fmt " && p + 24 <= b.len() {
                rate = u32::from_le_bytes(b[p + 12..p + 16].try_into().ok()?);
                block = u16::from_le_bytes(b[p + 20..p + 22].try_into().ok()?);
            }
            if id == b"data" && rate > 0 && block > 0 {
                return Some(size as f64 / block as f64 / rate as f64);
            }
            p += 8 + size + (size & 1);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GarageBand archive (NSKeyedArchiver: $top -> UID -> {NS.data}) holding ProjectData
    /// bytes gives them back.
    #[test]
    fn band_archive_holds_the_logic_model() {
        let mut data = chunks::MAGIC.to_vec();
        data.extend_from_slice(&[0; 40]);
        // as GarageBand writes it: XML, references as {CF$UID: n}
        let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
        let _ = hex;
        let b64 = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut o = String::new();
            for c in data.chunks(3) {
                let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
                for k in 0..4 {
                    o.push(if k <= c.len() { T[(n >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
                }
            }
            o
        };
        let xml = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>$archiver</key><string>NSKeyedArchiver</string>
<key>$objects</key><array><string>$null</string><dict><key>NS.data</key><data>{b64}</data></dict></array>
<key>$top</key><dict><key>DfDocument logic model</key><dict><key>CF$UID</key><integer>1</integer></dict></dict>
<key>$version</key><integer>100000</integer>
</dict></plist>"#);
        let back = plist::Value::from_reader_xml(xml.as_bytes()).unwrap();
        assert_eq!(band_logic_model(&back), Some(data));
    }
}
