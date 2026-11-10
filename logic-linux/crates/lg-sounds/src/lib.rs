//! Bramble Studio's bundled sampled instruments.
//!
//! This crate holds the SFZ mappings (compiled in), the licences (`LICENSES.md`) and the index:
//! which instrument covers which Logic patch-name keywords and Drum Kit Designer kits. The sample
//! audio is not in git: it ships as a release archive (`fetch.sh`) and is looked up at run time in,
//! in order:
//!
//! 1. `$BRAMBLE_SOUNDS` (set it to `off` to force the synthesized instruments),
//! 2. `<exe dir>/sounds`,
//! 3. `~/.local/share/bramble-studio/sounds` (`$XDG_DATA_HOME/bramble-studio/sounds`),
//! 4. `/usr/share/bramble-studio/sounds`.
//!
//! A directory counts when it holds the `VERSION` file the archive carries. SFZ sample paths are
//! relative to that directory (`samples/<id>/<file>.flac`).

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The sample archive this crate's SFZs are written for.
pub const LIBRARY_VERSION: &str = "1";

/// What kind of instrument a sound is (the synthesized engine family it replaces).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Drums,
    Piano,
    Strings,
    StringsPizz,
    Brass,
    Woodwind,
    Saxophone,
    Guitar,
    Bass,
    Mallet,
    Harp,
    Timpani,
}

/// One sampled instrument.
#[derive(Clone, Copy, Debug)]
pub struct Sound {
    /// Stable id: the SFZ file name and the sample folder.
    pub id: &'static str,
    /// Display name.
    pub name: &'static str,
    pub family: Family,
    /// The SFZ mapping (sample paths relative to the library root).
    pub sfz: &'static str,
    /// Words in a Logic patch name that pick this sound within its family (lower case).
    pub keywords: &'static [&'static str],
    /// Drum Kit Designer / Drummer kit names this kit plays (drums only).
    pub kits: &'static [&'static str],
    /// The source library (see LICENSES.md).
    pub source: &'static str,
    /// Voices the Sampler gets for it.
    pub polyphony: u8,
}

macro_rules! sound {
    ($id:literal, $name:literal, $fam:ident, $src:literal, $poly:literal, [$($kw:literal),* $(,)?], [$($kit:literal),* $(,)?]) => {
        Sound {
            id: $id,
            name: $name,
            family: Family::$fam,
            sfz: include_str!(concat!("../sfz/", $id, ".sfz")),
            keywords: &[$($kw),*],
            kits: &[$($kit),*],
            source: $src,
            polyphony: $poly,
        }
    };
}

/// The index. Within a family the first entry is the default.
pub static SOUNDS: &[Sound] = &[
    // drums: the Drummer's kits by character
    sound!("drums-modern", "Studio Kit (sampled)", Drums, "DRSKit 2.1", 64,
        ["rock", "pop", "studio", "punk", "indie", "garage", "funk", "heavy", "metal", "arena", "stadium"],
        ["SoCal", "Brooklyn", "Heavy", "East Bay", "Smash", "Retro Rock", "Manchester", "Portland",
         "Four on the Floor", "Detroit Garage", "Sunset", "Scientific Method", "Studio",
         "Drum Kit Designer", "Drum Kits", "Drum Kit"]),
    sound!("drums-vintage", "Vintage Kit (sampled)", Drums, "Virtuosity Drums 0.925", 64,
        ["vintage", "jazz", "bebop", "swing", "motown", "liverpool", "60s", "70s", "roots", "soul",
         "songwriter", "retro", "classic", "neo soul", "slow jam", "coffee"],
        ["Bluebird", "Liverpool", "Roots", "Motown Revisited", "Neo Soul", "Slow Jam", "Coffee Shop",
         "Latin"]),
    sound!("drums-brush", "Brush Kit (sampled)", Drums, "DRSKit 2.1 (whiskers)", 64,
        ["brush", "brushes", "speakeasy", "blue ridge"],
        ["Speakeasy", "Blue Ridge"]),
    // keys
    sound!("piano-grand", "Grand Piano (sampled)", Piano, "Salamander Grand Piano V3", 48,
        ["grand", "piano", "steinway", "concert", "studio", "bright", "felt", "soft", "upright"], []),
    // strings
    sound!("strings-section", "String Section (sampled)", Strings, "VSCO 2 CE", 48,
        ["string", "strings", "orchestra", "ensemble", "chamber", "sustain"], []),
    sound!("violins", "Violins (sampled)", Strings, "VSCO 2 CE", 32, ["violins", "violin section"], []),
    sound!("violas", "Violas (sampled)", Strings, "VSCO 2 CE", 32, ["viola"], []),
    sound!("cellos", "Cellos (sampled)", Strings, "VSCO 2 CE", 32, ["cello"], []),
    sound!("basses", "Double Basses (sampled)", Strings, "VSCO 2 CE", 32, ["double bass", "contrabass", "basses"], []),
    sound!("violin-solo", "Solo Violin (sampled)", Strings, "VSCO 2 CE", 16, ["solo violin", "fiddle", "violin solo"], []),
    sound!("strings-pizz", "Pizzicato Strings (sampled)", StringsPizz, "VSCO 2 CE", 48, ["pizz", "pizzicato"], []),
    sound!("harp", "Harp (sampled)", Harp, "VSCO 2 CE", 32, ["harp"], []),
    // brass
    sound!("brass-section", "Brass Section (sampled)", Brass, "VSCO 2 CE", 48,
        ["brass", "horns", "section", "ensemble", "fanfare"], []),
    sound!("trumpet", "Trumpet (sampled)", Brass, "VSCO 2 CE", 16, ["trumpet", "flugel", "cornet"], []),
    sound!("french-horn", "French Horn (sampled)", Brass, "VSCO 2 CE", 16, ["french horn", "horn solo"], []),
    sound!("trombone", "Trombone (sampled)", Brass, "VSCO 2 CE", 16, ["trombone"], []),
    sound!("tuba", "Tuba (sampled)", Brass, "VSCO 2 CE", 16, ["tuba"], []),
    // woodwinds
    sound!("woodwind-section", "Woodwind Section (sampled)", Woodwind, "VSCO 2 CE", 48,
        ["woodwind", "winds", "ensemble", "section"], []),
    sound!("flute", "Flute (sampled)", Woodwind, "VSCO 2 CE", 16, ["flute", "piccolo", "whistle", "recorder"], []),
    sound!("oboe", "Oboe (sampled)", Woodwind, "VSCO 2 CE", 16, ["oboe", "english horn"], []),
    sound!("clarinet", "Clarinet (sampled)", Woodwind, "VSCO 2 CE", 16, ["clarinet"], []),
    sound!("bassoon", "Bassoon (sampled)", Woodwind, "VSCO 2 CE", 16, ["bassoon"], []),
    sound!("tenor-sax", "Tenor Saxophone (sampled)", Saxophone, "VCSL", 16, ["sax", "saxophone", "tenor"], []),
    // guitars and basses
    sound!("guitar-steel", "Steel-String Acoustic Guitar (sampled)", Guitar, "FreePats FSS Steel-String Guitar", 24,
        ["acoustic", "steel", "folk", "strum", "songwriter"], []),
    sound!("guitar-nylon", "Nylon Guitar (sampled)", Guitar, "FreePats Spanish Classical Guitar", 24,
        ["nylon", "classical", "spanish", "flamenco", "bossa"], []),
    sound!("guitar-electric", "Clean Electric Guitar (sampled)", Guitar, "Karoryfer Emilyguitar", 24,
        ["electric", "clean", "jazz guitar", "muted", "rock", "funk"], []),
    sound!("bass-electric", "Electric Bass (sampled)", Bass, "Karoryfer Fashionbass", 16,
        ["electric", "finger", "pick", "slap", "fretless", "rock", "funk"], []),
    sound!("bass-upright", "Upright Bass (sampled)", Bass, "Karoryfer Meatbass", 16,
        ["upright", "acoustic bass", "double bass", "jazz", "monstera"], []),
    // mallets and pitched percussion
    sound!("vibraphone", "Vibraphone (sampled)", Mallet, "VCSL", 32, ["vibe", "vibra"], []),
    sound!("marimba", "Marimba (sampled)", Mallet, "VCSL", 32, ["marimba"], []),
    sound!("xylophone", "Xylophone (sampled)", Mallet, "VCSL", 32, ["xylo"], []),
    sound!("glockenspiel", "Glockenspiel (sampled)", Mallet, "VCSL", 32, ["glock", "crotales", "bells"], []),
    sound!("tubular-bells", "Tubular Bells (sampled)", Mallet, "VCSL", 32, ["tubular", "chime"], []),
    sound!("kalimba", "Kalimba (sampled)", Mallet, "VCSL", 32, ["kalimba", "mbira", "thumb piano"], []),
    sound!("timpani", "Timpani (sampled)", Timpani, "VSCO 2 CE", 16, ["timpani"], []),
];

/// Every sampled instrument.
pub fn index() -> &'static [Sound] {
    SOUNDS
}

/// The sound with this id.
pub fn find(id: &str) -> Option<&'static Sound> {
    SOUNDS.iter().find(|s| s.id == id)
}

/// The kit that plays a Drum Kit Designer / Drummer kit name ("SoCal", "Bluebird"…), by its
/// exact name, then by words in it (brush → brush kit).
pub fn for_kit(kit_name: &str) -> Option<&'static Sound> {
    let n = kit_name.trim();
    if let Some(s) = SOUNDS
        .iter()
        .find(|s| s.kits.iter().any(|k| k.eq_ignore_ascii_case(n)))
    {
        return Some(s);
    }
    if n.to_lowercase().contains("brush") {
        return find("drums-brush");
    }
    None
}

/// The sound of a family whose keywords best match a (lower-case) patch name; the family's
/// first sound when none match.
pub fn pick(family: Family, low_name: &str) -> Option<&'static Sound> {
    let mut best: Option<(&'static Sound, usize)> = None;
    for s in SOUNDS.iter().filter(|s| s.family == family) {
        // the longest matching keyword wins (so "solo violin" beats "violin")
        let score = s
            .keywords
            .iter()
            .filter(|k| low_name.contains(*k))
            .map(|k| k.len())
            .max()
            .unwrap_or(0);
        match best {
            Some((_, b)) if score <= b => {}
            _ => best = Some((s, score)),
        }
    }
    best.map(|(s, _)| s)
}

fn valid(dir: &Path) -> bool {
    dir.join("VERSION").is_file()
}

fn search() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("BRAMBLE_SOUNDS") {
        let s = v.to_string_lossy();
        if s.is_empty() || s.eq_ignore_ascii_case("off") || s == "0" {
            return None;
        }
        let p = PathBuf::from(&v);
        if valid(&p) {
            return Some(p);
        }
    }
    let mut dirs = Vec::new();
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("sounds")))
    {
        dirs.push(d);
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(d) = data {
        dirs.push(d.join("bramble-studio/sounds"));
    }
    dirs.push(PathBuf::from("/usr/share/bramble-studio/sounds"));
    dirs.into_iter().find(|d| valid(d))
}

/// Where the sample library is installed (searched once per process).
pub fn root() -> Option<&'static Path> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(search).as_deref()
}

/// Whether the sample library is installed.
pub fn installed() -> bool {
    root().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_is_consistent() {
        let mut ids = std::collections::HashSet::new();
        for s in SOUNDS {
            assert!(ids.insert(s.id), "duplicate id {}", s.id);
            assert!(s.sfz.contains("<region>"), "{} has no regions", s.id);
            for line in s.sfz.lines().filter(|l| l.starts_with("sample=")) {
                assert!(line.starts_with("sample=samples/"), "{}: {line}", s.id);
            }
        }
    }

    #[test]
    fn kits_by_character() {
        assert_eq!(for_kit("SoCal").unwrap().id, "drums-modern");
        assert_eq!(for_kit("Brooklyn").unwrap().id, "drums-modern");
        assert_eq!(for_kit("Bluebird").unwrap().id, "drums-vintage");
        assert_eq!(for_kit("Speakeasy").unwrap().id, "drums-brush");
        assert_eq!(for_kit("Blue Ridge").unwrap().id, "drums-brush");
        assert!(for_kit("Trap Door").is_none());
    }

    #[test]
    fn keyword_pick() {
        assert_eq!(pick(Family::Brass, "solo trumpet").unwrap().id, "trumpet");
        assert_eq!(pick(Family::Brass, "pop horn section").unwrap().id, "brass-section");
        assert_eq!(pick(Family::Strings, "solo violin").unwrap().id, "violin-solo");
        assert_eq!(pick(Family::Guitar, "nylon guitar").unwrap().id, "guitar-nylon");
    }
}
