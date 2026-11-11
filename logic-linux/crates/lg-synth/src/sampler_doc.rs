//! A Sampler instrument as the Sampler editor edits it: groups and zones with Logic's
//! parameters (Mapping pane: Key Mapping Editor, Group view, Zone view; Zone pane), and the
//! Mapping pane's commands (Group ▸ New / Duplicate / Create Round Robin / Merge, Zone ▸ New /
//! Load Audio Files / Split / Automap / Remap / Loop / Root Key / Rename / Normalize Loudness /
//! Retune / Assign to New Group, Edit ▸ Select Unused …). `compile` turns it into the zones and
//! group rules the Sampler engine plays.
//!
//! Instruments are saved as SFZ text (open, readable by other samplers): standard opcodes where
//! SFZ has one, and `bs_`-prefixed ones for Logic's parameters SFZ lacks (other players ignore
//! them). `from_sfz_text` reads both, so any SFZ instrument opens in the editor.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::sampler::{self, GroupRule, LoopMode, LoopDir, Pcm, SampleData, Trigger, Zone};

/// Logic's note names: middle C (60) is C3.
pub fn note_name(n: u8) -> String {
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", N[n as usize % 12], n as i32 / 12 - 2)
}

/// A note name ("C3", "F#-1", "Bb2", Logic's octaves) or number.
pub fn parse_note(s: &str) -> Option<u8> {
    let t = s.trim();
    if let Ok(n) = t.parse::<i32>() {
        return (0..=127).contains(&n).then_some(n as u8);
    }
    let b = t.as_bytes();
    let base = match b.first()?.to_ascii_uppercase() {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return None,
    };
    let mut i = 1;
    let mut acc = 0;
    if i < b.len() && b[i] == b'#' {
        acc = 1;
        i += 1;
    } else if i < b.len() && b[i] == b'b' && i + 1 < b.len() {
        acc = -1;
        i += 1;
    }
    let oct: i32 = t[i..].parse().ok()?;
    let n = (oct + 2) * 12 + base + acc;
    (0..=127).contains(&n).then_some(n as u8)
}

/// The root key a file name gives ("Piano_C3_f.wav", "Horn A#2", "bass-45.aif"), Logic octaves.
pub fn root_from_name(name: &str) -> Option<u8> {
    let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| name.to_string());
    let tokens: Vec<&str> = stem.split(|c: char| c == '_' || c == ' ' || c == '-' && false || c == '.' || c == '(' || c == ')' || c == ',').filter(|t| !t.is_empty()).collect();
    // a note name first (its own token, or at a token's end: "PianoC3"), then a bare number
    for t in tokens.iter().rev() {
        for start in 0..t.len() {
            if !t.is_char_boundary(start) {
                continue;
            }
            let tail = &t[start..];
            if tail.len() <= 4
                && tail.as_bytes()[0].is_ascii_alphabetic()
                && (start == 0 || !t.as_bytes()[start - 1].is_ascii_uppercase() || true)
                && let Some(n) = parse_note(tail)
                && tail.chars().any(|c| c.is_ascii_digit())
            {
                return Some(n);
            }
        }
    }
    for t in tokens.iter().rev() {
        let digits: String = t.chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
        if !digits.is_empty() && digits.len() <= 3
            && let Ok(n) = digits.parse::<u8>()
            && n <= 127
            && n >= 12
        {
            return Some(n);
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum XfadeType {
    #[default]
    LinearDb,
    LinearGain,
    EqualPower,
}

impl XfadeType {
    pub const NAMES: &'static [&'static str] = &["Linear dB", "Linear Gain", "Equal Power"];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn from_index(i: usize) -> XfadeType {
        match i {
            1 => XfadeType::LinearGain,
            2 => XfadeType::EqualPower,
            _ => XfadeType::LinearDb,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub name: String,
    pub volume_db: f32,
    pub pan: f32,
    pub output: u8,
    pub key_lo: u8,
    pub key_hi: u8,
    pub key_xfade: u8,
    pub key_xfade_type: XfadeType,
    pub vel_lo: u8,
    pub vel_hi: u8,
    pub vel_xfade: u8,
    pub vel_xfade_type: XfadeType,
    pub mute: bool,
    pub solo: bool,
    /// Round robin: in a cycle (0 = A, 1 = B …).
    pub round_robin: Option<u8>,
    pub articulation: Option<u8>,
    pub on_note: Option<u8>,
    pub on_control: Option<(u8, u8, u8)>,
    pub on_bend: Option<(i8, i8)>,
    pub on_channel: Option<u8>,
    pub on_tempo: Option<(f32, f32)>,
    pub release_trigger: bool,
    pub release_decay: Option<f32>,
    /// Voices (0: Max).
    pub voices: u8,
    /// Exclusive class (0: off).
    pub exclusive: u8,
    /// Envelope 1 offsets: attack, decay, release (s), sustain (fraction).
    pub env1_offset: [f32; 4],
    pub env2_offset: [f32; 4],
    /// Filter offsets: cutoff (octaves), resonance.
    pub cutoff_offset: f32,
    pub res_offset: f32,
    pub sel_random_offset: f32,
}

impl Group {
    pub fn new(name: &str) -> Group {
        Group {
            name: name.to_string(),
            volume_db: 0.0,
            pan: 0.0,
            output: 0,
            key_lo: 0,
            key_hi: 127,
            key_xfade: 0,
            key_xfade_type: XfadeType::LinearDb,
            vel_lo: 1,
            vel_hi: 127,
            vel_xfade: 0,
            vel_xfade_type: XfadeType::LinearDb,
            mute: false,
            solo: false,
            round_robin: None,
            articulation: None,
            on_note: None,
            on_control: None,
            on_bend: None,
            on_channel: None,
            on_tempo: None,
            release_trigger: false,
            release_decay: None,
            voices: 0,
            exclusive: 0,
            env1_offset: [0.0; 4],
            env2_offset: [0.0; 4],
            cutoff_offset: 0.0,
            res_offset: 0.0,
            sel_random_offset: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ZoneDoc {
    pub name: String,
    pub file: Option<PathBuf>,
    pub audio: Option<Arc<SampleData>>,
    /// The file couldn't be found when the instrument loaded.
    pub missing: bool,
    pub group: usize,
    pub root: u8,
    /// Tune (cents; −100 … 100 per semitone, Logic shows semitones and cents).
    pub tune: f32,
    pub pitch: bool,
    pub volume_db: f32,
    pub pan: f32,
    /// Scale: level across the key range (−100 … 100).
    pub scale: f32,
    pub key_lo: u8,
    pub key_hi: u8,
    pub vel_lo: u8,
    pub vel_hi: u8,
    pub mute: bool,
    pub one_shot: bool,
    pub reverse: bool,
    pub output: u8,
    pub start: usize,
    pub end: usize,
    pub fade_in: usize,
    pub fade_out: usize,
    pub loop_on: bool,
    pub loop_dir: LoopDir,
    pub play_to_end: bool,
    pub loop_start: usize,
    pub loop_end: usize,
    pub loop_tune: f32,
    pub xfade: usize,
    pub eq_power: bool,
    pub flex: bool,
    pub follow_tempo: bool,
    pub flex_speed: u8,
    pub anchor: usize,
}

impl ZoneDoc {
    pub fn empty(group: usize) -> ZoneDoc {
        ZoneDoc {
            name: String::new(),
            file: None,
            audio: None,
            missing: false,
            group,
            root: 60,
            tune: 0.0,
            pitch: true,
            volume_db: 0.0,
            pan: 0.0,
            scale: 0.0,
            key_lo: 60,
            key_hi: 60,
            vel_lo: 1,
            vel_hi: 127,
            mute: false,
            one_shot: false,
            reverse: false,
            output: 0,
            start: 0,
            end: 0,
            fade_in: 0,
            fade_out: 0,
            loop_on: false,
            loop_dir: LoopDir::Forward,
            play_to_end: false,
            loop_start: 0,
            loop_end: 0,
            loop_tune: 0.0,
            xfade: 0,
            eq_power: false,
            flex: false,
            follow_tempo: false,
            flex_speed: 0,
            anchor: 0,
        }
    }
    pub fn frames(&self) -> usize {
        self.audio.as_ref().and_then(|a| a.ch.first()).map(pcm_len).unwrap_or(0)
    }
    pub fn rate(&self) -> f32 {
        self.audio.as_ref().map(|a| a.rate).unwrap_or(48000.0)
    }
    /// Puts an audio file into the zone (its loop and root from the header when it has them).
    pub fn set_audio(&mut self, data: Arc<SampleData>, file: Option<PathBuf>, header_loop: Option<(u32, u32)>, header_root: Option<u8>) {
        let n = data.ch.first().map(pcm_len).unwrap_or(0);
        self.audio = Some(data);
        self.start = 0;
        self.end = n;
        self.missing = false;
        if let Some((s, e)) = header_loop {
            self.loop_on = true;
            self.loop_start = (s as usize).min(n);
            self.loop_end = (e as usize).clamp(self.loop_start, n);
        } else {
            self.loop_start = 0;
            self.loop_end = n;
        }
        if let Some(r) = header_root {
            self.root = r;
        }
        if self.name.is_empty() || self.file.is_some() {
            self.name = file.as_ref().and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Zone".into());
        }
        self.file = file;
    }
    pub fn file_name(&self) -> String {
        self.file.as_ref().and_then(|f| f.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

pub(crate) fn pcm_len(p: &Pcm) -> usize {
    match p {
        Pcm::F32(v) => v.len(),
        Pcm::I16(v) => v.len(),
    }
}

fn pcm_at(p: &Pcm, i: usize) -> f32 {
    match p {
        Pcm::F32(v) => v.get(i).copied().unwrap_or(0.0),
        Pcm::I16(v) => v.get(i).map(|&x| x as f32 / 32768.0).unwrap_or(0.0),
    }
}

/// Mono sum of a sample.
fn mono(d: &SampleData) -> Vec<f32> {
    let n = d.ch.first().map(pcm_len).unwrap_or(0);
    (0..n).map(|i| d.ch.iter().map(|c| pcm_at(c, i)).sum::<f32>() / d.ch.len().max(1) as f32).collect()
}

/// How dropped files are mapped (the Navigation bar's dropzones, the Key Mapping Editor).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drop {
    /// Successive keys from C2 (48), original tuning; extra groups when the keys run out.
    Chromatic,
    /// Root notes found by analysis; same roots become velocity layers by loudness; ranges filled.
    Optimized,
    /// One group, one zone per file (on successive keys from C2).
    ZonePerFile,
    /// A group per file, cut at long silences into zones from C2.
    SplitAtSilence,
    /// On one key (several files: a velocity stack).
    Key(u8),
    /// Over a key range (dropped above the keyboard).
    Range(u8, u8),
    /// Into a group: one zone per file at the files' root keys (or C3).
    Group(usize),
}

#[derive(Clone, Debug, Default)]
pub struct SamplerDoc {
    pub name: String,
    pub groups: Vec<Group>,
    pub zones: Vec<ZoneDoc>,
    pub key_labels: std::collections::BTreeMap<u8, String>,
    pub output_labels: std::collections::BTreeMap<u8, String>,
    /// Where it was loaded from or saved to.
    pub path: Option<PathBuf>,
    /// Mapping options (the Zone menu's): Always Move Root Key with Zone, Prefer Splitting Zones
    /// by Velocity, Slide Zones Over.
    pub move_root: bool,
    pub prefer_velocity: bool,
    pub slide_over: bool,
}

impl SamplerDoc {
    pub fn new() -> SamplerDoc {
        SamplerDoc { name: String::new(), groups: vec![Group::new("Group #1")], move_root: true, ..Default::default() }
    }

    /// Sampler always has at least one group.
    pub fn ensure_group(&mut self) {
        if self.groups.is_empty() {
            self.groups.push(Group::new("Group #1"));
        }
    }

    fn next_group_name(&self) -> String {
        let mut k = self.groups.len() + 1;
        while self.groups.iter().any(|g| g.name == format!("Group #{k}")) {
            k += 1;
        }
        format!("Group #{k}")
    }

    /// Group ▸ New: a new empty group (numbered on).
    pub fn new_group(&mut self) -> usize {
        let n = self.next_group_name();
        self.groups.push(Group::new(&n));
        self.groups.len() - 1
    }

    /// Zone ▸ New: an empty zone (no audio file) in the group, or in a new group.
    pub fn new_zone(&mut self, group: Option<usize>) -> usize {
        self.ensure_group();
        let g = match group {
            Some(g) if g < self.groups.len() => g,
            _ => self.new_group(),
        };
        let mut z = ZoneDoc::empty(g);
        z.name = format!("Zone #{}", self.zones.len() + 1);
        self.zones.push(z);
        self.zones.len() - 1
    }

    /// Loads an audio file into a zone (Load or Replace Audio File…).
    pub fn load_into(&mut self, zone: usize, path: &Path) -> std::io::Result<()> {
        let (data, lp, root) = sampler::load_sample(path)?;
        let z = self.zones.get_mut(zone).ok_or_else(|| std::io::Error::other("no zone"))?;
        z.name.clear();
        z.set_audio(data, Some(path.to_path_buf()), lp, root);
        Ok(())
    }

    /// Zone ▸ Audio Files ▸ Detach: the zone stays, empty.
    pub fn detach(&mut self, zone: usize) {
        if let Some(z) = self.zones.get_mut(zone) {
            z.audio = None;
            z.file = None;
            z.start = 0;
            z.end = 0;
        }
    }

    /// Adds files as zones, as the drop targets map them; returns the new zones.
    pub fn add_files(&mut self, paths: &[PathBuf], how: Drop) -> Vec<usize> {
        let mut loaded = Vec::new();
        for p in paths {
            if let Ok((d, lp, root)) = sampler::load_sample(p) {
                loaded.push((p.clone(), d, lp, root));
            }
        }
        self.add_loaded(loaded, how)
    }

    /// Adds audio already in memory (regions, a Quick Sampler's sample).
    pub fn add_loaded(&mut self, loaded: Vec<(PathBuf, Arc<SampleData>, Option<(u32, u32)>, Option<u8>)>, how: Drop) -> Vec<usize> {
        self.ensure_group();
        let mut out = Vec::new();
        if loaded.is_empty() {
            return out;
        }
        let mk = |doc: &mut SamplerDoc, g: usize, (p, d, lp, root): &(PathBuf, Arc<SampleData>, Option<(u32, u32)>, Option<u8>)| {
            let mut z = ZoneDoc::empty(g);
            let file = (!p.as_os_str().is_empty()).then(|| p.clone());
            z.set_audio(d.clone(), file, *lp, *root);
            if z.name.is_empty() {
                z.name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Zone".into());
            }
            doc.zones.push(z);
            doc.zones.len() - 1
        };
        match how {
            Drop::Chromatic | Drop::ZonePerFile => {
                let mut g = self.new_group();
                let mut key = 48u8;
                for f in &loaded {
                    if key > 127 {
                        // more samples than keys: another group, from C2 again
                        g = self.new_group();
                        key = 48;
                    }
                    let zi = mk(self, g, f);
                    let z = &mut self.zones[zi];
                    z.key_lo = key;
                    z.key_hi = key;
                    if how == Drop::Chromatic {
                        // Read Root Key from: the file name, else the key it lands on
                        z.root = root_from_name(&f.0.to_string_lossy()).or(f.3).unwrap_or(key);
                    } else {
                        z.root = key;
                    }
                    if how == Drop::ZonePerFile {
                        z.root = key;
                    }
                    key += 1;
                    out.push(zi);
                }
            }
            Drop::SplitAtSilence => {
                for f in &loaded {
                    let g = self.new_group();
                    self.groups[g].name = f.0.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| self.groups[g].name.clone());
                    let segs = split_at_silence(&f.1);
                    for (k, (s, e)) in segs.into_iter().enumerate() {
                        let zi = mk(self, g, f);
                        let key = (48 + k).min(127) as u8;
                        let z = &mut self.zones[zi];
                        z.start = s;
                        z.end = e;
                        z.loop_on = false;
                        z.key_lo = key;
                        z.key_hi = key;
                        z.root = key;
                        z.name = format!("{} {}", z.name, k + 1);
                        out.push(zi);
                    }
                }
            }
            Drop::Optimized => {
                let g = self.new_group();
                for f in &loaded {
                    let zi = mk(self, g, f);
                    let z = &mut self.zones[zi];
                    let (root, cents) = f.3.map(|r| (r, 0.0)).or_else(|| detect_pitch(&f.1)).unwrap_or((60, 0.0));
                    z.root = root;
                    z.tune = -cents;
                    z.key_lo = root;
                    z.key_hi = root;
                    // crop leading and trailing silence; a loop from the header or found
                    let (s, e) = trim_silence(&f.1);
                    z.start = s;
                    z.end = e;
                    if f.2.is_none()
                        && let Some((ls, le, xf)) = find_loop(&f.1, s, e, 0)
                    {
                        z.loop_on = true;
                        z.loop_start = ls;
                        z.loop_end = le;
                        z.xfade = xf;
                    }
                    // to −12 LUFS (perceived loudness, RMS-based here)
                    z.volume_db = (-12.0 - loudness_db(&f.1, s, e)).clamp(-40.0, 24.0);
                    out.push(zi);
                }
                self.automap_current_root(&out);
            }
            Drop::Key(k) => {
                let g = self.groups.len() - 1;
                let g = if self.zones.iter().any(|z| z.group == g && z.key_lo <= k && z.key_hi >= k) { self.new_group() } else { g };
                let n = loaded.len();
                for (i, f) in loaded.iter().enumerate() {
                    let zi = mk(self, g, f);
                    let z = &mut self.zones[zi];
                    z.key_lo = k;
                    z.key_hi = k;
                    z.root = k;
                    // several files on one key: a velocity stack
                    let lo = 1 + (127 * i / n) as u8;
                    let hi = (127 * (i + 1) / n) as u8;
                    z.vel_lo = lo.min(127);
                    z.vel_hi = hi.max(z.vel_lo);
                    out.push(zi);
                }
            }
            Drop::Range(lo, hi) => {
                let g = self.new_group();
                let w = hi.saturating_sub(lo) as usize + 1;
                let n = loaded.len();
                for (i, f) in loaded.iter().enumerate() {
                    let zi = mk(self, g, f);
                    let z = &mut self.zones[zi];
                    let a = lo as usize + w * i / n;
                    let b = (lo as usize + w * (i + 1) / n).saturating_sub(1).max(a);
                    z.key_lo = a as u8;
                    z.key_hi = b.min(127) as u8;
                    z.root = f.3.unwrap_or(z.key_lo);
                    out.push(zi);
                }
            }
            Drop::Group(g) => {
                let g = g.min(self.groups.len() - 1);
                for f in &loaded {
                    let zi = mk(self, g, f);
                    let z = &mut self.zones[zi];
                    let r = root_from_name(&f.0.to_string_lossy()).or(f.3).unwrap_or(60);
                    z.root = r;
                    z.key_lo = r;
                    z.key_hi = r;
                    out.push(zi);
                }
            }
        }
        for &zi in &out {
            self.resolve_overlaps(&[zi]);
        }
        out
    }

    /// Zones within one group may not overlap in notes and velocities: the others are cut back
    /// (Prefer Splitting Zones by Velocity: by velocity, else by notes). With Slide Zones Over
    /// off, the moved zones give way instead.
    pub fn resolve_overlaps(&mut self, moved: &[usize]) {
        for &m in moved {
            if m >= self.zones.len() {
                continue;
            }
            for o in 0..self.zones.len() {
                if o == m || moved.contains(&o) || self.zones[o].group != self.zones[m].group {
                    continue;
                }
                let (cut, keep) = if self.slide_over || true { (o, m) } else { (m, o) };
                let k = self.zones[keep].clone();
                let c = &mut self.zones[cut];
                let key_ov = c.key_lo <= k.key_hi && c.key_hi >= k.key_lo;
                let vel_ov = c.vel_lo <= k.vel_hi && c.vel_hi >= k.vel_lo;
                if !(key_ov && vel_ov) {
                    continue;
                }
                let by_vel = |c: &mut ZoneDoc| -> bool {
                    if c.vel_lo < k.vel_lo {
                        c.vel_hi = k.vel_lo - 1;
                        true
                    } else if c.vel_hi > k.vel_hi {
                        c.vel_lo = k.vel_hi + 1;
                        true
                    } else {
                        false
                    }
                };
                let by_key = |c: &mut ZoneDoc| -> bool {
                    if c.key_lo < k.key_lo {
                        c.key_hi = k.key_lo - 1;
                        true
                    } else if c.key_hi > k.key_hi {
                        c.key_lo = k.key_hi + 1;
                        true
                    } else {
                        false
                    }
                };
                let done = if self.prefer_velocity { by_vel(c) || by_key(c) } else { by_key(c) || by_vel(c) };
                if !done {
                    // covered completely: it moves to a group of its own (nothing is lost)
                    let g = self.groups.len();
                    let name = self.next_group_name();
                    self.groups.push(Group::new(&name));
                    self.zones[cut].group = g;
                }
            }
        }
    }

    /// Edit ▸ Delete: zones.
    pub fn delete_zones(&mut self, zones: &[usize]) {
        let mut v: Vec<usize> = zones.to_vec();
        v.sort_unstable();
        v.dedup();
        for &z in v.iter().rev() {
            if z < self.zones.len() {
                self.zones.remove(z);
            }
        }
    }

    /// Edit ▸ Delete: groups, with their zones (Sampler keeps one group).
    pub fn delete_groups(&mut self, groups: &[usize]) {
        let mut v: Vec<usize> = groups.to_vec();
        v.sort_unstable();
        v.dedup();
        for &g in v.iter().rev() {
            if g >= self.groups.len() {
                continue;
            }
            self.zones.retain(|z| z.group != g);
            for z in &mut self.zones {
                if z.group > g {
                    z.group -= 1;
                }
            }
            self.groups.remove(g);
        }
        self.ensure_group();
    }

    /// Group ▸ Duplicate: the groups with their zones and every group parameter.
    pub fn duplicate_groups(&mut self, groups: &[usize]) -> Vec<usize> {
        let mut out = Vec::new();
        for &g in groups {
            let Some(src) = self.groups.get(g).cloned() else { continue };
            let mut ng = src;
            ng.name = format!("{} copy", ng.name);
            self.groups.push(ng);
            let ni = self.groups.len() - 1;
            let copies: Vec<ZoneDoc> = self.zones.iter().filter(|z| z.group == g).cloned().map(|mut z| {
                z.group = ni;
                z
            }).collect();
            self.zones.extend(copies);
            out.push(ni);
        }
        out
    }

    /// Group ▸ Merge: the zones of the selected groups go into the first.
    pub fn merge_groups(&mut self, groups: &[usize]) {
        let mut v: Vec<usize> = groups.iter().copied().filter(|&g| g < self.groups.len()).collect();
        v.sort_unstable();
        v.dedup();
        let Some(&first) = v.first() else { return };
        for z in &mut self.zones {
            if v.contains(&z.group) {
                z.group = first;
            }
        }
        let rest: Vec<usize> = v[1..].to_vec();
        for &g in rest.iter().rev() {
            for z in &mut self.zones {
                if z.group > g {
                    z.group -= 1;
                }
            }
            self.groups.remove(g);
        }
    }

    /// Group ▸ Create Round Robin: the groups (top to bottom) cycle on repeated strikes.
    pub fn create_round_robin(&mut self, groups: &[usize]) {
        let cycle = self.groups.iter().filter_map(|g| g.round_robin).max().map_or(0, |c| c + 1);
        for &g in groups {
            if let Some(gr) = self.groups.get_mut(g) {
                gr.round_robin = Some(cycle);
            }
        }
    }

    /// A round robin group's label in the Cycle column ("A.1", "A.2" …).
    pub fn cycle_label(&self, g: usize) -> Option<String> {
        let c = self.groups.get(g)?.round_robin?;
        let place = self.groups[..g].iter().filter(|x| x.round_robin == Some(c)).count() + 1;
        Some(format!("{}.{}", (b'A' + c.min(25)) as char, place))
    }

    /// Zone ▸ Assign to New Group.
    pub fn assign_to_new_group(&mut self, zones: &[usize]) -> usize {
        let g = self.new_group();
        for &z in zones {
            if let Some(z) = self.zones.get_mut(z) {
                z.group = g;
            }
        }
        g
    }

    /// Shift selected zones left / right (with their root keys, or as Always Move Root Key says).
    pub fn shift_zones(&mut self, zones: &[usize], by: i32, with_root: bool) {
        let can = zones.iter().filter_map(|&z| self.zones.get(z)).all(|z| (z.key_lo as i32 + by) >= 0 && (z.key_hi as i32 + by) <= 127);
        if !can {
            return;
        }
        for &zi in zones {
            if let Some(z) = self.zones.get_mut(zi) {
                z.key_lo = (z.key_lo as i32 + by) as u8;
                z.key_hi = (z.key_hi as i32 + by) as u8;
                if with_root {
                    z.root = (z.root as i32 + by).clamp(0, 127) as u8;
                }
            }
        }
        self.resolve_overlaps(zones);
    }

    /// Shift selected groups left / right: their key ranges and every zone in them.
    pub fn shift_groups(&mut self, groups: &[usize], by: i32, with_root: bool) {
        let zs: Vec<usize> = (0..self.zones.len()).filter(|&i| groups.contains(&self.zones[i].group)).collect();
        self.shift_zones(&zs, by, with_root);
        for &g in groups {
            if let Some(gr) = self.groups.get_mut(g) {
                if gr.key_lo == 0 && gr.key_hi == 127 {
                    continue;
                }
                gr.key_lo = (gr.key_lo as i32 + by).clamp(0, 127) as u8;
                gr.key_hi = (gr.key_hi as i32 + by).clamp(0, 127) as u8;
            }
        }
    }

    /// Edit ▸ Select Unused: zones without audio, and groups with no zones, round robin or keyswitch.
    pub fn unused(&self) -> (Vec<usize>, Vec<usize>) {
        let z = (0..self.zones.len()).filter(|&i| self.zones[i].audio.is_none() && self.zones[i].file.is_none()).collect();
        let g = (0..self.groups.len()).filter(|&g| !self.zones.iter().any(|z| z.group == g) && self.groups[g].round_robin.is_none() && self.groups[g].on_note.is_none()).collect();
        (z, g)
    }

    fn sorted_by_key(&self, zones: &[usize]) -> Vec<usize> {
        let mut v: Vec<usize> = zones.iter().copied().filter(|&z| z < self.zones.len()).collect();
        v.sort_by_key(|&z| (self.zones[z].key_lo, self.zones[z].vel_lo));
        v
    }

    /// Remap All Notes: one key each, from the lowest selected zone's key, on free keys.
    pub fn remap_all_notes(&mut self, zones: &[usize]) {
        let v = self.sorted_by_key(zones);
        let Some(&first) = v.first() else { return };
        let mut key = self.zones[first].key_lo as i32;
        for zi in v {
            let g = self.zones[zi].group;
            while key <= 127 && self.zones.iter().enumerate().any(|(i, z)| i != zi && !zones.contains(&i) && z.group == g && z.key_lo as i32 <= key && z.key_hi as i32 >= key) {
                key += 1;
            }
            if key > 127 {
                break;
            }
            let z = &mut self.zones[zi];
            z.key_lo = key as u8;
            z.key_hi = key as u8;
            if self.move_root {
                z.root = key as u8;
            }
            key += 1;
        }
    }

    /// Remap Notes…: from a start note, `width` keys each.
    pub fn remap_notes(&mut self, zones: &[usize], start: u8, width: u8) {
        let v = self.sorted_by_key(zones);
        let w = width.max(1) as i32;
        for (i, zi) in v.into_iter().enumerate() {
            let lo = start as i32 + i as i32 * w;
            if lo > 127 {
                break;
            }
            let z = &mut self.zones[zi];
            z.key_lo = lo as u8;
            z.key_hi = (lo + w - 1).min(127) as u8;
            if self.move_root {
                z.root = z.key_lo;
            }
        }
    }

    /// Remap Velocities…: the zones share the velocity range.
    pub fn remap_velocities(&mut self, zones: &[usize], lo: u8, hi: u8) {
        for &zi in zones {
            if let Some(z) = self.zones.get_mut(zi) {
                z.vel_lo = lo.max(1);
                z.vel_hi = hi.max(z.vel_lo);
            }
        }
    }

    /// Remap White Notes / Remap Black Notes: one key each on white (or black) keys only.
    pub fn remap_colour(&mut self, zones: &[usize], black: bool) {
        let v = self.sorted_by_key(zones);
        let Some(&first) = v.first() else { return };
        let is_black = |k: i32| matches!(k % 12, 1 | 3 | 6 | 8 | 10);
        let mut key = self.zones[first].key_lo as i32;
        for zi in v {
            while key <= 127 && is_black(key) != black {
                key += 1;
            }
            if key > 127 {
                break;
            }
            let z = &mut self.zones[zi];
            z.key_lo = key as u8;
            z.key_hi = key as u8;
            if self.move_root {
                z.root = key as u8;
            }
            key += 1;
        }
    }

    /// Remap to Root Notes: each zone to its root key.
    pub fn remap_to_root(&mut self, zones: &[usize]) {
        for &zi in zones {
            if let Some(z) = self.zones.get_mut(zi) {
                z.key_lo = z.root;
                z.key_hi = z.root;
            }
        }
    }

    /// Remap Evenly: the zones spread across the whole keyboard.
    pub fn remap_evenly(&mut self, zones: &[usize]) {
        let v = self.sorted_by_key(zones);
        let n = v.len();
        if n == 0 {
            return;
        }
        for (i, zi) in v.into_iter().enumerate() {
            let lo = 128 * i / n;
            let hi = (128 * (i + 1) / n).saturating_sub(1).max(lo);
            let z = &mut self.zones[zi];
            z.key_lo = lo as u8;
            z.key_hi = hi.min(127) as u8;
        }
    }

    /// Remap to Round Robin: the zones go into a new round robin of groups, one each, sharing
    /// the first zone's keys.
    pub fn remap_to_round_robin(&mut self, zones: &[usize]) {
        let v = self.sorted_by_key(zones);
        let Some(&first) = v.first() else { return };
        let (lo, hi, root) = (self.zones[first].key_lo, self.zones[first].key_hi, self.zones[first].root);
        let mut gs = Vec::new();
        for zi in v {
            let g = self.new_group();
            gs.push(g);
            let z = &mut self.zones[zi];
            z.group = g;
            z.key_lo = lo;
            z.key_hi = hi;
            z.root = root;
        }
        self.create_round_robin(&gs);
    }

    /// Remap ▸ Swap: two zones trade places.
    pub fn swap(&mut self, a: usize, b: usize) {
        if a >= self.zones.len() || b >= self.zones.len() || a == b {
            return;
        }
        let (za, zb) = (self.zones[a].clone(), self.zones[b].clone());
        let put = |z: &mut ZoneDoc, o: &ZoneDoc| {
            z.key_lo = o.key_lo;
            z.key_hi = o.key_hi;
            z.vel_lo = o.vel_lo;
            z.vel_hi = o.vel_hi;
            z.root = o.root;
        };
        put(&mut self.zones[a], &zb);
        put(&mut self.zones[b], &za);
    }

    /// Pivot on Corner: zones on separate keys stack on the lowest key (velocity layers); a stack
    /// unstacks onto successive keys.
    pub fn pivot(&mut self, zones: &[usize]) {
        let v = self.sorted_by_key(zones);
        let Some(&first) = v.first() else { return };
        let key = self.zones[first].key_lo;
        let stacked = v.iter().all(|&z| self.zones[z].key_lo == key && self.zones[z].key_hi == key);
        let n = v.len();
        if stacked {
            let mut order = v.clone();
            order.sort_by_key(|&z| self.zones[z].vel_lo);
            for (i, zi) in order.into_iter().enumerate() {
                let z = &mut self.zones[zi];
                let k = (key as usize + i).min(127) as u8;
                z.key_lo = k;
                z.key_hi = k;
                z.vel_lo = 1;
                z.vel_hi = 127;
            }
        } else {
            for (i, zi) in v.into_iter().enumerate() {
                let z = &mut self.zones[zi];
                z.key_lo = key;
                z.key_hi = key;
                z.vel_lo = (1 + 127 * i / n).min(127) as u8;
                z.vel_hi = ((127 * (i + 1) / n) as u8).max(z.vel_lo);
            }
        }
    }

    /// Mirror Velocities: the velocity ranges flip top to bottom.
    pub fn mirror_velocities(&mut self, zones: &[usize]) {
        for &zi in zones {
            if let Some(z) = self.zones.get_mut(zi) {
                let (lo, hi) = (128 - z.vel_hi as i32, 128 - z.vel_lo as i32);
                z.vel_lo = lo.clamp(1, 127) as u8;
                z.vel_hi = hi.clamp(1, 127) as u8;
            }
        }
    }

    /// Fill Gaps (notes and / or velocities): ranges grow to meet their neighbours.
    pub fn fill_gaps(&mut self, zones: &[usize], notes: bool, vels: bool) {
        if notes {
            // per group, zones side by side split the gap between them
            let mut v = self.sorted_by_key(zones);
            v.sort_by_key(|&z| (self.zones[z].group, self.zones[z].key_lo));
            for w in 0..v.len() {
                let zi = v[w];
                let g = self.zones[zi].group;
                let prev = (0..w).rev().map(|k| v[k]).find(|&o| self.zones[o].group == g && self.zones[o].key_hi < self.zones[zi].key_lo);
                let lo_bound = prev.map_or(0, |o| self.zones[o].key_hi as i32 + 1);
                let gap = self.zones[zi].key_lo as i32 - lo_bound;
                if gap > 0 {
                    let take = if prev.is_some() { (gap + 1) / 2 } else { gap };
                    if let Some(o) = prev {
                        self.zones[o].key_hi = (self.zones[o].key_hi as i32 + gap - take) as u8;
                    }
                    self.zones[zi].key_lo = (self.zones[zi].key_lo as i32 - take) as u8;
                }
                let has_next = v[w + 1..].iter().any(|&o| self.zones[o].group == g && self.zones[o].key_lo > self.zones[zi].key_hi);
                if !has_next {
                    self.zones[zi].key_hi = 127;
                }
            }
        }
        if vels {
            // zones sharing keys split the velocities between them
            let v = self.sorted_by_key(zones);
            for &zi in &v {
                let mut stack: Vec<usize> = v.iter().copied().filter(|&o| self.zones[o].group == self.zones[zi].group && self.zones[o].key_lo <= self.zones[zi].key_hi && self.zones[o].key_hi >= self.zones[zi].key_lo).collect();
                stack.sort_by_key(|&o| self.zones[o].vel_lo);
                for k in 0..stack.len() {
                    let lo = if k == 0 { 1 } else { self.zones[stack[k - 1]].vel_hi.saturating_add(1) };
                    let hi = if k + 1 == stack.len() { 127 } else { ((self.zones[stack[k]].vel_hi as u16 + self.zones[stack[k + 1]].vel_lo as u16) / 2).max(lo as u16) as u8 };
                    self.zones[stack[k]].vel_lo = lo.min(127);
                    self.zones[stack[k]].vel_hi = hi.max(self.zones[stack[k]].vel_lo);
                }
            }
        }
    }

    /// Automap using Current Root Note: zones spread around their roots without gaps; zones on
    /// the same root become velocity layers by loudness.
    pub fn automap_current_root(&mut self, zones: &[usize]) {
        let mut roots: Vec<u8> = zones.iter().filter_map(|&z| self.zones.get(z)).map(|z| z.root).collect();
        roots.sort_unstable();
        roots.dedup();
        for (i, &r) in roots.iter().enumerate() {
            let lo = if i == 0 { 0 } else { (roots[i - 1] as u16 + r as u16) / 2 + 1 };
            let hi = if i + 1 == roots.len() { 127 } else { (r as u16 + roots[i + 1] as u16) / 2 };
            let mut same: Vec<usize> = zones.iter().copied().filter(|&z| self.zones.get(z).is_some_and(|x| x.root == r)).collect();
            same.sort_by(|&a, &b| {
                let la = self.zones[a].audio.as_ref().map(|d| loudness_db(d, self.zones[a].start, self.zones[a].end)).unwrap_or(-100.0);
                let lb = self.zones[b].audio.as_ref().map(|d| loudness_db(d, self.zones[b].start, self.zones[b].end)).unwrap_or(-100.0);
                la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal)
            });
            let n = same.len();
            for (k, zi) in same.into_iter().enumerate() {
                let z = &mut self.zones[zi];
                z.key_lo = lo as u8;
                z.key_hi = hi as u8;
                z.vel_lo = (1 + 127 * k / n).min(127) as u8;
                z.vel_hi = ((127 * (k + 1) / n) as u8).max(z.vel_lo);
            }
        }
    }

    /// Automap using Root Note from Audio File Names.
    pub fn automap_from_names(&mut self, zones: &[usize]) {
        for &zi in zones {
            if let Some(z) = self.zones.get_mut(zi)
                && let Some(r) = root_from_name(&z.file.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| z.name.clone()))
            {
                z.root = r;
            }
        }
        self.automap_current_root(zones);
    }

    /// Automap using Pitch Detection (also Root Key ▸ from audio analysis, Zone ▸ Retune).
    pub fn retune(&mut self, zones: &[usize]) {
        for &zi in zones {
            if let Some(z) = self.zones.get_mut(zi)
                && let Some(d) = z.audio.clone()
                && let Some((r, c)) = detect_pitch(&d)
            {
                z.root = r;
                z.tune = -c;
            }
        }
    }

    /// Automap Velocities Only: stacks on the same keys share velocities by loudness.
    pub fn automap_velocities(&mut self, zones: &[usize]) {
        let v = self.sorted_by_key(zones);
        let mut done: Vec<usize> = Vec::new();
        for &zi in &v {
            if done.contains(&zi) {
                continue;
            }
            let mut stack: Vec<usize> = v.iter().copied().filter(|&o| self.zones[o].key_lo == self.zones[zi].key_lo && self.zones[o].key_hi == self.zones[zi].key_hi && self.zones[o].group == self.zones[zi].group).collect();
            stack.sort_by(|&a, &b| {
                let la = self.zones[a].audio.as_ref().map(|d| loudness_db(d, 0, usize::MAX)).unwrap_or(-100.0);
                let lb = self.zones[b].audio.as_ref().map(|d| loudness_db(d, 0, usize::MAX)).unwrap_or(-100.0);
                la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal)
            });
            let n = stack.len();
            for (k, &o) in stack.iter().enumerate() {
                self.zones[o].vel_lo = (1 + 127 * k / n).min(127) as u8;
                self.zones[o].vel_hi = ((127 * (k + 1) / n) as u8).max(self.zones[o].vel_lo);
            }
            done.extend(stack);
        }
    }

    /// Zone ▸ Normalize Loudness: each zone to −12 (perceived, RMS-based) loudness.
    pub fn normalize_loudness(&mut self, zones: &[usize]) {
        for &zi in zones {
            if let Some(z) = self.zones.get_mut(zi)
                && let Some(d) = z.audio.clone()
            {
                z.volume_db = (-12.0 - loudness_db(&d, z.start, z.end)).clamp(-60.0, 24.0);
            }
        }
    }

    /// Zone ▸ Loop ▸ Auto-Loop (again: another loop); within the loop area: shorter loops inside.
    pub fn auto_loop(&mut self, zone: usize, attempt: usize, within: bool) -> bool {
        let Some(z) = self.zones.get(zone) else { return false };
        let Some(d) = z.audio.clone() else { return false };
        let (s, e) = if within { (z.loop_start, z.loop_end) } else { (z.start, z.end) };
        let Some((ls, le, xf)) = find_loop(&d, s, e, attempt) else { return false };
        let z = &mut self.zones[zone];
        z.loop_on = true;
        z.loop_start = ls;
        z.loop_end = le;
        z.xfade = xf;
        true
    }

    /// Loop ▸ Set Start to Loop Start / Set End to Loop End / Set Loop from Start to End.
    pub fn loop_command(&mut self, zone: usize, which: &str) {
        let Some(z) = self.zones.get_mut(zone) else { return };
        match which {
            "Set Start to Loop Start" => z.start = z.loop_start,
            "Set End to Loop End" => z.end = z.loop_end,
            "Set Loop from Start to End" => {
                z.loop_start = z.start;
                z.loop_end = z.end;
                z.loop_on = true;
            }
            "Optimize Loop Start" => {
                if let Some(d) = z.audio.clone() {
                    z.loop_start = optimize_loop_start(&d, z.loop_start, z.loop_end);
                }
            }
            "Optimize Loop Crossfade/End" => {
                if let Some(d) = z.audio.clone() {
                    z.xfade = best_crossfade(&d, z.loop_start, z.loop_end);
                }
            }
            _ => {}
        }
    }

    /// Split at Transients / Silence: the zone becomes one zone per part, on successive keys.
    pub fn split_zone(&mut self, zone: usize, silence: bool) -> Vec<usize> {
        let Some(z) = self.zones.get(zone).cloned() else { return Vec::new() };
        let Some(d) = z.audio.clone() else { return Vec::new() };
        let cuts: Vec<(usize, usize)> = if silence {
            split_at_silence(&d).into_iter().filter(|(a, b)| *a >= z.start && *b <= z.end.max(1)).collect()
        } else {
            let s = crate::quick::detect_slices(&d, z.start, z.end, 0.5);
            s.iter().enumerate().map(|(i, &a)| (a, s.get(i + 1).copied().unwrap_or(z.end))).collect()
        };
        if cuts.len() <= 1 {
            return vec![zone];
        }
        let mut out = vec![zone];
        for (k, (a, b)) in cuts.iter().enumerate() {
            let zi = if k == 0 {
                zone
            } else {
                self.zones.push(z.clone());
                self.zones.len() - 1
            };
            let key = (z.key_lo as usize + k).min(127) as u8;
            let n = &mut self.zones[zi];
            n.start = *a;
            n.end = (*b).max(*a + 1);
            n.key_lo = key;
            n.key_hi = key;
            n.root = key;
            n.loop_on = false;
            n.name = format!("{} {}", z.name, k + 1);
            if k > 0 {
                out.push(zi);
            }
        }
        out
    }

    /// Zone ▸ Rename (from the audio file name, optionally with root and velocity).
    pub fn rename_from_files(&mut self, zones: &[usize], add_root_vel: bool) {
        for &zi in zones {
            if let Some(z) = self.zones.get_mut(zi) {
                let base = z.file.as_ref().and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| z.name.clone());
                z.name = if add_root_vel { format!("{base} {} {}-{}", note_name(z.root), z.vel_lo, z.vel_hi) } else { base };
            }
        }
    }

    /// Add Root and Velocity to Existing Name.
    pub fn add_root_vel_to_names(&mut self, zones: &[usize]) {
        for &zi in zones {
            if let Some(z) = self.zones.get_mut(zi) {
                z.name = format!("{} {} {}-{}", z.name, note_name(z.root), z.vel_lo, z.vel_hi);
            }
        }
    }

    /// Crop Sample / Crop Loop: a new file holds only the audio inside the markers; the zone uses
    /// it (the original stays). `dir`: where the new file goes when the zone has none.
    pub fn crop(&mut self, zone: usize, to_loop: bool, dir: &Path) -> std::io::Result<PathBuf> {
        let z = self.zones.get(zone).ok_or_else(|| std::io::Error::other("no zone"))?.clone();
        let d = z.audio.clone().ok_or_else(|| std::io::Error::other("the zone has no audio file"))?;
        let (a, b) = if to_loop { (z.loop_start, z.loop_end) } else { (z.start, z.end) };
        let n = d.ch.first().map(pcm_len).unwrap_or(0);
        let (a, b) = (a.min(n), b.min(n).max(a.min(n) + 1));
        let l: Vec<f32> = (a..b).map(|i| pcm_at(&d.ch[0], i)).collect();
        let r: Vec<f32> = (a..b).map(|i| pcm_at(d.ch.get(1).unwrap_or(&d.ch[0]), i)).collect();
        let stem = z.file.as_ref().and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| z.name.clone());
        let folder = z.file.as_ref().and_then(|f| f.parent()).map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf());
        std::fs::create_dir_all(&folder)?;
        let mut path = folder.join(format!("{stem} cropped.wav"));
        let mut k = 2;
        while path.exists() {
            path = folder.join(format!("{stem} cropped {k}.wav"));
            k += 1;
        }
        crate::wav::write_wav_24(&path, d.rate as u32, &l, &r)?;
        let ch = if d.ch.len() > 1 { vec![Pcm::F32(l), Pcm::F32(r)] } else { vec![Pcm::F32(l)] };
        let nd = Arc::new(SampleData { rate: d.rate, ch });
        let zz = &mut self.zones[zone];
        let shift = a;
        zz.audio = Some(nd);
        zz.file = Some(path.clone());
        zz.start = z.start.saturating_sub(shift).min(b - a);
        zz.end = z.end.saturating_sub(shift).min(b - a);
        zz.loop_start = z.loop_start.saturating_sub(shift).min(b - a);
        zz.loop_end = z.loop_end.saturating_sub(shift).min(b - a);
        if to_loop {
            zz.start = 0;
            zz.end = b - a;
        }
        Ok(path)
    }

    /// The zones and group rules the engine plays.
    pub fn compile(&self) -> (Vec<Zone>, Vec<GroupRule>) {
        let mut zones = Vec::new();
        for z in &self.zones {
            let Some(d) = &z.audio else { continue };
            if z.mute {
                continue;
            }
            let g = self.groups.get(z.group).cloned().unwrap_or_else(|| Group::new(""));
            let (klo, khi) = (z.key_lo.max(g.key_lo), z.key_hi.min(g.key_hi));
            let (vlo, vhi) = (z.vel_lo.max(g.vel_lo), z.vel_hi.min(g.vel_hi));
            if klo > khi || vlo > vhi {
                continue;
            }
            let mut e = Zone::new(d.clone());
            let n = e.end;
            e.lokey = klo;
            e.hikey = khi;
            e.lovel = vlo;
            e.hivel = vhi;
            e.root = z.root;
            e.tune_cents = z.tune;
            e.keytrack = if z.pitch { 100.0 } else { 0.0 };
            e.gain = lg_dsp::db_to_gain(z.volume_db + g.volume_db);
            e.pan = (z.pan + g.pan).clamp(-1.0, 1.0);
            e.scale = z.scale / 100.0;
            e.start = z.start.min(n);
            e.end = if z.end == 0 { n } else { z.end.min(n) };
            e.fade_in = z.fade_in;
            e.fade_out = z.fade_out;
            e.reverse = z.reverse;
            e.loop_mode = if z.one_shot {
                LoopMode::OneShot
            } else if z.loop_on {
                if z.play_to_end { LoopMode::Sustain } else { LoopMode::Continuous }
            } else {
                LoopMode::NoLoop
            };
            e.loop_start = z.loop_start.min(n);
            e.loop_end = z.loop_end.min(n);
            e.loop_dir = z.loop_dir;
            e.loop_xfade = z.xfade;
            e.loop_eq_power = z.eq_power;
            e.loop_tune = z.loop_tune;
            e.synth_env = true;
            e.env_offset = [g.env1_offset[0], g.env1_offset[1], g.env1_offset[2], g.env1_offset[3]];
            e.cutoff_offset = g.cutoff_offset;
            e.res_offset = g.res_offset;
            e.layer = z.group as u16;
            e.trigger = if g.release_trigger { Trigger::Release } else { Trigger::Attack };
            e.key_xfade = (g.key_lo, g.key_hi, g.key_xfade, g.key_xfade_type.index() as u8);
            e.vel_xfade = (g.vel_lo, g.vel_hi, g.vel_xfade, g.vel_xfade_type.index() as u8);
            zones.push(e);
        }
        let rules = (0..self.groups.len()).map(|gi| {
            let g = &self.groups[gi];
            let rr = g.round_robin.map(|c| {
                let members: Vec<usize> = (0..self.groups.len()).filter(|&o| self.groups[o].round_robin == Some(c)).collect();
                let place = members.iter().position(|&o| o == gi).unwrap_or(0);
                (c, place as u8, members.len() as u8)
            });
            GroupRule {
                mute: g.mute,
                solo: g.solo,
                articulation: g.articulation,
                on_note: g.on_note,
                on_control: g.on_control,
                on_bend: g.on_bend,
                on_channel: g.on_channel,
                on_tempo: g.on_tempo,
                round_robin: rr,
                voices: g.voices,
                exclusive: g.exclusive,
                release_decay: if g.release_trigger { g.release_decay } else { None },
            }
        }).collect();
        (zones, rules)
    }

    /// An instrument from engine zones (an SFZ, SoundFont or EXS loaded by the engine's
    /// loaders): one group per engine group number, the zones' own settings carried over.
    pub fn from_zones(zones: &[Zone], name: &str) -> SamplerDoc {
        let mut doc = SamplerDoc { name: name.to_string(), move_root: true, ..Default::default() };
        let mut keys: Vec<i32> = Vec::new();
        for z in zones {
            let k = if z.trigger == Trigger::Release { -1 - z.group } else { z.group };
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
        for (i, &k) in keys.iter().enumerate() {
            let mut g = Group::new(&format!("Group #{}", i + 1));
            g.release_trigger = k < 0;
            doc.groups.push(g);
        }
        doc.ensure_group();
        for (i, z) in zones.iter().enumerate() {
            let k = if z.trigger == Trigger::Release { -1 - z.group } else { z.group };
            let mut d = ZoneDoc::empty(keys.iter().position(|&x| x == k).unwrap_or(0));
            d.name = format!("Zone #{}", i + 1);
            d.audio = Some(z.sample.clone());
            d.root = z.root;
            d.tune = z.tune_cents;
            d.pitch = z.keytrack > 0.0;
            d.volume_db = lg_dsp::gain_to_db(z.gain).max(-96.0);
            d.pan = z.pan;
            d.key_lo = z.lokey;
            d.key_hi = z.hikey;
            d.vel_lo = z.lovel;
            d.vel_hi = z.hivel;
            d.start = z.start;
            d.end = z.end;
            d.one_shot = z.loop_mode == LoopMode::OneShot;
            d.loop_on = matches!(z.loop_mode, LoopMode::Continuous | LoopMode::Sustain);
            d.play_to_end = z.loop_mode == LoopMode::Sustain;
            d.loop_start = z.loop_start;
            d.loop_end = z.loop_end;
            d.reverse = z.reverse;
            doc.zones.push(d);
        }
        doc
    }

    // -----------------------------------------------------------------------------------------
    // SFZ

    /// The instrument as SFZ text; sample paths relative to `base` when they're under it.
    pub fn to_sfz(&self, base: &Path) -> String {
        let mut s = String::from("// Bramble Studio Sampler instrument\n");
        if !self.name.is_empty() {
            s += &format!("// name: {}\n", self.name);
        }
        for (k, l) in &self.key_labels {
            s += &format!("// bs_key_label {k} {l}\n");
        }
        s += "<control>\n";
        for (gi, g) in self.groups.iter().enumerate() {
            s += &format!("\n<group> group_label={}", g.name.replace('\n', " "));
            s += &format!(" volume={:.2} pan={:.1} lokey={} hikey={} lovel={} hivel={}", g.volume_db, g.pan * 100.0, g.key_lo, g.key_hi, g.vel_lo, g.vel_hi);
            if g.key_xfade > 0 {
                s += &format!(" bs_key_xfade={} bs_key_xfade_type={}", g.key_xfade, g.key_xfade_type.index());
            }
            if g.vel_xfade > 0 {
                s += &format!(" bs_vel_xfade={} bs_vel_xfade_type={}", g.vel_xfade, g.vel_xfade_type.index());
            }
            if g.mute {
                s += " bs_mute=1";
            }
            if g.solo {
                s += " bs_solo=1";
            }
            if let Some(c) = g.round_robin {
                let members: Vec<usize> = (0..self.groups.len()).filter(|&o| self.groups[o].round_robin == Some(c)).collect();
                let place = members.iter().position(|&o| o == gi).unwrap_or(0) + 1;
                s += &format!(" seq_length={} seq_position={place} bs_cycle={c}", members.len());
            }
            if let Some(a) = g.articulation {
                s += &format!(" bs_articulation={a}");
            }
            if let Some(n) = g.on_note {
                s += &format!(" sw_last={n}");
            }
            if let Some((cc, lo, hi)) = g.on_control {
                s += &format!(" bs_on_cc={cc} bs_on_cc_lo={lo} bs_on_cc_hi={hi}");
            }
            if let Some((lo, hi)) = g.on_bend {
                s += &format!(" bs_on_bend_lo={lo} bs_on_bend_hi={hi}");
            }
            if let Some(c) = g.on_channel {
                s += &format!(" bs_on_channel={c}");
            }
            if let Some((lo, hi)) = g.on_tempo {
                s += &format!(" bs_on_tempo_lo={lo} bs_on_tempo_hi={hi}");
            }
            if g.release_trigger {
                s += " trigger=release";
                if let Some(t) = g.release_decay {
                    s += &format!(" bs_release_decay={t}");
                }
            }
            if g.voices > 0 {
                s += &format!(" polyphony={}", g.voices);
            }
            if g.exclusive > 0 {
                s += &format!(" group={} off_by={}", g.exclusive, g.exclusive);
            }
            let o = g.env1_offset;
            if o != [0.0; 4] {
                s += &format!(" bs_env1_offset={},{},{},{}", o[0], o[1], o[2], o[3]);
            }
            if g.cutoff_offset != 0.0 || g.res_offset != 0.0 {
                s += &format!(" bs_cutoff_offset={} bs_res_offset={}", g.cutoff_offset, g.res_offset);
            }
            s += "\n";
            for z in self.zones.iter().filter(|z| z.group == gi) {
                let file = z.file.as_ref().map(|f| f.strip_prefix(base).map(Path::to_path_buf).unwrap_or_else(|_| f.clone()));
                s += "<region>";
                if let Some(f) = file {
                    s += &format!(" sample={}", f.to_string_lossy());
                }
                s += &format!(" region_label={} pitch_keycenter={} lokey={} hikey={} lovel={} hivel={}", z.name.replace(['\n', ' '], "_"), z.root, z.key_lo, z.key_hi, z.vel_lo, z.vel_hi);
                if z.tune != 0.0 {
                    s += &format!(" tune={:.1}", z.tune);
                }
                if !z.pitch {
                    s += " pitch_keytrack=0";
                }
                if z.volume_db != 0.0 {
                    s += &format!(" volume={:.2}", z.volume_db);
                }
                if z.pan != 0.0 {
                    s += &format!(" pan={:.1}", z.pan * 100.0);
                }
                if z.scale != 0.0 {
                    s += &format!(" bs_scale={}", z.scale);
                }
                if z.start > 0 {
                    s += &format!(" offset={}", z.start);
                }
                if z.end > 0 && z.end < z.frames() {
                    s += &format!(" end={}", z.end);
                }
                if z.one_shot {
                    s += " loop_mode=one_shot";
                } else if z.loop_on {
                    s += &format!(" loop_mode={} loop_start={} loop_end={}", if z.play_to_end { "loop_sustain" } else { "loop_continuous" }, z.loop_start, z.loop_end);
                    match z.loop_dir {
                        LoopDir::Forward => {}
                        LoopDir::Reverse => s += " bs_loop_dir=reverse",
                        LoopDir::Alternate => s += " bs_loop_dir=alternate",
                    }
                    if z.xfade > 0 {
                        s += &format!(" bs_loop_xfade={}", z.xfade);
                    }
                    if z.eq_power {
                        s += " bs_loop_eq_power=1";
                    }
                    if z.loop_tune != 0.0 {
                        s += &format!(" bs_loop_tune={}", z.loop_tune);
                    }
                } else {
                    s += " loop_mode=no_loop";
                }
                if z.reverse {
                    s += " direction=reverse";
                }
                if z.fade_in > 0 {
                    s += &format!(" bs_fade_in={}", z.fade_in);
                }
                if z.fade_out > 0 {
                    s += &format!(" bs_fade_out={}", z.fade_out);
                }
                if z.mute {
                    s += " bs_mute=1";
                }
                s += "\n";
            }
        }
        s
    }

    /// Reads SFZ text (ours or any): <global>/<master>/<group> opcodes apply to the regions under
    /// them; each <group> header starts a Sampler group. Samples load relative to `base` and the
    /// `default_path`; missing ones leave empty zones marked missing.
    pub fn from_sfz_text(text: &str, base: &Path) -> SamplerDoc {
        let mut doc = SamplerDoc { move_root: true, ..Default::default() };
        let mut default_path = String::new();
        let mut global: Vec<(String, String)> = Vec::new();
        let mut group: Vec<(String, String)> = Vec::new();
        let mut header = String::new();
        let mut region: Option<Vec<(String, String)>> = None;
        let mut cur_group: Option<usize> = None;
        // comments: our name and key labels
        for line in text.lines() {
            let l = line.trim();
            if let Some(n) = l.strip_prefix("// name: ") {
                doc.name = n.to_string();
            }
            if let Some(rest) = l.strip_prefix("// bs_key_label ") {
                let mut it = rest.splitn(2, ' ');
                if let (Some(k), Some(v)) = (it.next().and_then(|k| k.parse().ok()), it.next()) {
                    doc.key_labels.insert(k, v.to_string());
                }
            }
        }
        let clean: String = text.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n");
        let mut tokens: Vec<String> = Vec::new();
        // split into headers and opcode=value pairs (values may contain spaces: up to the next opcode)
        let mut i = 0;
        let b = clean.as_bytes();
        while i < b.len() {
            if b[i] == b'<' {
                let e = clean[i..].find('>').map_or(b.len(), |k| i + k + 1);
                tokens.push(clean[i..e].to_string());
                i = e;
            } else if b[i].is_ascii_whitespace() {
                i += 1;
            } else {
                // an opcode: name=value until the next " name=" or "<" or newline
                let rest = &clean[i..];
                let eq = rest.find('=').unwrap_or(rest.len());
                let name = rest[..eq].trim().to_string();
                let mut j = eq + 1;
                let vb = rest.as_bytes();
                let mut end = rest.len();
                while j < vb.len() {
                    if vb[j] == b'\n' || vb[j] == b'<' {
                        end = j;
                        break;
                    }
                    if vb[j] == b' ' {
                        let after = &rest[j + 1..];
                        let w = after.find(|c: char| c == '=' || c == ' ' || c == '\n' || c == '<');
                        if let Some(k) = w
                            && after.as_bytes()[k] == b'='
                            && k > 0
                        {
                            end = j;
                            break;
                        }
                    }
                    j += 1;
                }
                let value = rest.get(eq + 1..end).unwrap_or("").trim().to_string();
                tokens.push(format!("{name}={value}"));
                i += end.max(1);
            }
        }
        let finish_region = |doc: &mut SamplerDoc, ops: Vec<(String, String)>, gi: usize, default_path: &str| {
            let get = |k: &str| ops.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
            let mut z = ZoneDoc::empty(gi);
            if let Some(smp) = get("sample") {
                let p = base.join(default_path).join(smp.replace('\\', "/"));
                match sampler::load_sample(&p) {
                    Ok((d, lp, _)) => z.set_audio(d, Some(p), lp, None),
                    Err(_) => {
                        z.file = Some(p);
                        z.missing = true;
                    }
                }
            }
            z.name = get("region_label").map(|s| s.to_string()).unwrap_or_else(|| z.file.as_ref().and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| format!("Zone #{}", doc.zones.len() + 1)));
            let note = |k: &str| get(k).and_then(sfz_note_any);
            if let Some(k) = note("key") {
                z.key_lo = k;
                z.key_hi = k;
                z.root = k;
            }
            z.key_lo = note("lokey").unwrap_or(if get("key").is_some() { z.key_lo } else { 0 });
            z.key_hi = note("hikey").unwrap_or(if get("key").is_some() { z.key_hi } else { 127 });
            z.root = note("pitch_keycenter").unwrap_or(z.root);
            let num = |k: &str| get(k).and_then(|v| v.trim().parse::<f32>().ok());
            z.vel_lo = num("lovel").map_or(1, |v| v.clamp(1.0, 127.0) as u8);
            z.vel_hi = num("hivel").map_or(127, |v| v.clamp(1.0, 127.0) as u8);
            z.tune = num("tune").unwrap_or(0.0) + num("transpose").unwrap_or(0.0) * 100.0;
            z.pitch = num("pitch_keytrack").is_none_or(|v| v != 0.0);
            z.volume_db = num("volume").unwrap_or(0.0);
            z.pan = num("pan").unwrap_or(0.0) / 100.0;
            z.scale = num("bs_scale").unwrap_or(0.0);
            if let Some(o) = num("offset") {
                z.start = o as usize;
            }
            if let Some(e) = num("end")
                && e > 0.0
            {
                z.end = e as usize;
            }
            match get("loop_mode") {
                Some("one_shot") => z.one_shot = true,
                Some("loop_continuous") => z.loop_on = true,
                Some("loop_sustain") => {
                    z.loop_on = true;
                    z.play_to_end = true;
                }
                Some("no_loop") => z.loop_on = false,
                _ => {}
            }
            if let Some(v) = num("loop_start").or(num("loopstart")) {
                z.loop_start = v as usize;
            }
            if let Some(v) = num("loop_end").or(num("loopend")) {
                z.loop_end = v as usize;
            }
            z.loop_dir = match get("bs_loop_dir") {
                Some("reverse") => LoopDir::Reverse,
                Some("alternate") => LoopDir::Alternate,
                _ => LoopDir::Forward,
            };
            z.xfade = num("bs_loop_xfade").unwrap_or(0.0) as usize;
            z.eq_power = num("bs_loop_eq_power").is_some_and(|v| v != 0.0);
            z.loop_tune = num("bs_loop_tune").unwrap_or(0.0);
            z.reverse = get("direction") == Some("reverse");
            z.fade_in = num("bs_fade_in").unwrap_or(0.0) as usize;
            z.fade_out = num("bs_fade_out").unwrap_or(0.0) as usize;
            z.mute = get("bs_mute") == Some("1");
            doc.zones.push(z);
        };
        let group_from = |ops: &[(String, String)], name: String| -> Group {
            let get = |k: &str| ops.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
            let num = |k: &str| get(k).and_then(|v| v.trim().parse::<f32>().ok());
            let mut g = Group::new(&get("group_label").map(|s| s.to_string()).unwrap_or(name));
            g.mute = get("bs_mute") == Some("1");
            g.solo = get("bs_solo") == Some("1");
            g.round_robin = num("bs_cycle").map(|c| c as u8).or_else(|| num("seq_length").filter(|&l| l > 1.0).map(|_| 0));
            g.articulation = num("bs_articulation").map(|v| v as u8);
            g.on_note = get("sw_last").and_then(sfz_note_any);
            if let (Some(cc), Some(lo), Some(hi)) = (num("bs_on_cc"), num("bs_on_cc_lo"), num("bs_on_cc_hi")) {
                g.on_control = Some((cc as u8, lo as u8, hi as u8));
            }
            if let (Some(lo), Some(hi)) = (num("bs_on_bend_lo"), num("bs_on_bend_hi")) {
                g.on_bend = Some((lo as i8, hi as i8));
            }
            g.on_channel = num("bs_on_channel").map(|v| v as u8);
            if let (Some(lo), Some(hi)) = (num("bs_on_tempo_lo"), num("bs_on_tempo_hi")) {
                g.on_tempo = Some((lo, hi));
            }
            g.release_trigger = get("trigger") == Some("release");
            g.release_decay = num("bs_release_decay");
            g.voices = num("polyphony").map_or(0, |v| v.clamp(0.0, 255.0) as u8);
            g.exclusive = num("group").map_or(0, |v| v.clamp(0.0, 255.0) as u8);
            g.key_xfade = num("bs_key_xfade").unwrap_or(0.0) as u8;
            g.key_xfade_type = XfadeType::from_index(num("bs_key_xfade_type").unwrap_or(0.0) as usize);
            g.vel_xfade = num("bs_vel_xfade").unwrap_or(0.0) as u8;
            g.vel_xfade_type = XfadeType::from_index(num("bs_vel_xfade_type").unwrap_or(0.0) as usize);
            if let Some(v) = get("bs_env1_offset") {
                let p: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if p.len() == 4 {
                    g.env1_offset = [p[0], p[1], p[2], p[3]];
                }
            }
            g.cutoff_offset = num("bs_cutoff_offset").unwrap_or(0.0);
            g.res_offset = num("bs_res_offset").unwrap_or(0.0);
            g
        };
        let ensure_cur = |doc: &mut SamplerDoc, cur: &mut Option<usize>, ops: &[(String, String)]| -> usize {
            if let Some(c) = *cur {
                return c;
            }
            let name = format!("Group #{}", doc.groups.len() + 1);
            doc.groups.push(group_from(ops, name));
            *cur = Some(doc.groups.len() - 1);
            doc.groups.len() - 1
        };
        for t in tokens {
            if t.starts_with('<') {
                if let Some(ops) = region.take() {
                    let gi = ensure_cur(&mut doc, &mut cur_group, &[global.clone(), group.clone()].concat());
                    finish_region(&mut doc, ops, gi, &default_path);
                }
                header = t.trim_matches(|c| c == '<' || c == '>').to_string();
                match header.as_str() {
                    "region" => region = Some([global.clone(), group.clone()].concat()),
                    "group" => {
                        group.clear();
                        cur_group = None;
                    }
                    "global" | "master" => {
                        if header == "global" {
                            global.clear();
                        }
                        group.clear();
                        cur_group = None;
                    }
                    _ => {}
                }
                continue;
            }
            let Some((k, v)) = t.split_once('=') else { continue };
            let pair = (k.to_string(), v.to_string());
            match header.as_str() {
                "control" => {
                    if k == "default_path" {
                        default_path = v.replace('\\', "/");
                    }
                }
                "global" | "master" => global.push(pair),
                "group" => group.push(pair),
                "region" => {
                    if let Some(r) = region.as_mut() {
                        r.push(pair);
                    }
                }
                _ => {}
            }
        }
        if let Some(ops) = region.take() {
            let gi = ensure_cur(&mut doc, &mut cur_group, &[global.clone(), group.clone()].concat());
            finish_region(&mut doc, ops, gi, &default_path);
        }
        // a group's own key / velocity window is written on the group header
        doc.ensure_group();
        doc
    }

    /// Saves as SFZ; with `copy_audio` every sample goes into `<Samples>/<name>/` first (the
    /// originals stay), in-memory audio is written as WAV there.
    pub fn save(&mut self, path: &Path, copy_audio: Option<&Path>) -> std::io::Result<()> {
        let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        if let Some(dir) = copy_audio {
            std::fs::create_dir_all(dir)?;
            for (i, z) in self.zones.iter_mut().enumerate() {
                let Some(d) = z.audio.clone() else { continue };
                let name = z.file.as_ref().and_then(|f| f.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| format!("{} {}.wav", z.name, i + 1));
                let dest = dir.join(&name);
                match &z.file {
                    Some(f) if f.exists() && f != &dest => {
                        std::fs::copy(f, &dest)?;
                    }
                    Some(f) if f == &dest => {}
                    _ => {
                        let n = d.ch.first().map(pcm_len).unwrap_or(0);
                        let l: Vec<f32> = (0..n).map(|k| pcm_at(&d.ch[0], k)).collect();
                        let r: Vec<f32> = (0..n).map(|k| pcm_at(d.ch.get(1).unwrap_or(&d.ch[0]), k)).collect();
                        let dest = if dest.extension().is_some_and(|e| e == "wav") { dest } else { dest.with_extension("wav") };
                        crate::wav::write_wav_24(&dest, d.rate as u32, &l, &r)?;
                        z.file = Some(dest);
                        continue;
                    }
                }
                z.file = Some(dest);
            }
        } else {
            // samples without a file must be written somewhere: next to the instrument
            let dir = base.join(format!("{} Samples", path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()));
            for (i, z) in self.zones.iter_mut().enumerate() {
                if z.file.is_some() {
                    continue;
                }
                let Some(d) = z.audio.clone() else { continue };
                std::fs::create_dir_all(&dir)?;
                let dest = dir.join(format!("{} {}.wav", z.name.replace('/', "-"), i + 1));
                let n = d.ch.first().map(pcm_len).unwrap_or(0);
                let l: Vec<f32> = (0..n).map(|k| pcm_at(&d.ch[0], k)).collect();
                let r: Vec<f32> = (0..n).map(|k| pcm_at(d.ch.get(1).unwrap_or(&d.ch[0]), k)).collect();
                crate::wav::write_wav_24(&dest, d.rate as u32, &l, &r)?;
                z.file = Some(dest);
            }
        }
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        if self.name.is_empty() {
            self.name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        }
        std::fs::write(path, self.to_sfz(&base))?;
        self.path = Some(path.to_path_buf());
        Ok(())
    }

    pub fn load(path: &Path) -> std::io::Result<SamplerDoc> {
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut doc = match ext.as_str() {
            "sf2" => {
                let s = sampler::Sampler::from_sf2(path, 0, 0, 48000.0)?;
                SamplerDoc::from_zones(s.zones(), &name)
            }
            "exs" => {
                let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
                let s = sampler::Sampler::from_exs(path, &[dir], 48000.0)?;
                SamplerDoc::from_zones(s.zones(), &name)
            }
            _ => {
                let text = std::fs::read_to_string(path)?;
                SamplerDoc::from_sfz_text(&text, path.parent().unwrap_or(Path::new(".")))
            }
        };
        if doc.name.is_empty() {
            doc.name = name;
        }
        doc.path = Some(path.to_path_buf());
        Ok(doc)
    }
}

/// SFZ note values: numbers or names with SFZ's octaves (c4 = 60).
fn sfz_note_any(v: &str) -> Option<u8> {
    let v = v.trim();
    if let Ok(n) = v.parse::<i32>() {
        return Some(n.clamp(0, 127) as u8);
    }
    parse_note(v).map(|n| n.saturating_sub(12))
}

// ---------------------------------------------------------------------------------------------
// Analysis

/// Loudness of a stretch (dB RMS, a stand-in for LUFS).
pub fn loudness_db(d: &SampleData, s: usize, e: usize) -> f32 {
    let n = d.ch.first().map(pcm_len).unwrap_or(0);
    let (s, e) = (s.min(n), e.min(n));
    if e <= s {
        return -100.0;
    }
    let mut acc = 0.0f64;
    let step = ((e - s) / 200_000).max(1);
    let mut cnt = 0usize;
    for i in (s..e).step_by(step) {
        for c in &d.ch {
            let x = pcm_at(c, i) as f64;
            acc += x * x;
        }
        cnt += d.ch.len();
    }
    (10.0 * (acc / cnt.max(1) as f64).max(1e-12).log10()) as f32
}

/// The pitch of a sample: (root key, cents above it), by autocorrelation on its loudest part.
pub fn detect_pitch(d: &SampleData) -> Option<(u8, f32)> {
    let x = mono(d);
    let rate = d.rate;
    let win = ((rate * 0.08) as usize).max(256);
    if x.len() < win * 2 {
        return None;
    }
    let mut best = (0usize, 0.0f32);
    let mut i = 0;
    while i + win < x.len() {
        let e: f32 = x[i..i + win].iter().map(|v| v * v).sum();
        if e > best.1 {
            best = (i, e);
        }
        i += win / 2;
    }
    let seg = &x[best.0..best.0 + win];
    let (lo, hi) = ((rate / 2000.0) as usize, ((rate / 40.0) as usize).min(win / 2));
    // YIN difference function with cumulative normalisation
    let mut dval = vec![0.0f32; hi + 2];
    for lag in 1..=hi + 1 {
        let mut s = 0.0;
        for k in 0..win - lag {
            let q = seg[k] - seg[k + lag];
            s += q * q;
        }
        dval[lag] = s;
    }
    let mut cmnd = vec![1.0f32; hi + 2];
    let mut run = 0.0;
    for lag in 1..=hi + 1 {
        run += dval[lag];
        cmnd[lag] = dval[lag] * lag as f32 / run.max(1e-12);
    }
    let mut lag = 0;
    for l in lo.max(2)..=hi {
        if cmnd[l] < 0.15 && cmnd[l] <= cmnd[l + 1] {
            lag = l;
            break;
        }
    }
    if lag == 0 {
        let (l, v) = (lo.max(2)..=hi).map(|l| (l, cmnd[l])).fold((0, f32::MAX), |a, b| if b.1 < a.1 { b } else { a });
        if v > 0.4 {
            return None;
        }
        lag = l;
    }
    // parabolic refinement
    let (a, b, c) = (cmnd[lag - 1], cmnd[lag], cmnd[lag + 1]);
    let den = a - 2.0 * b + c;
    let off = if den.abs() > 1e-9 { 0.5 * (a - c) / den } else { 0.0 };
    let period = lag as f32 + off.clamp(-1.0, 1.0);
    let hz = rate / period;
    let midi = 69.0 + 12.0 * (hz / 440.0).log2();
    let root = midi.round().clamp(0.0, 127.0);
    Some((root as u8, (midi - root) * 100.0))
}

/// Leading and trailing silence (below −60 dB of the peak) cut off.
pub fn trim_silence(d: &SampleData) -> (usize, usize) {
    let x = mono(d);
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let th = peak * 0.001;
    let s = x.iter().position(|v| v.abs() > th).unwrap_or(0);
    let e = x.iter().rposition(|v| v.abs() > th).map_or(x.len(), |p| (p + 1).min(x.len()));
    (s, e.max(s + 1))
}

/// Non-silent parts separated by long silences (≥ 0.25 s below −50 dB of the peak).
pub fn split_at_silence(d: &SampleData) -> Vec<(usize, usize)> {
    let x = mono(d);
    let rate = d.rate;
    let frame = ((rate * 0.01) as usize).max(32);
    let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
    let th = peak * 0.00316;
    let min_gap = ((0.25 * rate) as usize / frame).max(1);
    let loud: Vec<bool> = x.chunks(frame).map(|c| c.iter().fold(0.0f32, |m, v| m.max(v.abs())) > th).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < loud.len() {
        if !loud[i] {
            i += 1;
            continue;
        }
        let s = i;
        let mut quiet = 0;
        let mut e = i;
        while i < loud.len() {
            if loud[i] {
                quiet = 0;
                e = i;
            } else {
                quiet += 1;
                if quiet >= min_gap {
                    break;
                }
            }
            i += 1;
        }
        out.push((s * frame, ((e + 1) * frame).min(x.len())));
    }
    if out.is_empty() {
        out.push((0, x.len()));
    }
    out
}

/// Similarity (lower is better) of the audio around two points.
fn join_cost(x: &[f32], a: usize, b: usize, w: usize) -> f32 {
    let mut s = 0.0;
    for k in 0..w {
        let (i, j) = (a + k, b + k);
        if i >= x.len() || j >= x.len() || a < w / 2 || b < w / 2 {
            return f32::MAX;
        }
        let q = x[i - w / 2] - x[j - w / 2];
        s += q * q;
    }
    s
}

/// A loop (start, end, crossfade) inside [s, e): the `attempt`-th best of the candidates.
pub fn find_loop(d: &SampleData, s: usize, e: usize, attempt: usize) -> Option<(usize, usize, usize)> {
    let x = mono(d);
    let n = x.len().min(e);
    let s = s.min(n);
    let rate = d.rate as usize;
    if n < s + rate / 5 {
        return None;
    }
    let w = 256.min((n - s) / 8).max(16);
    let min_len = (rate / 10).max(w * 2);
    // candidate ends near the end; starts searched across the middle, at upward zero crossings
    let zc: Vec<usize> = (s + 1..n).filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0).collect();
    if zc.len() < 4 {
        return None;
    }
    let ends: Vec<usize> = zc.iter().copied().filter(|&i| i > s + (n - s) * 3 / 4 && i + w < n).step_by(1.max(zc.len() / 400)).take(40).collect();
    let starts: Vec<usize> = zc.iter().copied().filter(|&i| i > s + (n - s) / 5 && i < s + (n - s) * 3 / 4).step_by(1.max(zc.len() / 400)).collect();
    let mut cands: Vec<(f32, usize, usize)> = Vec::new();
    for &b in &ends {
        for &a in &starts {
            if b < a + min_len {
                continue;
            }
            let c = join_cost(&x, a, b, w);
            if c.is_finite() {
                cands.push((c, a, b));
            }
        }
    }
    cands.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(std::cmp::Ordering::Equal));
    // distinct loops (not the same one shifted a few samples)
    let mut picked: Vec<(usize, usize)> = Vec::new();
    for (_, a, b) in cands {
        if picked.iter().any(|&(pa, pb)| pa.abs_diff(a) < rate / 50 && pb.abs_diff(b) < rate / 50) {
            continue;
        }
        picked.push((a, b));
        if picked.len() > attempt {
            break;
        }
    }
    let (a, b) = *picked.get(attempt.min(picked.len().saturating_sub(1)))?;
    let xf = best_crossfade(d, a, b);
    Some((a, b, xf))
}

/// A crossfade length for a loop join (longer where the join is rough).
pub fn best_crossfade(d: &SampleData, a: usize, b: usize) -> usize {
    let x = mono(d);
    let w = 64;
    let c = join_cost(&x, a, b, w);
    let e: f32 = x.iter().skip(a.saturating_sub(w)).take(w * 2).map(|v| v * v).sum::<f32>().max(1e-9);
    let rough = (c / e).clamp(0.0, 4.0);
    let len = b.saturating_sub(a);
    ((len as f32 * 0.02 * (0.25 + rough)) as usize).min(len / 2)
}

/// The loop start moved (±10 ms) to the point most like the loop end.
pub fn optimize_loop_start(d: &SampleData, a: usize, b: usize) -> usize {
    let x = mono(d);
    let r = (d.rate * 0.01) as usize;
    let w = 128;
    (a.saturating_sub(r)..(a + r).min(b.saturating_sub(w)))
        .min_by(|&p, &q| join_cost(&x, p, b, w).partial_cmp(&join_cost(&x, q, b, w)).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, secs: f32, amp: f32) -> Arc<SampleData> {
        let rate = 48000.0;
        let n = (secs * rate) as usize;
        let v: Vec<f32> = (0..n).map(|i| (std::f32::consts::TAU * hz * i as f32 / rate).sin() * amp).collect();
        Arc::new(SampleData { rate, ch: vec![Pcm::F32(v)] })
    }

    fn loaded(name: &str, d: Arc<SampleData>) -> (PathBuf, Arc<SampleData>, Option<(u32, u32)>, Option<u8>) {
        (PathBuf::from(name), d, None, None)
    }

    #[test]
    fn note_names_are_logic_s() {
        assert_eq!(note_name(60), "C3");
        assert_eq!(parse_note("C3"), Some(60));
        assert_eq!(parse_note("F#-1"), Some(18));
        assert_eq!(parse_note("Bb2"), Some(58));
        assert_eq!(root_from_name("Piano_C3_f.wav"), Some(60));
        assert_eq!(root_from_name("Horn A#2.aif"), Some(58));
        assert_eq!(root_from_name("bass 45.wav"), Some(45));
        assert_eq!(root_from_name("kick.wav"), None);
    }

    #[test]
    fn pitch_detection_finds_root_and_cents() {
        let (r, c) = detect_pitch(&tone(440.0, 0.5, 0.5)).unwrap();
        assert_eq!(r, 69);
        assert!(c.abs() < 5.0, "{c}");
        let (r, c) = detect_pitch(&tone(lg_dsp::midi_hz(52.3), 0.5, 0.5)).unwrap();
        assert_eq!(r, 52);
        assert!((c - 30.0).abs() < 6.0, "{c}");
    }

    #[test]
    fn chromatic_drop_and_groups() {
        let mut d = SamplerDoc::new();
        let files: Vec<_> = (0..3).map(|i| loaded(&format!("s{i}.wav"), tone(220.0, 0.1, 0.3))).collect();
        let z = d.add_loaded(files, Drop::Chromatic);
        assert_eq!(z.len(), 3);
        assert_eq!((d.zones[z[0]].key_lo, d.zones[z[2]].key_hi), (48, 50), "from C2 upward");
        // more files than keys: extra groups from C2
        let many: Vec<_> = (0..90).map(|i| loaded(&format!("m{i}.wav"), tone(220.0, 0.01, 0.3))).collect();
        let g0 = d.groups.len();
        d.add_loaded(many, Drop::Chromatic);
        assert_eq!(d.groups.len(), g0 + 2);
        assert_eq!(d.zones.last().unwrap().key_lo, 48 + 89 - 80);
    }

    #[test]
    fn stack_on_one_key_splits_velocity() {
        let mut d = SamplerDoc::new();
        let z = d.add_loaded(vec![loaded("a.wav", tone(220.0, 0.1, 0.1)), loaded("b.wav", tone(220.0, 0.1, 0.5))], Drop::Key(60));
        let (a, b) = (&d.zones[z[0]], &d.zones[z[1]]);
        assert_eq!((a.vel_lo, a.vel_hi, b.vel_lo, b.vel_hi), (1, 63, 64, 127));
        assert_eq!((a.root, a.key_lo, a.key_hi), (60, 60, 60));
    }

    #[test]
    fn overlaps_are_cut_and_shift_moves_roots() {
        let mut d = SamplerDoc::new();
        let a = d.new_zone(Some(0));
        let b = d.new_zone(Some(0));
        d.zones[a].key_lo = 40;
        d.zones[a].key_hi = 50;
        d.zones[b].key_lo = 45;
        d.zones[b].key_hi = 60;
        d.resolve_overlaps(&[b]);
        assert_eq!((d.zones[a].key_lo, d.zones[a].key_hi), (40, 44));
        d.shift_zones(&[b], 1, true);
        assert_eq!((d.zones[b].key_lo, d.zones[b].key_hi, d.zones[b].root), (46, 61, 61));
        d.shift_zones(&[b], -1, false);
        assert_eq!(d.zones[b].root, 61);
    }

    #[test]
    fn remaps_fill_gaps_and_round_robin() {
        let mut d = SamplerDoc::new();
        let zs: Vec<usize> = (0..4).map(|_| d.new_zone(Some(0))).collect();
        for (i, &z) in zs.iter().enumerate() {
            d.zones[z].key_lo = 40 + i as u8 * 4;
            d.zones[z].key_hi = 40 + i as u8 * 4;
        }
        d.fill_gaps(&zs, true, false);
        assert_eq!(d.zones[zs[0]].key_lo, 0);
        assert_eq!(d.zones[zs[3]].key_hi, 127);
        for w in zs.windows(2) {
            assert_eq!(d.zones[w[0]].key_hi + 1, d.zones[w[1]].key_lo, "no gaps");
        }
        d.remap_evenly(&zs);
        assert_eq!((d.zones[zs[0]].key_lo, d.zones[zs[3]].key_hi), (0, 127));
        d.remap_all_notes(&zs);
        assert_eq!(d.zones[zs[1]].key_lo, d.zones[zs[0]].key_lo + 1);
        d.remap_colour(&zs, false);
        assert!(zs.iter().all(|&z| !matches!(d.zones[z].key_lo % 12, 1 | 3 | 6 | 8 | 10)));
        d.remap_to_round_robin(&zs);
        let labels: Vec<String> = zs.iter().map(|&z| d.cycle_label(d.zones[z].group).unwrap()).collect();
        assert_eq!(labels, ["A.1", "A.2", "A.3", "A.4"]);
        d.pivot(&zs);
        let (_, rules) = d.compile();
        assert_eq!(rules.iter().filter(|r| r.round_robin.is_some()).count(), 4);
    }

    #[test]
    fn sfz_round_trip_keeps_logic_parameters() {
        let dir = std::env::temp_dir().join(format!("lg-sampler-doc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut d = SamplerDoc::new();
        let z = d.add_loaded(vec![loaded("", tone(330.0, 0.3, 0.4))], Drop::Key(64));
        let g = d.zones[z[0]].group;
        d.groups[g].articulation = Some(3);
        d.groups[g].on_note = Some(24);
        d.groups[g].release_trigger = true;
        d.groups[g].release_decay = Some(0.5);
        d.groups[g].vel_xfade = 10;
        d.groups[g].vel_xfade_type = XfadeType::EqualPower;
        d.zones[z[0]].loop_on = true;
        d.zones[z[0]].loop_dir = LoopDir::Alternate;
        d.zones[z[0]].loop_start = 1000;
        d.zones[z[0]].loop_end = 9000;
        d.zones[z[0]].xfade = 200;
        d.zones[z[0]].reverse = true;
        d.zones[z[0]].tune = -12.0;
        d.key_labels.insert(64, "Snare".into());
        let path = dir.join("Test Inst.sfz");
        d.save(&path, None).unwrap();
        let back = SamplerDoc::load(&path).unwrap();
        let bz = back.zones.iter().find(|z| z.key_lo == 64).unwrap();
        let bg = &back.groups[bz.group];
        assert_eq!((bg.articulation, bg.on_note, bg.release_trigger, bg.release_decay), (Some(3), Some(24), true, Some(0.5)));
        assert_eq!((bg.vel_xfade, bg.vel_xfade_type), (10, XfadeType::EqualPower));
        assert_eq!((bz.loop_dir, bz.loop_start, bz.loop_end, bz.xfade, bz.reverse), (LoopDir::Alternate, 1000, 9000, 200, true));
        assert!((bz.tune + 12.0).abs() < 0.01);
        assert!(bz.audio.is_some(), "the in-memory sample was written next to the instrument");
        assert_eq!(back.key_labels.get(&64).map(String::as_str), Some("Snare"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn auto_loop_and_crop() {
        let dir = std::env::temp_dir().join(format!("lg-sampler-crop-{}", std::process::id()));
        let mut d = SamplerDoc::new();
        let z = d.add_loaded(vec![loaded("", tone(200.0, 1.0, 0.4))], Drop::Key(60))[0];
        assert!(d.auto_loop(z, 0, false));
        let zz = &d.zones[z];
        assert!(zz.loop_on && zz.loop_end > zz.loop_start + 4800);
        d.zones[z].start = 4800;
        d.zones[z].end = 24000;
        let p = d.crop(z, false, &dir).unwrap();
        assert!(p.exists());
        assert_eq!((d.zones[z].start, d.zones[z].end, d.zones[z].frames()), (0, 19200, 19200));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn compiled_instrument_plays_layers_and_keyswitches() {
        use lg_dsp::{Instrument, Plugin};
        let mut d = SamplerDoc::new();
        let lo = d.add_loaded(vec![loaded("", tone(261.6, 0.5, 0.2))], Drop::Key(60))[0];
        let hi = d.new_zone(Some(d.zones[lo].group));
        let data = tone(261.6, 0.5, 0.8);
        d.zones[hi].set_audio(data, None, None, None);
        d.zones[hi].key_lo = 60;
        d.zones[hi].key_hi = 60;
        d.zones[lo].vel_hi = 63;
        d.zones[hi].vel_lo = 64;
        let (zones, rules) = d.compile();
        let mut s = sampler::Sampler::new(zones, 48000.0);
        s.set_groups(rules);
        let peak = |s: &mut sampler::Sampler, vel: u8| {
            s.reset();
            s.note_on(60, vel);
            let mut l = vec![0.0f32; 4800];
            let mut r = l.clone();
            s.render(&mut l, &mut r);
            s.note_off(60);
            l.iter().fold(0.0f32, |m, v| m.max(v.abs()))
        };
        let (soft, loud) = (peak(&mut s, 40), peak(&mut s, 120));
        assert!(loud > soft * 2.0, "velocity picks the louder layer: {soft} {loud}");
        // a keyswitch: the second group plays only after its note
        let g2 = d.new_group();
        d.groups[g2].on_note = Some(30);
        let g1 = d.zones[lo].group;
        d.groups[g1].on_note = Some(31);
        let z3 = d.new_zone(Some(g2));
        d.zones[z3].set_audio(tone(523.0, 0.5, 0.5), None, None, None);
        d.zones[z3].key_lo = 60;
        d.zones[z3].key_hi = 60;
        let (zones, rules) = d.compile();
        s.set_zones(zones);
        s.set_groups(rules);
        s.note_on(31, 100);
        s.note_off(31);
        assert!(peak(&mut s, 120) > 0.3);
        s.note_on(30, 100);
        s.note_off(30);
        let p = peak(&mut s, 120);
        assert!(p > 0.1 && p < 0.6, "the switched group plays: {p}");
        assert!(s.params().len() > 250);
    }
}
