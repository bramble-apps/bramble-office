//! The Browser's sounds and search (Logic 13's Browser, as the guide's Browser pages describe
//! it): every item (patches, loops, one-shots) carries tags in categories (genre, instrument
//! with subtags, paired descriptors, group tags with values, sound packs); a search combines
//! criteria (Name Contains, tags, group values, Your Sounds, Favorites, a location, a
//! similarity reference, loops or one-shots) and the active ones show as chips.
//!
//! Content: Logic's patch names (data/patches.txt, played by our stand-ins), the one-shots of
//! Bramble's own sample library (lg-sounds: drum and percussion hits), and the folders the user
//! adds as locations (their loops, one-shots and MIDI files, analysed for tempo, key, bars and
//! instruments as Logic does; files longer than 60 seconds aren't analysed). No Apple loops ship.
//!
//! Favorites, locations, Smart Searches and the Browser's settings persist in
//! ~/.config/bramble-studio/browser.txt.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Patch,
    Loop,
    OneShot,
}

/// A loop's type (its colour in the list: audio blue, MIDI green, Session Player yellow,
/// pattern violet). One-shots are always audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoopType {
    Audio,
    Midi,
    SessionPlayer,
    Pattern,
}

impl LoopType {
    pub fn color(self) -> egui::Color32 {
        match self {
            LoopType::Audio => egui::Color32::from_rgb(0x3d, 0x8f, 0xe0),
            LoopType::Midi => egui::Color32::from_rgb(0x52, 0xc0, 0x3c),
            LoopType::SessionPlayer => egui::Color32::from_rgb(0xe3, 0xb3, 0x2b),
            LoopType::Pattern => egui::Color32::from_rgb(0x9b, 0x6c, 0xe6),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Stable id: "patch:<name>" or the file's path.
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub loop_type: LoopType,
    /// A patch's Logic category ("Keyboard"); a file's main instrument tag.
    pub category: String,
    pub path: Option<PathBuf>,
    /// Tags (display names, "Drums", "Kick", "Dark").
    pub tags: Vec<String>,
    /// Group tags: (group, value), e.g. ("Tonality", "Minor").
    pub groups: Vec<(String, String)>,
    /// The sound pack or location it belongs to.
    pub pack: String,
    /// The user's own content (an added location, saved patches).
    pub user: bool,
    pub bpm: Option<f64>,
    /// Key root ("A", "F#") and minor.
    pub key: Option<String>,
    pub minor: bool,
    pub bars: Option<u32>,
    pub seconds: Option<f64>,
    /// Index of the location it came from.
    pub location: Option<usize>,
}

impl Item {
    pub fn is_file(&self) -> bool {
        self.path.is_some() && self.kind != Kind::Patch
    }
    pub fn key_text(&self) -> String {
        self.key.as_ref().map(|k| format!("{k}{}", if self.minor { "m" } else { "" })).unwrap_or_default()
    }
}

// ---- tags

/// A tag category in All Filters.
pub struct Category {
    pub name: &'static str,
    /// Tags, with their subtags.
    pub tags: &'static [(&'static str, &'static [&'static str])],
}

pub const GENRES: &[(&str, &[&str])] = &[
    ("Ambient", &[]), ("Cinematic", &[]), ("Country", &[]), ("Disco", &[]), ("Electronic", &[]), ("Funk", &[]), ("Hip Hop", &[]), ("House", &[]), ("Jazz", &[]), ("Latin", &[]), ("Lo-Fi", &[]), ("Orchestral", &[]), ("Pop", &[]), ("R&B", &[]), ("Reggae", &[]), ("Rock", &[]), ("Soul", &[]), ("Techno", &[]), ("World", &[]),
];

pub const INSTRUMENTS: &[(&str, &[&str])] = &[
    ("Bass", &["Electric Bass", "Upright Bass", "Synth Bass"]),
    ("Brass", &["Trumpet", "Trombone", "French Horn", "Tuba"]),
    ("Drums", &["Kick", "Snare", "Hi-Hat", "Cymbal", "Tom", "Clap", "Drum Kit"]),
    ("Guitar", &["Acoustic Guitar", "Electric Guitar", "Nylon Guitar"]),
    ("Keyboards", &["Piano", "Electric Piano", "Organ", "Clavinet", "Mellotron"]),
    ("Mallets", &["Vibraphone", "Marimba", "Xylophone", "Glockenspiel", "Bells"]),
    ("Percussion", &["Shaker", "Tambourine", "Conga", "Bongo", "Cajon", "Cowbell"]),
    ("Sound Effects", &[]),
    ("Strings", &["Violin", "Viola", "Cello", "Harp"]),
    ("Synthesizer", &["Lead", "Pad", "Arpeggiated", "Plucked", "Soundscape"]),
    ("Vocals", &[]),
    ("Woodwinds", &["Flute", "Clarinet", "Oboe", "Bassoon", "Saxophone"]),
];

/// Paired descriptors: the two of a pair exclude each other.
pub const PAIRS: &[(&str, &str)] = &[("Acoustic", "Electric"), ("Cheerful", "Dark"), ("Clean", "Distorted"), ("Dry", "Processed"), ("Grooving", "Arrhythmic"), ("Melodic", "Dissonant"), ("Part", "Fill"), ("Relaxed", "Intense"), ("Single", "Ensemble")];

/// Group tags and their values (a group tag is used only with a value).
pub const GROUPS: &[(&str, &[&str])] = &[("Tonality", &["Major", "Minor", "Neither"]), ("Tempo", &["Slow", "Medium", "Fast"]), ("Length", &["1 Bar", "2 Bars", "4 Bars", "8 Bars or more"])];

pub fn categories() -> [Category; 2] {
    [Category { name: "Genre", tags: GENRES }, Category { name: "Instrument", tags: INSTRUMENTS }]
}

/// The main tag a subtag belongs to.
pub fn parent_of(tag: &str) -> Option<&'static str> {
    INSTRUMENTS.iter().find(|(_, subs)| subs.contains(&tag)).map(|(t, _)| *t)
}

/// The other tag of `tag`'s pair.
pub fn pair_of(tag: &str) -> Option<&'static str> {
    PAIRS.iter().find_map(|(a, b)| if *a == tag { Some(*b) } else if *b == tag { Some(*a) } else { None })
}

/// Every plain tag name (genres, instruments and subtags, descriptors).
pub fn all_tags() -> Vec<&'static str> {
    let mut v = Vec::new();
    for c in categories() {
        for (t, subs) in c.tags {
            v.push(*t);
            v.extend(subs.iter().copied());
        }
    }
    for (a, b) in PAIRS {
        v.push(*a);
        v.push(*b);
    }
    v
}

/// Words in a name or folder → tags (lower-case word, tags).
const WORDS: &[(&str, &[&str])] = &[
    ("kick", &["Drums", "Kick"]), ("bd", &["Drums", "Kick"]), ("bassdrum", &["Drums", "Kick"]),
    ("snare", &["Drums", "Snare"]), ("sd", &["Drums", "Snare"]), ("rim", &["Drums", "Snare"]),
    ("hat", &["Drums", "Hi-Hat"]), ("hihat", &["Drums", "Hi-Hat"]), ("hh", &["Drums", "Hi-Hat"]),
    ("crash", &["Drums", "Cymbal"]), ("ride", &["Drums", "Cymbal"]), ("cymbal", &["Drums", "Cymbal"]), ("china", &["Drums", "Cymbal"]), ("splash", &["Drums", "Cymbal"]),
    ("tom", &["Drums", "Tom"]), ("clap", &["Drums", "Clap"]), ("claps", &["Drums", "Clap"]),
    ("drum", &["Drums"]), ("drums", &["Drums", "Drum Kit"]), ("beat", &["Drums", "Drum Kit"]), ("breakbeat", &["Drums", "Drum Kit"]), ("kit", &["Drums", "Drum Kit"]),
    ("perc", &["Percussion"]), ("percussion", &["Percussion"]), ("shaker", &["Percussion", "Shaker"]), ("tambourine", &["Percussion", "Tambourine"]), ("conga", &["Percussion", "Conga"]), ("bongo", &["Percussion", "Bongo"]), ("cajon", &["Percussion", "Cajon", "Acoustic"]), ("cowbell", &["Percussion", "Cowbell"]),
    ("bass", &["Bass"]), ("sub", &["Bass", "Synth Bass"]), ("808", &["Drums", "Electronic"]), ("upright", &["Bass", "Upright Bass", "Acoustic"]),
    ("guitar", &["Guitar"]), ("gtr", &["Guitar"]), ("nylon", &["Guitar", "Nylon Guitar", "Acoustic"]), ("strum", &["Guitar"]),
    ("piano", &["Keyboards", "Piano"]), ("keys", &["Keyboards"]), ("rhodes", &["Keyboards", "Electric Piano", "Electric"]), ("wurli", &["Keyboards", "Electric Piano", "Electric"]), ("ep", &["Keyboards", "Electric Piano", "Electric"]), ("organ", &["Keyboards", "Organ"]), ("clav", &["Keyboards", "Clavinet"]), ("mellotron", &["Keyboards", "Mellotron"]),
    ("synth", &["Synthesizer", "Electric"]), ("lead", &["Synthesizer", "Lead"]), ("pad", &["Synthesizer", "Pad"]), ("arp", &["Synthesizer", "Arpeggiated"]), ("pluck", &["Synthesizer", "Plucked"]), ("soundscape", &["Synthesizer", "Soundscape"]),
    ("strings", &["Strings", "Ensemble"]), ("violin", &["Strings", "Violin"]), ("violins", &["Strings", "Violin", "Ensemble"]), ("viola", &["Strings", "Viola"]), ("violas", &["Strings", "Viola", "Ensemble"]), ("cello", &["Strings", "Cello"]), ("cellos", &["Strings", "Cello", "Ensemble"]), ("harp", &["Strings", "Harp"]),
    ("brass", &["Brass", "Ensemble"]), ("horn", &["Brass", "French Horn"]), ("horns", &["Brass", "Ensemble"]), ("trumpet", &["Brass", "Trumpet"]), ("trombone", &["Brass", "Trombone"]), ("tuba", &["Brass", "Tuba"]),
    ("flute", &["Woodwinds", "Flute"]), ("clarinet", &["Woodwinds", "Clarinet"]), ("oboe", &["Woodwinds", "Oboe"]), ("bassoon", &["Woodwinds", "Bassoon"]), ("sax", &["Woodwinds", "Saxophone"]), ("saxophone", &["Woodwinds", "Saxophone"]), ("woodwind", &["Woodwinds", "Ensemble"]),
    ("vibraphone", &["Mallets", "Vibraphone"]), ("vibes", &["Mallets", "Vibraphone"]), ("marimba", &["Mallets", "Marimba"]), ("xylophone", &["Mallets", "Xylophone"]), ("glockenspiel", &["Mallets", "Glockenspiel"]), ("bells", &["Mallets", "Bells"]), ("kalimba", &["Mallets"]),
    ("vocal", &["Vocals"]), ("vox", &["Vocals"]), ("voice", &["Vocals"]), ("choir", &["Vocals", "Ensemble"]),
    ("fx", &["Sound Effects"]), ("riser", &["Sound Effects"]), ("sweep", &["Sound Effects"]), ("impact", &["Sound Effects"]), ("noise", &["Sound Effects"]),
    ("rock", &["Rock"]), ("pop", &["Pop"]), ("jazz", &["Jazz"]), ("funk", &["Funk"]), ("funky", &["Funk", "Grooving"]), ("soul", &["Soul"]), ("hiphop", &["Hip Hop"]), ("trap", &["Hip Hop", "Electronic"]), ("house", &["House", "Electronic"]), ("techno", &["Techno", "Electronic"]), ("edm", &["Electronic"]), ("electronic", &["Electronic"]), ("ambient", &["Ambient", "Relaxed"]), ("cinematic", &["Cinematic"]), ("orchestral", &["Orchestral"]), ("country", &["Country"]), ("latin", &["Latin"]), ("reggae", &["Reggae"]), ("disco", &["Disco"]), ("lofi", &["Lo-Fi"]), ("world", &["World"]), ("rnb", &["R&B"]),
    ("acoustic", &["Acoustic"]), ("electric", &["Electric"]), ("clean", &["Clean"]), ("dist", &["Distorted"]), ("distorted", &["Distorted"]), ("dirty", &["Distorted"]), ("dry", &["Dry"]), ("processed", &["Processed"]), ("wet", &["Processed"]), ("dark", &["Dark"]), ("happy", &["Cheerful"]), ("bright", &["Cheerful"]), ("chill", &["Relaxed"]), ("soft", &["Relaxed"]), ("hard", &["Intense"]), ("heavy", &["Intense", "Distorted"]), ("groove", &["Grooving"]), ("fill", &["Fill"]), ("solo", &["Single"]), ("ensemble", &["Ensemble"]), ("section", &["Ensemble"]),
];

/// The words of a name: split at non-letters and at lower→upper case changes ("HiHat_01" →
/// hi, hat).
fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && prev_lower && !cur.is_empty() {
                out.push(std::mem::take(&mut cur).to_lowercase());
            }
            prev_lower = c.is_lowercase();
            cur.push(c);
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur).to_lowercase());
            }
            prev_lower = false;
        }
    }
    if !cur.is_empty() {
        out.push(cur.to_lowercase());
    }
    out
}

/// Tags found in `text` (file name, folder names, a patch's category path).
pub fn infer_tags(text: &str) -> Vec<String> {
    let w = words(text);
    let mut out: Vec<String> = Vec::new();
    let push = |t: &str, out: &mut Vec<String>| {
        if !out.iter().any(|x| x == t) {
            // (a descriptor's pair partner already there: the first one found stays)
            if pair_of(t).is_some_and(|p| out.iter().any(|x| x == p)) {
                return;
            }
            out.push(t.to_string());
        }
    };
    // "hi hat", "hip hop", "lo fi", "r n b" as one word
    let joined: Vec<String> = w.windows(2).map(|p| format!("{}{}", p[0], p[1])).collect();
    for word in w.iter().chain(joined.iter()) {
        // ("kick01" → kick)
        let bare = word.trim_end_matches(|c: char| c.is_ascii_digit());
        for (k, tags) in WORDS {
            if *k == word || (*k == bare && !bare.is_empty()) {
                for t in *tags {
                    push(t, &mut out);
                }
            }
        }
    }
    out
}

/// A patch's tags, from Logic's category and subcategory.
fn patch_tags(cat: &str, sub: &str, name: &str) -> Vec<String> {
    let mut t = infer_tags(&format!("{sub} {name}"));
    let main = match cat {
        "Keyboard" => "Keyboards",
        "Bass" => "Bass",
        "Guitar" => "Guitar",
        "Strings" => "Strings",
        "Brass & Woodwind" => if sub.contains("Wood") || name.to_lowercase().contains("flute") || name.to_lowercase().contains("sax") { "Woodwinds" } else { "Brass" },
        "Mallet" => "Mallets",
        "Synthesizer" => "Synthesizer",
        "Drums & Percussion" => if sub.contains("Percussion") { "Percussion" } else { "Drums" },
        _ => "",
    };
    if !main.is_empty() && !t.iter().any(|x| x == main) {
        t.insert(0, main.to_string());
    }
    for (k, tag) in [("Acoustic Piano", "Piano"), ("Electric Piano", "Electric Piano"), ("Organ", "Organ"), ("Clavinet", "Clavinet"), ("Mellotron", "Mellotron"), ("Lead", "Lead"), ("Pad", "Pad"), ("Arpeggiated", "Arpeggiated"), ("Plucked", "Plucked"), ("Soundscape", "Soundscape"), ("Electronic Drums", "Electronic"), ("Acoustic Drums", "Acoustic"), ("Processed", "Processed"), ("Sound Effects", "Sound Effects")] {
        if sub.contains(k) && !t.iter().any(|x| x == tag) {
            t.push(tag.to_string());
        }
    }
    if cat == "Synthesizer" && !t.iter().any(|x| x == "Electric" || x == "Acoustic") {
        t.push("Electric".into());
    }
    t
}

/// Logic's patch library as Browser items.
pub fn patch_items() -> Vec<Item> {
    crate::browser::patches().iter().map(|(cat, sub, name)| Item {
        id: format!("patch:{name}"),
        name: name.clone(),
        kind: Kind::Patch,
        loop_type: LoopType::Midi,
        category: cat.clone(),
        path: None,
        tags: patch_tags(cat, sub, name),
        groups: Vec::new(),
        pack: "Logic Patches".into(),
        user: false,
        bpm: None,
        key: None,
        minor: false,
        bars: None,
        seconds: None,
        location: None,
    }).collect()
}

// ---- file analysis

pub const AUDIO_EXT: &[&str] = &["wav", "aif", "aiff", "caf", "mp3", "flac", "ogg", "m4a"];
pub const MIDI_EXT: &[&str] = &["mid", "midi", "smf"];

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn is_audio(p: &Path) -> bool {
    AUDIO_EXT.contains(&ext_of(p).as_str())
}

pub fn is_midi(p: &Path) -> bool {
    MIDI_EXT.contains(&ext_of(p).as_str())
}

/// A tempo in a file name: "120bpm", "120 BPM", "_96_" (between 60 and 200).
pub fn bpm_in_name(name: &str) -> Option<f64> {
    let lower = name.to_lowercase();
    let b = lower.as_bytes();
    let mut i = 0;
    let mut loose = None;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let s = i;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            let num: f64 = lower[s..i].trim_end_matches('.').parse().unwrap_or(0.0);
            let rest = lower[i..].trim_start_matches([' ', '_', '-']);
            let before_ok = s == 0 || !b[s - 1].is_ascii_alphabetic();
            if rest.starts_with("bpm") && (40.0..=300.0).contains(&num) {
                return Some(num);
            }
            let after_ok = i >= b.len() || !b[i].is_ascii_alphabetic();
            if before_ok && after_ok && (60.0..=200.0).contains(&num) && (i - s) >= 2 && !lower[s..i].contains('.') && loose.is_none() {
                loose = Some(num);
            }
        } else {
            i += 1;
        }
    }
    loose
}

/// A key in a file name: "Am", "C#min", "F_major", "Bb" as its own word. (root, minor)
pub fn key_in_name(name: &str) -> Option<(String, bool)> {
    let tokens: Vec<&str> = name.split(|c: char| !c.is_alphanumeric() && c != '#').filter(|t| !t.is_empty()).collect();
    let mut best = None;
    for (k, t) in tokens.iter().enumerate() {
        let mut ch = t.chars();
        let Some(r) = ch.next().filter(|c| ('A'..='G').contains(c)) else { continue };
        let rest: String = ch.collect();
        let (acc, tail) = if rest.starts_with('#') || rest.starts_with('b') && rest.len() != 0 && !rest.starts_with("bass") {
            (rest[..1].to_string(), rest[1..].to_string())
        } else {
            (String::new(), rest.clone())
        };
        let next = tokens.get(k + 1).map(|s| s.to_lowercase()).unwrap_or_default();
        let tail_l = tail.to_lowercase();
        let minor = match tail_l.as_str() {
            "m" | "min" | "minor" => Some(true),
            "" | "maj" | "major" => Some(next == "minor" || next == "min"),
            _ => None,
        };
        let Some(minor) = minor else { continue };
        // a bare letter counts only beside a key word or as the last token ("Loop_A")
        if tail.is_empty() && acc.is_empty() && !matches!(next.as_str(), "minor" | "min" | "major" | "maj") && k + 1 != tokens.len() {
            continue;
        }
        best = Some((format!("{r}{acc}").replace('b', "♭"), minor));
    }
    best
}

/// An audio file's length in seconds (from its header when it can).
pub fn audio_seconds(p: &Path) -> Option<f64> {
    if let Some(s) = lg_core::project::wav_seconds(p) {
        return Some(s);
    }
    use rodio::Source;
    let f = std::fs::File::open(p).ok()?;
    let d = rodio::Decoder::try_from(f).ok()?;
    d.total_duration().map(|t| t.as_secs_f64())
}

/// The tempo a loop of `seconds` most likely has (whole bars of 4/4 between 70 and 180 bpm,
/// nearest 110), and its bars.
pub fn estimate_tempo(seconds: f64) -> Option<(f64, u32)> {
    if seconds < 1.0 {
        return None;
    }
    let mut best: Option<(f64, u32)> = None;
    for bars in [1u32, 2, 4, 8, 16] {
        let bpm = bars as f64 * 4.0 * 60.0 / seconds;
        if (70.0..=180.0).contains(&bpm) && best.is_none_or(|(b, _)| (bpm - 110.0).abs() < (b - 110.0).abs()) {
            best = Some((bpm, bars));
        }
    }
    // (a tempo off a round number by more than a little isn't a loop)
    best.filter(|(bpm, _)| (bpm - bpm.round()).abs() < 0.08)
}

/// Analyses a file as the Browser does when a location is added: its type (loop or one-shot),
/// tempo, key, bars and instrument tags. Files over 60 seconds keep only their name.
pub fn analyze(path: &Path, pack: &str, location: Option<usize>, folder_words: &str) -> Option<Item> {
    let midi = is_midi(path);
    if !midi && !is_audio(path) {
        return None;
    }
    let stem = path.file_stem()?.to_string_lossy().to_string();
    let name = stem.replace('_', " ");
    let mut it = Item { id: path.display().to_string(), name, kind: Kind::OneShot, loop_type: if midi { LoopType::Midi } else { LoopType::Audio }, category: String::new(), path: Some(path.to_path_buf()), tags: Vec::new(), groups: Vec::new(), pack: pack.to_string(), user: true, bpm: None, key: None, minor: false, bars: None, seconds: None, location };
    if midi {
        it.kind = Kind::Loop;
        if let Some(m) = std::fs::read(path).ok().and_then(|d| lg_core::midi::read(&d)) {
            let end = m.tracks.iter().flat_map(|t| t.2.iter()).map(|n| n.pos + n.len).max().unwrap_or(0);
            let bar = lg_core::events::PPQ * 4;
            it.bars = Some(end.div_ceil(bar).max(1));
            it.bpm = m.tempo.or_else(|| bpm_in_name(&stem));
            if m.tracks.iter().any(|t| t.1 == 9) {
                it.tags.push("Drums".into());
            }
        }
    } else {
        it.seconds = audio_seconds(path);
        let secs = it.seconds.unwrap_or(0.0);
        if secs > 60.0 {
            return Some(it);
        }
        let named = bpm_in_name(&stem);
        match (named, estimate_tempo(secs)) {
            (Some(bpm), _) if secs > 0.4 => {
                it.kind = Kind::Loop;
                it.bpm = Some(bpm);
                it.bars = Some(((secs * bpm / 60.0 / 4.0).round() as u32).max(1));
            }
            (None, Some((bpm, bars))) if secs >= 1.9 => {
                it.kind = Kind::Loop;
                it.bpm = Some(bpm.round());
                it.bars = Some(bars);
            }
            _ => it.kind = Kind::OneShot,
        }
    }
    for t in infer_tags(&format!("{folder_words} {stem}")) {
        if !it.tags.contains(&t) {
            it.tags.push(t);
        }
    }
    if let Some((k, m)) = key_in_name(&stem) {
        it.key = Some(k);
        it.minor = m;
    }
    fill_groups(&mut it);
    it.category = it.tags.iter().find(|t| INSTRUMENTS.iter().any(|(m, _)| m == t)).cloned().unwrap_or_default();
    Some(it)
}

/// The group tags an item's analysis gives (tonality, tempo range, length).
fn fill_groups(it: &mut Item) {
    it.groups.clear();
    if it.kind == Kind::Loop {
        let tone = if it.key.is_none() { "Neither" } else if it.minor { "Minor" } else { "Major" };
        it.groups.push(("Tonality".into(), tone.into()));
        if let Some(b) = it.bpm {
            it.groups.push(("Tempo".into(), if b < 90.0 { "Slow" } else if b <= 130.0 { "Medium" } else { "Fast" }.into()));
        }
        if let Some(n) = it.bars {
            it.groups.push(("Length".into(), match n { 0 | 1 => "1 Bar", 2 | 3 => "2 Bars", 4..=7 => "4 Bars", _ => "8 Bars or more" }.into()));
        }
    }
}

/// Every sound file under `dir` (hidden files and folders skipped), up to `max`.
pub fn scan(dir: &Path, max: usize) -> Vec<PathBuf> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>, depth: u32, max: usize) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        let mut ents: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect();
        ents.sort();
        for p in ents {
            if out.len() >= max {
                return;
            }
            if p.is_dir() {
                if depth > 0 {
                    walk(&p, out, depth - 1, max);
                }
            } else if is_audio(&p) || is_midi(&p) {
                out.push(p);
            }
        }
    }
    let mut v = Vec::new();
    walk(dir, &mut v, 6, max);
    v
}

/// A location's items (analysed).
pub fn location_items(dir: &Path, name: &str, index: usize) -> Vec<Item> {
    scan(dir, 5000).iter().filter_map(|f| {
        let folders = f.parent().and_then(|p| p.strip_prefix(dir).ok()).map(|p| p.display().to_string()).unwrap_or_default();
        analyze(f, name, Some(index), &folders)
    }).collect()
}

/// Bramble's own one-shots: one hit per articulation from the sample library's drum kits and
/// percussion (lg-sounds), when it's installed.
pub fn library_one_shots(root: &Path) -> Vec<Item> {
    let kits = [("drums-modern", "Studio Kit"), ("drums-vintage", "Vintage Kit"), ("drums-brush", "Brush Kit"), ("perc", "Percussion"), ("claps", "Claps"), ("cajon", "Cajon")];
    let mut out = Vec::new();
    for (dir, label) in kits {
        let Ok(rd) = std::fs::read_dir(root.join("samples").join(dir)) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| is_audio(p)).collect();
        files.sort();
        // group by the name without its take / velocity / round-robin suffixes
        let mut groups: Vec<(String, Vec<PathBuf>)> = Vec::new();
        for f in files {
            let stem = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let base = strip_variants(&stem);
            match groups.iter_mut().find(|g| g.0 == base) {
                Some(g) => g.1.push(f),
                None => groups.push((base, vec![f])),
            }
        }
        for (base, fs) in groups {
            // (the loudest-but-one take: a typical hit)
            let pick = fs[fs.len().saturating_sub(2).min(fs.len() - 1)].clone();
            let pretty = pretty_name(&base);
            let mut tags = infer_tags(&format!("{base} {dir}"));
            if !tags.iter().any(|t| t == "Acoustic") {
                tags.push("Acoustic".into());
            }
            tags.push("Single".into());
            let category = tags.iter().find(|t| INSTRUMENTS.iter().any(|(m, _)| m == t)).cloned().unwrap_or_else(|| "Drums".into());
            out.push(Item { id: pick.display().to_string(), name: format!("{label} {pretty}"), kind: Kind::OneShot, loop_type: LoopType::Audio, category, path: Some(pick), tags, groups: Vec::new(), pack: "Bramble Sounds".into(), user: false, bpm: None, key: None, minor: false, bars: None, seconds: None, location: None });
        }
    }
    out
}

fn strip_variants(stem: &str) -> String {
    let mut parts: Vec<&str> = stem.split('_').collect();
    while parts.len() > 1 {
        let l = parts[parts.len() - 1].to_lowercase();
        let variant = l.chars().all(|c| c.is_ascii_digit()) || l.starts_with("rr") || l.starts_with("vl") || l.starts_with('v') && l[1..].chars().all(|c| c.is_ascii_digit()) || ["pp", "p", "mp", "mf", "f", "ff", "fff"].contains(&l.as_str());
        if !variant {
            break;
        }
        parts.pop();
    }
    parts.join("_")
}

fn pretty_name(base: &str) -> String {
    let w: Vec<String> = words(&base.replace("hh", "hi hat")).into_iter().map(|w| {
        let mut c = w.chars();
        let first = c.next().map(|f| f.to_uppercase().collect::<String>()).unwrap_or_default();
        first + c.as_str()
    }).collect();
    let mut s = w.join(" ");
    for (a, b) in [("Hi Hat", "Hi-Hat")] {
        s = s.replace(a, b);
    }
    // ("Crash1" → "Crash 1")
    let mut out = String::new();
    let mut prev_alpha = false;
    for ch in s.chars() {
        if ch.is_ascii_digit() && prev_alpha {
            out.push(' ');
        }
        prev_alpha = ch.is_alphabetic();
        out.push(ch);
    }
    out
}

// ---- search

/// What the Browser shows: the view, then the search's criteria.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Search {
    /// Name Contains terms.
    pub names: Vec<String>,
    pub tags: Vec<String>,
    pub groups: Vec<(String, String)>,
    pub your_sounds: bool,
    pub favorites: bool,
    /// Loops & One-Shots view: only loops (true) or only one-shots (false).
    pub loops_only: Option<bool>,
    /// A sound to find similar ones to (its id and name).
    pub similar: Option<(String, String)>,
    /// A plug-in name or sound pack named in a phrase.
    pub packs: Vec<String>,
    /// A tempo and key from a phrase.
    pub bpm: Option<f64>,
    pub key: Option<(String, bool)>,
    /// A content type from a phrase (loops / one-shots / patches).
    pub kind: Option<Kind>,
}

/// One active criterion, as a chip above the results.
#[derive(Clone, Debug, PartialEq)]
pub enum Chip {
    Name(String),
    Tag(String),
    Group(String, String),
    YourSounds,
    Favorites,
    Similar(String),
    Pack(String),
    Bpm(f64),
    Key(String),
    Kind(Kind),
}

impl Chip {
    pub fn label(&self) -> String {
        match self {
            Chip::Name(s) => format!("Name: {s}"),
            Chip::Tag(t) => t.clone(),
            Chip::Group(g, v) => format!("{g}: {v}"),
            Chip::YourSounds => "Your Sounds".into(),
            Chip::Favorites => "Favorites".into(),
            Chip::Similar(n) => format!("Sounds like {n}"),
            Chip::Pack(p) => p.clone(),
            Chip::Bpm(b) => format!("{b:.0} bpm"),
            Chip::Key(k) => format!("Key: {k}"),
            Chip::Kind(k) => match k {
                Kind::Patch => "Patches",
                Kind::Loop => "Loops",
                Kind::OneShot => "One-Shots",
            }.into(),
        }
    }
}

impl Search {
    pub fn is_empty(&self) -> bool {
        self.chips().is_empty()
    }

    pub fn chips(&self) -> Vec<Chip> {
        let mut v: Vec<Chip> = self.names.iter().cloned().map(Chip::Name).collect();
        v.extend(self.tags.iter().cloned().map(Chip::Tag));
        v.extend(self.groups.iter().cloned().map(|(g, x)| Chip::Group(g, x)));
        v.extend(self.packs.iter().cloned().map(Chip::Pack));
        if let Some(k) = self.kind {
            v.push(Chip::Kind(k));
        }
        if let Some(b) = self.bpm {
            v.push(Chip::Bpm(b));
        }
        if let Some((k, m)) = &self.key {
            v.push(Chip::Key(format!("{k}{}", if *m { " minor" } else { " major" })));
        }
        if self.your_sounds {
            v.push(Chip::YourSounds);
        }
        if self.favorites {
            v.push(Chip::Favorites);
        }
        if let Some((_, n)) = &self.similar {
            v.push(Chip::Similar(n.clone()));
        }
        v
    }

    pub fn remove(&mut self, c: &Chip) {
        match c {
            Chip::Name(s) => self.names.retain(|x| x != s),
            Chip::Tag(t) => self.remove_tag(t),
            Chip::Group(g, x) => self.groups.retain(|y| !(y.0 == *g && y.1 == *x)),
            Chip::YourSounds => self.your_sounds = false,
            Chip::Favorites => self.favorites = false,
            Chip::Similar(_) => self.similar = None,
            Chip::Pack(p) => self.packs.retain(|x| x != p),
            Chip::Bpm(_) => self.bpm = None,
            Chip::Key(_) => self.key = None,
            Chip::Kind(_) => self.kind = None,
        }
    }

    /// Clears every criterion (the cross at the chips' end; Clear Applied Tags). The Loops /
    /// One-Shots buttons stay as they are.
    pub fn clear(&mut self) {
        let l = self.loops_only;
        *self = Search { loops_only: l, ..Default::default() };
    }

    pub fn has_tag(&self, t: &str) -> bool {
        self.tags.iter().any(|x| x == t)
    }

    /// Adds a tag (its pair partner goes; a subtag brings its main tag).
    pub fn add_tag(&mut self, t: &str) {
        if let Some(p) = pair_of(t) {
            self.tags.retain(|x| x != p);
        }
        if let Some(parent) = parent_of(t) {
            if !self.has_tag(parent) {
                self.tags.push(parent.to_string());
            }
        }
        if !self.has_tag(t) {
            self.tags.push(t.to_string());
        }
    }

    /// Removes a tag (and its subtags).
    pub fn remove_tag(&mut self, t: &str) {
        let subs: Vec<&str> = INSTRUMENTS.iter().find(|(m, _)| *m == t).map(|(_, s)| s.to_vec()).unwrap_or_default();
        self.tags.retain(|x| x != t && !subs.contains(&x.as_str()));
    }

    pub fn toggle_tag(&mut self, t: &str) {
        if self.has_tag(t) { self.remove_tag(t) } else { self.add_tag(t) }
    }

    /// Sets a group tag's value (one value per group; the same value again removes it).
    pub fn set_group(&mut self, g: &str, v: &str) {
        let had = self.groups.iter().any(|x| x.0 == g && x.1 == v);
        self.groups.retain(|x| x.0 != g);
        if !had {
            self.groups.push((g.to_string(), v.to_string()));
        }
    }

    /// Does `it` meet every criterion? (`fav`: the favourites' ids.)
    pub fn matches(&self, it: &Item, fav: &HashSet<String>) -> bool {
        let lname = it.name.to_lowercase();
        if !self.names.iter().all(|n| lname.contains(&n.to_lowercase())) {
            return false;
        }
        if !self.tags.iter().all(|t| it.tags.iter().any(|x| x == t) || (t == &it.pack)) {
            return false;
        }
        if !self.groups.iter().all(|g| it.groups.iter().any(|x| x == g)) {
            return false;
        }
        if !self.packs.iter().all(|p| it.pack.eq_ignore_ascii_case(p) || it.category.eq_ignore_ascii_case(p)) {
            return false;
        }
        if self.your_sounds && !it.user {
            return false;
        }
        if self.favorites && !fav.contains(&it.id) {
            return false;
        }
        if let Some(k) = self.kind {
            if it.kind != k {
                return false;
            }
        }
        if let Some(b) = self.bpm {
            if it.bpm.is_none_or(|x| (x - b).abs() > 8.0) {
                return false;
            }
        }
        if let Some((k, m)) = &self.key {
            if it.key.as_ref() != Some(k) || it.minor != *m {
                return false;
            }
        }
        true
    }
}

/// How alike two items are (0..1): shared tags, kind, type, tempo, key.
pub fn similarity(a: &Item, b: &Item) -> f32 {
    let ta: BTreeSet<&str> = a.tags.iter().map(|s| s.as_str()).collect();
    let tb: BTreeSet<&str> = b.tags.iter().map(|s| s.as_str()).collect();
    let inter = ta.intersection(&tb).count() as f32;
    let uni = ta.union(&tb).count().max(1) as f32;
    let mut s = inter / uni * 0.7;
    if a.kind == b.kind {
        s += 0.1;
    }
    if a.category == b.category && !a.category.is_empty() {
        s += 0.1;
    }
    if let (Some(x), Some(y)) = (a.bpm, b.bpm) {
        s += 0.05 * (1.0 - ((x - y).abs() / 40.0).min(1.0)) as f32;
    }
    if a.key.is_some() && a.key == b.key && a.minor == b.minor {
        s += 0.05;
    }
    s
}

/// The most results a "sounds like" or similarity search shows.
pub const SIMILAR_MAX: usize = 1000;

/// Reads a description ("dark synth bass loops in A minor around 120 bpm, my favorites"):
/// content type, instruments and descriptors, sound pack or plug-in names, key, tempo and
/// favorites. Words that are none of these become Name Contains terms only when nothing else
/// was understood.
pub fn parse_phrase(text: &str, packs: &[String]) -> Search {
    let mut s = Search::default();
    let lower = text.to_lowercase();
    let w = words(&lower);
    let mut used = vec![false; w.len()];
    for (i, word) in w.iter().enumerate() {
        match word.as_str() {
            "loop" | "loops" => s.kind = Some(Kind::Loop),
            "oneshot" | "oneshots" | "hit" | "hits" | "sample" | "samples" => s.kind = Some(Kind::OneShot),
            "one" if w.get(i + 1).is_some_and(|n| n.starts_with("shot")) => s.kind = Some(Kind::OneShot),
            "shot" | "shots" if i > 0 && w[i - 1] == "one" => {}
            "patch" | "patches" | "instrument" | "instruments" => s.kind = Some(Kind::Patch),
            "favorite" | "favorites" | "favourite" | "favourites" | "starred" => s.favorites = true,
            "my" | "mine" | "own" => {
                if w.get(i + 1).is_some_and(|n| n == "sounds" || n == "own") {
                    s.your_sounds = true;
                }
            }
            "bpm" => {}
            "in" | "a" | "the" | "and" | "with" | "around" | "at" | "of" | "like" | "some" | "sounds" | "sound" | "that" | "for" => {}
            _ => {
                if let Ok(n) = word.parse::<f64>() {
                    if (40.0..=300.0).contains(&n) {
                        s.bpm = Some(n);
                    }
                } else {
                    let tags = infer_tags(word);
                    if tags.is_empty() {
                        continue;
                    }
                    for t in tags {
                        s.add_tag(&t);
                    }
                }
            }
        }
        used[i] = true;
    }
    // a key: "A minor", "F# major", "in Am"
    if let Some((k, m)) = key_in_name(text) {
        if text.contains(" minor") || text.contains(" major") || text.contains(" in ") || text.trim_end().ends_with('m') {
            s.key = Some((k.replace('b', "♭"), m));
        }
    }
    for p in packs {
        if !p.is_empty() && lower.contains(&p.to_lowercase()) {
            s.packs.push(p.clone());
        }
    }
    if s.is_empty() && !text.trim().is_empty() {
        s.names.push(text.trim().to_string());
    }
    s
}

/// The tags the search field suggests for what's typed: exact matches first, then those that
/// contain it.
pub fn suggest(typed: &str, tags: &[String]) -> Vec<String> {
    let q = typed.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut exact: Vec<String> = tags.iter().filter(|t| t.to_lowercase() == q).cloned().collect();
    let mut starts: Vec<String> = tags.iter().filter(|t| t.to_lowercase() != q && t.to_lowercase().starts_with(&q)).cloned().collect();
    let mut part: Vec<String> = tags.iter().filter(|t| !t.to_lowercase().starts_with(&q) && t.to_lowercase().contains(&q)).cloned().collect();
    starts.sort();
    part.sort();
    exact.append(&mut starts);
    exact.append(&mut part);
    exact.dedup();
    exact.truncate(12);
    exact
}

/// Tags that still have matches among `items` (All Filters shrinks the rest away), with counts.
pub fn tag_counts<'a>(items: impl Iterator<Item = &'a Item>) -> HashMap<String, usize> {
    let mut m: HashMap<String, usize> = HashMap::new();
    for it in items {
        for t in &it.tags {
            *m.entry(t.clone()).or_default() += 1;
        }
        for (g, v) in &it.groups {
            *m.entry(format!("{g}\u{1f}{v}")).or_default() += 1;
        }
        *m.entry(format!("pack\u{1f}{}", it.pack)).or_default() += 1;
    }
    m
}

// ---- saved state: favourites, locations, Smart Searches, settings

#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Saved {
    pub favorites: HashSet<String>,
    pub locations: Vec<Location>,
    pub smart: Vec<(String, Search)>,
    pub settings: HashMap<String, String>,
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A search as one line ("n=…;t=…" fields joined by U+1E).
pub fn search_to_text(s: &Search) -> String {
    let mut f: Vec<String> = Vec::new();
    for n in &s.names {
        f.push(format!("n={n}"));
    }
    for t in &s.tags {
        f.push(format!("t={t}"));
    }
    for (g, v) in &s.groups {
        f.push(format!("g={g}\u{1f}{v}"));
    }
    for p in &s.packs {
        f.push(format!("p={p}"));
    }
    if s.your_sounds {
        f.push("y=1".into());
    }
    if s.favorites {
        f.push("f=1".into());
    }
    if let Some(l) = s.loops_only {
        f.push(format!("l={}", l as u8));
    }
    if let Some(b) = s.bpm {
        f.push(format!("b={b}"));
    }
    if let Some((k, m)) = &s.key {
        f.push(format!("k={k}\u{1f}{}", *m as u8));
    }
    if let Some(k) = s.kind {
        f.push(format!("c={}", match k { Kind::Patch => "p", Kind::Loop => "l", Kind::OneShot => "o" }));
    }
    if let Some((id, n)) = &s.similar {
        f.push(format!("s={id}\u{1f}{n}"));
    }
    f.join("\u{1e}")
}

pub fn search_from_text(t: &str) -> Search {
    let mut s = Search::default();
    for f in t.split('\u{1e}') {
        let Some((k, v)) = f.split_once('=') else { continue };
        let two = || v.split_once('\u{1f}').map(|(a, b)| (a.to_string(), b.to_string()));
        match k {
            "n" => s.names.push(v.into()),
            "t" => s.tags.push(v.into()),
            "g" => s.groups.extend(two()),
            "p" => s.packs.push(v.into()),
            "y" => s.your_sounds = true,
            "f" => s.favorites = true,
            "l" => s.loops_only = Some(v == "1"),
            "b" => s.bpm = v.parse().ok(),
            "k" => s.key = two().map(|(a, b)| (a, b == "1")),
            "c" => s.kind = match v { "p" => Some(Kind::Patch), "l" => Some(Kind::Loop), "o" => Some(Kind::OneShot), _ => None },
            "s" => s.similar = two(),
            _ => {}
        }
    }
    s
}

impl Saved {
    pub fn to_text(&self) -> String {
        let mut out = String::from("# Bramble Studio Browser: favorites, locations, Smart Searches, settings\n");
        let mut fav: Vec<&String> = self.favorites.iter().collect();
        fav.sort();
        for f in fav {
            out += &format!("fav\t{}\n", esc(f));
        }
        for l in &self.locations {
            out += &format!("loc\t{}\t{}\n", esc(&l.name), esc(&l.path.display().to_string()));
        }
        for (n, s) in &self.smart {
            out += &format!("smart\t{}\t{}\n", esc(n), esc(&search_to_text(s)));
        }
        let mut keys: Vec<&String> = self.settings.keys().collect();
        keys.sort();
        for k in keys {
            out += &format!("set\t{}\t{}\n", esc(k), esc(&self.settings[k]));
        }
        out
    }

    pub fn from_text(t: &str) -> Saved {
        let mut s = Saved::default();
        for line in t.lines() {
            let f: Vec<String> = line.split('\t').map(unesc).collect();
            match (f.first().map(|x| x.as_str()), f.len()) {
                (Some("fav"), 2) => {
                    s.favorites.insert(f[1].clone());
                }
                (Some("loc"), 3) => s.locations.push(Location { name: f[1].clone(), path: PathBuf::from(&f[2]) }),
                (Some("smart"), 3) => s.smart.push((f[1].clone(), search_from_text(&f[2]))),
                (Some("set"), 3) => {
                    s.settings.insert(f[1].clone(), f[2].clone());
                }
                _ => {}
            }
        }
        s
    }

    fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("bramble-studio").join("browser.txt"))
    }

    pub fn load() -> Saved {
        if std::env::var_os("LG_SNAP").is_some() && std::env::var_os("LG_BROWSER_STATE").is_none() {
            return Saved::default();
        }
        Saved::path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| Saved::from_text(&t)).unwrap_or_default()
    }

    pub fn save(&self) {
        if std::env::var_os("LG_SNAP").is_some() && std::env::var_os("LG_BROWSER_STATE").is_none() {
            return;
        }
        if let Some(p) = Saved::path() {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, self.to_text());
        }
    }

    pub fn flag(&self, k: &str, default: bool) -> bool {
        self.settings.get(k).map_or(default, |v| v == "1")
    }

    pub fn set_flag(&mut self, k: &str, v: bool) {
        self.settings.insert(k.to_string(), if v { "1" } else { "0" }.into());
    }
}

/// Where Bramble keeps the user's own sounds (saved patches, loops added to the Browser,
/// plug-in presets): ~/.local/share/bramble-studio/.
pub fn user_dir(sub: &str) -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"))).unwrap_or_else(|| PathBuf::from("."));
    base.join("bramble-studio").join(sub)
}

// ---- sampler instruments made from a loop (Create Track with Loop ▸)

/// The instrument name a sampler track made from files carries: "<plug-in>\u{1f}<files>"
/// (Drum Machine Designer: "<note>=<file>" per pad).
pub fn sampler_choice(plugin: &str, files: &[(u8, PathBuf)]) -> String {
    let mut s = plugin.to_string();
    for (n, f) in files {
        s.push('\u{1f}');
        if plugin == "Drum Machine Designer" {
            s += &format!("{n}=");
        }
        s += &f.display().to_string();
    }
    s
}

/// The plug-in part of an instrument choice (what to show).
pub fn plugin_of(choice: &str) -> &str {
    choice.split('\u{1f}').next().unwrap_or(choice)
}

/// A Drum Machine Designer choice's pads (note, file).
pub fn pads_of(choice: &str) -> Vec<(u8, PathBuf)> {
    choice.split('\u{1f}').skip(1).filter_map(|p| p.split_once('=').and_then(|(n, f)| Some((n.parse().ok()?, PathBuf::from(f))))).collect()
}

/// Builds the instrument a sampler choice names (units.rs asks).
pub fn sampler_instrument(choice: &str, rate: f32) -> Option<Box<dyn lg_dsp::Instrument>> {
    let mut parts = choice.split('\u{1f}');
    let plugin = parts.next()?;
    match plugin {
        // (an instrument Auto Sampler made: its SFZ)
        "Sampler" => {
            let f = parts.next()?;
            lg_synth::sampler::Sampler::from_sfz(Path::new(f), rate).ok().map(|q| Box::new(q) as Box<dyn lg_dsp::Instrument>)
        }
        "Quick Sampler" => {
            let f = parts.next()?;
            lg_synth::quick::QuickSampler::from_file(Path::new(f), rate).ok().map(|q| Box::new(q) as Box<dyn lg_dsp::Instrument>)
        }
        "Sample Alchemy" => {
            let f = parts.next()?;
            let a = lg_synth::wav::read_audio(Path::new(f)).ok()?;
            let mono: Vec<f32> = (0..a.frames()).map(|i| a.channels.iter().map(|c| c.get(i).copied().unwrap_or(0.0)).sum::<f32>() / a.channels.len().max(1) as f32).collect();
            let mut al = lg_synth::sample_alchemy::SampleAlchemy::new(rate);
            al.set_sample(mono, a.rate as f32);
            Some(Box::new(al))
        }
        "Drum Machine Designer" => {
            let mut d = lg_synth::dmd::DrumMachineDesigner::new(rate);
            for (n, f) in pads_of(choice) {
                if let Ok(a) = lg_synth::wav::read_audio(&f) {
                    d.set_pad_sample(n, &a);
                }
            }
            Some(Box::new(d))
        }
        _ => None,
    }
}

/// Drum Machine Designer's pads in order (C1 up): the next empty one after `used`.
pub fn next_pad(used: &[(u8, PathBuf)]) -> u8 {
    (36u8..=127).find(|n| !used.iter().any(|u| u.0 == *n)).unwrap_or(36)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, kind: Kind, tags: &[&str]) -> Item {
        Item { id: name.into(), name: name.into(), kind, loop_type: LoopType::Audio, category: String::new(), path: None, tags: tags.iter().map(|s| s.to_string()).collect(), groups: Vec::new(), pack: "P".into(), user: false, bpm: None, key: None, minor: false, bars: None, seconds: None, location: None }
    }

    #[test]
    fn tags_come_from_names() {
        let t = infer_tags("Dirty_Funk_Bass_02");
        assert!(t.contains(&"Distorted".to_string()) && t.contains(&"Funk".to_string()) && t.contains(&"Bass".to_string()), "{t:?}");
        let t = infer_tags("HiHat_closed");
        assert!(t.contains(&"Hi-Hat".to_string()) && t.contains(&"Drums".to_string()), "{t:?}");
        // paired descriptors: the first found stays
        let t = infer_tags("clean dist guitar");
        assert!(t.contains(&"Clean".to_string()) && !t.contains(&"Distorted".to_string()));
    }

    #[test]
    fn tempo_and_key_from_file_names() {
        assert_eq!(bpm_in_name("Groove 120bpm"), Some(120.0));
        assert_eq!(bpm_in_name("loop_96_Am"), Some(96.0));
        assert_eq!(bpm_in_name("kick01"), None);
        assert_eq!(key_in_name("Pad_C#min_90"), Some(("C#".into(), true)));
        assert_eq!(key_in_name("Bass_F_major"), Some(("F".into(), false)));
        assert_eq!(key_in_name("Bass Am"), Some(("A".into(), true)));
        assert_eq!(key_in_name("Drum Beat"), None);
        // whole bars: 8 s is 4 bars at 120
        assert_eq!(estimate_tempo(8.0), Some((120.0, 4)));
        assert_eq!(estimate_tempo(0.5), None);
        assert_eq!(estimate_tempo(7.31), None);
    }

    #[test]
    fn searches_combine_and_pairs_exclude() {
        let fav: HashSet<String> = ["b".to_string()].into();
        let a = item("a dark pad", Kind::Loop, &["Synthesizer", "Pad", "Dark"]);
        let b = item("b", Kind::OneShot, &["Drums", "Kick"]);
        let mut s = Search::default();
        s.add_tag("Kick");
        // a subtag brings its main tag
        assert!(s.has_tag("Drums") && s.has_tag("Kick"));
        assert!(s.matches(&b, &fav) && !s.matches(&a, &fav));
        // removing the main tag takes its subtags
        s.remove_tag("Drums");
        assert!(s.tags.is_empty());
        s.add_tag("Dark");
        s.add_tag("Cheerful");
        assert_eq!(s.tags, vec!["Cheerful".to_string()]);
        let mut s = Search { favorites: true, ..Default::default() };
        assert!(s.matches(&b, &fav) && !s.matches(&a, &fav));
        s.names.push("PAD".into());
        assert!(!s.matches(&b, &fav));
        assert_eq!(s.chips().len(), 2);
        s.remove(&Chip::Favorites);
        assert!(s.matches(&a, &fav));
        s.clear();
        assert!(s.is_empty());
        // group tags: one value each, the same again removes it
        s.set_group("Tonality", "Minor");
        s.set_group("Tonality", "Major");
        assert_eq!(s.groups, vec![("Tonality".to_string(), "Major".to_string())]);
        s.set_group("Tonality", "Major");
        assert!(s.groups.is_empty());
    }

    #[test]
    fn suggestions_exact_first() {
        let tags: Vec<String> = ["Bass", "Synth Bass", "Bassoon", "Upright Bass"].iter().map(|s| s.to_string()).collect();
        let s = suggest("bass", &tags);
        assert_eq!(s[0], "Bass");
        assert_eq!(s[1], "Bassoon");
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn phrases_are_understood() {
        let s = parse_phrase("dark synth bass loops in A minor 120 bpm favorites", &[]);
        assert_eq!(s.kind, Some(Kind::Loop));
        assert!(s.has_tag("Dark") && s.has_tag("Bass") && s.has_tag("Synthesizer"));
        assert_eq!(s.bpm, Some(120.0));
        assert_eq!(s.key, Some(("A".into(), true)));
        assert!(s.favorites);
        let s = parse_phrase("zorblax", &[]);
        assert_eq!(s.names, vec!["zorblax".to_string()]);
    }

    #[test]
    fn saved_state_round_trips() {
        let mut s = Saved::default();
        s.favorites.insert("patch:Deluxe Classic".into());
        s.locations.push(Location { name: "My\tLoops".into(), path: "/tmp/x".into() });
        let mut q = Search::default();
        q.add_tag("Kick");
        q.set_group("Tempo", "Fast");
        q.loops_only = Some(false);
        q.key = Some(("A".into(), true));
        s.smart.push(("Kicks".into(), q.clone()));
        s.set_flag("compact", true);
        let back = Saved::from_text(&s.to_text());
        assert_eq!(back, s);
        assert_eq!(back.smart[0].1, q);
    }

    #[test]
    fn similarity_prefers_shared_tags() {
        let a = item("a", Kind::Loop, &["Drums", "Kick", "Electronic"]);
        let b = item("b", Kind::Loop, &["Drums", "Kick"]);
        let c = item("c", Kind::Loop, &["Strings"]);
        assert!(similarity(&a, &b) > similarity(&a, &c));
    }

    #[test]
    fn library_one_shot_names() {
        assert_eq!(strip_variants("Cajon_hit1_f_rr1"), "Cajon_hit1");
        assert_eq!(strip_variants("hh_closed_14"), "hh_closed");
        assert_eq!(pretty_name("hh_closed"), "Hi-Hat Closed");
        assert_eq!(pretty_name("crash1"), "Crash 1");
    }

    #[test]
    fn patches_have_tags() {
        let p = patch_items();
        let ep = p.iter().find(|i| i.name == "Deluxe Modern").unwrap();
        assert!(ep.tags.contains(&"Keyboards".to_string()) && ep.tags.contains(&"Electric Piano".to_string()), "{:?}", ep.tags);
    }

    #[test]
    fn analysis_of_a_wav_loop_and_hit() {
        let dir = std::env::temp_dir().join(format!("lg-soundlib-{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("Drums"));
        // 4 s of silence at 48 kHz: 2 bars at 120
        let loop_path = dir.join("Drums").join("Funky Beat.wav");
        std::fs::write(&loop_path, crate::audio::wav_bytes(&vec![0.0; 48_000 * 4 * 2])).unwrap();
        let hit = dir.join("Snare_01.wav");
        std::fs::write(&hit, crate::audio::wav_bytes(&vec![0.0; 9600 * 2])).unwrap();
        let items = location_items(&dir, "Mine", 0);
        let l = items.iter().find(|i| i.name == "Funky Beat").unwrap();
        assert_eq!(l.kind, Kind::Loop);
        assert_eq!((l.bpm, l.bars), (Some(120.0), Some(2)));
        assert!(l.tags.contains(&"Drums".to_string()) && l.tags.contains(&"Funk".to_string()) && l.user);
        let h = items.iter().find(|i| i.name == "Snare 01").unwrap();
        assert_eq!(h.kind, Kind::OneShot);
        assert!(h.tags.contains(&"Snare".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sampler_choices() {
        let c = sampler_choice("Drum Machine Designer", &[(36, "/a.wav".into()), (37, "/b.wav".into())]);
        assert_eq!(plugin_of(&c), "Drum Machine Designer");
        assert_eq!(pads_of(&c), vec![(36, PathBuf::from("/a.wav")), (37, PathBuf::from("/b.wav"))]);
        assert_eq!(next_pad(&pads_of(&c)), 38);
    }
}
