//! Patch character: every Logic patch name gets its own deterministic voicing, so that patches
//! with different names sound different, as in Logic (where each patch is its own channel strip).
//!
//! - Descriptor words in the name ("Dusty", "Absolute Zero", "Dark", "Wide", "Driven"…) and genre
//!   hints (the Session Players' default kits: Trap Door = trap, Crate Digger = boom bap…) give a
//!   patch its traits: lo-fi, dark, bright, cold, warm, heavy, soft, space, dirty, tight, wide.
//! - The traits bias the instrument's own voicing knobs; a stable hash of the name gives the
//!   remaining variation, within musical ranges.
//! - Drum kits: each Drum Machine Designer pad gets its kit, tuning and decay per piece (kick,
//!   snare, hats, toms, percussion), plus punch, hi-hat tone, snare wires and room; acoustic kits
//!   their tuning, decays, room and overheads.
//! - `fx_chain`: Studio's own effects as the patch's channel-strip Audio FX (drive or amp, bit
//!   crusher, phaser, chorus, tremolo, auto filter, EQ, compressor, delay, reverb).
//!
//! Nothing here comes from Apple's patches: the words are the patch names' own, the values ours.

use crate::patches::{Cat, Engine, FxSpec, PatchSpec};
use std::sync::OnceLock;

/// FNV-1a over the lower-case name: stable across runs, builds and platforms.
pub fn hash(low: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in low.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A uniform value in [0, 1) for one knob (`salt`) of a patch.
pub fn unit(h: u64, salt: u32) -> f32 {
    let mut z = h ^ (salt as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn pick<T: Copy>(h: u64, salt: u32, xs: &[T]) -> T {
    xs[((unit(h, salt) * xs.len() as f32) as usize).min(xs.len() - 1)]
}

/// What the name says about the sound (0 = not at all, 1 = strongly).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Traits {
    pub lofi: f32,
    pub bits: f32,
    pub dark: f32,
    pub bright: f32,
    pub cold: f32,
    pub warm: f32,
    pub heavy: f32,
    pub soft: f32,
    pub space: f32,
    pub dirty: f32,
    pub tight: f32,
    pub wide: f32,
    pub vintage: f32,
    pub metallic: f32,
}

fn score(low: &str, words: &[&str]) -> f32 {
    let n = words.iter().filter(|w| low.contains(*w)).count();
    (n as f32 * 0.7).min(1.0)
}

/// Genre and descriptor hints for kits whose names don't say it themselves: the Session
/// Players' default kits (character → kit, logic-research-private session-players/characters.csv)
/// and Drum Kit Designer kits by their drummer's genre.
const HINTS: &[(&str, &str)] = &[
    ("trap door", "trap 808 dark"),
    ("beat machine", "hip hop 808"),
    ("crate digger", "boom bap dusty vintage"),
    ("big room", "house festival wide heavy"),
    ("deep tech", "house tech dark tight"),
    ("pile driver", "heavy dirty dubstep"),
    ("seismic", "trap heavy 808"),
    ("after party", "house warm"),
    ("neon", "80s bright"),
    ("silverlake", "80s synthpop"),
    ("socal", "pop rock bright tight"),
    ("retro rock", "rock vintage warm"),
    ("heavy", "heavy dirty rock big"),
    ("east bay", "punk dirty bright tight"),
    ("smash", "funk rock heavy compressed"),
    ("manchester", "britpop vintage warm"),
    ("portland", "indie soft warm"),
    ("four on the floor", "disco bright tight"),
    ("brooklyn", "indie rock dirty"),
    ("detroit garage", "garage dirty vintage"),
    ("slow jam", "r&b soft warm dark"),
    ("neo soul", "soul warm soft"),
    ("motown revisited", "motown vintage warm"),
    ("sunset", "funk warm bright"),
    ("bluebird", "songwriter soft warm"),
    ("scientific method", "experimental dirty space"),
    ("roots", "roots vintage warm soft"),
    ("liverpool", "60s vintage dark"),
    ("blue ridge", "brush soft vintage"),
    ("speakeasy", "brush soft dark vintage"),
    ("8-bit", "bits chiptune"),
    ("demolition", "heavy dirty"),
    ("dirty jungle", "dirty lo-fi breaks"),
    ("fake machine", "tight cold 808"),
    ("french connection", "disco warm"),
    ("hifi pop", "bright pop clean"),
    ("ny garage", "garage dirty"),
    ("old school", "vintage hip hop dusty"),
    ("sixties soul", "60s vintage warm"),
    ("studio r&b", "r&b warm"),
    ("unmixed", "raw"),
    ("vintage breaks", "breaks vintage dusty"),
    ("absolute zero", "cold clean precise"),
    ("hyper drop", "festival heavy bright"),
    ("analog haze", "analog haze warm"),
    ("dusty fingers", "dusty lo-fi vintage boom bap"),
];

/// The traits of a (lower-case) name: its own words plus the hints for it.
pub fn traits(low: &str) -> Traits {
    let base = low.trim_end_matches('+').trim();
    let mut text = low.to_string();
    for (k, v) in HINTS {
        if base == *k || base.starts_with(&format!("{k} ")) {
            text.push(' ');
            text.push_str(v);
        }
    }
    let s = text.as_str();
    Traits {
        lofi: score(
            s,
            &[
                "lo-fi", "lofi", "dust", "cassette", "tape", "reel to reel", "vinyl", "crate",
                "worn", "noise reduction", "lowtech", "pawn shop", "found sound", "radio",
                "boom bap", "broken", "old-time", "degrad", "grandma", "record",
            ],
        ),
        bits: score(
            s,
            &[
                "8-bit", "chiptune", "arcade", "popping bits", "bits", "pixel", "gamer", "battle blocks",
                "bitrate", "crush", "16-bit", "toy",
            ],
        ),
        dark: score(
            s,
            &[
                "dark", "deep", "shadow", "midnight", "night", "phantom", "mystic", "myster",
                "onyx", "secret", "smoke", "black", "grim", "moody", "noir", "dusk", "abyss",
                "below", "void", "muted", "mellow", "haze", "sub", "underground",
            ],
        ),
        bright: score(
            s,
            &[
                "bright", "shine", "shiny", "sparkl", "crystal", "sapphire", "topaz", "gem",
                "platinum", "diamond", "neon", "radiant", "prism", "solaris", "summer", "glow",
                "stardust", "strobe", "glass", "air", "crisp", "clear", "pop", "hifi", "light",
            ],
        ),
        cold: score(
            s,
            &[
                "zero", "chrome", "steel", "ice", "frost", "cold", "glacier", "minimal",
                "clean", "precise", "digital", "hardwired", "machine", "control voltage",
                "interchange", "metallic", "frozen", "arctic", "robot", "cyber", "laser",
            ],
        ),
        warm: score(
            s,
            &[
                "warm", "ember", "soul", "cream", "breeze", "autumn", "velvet", "honey", "heat",
                "analog", "vintage", "retro", "classic", "mellow", "smooth", "golden", "70s",
                "sunset", "cozy", "funk",
            ],
        ),
        heavy: score(
            s,
            &[
                "heavy", "hard", "titan", "big", "pile", "seismic", "shockwave", "industr",
                "thick", "ton", "boom", "bump", "knock", "drop", "massive", "bang", "solid",
                "tough", "demolition", "smash", "huge", "monster", "punch", "power", "fat",
                "dubstep",
            ],
        ),
        soft: score(
            s,
            &[
                "soft", "gentle", "breath", "feather", "dream", "bliss", "flow", "fluid", "calm",
                "whisper", "brush", "lullaby", "felt", "tender", "quiet", "delicate", "silk",
                "sweet",
            ],
        ),
        space: score(
            s,
            &[
                "space", "cosmic", "orion", "lunar", "infinity", "pulsar", "ancient",
                "cinematic", "hollywood", "dream", "haze", "stardust", "galax", "echo",
                "ambient", "hall", "cathedral", "cloud", "atmos", "ethereal", "deep end",
                "above", "heaven", "orbit", "nebula", "vast", "horizon", "reflect", "verb",
            ],
        ),
        dirty: score(
            s,
            &[
                "drive", "distort", "dirty", "grit", "grime", "crunch", "blowing", "amped",
                "blazing", "fire", "fuzz", "rough", "jagged", "saturat", "burn", "red line",
                "hot", "garage", "punk", "broken", "filthy", "nasty", "growl", "raw",
            ],
        ),
        tight: score(
            s,
            &[
                "tight", "compact", "small", "minimal", "click", "stick", "micro", "precise",
                "dry", "clean", "short", "staccato", "pluck", "punchy", "snappy",
            ],
        ),
        wide: score(
            s,
            &[
                "wide", "stereo", "huge", "big room", "spread", "festival", "stadium",
                "double", "anthem", "epic", "massive",
            ],
        ),
        vintage: score(
            s,
            &[
                "vintage", "retro", "classic", "old", "60s", "70s", "motown", "sixties",
                "liverpool", "roots", "throwback", "flashback",
            ],
        ),
        metallic: score(
            s,
            &["metal", "steel", "chrome", "plates", "clicks", "shell", "edge", "iron", "bell"],
        ),
    }
}

// ---------------------------------------------------------------------------------------------
// Drum kits

/// The kit pieces (Drum Kit Designer groups) of every note, as `drums::piece_of` says.
fn pieces() -> &'static [Option<crate::drums::Group>; 128] {
    static P: OnceLock<[Option<crate::drums::Group>; 128]> = OnceLock::new();
    P.get_or_init(|| std::array::from_fn(|n| crate::drums::piece_of(n as u8)))
}

fn gi(g: Option<crate::drums::Group>) -> usize {
    use crate::drums::Group::*;
    match g {
        Some(Kick) => 0,
        Some(Snare) => 1,
        Some(Tom) => 2,
        Some(HiHat) => 3,
        Some(Cymbal) => 4,
        _ => 5,
    }
}

/// A kit's voicing per piece (kick, snare, tom, hi-hat, cymbal, percussion).
#[derive(Clone, Copy, Debug)]
struct KitVoice {
    /// DMD source kit per piece (drums::KITS index).
    kits: [u8; 6],
    tune: [f32; 6],
    decay: [f32; 6],
    punch: f32,
    hat_tone: f32,
    wires: f32,
    room: f32,
    width: f32,
}

fn has(s: &str, w: &[&str]) -> bool {
    w.iter().any(|x| s.contains(x))
}

fn kit_voice(low: &str, t: &Traits, electronic: bool) -> KitVoice {
    let h = hash(low);
    let u = |k: u32| unit(h, k);
    let b = |k: u32| unit(h, k) * 2.0 - 1.0;
    // the family of the machine, from genre words
    let fam = if !electronic {
        3
    } else if has(low, &["808", "trap", "boom", "hip hop", "flex", "bump", "knock", "h-town", "dembow", "riddim", "uptown", "platinum", "heat", "seismic", "bass house", "crate", "beat machine"]) {
        0
    } else if has(low, &["909", "house", "techno", "tech", "club", "hacienda", "ibiza", "dance", "festival", "drop", "rave", "disco", "big room", "after party"]) {
        1
    } else if has(low, &["linn", "drumtraks", "cr-", "tr-", "simmons", "80s", "neon", "flashback", "synthia", "retro box", "vintage", "throwback", "silverlake", "pop"]) {
        2
    } else if has(low, &["acoustic", "live", "found sound", "foley", "kitchen", "natural", "organic", "jazz", "birdland", "agogo", "clicks and sticks", "shell game"]) {
        3
    } else {
        (h % 4) as usize
    };
    let sets: [[&[u8]; 6]; 4] = [
        // kick, snare, tom, hat, cymbal, perc
        [&[4, 4, 4, 6, 2], &[4, 6, 5, 1, 4], &[4, 4, 6], &[4, 6, 5, 4], &[4, 5, 6], &[4, 6, 5]],
        [&[5, 5, 5, 4, 6], &[5, 6, 4, 5], &[5, 4], &[5, 6, 5, 0], &[5, 5, 0], &[5, 6, 4]],
        [&[6, 5, 6, 0, 4], &[6, 1, 5, 6], &[6, 0, 6], &[6, 5, 0, 6], &[6, 0, 5], &[6, 4, 1]],
        [&[0, 1, 2, 3, 6], &[0, 1, 3, 2], &[0, 1, 2, 3], &[0, 1, 3, 0], &[0, 1, 2, 3], &[0, 1, 3]],
    ];
    let mut kits: [u8; 6] = std::array::from_fn(|g| pick(h, 10 + g as u32, sets[fam][g]));
    if t.lofi > 0.3 && electronic {
        kits[1] = pick(h, 17, &[1, 4, 6]);
    }
    if t.heavy > 0.5 && electronic && fam != 3 {
        kits[0] = pick(h, 18, &[4, 2, kits[0]]);
    }
    let len = t.soft * 0.4 + t.space * 0.5 - t.tight * 0.6 - t.cold * 0.3;
    let kick_tune = b(20) * 4.0 - t.heavy * 3.0 + t.tight * 2.0 + t.cold * 1.5 - t.dark * 1.5;
    let kick_decay = if fam == 0 { lerp(1.2, 2.6, u(21) + t.heavy * 0.3) } else { lerp(0.55, 1.7, u(21) + len * 0.4 + t.heavy * 0.2) };
    let snare_tune = b(22) * 4.5 + t.bright * 2.0 - t.dark * 2.5 + t.cold;
    let snare_decay = lerp(0.6, 1.6, u(23) + len * 0.4);
    let tom_tune = b(24) * 5.0 - t.heavy * 2.0;
    let tom_decay = lerp(0.6, 1.7, u(25) + len * 0.3);
    let hat_tune = b(26) * 4.0 + t.bright * 2.0 - t.dark * 2.0 + t.cold * 1.5;
    let hat_decay = lerp(0.5, 1.6, u(27) + len * 0.4);
    let cym_tune = b(28) * 3.0;
    let cym_decay = lerp(0.6, 1.6, u(29) + len * 0.4);
    let perc_tune = b(30) * 5.0;
    let perc_decay = lerp(0.6, 1.6, u(31) + len * 0.3);
    KitVoice {
        kits,
        tune: [kick_tune, snare_tune, tom_tune, hat_tune, cym_tune, perc_tune],
        decay: [kick_decay, snare_decay, tom_decay, hat_decay, cym_decay, perc_decay],
        punch: lerp(0.1, 0.85, u(32) * 0.7 + t.heavy * 0.3 + t.tight * 0.2 - t.soft * 0.3),
        hat_tone: (b(33) * 0.6 + t.bright * 0.4 + t.cold * 0.3 - t.dark * 0.5 - t.lofi * 0.4).clamp(-1.0, 1.0),
        wires: lerp(0.4, 1.7, u(34) + t.dirty * 0.2 - t.cold * 0.3),
        room: lerp(0.0, 0.5, u(35) * 0.55 + t.space * 0.45 + t.soft * 0.1 - t.tight * 0.25 - t.cold * 0.3),
        width: lerp(0.3, 0.95, u(36) * 0.7 + t.wide * 0.4),
    }
}

/// Drum Machine Designer settings for an electronic kit: its style, then every pad's kit,
/// tuning and decay by piece, then the kit-wide knobs.
fn drum_machine(low: &str, style: f32) -> Vec<(u32, f32)> {
    use crate::dmd;
    let t = traits(low);
    let v = kit_voice(low, &t, true);
    let h = hash(low);
    let mut s = vec![(2, style)];
    let layout = dmd::default_layout(style.round() as u8);
    let pc = pieces();
    for k in 0..dmd::PADS {
        let note = dmd::FIRST_PAD as usize + k;
        let p = layout[note];
        let src = if p.src == 0 { note as u8 } else { p.src };
        let g = gi(pc[src as usize & 127]);
        // the style's own pad tuning plus the kit's for the piece, and a little per pad
        let tune = (p.tune + v.tune[g] + (unit(h, 100 + k as u32) * 2.0 - 1.0) * 0.6).clamp(-24.0, 24.0);
        s.push((dmd::pad_param(k, 0), v.kits[g] as f32));
        s.push((dmd::pad_param(k, 2), tune));
        s.push((dmd::pad_param(k, 5), v.decay[g].clamp(0.1, 4.0)));
    }
    s.push((1, v.room));
    s.push((dmd::PUNCH, v.punch));
    s.push((dmd::HAT_TONE, v.hat_tone));
    s.push((dmd::WIRES, v.wires));
    s.push((dmd::WIDTH, v.width));
    s
}

/// Drum Kit Designer settings for an acoustic kit (or a percussion set).
fn acoustic_kit(low: &str, kit: f32) -> Vec<(u32, f32)> {
    let t = traits(low);
    let v = kit_voice(low, &t, false);
    let mut s = vec![(0, kit)];
    // (kick, snare, toms, hi-hat, cymbals, percussion): tune ids, decay ids
    let tune_ids = [3u32, 6, 10, 30, 31, 32];
    let decay_ids = [4u32, 7, 11, 14, 16, 33];
    for g in 0..6 {
        // acoustic drums tune less far than machines
        s.push((tune_ids[g], (v.tune[g] * 0.7).clamp(-12.0, 12.0)));
        s.push((decay_ids[g], v.decay[g].clamp(0.2, 3.0)));
    }
    s.push((8, v.wires.clamp(0.0, 2.0)));
    s.push((13, v.hat_tone));
    s.push((18, (v.room + 0.1).min(1.0)));
    s.push((21, v.width));
    s.push((23, v.punch));
    s
}

// ---------------------------------------------------------------------------------------------
// Other instruments: voicing knobs per engine

#[derive(Clone, Copy)]
enum Bias {
    None,
    /// Raises brightness.
    Bright,
    /// Lowers brightness.
    Dark,
    /// Lengthens the sound.
    Long,
    /// More drive / grit.
    Dirty,
    /// More room / space.
    Space,
    /// More stereo / ensemble width.
    Wide,
}

/// (parameter id, low, high, bias, integer).
type Knob = (u32, f32, f32, Bias, bool);

fn knobs(engine: Engine) -> &'static [Knob] {
    use Bias::*;
    match engine {
        Engine::RetroSynth => &[
            (36, 0.22, 0.8, Bright, false),
            (37, 0.0, 0.5, None, false),
            (40, 0.05, 0.65, Bright, false),
            (9, 0.05, 0.55, Wide, false),
            (8, 0.2, 0.95, Wide, false),
            (26, 0.1, 0.9, None, false),
            (33, -18.0, 18.0, None, false),
            (41, 0.0, 0.5, Dirty, false),
            (44, 0.05, 0.75, Wide, false),
            (45, 0.05, 0.8, None, false),
            (57, 0.08, 1.6, Long, false),
            (58, 0.0, 0.8, None, false),
            (29, 0.15, 0.85, None, false),
            (3, -12.0, 12.0, None, false),
        ],
        Engine::Alchemy => &[
            (85, 0.3, 0.88, Bright, false),
            (1, 0.05, 0.95, None, false),
            (6, 0.0, 0.45, Dirty, false),
            (8, 0.05, 0.95, None, false),
            (10, 0.05, 0.85, Wide, false),
            (5, 0.08, 0.75, Space, false),
            (3, 0.05, 0.9, Bright, false),
            (7, 0.1, 0.9, Long, false),
            (35, 0.0, 1.0, None, false),
            (4, 0.0, 0.6, None, false),
        ],
        Engine::VintageKeysEp => &[
            (1, 80.0, 185.0, Long, false),
            (2, 60.0, 170.0, Long, false),
            (3, 0.3, 1.7, Bright, false),
            (4, 0.3, 1.6, None, false),
            (10, 0.05, 0.95, Dark, false),
            (11, -4.0, 10.0, Bright, false),
            (12, -4.0, 8.0, None, false),
            (9, -8.0, 8.0, None, false),
            (5, -0.6, 0.6, None, false),
            (6, 0.0, 0.9, Bright, false),
            (39, 0.1, 0.9, None, false),
        ],
        Engine::TonewheelB3 => &[
            (16, 0.05, 0.7, None, false),
            (17, 0.05, 0.65, Dirty, false),
            (26, -0.7, 0.7, Bright, false),
            (13, 0.0, 0.55, Bright, false),
            (14, 0.3, 0.9, Bright, false),
            (19, 0.4, 1.3, None, false),
            (20, 5.0, 8.0, None, false),
            (24, 0.4, 1.0, Wide, false),
            (15, 0.0, 6.0, None, true),
        ],
        Engine::Clav => &[
            (5, 2.0, 9.0, Long, false),
            (8, 0.15, 0.85, Bright, false),
            (10, 0.15, 0.85, Bright, false),
            (31, -0.7, 0.7, Bright, false),
            (24, 0.03, 0.2, Bright, false),
            (26, 0.1, 0.35, None, false),
            (0, 0.0, 3.0, None, true),
            (23, 0.0, 10.0, None, true),
        ],
        Engine::Plucks => &[
            (1, 0.2, 0.9, Bright, false),
            (2, 0.55, 1.9, Long, false),
            (4, 0.25, 0.95, None, false),
            (6, 0.03, 0.55, Space, false),
            (11, 0.25, 0.95, Wide, false),
            (3, 0.08, 0.6, Long, false),
        ],
        Engine::Ensemble => &[
            (3, 0.2, 0.85, Bright, false),
            (5, 0.05, 0.75, Wide, false),
            (6, 0.05, 0.65, None, false),
            (7, 0.15, 0.9, Wide, false),
            (8, 0.08, 0.65, Space, false),
            (2, 0.3, 2.0, Long, false),
        ],
        Engine::Mellotron => &[
            (2, 0.15, 0.85, Bright, false),
            (11, -0.2, 0.2, None, false),
            (5, 0.02, 0.35, Long, false),
            (6, 0.1, 0.6, Long, false),
            (4, 0.0, 0.5, None, false),
            (8, 0.0, 9.0, None, true),
        ],
        Engine::Piano => &[
            (1, -0.7, 0.7, Bright, false),
            (2, 0.2, 0.85, Bright, false),
            (3, 0.65, 1.5, Long, false),
            (5, 0.15, 0.85, None, false),
            (7, 0.05, 0.65, Space, false),
            (8, 0.25, 0.95, None, false),
            (15, 0.05, 0.55, None, false),
        ],
        Engine::Bass => &[
            (1, 0.2, 0.85, Bright, false),
            (2, 0.55, 1.7, Long, false),
            (3, 0.05, 0.32, Bright, false),
            (4, 0.05, 0.32, None, false),
            (5, 0.0, 0.55, Dirty, false),
            (10, 0.05, 0.75, None, false),
        ],
        _ => &[],
    }
}

fn bias_of(b: Bias, t: &Traits) -> f32 {
    match b {
        Bias::None => 0.0,
        Bias::Bright => t.bright + t.cold * 0.5 - t.dark - t.warm * 0.4 - t.lofi * 0.5 - t.soft * 0.4,
        Bias::Dark => -(t.bright + t.cold * 0.5 - t.dark - t.warm * 0.4 - t.lofi * 0.5 - t.soft * 0.4),
        Bias::Long => t.soft * 0.6 + t.space * 0.6 - t.tight * 0.8,
        Bias::Dirty => t.dirty - t.cold * 0.4 - t.soft * 0.3,
        Bias::Space => t.space - t.tight * 0.5,
        Bias::Wide => t.wide + t.space * 0.3 - t.tight * 0.3,
    }
}

/// Spreads a spec's voicing knobs: an unset knob takes a value from the name's hash, biased by
/// its traits; a knob the patch table set keeps its value, moved a little.
fn spread(spec: &mut PatchSpec, low: &str) {
    let t = traits(low);
    let h = hash(low);
    for (i, &(id, lo, hi, b, int)) in knobs(spec.engine).iter().enumerate() {
        let u = unit(h, 200 + i as u32);
        let x = (0.15 + 0.7 * u + 0.45 * bias_of(b, &t)).clamp(0.0, 1.0);
        let set = spec.settings.iter().rposition(|(k, _)| *k == id);
        let mut v = match set {
            Some(j) => {
                if int {
                    continue;
                }
                let cur = spec.settings[j].1;
                let (a, z) = (lo.min(cur), hi.max(cur));
                (cur + (x - 0.5) * (hi - lo) * 0.35).clamp(a, z)
            }
            None => lerp(lo, hi, x),
        };
        if int {
            v = v.round();
        }
        spec.settings.push((id, v));
    }
    // the Retro Synth's oscillator shapes (half the patches change them)
    if spec.engine == Engine::RetroSynth && unit(h, 260) < 0.5 {
        spec.settings.push((28, pick(h, 261, &[0.0, 1.0, 2.0, 3.0])));
        spec.settings.push((30, pick(h, 262, &[0.0, 1.0, 2.0, 3.0])));
    }
    // organ drawbars: the upper ones moved a little
    if spec.engine == Engine::TonewheelB3 {
        for (k, id) in [3u32, 4, 5, 6, 7, 8].iter().enumerate() {
            let cur = spec.settings.iter().rev().find(|(i, _)| i == id).map(|p| p.1).unwrap_or(0.0);
            let d = (unit(h, 270 + k as u32) * 6.0 - 3.0 + (t.bright - t.dark) * 2.0).round();
            spec.settings.push((*id, (cur + d).clamp(0.0, 8.0)));
        }
    }
}

/// Sampler settings for a sampled stand-in of a patch (tuning, filter, envelope scales), so the
/// sampled pianos, strings, guitars, basses and kits keep their patch's character.
pub fn sampler_settings(name: &str) -> Vec<(u32, f32)> {
    let low = name.trim().trim_end_matches(".patch").to_lowercase();
    let t = traits(&low);
    let h = hash(&low);
    let br = bias_of(Bias::Bright, &t);
    let long = bias_of(Bias::Long, &t);
    let cutoff = 20000.0 * (0.3f32).powf(1.0 - (0.62 + 0.45 * unit(h, 300) + 0.35 * br).clamp(0.0, 1.0));
    vec![
        (11, (unit(h, 301) * 2.0 - 1.0) * 14.0),
        (5, 1.0),
        (6, cutoff.clamp(2500.0, 20000.0)),
        (7, unit(h, 302) * 0.25),
        (2, lerp(0.7, 1.4, unit(h, 303) + long * 0.3)),
        (4, lerp(0.6, 1.6, unit(h, 304) + long * 0.4)),
        (1, lerp(0.8, 1.5, unit(h, 305) * 0.5 + t.soft * 0.5)),
    ]
}

/// Gives a looked-up patch its own character (see the module notes).
pub fn voice(spec: &mut PatchSpec, low: &str) {
    if is_empty_kit(low) {
        spec.engine = Engine::DrumMachine;
        spec.settings = vec![(2, crate::dmd::EMPTY_STYLE as f32)];
        return;
    }
    match spec.engine {
        Engine::DrumMachine => {
            let style = spec.settings.iter().rev().find(|(i, _)| *i == 2).map(|p| p.1).unwrap_or(0.0);
            spec.settings = drum_machine(low, style);
        }
        Engine::DrumKit => {
            // tuned membranes (timpani, taiko…) keep their tuning and stay as they are
            if spec.settings.iter().any(|(i, _)| *i == 10) {
                return;
            }
            let kit = spec.settings.iter().rev().find(|(i, _)| *i == 0).map(|p| p.1).unwrap_or(0.0);
            spec.settings = acoustic_kit(low, kit);
        }
        _ => spread(spec, low),
    }
}

/// "Empty Kit" (and "Empty Pad"): a Drum Machine Designer with no sounds, as Logic's.
pub fn is_empty_kit(low: &str) -> bool {
    matches!(low, "empty kit" | "empty pad")
}

// ---------------------------------------------------------------------------------------------
// Channel-strip effects

fn fx(name: &'static str, settings: Vec<(u32, f32)>) -> FxSpec {
    FxSpec { name, settings }
}

fn kit_fx(low: &str, t: &Traits, electronic: bool, producer: bool) -> Vec<FxSpec> {
    let h = hash(low);
    let u = |k: u32| unit(h, 400 + k);
    let b = |k: u32| unit(h, 400 + k) * 2.0 - 1.0;
    let mut v = Vec::new();
    if t.bits > 0.0 {
        v.push(fx("Bitcrusher", vec![(0, lerp(0.0, 6.0, u(0))), (2, 1.0), (3, lerp(4.0, 8.0, u(1))), (4, lerp(2.0, 7.0, u(2))), (5, 100.0)]));
    } else if t.lofi > 0.0 {
        v.push(fx("Bitcrusher", vec![(0, lerp(0.0, 4.0, u(0))), (2, 1.0), (3, lerp(8.0, 12.0, u(1))), (4, lerp(1.0, 3.0, u(2))), (5, lerp(35.0, 75.0, u(3)))]));
    }
    if t.dirty > 0.0 {
        v.push(fx("Overdrive", vec![(0, lerp(6.0, 20.0, u(4) * 0.6 + t.dirty * 0.4)), (1, lerp(1200.0, 6000.0, u(5))), (3, 1.0)]));
    }
    if !low.starts_with("unmixed") {
        let high = (b(6) * 4.0 + t.bright * 6.0 + t.cold * 4.0 - t.dark * 6.0 - t.lofi * 8.0 - t.warm * 2.0).clamp(-14.0, 12.0);
        let lowg = (b(7) * 3.5 + t.heavy * 6.0 + t.warm * 2.0 - t.cold * 4.0 - t.tight * 2.0).clamp(-10.0, 12.0);
        let hp = if t.cold > 0.0 || t.tight > 0.3 { lerp(70.0, 180.0, u(8)) } else { 50.0 };
        v.push(fx(
            "Vintage Console EQ",
            vec![(0, high), (3, lowg), (4, lerp(50.0, 160.0, u(9))), (1, b(10) * 5.0), (2, lerp(0.5, 5.0, u(11))), (5, hp), (7, 1.0)],
        ));
        let circuit = if t.vintage > 0.0 || t.warm > 0.3 {
            pick(h, 412, &[4.0, 5.0, 6.0])
        } else if t.cold > 0.0 {
            pick(h, 412, &[0.0, 1.0])
        } else {
            pick(h, 412, &[0.0, 1.0, 2.0, 3.0, 5.0])
        };
        let mut comp = vec![
            (0, lerp(-30.0, -12.0, u(13)) - t.heavy * 4.0),
            (1, lerp(1.8, 6.0, u(14) * 0.7 + t.heavy * 0.3)),
            (2, lerp(1.0, 30.0, u(15) * 0.7 + t.tight * 0.3)),
            (3, lerp(40.0, 260.0, u(16))),
            (4, lerp(1.0, 5.0, u(17))),
            (7, 0.0),
            (9, circuit),
        ];
        if t.dirty > 0.3 {
            comp.push((8, 1.0));
        }
        if producer {
            // the Producer kit's parallel bus compression
            comp.extend_from_slice(&[(0, -34.0), (1, 8.0), (24, 55.0)]);
        }
        v.push(fx("Compressor", comp));
    }
    let space = t.space + if producer { 0.3 } else { 0.0 } + if !electronic { 0.2 } else { 0.0 };
    if space > 0.0 {
        let room = if electronic { pick(h, 418, &[1.0, 2.0, 4.0, 7.0, 11.0, 12.0, 13.0]) } else { pick(h, 418, &[0.0, 1.0, 5.0, 6.0]) };
        v.push(fx(
            "ChromaVerb",
            vec![(0, 100.0), (1, lerp(10.0, 38.0, u(19) * 0.6 + space * 0.4)), (2, room), (5, lerp(0.6, if electronic { 3.5 } else { 1.6 }, u(20))), (8, lerp(30.0, 85.0, u(21)))],
        ));
    }
    v
}

fn keys_fx(low: &str, t: &Traits, cat: Option<Cat>, engine: Option<Engine>) -> Vec<FxSpec> {
    let h = hash(low);
    let u = |k: u32| unit(h, 500 + k);
    let b = |k: u32| unit(h, 500 + k) * 2.0 - 1.0;
    let ep = engine == Some(Engine::VintageKeysEp);
    let organ = engine == Some(Engine::TonewheelB3);
    let mut v = Vec::new();
    let amp_family = matches!(cat, Some(Cat::ElectricPiano | Cat::Clavinet | Cat::Organ | Cat::Guitar));
    if has(low, &["amp", "driven", "drive", "distort", "overdriv", "fuzz", "crunch", "gritty", "dirty", "grit", "growl", "ferrous", "powered"]) {
        if amp_family {
            v.push(fx(
                "Amp Designer",
                vec![
                    (18, pick(h, 501, &[0.0, 1.0, 2.0, 5.0, 6.0, 9.0, 12.0, 17.0])),
                    (1, lerp(2.0, 8.0, u(2) * 0.6 + t.dirty * 0.4)),
                    (2, lerp(3.0, 7.5, u(3))),
                    (3, lerp(3.0, 7.0, u(4))),
                    (4, lerp(3.0, 8.0, u(5))),
                    (7, lerp(-14.0, -6.0, u(6))),
                    (30, 0.0),
                ],
            ));
        } else {
            v.push(fx("Overdrive", vec![(0, lerp(4.0, 16.0, u(7))), (1, lerp(1200.0, 6000.0, u(8))), (3, 1.0)]));
        }
    } else if cat == Some(Cat::SynthBass) && has(low, &["808", "sub"]) {
        // an 808 / sub bass is saturated by ear: its own amount of it
        v.push(fx("Overdrive", vec![(0, lerp(1.0, 14.0, u(7))), (1, lerp(400.0, 4000.0, u(8))), (3, 1.0)]));
    }
    if t.bits > 0.0 || has(low, &["lo-fi", "lofi", "dusty", "vinyl", "cassette", "radio", "degrad", "broken"]) {
        let hard = t.bits > 0.0;
        v.push(fx(
            "Bitcrusher",
            vec![(2, 1.0), (3, if hard { lerp(5.0, 8.0, u(9)) } else { lerp(9.0, 12.0, u(9)) }), (4, if hard { lerp(2.0, 6.0, u(10)) } else { lerp(1.0, 2.5, u(10)) }), (5, if hard { 100.0 } else { lerp(40.0, 70.0, u(11)) })],
        ));
    }
    if has(low, &["phas"]) && !ep {
        v.push(fx("Phaser", vec![(0, lerp(0.1, 1.6, u(12))), (3, lerp(20.0, 70.0, u(13)))]));
    }
    if has(low, &["flang", "jet"]) && !ep {
        v.push(fx("Flanger", vec![(0, lerp(30.0, 60.0, u(14))), (3, lerp(30.0, 75.0, u(15)))]));
    }
    if has(low, &["chorus", "shimmer", "lush", "detuned"]) && !ep {
        v.push(fx("Chorus", vec![(0, lerp(30.0, 60.0, u(16))), (1, lerp(15.0, 50.0, u(17))), (2, lerp(0.3, 2.0, u(18)))]));
    }
    if has(low, &["ensemble", "wide", "stereo", "huge", "spread"]) && !organ {
        v.push(fx("Ensemble", vec![(0, lerp(35.0, 70.0, u(19))), (2, lerp(3.0, 6.0, u(20)).round())]));
    }
    if has(low, &["tremolo", "trem ", "pulsing", "throb", "suitcase"]) && !ep {
        v.push(fx("Tremolo", vec![(0, lerp(35.0, 85.0, u(21))), (8, 0.0), (1, lerp(2.5, 7.0, u(22)))]));
    }
    if has(low, &["wah", "auto filter", "autofilter", "sweep", "wobble", "filtered", "envelope"]) {
        v.push(fx("AutoFilter", vec![(2, lerp(30.0, 65.0, u(23))), (3, lerp(20.0, 60.0, u(24))), (12, lerp(20.0, 60.0, u(25)))]));
    }
    // the patch's tone EQ: every patch has its own
    let br = bias_of(Bias::Bright, t);
    let high = (b(26) * 5.5 + br * 7.0).clamp(-12.0, 10.0);
    let lowg = (b(27) * 4.5 + t.warm * 2.5 + t.heavy * 3.0 - t.cold * 3.0).clamp(-8.0, 9.0);
    v.push(fx("Vintage Console EQ", vec![(0, high), (3, lowg), (4, lerp(40.0, 200.0, u(28))), (1, b(29) * 7.0), (2, lerp(0.5, 6.0, u(30))), (12, lerp(1.5, 4.0, u(31) + t.dirty * 0.5))]));
    if has(low, &["compress", "punch", "squash", "slap", "funk", "pumping"]) || matches!(cat, Some(Cat::Bass | Cat::SynthBass)) {
        v.push(fx("Compressor", vec![(0, lerp(-28.0, -12.0, u(32))), (1, lerp(2.0, 6.0, u(33))), (2, lerp(3.0, 30.0, u(34))), (3, lerp(60.0, 300.0, u(35))), (4, lerp(1.0, 4.0, u(36))), (7, 0.0), (9, pick(h, 537, &[0.0, 1.0, 2.0, 4.0, 5.0, 6.0]))]));
    }
    if has(low, &["echo", "delay", "dub", "repeat", "tape echo"]) {
        v.push(fx("Tape Delay", vec![(20, lerp(12.0, 30.0, u(38))), (3, lerp(20.0, 55.0, u(39)))]));
    }
    let pad = matches!(cat, Some(Cat::SynthPad | Cat::SynthSoundscape | Cat::SynthFx));
    if t.space > 0.0 || pad {
        v.push(fx(
            "ChromaVerb",
            vec![(0, 100.0), (1, lerp(8.0, 35.0, u(40) * 0.6 + t.space * 0.4)), (2, pick(h, 541, &[1.0, 2.0, 3.0, 4.0, 7.0, 10.0, 11.0, 12.0])), (5, lerp(1.0, 5.0, u(42))), (8, lerp(35.0, 90.0, u(43)))],
        ));
    }
    v
}

/// The channel-strip Audio FX of a patch.
pub fn fx_chain(name: &str) -> Vec<FxSpec> {
    let name = name.trim().trim_end_matches(".patch");
    let low = name.to_lowercase();
    if is_empty_kit(&low) {
        return Vec::new();
    }
    let Some(spec) = crate::patches::patch(name) else { return Vec::new() };
    let t = traits(&low);
    match spec.engine {
        Engine::DrumMachine => kit_fx(&low, &t, true, false),
        Engine::DrumKit => kit_fx(&low, &t, false, low.ends_with('+')),
        e => keys_fx(&low, &t, spec.category, Some(e)),
    }
}
