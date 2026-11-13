//! MIDI FX: Logic's MIDI plug-ins (Arpeggiator, Chord Trigger, Modifier, Modulator, Note
//! Repeater, Randomizer, Scripter, Transposer, Velocity Processor), written from Logic's user
//! guide. They sit in series before a software instrument and process (or make) MIDI events in
//! real time, on the song's tick clock (960 per quarter note).
//!
//! - A unit's state is one float vector: its parameters in window order (`params`), then the
//!   data its editors keep (the Arpeggiator's grid, Chord Trigger's chords, the Transposer's
//!   User scale), so it saves, copies and undoes like an effect's settings.
//! - `process` gets the block's incoming events (ticks inside the block) and writes what goes
//!   on, at ticks inside the block; generated notes (arpeggio steps, repeats) are queued inside
//!   the unit until their block comes. `reset` stops everything the unit holds (locate, stop).

use crate::{Param, Unit};
use std::sync::OnceLock;

pub const PPQ: f64 = 960.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Msg {
    On { ch: u8, pitch: u8, vel: u8 },
    Off { ch: u8, pitch: u8 },
    Cc { ch: u8, num: u8, val: u8 },
    /// Pitch bend, -8192..8191.
    Bend { ch: u8, val: i16 },
    /// Channel pressure (aftertouch).
    Pressure { ch: u8, val: u8 },
    PolyAt { ch: u8, pitch: u8, val: u8 },
    Program { ch: u8, num: u8 },
}

impl Msg {
    pub fn ch(&self) -> u8 {
        match *self {
            Msg::On { ch, .. } | Msg::Off { ch, .. } | Msg::Cc { ch, .. } | Msg::Bend { ch, .. } | Msg::Pressure { ch, .. } | Msg::PolyAt { ch, .. } | Msg::Program { ch, .. } => ch,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ev {
    pub tick: f64,
    pub msg: Msg,
}

impl Ev {
    pub fn new(tick: f64, msg: Msg) -> Ev {
        Ev { tick, msg }
    }
}

/// The block: ticks [from, to) on the song clock while playing, on a free clock while stopped.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub from: f64,
    pub to: f64,
    pub bpm: f64,
    pub playing: bool,
}

impl Ctx {
    pub fn ticks_per_sec(&self) -> f64 {
        self.bpm.max(1.0) / 60.0 * PPQ
    }
    pub fn secs(&self) -> f64 {
        (self.to - self.from).max(0.0) / self.ticks_per_sec()
    }
    fn ms_to_ticks(&self, ms: f64) -> f64 {
        ms / 1000.0 * self.ticks_per_sec()
    }
}

/// What a unit's window shows while it runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// The step playing (Arpeggiator grid), the cycle count.
    pub step: Option<usize>,
    pub cycle: u32,
    /// Keys coming in, and notes going out.
    pub input: Vec<u8>,
    pub output: Vec<u8>,
    /// Last control value made (Modulator: LFO, envelope; -1..1 before scaling).
    pub lfo: f32,
    pub env: f32,
    /// The Arpeggiator runs while the song is stopped (Play clicked then).
    pub free: bool,
    /// Scripter: its console, parameters and error.
    pub script: Option<std::sync::Arc<crate::scripter::View>>,
}

pub trait MidiFx: Send {
    fn load(&mut self, values: &[f32]);
    fn process(&mut self, input: &[Ev], ctx: &Ctx, out: &mut Vec<Ev>);
    /// Note offs for everything the unit holds; queued events dropped.
    fn reset(&mut self, out: &mut Vec<Ev>);
    /// The unit was bypassed (Scripter calls the script's Reset then).
    fn bypassed(&mut self, out: &mut Vec<Ev>) {
        self.reset(out);
    }
    fn status(&self) -> Status {
        Status::default()
    }
    /// A one-off command from the window (Arpeggiator: "Clear", "Play").
    fn command(&mut self, _name: &str) {}
}

pub const NAMES: [&str; 9] = ["Arpeggiator", "Chord Trigger", "Modifier", "Modulator", "Note Repeater", "Randomizer", "Scripter", "Transposer", "Velocity Processor"];

// ---------------------------------------------------------------------------------------
// shared bits

#[derive(Clone, Debug)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed.max(1).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// 0..1
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

fn time_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7)
}

fn leak(v: Vec<String>) -> &'static [&'static str] {
    Box::leak(v.into_iter().map(|s| &*Box::leak(s.into_boxed_str())).collect::<Vec<&'static str>>().into_boxed_slice())
}

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// A key's name as Logic writes it (60 = C3).
pub fn note_name(p: u8) -> String {
    format!("{}{}", NOTE_NAMES[p as usize % 12], p as i32 / 12 - 2)
}

fn key_names() -> &'static [&'static str] {
    static N: OnceLock<&'static [&'static str]> = OnceLock::new();
    N.get_or_init(|| leak((0..128u8).map(note_name).collect()))
}

/// Controller names for the event pop-ups: CC 0..127, then Aftertouch (128), Pitch Bend (129).
pub fn cc_label(n: u8) -> &'static str {
    match n {
        0 => "Bank MSB",
        1 => "Modulation",
        2 => "Breath",
        4 => "Foot Control",
        5 => "Portamento Time",
        7 => "Volume",
        8 => "Balance",
        10 => "Pan",
        11 => "Expression",
        32 => "Bank LSB",
        64 => "Sustain",
        65 => "Portamento",
        66 => "Sostenuto",
        67 => "Soft Pedal",
        71 => "Resonance",
        74 => "Cutoff",
        91 => "Reverb",
        93 => "Chorus",
        _ => "",
    }
}

fn event_names() -> &'static [&'static str] {
    static N: OnceLock<&'static [&'static str]> = OnceLock::new();
    N.get_or_init(|| {
        let mut v: Vec<String> = (0..128u8).map(|i| if cc_label(i).is_empty() { format!("{i}") } else { format!("{i} {}", cc_label(i)) }).collect();
        v.push("Aftertouch".into());
        v.push("Pitch Bend".into());
        leak(v)
    })
}

/// Off, then the controllers (Modifier's Re-Assign To).
fn event_names_off() -> &'static [&'static str] {
    static N: OnceLock<&'static [&'static str]> = OnceLock::new();
    N.get_or_init(|| leak(std::iter::once("Off".to_string()).chain(event_names().iter().map(|s| s.to_string())).collect()))
}

/// Note Number, Velocity, then the controllers (Randomizer's Event Type).
fn random_names() -> &'static [&'static str] {
    static N: OnceLock<&'static [&'static str]> = OnceLock::new();
    N.get_or_init(|| leak(["Note Number".to_string(), "Velocity".to_string()].into_iter().chain(event_names().iter().map(|s| s.to_string())).collect()))
}

/// A controller code's value (0..127; pitch bend scaled down) when `m` is that event.
fn code_value(m: &Msg, code: i32) -> Option<f32> {
    match *m {
        Msg::Cc { num, val, .. } if code == num as i32 => Some(val as f32),
        Msg::Pressure { val, .. } if code == 128 => Some(val as f32),
        Msg::Bend { val, .. } if code == 129 => Some((val as f32 + 8192.0) / 128.0),
        _ => None,
    }
}

fn code_msg(code: i32, ch: u8, v: f32) -> Option<Msg> {
    let v = v.clamp(0.0, 127.0);
    match code {
        0..=127 => Some(Msg::Cc { ch, num: code as u8, val: v.round() as u8 }),
        128 => Some(Msg::Pressure { ch, val: v.round() as u8 }),
        129 => Some(Msg::Bend { ch, val: ((v * 128.0).round() as i32 - 8192).clamp(-8192, 8191) as i16 }),
        _ => None,
    }
}

/// A value inside min..max, or outside it when the two cross (Logic: crossing inverts).
fn in_range(v: f32, min: f32, max: f32) -> bool {
    if min <= max { v >= min && v <= max } else { v >= min || v <= max }
}

pub const SCALES: [(&str, &[u8]); 17] = [
    ("Chromatic", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
    ("Major", &[0, 2, 4, 5, 7, 9, 11]),
    ("Major Pentatonic", &[0, 2, 4, 7, 9]),
    ("Major Blues", &[0, 2, 3, 4, 7, 9]),
    ("Lydian", &[0, 2, 4, 6, 7, 9, 11]),
    ("Mixolydian", &[0, 2, 4, 5, 7, 9, 10]),
    ("Klezmer", &[0, 1, 4, 5, 7, 8, 10]),
    ("Minor Pentatonic", &[0, 3, 5, 7, 10]),
    ("Minor Blues", &[0, 3, 5, 6, 7, 10]),
    ("Japanese", &[0, 1, 5, 7, 10]),
    ("Natural Minor", &[0, 2, 3, 5, 7, 8, 10]),
    ("Harmonic Minor", &[0, 2, 3, 5, 7, 8, 11]),
    ("Melodic Minor", &[0, 2, 3, 5, 7, 9, 11]),
    ("Dorian", &[0, 2, 3, 5, 7, 9, 10]),
    ("Phrygian", &[0, 1, 3, 5, 7, 8, 10]),
    ("Locrian", &[0, 1, 3, 5, 6, 8, 10]),
    ("South-East Asian", &[0, 1, 3, 7, 8]),
];

const SCALE_NAMES: [&str; 17] = ["Chromatic", "Major", "Major Pentatonic", "Major Blues", "Lydian", "Mixolydian", "Klezmer", "Minor Pentatonic", "Minor Blues", "Japanese", "Natural Minor", "Harmonic Minor", "Melodic Minor", "Dorian", "Phrygian", "Locrian", "South-East Asian"];
const SCALE_NAMES_USER: [&str; 18] = ["Chromatic", "Major", "Major Pentatonic", "Major Blues", "Lydian", "Mixolydian", "Klezmer", "Minor Pentatonic", "Minor Blues", "Japanese", "Natural Minor", "Harmonic Minor", "Melodic Minor", "Dorian", "Phrygian", "Locrian", "South-East Asian", "User"];

/// The nearest key in the scale (`mask`: 12 pitch classes from the root); ties go down.
pub fn snap_to_scale(pitch: u8, root: u8, mask: &[bool; 12]) -> u8 {
    if !mask.iter().any(|&b| b) {
        return pitch;
    }
    let on = |p: i32| p >= 0 && p <= 127 && mask[((p - root as i32).rem_euclid(12)) as usize];
    for d in 0..12 {
        for p in [pitch as i32 - d, pitch as i32 + d] {
            if on(p) {
                return p as u8;
            }
        }
    }
    pitch
}

fn scale_mask(k: usize) -> [bool; 12] {
    let mut m = [false; 12];
    for &i in SCALES.get(k).map_or(&SCALES[0].1[..], |s| s.1) {
        m[i as usize] = true;
    }
    m
}

/// Note values as quarter notes.
const RATES: [(&str, f64); 10] = [("1/4.", 1.5), ("1/4", 1.0), ("1/4T", 2.0 / 3.0), ("1/8.", 0.75), ("1/8", 0.5), ("1/8T", 1.0 / 3.0), ("1/16.", 0.375), ("1/16", 0.25), ("1/16T", 1.0 / 6.0), ("1/32", 0.125)];
const RATE_NAMES: [&str; 10] = ["1/4.", "1/4", "1/4T", "1/8.", "1/8", "1/8T", "1/16.", "1/16", "1/16T", "1/32"];
const SYNC_NOTES: [(&str, f64); 16] = [("4 bars", 16.0), ("2 bars", 8.0), ("1/1", 4.0), ("1/2.", 3.0), ("1/2", 2.0), ("1/2T", 4.0 / 3.0), ("1/4.", 1.5), ("1/4", 1.0), ("1/4T", 2.0 / 3.0), ("1/8.", 0.75), ("1/8", 0.5), ("1/8T", 1.0 / 3.0), ("1/16.", 0.375), ("1/16", 0.25), ("1/16T", 1.0 / 6.0), ("1/32", 0.125)];
const SYNC_NAMES: [&str; 16] = ["4 bars", "2 bars", "1/1", "1/2.", "1/2", "1/2T", "1/4.", "1/4", "1/4T", "1/8.", "1/8", "1/8T", "1/16.", "1/16", "1/16T", "1/32"];

fn ix(v: &[f32], i: usize) -> usize {
    v.get(i).copied().unwrap_or(0.0).round().max(0.0) as usize
}
fn fv(v: &[f32], i: usize) -> f32 {
    v.get(i).copied().unwrap_or(0.0)
}
fn on(v: &[f32], i: usize) -> bool {
    fv(v, i) >= 0.5
}

// ---------------------------------------------------------------------------------------
// the registry

/// The parameters a unit's window shows, in order (id = index in the value vector).
pub fn params(name: &str) -> Vec<Param> {
    use Unit::*;
    let p = Param::new;
    let m = Param::menu;
    match name {
        "Arpeggiator" => vec![
            m(arp::PLAY, "Play", &["Off", "On"], 1.0),
            m(arp::LATCH, "Latch", &["Off", "On"], 0.0),
            m(arp::LATCH_MODE, "Latch Mode", &["Reset", "Transpose", "Gated Transpose", "Add", "Add Temporarily", "Through"], 1.0),
            m(arp::RATE, "Rate", &RATE_NAMES, 7.0),
            m(arp::ORDER, "Note Order", &["Up", "Down", "Up/Down", "Outside-in", "Random", "As Played"], 0.0),
            m(arp::VARIATION, "Variation", &["1", "2", "3", "4"], 0.0),
            m(arp::OCT_MODE, "Oct Range/Inversions", &["Octave Range", "Inversions"], 0.0),
            m(arp::RANGE, "Range", &["1", "2", "3", "4"], 0.0),
            p(arp::LENGTH, "Note Length", 1.0, 150.0, 100.0, Percent),
            p(arp::LENGTH_RANDOM, "Note Length Random", 0.0, 100.0, 0.0, Percent),
            p(arp::VELOCITY, "Velocity", 0.0, 100.0, 100.0, Percent),
            p(arp::VEL_BASE, "Vel (Velocity Base)", 1.0, 127.0, 1.0, None),
            m(arp::VEL_MODE, "Crescendo/Random", &["Crescendo", "Random"], 0.0),
            p(arp::VEL_AMOUNT, "Crescendo/Random Amount", -100.0, 100.0, 0.0, Percent),
            p(arp::SWING, "Swing", 0.0, 100.0, 0.0, Percent),
            Param { id: arp::CYCLE, name: "Cycle Length", min: 0.0, max: 33.0, default: 33.0, unit: Menu, choices: cycle_names(), hidden: false },
            m(arp::PATTERN, "Pattern", &["Live", "Grid"], 0.0),
            p(arp::GRID_LEN, "Grid Length", 1.0, 32.0, 16.0, None),
            m(arp::SNAP, "Input Snap", &["Link to Rate", "Off", "1/1", "1/2", "1/4", "1/8", "1/16"], 0.0),
            m(arp::ROOT, "Root", &NOTE_NAMES, 0.0),
            m(arp::SCALE, "Scale", &SCALE_NAMES, 0.0),
            m(arp::SPLIT, "Keyboard Split", &["Off", "On"], 0.0),
            Param { id: arp::ZONE_LO, name: "Arpeggio Zone Low", min: 0.0, max: 127.0, default: 0.0, unit: Menu, choices: key_names(), hidden: false },
            Param { id: arp::ZONE_HI, name: "Arpeggio Zone High", min: 0.0, max: 127.0, default: 127.0, unit: Menu, choices: key_names(), hidden: false },
            Param { id: arp::CTL, name: "Controller 1", min: 0.0, max: 129.0, default: 1.0, unit: Menu, choices: event_names(), hidden: false },
            m(arp::CTL + 1, "Destination 1", &arp::DESTS, 0.0),
            Param { id: arp::CTL + 2, name: "Controller 2", min: 0.0, max: 129.0, default: 2.0, unit: Menu, choices: event_names(), hidden: false },
            m(arp::CTL + 3, "Destination 2", &arp::DESTS, 0.0),
            Param { id: arp::CTL + 4, name: "Controller 3", min: 0.0, max: 129.0, default: 4.0, unit: Menu, choices: event_names(), hidden: false },
            m(arp::CTL + 5, "Destination 3", &arp::DESTS, 0.0),
            Param { id: arp::CTL + 6, name: "Controller 4", min: 0.0, max: 129.0, default: 11.0, unit: Menu, choices: event_names(), hidden: false },
            m(arp::CTL + 7, "Destination 4", &arp::DESTS, 0.0),
        ],
        "Chord Trigger" => vec![
            m(chord::MODE, "Mode", &["Single", "Multi"], 0.0),
            Param { id: chord::LO, name: "Trigger Range Low", min: 0.0, max: 127.0, default: 48.0, unit: Menu, choices: key_names(), hidden: false },
            Param { id: chord::HI, name: "Trigger Range High", min: 0.0, max: 127.0, default: 72.0, unit: Menu, choices: key_names(), hidden: false },
            p(chord::TRANSPOSE, "Chord Transpose", -48.0, 48.0, 0.0, Semitones),
            Param { id: chord::REMOTE, name: "Learn Remote", min: 0.0, max: 128.0, default: 128.0, unit: Menu, choices: learn_remote_names(), hidden: false },
        ],
        "Modifier" => vec![
            m(0, "Input Thru", &["Off", "On"], 0.0),
            Param { id: 1, name: "Input Event", min: 0.0, max: 129.0, default: 1.0, unit: Menu, choices: event_names(), hidden: false },
            Param { id: 2, name: "Re-Assign To", min: 0.0, max: 130.0, default: 3.0, unit: Menu, choices: event_names_off(), hidden: false },
            p(3, "Scale", -800.0, 800.0, 100.0, Percent),
            p(4, "Add", -127.0, 127.0, 0.0, None),
        ],
        "Modulator" => vec![
            m(modu::LFO_ON, "LFO", &["Off", "On"], 1.0),
            m(modu::WAVE, "Waveform", &["Triangle", "Sine", "Square", "Random"], 0.0),
            p(modu::SYMMETRY, "Symmetry", -100.0, 100.0, 0.0, Percent),
            p(modu::SIMILARITY, "Similarity", 0.0, 100.0, 50.0, Percent),
            m(modu::LFO_TRIGGER, "LFO Trigger", &["Free", "Single", "Multi"], 0.0),
            p(modu::LFO_STEPS, "Steps per LFO Cycle", 0.0, 128.0, 0.0, None),
            m(modu::SYNC, "Sync", &["Off", "On"], 1.0),
            p(modu::RATE_HZ, "Rate", 0.01, 40.0, 1.0, Hz),
            m(modu::RATE_SYNC, "Rate (Sync)", &SYNC_NAMES, 7.0),
            Param { id: modu::LFO_TO, name: "LFO To", min: 0.0, max: 129.0, default: 1.0, unit: Menu, choices: event_names(), hidden: false },
            p(modu::LFO_LEVEL, "LFO Output Level", 0.0, 100.0, 100.0, Percent),
            p(modu::LFO_OFFSET, "LFO Offset", -100.0, 100.0, 0.0, Percent),
            p(modu::LFO_CH, "LFO MIDI Channel", 1.0, 16.0, 1.0, None),
            m(modu::ENV_ON, "Envelope", &["Off", "On"], 0.0),
            p(modu::DELAY, "Delay", 0.0, 10.0, 0.0, Seconds),
            p(modu::ATTACK, "Attack", 0.0, 10.0, 0.1, Seconds),
            p(modu::HOLD, "Hold", 0.0, 10.0, 0.5, Seconds),
            p(modu::RELEASE, "Release", 0.0, 10.0, 0.5, Seconds),
            m(modu::ENV_TRIGGER, "Envelope Trigger", &["LFO", "Single", "Multi"], 2.0),
            p(modu::ENV_STEPS, "Steps per Env Pass", 0.0, 128.0, 0.0, None),
            p(modu::ENV_TO_RATE, "Env to LFO Rate", -100.0, 100.0, 0.0, Percent),
            p(modu::ENV_TO_AMP, "Env to LFO Amp", -100.0, 100.0, 0.0, Percent),
            Param { id: modu::ENV_TO, name: "Envelope To", min: 0.0, max: 129.0, default: 74.0, unit: Menu, choices: event_names(), hidden: false },
            p(modu::ENV_LEVEL, "Envelope Output Level", 0.0, 100.0, 100.0, Percent),
            p(modu::ENV_OFFSET, "Envelope Offset", -100.0, 100.0, 0.0, Percent),
            p(modu::ENV_CH, "Envelope MIDI Channel", 1.0, 16.0, 1.0, None),
        ],
        "Note Repeater" => vec![
            m(0, "Input Thru", &["Off", "On"], 1.0),
            m(1, "Delay Sync", &["Off", "On"], 1.0),
            p(2, "Delay", 1.0, 2000.0, 250.0, Ms),
            m(3, "Delay (Sync)", &SYNC_NAMES, 10.0),
            p(4, "Repeats", 1.0, 32.0, 3.0, None),
            p(5, "Transpose", -24.0, 24.0, 0.0, Semitones),
            p(6, "Velocity Ramp", 0.0, 200.0, 80.0, Percent),
            Param { id: 7, name: "Note Range Min", min: 0.0, max: 127.0, default: 1.0, unit: Menu, choices: key_names(), hidden: false },
            Param { id: 8, name: "Note Range Max", min: 0.0, max: 127.0, default: 127.0, unit: Menu, choices: key_names(), hidden: false },
        ],
        "Randomizer" => vec![
            Param { id: 0, name: "Event Type", min: 0.0, max: 131.0, default: 1.0, unit: Menu, choices: random_names(), hidden: false },
            p(1, "Input Range Min", 0.0, 127.0, 0.0, None),
            p(2, "Input Range Max", 0.0, 127.0, 127.0, None),
            p(3, "Amount", 0.0, 127.0, 20.0, None),
            p(4, "Weight", -100.0, 100.0, 0.0, Percent),
            p(5, "Output Offset", -127.0, 127.0, 0.0, None),
            Param { id: 6, name: "Seed", min: 0.0, max: 1000.0, default: 0.0, unit: Menu, choices: seed_names(), hidden: false },
        ],
        // the script declares its own (scripter.rs)
        "Scripter" => vec![],
        "Transposer" => vec![p(0, "Transpose", -24.0, 24.0, 0.0, Semitones), m(1, "Root", &NOTE_NAMES, 0.0), m(2, "Scale", &SCALE_NAMES_USER, 0.0)],
        "Velocity Processor" => vec![
            m(vel::PROCESS, "Process", &["Note On", "Note Off", "Both"], 0.0),
            m(vel::MODE, "Mode", &["Compress/Expand", "Value/Range", "Add/Scale"], 0.0),
            p(vel::THRESHOLD, "Threshold", 1.0, 127.0, 64.0, None),
            p(vel::RATIO, "Ratio", 0.1, 10.0, 2.0, Ratio),
            p(vel::MAKEUP, "Make-up", -127.0, 127.0, 0.0, None),
            m(vel::AUTO, "Auto (Gain)", &["Off", "On"], 0.0),
            m(vel::VALUE_RANGE, "Value/Range", &["Value", "Range"], 0.0),
            p(vel::VALUE, "Value", 1.0, 127.0, 100.0, None),
            p(vel::RANGE_MIN, "Range Min", 1.0, 127.0, 1.0, None),
            p(vel::RANGE_MAX, "Range Max", 1.0, 127.0, 127.0, None),
            p(vel::SCALE, "Scale", 0.0, 200.0, 100.0, Percent),
            p(vel::ADD, "Add", -127.0, 127.0, 0.0, None),
            Param { id: vel::NOTE_MIN, name: "Note Range Min", min: 0.0, max: 127.0, default: 1.0, unit: Menu, choices: key_names(), hidden: false },
            Param { id: vel::NOTE_MAX, name: "Note Range Max", min: 0.0, max: 127.0, default: 127.0, unit: Menu, choices: key_names(), hidden: false },
        ],
        _ => Vec::new(),
    }
}

fn cycle_names() -> &'static [&'static str] {
    static N: OnceLock<&'static [&'static str]> = OnceLock::new();
    N.get_or_init(|| leak(std::iter::once("By Grid".to_string()).chain((1..=32).map(|i| format!("{i}"))).chain(std::iter::once("As Played".to_string())).collect()))
}

fn seed_names() -> &'static [&'static str] {
    static N: OnceLock<&'static [&'static str]> = OnceLock::new();
    N.get_or_init(|| leak(std::iter::once("Random".to_string()).chain((1..=1000).map(|i| format!("{i}"))).collect()))
}

fn learn_remote_names() -> &'static [&'static str] {
    static N: OnceLock<&'static [&'static str]> = OnceLock::new();
    N.get_or_init(|| leak((0..128u8).map(note_name).chain(std::iter::once("Off".to_string())).collect()))
}

/// How many floats the unit keeps after its parameters (its editors' data).
pub fn data_len(name: &str) -> usize {
    match name {
        "Arpeggiator" => arp::STEPS * arp::STEP_FLOATS,
        "Chord Trigger" => chord::DATA,
        "Transposer" => 12,
        _ => 0,
    }
}

/// Where the data starts in the value vector.
pub fn data_at(name: &str) -> usize {
    match name {
        "Arpeggiator" => arp::DATA,
        "Chord Trigger" => chord::DATA_AT,
        "Transposer" => 3,
        "Scripter" => crate::scripter::SCRIPT_AT,
        _ => params(name).len(),
    }
}

/// A new unit's values: the parameters' defaults, then its data.
pub fn defaults(name: &str) -> Vec<f32> {
    if name == "Scripter" {
        return crate::scripter::defaults();
    }
    let ps = params(name);
    let mut v = vec![0.0; data_at(name) + data_len(name)];
    for p in &ps {
        if let Some(x) = v.get_mut(p.id as usize) {
            *x = p.default;
        }
    }
    match name {
        "Arpeggiator" => {
            for s in 0..arp::STEPS {
                let b = arp::DATA + s * arp::STEP_FLOATS;
                v[b] = 1.0; // on
                v[b + 1] = 100.0; // velocity
                v[b + 2] = 100.0; // length %
            }
        }
        "Chord Trigger" => {
            for x in &mut v[chord::DATA_AT..] {
                *x = -1.0;
            }
        }
        "Transposer" => {
            for x in &mut v[3..15] {
                *x = 1.0;
            }
        }
        _ => {}
    }
    v
}

pub fn build(name: &str, values: &[f32]) -> Option<Box<dyn MidiFx>> {
    if name == "Scripter" {
        // the script follows the parameters: keep every float
        return Some(Box::new(crate::scripter::Scripter::new(values)));
    }
    let mut v = defaults(name);
    for (i, x) in values.iter().enumerate().take(v.len()) {
        v[i] = *x;
    }
    let b: Box<dyn MidiFx> = match name {
        "Arpeggiator" => Box::new(arp::Arpeggiator::new(v)),
        "Chord Trigger" => Box::new(chord::ChordTrigger::new(v)),
        "Modifier" => Box::new(Modifier { v }),
        "Modulator" => Box::new(modu::Modulator::new(v)),
        "Note Repeater" => Box::new(NoteRepeater { v, queue: Vec::new(), starts: Vec::new() }),
        "Randomizer" => Box::new(Randomizer::new(v)),
        "Transposer" => Box::new(Transposer { v, map: [None; 128] }),
        "Velocity Processor" => Box::new(VelocityProcessor { v }),
        _ => return None,
    };
    Some(b)
}

// ---------------------------------------------------------------------------------------
// a chain of units in series

/// A track's MIDI FX slots, as running: (the unit's name, the unit, bypassed).
#[derive(Default)]
pub struct Chain {
    pub units: Vec<(String, Box<dyn MidiFx>, bool)>,
    /// Ticks of the free clock (when the song isn't playing).
    pub clock: f64,
}

impl Chain {
    /// Rebuilds or reloads the units for `slots` (name, values, bypass); returns note offs of
    /// units that went away.
    pub fn set(&mut self, slots: &[(String, Vec<f32>, bool)]) -> Vec<Ev> {
        let mut out = Vec::new();
        let same = self.units.len() == slots.len() && self.units.iter().zip(slots).all(|(u, s)| u.0 == s.0);
        if same {
            for (u, s) in self.units.iter_mut().zip(slots) {
                u.1.load(&s.1);
                if s.2 && !u.2 {
                    u.1.bypassed(&mut out);
                }
                u.2 = s.2;
            }
        } else {
            for u in &mut self.units {
                u.1.reset(&mut out);
            }
            self.units = slots.iter().filter_map(|s| build(&s.0, &s.1).map(|b| (s.0.clone(), b, s.2))).collect();
        }
        out
    }

    pub fn active(&self) -> bool {
        self.units.iter().any(|u| !u.2)
    }

    /// The block's events through every unit not bypassed, in order.
    pub fn process(&mut self, input: Vec<Ev>, ctx: &Ctx) -> Vec<Ev> {
        let mut evs = input;
        for (_, u, bypass) in &mut self.units {
            if *bypass {
                continue;
            }
            evs.sort_by(|a, b| a.tick.total_cmp(&b.tick));
            let mut out = Vec::new();
            u.process(&evs, ctx, &mut out);
            evs = out;
        }
        evs.sort_by(|a, b| a.tick.total_cmp(&b.tick));
        evs
    }

    pub fn reset(&mut self) -> Vec<Ev> {
        let mut out = Vec::new();
        for (_, u, _) in &mut self.units {
            u.reset(&mut out);
        }
        out
    }
}

/// Runs `slots` over a list of notes (start, end, pitch, velocity, song ticks) as if played at
/// `bpm` from `from` to `to`: the notes the instrument would get (for capturing a MIDI FX's
/// output as a region).
pub fn render_notes(slots: &[(String, Vec<f32>, bool)], notes: &[(u32, u32, u8, u8)], from: u32, to: u32, bpm: f64) -> Vec<(u32, u32, u8, u8)> {
    let mut chain = Chain::default();
    chain.set(slots);
    let block = 60.0;
    let mut t = from as f64;
    let mut open: Vec<(u8, u32, u8)> = Vec::new();
    let mut done = Vec::new();
    while t < to as f64 {
        let ctx = Ctx { from: t, to: (t + block).min(to as f64), bpm, playing: true };
        let mut input = Vec::new();
        for &(s, e, p, v) in notes {
            if (s as f64) >= ctx.from && (s as f64) < ctx.to {
                input.push(Ev::new(s as f64, Msg::On { ch: 0, pitch: p, vel: v }));
            }
            if (e as f64) >= ctx.from && (e as f64) < ctx.to {
                input.push(Ev::new(e as f64, Msg::Off { ch: 0, pitch: p }));
            }
        }
        for ev in chain.process(input, &ctx) {
            match ev.msg {
                Msg::On { pitch, vel, .. } => open.push((pitch, ev.tick.round() as u32, vel)),
                Msg::Off { pitch, .. } => {
                    if let Some(k) = open.iter().position(|o| o.0 == pitch) {
                        let (p, s, v) = open.remove(k);
                        done.push((s, (ev.tick.round() as u32).max(s + 1), p, v));
                    }
                }
                _ => {}
            }
        }
        t = ctx.to;
    }
    for (p, s, v) in open {
        done.push((s, to.max(s + 1), p, v));
    }
    done.sort_by_key(|n| (n.0, n.2));
    done
}

// ---------------------------------------------------------------------------------------
// Arpeggiator

pub mod arp {
    use super::*;

    pub const PLAY: u32 = 0;
    pub const LATCH: u32 = 1;
    pub const LATCH_MODE: u32 = 2;
    pub const RATE: u32 = 3;
    pub const ORDER: u32 = 4;
    pub const VARIATION: u32 = 5;
    pub const OCT_MODE: u32 = 6;
    pub const RANGE: u32 = 7;
    pub const LENGTH: u32 = 8;
    pub const LENGTH_RANDOM: u32 = 9;
    pub const VELOCITY: u32 = 10;
    pub const VEL_BASE: u32 = 11;
    pub const VEL_MODE: u32 = 12;
    pub const VEL_AMOUNT: u32 = 13;
    pub const SWING: u32 = 14;
    pub const CYCLE: u32 = 15;
    pub const PATTERN: u32 = 16;
    pub const GRID_LEN: u32 = 17;
    pub const SNAP: u32 = 18;
    pub const ROOT: u32 = 19;
    pub const SCALE: u32 = 20;
    pub const SPLIT: u32 = 21;
    pub const ZONE_LO: u32 = 22;
    pub const ZONE_HI: u32 = 23;
    /// Four (controller, destination) pairs.
    pub const CTL: u32 = 24;
    pub const DESTS: [&str; 6] = ["Off", "Note Length", "Note Length Random", "Velocity Range", "Velocity Base", "(De-)Crescendo"];
    /// The grid: 32 steps of (on, velocity 1..127, length % 1..100, tie, chord).
    pub const DATA: usize = 32;
    pub const STEPS: usize = 32;
    pub const STEP_FLOATS: usize = 5;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Order {
        Up,
        Down,
        UpDown,
        OutsideIn,
        Random,
        AsPlayed,
    }

    impl Order {
        fn of(k: usize) -> Order {
            [Order::Up, Order::Down, Order::UpDown, Order::OutsideIn, Order::Random, Order::AsPlayed][k.min(5)]
        }
    }

    /// The order of the held notes' indices for a note order and variation (0..4): `n` notes
    /// sorted low to high (As Played: in played order). Random orders are drawn per step.
    pub fn order(o: Order, variation: usize, n: usize) -> Vec<usize> {
        if n == 0 {
            return Vec::new();
        }
        let up: Vec<usize> = (0..n).collect();
        let groups = |first: usize| -> Vec<usize> {
            // groups of four with the `first` step played first, missing notes skipped
            let mut v = Vec::new();
            for g in (0..n).step_by(4) {
                let mut grp: Vec<usize> = (g..(g + 4)).collect();
                let f = grp.remove(first);
                grp.insert(0, f);
                v.extend(grp.into_iter().filter(|&i| i < n));
            }
            v
        };
        let threes = || -> Vec<usize> {
            if n < 3 {
                return (0..n).collect();
            }
            (0..=n - 3).flat_map(|s| [s, s + 1, s + 2]).collect()
        };
        let rev = |v: Vec<usize>| -> Vec<usize> { v.into_iter().map(|i| n - 1 - i).collect() };
        match (o, variation) {
            (Order::Up, 1) => groups(1),
            (Order::Up, 2) => groups(2),
            (Order::Up, 3) => threes(),
            (Order::Up, _) => up,
            (Order::Down, 1) => rev(groups(1)),
            (Order::Down, 2) => rev(groups(2)),
            (Order::Down, 3) => rev(threes()),
            (Order::Down, _) => rev(up),
            (Order::UpDown, v) => {
                let fwd: Vec<usize> = match v {
                    2 => (0..n).collect::<Vec<_>>().chunks(2).flat_map(|c| c.iter().rev().copied().collect::<Vec<_>>()).collect(),
                    3 => (0..n).collect::<Vec<_>>().chunks(3).flat_map(|c| if c.len() == 3 { vec![c[0], c[2], c[1]] } else { c.to_vec() }).collect(),
                    _ => up.clone(),
                };
                let mut back: Vec<usize> = fwd.iter().rev().copied().collect();
                if v == 1 && n > 2 {
                    // down from the second highest to the second lowest
                    back = (1..n - 1).rev().collect();
                } else if v == 1 {
                    back.clear();
                }
                fwd.into_iter().chain(back).collect()
            }
            (Order::OutsideIn, v) => {
                let mut out = Vec::new();
                match v {
                    0 | 1 => {
                        let (mut lo, mut hi) = (0i64, n as i64 - 1);
                        let mut high_first = v == 0;
                        while lo <= hi {
                            if high_first {
                                out.push(hi as usize);
                                hi -= 1;
                            } else {
                                out.push(lo as usize);
                                lo += 1;
                            }
                            high_first = !high_first;
                        }
                    }
                    _ => {
                        // inside out from the high (2) or low (3) centre: 6 notes give
                        // 4 3 5 2 6 1 / 3 4 2 5 1 6 (1-based)
                        let c = if v == 2 { n / 2 } else { (n - 1) / 2 };
                        out.push(c);
                        for d in 1..n {
                            let (a, b) = if v == 2 { (c as i64 - d as i64, c as i64 + d as i64) } else { (c as i64 + d as i64, c as i64 - d as i64) };
                            for k in [a, b] {
                                if k >= 0 && (k as usize) < n && !out.contains(&(k as usize)) {
                                    out.push(k as usize);
                                }
                            }
                        }
                    }
                }
                out
            }
            (Order::Random, _) => up,
            (Order::AsPlayed, 1) => rev(up),
            (Order::AsPlayed, 2) => up.iter().copied().chain(up.iter().rev().copied()).collect(),
            (Order::AsPlayed, 3) => {
                let back: Vec<usize> = if n > 2 { (1..n - 1).rev().collect() } else { Vec::new() };
                up.into_iter().chain(back).collect()
            }
            (Order::AsPlayed, _) => up,
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Held {
        pitch: u8,
        vel: u8,
        ch: u8,
    }

    pub struct Arpeggiator {
        v: Vec<f32>,
        /// Keys down (played order).
        keys: Vec<Held>,
        /// The notes arpeggiated (latched or held), in played order.
        notes: Vec<Held>,
        /// Notes added temporarily (Add Temporarily) while their keys are down.
        temp: Vec<Held>,
        transpose: i32,
        /// Running: the next step's tick and its number.
        next: Option<f64>,
        step: u64,
        /// Active steps played (the note sequence moves only on those).
        played: u64,
        sounding: Vec<(u8, u8, f64)>,
        /// Notes passed through (split, Through): pitch → channel.
        thru: Vec<(u8, u8)>,
        rng: Rng,
        /// No-repeat random: the indices left in this pass.
        bag: Vec<usize>,
        last_tie: Option<(u8, u8)>,
        /// Started with Play while the song was stopped.
        free: bool,
        was_playing: bool,
        status: Status,
    }

    impl Arpeggiator {
        pub fn new(v: Vec<f32>) -> Arpeggiator {
            Arpeggiator { v, keys: Vec::new(), notes: Vec::new(), temp: Vec::new(), transpose: 0, next: None, step: 0, played: 0, sounding: Vec::new(), thru: Vec::new(), rng: Rng::new(time_seed()), bag: Vec::new(), last_tie: None, free: false, was_playing: false, status: Status::default() }
        }

        fn p(&self, id: u32) -> f32 {
            fv(&self.v, id as usize)
        }

        fn step_q(&self) -> f64 {
            RATES[ix(&self.v, RATE as usize).min(RATES.len() - 1)].1
        }

        fn grid(&self) -> bool {
            on(&self.v, PATTERN as usize)
        }

        fn grid_len(&self) -> usize {
            (self.p(GRID_LEN).round() as usize).clamp(1, STEPS)
        }

        fn grid_step(&self, s: usize) -> (bool, f32, f32, bool, bool) {
            let b = DATA + s * STEP_FLOATS;
            (fv(&self.v, b) >= 0.5, fv(&self.v, b + 1), fv(&self.v, b + 2), fv(&self.v, b + 3) >= 0.5, fv(&self.v, b + 4) >= 0.5)
        }

        fn in_zone(&self, pitch: u8) -> bool {
            !on(&self.v, SPLIT as usize) || in_range(pitch as f32, self.p(ZONE_LO), self.p(ZONE_HI))
        }

        /// The notes the arpeggio plays, low to high (As Played: in played order), with the
        /// latch transposition and the scale applied.
        fn pool(&self) -> Vec<Held> {
            let mut v: Vec<Held> = self.notes.iter().chain(self.temp.iter()).copied().collect();
            let mask = scale_mask(ix(&self.v, SCALE as usize));
            let root = ix(&self.v, ROOT as usize) as u8;
            for h in &mut v {
                let p = (h.pitch as i32 + self.transpose).clamp(0, 127) as u8;
                h.pitch = snap_to_scale(p, root, &mask);
            }
            if Order::of(ix(&self.v, ORDER as usize)) != Order::AsPlayed {
                v.sort_by_key(|h| h.pitch);
            }
            v
        }

        fn cycle_len(&self, n: usize) -> usize {
            match ix(&self.v, CYCLE as usize) {
                0 => self.grid_len(),
                k @ 1..=32 => k,
                _ => n.max(1),
            }
        }

        /// The note(s) for the next active step, with octave / inversion applied.
        fn pick(&mut self, chord: bool) -> Vec<Held> {
            let pool = self.pool();
            let n = pool.len();
            if n == 0 {
                return Vec::new();
            }
            let o = Order::of(ix(&self.v, ORDER as usize));
            let var = ix(&self.v, VARIATION as usize).min(3);
            let range = ix(&self.v, RANGE as usize).min(3) + 1;
            let inversions = ix(&self.v, OCT_MODE as usize) == 1;
            let seq = order(o, var, n);
            let k = self.played as usize;
            let pass = k / seq.len().max(1);
            // octave range: each pass one octave higher, then back; inversions: the chord
            // inverted once more each pass
            let (oct, inv) = if inversions { (0, pass % range) } else { (pass % range, 0) };
            let mut chordv = pool.clone();
            for _ in 0..inv {
                if let Some(first) = chordv.first().copied() {
                    chordv.remove(0);
                    chordv.push(Held { pitch: (first.pitch as i32 + 12).min(127) as u8, ..first });
                }
            }
            let shift = |h: Held| Held { pitch: (h.pitch as i32 + 12 * oct as i32).clamp(0, 127) as u8, ..h };
            if chord {
                return chordv.into_iter().map(shift).collect();
            }
            let idx = match o {
                Order::Random => match var {
                    1 => {
                        if self.bag.is_empty() {
                            self.bag = (0..n).collect();
                        }
                        let j = self.rng.below(self.bag.len());
                        self.bag.remove(j)
                    }
                    2 => ((self.rng.f() * self.rng.f()) * n as f64) as usize,
                    3 => n - 1 - ((self.rng.f() * self.rng.f()) * n as f64) as usize,
                    _ => self.rng.below(n),
                },
                _ => seq[k % seq.len()],
            };
            let oct = if o == Order::Down && !inversions { range - 1 - oct } else { oct };
            let h = chordv[idx.min(n - 1)];
            vec![Held { pitch: (h.pitch as i32 + 12 * oct as i32).clamp(0, 127) as u8, ..h }]
        }

        fn velocity(&mut self, played: u8, cycle: usize) -> u8 {
            let range = self.p(VELOCITY) / 100.0;
            let base = self.p(VEL_BASE).clamp(1.0, 127.0);
            let mut v = base + (played as f32 - base).max(0.0) * range;
            let amt = self.p(VEL_AMOUNT) / 100.0;
            if ix(&self.v, VEL_MODE as usize) == 0 {
                v += amt * 127.0 / 8.0 * cycle as f32;
            } else {
                v += (self.rng.f() as f32 * 2.0 - 1.0) * amt.abs() * 63.0;
            }
            v.round().clamp(base.max(1.0), 127.0) as u8
        }

        fn running(&self, ctx: &Ctx) -> bool {
            on(&self.v, PLAY as usize) && (ctx.playing || self.free)
        }

        fn has_notes(&self) -> bool {
            let gated = ix(&self.v, LATCH_MODE as usize) == 2 && on(&self.v, LATCH as usize);
            !(self.notes.is_empty() && self.temp.is_empty()) && (!gated || !self.keys.is_empty())
        }

        fn key_on(&mut self, h: Held, at: f64, ctx: &Ctx, out: &mut Vec<Ev>) {
            let latch = on(&self.v, LATCH as usize);
            let none_down = self.keys.is_empty();
            self.keys.push(h);
            if !latch {
                self.notes.push(h);
            } else {
                match ix(&self.v, LATCH_MODE as usize) {
                    0 => {
                        if none_down {
                            self.notes.clear();
                        }
                        self.notes.push(h);
                    }
                    1 | 2 => {
                        if none_down && !self.notes.is_empty() {
                            let low = self.notes.iter().map(|n| n.pitch).min().unwrap_or(h.pitch);
                            self.transpose = h.pitch as i32 - low as i32;
                        } else {
                            // several keys at once: a new arpeggio from the keys down
                            self.notes = self.keys.clone();
                            self.transpose = 0;
                        }
                    }
                    3 => self.notes.push(h),
                    4 => {
                        if self.notes.is_empty() {
                            self.notes.push(h);
                        } else {
                            self.temp.push(h);
                        }
                    }
                    _ => {
                        // Through: the first keys latch, later ones play along unprocessed
                        let building = !none_down && self.keys[..self.keys.len() - 1].iter().all(|k| self.notes.contains(k));
                        if self.notes.is_empty() || building {
                            if none_down {
                                self.notes.clear();
                            }
                            self.notes.push(h);
                        } else {
                            out.push(Ev::new(at, Msg::On { ch: h.ch, pitch: h.pitch, vel: h.vel }));
                            self.thru.push((h.pitch, h.ch));
                        }
                    }
                }
            }
            if self.next.is_none() && self.running(ctx) && self.has_notes() {
                self.start(at);
            }
        }

        fn key_off(&mut self, pitch: u8, at: f64, out: &mut Vec<Ev>) {
            if let Some(k) = self.keys.iter().position(|h| h.pitch == pitch) {
                self.keys.remove(k);
            }
            if let Some(k) = self.thru.iter().position(|t| t.0 == pitch) {
                let (p, ch) = self.thru.remove(k);
                out.push(Ev::new(at, Msg::Off { ch, pitch: p }));
                return;
            }
            if !on(&self.v, LATCH as usize) {
                if let Some(k) = self.notes.iter().position(|h| h.pitch == pitch) {
                    self.notes.remove(k);
                }
            } else if let Some(k) = self.temp.iter().position(|h| h.pitch == pitch) {
                self.temp.remove(k);
            }
            if !self.has_notes() {
                self.next = None;
                self.played = 0;
                self.step = 0;
                self.bag.clear();
            }
        }

        /// The first step: on the next Input Snap grid line (or right away).
        fn start(&mut self, at: f64) {
            let q = match ix(&self.v, SNAP as usize) {
                0 => self.step_q(),
                1 => 0.0,
                k => [4.0, 2.0, 1.0, 0.5, 0.25][(k - 2).min(4)],
            } * PPQ;
            let t = if q > 0.0 { ((at - 1e-6) / q).ceil() * q } else { at };
            self.next = Some(t.max(at));
            self.step = 0;
            self.played = 0;
        }

        fn play_step(&mut self, t: f64, out: &mut Vec<Ev>) {
            let step_t = self.step_q() * PPQ;
            let grid = self.grid();
            let gs = (self.step as usize) % self.grid_len();
            let (son, svel, slen, stie, schord) = if grid { self.grid_step(gs) } else { (true, 0.0, 100.0, false, false) };
            self.status.step = Some(gs);
            let n = self.pool().len();
            let cycle = (self.played as usize) / self.cycle_len(n).max(1);
            self.status.cycle = cycle as u32;
            // swing: every second step later
            let swing = if self.step % 2 == 1 { self.p(SWING) as f64 / 100.0 * step_t * 0.5 } else { 0.0 };
            let at = t + swing;
            if son && stie {
                // a tie: the note before goes on sounding through this step
                if let Some((p, _)) = self.last_tie {
                    for s in self.sounding.iter_mut().filter(|s| s.0 == p) {
                        s.2 += step_t;
                    }
                }
            } else if son {
                let notes = self.pick(schord);
                self.played += 1;
                let len_pct = (self.p(LENGTH) / 100.0) as f64 * (slen as f64 / 100.0);
                let rnd = self.p(LENGTH_RANDOM) as f64 / 100.0;
                for h in notes {
                    let mut len = step_t * len_pct;
                    if rnd > 0.0 {
                        len *= 1.0 + (self.rng.f() * 2.0 - 1.0) * rnd * 0.5;
                    }
                    let vel = if grid { svel.round().clamp(1.0, 127.0) as u8 } else { self.velocity(h.vel, cycle) };
                    // (a note still sounding at that pitch stops first)
                    if let Some(k) = self.sounding.iter().position(|s| s.0 == h.pitch) {
                        let (p, ch, _) = self.sounding.remove(k);
                        out.push(Ev::new(at, Msg::Off { ch, pitch: p }));
                    }
                    out.push(Ev::new(at, Msg::On { ch: h.ch, pitch: h.pitch, vel }));
                    self.sounding.push((h.pitch, h.ch, at + len.max(1.0)));
                    self.last_tie = Some((h.pitch, h.ch));
                }
            }
            self.step += 1;
            self.next = Some(t + step_t);
        }

        fn offs_until(&mut self, to: f64, out: &mut Vec<Ev>) {
            let mut k = 0;
            while k < self.sounding.len() {
                if self.sounding[k].2 < to {
                    let (p, ch, t) = self.sounding.remove(k);
                    out.push(Ev::new(t, Msg::Off { ch, pitch: p }));
                } else {
                    k += 1;
                }
            }
        }

        fn controller(&mut self, m: &Msg) -> bool {
            let mut used = false;
            for k in 0..4 {
                let code = ix(&self.v, (CTL + 2 * k) as usize) as i32;
                let dest = ix(&self.v, (CTL + 2 * k + 1) as usize);
                if dest == 0 {
                    continue;
                }
                if let Some(x) = code_value(m, code) {
                    let f = x / 127.0;
                    let (id, val) = match dest {
                        1 => (LENGTH, 1.0 + f * 149.0),
                        2 => (LENGTH_RANDOM, f * 100.0),
                        3 => (VELOCITY, f * 100.0),
                        4 => (VEL_BASE, 1.0 + f * 126.0),
                        _ => (VEL_AMOUNT, f * 200.0 - 100.0),
                    };
                    self.v[id as usize] = val;
                    used = true;
                }
            }
            used
        }
    }

    impl MidiFx for Arpeggiator {
        fn load(&mut self, values: &[f32]) {
            let was_latch = on(&self.v, LATCH as usize);
            let n = self.v.len().min(values.len());
            self.v[..n].copy_from_slice(&values[..n]);
            if was_latch && !on(&self.v, LATCH as usize) {
                // latch off: only the keys down play on
                self.notes = self.keys.clone();
                self.temp.clear();
                self.transpose = 0;
            }
        }

        fn process(&mut self, input: &[Ev], ctx: &Ctx, out: &mut Vec<Ev>) {
            if ctx.playing && !self.was_playing {
                self.free = false;
            }
            self.was_playing = ctx.playing;
            let run = self.running(ctx);
            let mut evs = input.iter().peekable();
            loop {
                let next_in = evs.peek().map(|e| e.tick);
                let next_step = self.next.filter(|&t| t < ctx.to && run);
                let take_input = match (next_in, next_step) {
                    (Some(a), Some(b)) => a <= b,
                    (Some(_), None) => true,
                    (None, Some(_)) => false,
                    (None, None) => break,
                };
                if take_input {
                    let ev = *evs.next().unwrap();
                    match ev.msg {
                        Msg::On { ch, pitch, vel } if vel > 0 => {
                            self.status.input.push(pitch);
                            if !run || !self.in_zone(pitch) {
                                out.push(ev);
                                self.thru.push((pitch, ch));
                            } else {
                                self.key_on(Held { pitch, vel, ch }, ev.tick, ctx, out);
                            }
                        }
                        Msg::On { ch, pitch, .. } | Msg::Off { ch, pitch } => {
                            self.status.input.retain(|&p| p != pitch);
                            if let Some(k) = self.thru.iter().position(|t| t.0 == pitch) {
                                self.thru.remove(k);
                                if let Some(j) = self.keys.iter().position(|h| h.pitch == pitch) {
                                    self.keys.remove(j);
                                }
                                out.push(Ev::new(ev.tick, Msg::Off { ch, pitch }));
                            } else {
                                self.key_off(pitch, ev.tick, out);
                            }
                        }
                        ref m => {
                            if !self.controller(m) {
                                out.push(ev);
                            }
                        }
                    }
                } else if let Some(t) = next_step {
                    self.offs_until(t + 1e-6, out);
                    if self.has_notes() {
                        self.play_step(t, out);
                    } else {
                        self.next = None;
                    }
                }
            }
            if !run && self.next.is_some() {
                self.next = None;
            }
            if run && self.next.is_none() && self.has_notes() {
                self.start(ctx.from);
            }
            self.offs_until(ctx.to, out);
            self.status.output = self.sounding.iter().map(|s| s.0).collect();
            self.status.free = self.free;
        }

        fn reset(&mut self, out: &mut Vec<Ev>) {
            for (p, ch, _) in self.sounding.drain(..) {
                out.push(Ev::new(0.0, Msg::Off { ch, pitch: p }));
            }
            for (p, ch) in self.thru.drain(..) {
                out.push(Ev::new(0.0, Msg::Off { ch, pitch: p }));
            }
            self.keys.clear();
            if !on(&self.v, LATCH as usize) {
                self.notes.clear();
            }
            self.temp.clear();
            self.next = None;
            self.free = false;
            self.status = Status::default();
        }

        fn status(&self) -> Status {
            self.status.clone()
        }

        fn command(&mut self, name: &str) {
            match name {
                // Clear: latch memory emptied, the arpeggio stops, positions back to zero
                "Clear" => {
                    self.notes.clear();
                    self.temp.clear();
                    self.transpose = 0;
                    self.next = None;
                    self.step = 0;
                    self.played = 0;
                }
                // Play clicked while the song is stopped: run from the first step
                "Play" => {
                    self.free = true;
                    self.step = 0;
                    self.played = 0;
                }
                "Delete Last" => {
                    self.notes.pop();
                }
                "Stop" => {
                    self.free = false;
                    self.next = None;
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Chord Trigger

pub mod chord {
    use super::*;

    pub const MODE: u32 = 0;
    pub const LO: u32 = 1;
    pub const HI: u32 = 2;
    pub const TRANSPOSE: u32 = 3;
    pub const REMOTE: u32 = 4;
    /// Chords: 128 trigger keys × 12 notes (-1: none); Single mode keeps its one chord at
    /// slot 128 with its trigger key in the first float.
    pub const DATA_AT: usize = 8;
    pub const NOTES: usize = 12;
    pub const DATA: usize = 129 * (NOTES + 1);

    /// A key's chord (trigger key, notes) as stored.
    pub fn chord_of(v: &[f32], slot: usize) -> (i32, Vec<u8>) {
        let b = DATA_AT + slot * (NOTES + 1);
        let key = fv(v, b).round() as i32;
        let notes = (1..=NOTES).map(|i| fv(v, b + i)).filter(|&x| x >= 0.0).map(|x| x.round() as u8).collect();
        (key, notes)
    }

    pub fn set_chord(v: &mut Vec<f32>, slot: usize, key: i32, notes: &[u8]) {
        let b = DATA_AT + slot * (NOTES + 1);
        if v.len() < b + NOTES + 1 {
            v.resize(b + NOTES + 1, -1.0);
        }
        v[b] = if notes.is_empty() { -1.0 } else { key as f32 };
        for i in 0..NOTES {
            v[b + 1 + i] = notes.get(i).map_or(-1.0, |&n| n as f32);
        }
    }

    pub struct ChordTrigger {
        v: Vec<f32>,
        playing: Vec<(u8, Vec<(u8, u8)>)>,
        thru: Vec<(u8, u8)>,
        status: Status,
    }

    impl ChordTrigger {
        pub fn new(v: Vec<f32>) -> ChordTrigger {
            ChordTrigger { v, playing: Vec::new(), thru: Vec::new(), status: Status::default() }
        }

        /// The notes a trigger key plays.
        pub fn notes_for(v: &[f32], key: u8) -> Vec<u8> {
            let tr = fv(v, TRANSPOSE as usize).round() as i32;
            let (notes, shift) = if ix(v, MODE as usize) == 0 {
                let (k, n) = chord_of(v, 128);
                (n, key as i32 - k)
            } else {
                (chord_of(v, key as usize).1, 0)
            };
            let mut out: Vec<u8> = notes.into_iter().map(|n| (n as i32 + shift + tr).clamp(0, 127) as u8).collect();
            out.sort();
            out.dedup();
            out
        }
    }

    impl MidiFx for ChordTrigger {
        fn load(&mut self, values: &[f32]) {
            let n = self.v.len().min(values.len());
            self.v[..n].copy_from_slice(&values[..n]);
        }

        fn process(&mut self, input: &[Ev], _ctx: &Ctx, out: &mut Vec<Ev>) {
            let (lo, hi) = (fv(&self.v, LO as usize), fv(&self.v, HI as usize));
            for ev in input {
                match ev.msg {
                    Msg::On { ch, pitch, vel } if vel > 0 => {
                        self.status.input.push(pitch);
                        if !in_range(pitch as f32, lo.min(hi), lo.max(hi)) {
                            self.thru.push((pitch, ch));
                            out.push(*ev);
                            continue;
                        }
                        let notes = Self::notes_for(&self.v, pitch);
                        for &n in &notes {
                            out.push(Ev::new(ev.tick, Msg::On { ch, pitch: n, vel }));
                        }
                        self.status.output.extend(notes.iter().copied());
                        self.playing.push((pitch, notes.into_iter().map(|n| (n, ch)).collect()));
                    }
                    Msg::On { pitch, .. } | Msg::Off { pitch, .. } => {
                        self.status.input.retain(|&p| p != pitch);
                        if let Some(k) = self.thru.iter().position(|t| t.0 == pitch) {
                            let (p, ch) = self.thru.remove(k);
                            out.push(Ev::new(ev.tick, Msg::Off { ch, pitch: p }));
                        } else if let Some(k) = self.playing.iter().position(|t| t.0 == pitch) {
                            let (_, notes) = self.playing.remove(k);
                            for (n, ch) in notes {
                                out.push(Ev::new(ev.tick, Msg::Off { ch, pitch: n }));
                                if let Some(j) = self.status.output.iter().position(|&o| o == n) {
                                    self.status.output.remove(j);
                                }
                            }
                        }
                    }
                    _ => out.push(*ev),
                }
            }
        }

        fn reset(&mut self, out: &mut Vec<Ev>) {
            for (_, notes) in self.playing.drain(..) {
                for (n, ch) in notes {
                    out.push(Ev::new(0.0, Msg::Off { ch, pitch: n }));
                }
            }
            for (p, ch) in self.thru.drain(..) {
                out.push(Ev::new(0.0, Msg::Off { ch, pitch: p }));
            }
            self.status = Status::default();
        }

        fn status(&self) -> Status {
            self.status.clone()
        }
    }
}

// ---------------------------------------------------------------------------------------
// Modifier: one controller re-assigned, scaled, offset (or filtered out)

struct Modifier {
    v: Vec<f32>,
}

impl MidiFx for Modifier {
    fn load(&mut self, values: &[f32]) {
        let n = self.v.len().min(values.len());
        self.v[..n].copy_from_slice(&values[..n]);
    }

    fn process(&mut self, input: &[Ev], _ctx: &Ctx, out: &mut Vec<Ev>) {
        let thru = on(&self.v, 0);
        let input_code = ix(&self.v, 1) as i32;
        let to = ix(&self.v, 2) as i32 - 1; // -1: Off
        let (scale, add) = (fv(&self.v, 3) / 100.0, fv(&self.v, 4));
        for ev in input {
            let Some(x) = code_value(&ev.msg, input_code) else {
                out.push(*ev);
                continue;
            };
            if thru {
                out.push(*ev);
            }
            if to >= 0 {
                if let Some(m) = code_msg(to, ev.msg.ch(), x * scale + add) {
                    out.push(Ev::new(ev.tick, m));
                }
            }
        }
    }

    fn reset(&mut self, _out: &mut Vec<Ev>) {}
}

// ---------------------------------------------------------------------------------------
// Modulator: an LFO and a Delay/Attack/Hold/Release envelope making controller streams

pub mod modu {
    use super::*;

    pub const LFO_ON: u32 = 0;
    pub const WAVE: u32 = 1;
    pub const SYMMETRY: u32 = 2;
    pub const SIMILARITY: u32 = 3;
    pub const LFO_TRIGGER: u32 = 4;
    pub const LFO_STEPS: u32 = 5;
    pub const SYNC: u32 = 6;
    pub const RATE_HZ: u32 = 7;
    pub const RATE_SYNC: u32 = 8;
    pub const LFO_TO: u32 = 9;
    pub const LFO_LEVEL: u32 = 10;
    pub const LFO_OFFSET: u32 = 11;
    pub const LFO_CH: u32 = 12;
    pub const ENV_ON: u32 = 13;
    pub const DELAY: u32 = 14;
    pub const ATTACK: u32 = 15;
    pub const HOLD: u32 = 16;
    pub const RELEASE: u32 = 17;
    pub const ENV_TRIGGER: u32 = 18;
    pub const ENV_STEPS: u32 = 19;
    pub const ENV_TO_RATE: u32 = 20;
    pub const ENV_TO_AMP: u32 = 21;
    pub const ENV_TO: u32 = 22;
    pub const ENV_LEVEL: u32 = 23;
    pub const ENV_OFFSET: u32 = 24;
    pub const ENV_CH: u32 = 25;

    /// The LFO's shape at phase 0..1 (-1..1); symmetry -1..1.
    pub fn wave(shape: usize, phase: f64, sym: f64) -> f64 {
        let ph = phase.rem_euclid(1.0);
        match shape {
            // triangle; symmetry moves the peak toward a saw
            0 => {
                let peak = (0.5 + sym * 0.5).clamp(0.001, 0.999);
                if ph < peak { -1.0 + 2.0 * ph / peak } else { 1.0 - 2.0 * (ph - peak) / (1.0 - peak) }
            }
            // sine leaning left / right
            1 => {
                let peak = (0.25 + sym * 0.2).clamp(0.05, 0.45);
                let warped = if ph < peak * 2.0 { ph / (peak * 2.0) * 0.5 } else { 0.5 + (ph - peak * 2.0) / (1.0 - peak * 2.0) * 0.5 };
                (warped * std::f64::consts::TAU).sin()
            }
            // square, symmetry = pulse width
            2 => if ph < (0.5 + sym * 0.49) { 1.0 } else { -1.0 },
            _ => 0.0,
        }
    }

    pub struct Modulator {
        v: Vec<f32>,
        phase: f64,
        rnd: f64,
        rnd_from: f64,
        /// Envelope time since its trigger (s), None: idle.
        env_t: Option<f64>,
        held: usize,
        last: (Option<i32>, Option<i32>),
        rng: Rng,
        status: Status,
    }

    impl Modulator {
        pub fn new(v: Vec<f32>) -> Modulator {
            Modulator { v, phase: 0.0, rnd: 0.0, rnd_from: 0.0, env_t: None, held: 0, last: (None, None), rng: Rng::new(time_seed()), status: Status::default() }
        }

        fn p(&self, id: u32) -> f64 {
            fv(&self.v, id as usize) as f64
        }

        /// The envelope level 0..1 at time `t` after its trigger.
        pub fn env_at(&self, t: f64) -> f64 {
            let (d, a, h, r) = (self.p(DELAY), self.p(ATTACK), self.p(HOLD), self.p(RELEASE));
            if t < d {
                0.0
            } else if t < d + a {
                (t - d) / a.max(1e-6)
            } else if t < d + a + h {
                1.0
            } else if t < d + a + h + r {
                1.0 - (t - d - a - h) / r.max(1e-6)
            } else {
                0.0
            }
        }

        fn stepped(x: f64, steps: f64) -> f64 {
            if steps >= 1.0 { (x * steps).round() / steps } else { x }
        }

        fn trigger(&mut self) {
            let first = self.held == 0;
            self.held += 1;
            match ix(&self.v, LFO_TRIGGER as usize) {
                1 if first => self.phase = 0.0,
                2 => self.phase = 0.0,
                _ => {}
            }
            match ix(&self.v, ENV_TRIGGER as usize) {
                1 if first => self.env_t = Some(0.0),
                2 => self.env_t = Some(0.0),
                _ => {}
            }
        }
    }

    impl MidiFx for Modulator {
        fn load(&mut self, values: &[f32]) {
            let n = self.v.len().min(values.len());
            self.v[..n].copy_from_slice(&values[..n]);
        }

        fn process(&mut self, input: &[Ev], ctx: &Ctx, out: &mut Vec<Ev>) {
            for ev in input {
                match ev.msg {
                    Msg::On { vel, .. } if vel > 0 => self.trigger(),
                    Msg::On { .. } | Msg::Off { .. } => self.held = self.held.saturating_sub(1),
                    _ => {}
                }
                out.push(*ev);
            }
            let dt = ctx.secs();
            // the envelope
            let env = match self.env_t {
                Some(t) => {
                    let e = self.env_at(t);
                    let total = self.p(DELAY) + self.p(ATTACK) + self.p(HOLD) + self.p(RELEASE);
                    self.env_t = if t + dt > total { None } else { Some(t + dt) };
                    e
                }
                None => 0.0,
            };
            let env_on = on(&self.v, ENV_ON as usize);
            let env_mod = if env_on { env } else { 0.0 };
            // the LFO
            let hz = if on(&self.v, SYNC as usize) {
                let q = SYNC_NOTES[ix(&self.v, RATE_SYNC as usize).min(SYNC_NOTES.len() - 1)].1;
                ctx.bpm / 60.0 / q
            } else {
                self.p(RATE_HZ)
            };
            let hz = hz * (1.0 + self.p(ENV_TO_RATE) / 100.0 * env_mod).max(0.0);
            let before = self.phase;
            self.phase += hz * dt;
            let wrapped = self.phase.floor() > before.floor();
            let shape = ix(&self.v, WAVE as usize);
            if shape == 3 && wrapped || shape == 3 && before == 0.0 && self.rnd == 0.0 {
                let sim = self.p(SIMILARITY) / 100.0;
                self.rnd_from = self.rnd;
                let target = self.rng.f() * 2.0 - 1.0;
                self.rnd = (self.rnd + (target - self.rnd) * (1.0 - sim * 0.9)).clamp(-1.0, 1.0);
            }
            // (Envelope trigger LFO: at each positive peak)
            if env_on && ix(&self.v, ENV_TRIGGER as usize) == 0 && wrapped && self.env_t.is_none() {
                self.env_t = Some(0.0);
            }
            let steps = self.p(LFO_STEPS);
            let mut lfo = if shape == 3 {
                let smooth = (steps / 128.0).clamp(0.0, 1.0);
                let f = self.phase.fract();
                self.rnd_from + (self.rnd - self.rnd_from) * (f / smooth.max(1e-6)).min(1.0)
            } else {
                let ph = if steps >= 1.0 && shape < 2 { (self.phase.fract() * steps).floor() / steps } else { self.phase };
                wave(shape, ph, self.p(SYMMETRY) / 100.0)
            };
            if self.p(ENV_TO_AMP) != 0.0 {
                // the envelope fades the LFO in (positive) or out (negative)
                let a = self.p(ENV_TO_AMP) / 100.0;
                lfo *= if a > 0.0 { 1.0 - a + a * env_mod } else { 1.0 + a * env_mod };
            }
            self.status.lfo = lfo as f32;
            self.status.env = env as f32;
            if on(&self.v, LFO_ON as usize) {
                let x = lfo * self.p(LFO_LEVEL) / 100.0 + self.p(LFO_OFFSET) / 100.0;
                let code = ix(&self.v, LFO_TO as usize) as i32;
                let ch = (self.p(LFO_CH).round() as i32 - 1).clamp(0, 15) as u8;
                let val = 63.5 + x.clamp(-1.0, 1.0) * 63.5;
                let key = (val * if code == 129 { 128.0 } else { 1.0 }).round() as i32;
                if self.last.0 != Some(key) {
                    self.last.0 = Some(key);
                    if let Some(m) = code_msg(code, ch, val as f32) {
                        out.push(Ev::new(ctx.from, m));
                    }
                }
            }
            if env_on {
                let e = Self::stepped(env, self.p(ENV_STEPS));
                let x = e * self.p(ENV_LEVEL) / 100.0 + self.p(ENV_OFFSET) / 100.0;
                let code = ix(&self.v, ENV_TO as usize) as i32;
                let ch = (self.p(ENV_CH).round() as i32 - 1).clamp(0, 15) as u8;
                let val = (x.clamp(0.0, 1.0) * 127.0) as f32;
                let key = (val * if code == 129 { 128.0 } else { 1.0 }).round() as i32;
                if self.last.1 != Some(key) {
                    self.last.1 = Some(key);
                    if let Some(m) = code_msg(code, ch, val) {
                        out.push(Ev::new(ctx.from, m));
                    }
                }
            }
        }

        fn reset(&mut self, _out: &mut Vec<Ev>) {
            self.held = 0;
            self.env_t = None;
            self.last = (None, None);
        }

        fn status(&self) -> Status {
            self.status.clone()
        }
    }
}

// ---------------------------------------------------------------------------------------
// Note Repeater: repeats of each note, like a delay

struct NoteRepeater {
    v: Vec<f32>,
    queue: Vec<Ev>,
    /// Notes whose repeats are scheduled: (input pitch, its on tick, the repeats' (tick, pitch)).
    starts: Vec<(u8, f64, Vec<(f64, u8, u8)>)>,
}

impl NoteRepeater {
    fn delay(&self, ctx: &Ctx) -> f64 {
        if on(&self.v, 1) {
            SYNC_NOTES[ix(&self.v, 3).min(SYNC_NOTES.len() - 1)].1 * PPQ
        } else {
            ctx.ms_to_ticks(fv(&self.v, 2) as f64)
        }
    }
}

impl MidiFx for NoteRepeater {
    fn load(&mut self, values: &[f32]) {
        let n = self.v.len().min(values.len());
        self.v[..n].copy_from_slice(&values[..n]);
    }

    fn process(&mut self, input: &[Ev], ctx: &Ctx, out: &mut Vec<Ev>) {
        let thru = on(&self.v, 0);
        let repeats = fv(&self.v, 4).round().max(1.0) as usize;
        let (tr, ramp) = (fv(&self.v, 5).round() as i32, fv(&self.v, 6) / 100.0);
        let (lo, hi) = (fv(&self.v, 7), fv(&self.v, 8));
        let d = self.delay(ctx).max(1.0);
        for ev in input {
            match ev.msg {
                Msg::On { ch, pitch, vel } if vel > 0 && in_range(pitch as f32, lo, hi) => {
                    if thru {
                        out.push(*ev);
                    }
                    let mut reps = Vec::new();
                    for i in 1..=repeats {
                        let p = (pitch as i32 + tr * i as i32).clamp(0, 127) as u8;
                        let v = (vel as f32 * ramp.powi(i as i32)).round().clamp(1.0, 127.0) as u8;
                        let t = ev.tick + d * i as f64;
                        self.queue.push(Ev::new(t, Msg::On { ch, pitch: p, vel: v }));
                        reps.push((t, p, ch));
                    }
                    self.starts.push((pitch, ev.tick, reps));
                }
                Msg::On { pitch, ch, .. } | Msg::Off { pitch, ch } => {
                    if let Some(k) = self.starts.iter().position(|s| s.0 == pitch) {
                        let (_, on_t, reps) = self.starts.remove(k);
                        if thru {
                            out.push(Ev::new(ev.tick, Msg::Off { ch, pitch }));
                        }
                        // each repeat as long as the note played (at most up to the next one)
                        let len = (ev.tick - on_t).clamp(1.0, d * 0.95);
                        for (t, p, c) in reps {
                            self.queue.push(Ev::new(t + len, Msg::Off { ch: c, pitch: p }));
                        }
                    } else {
                        out.push(*ev);
                    }
                }
                _ => out.push(*ev),
            }
        }
        let mut k = 0;
        while k < self.queue.len() {
            if self.queue[k].tick < ctx.to {
                let mut e = self.queue.remove(k);
                e.tick = e.tick.max(ctx.from);
                out.push(e);
            } else {
                k += 1;
            }
        }
    }

    fn reset(&mut self, out: &mut Vec<Ev>) {
        for e in self.queue.drain(..) {
            if let Msg::Off { .. } = e.msg {
                out.push(Ev::new(0.0, e.msg));
            }
        }
        for (_, _, reps) in self.starts.drain(..) {
            for (_, p, ch) in reps {
                out.push(Ev::new(0.0, Msg::Off { ch, pitch: p }));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Randomizer

struct Randomizer {
    v: Vec<f32>,
    rng: Rng,
    /// Notes whose pitch was changed: input → output.
    map: Vec<(u8, u8)>,
}

impl Randomizer {
    fn new(v: Vec<f32>) -> Randomizer {
        let seed = fv(&v, 6).round() as u64;
        Randomizer { v, rng: Rng::new(if seed == 0 { time_seed() } else { seed }), map: Vec::new() }
    }

    /// A random offset: -amount..amount, the weight leaning it low (-) or high (+).
    fn offset(&mut self) -> f32 {
        let amount = fv(&self.v, 3);
        let w = fv(&self.v, 4) / 100.0;
        let mut r = self.rng.f() as f32 * 2.0 - 1.0;
        if w != 0.0 {
            let e = 1.0 + w.abs() * 3.0;
            let u = self.rng.f() as f32;
            let biased = u.powf(e); // toward 0
            r = if w > 0.0 { 1.0 - 2.0 * biased } else { -1.0 + 2.0 * biased };
        }
        r * amount + fv(&self.v, 5)
    }
}

impl MidiFx for Randomizer {
    fn load(&mut self, values: &[f32]) {
        let reseed = fv(values, 6) != fv(&self.v, 6);
        let n = self.v.len().min(values.len());
        self.v[..n].copy_from_slice(&values[..n]);
        if reseed {
            self.reset(&mut Vec::new());
        }
    }

    fn process(&mut self, input: &[Ev], _ctx: &Ctx, out: &mut Vec<Ev>) {
        let kind = ix(&self.v, 0);
        let (lo, hi) = (fv(&self.v, 1), fv(&self.v, 2));
        for ev in input {
            let mut e = *ev;
            match (kind, ev.msg) {
                (0, Msg::On { ch, pitch, vel }) if vel > 0 && in_range(pitch as f32, lo, hi) => {
                    let p = (pitch as f32 + self.offset()).round().clamp(0.0, 127.0) as u8;
                    self.map.push((pitch, p));
                    e.msg = Msg::On { ch, pitch: p, vel };
                }
                (0, Msg::Off { ch, pitch }) | (0, Msg::On { ch, pitch, .. }) => {
                    if let Some(k) = self.map.iter().position(|m| m.0 == pitch) {
                        e.msg = Msg::Off { ch, pitch: self.map.remove(k).1 };
                    }
                }
                (1, Msg::On { ch, pitch, vel }) if vel > 0 && in_range(vel as f32, lo, hi) => {
                    e.msg = Msg::On { ch, pitch, vel: (vel as f32 + self.offset()).round().clamp(1.0, 127.0) as u8 };
                }
                (k, m) if k >= 2 => {
                    let code = k as i32 - 2;
                    if let Some(x) = code_value(&m, code) {
                        if in_range(x, lo, hi) {
                            if let Some(nm) = code_msg(code, m.ch(), x + self.offset()) {
                                e.msg = nm;
                            }
                        }
                    }
                }
                _ => {}
            }
            out.push(e);
        }
    }

    fn reset(&mut self, _out: &mut Vec<Ev>) {
        let seed = fv(&self.v, 6).round() as u64;
        if seed > 0 {
            self.rng = Rng::new(seed);
        }
    }
}

// ---------------------------------------------------------------------------------------
// Transposer

struct Transposer {
    v: Vec<f32>,
    map: [Option<u8>; 128],
}

impl Transposer {
    pub fn out_pitch(v: &[f32], pitch: u8) -> u8 {
        let p = (pitch as i32 + fv(v, 0).round() as i32).clamp(0, 127) as u8;
        let scale = ix(v, 2);
        let root = ix(v, 1) as u8;
        let mask = if scale == 17 {
            let mut m = [false; 12];
            for (i, x) in m.iter_mut().enumerate() {
                *x = fv(v, 3 + i) >= 0.5;
            }
            m
        } else {
            scale_mask(scale)
        };
        snap_to_scale(p, root, &mask)
    }
}

/// The note the Transposer plays for `pitch` with values `v`.
pub fn transposer_pitch(v: &[f32], pitch: u8) -> u8 {
    Transposer::out_pitch(v, pitch)
}

impl MidiFx for Transposer {
    fn load(&mut self, values: &[f32]) {
        let n = self.v.len().min(values.len());
        self.v[..n].copy_from_slice(&values[..n]);
    }

    fn process(&mut self, input: &[Ev], _ctx: &Ctx, out: &mut Vec<Ev>) {
        for ev in input {
            let mut e = *ev;
            match ev.msg {
                Msg::On { ch, pitch, vel } if vel > 0 => {
                    let p = Self::out_pitch(&self.v, pitch);
                    self.map[pitch as usize] = Some(p);
                    e.msg = Msg::On { ch, pitch: p, vel };
                }
                Msg::On { ch, pitch, .. } | Msg::Off { ch, pitch } => {
                    let p = self.map[pitch as usize].take().unwrap_or_else(|| Self::out_pitch(&self.v, pitch));
                    e.msg = Msg::Off { ch, pitch: p };
                }
                Msg::PolyAt { ch, pitch, val } => e.msg = Msg::PolyAt { ch, pitch: self.map[pitch as usize].unwrap_or(pitch), val },
                _ => {}
            }
            out.push(e);
        }
    }

    fn reset(&mut self, out: &mut Vec<Ev>) {
        for (i, m) in self.map.iter_mut().enumerate() {
            if let Some(p) = m.take() {
                let _ = i;
                out.push(Ev::new(0.0, Msg::Off { ch: 0, pitch: p }));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// Velocity Processor

pub mod vel {
    pub const PROCESS: u32 = 0;
    pub const MODE: u32 = 1;
    pub const THRESHOLD: u32 = 2;
    pub const RATIO: u32 = 3;
    pub const MAKEUP: u32 = 4;
    pub const AUTO: u32 = 5;
    pub const VALUE_RANGE: u32 = 6;
    pub const VALUE: u32 = 7;
    pub const RANGE_MIN: u32 = 8;
    pub const RANGE_MAX: u32 = 9;
    pub const SCALE: u32 = 10;
    pub const ADD: u32 = 11;
    pub const NOTE_MIN: u32 = 12;
    pub const NOTE_MAX: u32 = 13;
}

struct VelocityProcessor {
    v: Vec<f32>,
}

/// A velocity through the Velocity Processor with values `v`.
pub fn velocity_out(v: &[f32], vel: u8) -> u8 {
    use vel::*;
    let x = vel as f32;
    let out = match ix(v, MODE as usize) {
        0 => {
            let thr = fv(v, THRESHOLD as usize);
            let ratio = fv(v, RATIO as usize).max(0.01);
            let curve = |x: f32| -> f32 {
                // soft knee: 6 velocity steps either side of the threshold
                let knee = 6.0;
                if x <= thr - knee {
                    x
                } else if x >= thr + knee {
                    thr + (x - thr) / ratio
                } else {
                    let d = x - thr + knee;
                    x + (1.0 / ratio - 1.0) * d * d / (4.0 * knee)
                }
            };
            let y = curve(x);
            if on(v, AUTO as usize) {
                // the loudest input lands on the Make-up reference
                let top = curve(127.0).max(1.0);
                let reference = if fv(v, MAKEUP as usize) > 0.0 { fv(v, MAKEUP as usize) } else { 127.0 };
                y * reference / top
            } else {
                y + fv(v, MAKEUP as usize)
            }
        }
        1 => {
            if ix(v, VALUE_RANGE as usize) == 0 {
                fv(v, VALUE as usize)
            } else {
                let (a, b) = (fv(v, RANGE_MIN as usize), fv(v, RANGE_MAX as usize));
                x.clamp(a.min(b), a.max(b))
            }
        }
        _ => x * fv(v, SCALE as usize) / 100.0 + fv(v, ADD as usize),
    };
    out.round().clamp(1.0, 127.0) as u8
}

impl MidiFx for VelocityProcessor {
    fn load(&mut self, values: &[f32]) {
        let n = self.v.len().min(values.len());
        self.v[..n].copy_from_slice(&values[..n]);
    }

    fn process(&mut self, input: &[Ev], _ctx: &Ctx, out: &mut Vec<Ev>) {
        use vel::*;
        let process = ix(&self.v, PROCESS as usize);
        let (lo, hi) = (fv(&self.v, NOTE_MIN as usize), fv(&self.v, NOTE_MAX as usize));
        for ev in input {
            let mut e = *ev;
            if let Msg::On { ch, pitch, vel } = ev.msg {
                // (note-off velocities aren't carried by Studio's events: Note Off mode leaves
                // note-ons alone)
                if vel > 0 && process != 1 && in_range(pitch as f32, lo, hi) {
                    e.msg = Msg::On { ch, pitch, vel: velocity_out(&self.v, vel) };
                }
            }
            out.push(e);
        }
    }

    fn reset(&mut self, _out: &mut Vec<Ev>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(from: f64, to: f64) -> Ctx {
        Ctx { from, to, bpm: 120.0, playing: true }
    }

    fn on_ev(t: f64, p: u8, v: u8) -> Ev {
        Ev::new(t, Msg::On { ch: 0, pitch: p, vel: v })
    }

    fn ons(evs: &[Ev]) -> Vec<(u32, u8)> {
        evs.iter().filter_map(|e| if let Msg::On { pitch, .. } = e.msg { Some((e.tick.round() as u32, pitch)) } else { None }).collect()
    }

    #[test]
    fn orders_follow_logics_variations() {
        use arp::{order, Order};
        assert_eq!(order(Order::Up, 0, 4), vec![0, 1, 2, 3]);
        assert_eq!(order(Order::Up, 1, 4), vec![1, 0, 2, 3]);
        assert_eq!(order(Order::Up, 3, 4), vec![0, 1, 2, 1, 2, 3]);
        assert_eq!(order(Order::Down, 0, 3), vec![2, 1, 0]);
        assert_eq!(order(Order::UpDown, 0, 3), vec![0, 1, 2, 2, 1, 0]);
        assert_eq!(order(Order::UpDown, 1, 4), vec![0, 1, 2, 3, 2, 1]);
        assert_eq!(order(Order::UpDown, 2, 4), vec![1, 0, 3, 2, 2, 3, 0, 1]);
        assert_eq!(order(Order::OutsideIn, 0, 4), vec![3, 0, 2, 1]);
        assert_eq!(order(Order::OutsideIn, 1, 4), vec![0, 3, 1, 2]);
        // inside out: 4 3 5 2 6 1 and 3 4 2 5 1 6 (1-based)
        assert_eq!(order(Order::OutsideIn, 2, 6), vec![3, 2, 4, 1, 5, 0]);
        assert_eq!(order(Order::OutsideIn, 3, 6), vec![2, 3, 1, 4, 0, 5]);
        assert_eq!(order(Order::AsPlayed, 1, 3), vec![2, 1, 0]);
        assert_eq!(order(Order::AsPlayed, 3, 4), vec![0, 1, 2, 3, 2, 1]);
    }

    #[test]
    fn arpeggiator_steps_held_chord_on_the_grid() {
        let mut a = build("Arpeggiator", &[]).unwrap();
        let mut out = Vec::new();
        // C E G held at 0, 1/16 steps (240 ticks)
        a.process(&[on_ev(0.0, 60, 100), on_ev(0.0, 64, 100), on_ev(0.0, 67, 100)], &ctx(0.0, 960.0), &mut out);
        assert_eq!(ons(&out), vec![(0, 60), (240, 64), (480, 67), (720, 60)]);
        // every note stops before the next
        let offs = out.iter().filter(|e| matches!(e.msg, Msg::Off { .. })).count();
        assert!(offs >= 3);
        // keys up: the arpeggio stops
        let mut out = Vec::new();
        a.process(&[Ev::new(1000.0, Msg::Off { ch: 0, pitch: 60 }), Ev::new(1000.0, Msg::Off { ch: 0, pitch: 64 }), Ev::new(1000.0, Msg::Off { ch: 0, pitch: 67 })], &ctx(960.0, 1920.0), &mut out);
        assert!(ons(&out).len() <= 1);
    }

    #[test]
    fn arpeggiator_latch_transpose_and_octaves() {
        let mut v = defaults("Arpeggiator");
        v[arp::LATCH as usize] = 1.0;
        v[arp::RANGE as usize] = 1.0; // 2 octaves
        let mut a = build("Arpeggiator", &v).unwrap();
        let mut out = Vec::new();
        a.process(&[on_ev(0.0, 60, 100), on_ev(0.0, 64, 100), Ev::new(100.0, Msg::Off { ch: 0, pitch: 60 }), Ev::new(100.0, Msg::Off { ch: 0, pitch: 64 })], &ctx(0.0, 960.0), &mut out);
        // latched: goes on after release; second pass an octave up
        assert_eq!(ons(&out), vec![(0, 60), (240, 64), (480, 72), (720, 76)]);
        // one key transposes relative to the lowest latched note
        let mut out = Vec::new();
        a.process(&[on_ev(960.0, 62, 100)], &ctx(960.0, 1440.0), &mut out);
        assert_eq!(ons(&out), vec![(960, 62), (1200, 66)]);
    }

    #[test]
    fn arpeggiator_grid_rests_and_chords() {
        let mut v = defaults("Arpeggiator");
        v[arp::PATTERN as usize] = 1.0;
        v[arp::GRID_LEN as usize] = 4.0;
        v[arp::DATA + arp::STEP_FLOATS] = 0.0; // step 2 off
        v[arp::DATA + 3 * arp::STEP_FLOATS + 4] = 1.0; // step 4 chord
        let mut a = build("Arpeggiator", &v).unwrap();
        let mut out = Vec::new();
        a.process(&[on_ev(0.0, 60, 90), on_ev(0.0, 64, 90), on_ev(0.0, 67, 90)], &ctx(0.0, 960.0), &mut out);
        // step 2 rests and its note moves to step 3; step 4 plays the whole chord
        assert_eq!(ons(&out), vec![(0, 60), (480, 64), (720, 60), (720, 64), (720, 67)]);
    }

    #[test]
    fn arpeggiator_passes_notes_when_the_song_is_stopped() {
        let mut a = build("Arpeggiator", &[]).unwrap();
        let mut out = Vec::new();
        a.process(&[on_ev(0.0, 60, 100)], &Ctx { playing: false, ..ctx(0.0, 960.0) }, &mut out);
        assert_eq!(ons(&out), vec![(0, 60)]);
        // Play clicked while stopped: it runs
        let mut a = build("Arpeggiator", &[]).unwrap();
        a.command("Play");
        let mut out = Vec::new();
        a.process(&[on_ev(0.0, 60, 100)], &Ctx { playing: false, ..ctx(0.0, 960.0) }, &mut out);
        assert_eq!(ons(&out).len(), 4);
    }

    #[test]
    fn chord_trigger_single_and_multi() {
        let mut v = defaults("Chord Trigger");
        chord::set_chord(&mut v, 128, 60, &[60, 64, 67]);
        let mut c = build("Chord Trigger", &v).unwrap();
        let mut out = Vec::new();
        c.process(&[on_ev(0.0, 62, 100), on_ev(10.0, 80, 100)], &ctx(0.0, 960.0), &mut out);
        // D plays the chord a tone up; 80 is outside the range and passes
        assert_eq!(ons(&out), vec![(0, 62), (0, 66), (0, 69), (10, 80)]);
        v[chord::MODE as usize] = 1.0;
        chord::set_chord(&mut v, 50, 50, &[50, 53, 57]);
        let mut c = build("Chord Trigger", &v).unwrap();
        let mut out = Vec::new();
        c.process(&[on_ev(0.0, 50, 100), on_ev(0.0, 51, 100)], &ctx(0.0, 960.0), &mut out);
        // 51 has no chord: silent
        assert_eq!(ons(&out), vec![(0, 50), (0, 53), (0, 57)]);
        let mut out = Vec::new();
        c.process(&[Ev::new(5.0, Msg::Off { ch: 0, pitch: 50 })], &ctx(0.0, 960.0), &mut out);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn transposer_and_velocity_processor() {
        let mut v = defaults("Transposer");
        v[0] = 2.0;
        v[2] = 1.0; // C major
        assert_eq!(transposer_pitch(&v, 60), 62);
        assert_eq!(transposer_pitch(&v, 62), 64);
        // F# isn't in C major: 64 + 2 = 66 snaps down to F (65)
        assert_eq!(transposer_pitch(&v, 64), 65);
        let mut vp = defaults("Velocity Processor");
        vp[vel::MODE as usize] = 2.0;
        vp[vel::SCALE as usize] = 50.0;
        vp[vel::ADD as usize] = 10.0;
        assert_eq!(velocity_out(&vp, 100), 60);
        vp[vel::MODE as usize] = 0.0;
        vp[vel::THRESHOLD as usize] = 64.0;
        vp[vel::RATIO as usize] = 4.0;
        assert_eq!(velocity_out(&vp, 40), 40);
        assert_eq!(velocity_out(&vp, 124), 79);
        vp[vel::MODE as usize] = 1.0;
        vp[vel::VALUE as usize] = 90.0;
        assert_eq!(velocity_out(&vp, 3), 90);
        // crossing the note range skips the notes inside
        let mut vp = defaults("Velocity Processor");
        vp[vel::MODE as usize] = 1.0;
        vp[vel::NOTE_MIN as usize] = 70.0;
        vp[vel::NOTE_MAX as usize] = 50.0;
        let mut u = build("Velocity Processor", &vp).unwrap();
        let mut out = Vec::new();
        u.process(&[on_ev(0.0, 60, 30), on_ev(0.0, 80, 30)], &ctx(0.0, 10.0), &mut out);
        assert_eq!(out.iter().map(|e| if let Msg::On { vel, .. } = e.msg { vel } else { 0 }).collect::<Vec<_>>(), vec![30, 100]);
    }

    #[test]
    fn note_repeater_repeats_with_ramp_and_transpose() {
        let mut v = defaults("Note Repeater");
        v[3] = 10.0; // 1/8
        v[4] = 2.0;
        v[5] = 12.0;
        v[6] = 50.0;
        let mut u = build("Note Repeater", &v).unwrap();
        let mut out = Vec::new();
        u.process(&[on_ev(0.0, 60, 100), Ev::new(200.0, Msg::Off { ch: 0, pitch: 60 })], &ctx(0.0, 2000.0), &mut out);
        let got: Vec<(u32, u8, u8)> = out.iter().filter_map(|e| if let Msg::On { pitch, vel, .. } = e.msg { Some((e.tick as u32, pitch, vel)) } else { None }).collect();
        assert_eq!(got, vec![(0, 60, 100), (480, 72, 50), (960, 84, 25)]);
        assert_eq!(out.iter().filter(|e| matches!(e.msg, Msg::Off { .. })).count(), 3);
    }

    #[test]
    fn modifier_reassigns_and_filters() {
        let mut v = defaults("Modifier");
        v[1] = 1.0; // mod wheel in
        v[2] = 12.0; // to CC 11
        v[3] = 50.0;
        let mut u = build("Modifier", &v).unwrap();
        let mut out = Vec::new();
        u.process(&[Ev::new(0.0, Msg::Cc { ch: 0, num: 1, val: 100 }), Ev::new(0.0, Msg::Cc { ch: 0, num: 7, val: 3 })], &ctx(0.0, 10.0), &mut out);
        assert_eq!(out[0].msg, Msg::Cc { ch: 0, num: 11, val: 50 });
        assert_eq!(out[1].msg, Msg::Cc { ch: 0, num: 7, val: 3 });
        v[2] = 0.0; // Off: filtered
        let mut u = build("Modifier", &v).unwrap();
        let mut out = Vec::new();
        u.process(&[Ev::new(0.0, Msg::Cc { ch: 0, num: 1, val: 100 })], &ctx(0.0, 10.0), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn modulator_makes_a_controller_stream() {
        let mut v = defaults("Modulator");
        v[modu::SYNC as usize] = 0.0;
        v[modu::RATE_HZ as usize] = 1.0;
        let mut u = build("Modulator", &v).unwrap();
        let mut vals = Vec::new();
        let mut t = 0.0;
        for _ in 0..100 {
            let c = ctx(t, t + 19.2); // 10 ms at 120 BPM
            let mut out = Vec::new();
            u.process(&[], &c, &mut out);
            for e in out {
                if let Msg::Cc { num: 1, val, .. } = e.msg {
                    vals.push(val);
                }
            }
            t = c.to;
        }
        assert!(vals.iter().any(|&v| v > 110) && vals.iter().any(|&v| v < 20), "{vals:?}");
        assert_eq!(modu::wave(2, 0.25, 0.0), 1.0);
        assert_eq!(modu::wave(2, 0.75, 0.0), -1.0);
    }

    #[test]
    fn randomizer_seed_repeats_and_note_offs_follow() {
        let mut v = defaults("Randomizer");
        v[0] = 0.0; // note number
        v[3] = 12.0;
        v[6] = 5.0;
        let run = |v: &Vec<f32>| {
            let mut u = build("Randomizer", v).unwrap();
            let mut out = Vec::new();
            u.process(&[on_ev(0.0, 60, 100), Ev::new(10.0, Msg::Off { ch: 0, pitch: 60 })], &ctx(0.0, 20.0), &mut out);
            out
        };
        let a = run(&v);
        assert_eq!(a, run(&v));
        let (Msg::On { pitch: p1, .. }, Msg::Off { pitch: p2, .. }) = (a[0].msg, a[1].msg) else { panic!() };
        assert_eq!(p1, p2);
        assert!((48..=72).contains(&p1));
    }

    #[test]
    fn chain_runs_in_series_and_renders_notes() {
        let mut tr = defaults("Transposer");
        tr[0] = 12.0;
        let slots = vec![("Transposer".to_string(), tr, false), ("Velocity Processor".to_string(), { let mut v = defaults("Velocity Processor"); v[1] = 1.0; v[7] = 77.0; v }, false)];
        let notes = render_notes(&slots, &[(0, 480, 60, 100)], 0, 960, 120.0);
        assert_eq!(notes, vec![(0, 480, 72, 77)]);
        // a bypassed unit is skipped
        let mut s2 = slots.clone();
        s2[0].2 = true;
        assert_eq!(render_notes(&s2, &[(0, 480, 60, 100)], 0, 960, 120.0), vec![(0, 480, 60, 77)]);
    }
}
