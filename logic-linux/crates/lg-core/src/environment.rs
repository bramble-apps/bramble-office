//! Logic's MIDI Environment (Window ▸ Open MIDI Environment, ⌘0): objects on layers, the cables
//! between them, and MIDI flowing through them in real time. The Physical Input stands for every
//! MIDI input port, the Sequencer Input for Studio's own input (the selected track); between them
//! and beyond, faders, keyboards, monitors, transformers, arpeggiators, chord memorizers, delay
//! lines, voice limiters, channel splitters, cable switchers and instrument objects filter,
//! change and route the events, as the Logic Pro guide's Environment chapter describes.
//!
//! The model is generic: each object has a kind and named integer parameters (`params`, with
//! their ranges and defaults in `specs`), plus text values (names, port, chord and map tables).
//! That keeps the Object inspector, copying, undo and saving the same for every kind.

use std::collections::{BTreeMap, VecDeque};

use crate::events::PPQ;
use crate::transform::{Cond, Op, Rng};

/// An object's id (stable while the Environment lives; saved).
pub type Id = u32;

/// Meta events (never sent to MIDI outputs): the status byte Studio uses for them.
pub const META: u8 = 0x10;

/// A MIDI or meta event as faders and transformers see it: three bytes (status kind, channel
/// 0–15, data 1, data 2). Two-byte messages ignore `d2`; meta events carry their number in d1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ev {
    /// 0x80 note off, 0x90 note on, 0xA0 poly pressure, 0xB0 control, 0xC0 program,
    /// 0xD0 channel pressure, 0xE0 pitch bend, META.
    pub st: u8,
    pub ch: u8,
    pub d1: u8,
    pub d2: u8,
}

impl Ev {
    pub fn new(st: u8, ch: u8, d1: u8, d2: u8) -> Ev {
        Ev { st, ch: ch & 15, d1: d1 & 127, d2: d2 & 127 }
    }
    pub fn note(ch: u8, pitch: u8, vel: u8) -> Ev {
        Ev::new(0x90, ch, pitch, vel)
    }
    pub fn meta(num: u8, val: u8) -> Ev {
        Ev { st: META, ch: 0, d1: num, d2: val & 127 }
    }
    /// From raw MIDI bytes (channel messages only).
    pub fn from_bytes(b: &[u8]) -> Option<Ev> {
        let s = *b.first()?;
        if !(0x80..0xF0).contains(&s) {
            return None;
        }
        let mut e = Ev::new(s & 0xF0, s & 15, b.get(1).copied().unwrap_or(0), b.get(2).copied().unwrap_or(0));
        // (a note on of velocity 0 is a note off)
        if e.st == 0x90 && e.d2 == 0 {
            e.st = 0x80;
        }
        Some(e)
    }
    /// As raw MIDI bytes (None for meta events).
    pub fn bytes(&self) -> Option<Vec<u8>> {
        let s = (self.st & 0xF0) | (self.ch & 15);
        match self.st {
            0x80 | 0x90 | 0xA0 | 0xB0 | 0xE0 => Some(vec![s, self.d1, self.d2]),
            0xC0 | 0xD0 => Some(vec![s, self.d1]),
            _ => None,
        }
    }
    pub fn is_note_on(&self) -> bool {
        self.st == 0x90 && self.d2 > 0
    }
    pub fn is_note_off(&self) -> bool {
        self.st == 0x80 || (self.st == 0x90 && self.d2 == 0)
    }
    pub fn is_note(&self) -> bool {
        self.st == 0x80 || self.st == 0x90
    }
    /// One line as Logic's monitor shows it: type symbol, channel, data.
    pub fn describe(&self) -> String {
        let ch = self.ch + 1;
        match self.st {
            0x90 => format!("♪ {ch} {} {}", note_name(self.d1), self.d2),
            0x80 => format!("♪ {ch} {} 0", note_name(self.d1)),
            0xA0 => format!("P {ch} {} {}", note_name(self.d1), self.d2),
            0xB0 => format!("C {ch} {} {}", self.d1, self.d2),
            0xC0 => format!("Pg {ch} {}", self.d1),
            0xD0 => format!("A {ch} {}", self.d1),
            0xE0 => format!("PB {ch} {}", ((self.d2 as i32) << 7 | self.d1 as i32) - 8192),
            _ => format!("Meta {} {}", self.d1, self.d2),
        }
    }
}

/// A note's name as Logic writes it (middle C, 60, is C3).
pub fn note_name(p: u8) -> String {
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", N[p as usize % 12], p as i32 / 12 - 2)
}

/// The object types of the Environment's New menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    PhysicalInput,
    SequencerInput,
    Keyboard,
    Monitor,
    Fader,
    Transformer,
    Arpeggiator,
    ChordMemorizer,
    DelayLine,
    VoiceLimiter,
    ChannelSplitter,
    Instrument,
    MultiInstrument,
    MappedInstrument,
    TouchTracks,
    Ornament,
    MidiClick,
    MmcRecord,
    Alias,
    GmMixer,
    ChannelStrip,
    /// A macro: objects with their cabling packed into one (members carry param "mac").
    Macro,
}

impl Kind {
    pub const ALL: [Kind; 22] = [Kind::PhysicalInput, Kind::SequencerInput, Kind::Keyboard, Kind::Monitor, Kind::Fader, Kind::Transformer, Kind::Arpeggiator, Kind::ChordMemorizer, Kind::DelayLine, Kind::VoiceLimiter, Kind::ChannelSplitter, Kind::Instrument, Kind::MultiInstrument, Kind::MappedInstrument, Kind::TouchTracks, Kind::Ornament, Kind::MidiClick, Kind::MmcRecord, Kind::Alias, Kind::GmMixer, Kind::ChannelStrip, Kind::Macro];

    /// The name Logic gives a new object of the kind.
    pub fn name(&self) -> &'static str {
        match self {
            Kind::PhysicalInput => "Physical Input",
            Kind::SequencerInput => "Sequencer Input",
            Kind::Keyboard => "Keyboard",
            Kind::Monitor => "Monitor",
            Kind::Fader => "Fader",
            Kind::Transformer => "Transformer",
            Kind::Arpeggiator => "Arpeggiator",
            Kind::ChordMemorizer => "Chord Memorizer",
            Kind::DelayLine => "Delay Line",
            Kind::VoiceLimiter => "Voice Limiter",
            Kind::ChannelSplitter => "Channel Splitter",
            Kind::Instrument => "Instrument",
            Kind::MultiInstrument => "Multi Instrument",
            Kind::MappedInstrument => "Mapped Instrument",
            Kind::TouchTracks => "Touch Tracks",
            Kind::Ornament => "Ornament",
            Kind::MidiClick => "MIDI Click",
            Kind::MmcRecord => "MMC Record Buttons",
            Kind::Alias => "Alias",
            Kind::GmMixer => "GM Mixer",
            Kind::ChannelStrip => "Channel Strip",
            Kind::Macro => "Macro",
        }
    }

    fn tag(&self) -> &'static str {
        match self {
            Kind::PhysicalInput => "pi",
            Kind::SequencerInput => "si",
            Kind::Keyboard => "kb",
            Kind::Monitor => "mon",
            Kind::Fader => "fad",
            Kind::Transformer => "tr",
            Kind::Arpeggiator => "arp",
            Kind::ChordMemorizer => "cm",
            Kind::DelayLine => "dl",
            Kind::VoiceLimiter => "vl",
            Kind::ChannelSplitter => "cs",
            Kind::Instrument => "ins",
            Kind::MultiInstrument => "multi",
            Kind::MappedInstrument => "map",
            Kind::TouchTracks => "tt",
            Kind::Ornament => "orn",
            Kind::MidiClick => "click",
            Kind::MmcRecord => "mmc",
            Kind::Alias => "alias",
            Kind::GmMixer => "gm",
            Kind::ChannelStrip => "strip",
            Kind::Macro => "macro",
        }
    }

    fn from_tag(t: &str) -> Option<Kind> {
        Kind::ALL.iter().copied().find(|k| k.tag() == t)
    }

    /// The default size on the layer (Options ▸ Clean up ▸ Size by Default).
    pub fn default_size(&self) -> (f32, f32) {
        match self {
            Kind::PhysicalInput => (200.0, 92.0),
            Kind::Keyboard => (390.0, 50.0),
            Kind::Monitor => (78.0, 60.0),
            Kind::Fader => (26.0, 90.0),
            Kind::MultiInstrument => (120.0, 64.0),
            Kind::TouchTracks => (60.0, 40.0),
            Kind::Ornament => (160.0, 120.0),
            Kind::MmcRecord => (160.0, 24.0),
            Kind::GmMixer => (360.0, 220.0),
            Kind::ChannelStrip => (60.0, 220.0),
            Kind::Macro => (200.0, 140.0),
            _ => (40.0, 30.0),
        }
    }

    /// Instruments and the other objects that can be a track's destination (Assignable).
    pub fn assignable_default(&self) -> bool {
        matches!(self, Kind::Instrument | Kind::MultiInstrument | Kind::MappedInstrument | Kind::TouchTracks | Kind::GmMixer | Kind::ChannelStrip | Kind::Alias | Kind::Macro)
    }

    /// Objects that may have a direct physical MIDI output (Port).
    pub fn has_port(&self) -> bool {
        matches!(self, Kind::Instrument | Kind::MultiInstrument | Kind::MappedInstrument | Kind::TouchTracks | Kind::GmMixer | Kind::MidiClick)
    }

    /// Only one may exist (Physical Input, Sequencer Input, the MIDI click).
    pub fn unique(&self) -> bool {
        matches!(self, Kind::PhysicalInput | Kind::SequencerInput | Kind::MidiClick)
    }
}

/// How the Object inspector shows a parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Widget {
    Num(i32, i32),
    Check,
    Popup(&'static [&'static str]),
    /// A MIDI note number shown as a name.
    Note,
}

/// A parameter of a kind: key, inspector label, widget, default.
#[derive(Clone, Copy, Debug)]
pub struct Spec(pub &'static str, pub &'static str, pub Widget, pub i32);

pub const FADER_STYLES: &[&str] = &["Auto", "Vertical 1", "Vertical 2", "Vertical 3", "Vertical 4", "Vertical/Mute", "Horizontal", "Knob", "Numeric", "Button", "Text", "Vector", "Hidden"];
/// Fader definition types (Input / Output), and Switch/Meta for cable switchers, alias assigners
/// and meta faders.
pub const DEF_TYPES: &[&str] = &["Note", "P-Press", "Control", "Program", "C-Press", "PitchBd", "Switch/Meta", "SysEx"];
pub const VALUE_AS: &[&str] = &["Number", "Pan", "Hz", "Octave", "dB", "ms", "bpm", "Text"];
pub const FILTERS: &[&str] = &["Off", "Other", "Match", "All", "Thru", "Shot", "14 Bit"];
pub const TR_MODES: &[&str] = &[
    "Apply operation and let non-matching events pass thru",
    "Apply operation and filter non-matching events",
    "Filter matching events",
    "Copy matching events and apply operation",
    "Copy matching events and apply operation (reverse order)",
    "Condition splitter (true → top, false → bottom)",
    "Alternating split",
    "SysEx mapper",
    "Track automation splitter",
];
/// The transformer's Status condition / operation choices.
pub const TR_STATUS: &[&str] = &["All", "Note", "Poly Pressure", "Control", "Program", "Aftertouch", "Pitch Bend", "Meta"];
pub const TR_COND: &[&str] = &["All", "=", "≠", "<", ">", "Inside", "Outside"];
/// The transformer's operations (Logic's Transform names, and Use Map for data byte 2).
pub const TR_OPS: &[&str] = &["Thru", "Fix", "Add", "Sub", "Min", "Max", "Flip", "Mul", "Div", "Scale", "Range", "Random", "+- Rand", "Quantize", "Q Up", "Expo", "Use Map"];
pub const ARP_DIRS: &[&str] = &["Up", "Down", "Up/Down", "Auto", "Up/Down2", "Random", "All"];
/// Note divisions from whole notes to 768th notes (their ticks: DIV_TICKS).
pub const DIVS: &[&str] = &["1/1", "1/2", "1/3", "1/4", "1/6", "1/8", "1/12", "1/16", "1/24", "1/32", "1/48", "1/64", "1/96", "1/128", "1/192", "1/384", "1/768"];
pub const ARP_RES: &[&str] = &["None", "1/1", "1/2", "1/3", "1/4", "1/6", "1/8", "1/12", "1/16", "1/24", "1/32", "1/48", "1/64", "1/96", "1/128", "1/192", "1/384", "1/768"];
pub const ARP_LEN: &[&str] = &["Original", "1/1", "1/2", "1/3", "1/4", "1/6", "1/8", "1/12", "1/16", "1/24", "1/32", "1/48", "1/64", "1/96", "1/128", "1/192", "1/384", "1/768"];
pub const VL_PRIO: &[&str] = &["Last", "Top", "Bottom"];
pub const STAFF: &[&str] = &["Auto", "Treble", "Bass", "Piano", "Drums", "Guitar"];
pub const STRIP_TYPES: &[&str] = &["Audio", "Input", "Aux", "Instrument", "Output", "Bus", "Master"];
pub const TOUCH_TRIGGER: &[&str] = &["Multi", "Single", "Gate", "Gate Loop", "Toggle", "Toggle Loop"];
pub const TOUCH_START: &[&str] = &["Free", "Next 1/16", "Next 1/4", "Next 1/1"];
pub const GM_STD: &[&str] = &["GM", "GS", "XG"];

/// Ticks of a division in DIVS (index from 0 = 1/1).
pub fn div_ticks(i: usize) -> u32 {
    let d: u32 = DIVS.get(i).and_then(|s| s[2..].parse().ok()).unwrap_or(16);
    PPQ * 4 / d.max(1)
}

const INSTRUMENT: &[Spec] = &[
    Spec("ch", "Channel", Widget::Num(0, 16), 1),
    Spec("prog_on", "Program", Widget::Check, 0),
    Spec("prog", "Program Number", Widget::Num(0, 127), 0),
    Spec("bank", "Bank", Widget::Num(0, 15), 0),
    Spec("vol_on", "Volume", Widget::Check, 0),
    Spec("vol", "Volume Value", Widget::Num(0, 127), 100),
    Spec("pan_on", "Pan", Widget::Check, 0),
    Spec("pan", "Pan Value", Widget::Num(0, 127), 64),
    Spec("trans", "Transposition", Widget::Num(-96, 96), 0),
    Spec("velo", "Velocity", Widget::Num(-99, 99), 0),
    Spec("klo", "Key Limit Low", Widget::Note, 0),
    Spec("khi", "Key Limit High", Widget::Note, 127),
    Spec("vlo", "Vel Limit Low", Widget::Num(1, 127), 1),
    Spec("vhi", "Vel Limit High", Widget::Num(1, 127), 127),
    Spec("delay", "Delay (ms)", Widget::Num(-500, 500), 0),
    Spec("notrans", "No Transpose", Widget::Check, 0),
    Spec("noreset", "No Reset", Widget::Check, 0),
    Spec("staff", "Staff Style", Widget::Popup(STAFF), 0),
];

/// The Object inspector's parameters of a kind (besides Name, Icon, Assignable and Port).
pub fn specs(k: Kind) -> &'static [Spec] {
    match k {
        Kind::SequencerInput => &[Spec("chz", "Channelize", Widget::Check, 0)],
        Kind::Keyboard => &[Spec("ch", "Channel", Widget::Num(1, 16), 1), Spec("vel", "Velocity", Widget::Num(1, 127), 100), Spec("low", "Lowest", Widget::Note, 24)],
        Kind::Fader => &[
            Spec("style", "Style", Widget::Popup(FADER_STYLES), 1),
            Spec("in_t", "Input", Widget::Popup(DEF_TYPES), 2),
            Spec("in_ch", "Input Channel", Widget::Num(1, 16), 1),
            Spec("in_d1", "Input -1-", Widget::Num(0, 127), 7),
            Spec("out_t", "Output", Widget::Popup(DEF_TYPES), 2),
            Spec("out_ch", "Output Channel", Widget::Num(1, 16), 1),
            Spec("out_d1", "Output -1-", Widget::Num(0, 127), 7),
            Spec("min", "Range Min", Widget::Num(0, 16383), 0),
            Spec("max", "Range Max", Widget::Num(0, 16383), 127),
            Spec("as", "Value as", Widget::Popup(VALUE_AS), 0),
            Spec("filter", "Filter", Widget::Popup(FILTERS), 0),
            Spec("feedback", "Feedback", Widget::Check, 0),
            Spec("menu", "Behave as Menu", Widget::Check, 1),
            Spec("val", "Value", Widget::Num(0, 16383), 0),
            Spec("val2", "Value (horizontal)", Widget::Num(0, 16383), 64),
            Spec("mute", "Muted", Widget::Check, 0),
        ],
        Kind::Transformer => &[
            Spec("mode", "Mode", Widget::Popup(TR_MODES), 0),
            Spec("c_st", "Status Condition", Widget::Popup(TR_COND), 0),
            Spec("c_stv", "Status", Widget::Popup(TR_STATUS), 1),
            Spec("c_ch", "Channel Condition", Widget::Popup(TR_COND), 0),
            Spec("c_cha", "Channel A", Widget::Num(1, 16), 1),
            Spec("c_chb", "Channel B", Widget::Num(1, 16), 16),
            Spec("c_d1", "Data Byte 1 Condition", Widget::Popup(TR_COND), 0),
            Spec("c_d1a", "Data Byte 1 A", Widget::Num(0, 127), 0),
            Spec("c_d1b", "Data Byte 1 B", Widget::Num(0, 127), 127),
            Spec("c_d2", "Data Byte 2 Condition", Widget::Popup(TR_COND), 0),
            Spec("c_d2a", "Data Byte 2 A", Widget::Num(0, 127), 0),
            Spec("c_d2b", "Data Byte 2 B", Widget::Num(0, 127), 127),
            Spec("o_st", "Status Operation", Widget::Popup(&["Thru", "Fix"]), 0),
            Spec("o_stv", "Status Value", Widget::Popup(TR_STATUS), 1),
            Spec("o_ch", "Channel Operation", Widget::Popup(TR_OPS), 0),
            Spec("o_cha", "Channel Value", Widget::Num(-127, 127), 1),
            Spec("o_chb", "Channel Value B", Widget::Num(-127, 127), 16),
            Spec("o_d1", "Data Byte 1 Operation", Widget::Popup(TR_OPS), 0),
            Spec("o_d1a", "Data Byte 1 Value", Widget::Num(-127, 127), 0),
            Spec("o_d1b", "Data Byte 1 Value B", Widget::Num(-127, 127), 127),
            Spec("o_d1f", "Data Byte 1 Factor %", Widget::Num(0, 1000), 100),
            Spec("o_d2", "Data Byte 2 Operation", Widget::Popup(TR_OPS), 0),
            Spec("o_d2a", "Data Byte 2 Value", Widget::Num(-127, 127), 0),
            Spec("o_d2b", "Data Byte 2 Value B", Widget::Num(-127, 127), 127),
            Spec("o_d2f", "Data Byte 2 Factor %", Widget::Num(0, 1000), 100),
            Spec("mappos", "Map Position", Widget::Num(0, 127), 0),
        ],
        Kind::Arpeggiator => &[
            Spec("dir", "Direction", Widget::Popup(ARP_DIRS), 0),
            Spec("vel", "Velocity (0 Original, 128 Random)", Widget::Num(0, 128), 0),
            Spec("klo", "Key Limit Low", Widget::Note, 0),
            Spec("khi", "Key Limit High", Widget::Note, 127),
            Spec("res", "Resolution", Widget::Popup(ARP_RES), 8),
            Spec("len", "Length", Widget::Popup(ARP_LEN), 0),
            Spec("snap", "Snap to", Widget::Popup(ARP_RES), 0),
            Spec("rep", "Repeat", Widget::Check, 1),
            Spec("oct", "Octaves", Widget::Num(1, 10), 1),
            Spec("cresc", "Crescendo", Widget::Num(-99, 99), 0),
            Spec("cbase", "Controller Base (0 Off)", Widget::Num(0, 118), 0),
        ],
        Kind::ChordMemorizer => &[
            Spec("ch", "Channel (0 Original)", Widget::Num(0, 16), 0),
            Spec("klo", "Key Limit Low", Widget::Note, 0),
            Spec("khi", "Key Limit High", Widget::Note, 127),
            Spec("trans", "Transposition", Widget::Num(-127, 127), 0),
            Spec("key", "Key", Widget::Num(-11, 11), 0),
            Spec("split", "Cable Split", Widget::Check, 0),
        ],
        Kind::DelayLine => &[
            Spec("thru", "Thru Original", Widget::Check, 1),
            Spec("rep", "Repeats", Widget::Num(0, 99), 3),
            Spec("delay", "Delay (ticks)", Widget::Num(1, 256 * 3840), 240),
            Spec("trans", "Transposition", Widget::Num(-48, 48), 0),
            Spec("velo", "Velocity", Widget::Num(-99, 99), 0),
        ],
        Kind::VoiceLimiter => &[Spec("voices", "Voices", Widget::Num(1, 32), 1), Spec("prio", "Priority", Widget::Popup(VL_PRIO), 0), Spec("velmin", "VelOn-Min (0 Off)", Widget::Num(0, 127), 0)],
        Kind::Instrument => INSTRUMENT,
        Kind::MultiInstrument => &[Spec("progbox", "Show Program in track list", Widget::Check, 0), Spec("gmdrums", "GM Drum Program Names for Channel 10", Widget::Check, 1), Spec("bankmsg", "Bank Message", Widget::Popup(&["Control 32", "Control 0", "Custom"]), 0)],
        Kind::MappedInstrument => &[Spec("ch", "Channel (0 All)", Widget::Num(0, 16), 10), Spec("prog_on", "Program", Widget::Check, 0), Spec("prog", "Program Number", Widget::Num(0, 127), 0), Spec("delay", "Delay (ms)", Widget::Num(-500, 500), 0), Spec("notrans", "No Transpose", Widget::Check, 1)],
        Kind::MidiClick => &[
            Spec("bar", "Bar", Widget::Check, 1),
            Spec("bar_ch", "Bar Channel", Widget::Num(1, 16), 10),
            Spec("bar_n", "Bar Note", Widget::Note, 37),
            Spec("bar_v", "Bar Velocity", Widget::Num(1, 127), 112),
            Spec("beat", "Beat", Widget::Check, 1),
            Spec("beat_ch", "Beat Channel", Widget::Num(1, 16), 10),
            Spec("beat_n", "Beat Note", Widget::Note, 37),
            Spec("beat_v", "Beat Velocity", Widget::Num(1, 127), 80),
            Spec("div", "Division", Widget::Check, 0),
            Spec("div_ch", "Division Channel", Widget::Num(1, 16), 10),
            Spec("div_n", "Division Note", Widget::Note, 37),
            Spec("div_v", "Division Velocity", Widget::Num(1, 127), 50),
            Spec("play", "Click while playing", Widget::Check, 0),
            Spec("rec", "Click while recording", Widget::Check, 1),
        ],
        Kind::MmcRecord => &[Spec("extra", "Extra", Widget::Check, 0), Spec("armed", "Armed (bit mask)", Widget::Num(0, 65535), 0)],
        Kind::Alias => &[Spec("ref", "Reference", Widget::Num(0, 100000), 0), Spec("ch", "Channel (0 All)", Widget::Num(0, 16), 0), Spec("sname", "Share Name", Widget::Check, 1), Spec("ssize", "Share Size", Widget::Check, 1), Spec("val", "Value", Widget::Num(0, 16383), 0)],
        Kind::GmMixer => &[Spec("ch", "Channel", Widget::Num(1, 16), 1), Spec("legend", "Legend", Widget::Check, 1), Spec("bankrow", "Bank", Widget::Check, 0), Spec("std", "Standard", Widget::Popup(GM_STD), 0), Spec("k1", "Knob 1 Controller", Widget::Num(0, 127), 10), Spec("k2", "Knob 2 Controller", Widget::Num(0, 127), 91), Spec("k3", "Knob 3 Controller", Widget::Num(0, 127), 93)],
        Kind::ChannelStrip => &[Spec("trk", "Track", Widget::Num(0, 999), 0), Spec("type", "Channel", Widget::Popup(STRIP_TYPES), 3), Spec("eqs", "Show EQs", Widget::Check, 1), Spec("ins", "Show Inserts", Widget::Check, 1), Spec("sends", "Show Sends", Widget::Check, 1), Spec("io", "Show I/O", Widget::Check, 1)],
        Kind::TouchTracks => &[],
        Kind::Macro => &[Spec("autofader", "Auto Fader", Widget::Check, 0), Spec("showall", "Show All", Widget::Check, 1), Spec("noreset", "No Reset", Widget::Check, 0), Spec("protected", "Protected", Widget::Check, 0)],
        _ => &[],
    }
}

/// An object: kind, place, size, name, parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Obj {
    pub id: Id,
    pub kind: Kind,
    pub name: String,
    /// Layer index (1 = Global Objects; 0, All Objects, is never an object's layer).
    pub layer: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// A colour (RGB), from View ▸ Show Colors.
    pub color: u32,
    pub icon: u16,
    pub assignable: bool,
    /// The direct output: "" Off, "*" All, else a MIDI output port's name.
    pub port: String,
    pub params: BTreeMap<String, i32>,
    /// Text values: "names" (text fader), "c0".."c11" (chords), "map" (mapped instrument),
    /// "rows" (touch tracks), "progs<bank>" (multi-instrument), "banks" (custom bank messages).
    pub texts: BTreeMap<String, String>,
}

impl Obj {
    pub fn new(id: Id, kind: Kind, layer: usize, x: f32, y: f32) -> Obj {
        let (w, h) = kind.default_size();
        let mut o = Obj { id, kind, name: kind.name().to_string(), layer, x, y, w, h, color: 0xd0d0d0, icon: 0, assignable: kind.assignable_default(), port: String::new(), params: BTreeMap::new(), texts: BTreeMap::new() };
        if kind == Kind::Instrument || kind == Kind::MultiInstrument || kind == Kind::MappedInstrument {
            o.port = "*".into();
        }
        o
    }

    /// A parameter (its default when unset).
    pub fn p(&self, key: &str) -> i32 {
        if let Some(v) = self.params.get(key) {
            return *v;
        }
        // (multi-instrument subchannels: "s<n>.<key>" falls back to the instrument's spec)
        let base = key.split_once('.').map_or(key, |(_, k)| k);
        specs(self.kind).iter().chain(INSTRUMENT.iter()).find(|s| s.0 == base).map_or(0, |s| s.3)
    }

    pub fn set(&mut self, key: &str, v: i32) {
        let base = key.split_once('.').map_or(key, |(_, k)| k);
        let range = specs(self.kind).iter().chain(INSTRUMENT.iter()).find(|s| s.0 == base).map(|s| match s.2 {
            Widget::Num(a, b) => (a, b),
            Widget::Check => (0, 1),
            Widget::Popup(o) => (0, o.len() as i32 - 1),
            Widget::Note => (0, 127),
        });
        let v = range.map_or(v, |(a, b)| v.clamp(a, b));
        self.params.insert(key.to_string(), v);
    }

    pub fn on(&self, key: &str) -> bool {
        self.p(key) != 0
    }

    pub fn rect_contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }

    /// Is this a cable switcher (a Switch/Meta fader with -1- 48)?
    pub fn is_cable_switcher(&self) -> bool {
        self.kind == Kind::Fader && self.p("out_t") == 6 && self.p("out_d1") == 48
    }
    pub fn is_alias_assigner(&self) -> bool {
        self.kind == Kind::Fader && self.p("out_t") == 6 && self.p("out_d1") == 46
    }

    /// Chord memorizer: the chord for a pitch class, as semitones from the input note.
    pub fn chord(&self, pc: usize) -> Vec<i32> {
        self.texts.get(&format!("c{pc}")).map(|t| t.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default()
    }
    pub fn set_chord(&mut self, pc: usize, notes: &[i32]) {
        let mut n: Vec<i32> = notes.to_vec();
        n.sort();
        n.dedup();
        n.truncate(12);
        self.texts.insert(format!("c{pc}"), n.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","));
    }

    /// Text fader names (one per value from the range's minimum).
    pub fn names(&self) -> Vec<String> {
        self.texts.get("names").map(|t| t.lines().map(|l| l.to_string()).collect()).unwrap_or_default()
    }
}

/// A mapped instrument's row for one input note.
#[derive(Clone, Debug, PartialEq)]
pub struct MapRow {
    pub name: String,
    pub out: u8,
    pub vel: i32,
    /// 0 = Base (the instrument's channel), else 1–16.
    pub ch: u8,
    /// 1–16 (the top outlet is 1).
    pub cable: u8,
    /// Notation (a mapped staff style): the head (index into score::NOTE_HEADS), the place on
    /// the staff in half lines from the top line (Rel. Pos. shows half of it: whole numbers on
    /// lines, halves in spaces; up is positive) and the drum group ("" = not shown).
    pub head: u8,
    pub relpos: i8,
    pub group: String,
}

impl MapRow {
    /// A note's row as Logic starts it: General MIDI drum notation (heads, places, groups).
    pub fn default_for(n: u8) -> MapRow {
        let (head, relpos, group) = gm_drum_notation(n);
        MapRow { name: note_name(n), out: n, vel: 0, ch: 0, cable: 1, head, relpos, group: group.to_string() }
    }
}

/// The predefined drum groups (Mapped Instrument window ▸ Group); a project adds its own.
pub const DRUM_GROUPS: [&str; 6] = ["Kick", "Snare", "Hi-Hat", "Toms", "Cymbals", "Percussion"];

/// General MIDI drum notation for a key: (head, Rel. Pos. in half lines, group). Kick in the
/// bottom space, snare in the third space, toms around it, hi-hats and cymbals above the staff
/// with cross heads; other percussion in the middle space; keys outside 35–81 in no group.
pub fn gm_drum_notation(n: u8) -> (u8, i8, &'static str) {
    // (places in half spaces up from the bottom line; the top line is 8)
    let (pos, cross, group): (i8, bool, &str) = match n {
        35 | 36 => (1, false, "Kick"),
        37..=40 => (5, false, "Snare"),
        41 | 43 => (2, false, "Toms"),
        45 | 47 => (6, false, "Toms"),
        48 | 50 => (7, false, "Toms"),
        42 | 46 => (9, true, "Hi-Hat"),
        44 => (-1, true, "Hi-Hat"),
        49 | 52 | 55 | 57 => (10, true, "Cymbals"),
        51 | 53 | 59 => (8, true, "Cymbals"),
        54 | 56 | 69 | 70 | 75 => (9, false, "Percussion"),
        58 | 60..=68 | 71..=74 | 76..=81 => (4, false, "Percussion"),
        _ => (4, false, ""),
    };
    (cross as u8, pos - 8, group)
}

/// General MIDI drum names (key 35–81), for Initialize ▸ Names as General MIDI.
pub const GM_DRUMS: [&str; 47] = ["Acoustic Bass Drum", "Bass Drum 1", "Side Stick", "Acoustic Snare", "Hand Clap", "Electric Snare", "Low Floor Tom", "Closed Hi-Hat", "High Floor Tom", "Pedal Hi-Hat", "Low Tom", "Open Hi-Hat", "Low-Mid Tom", "Hi-Mid Tom", "Crash Cymbal 1", "High Tom", "Ride Cymbal 1", "Chinese Cymbal", "Ride Bell", "Tambourine", "Splash Cymbal", "Cowbell", "Crash Cymbal 2", "Vibraslap", "Ride Cymbal 2", "Hi Bongo", "Low Bongo", "Mute Hi Conga", "Open Hi Conga", "Low Conga", "High Timbale", "Low Timbale", "High Agogo", "Low Agogo", "Cabasa", "Maracas", "Short Whistle", "Long Whistle", "Short Guiro", "Long Guiro", "Claves", "Hi Wood Block", "Low Wood Block", "Mute Cuica", "Open Cuica", "Mute Triangle", "Open Triangle"];

/// The 128 General MIDI program names.
pub const GM_PROGRAMS: [&str; 128] = [
    "Acoustic Grand Piano", "Bright Acoustic Piano", "Electric Grand Piano", "Honky-tonk Piano", "Electric Piano 1", "Electric Piano 2", "Harpsichord", "Clavinet", "Celesta", "Glockenspiel", "Music Box", "Vibraphone", "Marimba", "Xylophone", "Tubular Bells", "Dulcimer", "Drawbar Organ", "Percussive Organ", "Rock Organ", "Church Organ", "Reed Organ", "Accordion", "Harmonica", "Tango Accordion", "Nylon Guitar", "Steel Guitar", "Jazz Guitar", "Clean Guitar", "Muted Guitar", "Overdriven Guitar", "Distortion Guitar", "Guitar Harmonics", "Acoustic Bass", "Fingered Bass", "Picked Bass", "Fretless Bass", "Slap Bass 1", "Slap Bass 2", "Synth Bass 1", "Synth Bass 2", "Violin", "Viola", "Cello", "Contrabass", "Tremolo Strings", "Pizzicato Strings", "Orchestral Harp", "Timpani", "String Ensemble 1", "String Ensemble 2", "Synth Strings 1", "Synth Strings 2", "Choir Aahs", "Voice Oohs", "Synth Voice", "Orchestra Hit", "Trumpet", "Trombone", "Tuba", "Muted Trumpet", "French Horn", "Brass Section", "Synth Brass 1", "Synth Brass 2", "Soprano Sax", "Alto Sax", "Tenor Sax", "Baritone Sax", "Oboe", "English Horn", "Bassoon", "Clarinet", "Piccolo", "Flute", "Recorder", "Pan Flute", "Blown Bottle", "Shakuhachi", "Whistle", "Ocarina", "Square Lead", "Sawtooth Lead", "Calliope Lead", "Chiff Lead", "Charang Lead", "Voice Lead", "Fifths Lead", "Bass + Lead", "New Age Pad", "Warm Pad", "Polysynth Pad", "Choir Pad", "Bowed Pad", "Metallic Pad", "Halo Pad", "Sweep Pad", "Rain", "Soundtrack", "Crystal", "Atmosphere", "Brightness", "Goblins", "Echoes", "Sci-Fi", "Sitar", "Banjo", "Shamisen", "Koto", "Kalimba", "Bagpipe", "Fiddle", "Shanai", "Tinkle Bell", "Agogo", "Steel Drums", "Woodblock", "Taiko Drum", "Melodic Tom", "Synth Drum", "Reverse Cymbal", "Guitar Fret Noise", "Breath Noise", "Seashore", "Bird Tweet", "Telephone Ring", "Helicopter", "Applause", "Gunshot",
];

impl Obj {
    pub fn map_row(&self, n: u8) -> MapRow {
        self.texts.get("map").and_then(|t| t.lines().find(|l| l.split('\t').next().and_then(|x| x.parse::<u8>().ok()) == Some(n)).and_then(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Some(MapRow { name: f.get(1)?.to_string(), out: f.get(2)?.parse().ok()?, vel: f.get(3)?.parse().ok()?, ch: f.get(4)?.parse().ok()?, cable: f.get(5)?.parse().ok()?, head: f.get(6).and_then(|x| x.parse().ok()).unwrap_or(0), relpos: f.get(7).and_then(|x| x.parse().ok()).unwrap_or(0), group: f.get(8).unwrap_or(&"").to_string() })
        })).unwrap_or_else(|| MapRow::default_for(n))
    }

    pub fn set_map_row(&mut self, n: u8, r: &MapRow) {
        let mut rows: BTreeMap<u8, String> = self.texts.get("map").map(|t| t.lines().filter_map(|l| Some((l.split('\t').next()?.parse::<u8>().ok()?, l.to_string()))).collect()).unwrap_or_default();
        if *r == MapRow::default_for(n) {
            rows.remove(&n);
        } else {
            rows.insert(n, format!("{n}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", r.name.chars().take(12).collect::<String>().replace(['\t', '\n'], " "), r.out, r.vel, r.ch, r.cable, r.head, r.relpos, r.group));
        }
        self.texts.insert("map".into(), rows.into_values().collect::<Vec<_>>().join("\n"));
    }

    /// Multi-instrument: the program name in a bank (bank 0's when that bank isn't initialized).
    pub fn program_name(&self, bank: usize, prog: usize) -> String {
        let pick = |b: usize| self.texts.get(&format!("progs{b}")).and_then(|t| t.lines().nth(prog).map(|s| s.to_string()));
        pick(bank).or_else(|| pick(0)).unwrap_or_else(|| format!("{}", prog + 1))
    }
}

/// Touch tracks: one row per input note.
#[derive(Clone, Debug, PartialEq)]
pub struct TouchRow {
    /// The region (track index, region index) and its name; None: nothing.
    pub region: Option<(usize, usize)>,
    pub name: String,
    /// 0 = off, 1.. = group.
    pub group: u8,
    pub transpose: i32,
    /// 0 = 100%, 1 = 50%, 2 = off.
    pub velocity: u8,
    pub trigger: u8,
    pub start: u8,
    pub delay: i32,
}

impl Obj {
    pub fn touch_row(&self, n: u8) -> Option<TouchRow> {
        let l = self.texts.get("rows")?.lines().find(|l| l.split('\t').next().and_then(|x| x.parse::<u8>().ok()) == Some(n))?.to_string();
        let f: Vec<&str> = l.split('\t').collect();
        let region = match (f.get(1)?.parse::<i64>().ok()?, f.get(2)?.parse::<i64>().ok()?) {
            (t, r) if t >= 0 && r >= 0 => Some((t as usize, r as usize)),
            _ => None,
        };
        Some(TouchRow { region, name: f.get(3)?.to_string(), group: f.get(4)?.parse().ok()?, transpose: f.get(5)?.parse().ok()?, velocity: f.get(6)?.parse().ok()?, trigger: f.get(7)?.parse().ok()?, start: f.get(8)?.parse().ok()?, delay: f.get(9)?.parse().ok()? })
    }

    pub fn set_touch_row(&mut self, n: u8, r: Option<&TouchRow>) {
        let mut rows: BTreeMap<u8, String> = self.texts.get("rows").map(|t| t.lines().filter_map(|l| Some((l.split('\t').next()?.parse::<u8>().ok()?, l.to_string()))).collect()).unwrap_or_default();
        match r {
            None => {
                rows.remove(&n);
            }
            Some(r) => {
                let (t, g) = r.region.map_or((-1, -1), |(a, b)| (a as i64, b as i64));
                rows.insert(n, format!("{n}\t{t}\t{g}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", r.name.replace(['\t', '\n'], " "), r.group, r.transpose, r.velocity, r.trigger, r.start, r.delay));
            }
        }
        self.texts.insert("rows".into(), rows.into_values().collect::<Vec<_>>().join("\n"));
    }
}

/// A cable: from an object's outlet to another object's input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cable {
    pub from: Id,
    /// The outlet: for the Physical Input 0 = SUM and 1–64 the ports; for a channel splitter
    /// 0 = SUM and 1–16 the channels; for other objects the order cables were made in.
    pub outlet: u16,
    pub to: Id,
}

/// What leaves the Environment.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    /// Reached the Sequencer Input: to the selected track (Channelize: on its channel).
    Seq { ev: Ev, channelize: bool },
    /// To a MIDI output port ("*" all ports).
    Port { port: String, ev: Ev },
    /// To a track's channel strip (an instrument track's instrument).
    Track { track: usize, ev: Ev },
    /// A meta event that works without cabling: 49 screenset, 50 project, 51 marker, 52 stop,
    /// 100 tempo.
    Meta { num: u8, val: u8 },
    /// A touch tracks object was played: the app starts or stops the row's region.
    Touch { obj: Id, ev: Ev },
    /// An MMC record buttons object's tracks changed (bit mask).
    MmcArm { mask: u32 },
    /// Raw bytes (a SysEx fader's System Exclusive messages) to a MIDI output port.
    Raw { port: String, bytes: Vec<u8> },
}

/// The clock the Environment runs on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    /// Milliseconds (any steady origin).
    pub ms: f64,
    pub bpm: f64,
    pub playing: bool,
    /// The song position in ticks (for Snap to).
    pub song: f64,
}

impl Clock {
    pub fn ms_per_tick(&self) -> f64 {
        60_000.0 / self.bpm.max(1.0) / PPQ as f64
    }
}

/// Live state that isn't saved: monitors' lists, notes held, arpeggiators.
#[derive(Clone, Debug, Default)]
pub struct Run {
    pub log: VecDeque<Ev>,
    /// Notes held: (pitch, velocity, channel, order).
    pub held: Vec<(u8, u8, u8, u64)>,
    arp_step: usize,
    arp_up: bool,
    arp_next: f64,
    arp_rep: i32,
    alt: bool,
    /// Voice limiter / chord memorizer / instrument: notes sounding out.
    pub out_notes: Vec<(u8, u8)>,
    order: u64,
    /// Arp: notes as last played (to turn off), with their end time.
    arp_playing: Vec<(f64, u8, u8)>,
}

#[derive(Clone, Debug)]
enum Pend {
    /// Emit from an object (one cable when `outlet` is Some).
    Emit { from: Id, outlet: Option<usize>, ev: Ev },
    Out(Out),
}

/// The Environment: layers, objects, cables, and what's running.
#[derive(Clone, Debug)]
pub struct Env {
    /// Layer names; 0 All Objects and 1 Global Objects are fixed.
    pub layers: Vec<String>,
    pub objects: Vec<Obj>,
    pub cables: Vec<Cable>,
    pub next_id: Id,
    pub run: BTreeMap<Id, Run>,
    pending: Vec<(f64, Pend)>,
    rng: Rng,
}

impl PartialEq for Env {
    fn eq(&self, o: &Env) -> bool {
        self.layers == o.layers && self.objects == o.objects && self.cables == o.cables
    }
}

/// Depth limit for events passing objects (cable loops).
const MAX_DEPTH: usize = 64;

impl Default for Env {
    fn default() -> Env {
        Env::new()
    }
}

impl Env {
    /// A new project's Environment as Logic makes it: All Objects, Global Objects, Click & Ports
    /// (Physical Input → Input Notes keyboard → Input View monitor → Sequencer Input, and the
    /// MIDI Click), MIDI Instr. and Mixer.
    pub fn new() -> Env {
        let mut e = Env { layers: vec!["All Objects".into(), "Global Objects".into(), "Click & Ports".into(), "MIDI Instr.".into(), "Mixer".into()], objects: Vec::new(), cables: Vec::new(), next_id: 1, run: BTreeMap::new(), pending: Vec::new(), rng: Rng(0x5eed) };
        let pi = e.add(Kind::PhysicalInput, 2, 30.0, 20.0);
        let kb = e.add(Kind::Keyboard, 2, 260.0, 190.0);
        e.obj_mut(kb).unwrap().name = "Input Notes".into();
        let mon = e.add(Kind::Monitor, 2, 260.0, 90.0);
        e.obj_mut(mon).unwrap().name = "Input View".into();
        let si = e.add(Kind::SequencerInput, 2, 560.0, 90.0);
        e.add(Kind::MidiClick, 2, 330.0, 10.0);
        e.connect(pi, 0, kb);
        e.connect_next(kb, mon);
        e.connect_next(mon, si);
        e
    }

    pub fn obj(&self, id: Id) -> Option<&Obj> {
        self.objects.iter().find(|o| o.id == id)
    }
    pub fn obj_mut(&mut self, id: Id) -> Option<&mut Obj> {
        self.objects.iter_mut().find(|o| o.id == id)
    }
    pub fn find(&self, kind: Kind) -> Option<Id> {
        self.objects.iter().find(|o| o.kind == kind).map(|o| o.id)
    }

    /// Adds an object (New menu); a unique kind that exists already is returned instead.
    pub fn add(&mut self, kind: Kind, layer: usize, x: f32, y: f32) -> Id {
        if kind.unique() {
            if let Some(id) = self.find(kind) {
                return id;
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut o = Obj::new(id, kind, layer.max(1), x, y);
        // (a new fader made from the Specials submenu sets its own type afterwards)
        if kind == Kind::Fader {
            o.params.insert("val".into(), 0);
        }
        self.objects.push(o);
        id
    }

    /// Deletes objects and every cable to or from them.
    pub fn delete(&mut self, ids: &[Id]) {
        let mut all = ids.to_vec();
        for &id in ids {
            all.extend(self.all_members(id));
        }
        let ids = &all[..];
        self.objects.retain(|o| !ids.contains(&o.id));
        self.cables.retain(|c| !ids.contains(&c.from) && !ids.contains(&c.to));
        for id in ids {
            self.run.remove(id);
        }
    }

    /// Cables from an object, in outlet order.
    pub fn cables_from(&self, id: Id) -> Vec<Cable> {
        let mut v: Vec<Cable> = self.cables.iter().filter(|c| c.from == id).copied().collect();
        v.sort_by_key(|c| c.outlet);
        v
    }

    /// Does the object use fixed outlets (Physical Input ports, channel splitter channels)?
    pub fn fixed_outlets(&self, id: Id) -> bool {
        self.obj(id).is_some_and(|o| matches!(o.kind, Kind::PhysicalInput | Kind::ChannelSplitter))
    }

    /// Cables an outlet (a fixed outlet is replaced; others are added after the last).
    pub fn connect(&mut self, from: Id, outlet: u16, to: Id) -> bool {
        if from == to || self.obj(from).is_none() || self.obj(to).is_none() {
            return false;
        }
        if self.fixed_outlets(from) {
            self.cables.retain(|c| !(c.from == from && c.outlet == outlet));
        }
        self.cables.push(Cable { from, outlet, to });
        true
    }

    /// Cables the next free outlet of an object (Logic adds a new triangle each time).
    pub fn connect_next(&mut self, from: Id, to: Id) -> bool {
        let n = self.cables.iter().filter(|c| c.from == from).map(|c| c.outlet + 1).max().unwrap_or(0);
        self.connect(from, n, to)
    }

    /// Removes a cable (and closes the gap in a regular object's outlets).
    pub fn disconnect(&mut self, c: Cable) {
        self.cables.retain(|x| *x != c);
        if !self.fixed_outlets(c.from) {
            for x in self.cables.iter_mut().filter(|x| x.from == c.from && x.outlet > c.outlet) {
                x.outlet -= 1;
            }
        }
    }

    /// Objects shown on a layer (Global Objects show on every layer; All Objects shows all).
    pub fn on_layer(&self, layer: usize) -> Vec<Id> {
        self.objects.iter().filter(|o| layer == 0 || o.layer == layer || o.layer == 1).map(|o| o.id).collect()
    }

    /// Options ▸ Layer ▸ Create Layer: an empty "(unnamed)" layer above (after) `at`.
    pub fn create_layer(&mut self, at: usize) -> usize {
        let i = (at + 1).clamp(2, self.layers.len());
        self.layers.insert(i, "(unnamed)".into());
        for o in self.objects.iter_mut().filter(|o| o.layer >= i) {
            o.layer += 1;
        }
        i
    }

    /// Delete Layer: the layer and its objects (All Objects and Global Objects can't go).
    pub fn delete_layer(&mut self, layer: usize) -> bool {
        if layer < 2 || layer >= self.layers.len() {
            return false;
        }
        let ids: Vec<Id> = self.objects.iter().filter(|o| o.layer == layer).map(|o| o.id).collect();
        self.delete(&ids);
        self.layers.remove(layer);
        for o in self.objects.iter_mut().filter(|o| o.layer > layer) {
            o.layer -= 1;
        }
        true
    }

    /// Copies objects (with the cables between them; a single object keeps its output cables),
    /// offset; returns the new ids.
    pub fn duplicate(&mut self, ids: &[Id], dx: f32, dy: f32, layer: Option<usize>) -> Vec<Id> {
        // (a macro is copied with its members)
        let mut all = ids.to_vec();
        for &id in ids {
            for m in self.all_members(id) {
                if !all.contains(&m) {
                    all.push(m);
                }
            }
        }
        let single = ids.len() == 1 && all.len() == 1;
        let ids = &all[..];
        let mut map = BTreeMap::new();
        let mut out = Vec::new();
        for &id in ids {
            let Some(o) = self.obj(id).cloned() else { continue };
            if o.kind.unique() {
                continue;
            }
            let nid = self.next_id;
            self.next_id += 1;
            let mut n = o;
            n.id = nid;
            n.x += dx;
            n.y += dy;
            if let Some(l) = layer {
                n.layer = l;
            }
            // (copying an alias alone makes another alias of the same original)
            self.objects.push(n);
            map.insert(id, nid);
            out.push(nid);
        }
        let old: Vec<Cable> = self.cables.clone();
        for c in old {
            match (map.get(&c.from), map.get(&c.to)) {
                (Some(&f), Some(&t)) => self.cables.push(Cable { from: f, outlet: c.outlet, to: t }),
                (Some(&f), None) if single => self.cables.push(Cable { from: f, outlet: c.outlet, to: c.to }),
                _ => {}
            }
        }
        for &nid in &out {
            let m = self.obj(nid).map_or(0, |o| o.p("mac")) as Id;
            if m != 0 {
                let nm = map.get(&m).copied().map_or(0, |x| x as i32);
                if let Some(o) = self.obj_mut(nid) {
                    if nm == 0 { o.params.remove("mac"); } else { o.params.insert("mac".into(), nm); }
                }
            }
        }
        // (an alias copied with its original refers to the new original)
        for &nid in &out {
            if let Some(o) = self.obj(nid) {
                if o.kind == Kind::Alias {
                    if let Some(&nr) = map.get(&(o.p("ref") as Id)) {
                        self.obj_mut(nid).unwrap().set("ref", nr as i32);
                    }
                }
            }
        }
        out
    }

    /// Options ▸ Cable serially: the objects in series from the upper left.
    pub fn cable_serially(&mut self, ids: &[Id]) {
        let mut v: Vec<&Obj> = self.objects.iter().filter(|o| ids.contains(&o.id)).collect();
        v.sort_by(|a, b| (a.y + a.x).partial_cmp(&(b.y + b.x)).unwrap_or(std::cmp::Ordering::Equal));
        let order: Vec<Id> = v.iter().map(|o| o.id).collect();
        for w in order.windows(2) {
            if !self.cables.iter().any(|c| c.from == w[0] && c.to == w[1]) {
                self.connect_next(w[0], w[1]);
            }
        }
    }

    /// Options ▸ Clean up ▸ Align Objects: a row (next object to the right) or a column (below),
    /// from the upper-left object.
    pub fn align(&mut self, ids: &[Id]) {
        let mut v: Vec<(Id, f32, f32, f32, f32)> = self.objects.iter().filter(|o| ids.contains(&o.id)).map(|o| (o.id, o.x, o.y, o.w, o.h)).collect();
        if v.len() < 2 {
            return;
        }
        v.sort_by(|a, b| (a.1 + a.2).partial_cmp(&(b.1 + b.2)).unwrap_or(std::cmp::Ordering::Equal));
        let first = v[0];
        let rest = &v[1..];
        let row = (rest[0].1 - first.1) >= (rest[0].2 - first.2);
        let mut at = if row { first.1 + first.3 + 10.0 } else { first.2 + first.4 + 10.0 };
        let mut sorted: Vec<_> = rest.to_vec();
        sorted.sort_by(|a, b| if row { a.1.partial_cmp(&b.1) } else { a.2.partial_cmp(&b.2) }.unwrap_or(std::cmp::Ordering::Equal));
        for (id, _, _, w, h) in sorted {
            let o = self.obj_mut(id).unwrap();
            if row {
                o.x = at;
                o.y = first.2;
                at += w + 10.0;
            } else {
                o.y = at;
                o.x = first.1;
                at += h + 10.0;
            }
        }
    }

    /// Edit ▸ Select Used Instruments: objects tracks are assigned to, and anything cabled from
    /// them (`used`: the objects tracks use).
    pub fn used_closure(&self, used: &[Id]) -> Vec<Id> {
        let mut out: Vec<Id> = used.to_vec();
        let mut i = 0;
        while i < out.len() {
            let id = out[i];
            for c in self.cables.iter().filter(|c| c.from == id) {
                if !out.contains(&c.to) {
                    out.push(c.to);
                }
            }
            i += 1;
        }
        out
    }

    /// Clears all live state (Reset messages, Stop).
    pub fn reset(&mut self) {
        // (a macro with No Reset keeps its members' state)
        let keep: Vec<Id> = self.objects.iter().filter(|o| o.kind == Kind::Macro && o.on("noreset")).flat_map(|o| self.all_members(o.id)).collect();
        self.run.retain(|id, _| keep.contains(id));
        self.pending.clear();
    }

    // ---- MIDI flowing ----

    /// Events from the MIDI inputs (port: the input port's index, 0-based) into the Physical
    /// Input. Without a Physical Input nothing comes in.
    pub fn input(&mut self, port: usize, ev: Ev, clock: Clock) -> Vec<Out> {
        let mut out = Vec::new();
        let Some(pi) = self.find(Kind::PhysicalInput) else { return out };
        self.run_due(clock, &mut out);
        // (a port's own outlet when it's cabled, else SUM)
        let outlet = port as u16 + 1;
        let cabled = self.cables.iter().any(|c| c.from == pi && c.outlet == outlet);
        let ev = Tagged { ev, from_pi: true };
        let path = vec![pi];
        self.emit_outlet(pi, if cabled { outlet } else { 0 }, ev, clock, &mut out, &path);
        out
    }

    /// An object sends an event from its outputs (a keyboard key clicked, a fader moved).
    pub fn inject(&mut self, id: Id, ev: Ev, clock: Clock) -> Vec<Out> {
        let mut out = Vec::new();
        let Some(o) = self.obj(id).cloned() else { return out };
        let t = Tagged { ev, from_pi: false };
        match o.kind {
            // (instruments and their kin send to their port and cables as if played through)
            Kind::Instrument | Kind::MultiInstrument | Kind::MappedInstrument | Kind::GmMixer | Kind::MidiClick => self.receive(id, t, clock, &mut out, &[]),
            Kind::TouchTracks => out.push(Out::Touch { obj: id, ev }),
            _ => {
                if o.kind == Kind::Keyboard {
                    self.note_held(id, ev);
                }
                self.emit_all(id, t, clock, &mut out, &[id]);
                // (global meta events work without cabling)
                if ev.st == META && matches!(ev.d1, 49 | 50 | 51 | 52 | 100) {
                    out.push(Out::Meta { num: ev.d1, val: ev.d2 });
                }
            }
        }
        out
    }

    /// An event sent into an object's input (a track routed to it, Option-cabled destinations).
    pub fn deliver(&mut self, id: Id, ev: Ev, clock: Clock) -> Vec<Out> {
        let mut out = Vec::new();
        self.run_due(clock, &mut out);
        self.receive(id, Tagged { ev, from_pi: false }, clock, &mut out, &[]);
        out
    }

    /// A fader moved by the mouse to `v`: its value, and its Output definition sent (a cable
    /// switcher steps; a muted Vertical/Mute fader sends nothing).
    pub fn move_fader(&mut self, id: Id, v: i32, clock: Clock) -> Vec<Out> {
        let Some(o) = self.obj_mut(id) else { return Vec::new() };
        if o.kind != Kind::Fader && o.kind != Kind::Alias {
            return Vec::new();
        }
        o.params.insert("val".into(), v);
        let o = o.clone();
        if o.kind == Kind::Alias {
            return Vec::new();
        }
        if o.is_cable_switcher() {
            return Vec::new();
        }
        if o.p("style") == 5 && o.on("mute") {
            return Vec::new();
        }
        if o.p("out_t") == 7 {
            let mut out = Vec::new();
            self.run_due(clock, &mut out);
            self.sysex_out(&o, v, false, clock, &mut out, &[id]);
            return out;
        }
        let evs = fader_output(&o, v, false);
        let mut out = Vec::new();
        for ev in evs {
            out.extend(self.inject(id, ev, clock));
        }
        out
    }

    /// A SysEx fader's list sent for value `v`: channel and meta events along its cables, SysEx
    /// strings to the MIDI ports its cables lead to.
    fn sysex_out(&mut self, o: &Obj, v: i32, from_pi: bool, clock: Clock, out: &mut Vec<Out>, path: &[Id]) {
        use crate::sysexfader::{list_from, render, Msg};
        let list = list_from(o.texts.get("sysex"));
        for m in render(&list, v, o.p("sx_fmt").max(0) as usize, o.p("sx_sum").max(0) as usize) {
            match m {
                Msg::Chan { st, ch, d1, d2 } => self.emit_all(o.id, Tagged { ev: Ev::new(st, ch, d1, d2), from_pi }, clock, out, path),
                Msg::Meta { num, val } => self.emit_all(o.id, Tagged { ev: Ev::meta(num, val), from_pi }, clock, out, path),
                Msg::Raw(bytes) => {
                    for port in self.ports_downstream(o.id) {
                        out.push(Out::Raw { port, bytes: bytes.clone() });
                    }
                }
            }
        }
    }

    /// The MIDI output ports an object's cables lead to (the first objects with a port).
    pub fn ports_downstream(&self, id: Id) -> Vec<String> {
        let mut seen = vec![id];
        let mut todo: Vec<Id> = self.cables_from(id).iter().map(|c| c.to).collect();
        let mut ports = Vec::new();
        while let Some(t) = todo.pop() {
            if seen.contains(&t) {
                continue;
            }
            seen.push(t);
            let Some(o) = self.obj(t) else { continue };
            if o.kind.has_port() && !o.port.is_empty() {
                if !ports.contains(&o.port) {
                    ports.push(o.port.clone());
                }
                continue;
            }
            let next: Vec<Id> = if o.kind == Kind::Macro { self.macro_in(t).into_iter().collect() } else { self.cables_from(t).iter().map(|c| c.to).collect() };
            todo.extend(next);
            if let Some(m) = self.macro_of(t).filter(|m| self.macro_out(*m) == Some(t)) {
                todo.extend(self.cables_from(m).iter().map(|c| c.to));
            }
        }
        ports
    }

    /// A SysEx message from a MIDI input: faders whose Input is SysEx take its value (only very
    /// short messages are recognised).
    pub fn input_sysex(&mut self, bytes: &[u8], clock: Clock) -> Vec<Out> {
        let mut out = Vec::new();
        let hits: Vec<(Obj, i32)> = self.objects.iter().filter(|o| o.kind == Kind::Fader && o.p("in_t") == 7).filter_map(|o| crate::sysexfader::short_input(bytes, &crate::sysexfader::list_from(o.texts.get("sysex"))).map(|v| (o.clone(), v))).collect();
        for (o, v) in hits {
            if let Some(x) = self.obj_mut(o.id) {
                x.params.insert("val".into(), v);
            }
            if o.p("out_t") == 7 {
                self.sysex_out(&o, v, true, clock, &mut out, &[o.id]);
            } else {
                for ev in fader_output(&o, v, true) {
                    self.emit_all(o.id, Tagged { ev, from_pi: true }, clock, &mut out, &[o.id]);
                }
            }
        }
        out
    }

    // ---- macros ----

    /// The macro an object is packed in.
    pub fn macro_of(&self, id: Id) -> Option<Id> {
        self.obj(id).map(|o| o.p("mac") as Id).filter(|m| *m != 0 && self.obj(*m).is_some_and(|x| x.kind == Kind::Macro))
    }
    /// A macro's own members (not those of macros inside it).
    pub fn members(&self, m: Id) -> Vec<Id> {
        self.objects.iter().filter(|o| o.p("mac") as Id == m && m != 0).map(|o| o.id).collect()
    }
    /// Every object inside a macro, nested ones too.
    pub fn all_members(&self, m: Id) -> Vec<Id> {
        let mut v = Vec::new();
        if self.obj(m).is_none_or(|o| o.kind != Kind::Macro) {
            return v;
        }
        for c in self.members(m) {
            v.push(c);
            v.extend(self.all_members(c));
        }
        v
    }
    /// The macro's input: the member named Macro-In, else the upper-leftmost.
    pub fn macro_in(&self, m: Id) -> Option<Id> {
        let mem: Vec<&Obj> = self.members(m).iter().filter_map(|i| self.obj(*i)).collect();
        mem.iter().find(|o| o.name == "Macro-In").or_else(|| mem.iter().min_by(|a, b| (a.x + a.y).partial_cmp(&(b.x + b.y)).unwrap_or(std::cmp::Ordering::Equal))).map(|o| o.id)
    }
    /// The macro's output: the member named Macro-Out, else the lower-rightmost.
    pub fn macro_out(&self, m: Id) -> Option<Id> {
        let mem: Vec<&Obj> = self.members(m).iter().filter_map(|i| self.obj(*i)).collect();
        mem.iter().find(|o| o.name == "Macro-Out").or_else(|| mem.iter().max_by(|a, b| (a.x + a.w + a.y + a.h).partial_cmp(&(b.x + b.w + b.y + b.h)).unwrap_or(std::cmp::Ordering::Equal))).map(|o| o.id)
    }
    /// With the macro's Show All off only faders, monitors, ornaments and keyboards inside it show.
    pub fn hidden_in_macro(&self, id: Id) -> bool {
        let Some(o) = self.obj(id) else { return false };
        let mut cur = self.macro_of(id);
        while let Some(m) = cur {
            if self.obj(m).is_some_and(|x| !x.on("showall")) && !matches!(o.kind, Kind::Fader | Kind::Monitor | Kind::Ornament | Kind::Keyboard) {
                return true;
            }
            cur = self.macro_of(m);
        }
        false
    }
    /// Cables between the objects and others (a new macro deletes them).
    pub fn crossing_cables(&self, ids: &[Id]) -> Vec<Cable> {
        self.cables.iter().filter(|c| ids.contains(&c.from) != ids.contains(&c.to)).copied().collect()
    }
    /// New ▸ Macro: the selected objects (with the cables between them) packed into a macro
    /// framed around them. Cables to objects left outside are deleted — then the macro is made
    /// from a copy and the originals stay as they were (`true` in the result: warn). Too many
    /// objects (over 200 with nested ones) fail.
    pub fn make_macro(&mut self, ids: &[Id]) -> Result<(Id, bool), String> {
        let top: Vec<Id> = ids.iter().copied().filter(|i| self.obj(*i).is_some_and(|o| !o.kind.unique()) && !ids.contains(&self.macro_of(*i).unwrap_or(0))).collect();
        if top.is_empty() {
            return Err("Select the objects to pack into a macro".into());
        }
        let count: usize = top.iter().map(|i| 1 + self.all_members(*i).len()).sum();
        if count > 200 {
            return Err(format!("A macro can hold about 200 objects ({count} selected)"));
        }
        let mut inner: Vec<Id> = top.clone();
        for &i in &top {
            inner.extend(self.all_members(i));
        }
        let crossing = !self.crossing_cables(&inner).is_empty();
        let members = if crossing {
            let new = self.duplicate(&top, 24.0, 24.0, None);
            let cross = self.crossing_cables(&new.iter().flat_map(|i| std::iter::once(*i).chain(self.all_members(*i))).collect::<Vec<_>>());
            self.cables.retain(|c| !cross.contains(c));
            new.into_iter().filter(|i| self.macro_of(*i).is_none()).collect::<Vec<_>>()
        } else {
            top.clone()
        };
        let parent = self.macro_of(top[0]);
        let objs: Vec<Obj> = members.iter().filter_map(|i| self.obj(*i).cloned()).collect();
        let x0 = objs.iter().map(|o| o.x).fold(f32::MAX, f32::min) - 8.0;
        let y0 = objs.iter().map(|o| o.y).fold(f32::MAX, f32::min) - 20.0;
        let x1 = objs.iter().map(|o| o.x + o.w).fold(f32::MIN, f32::max) + 8.0;
        let y1 = objs.iter().map(|o| o.y + o.h).fold(f32::MIN, f32::max) + 8.0;
        let layer = objs[0].layer;
        let m = self.add(Kind::Macro, layer, x0, y0);
        let mo = self.obj_mut(m).unwrap();
        mo.w = x1 - x0;
        mo.h = y1 - y0;
        mo.color = 0x9a9a9a;
        if let Some(p) = parent {
            mo.params.insert("mac".into(), p as i32);
        }
        for i in &members {
            if let Some(o) = self.obj_mut(*i) {
                o.params.insert("mac".into(), m as i32);
            }
        }
        Ok((m, crossing))
    }
    /// Double-clicking a macro's empty area: unpacked into its objects and cables (cables to the
    /// macro go to its input object, from it leave its output object). A protected macro stays.
    pub fn unpack_macro(&mut self, m: Id) -> bool {
        let Some(mo) = self.obj(m).cloned() else { return false };
        if mo.kind != Kind::Macro || mo.on("protected") {
            return false;
        }
        let (inp, outp) = (self.macro_in(m), self.macro_out(m));
        for c in self.cables.iter_mut() {
            if c.to == m {
                if let Some(i) = inp {
                    c.to = i;
                }
            }
        }
        let n = self.cables.iter().filter(|c| Some(c.from) == outp).map(|c| c.outlet + 1).max().unwrap_or(0);
        let mut k = 0;
        for c in self.cables.iter_mut() {
            if c.from == m {
                if let Some(o) = outp {
                    c.from = o;
                    c.outlet = n + k;
                    k += 1;
                }
            }
        }
        let parent = mo.p("mac");
        for i in self.members(m) {
            if let Some(o) = self.obj_mut(i) {
                if parent == 0 { o.params.remove("mac"); } else { o.params.insert("mac".into(), parent); }
            }
        }
        self.cables.retain(|c| c.from != m && c.to != m);
        self.objects.retain(|o| o.id != m);
        true
    }
    /// Moves objects (macros take their members along).
    pub fn move_with_members(&mut self, ids: &[Id], dx: f32, dy: f32, layer: Option<usize>) {
        let mut all = ids.to_vec();
        for &i in ids {
            all.extend(self.all_members(i));
        }
        all.sort();
        all.dedup();
        for o in self.objects.iter_mut().filter(|o| all.contains(&o.id)) {
            o.x += dx;
            o.y += dy;
            if let Some(l) = layer {
                o.layer = l;
            }
        }
    }

    /// A vector fader's crosshair moved: vertical and horizontal values (0–127 range).
    pub fn move_vector(&mut self, id: Id, vert: i32, horz: i32, clock: Clock) -> Vec<Out> {
        let Some(o) = self.obj_mut(id) else { return Vec::new() };
        o.params.insert("val".into(), vert);
        o.params.insert("val2".into(), horz);
        let o = o.clone();
        let mut evs = Vec::new();
        let same = o.p("in_t") == o.p("out_t") && o.p("in_ch") == o.p("out_ch") && o.p("in_d1") == o.p("out_d1");
        if same {
            // four channels from the Vert channel: upper-left, upper-right, lower-left, lower-right
            let (lo, hi) = (o.p("min"), o.p("max"));
            let fx = ((horz - lo) as f32 / (hi - lo).max(1) as f32).clamp(0.0, 1.0);
            let fy = ((vert - lo) as f32 / (hi - lo).max(1) as f32).clamp(0.0, 1.0);
            let corners = [(1.0 - fx) * fy, fx * fy, (1.0 - fx) * (1.0 - fy), fx * (1.0 - fy)];
            for (k, w) in corners.iter().enumerate() {
                // (the centre sends 32 on all four: a quarter of 127, rounded)
                let v = lo + ((hi - lo) as f32 * w).round() as i32;
                let mut def = o.clone();
                def.params.insert("out_ch".into(), (o.p("in_ch") + k as i32 - 1) % 16 + 1);
                evs.extend(fader_output(&def, v, false));
            }
        } else {
            let mut vdef = o.clone();
            vdef.params.insert("out_t".into(), o.p("in_t"));
            vdef.params.insert("out_ch".into(), o.p("in_ch"));
            vdef.params.insert("out_d1".into(), o.p("in_d1"));
            evs.extend(fader_output(&vdef, vert, false));
            evs.extend(fader_output(&o, horz, false));
        }
        let mut out = Vec::new();
        for ev in evs {
            out.extend(self.inject(id, ev, clock));
        }
        out
    }

    /// Steps a cable switcher by a click (through its outlets and the last, uncabled one).
    pub fn click_switcher(&mut self, id: Id) {
        let n = self.cables_from(id).len() as i32;
        if let Some(o) = self.obj_mut(id) {
            let v = (o.p("val") + 1) % (n + 1).max(1);
            o.params.insert("val".into(), v);
        }
    }

    /// Options ▸ Send All Fader Values (`only`: Send Selected Fader Values).
    pub fn send_fader_values(&mut self, only: Option<&[Id]>, clock: Clock) -> Vec<Out> {
        let ids: Vec<(Id, i32)> = self.objects.iter().filter(|o| o.kind == Kind::Fader && !o.is_cable_switcher() && only.is_none_or(|s| s.contains(&o.id))).map(|o| (o.id, o.p("val"))).collect();
        let mut out = Vec::new();
        for (id, v) in ids {
            out.extend(self.move_fader(id, v, clock));
        }
        out
    }

    /// Options ▸ Reset Selected Faders: to zero, sent.
    pub fn reset_faders(&mut self, ids: &[Id], clock: Clock) -> Vec<Out> {
        let mut out = Vec::new();
        for &id in ids {
            if self.obj(id).is_some_and(|o| o.kind == Kind::Fader) {
                out.extend(self.move_fader(id, 0, clock));
            }
        }
        out
    }

    /// Each frame: delayed events that are due, arpeggiator steps.
    pub fn tick(&mut self, clock: Clock) -> Vec<Out> {
        let mut out = Vec::new();
        self.run_due(clock, &mut out);
        // arpeggiators
        let arps: Vec<Id> = self.objects.iter().filter(|o| o.kind == Kind::Arpeggiator).map(|o| o.id).collect();
        for id in arps {
            self.arp_tick(id, clock, &mut out);
        }
        self.run_due(clock, &mut out);
        out
    }

    /// Anything waiting (delay lines, arpeggios): the app keeps repainting while so.
    pub fn busy(&self) -> bool {
        !self.pending.is_empty() || self.run.values().any(|r| !r.held.is_empty() || !r.arp_playing.is_empty())
    }

    fn run_due(&mut self, clock: Clock, out: &mut Vec<Out>) {
        // (in time order; events made now that are due run in the same pass)
        for _ in 0..8 {
            let mut due: Vec<(f64, Pend)> = Vec::new();
            let mut keep = Vec::new();
            for p in std::mem::take(&mut self.pending) {
                if p.0 <= clock.ms {
                    due.push(p);
                } else {
                    keep.push(p);
                }
            }
            self.pending = keep;
            if due.is_empty() {
                break;
            }
            due.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            for (_, p) in due {
                match p {
                    Pend::Emit { from, outlet, ev } => {
                        let t = Tagged { ev, from_pi: false };
                        match outlet {
                            Some(k) => {
                                if let Some(c) = self.cables_from(from).get(k).copied() {
                                    self.receive(c.to, t, clock, out, &[from]);
                                }
                            }
                            None => self.emit_all(from, t, clock, out, &[from]),
                        }
                    }
                    Pend::Out(o) => out.push(o),
                }
            }
        }
    }

    fn later(&mut self, at: f64, p: Pend) {
        self.pending.push((at, p));
    }

    fn note_held(&mut self, id: Id, ev: Ev) {
        let r = self.run.entry(id).or_default();
        if ev.is_note_on() {
            r.order += 1;
            let o = r.order;
            r.held.retain(|h| h.0 != ev.d1);
            r.held.push((ev.d1, ev.d2, ev.ch, o));
        } else if ev.is_note_off() {
            r.held.retain(|h| h.0 != ev.d1);
        }
    }

    fn emit_outlet(&mut self, from: Id, outlet: u16, ev: Tagged, clock: Clock, out: &mut Vec<Out>, path: &[Id]) {
        let targets: Vec<Id> = self.cables.iter().filter(|c| c.from == from && c.outlet == outlet).map(|c| c.to).collect();
        for t in targets {
            self.receive(t, ev, clock, out, path);
        }
    }

    fn emit_all(&mut self, from: Id, ev: Tagged, clock: Clock, out: &mut Vec<Out>, path: &[Id]) {
        let targets: Vec<Id> = self.cables_from(from).iter().map(|c| c.to).collect();
        let uncabled = targets.is_empty();
        for t in targets {
            self.receive(t, ev, clock, out, path);
        }
        self.leave_macro(from, uncabled, ev, clock, out, path);
    }

    /// Events leaving a macro member: Auto Fader passes an uncabled fader's output to the next
    /// fader (as if cabled in series); the macro's output object sends out of the macro.
    fn leave_macro(&mut self, from: Id, uncabled: bool, ev: Tagged, clock: Clock, out: &mut Vec<Out>, path: &[Id]) {
        let Some(m) = self.macro_of(from) else { return };
        if path.len() > MAX_DEPTH {
            return;
        }
        if uncabled && self.obj(m).is_some_and(|x| x.on("autofader")) && self.obj(from).is_some_and(|o| o.kind == Kind::Fader) {
            let mut faders: Vec<&Obj> = self.members(m).iter().filter_map(|i| self.obj(*i)).filter(|o| o.kind == Kind::Fader).collect();
            faders.sort_by(|a, b| (a.y + a.x).partial_cmp(&(b.y + b.x)).unwrap_or(std::cmp::Ordering::Equal));
            if let Some(k) = faders.iter().position(|o| o.id == from) {
                if let Some(next) = faders.get(k + 1).map(|o| o.id) {
                    self.receive(next, ev, clock, out, path);
                    return;
                }
            }
        }
        if self.macro_out(m) == Some(from) {
            let mut p = path.to_vec();
            p.push(m);
            self.emit_all(m, ev, clock, out, &p);
        }
    }

    fn emit_nth(&mut self, from: Id, k: usize, ev: Tagged, clock: Clock, out: &mut Vec<Out>, path: &[Id]) {
        if let Some(c) = self.cables_from(from).get(k).copied() {
            self.receive(c.to, ev, clock, out, path);
        }
    }

    /// An event arrives at an object's input.
    fn receive(&mut self, id: Id, ev: Tagged, clock: Clock, out: &mut Vec<Out>, path: &[Id]) {
        if path.len() > MAX_DEPTH {
            return;
        }
        let Some(o) = self.obj(id).cloned() else { return };
        let mut p: Vec<Id> = path.to_vec();
        p.push(id);
        let path = &p[..];
        // meta events aimed at faders and transformers change them instead of passing
        if ev.ev.st == META && self.meta_control(&o, ev.ev, clock, out, path) {
            return;
        }
        match o.kind {
            Kind::PhysicalInput => {}
            Kind::SequencerInput => out.push(Out::Seq { ev: ev.ev, channelize: o.on("chz") }),
            Kind::Keyboard | Kind::Monitor => {
                if o.kind == Kind::Monitor {
                    let r = self.run.entry(id).or_default();
                    r.log.push_back(ev.ev);
                    while r.log.len() > 32 {
                        r.log.pop_front();
                    }
                } else {
                    self.note_held(id, ev.ev);
                }
                self.emit_all(id, ev, clock, out, path);
            }
            Kind::Ornament | Kind::MmcRecord => {}
            Kind::Macro => {
                if let Some(i) = self.macro_in(id) {
                    self.receive(i, ev, clock, out, path);
                }
            }
            Kind::ChannelSplitter => {
                let outlet = ev.ev.ch as u16 + 1;
                let cabled = self.cables.iter().any(|c| c.from == id && c.outlet == outlet);
                self.emit_outlet(id, if cabled { outlet } else { 0 }, ev, clock, out, path);
            }
            Kind::Fader => self.fader_in(&o, ev, clock, out, path),
            Kind::Transformer => self.transformer_in(&o, ev, clock, out, path),
            Kind::Arpeggiator => self.arp_in(&o, ev, clock, out, path),
            Kind::ChordMemorizer => self.chord_in(&o, ev, clock, out, path),
            Kind::DelayLine => {
                let e = ev.ev;
                if o.on("thru") {
                    self.emit_all(id, ev, clock, out, path);
                }
                // (echoes only while playing, as Logic's)
                let reps = o.p("rep");
                if reps > 0 && clock.playing {
                    let n_cables = self.cables_from(id).len().max(1);
                    let step = o.p("delay").max(1) as f64 * clock.ms_per_tick();
                    for k in 1..=reps {
                        let mut x = e;
                        if x.is_note() {
                            x.d1 = (x.d1 as i32 + o.p("trans") * k).clamp(0, 127) as u8;
                            if x.is_note_on() {
                                x.d2 = (x.d2 as i32 + o.p("velo") * k).clamp(1, 127) as u8;
                            }
                        }
                        // (echo k to cable k, in turn)
                        let outlet = if n_cables == 1 { 0 } else { (k as usize - 1) % n_cables };
                        self.later(clock.ms + step * k as f64, Pend::Emit { from: id, outlet: Some(outlet), ev: x });
                    }
                }
            }
            Kind::VoiceLimiter => {
                let e = ev.ev;
                if e.is_note_on() {
                    let mut e2 = e;
                    if o.p("velmin") > 0 {
                        e2.d2 = o.p("velmin") as u8;
                    }
                    let max = o.p("voices").max(1) as usize;
                    let r = self.run.entry(id).or_default();
                    r.order += 1;
                    let ord = r.order;
                    let mut stolen = Vec::new();
                    while r.held.len() >= max {
                        // Last: the earliest played goes; Top: the lowest; Bottom: the highest
                        let k = match o.p("prio") {
                            1 => r.held.iter().enumerate().min_by_key(|h| h.1.0).map(|h| h.0),
                            2 => r.held.iter().enumerate().max_by_key(|h| h.1.0).map(|h| h.0),
                            _ => r.held.iter().enumerate().min_by_key(|h| h.1.3).map(|h| h.0),
                        };
                        let Some(k) = k else { break };
                        stolen.push(r.held.remove(k));
                    }
                    r.held.push((e.d1, e.d2, e.ch, ord));
                    for s in stolen {
                        self.emit_all(id, Tagged { ev: Ev::new(0x80, s.2, s.0, 0), from_pi: ev.from_pi }, clock, out, path);
                    }
                    self.emit_all(id, Tagged { ev: e2, from_pi: ev.from_pi }, clock, out, path);
                } else if e.is_note_off() {
                    let r = self.run.entry(id).or_default();
                    let had = r.held.iter().any(|h| h.0 == e.d1);
                    r.held.retain(|h| h.0 != e.d1);
                    // (a stolen note's release was sent already)
                    if had {
                        self.emit_all(id, ev, clock, out, path);
                    }
                } else {
                    self.emit_all(id, ev, clock, out, path);
                }
            }
            Kind::Instrument => {
                if let Some(e) = instrument_apply(&o, "", ev.ev) {
                    self.instrument_out(&o, e, clock, out, path, ev.from_pi);
                }
            }
            Kind::MultiInstrument => {
                let pre = format!("s{}", ev.ev.ch + 1);
                if let Some(mut e) = instrument_apply(&o, &pre, ev.ev) {
                    e.ch = ev.ev.ch;
                    self.instrument_out(&o, e, clock, out, path, ev.from_pi);
                }
            }
            Kind::MappedInstrument => {
                let mut e = ev.ev;
                let mut cable = 0usize;
                if e.is_note() || e.st == 0xA0 {
                    let row = o.map_row(e.d1);
                    e.d1 = row.out;
                    if e.is_note_on() {
                        e.d2 = (e.d2 as i32 + row.vel).clamp(1, 127) as u8;
                    }
                    if row.ch > 0 {
                        e.ch = row.ch - 1;
                    } else if o.p("ch") > 0 {
                        e.ch = (o.p("ch") - 1) as u8;
                    }
                    cable = row.cable.max(1) as usize - 1;
                    if cable >= 16 {
                        return;
                    }
                } else if o.p("ch") > 0 {
                    e.ch = (o.p("ch") - 1) as u8;
                }
                if !o.port.is_empty() {
                    out.push(Out::Port { port: o.port.clone(), ev: e });
                }
                let t = Tagged { ev: e, from_pi: ev.from_pi };
                if e.is_note() || e.st == 0xA0 {
                    self.emit_nth(id, cable, t, clock, out, path);
                } else {
                    self.emit_all(id, t, clock, out, path);
                }
            }
            Kind::TouchTracks => out.push(Out::Touch { obj: id, ev: ev.ev }),
            Kind::MidiClick | Kind::GmMixer => {
                if !o.port.is_empty() {
                    out.push(Out::Port { port: o.port.clone(), ev: ev.ev });
                }
                self.emit_all(id, ev, clock, out, path);
            }
            Kind::ChannelStrip => out.push(Out::Track { track: o.p("trk").max(0) as usize, ev: ev.ev }),
            Kind::Alias => {
                // behaves as its original, rechannelized, through the alias's own cables
                let Some(orig) = self.obj(o.p("ref") as Id).cloned() else { return };
                if orig.kind == Kind::Alias {
                    return;
                }
                let mut e = ev.ev;
                if o.p("ch") > 0 && e.st != META {
                    e.ch = (o.p("ch") - 1) as u8;
                }
                let mut stand = orig.clone();
                stand.id = id;
                if stand.kind == Kind::Fader {
                    stand.params.insert("val".into(), o.p("val"));
                    if o.p("ch") > 0 {
                        stand.params.insert("in_ch".into(), o.p("ch"));
                        stand.params.insert("out_ch".into(), o.p("ch"));
                    }
                    self.fader_in(&stand, Tagged { ev: e, from_pi: ev.from_pi }, clock, out, path);
                    return;
                }
                match stand.kind {
                    Kind::Instrument => {
                        if let Some(e) = instrument_apply(&stand, "", e) {
                            self.instrument_out(&stand, e, clock, out, path, ev.from_pi);
                        }
                    }
                    Kind::ChannelStrip => out.push(Out::Track { track: stand.p("trk").max(0) as usize, ev: e }),
                    _ => self.emit_all(id, Tagged { ev: e, from_pi: ev.from_pi }, clock, out, path),
                }
            }
        }
    }

    fn instrument_out(&mut self, o: &Obj, e: Ev, clock: Clock, out: &mut Vec<Out>, path: &[Id], from_pi: bool) {
        let delay = o.p("delay");
        if !o.port.is_empty() {
            let x = Out::Port { port: o.port.clone(), ev: e };
            if delay > 0 {
                self.later(clock.ms + delay as f64, Pend::Out(x));
            } else {
                out.push(x);
            }
        }
        self.emit_all(o.id, Tagged { ev: e, from_pi }, clock, out, path);
    }

    /// Meta events for faders (96 min, 97 max, 98 set without sending, 99 Bang!) and
    /// transformers (122 map value, 123 map position, 124/125 condition, 126/127 operation).
    /// True when the object took it.
    fn meta_control(&mut self, o: &Obj, ev: Ev, clock: Clock, out: &mut Vec<Out>, path: &[Id]) -> bool {
        let v = ev.d2 as i32;
        match (o.kind, ev.d1) {
            (Kind::Fader, 96) => self.obj_mut(o.id).unwrap().params.insert("min".into(), v).is_some() || true,
            (Kind::Fader, 97) => self.obj_mut(o.id).unwrap().params.insert("max".into(), v).is_some() || true,
            (Kind::Fader, 98) => self.obj_mut(o.id).unwrap().params.insert("val".into(), v).is_some() || true,
            (Kind::Fader, 99) => {
                // Bang!: resend; 127 / 125 increment (with / without rollover), 123 / 121 decrement;
                // one less passes the bang on to connected faders
                let (lo, hi, cur) = (o.p("min"), o.p("max"), o.p("val"));
                let pass = matches!(v, 126 | 124 | 122 | 120);
                let vv = if pass { v + 1 } else { v };
                let nv = match vv {
                    127 => if cur >= hi { lo } else { cur + 1 },
                    125 => (cur + 1).min(hi),
                    123 => if cur <= lo { hi } else { cur - 1 },
                    121 => (cur - 1).max(lo),
                    _ => cur,
                };
                self.obj_mut(o.id).unwrap().params.insert("val".into(), nv);
                let mut f = o.clone();
                f.params.insert("val".into(), nv);
                for e in fader_output(&f, nv, false) {
                    self.emit_all(o.id, Tagged { ev: e, from_pi: false }, clock, out, path);
                }
                if pass {
                    self.emit_all(o.id, Tagged { ev: Ev::meta(99, v as u8), from_pi: false }, clock, out, path);
                }
                true
            }
            (Kind::Transformer, 122..=127) => {
                let t = self.obj_mut(o.id).unwrap();
                match ev.d1 {
                    123 => {
                        t.params.insert("mappos".into(), v);
                    }
                    122 => {
                        let pos = t.p("mappos").clamp(0, 127) as usize;
                        let mut m = transformer_map(t);
                        m[pos] = v as u8;
                        t.texts.insert("map".into(), m.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","));
                    }
                    124 | 125 => {
                        // condition bottom (124) / top (125) values of conditions not All
                        for k in ["c_ch", "c_d1", "c_d2"] {
                            if t.p(k) != 0 {
                                let key = if ev.d1 == 125 { format!("{k}a") } else { format!("{k}b") };
                                t.params.insert(key, v);
                            }
                        }
                    }
                    _ => {
                        for k in ["o_ch", "o_d1", "o_d2"] {
                            if t.p(k) != 0 {
                                let key = if ev.d1 == 127 { format!("{k}a") } else { format!("{k}b") };
                                t.params.insert(key, v);
                            }
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn fader_in(&mut self, o: &Obj, ev: Tagged, clock: Clock, out: &mut Vec<Out>, path: &[Id]) {
        let id = o.id;
        // Feedback off: an event that passed this fader already stops here
        if !o.on("feedback") && path[..path.len() - 1].contains(&id) {
            return;
        }
        let filter = o.p("filter");
        // Thru: blocks what comes from the Physical Input (no MIDI feedback)
        if filter == 4 && ev.from_pi {
            return;
        }
        if filter == 3 {
            return;
        }
        let e = ev.ev;
        let matched = def_matches(o.p("in_t"), o.p("in_ch"), o.p("in_d1"), e);
        // a cable switcher: matching events move it; others pass to the current outlet
        if o.is_cable_switcher() {
            let n = self.cables_from(id).len() as i32;
            if let Some(v) = matched {
                let cur = o.p("val");
                let nv = match v {
                    127 => if cur + 1 > n { 0 } else { cur + 1 },
                    126 => if cur - 1 < 0 { n } else { cur - 1 },
                    v => v.min(n),
                };
                self.obj_mut(id).unwrap().params.insert("val".into(), nv);
                if !o.on("feedback") {
                    return;
                }
            }
            let pos = self.obj(id).map_or(0, |x| x.p("val")) as usize;
            self.emit_nth(id, pos, ev, clock, out, path);
            return;
        }
        // an alias assigner: the alias on the top cable takes the original on cable `value`
        if o.is_alias_assigner() {
            if let Some(v) = matched {
                let cs = self.cables_from(id);
                if let (Some(alias), Some(orig)) = (cs.first(), cs.get(v as usize)) {
                    let (a, r) = (alias.to, orig.to);
                    if let Some(x) = self.obj_mut(a) {
                        if x.kind == Kind::Alias {
                            x.params.insert("ref".into(), r as i32);
                        }
                    }
                }
                self.obj_mut(id).unwrap().params.insert("val".into(), v);
            }
            return;
        }
        match (matched, filter) {
            (Some(v), 2) => {
                // Match: blocks matching events
                let _ = v;
            }
            (Some(v), _) => {
                self.obj_mut(id).unwrap().params.insert("val".into(), v);
                if o.p("style") == 5 && o.on("mute") {
                    return;
                }
                let mut f = o.clone();
                f.params.insert("val".into(), v);
                if f.p("out_t") == 7 {
                    self.sysex_out(&f, v, ev.from_pi, clock, out, path);
                    return;
                }
                for x in fader_output(&f, v, true) {
                    self.emit_all(id, Tagged { ev: x, from_pi: ev.from_pi }, clock, out, path);
                    if x.st == META && matches!(x.d1, 49 | 50 | 51 | 52 | 100) {
                        out.push(Out::Meta { num: x.d1, val: x.d2 });
                    }
                }
            }
            // Other blocks non-matching events; Off, Match, Shot and 14 Bit pass them
            (None, 1) => {}
            (None, _) => self.emit_all(id, ev, clock, out, path),
        }
    }

    fn transformer_in(&mut self, o: &Obj, ev: Tagged, clock: Clock, out: &mut Vec<Out>, path: &[Id]) {
        let id = o.id;
        let e = ev.ev;
        let mode = o.p("mode");
        // (meta events controlling transformers never get here; others are processed)
        if mode == 6 {
            let r = self.run.entry(id).or_default();
            r.alt = !r.alt;
            let k = if r.alt { 0 } else { 1 };
            self.emit_nth(id, k, ev, clock, out, path);
            return;
        }
        let m = transformer_matches(o, e);
        let t = |x: Ev| Tagged { ev: x, from_pi: ev.from_pi };
        match (mode, m) {
            (0, true) => {
                let x = self.transformer_apply(o, e);
                self.emit_all(id, t(x), clock, out, path);
            }
            (0, false) | (2, false) => self.emit_all(id, ev, clock, out, path),
            (1, true) => {
                let x = self.transformer_apply(o, e);
                self.emit_all(id, t(x), clock, out, path);
            }
            (3, true) => {
                let x = self.transformer_apply(o, e);
                self.emit_all(id, ev, clock, out, path);
                self.emit_all(id, t(x), clock, out, path);
            }
            (4, true) => {
                let x = self.transformer_apply(o, e);
                self.emit_all(id, t(x), clock, out, path);
                self.emit_all(id, ev, clock, out, path);
            }
            (3, false) | (4, false) => self.emit_all(id, ev, clock, out, path),
            (5, true) => {
                let x = self.transformer_apply(o, e);
                self.emit_nth(id, 0, t(x), clock, out, path);
            }
            (5, false) => self.emit_nth(id, 1, ev, clock, out, path),
            // track automation splitter: matches to the top cable's object as automation
            (8, true) => self.emit_nth(id, 0, ev, clock, out, path),
            (8, false) => self.emit_nth(id, 1, ev, clock, out, path),
            // SysEx mapper: matching events set a byte of the SysEx message (data byte 1 the
            // position, data byte 2 the value); channel 1 sends the changed message, 2 only
            // changes it, 3 sends it unchanged
            (7, true) => {
                let mut bytes = sysex_map(o);
                let unchanged = bytes.clone();
                match e.ch {
                    0 | 1 => {
                        if let Some(b) = bytes.get_mut(e.d1 as usize) {
                            *b = e.d2 & 127;
                        }
                        if let Some(x) = self.obj_mut(id) {
                            x.texts.insert("sxmap".into(), bytes.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(" "));
                        }
                    }
                    _ => {}
                }
                let send = match e.ch {
                    0 => Some(bytes),
                    2 => Some(unchanged),
                    _ => None,
                };
                if let Some(b) = send {
                    let mut m = vec![0xF0];
                    m.extend(&b);
                    if let Some(c) = crate::sysexfader::checksum(o.p("sx_sum").max(0) as usize, &b) {
                        m.push(c);
                    }
                    m.push(0xF7);
                    for port in self.ports_downstream(id) {
                        out.push(Out::Raw { port, bytes: m.clone() });
                    }
                }
            }
            // (Filter non-matching: events that don't control the message stop here)
            (7, false) if o.on("sx_filter") => {}
            (7, false) => self.emit_all(id, ev, clock, out, path),
            _ => {}
        }
    }

    fn transformer_apply(&mut self, o: &Obj, e: Ev) -> Ev {
        let mut x = e;
        if o.p("o_st") == 1 {
            if let Some(st) = status_of(o.p("o_stv")) {
                // (a note-off turned into another type keeps its data)
                x.st = st;
            }
        }
        let map = transformer_map(o);
        let ch = apply_op(o, "o_ch", e.ch as i64 + 1, &mut self.rng, &map).clamp(1, 16);
        x.ch = (ch - 1) as u8;
        let pb = e.st == 0xE0;
        let d1 = apply_op(o, "o_d1", e.d1 as i64, &mut self.rng, &map);
        let d2 = apply_op(o, "o_d2", e.d2 as i64, &mut self.rng, &map);
        if pb && o.p("o_d1") != 0 && o.p("o_d2") == 0 {
            // (14-bit pitch bend: changing byte 1 carries into byte 2)
            let v = ((e.d2 as i64) << 7 | e.d1 as i64) + (d1 - e.d1 as i64);
            let v = v.clamp(0, 16383);
            x.d1 = (v & 127) as u8;
            x.d2 = (v >> 7) as u8;
        } else {
            x.d1 = d1.clamp(0, 127) as u8;
            x.d2 = d2.clamp(0, 127) as u8;
        }
        x
    }

    fn arp_in(&mut self, o: &Obj, ev: Tagged, clock: Clock, out: &mut Vec<Out>, path: &[Id]) {
        let id = o.id;
        let e = ev.ev;
        // the arpeggiator's parameters by controller: Controller Base and the nine after it
        if e.st == 0xB0 && o.p("cbase") > 0 {
            let b = o.p("cbase");
            let k = e.d1 as i32 - b;
            if (0..10).contains(&k) {
                let keys = ["dir", "vel", "klo", "khi", "res", "len", "snap", "rep", "oct", "cresc"];
                let key = keys[k as usize];
                let v = e.d2 as i32;
                let nv = match key {
                    "dir" => v * ARP_DIRS.len() as i32 / 128,
                    "res" | "len" | "snap" => v * ARP_RES.len() as i32 / 128,
                    "rep" => (v >= 64) as i32,
                    "oct" => 1 + v * 10 / 128,
                    "cresc" => v - 64,
                    "vel" => v,
                    _ => v,
                };
                self.obj_mut(id).unwrap().set(key, nv);
                return;
            }
        }
        if !e.is_note() || o.p("res") == 0 {
            // (not arpeggiated: other events, or Resolution None)
            self.emit_all(id, ev, clock, out, path);
            return;
        }
        if (e.d1 as i32) < o.p("klo") || (e.d1 as i32) > o.p("khi") {
            self.emit_all(id, ev, clock, out, path);
            return;
        }
        let first = self.run.get(&id).is_none_or(|r| r.held.is_empty());
        self.note_held(id, e);
        let r = self.run.entry(id).or_default();
        if e.is_note_on() && first {
            r.arp_step = 0;
            r.arp_up = true;
            r.arp_rep = 0;
            // Snap to: wait for the next division of the song while playing
            let snap = o.p("snap");
            r.arp_next = if snap > 0 && clock.playing {
                let d = div_ticks(snap as usize - 1) as f64;
                let next = (clock.song / d).ceil() * d;
                clock.ms + (next - clock.song) * clock.ms_per_tick()
            } else {
                clock.ms
            };
            // (Auto: the direction from the first two notes)
        }
        let _ = out;
        self.arp_tick(id, clock, out);
    }

    fn arp_tick(&mut self, id: Id, clock: Clock, out: &mut Vec<Out>) {
        let Some(o) = self.obj(id).cloned() else { return };
        // notes whose length ended
        let ended: Vec<(u8, u8)> = {
            let r = self.run.entry(id).or_default();
            let (done, keep): (Vec<_>, Vec<_>) = r.arp_playing.iter().partition(|p| p.0 <= clock.ms);
            r.arp_playing = keep;
            done.into_iter().map(|p| (p.1, p.2)).collect()
        };
        for (pitch, ch) in ended {
            self.emit_all(id, Tagged { ev: Ev::new(0x80, ch, pitch, 0), from_pi: false }, clock, out, &[id]);
        }
        let res = o.p("res");
        if res == 0 {
            return;
        }
        let step_ms = div_ticks(res as usize - 1) as f64 * clock.ms_per_tick();
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 16 {
                break;
            }
            let r = self.run.entry(id).or_default();
            if r.held.is_empty() || r.arp_next > clock.ms {
                break;
            }
            // the chord, held notes sorted, spread over the octaves
            let mut notes: Vec<(u8, u8, u8, u64)> = r.held.clone();
            notes.sort_by_key(|n| n.0);
            let oct = o.p("oct").max(1);
            let mut seq: Vec<(u8, u8, u8)> = Vec::new();
            for k in 0..oct {
                for n in &notes {
                    let p = n.0 as i32 + 12 * k;
                    if p <= 127 {
                        seq.push((p as u8, n.1, n.2));
                    }
                }
            }
            let len = seq.len();
            if len == 0 {
                break;
            }
            let dir = o.p("dir");
            // Auto: down when the second note played was lower than the first
            let auto_down = dir == 3 && {
                let mut by_order = notes.clone();
                by_order.sort_by_key(|n| n.3);
                by_order.len() >= 2 && by_order[1].0 < by_order[0].0
            };
            let picks: Vec<(u8, u8, u8)> = match dir {
                6 => seq.clone(),
                _ => {
                    let i = r.arp_step;
                    let cycle = match dir {
                        2 => (2 * len).max(1),
                        4 => (2 * len).saturating_sub(2).max(1),
                        _ => len,
                    };
                    let k = i % cycle;
                    let idx = match dir {
                        1 => len - 1 - k,
                        2 => if k < len { k } else { 2 * len - 1 - k },
                        3 => if auto_down { len - 1 - k } else { k },
                        4 => if k < len { k } else { 2 * len - 2 - k },
                        5 => (self.rng.next() % len as u64) as usize,
                        _ => k,
                    };
                    vec![seq[idx.min(len - 1)]]
                }
            };
            let r = self.run.entry(id).or_default();
            let wrapped = dir != 6 && r.arp_step > 0 && r.arp_step % {
                match dir {
                    2 => (2 * len).max(1),
                    4 => (2 * len).saturating_sub(2).max(1),
                    _ => len,
                }
            } == 0;
            if wrapped || dir == 6 && r.arp_step > 0 {
                r.arp_rep += 1;
                if !o.on("rep") {
                    // (Repeat off: once through)
                    r.arp_next = f64::INFINITY;
                    break;
                }
            }
            let rep = r.arp_rep;
            let len_ms = match o.p("len") {
                0 => step_ms * 0.9,
                l => div_ticks(l as usize - 1) as f64 * clock.ms_per_tick(),
            };
            r.arp_step += 1;
            let at = r.arp_next;
            r.arp_next += step_ms;
            for (p, v0, ch) in picks {
                let v = match o.p("vel") {
                    0 => v0 as i32,
                    128 => 1 + (self.rng.next() % v0.max(1) as u64) as i32,
                    x => x,
                } + o.p("cresc") * rep;
                let v = v.clamp(1, 127) as u8;
                self.run.entry(id).or_default().arp_playing.push((at + len_ms, p, ch));
                self.emit_all(id, Tagged { ev: Ev::note(ch, p, v), from_pi: false }, clock, out, &[id]);
            }
        }
        // released chord: the arpeggio stops
        let r = self.run.entry(id).or_default();
        if r.held.is_empty() {
            r.arp_step = 0;
            r.arp_rep = 0;
        }
    }

    /// The arpeggiator's position resets on a cycle jump (the app calls this).
    pub fn cycle_jump(&mut self) {
        for (id, r) in self.run.iter_mut() {
            if self.objects.iter().any(|o| o.id == *id && o.kind == Kind::Arpeggiator) {
                r.arp_step = 0;
                r.arp_rep = 0;
            }
        }
    }

    fn chord_in(&mut self, o: &Obj, ev: Tagged, clock: Clock, out: &mut Vec<Out>, path: &[Id]) {
        let id = o.id;
        let e = ev.ev;
        if !e.is_note() || (e.d1 as i32) < o.p("klo") || (e.d1 as i32) > o.p("khi") {
            self.emit_all(id, ev, clock, out, path);
            return;
        }
        // Key transposes the whole map: with Key 1, C♯ plays what C was set to, a semitone up
        let key = o.p("key");
        let pc = (e.d1 as i32 - key).rem_euclid(12) as usize;
        let chord = o.chord(pc);
        if chord.is_empty() {
            self.emit_all(id, ev, clock, out, path);
            return;
        }
        let split = o.on("split");
        for (k, off) in chord.iter().enumerate() {
            let p = e.d1 as i32 + off + o.p("trans");
            if !(0..=127).contains(&p) {
                continue;
            }
            let mut x = e;
            x.d1 = p as u8;
            if o.p("ch") > 0 {
                x.ch = (o.p("ch") - 1) as u8;
            }
            let t = Tagged { ev: x, from_pi: ev.from_pi };
            if split {
                self.emit_nth(id, k, t, clock, out, path);
            } else {
                self.emit_all(id, t, clock, out, path);
            }
        }
    }

    /// The MIDI click object: a click (0 bar, 1 beat, 2 division) to its port and cables.
    pub fn click(&mut self, which: u8, recording: bool, clock: Clock) -> Vec<Out> {
        let Some(id) = self.find(Kind::MidiClick) else { return Vec::new() };
        let o = self.obj(id).unwrap().clone();
        if !(if recording { o.on("rec") } else { o.on("play") }) || o.port.is_empty() && self.cables_from(id).is_empty() {
            return Vec::new();
        }
        let pre = ["bar", "beat", "div"][which.min(2) as usize];
        if !o.on(pre) {
            return Vec::new();
        }
        let (ch, n, v) = ((o.p(&format!("{pre}_ch")) - 1).max(0) as u8, o.p(&format!("{pre}_n")) as u8, o.p(&format!("{pre}_v")) as u8);
        let mut out = self.inject(id, Ev::note(ch, n, v), clock);
        // (a short click: its note off 50 ms later)
        self.later(clock.ms + 50.0, Pend::Emit { from: id, outlet: None, ev: Ev::new(0x80, ch, n, 0) });
        if !o.port.is_empty() {
            self.later(clock.ms + 50.0, Pend::Out(Out::Port { port: o.port.clone(), ev: Ev::new(0x80, ch, n, 0) }));
        }
        out.retain(|_| true);
        out
    }

    // ---- saving ----

    /// The Environment as text (one line per layer, object and cable).
    pub fn to_text(&self) -> String {
        let mut s = String::from("bramble-environment 1\n");
        for (i, l) in self.layers.iter().enumerate().skip(2) {
            s += &format!("layer\t{i}\t{}\n", esc(l));
        }
        for o in &self.objects {
            let params: Vec<String> = o.params.iter().map(|(k, v)| format!("{k}={v}")).collect();
            s += &format!("obj\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:06x}\t{}\t{}\t{}\t{}\n", o.id, o.kind.tag(), esc(&o.name), o.layer, o.x, o.y, o.w, o.h, o.color, o.icon, o.assignable as u8, esc(&o.port), params.join(","));
            for (k, v) in &o.texts {
                s += &format!("text\t{}\t{}\t{}\n", o.id, k, esc(v));
            }
        }
        for c in &self.cables {
            s += &format!("cable\t{}\t{}\t{}\n", c.from, c.outlet, c.to);
        }
        s
    }

    pub fn from_text(t: &str) -> Option<Env> {
        let mut lines = t.lines();
        if !lines.next()?.starts_with("bramble-environment") {
            return None;
        }
        let mut e = Env { layers: vec!["All Objects".into(), "Global Objects".into()], objects: Vec::new(), cables: Vec::new(), next_id: 1, run: BTreeMap::new(), pending: Vec::new(), rng: Rng(0x5eed) };
        for l in lines {
            let f: Vec<&str> = l.split('\t').collect();
            match f.first().copied() {
                Some("layer") if f.len() >= 3 => e.layers.push(unesc(f[2])),
                Some("obj") if f.len() >= 14 => {
                    let Some(kind) = Kind::from_tag(f[2]) else { continue };
                    let id: Id = f[1].parse().ok()?;
                    let mut o = Obj::new(id, kind, f[4].parse().unwrap_or(2), f[5].parse().unwrap_or(0.0), f[6].parse().unwrap_or(0.0));
                    o.name = unesc(f[3]);
                    o.w = f[7].parse().unwrap_or(o.w);
                    o.h = f[8].parse().unwrap_or(o.h);
                    o.color = u32::from_str_radix(f[9], 16).unwrap_or(0xd0d0d0);
                    o.icon = f[10].parse().unwrap_or(0);
                    o.assignable = f[11] == "1";
                    o.port = unesc(f[12]);
                    for kv in f[13].split(',').filter(|x| !x.is_empty()) {
                        if let Some((k, v)) = kv.split_once('=') {
                            if let Ok(v) = v.parse() {
                                o.params.insert(k.to_string(), v);
                            }
                        }
                    }
                    e.next_id = e.next_id.max(id + 1);
                    e.objects.push(o);
                }
                Some("text") if f.len() >= 4 => {
                    let id: Id = f[1].parse().unwrap_or(0);
                    if let Some(o) = e.obj_mut(id) {
                        o.texts.insert(f[2].to_string(), unesc(f[3]));
                    }
                }
                Some("cable") if f.len() >= 4 => {
                    if let (Ok(a), Ok(n), Ok(b)) = (f[1].parse(), f[2].parse(), f[3].parse()) {
                        e.cables.push(Cable { from: a, outlet: n, to: b });
                    }
                }
                _ => {}
            }
        }
        Some(e)
    }

    // ---- exchanging Environments ----

    /// Options ▸ Import Environment ▸ Layer: the source layer with its objects, inserted at the
    /// same layer position (existing layers shift); unique objects replace ours.
    pub fn import_layer(&mut self, src: &Env, layer: usize) {
        if layer < 2 || layer >= src.layers.len() {
            return;
        }
        let at = layer.min(self.layers.len());
        self.layers.insert(at, src.layers[layer].clone());
        for o in self.objects.iter_mut().filter(|o| o.layer >= at) {
            o.layer += 1;
        }
        let ids: Vec<Id> = src.objects.iter().filter(|o| o.layer == layer).map(|o| o.id).collect();
        self.import_objects(src, &ids, |_| at);
    }

    /// Options ▸ Import Environment ▸ Merge: every source object added, on its own layer (layers
    /// we don't have are added by name).
    pub fn merge(&mut self, src: &Env) {
        let mut layer_map = BTreeMap::new();
        for (i, l) in src.layers.iter().enumerate().skip(1) {
            let at = match self.layers.iter().position(|x| x == l) {
                Some(k) => k,
                None => {
                    self.layers.push(l.clone());
                    self.layers.len() - 1
                }
            };
            layer_map.insert(i, at);
        }
        let ids: Vec<Id> = src.objects.iter().map(|o| o.id).collect();
        self.import_objects(src, &ids, |l| *layer_map.get(&l).unwrap_or(&2));
    }

    /// Options ▸ Import Environment ▸ Update: only source objects we don't have (same kind and
    /// name), keeping ours with their cables.
    pub fn update_from(&mut self, src: &Env) {
        let ids: Vec<Id> = src.objects.iter().filter(|s| !self.objects.iter().any(|o| o.kind == s.kind && o.name == s.name)).map(|o| o.id).collect();
        let layers = self.layers.clone();
        let map = |l: usize| src.layers.get(l).and_then(|n| layers.iter().position(|x| x == n)).unwrap_or(2);
        self.import_objects(src, &ids, map);
    }

    /// Options ▸ Import Environment ▸ Total Replace.
    pub fn total_replace(&mut self, src: &Env) {
        let run = std::mem::take(&mut self.run);
        *self = src.clone();
        self.run = run;
        self.run.clear();
    }

    /// Replace by Name (by_port false) / by Port MIDI/Channel: our objects swapped for the
    /// matching source objects' settings, keeping our cables and place.
    pub fn replace_matching(&mut self, src: &Env, by_port: bool) {
        for o in self.objects.iter_mut() {
            let m = src.objects.iter().find(|s| s.kind == o.kind && if by_port { s.port == o.port && s.p("ch") == o.p("ch") } else { s.name == o.name });
            if let Some(s) = m {
                let (id, layer, x, y) = (o.id, o.layer, o.x, o.y);
                *o = s.clone();
                o.id = id;
                o.layer = layer;
                o.x = x;
                o.y = y;
            }
        }
    }

    fn import_objects(&mut self, src: &Env, ids: &[Id], layer_of: impl Fn(usize) -> usize) {
        // (importing an object brings the objects cabled from it)
        let all = src.used_closure(ids);
        let mut map = BTreeMap::new();
        for &sid in &all {
            let Some(s) = src.obj(sid) else { continue };
            if s.kind.unique() {
                // Physical Input / Sequencer Input / click replace ours (cables into ours go)
                if let Some(old) = self.find(s.kind) {
                    self.delete(&[old]);
                }
            }
            let nid = self.next_id;
            self.next_id += 1;
            let mut n = s.clone();
            n.id = nid;
            n.layer = layer_of(s.layer);
            self.objects.push(n);
            map.insert(sid, nid);
        }
        for c in &src.cables {
            if let (Some(&a), Some(&b)) = (map.get(&c.from), map.get(&c.to)) {
                self.cables.push(Cable { from: a, outlet: c.outlet, to: b });
            }
        }
    }
}

/// An event on its way, and whether it came from the Physical Input (fader Filter Thru).
#[derive(Clone, Copy, Debug)]
struct Tagged {
    ev: Ev,
    from_pi: bool,
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unesc(s: &str) -> String {
    let mut o = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('t') => o.push('\t'),
                Some('n') => o.push('\n'),
                Some(x) => o.push(x),
                None => {}
            }
        } else {
            o.push(c);
        }
    }
    o
}

/// The status of a definition type (DEF_TYPES index).
fn def_status(t: i32) -> u8 {
    match t {
        0 => 0x90,
        1 => 0xA0,
        2 => 0xB0,
        3 => 0xC0,
        4 => 0xD0,
        5 => 0xE0,
        _ => META,
    }
}

/// Does an event match a fader definition? Its value (data 2, or data 1 for program and
/// pressure, 14-bit for pitch bend) when it does.
fn def_matches(t: i32, ch: i32, d1: i32, e: Ev) -> Option<i32> {
    let st = def_status(t);
    let est = if e.st == 0x80 { 0x90 } else { e.st };
    if est != st {
        return None;
    }
    if st != META && e.ch as i32 + 1 != ch {
        return None;
    }
    match st {
        0x90 | 0xA0 | 0xB0 | META => (e.d1 as i32 == d1).then_some(if e.st == 0x80 { 0 } else { e.d2 as i32 }),
        0xC0 | 0xD0 => Some(e.d1 as i32),
        // pitch bend: the MSB is the value (the fader's 14 Bit filter: both bytes)
        0xE0 => Some(e.d2 as i32),
        _ => None,
    }
}

/// The GM mixer's Reset: GM On, GS Reset or XG System On for its standard, then Reset All
/// Controllers on each of its 16 channels.
pub fn gm_reset(std: i32) -> Vec<Vec<u8>> {
    let mut v = vec![match std {
        1 => vec![0xF0, 0x41, 0x10, 0x42, 0x12, 0x40, 0x00, 0x7F, 0x00, 0x41, 0xF7],
        2 => vec![0xF0, 0x43, 0x10, 0x4C, 0x00, 0x00, 0x7E, 0x00, 0xF7],
        _ => vec![0xF0, 0x7E, 0x7F, 0x09, 0x01, 0xF7],
    }];
    for ch in 0..16u8 {
        v.push(vec![0xB0 | ch, 121, 0]);
    }
    v
}

/// The GS / XG effect controls: reverb and chorus program, level and (reverb) time.
pub const GS_REVERBS: &[&str] = &["Room 1", "Room 2", "Room 3", "Hall 1", "Hall 2", "Plate", "Delay", "Panning Delay"];
pub const GS_CHORUSES: &[&str] = &["Chorus 1", "Chorus 2", "Chorus 3", "Chorus 4", "Feedback Chorus", "Flanger", "Short Delay", "Short Delay (FB)"];
pub const XG_REVERBS: &[&str] = &["No Effect", "Hall 1", "Hall 2", "Room 1", "Room 2", "Room 3", "Stage 1", "Stage 2", "Plate", "White Room", "Tunnel", "Basement"];
pub const XG_CHORUSES: &[&str] = &["No Effect", "Chorus 1", "Chorus 2", "Chorus 3", "Celeste 1", "Celeste 2", "Flanger 1", "Flanger 2"];
/// (what: 0 reverb program, 1 chorus program, 2 reverb level, 3 chorus level, 4 reverb time)
pub fn gsxg_effect(std: i32, what: u8, v: u8) -> Option<Vec<u8>> {
    let v = v & 127;
    match std {
        1 => {
            let addr = [0x30, 0x38, 0x33, 0x3A, 0x34][what.min(4) as usize];
            let body = [0x40u8, 0x01, addr, v];
            let sum = ((128 - body.iter().map(|b| *b as u32).sum::<u32>() % 128) % 128) as u8;
            Some([&[0xF0, 0x41, 0x10, 0x42, 0x12][..], &body, &[sum, 0xF7]].concat())
        }
        2 => {
            let (lo, data): (u8, Vec<u8>) = match what {
                // XG effect types: MSB type, LSB 0 (Hall 1 = 01, Room 1 = 02 … as listed)
                0 => (0x00, vec![xg_reverb(v), 0]),
                1 => (0x20, vec![if v == 0 { 0 } else { 0x40 + (v - 1) / 2 }, (v.saturating_sub(1)) % 2]),
                2 => (0x0C, vec![v]),
                3 => (0x2C, vec![v]),
                _ => (0x02, vec![v.min(69)]),
            };
            Some([&[0xF0, 0x43, 0x10, 0x4C, 0x02, 0x01, lo][..], &data, &[0xF7]].concat())
        }
        _ => None,
    }
}

fn xg_reverb(k: u8) -> u8 {
    [0x00, 0x01, 0x01, 0x02, 0x02, 0x02, 0x03, 0x03, 0x04, 0x10, 0x11, 0x13].get(k as usize).copied().unwrap_or(0)
}

/// A SysEx mapper transformer's message (without F0 / F7): its Length, the fixed byte values.
pub fn sysex_map(o: &Obj) -> Vec<u8> {
    let n = if o.params.contains_key("sx_len") { o.p("sx_len").clamp(1, 64) } else { 8 } as usize;
    let mut v: Vec<u8> = o.texts.get("sxmap").map(|t| t.split_whitespace().filter_map(|x| x.parse::<u8>().ok()).map(|b| b & 127).collect()).unwrap_or_default();
    v.resize(n, 0);
    v
}

/// What a fader sends for a value: its Output definition (Note On sends on and off at once;
/// 14 Bit sends pitch bend's two bytes or a controller's MSB and LSB, 32 above).
pub fn fader_output(o: &Obj, v: i32, _remote: bool) -> Vec<Ev> {
    let t = o.p("out_t");
    let ch = (o.p("out_ch") - 1).clamp(0, 15) as u8;
    let d1 = o.p("out_d1").clamp(0, 127) as u8;
    let fourteen = o.p("filter") == 6;
    let v7 = v.clamp(0, 127) as u8;
    match t {
        0 => vec![Ev::note(ch, d1, v7.max(1)), Ev::new(0x80, ch, d1, 0)],
        1 => vec![Ev::new(0xA0, ch, d1, v7)],
        2 if fourteen => {
            let v = v.clamp(0, 16383);
            vec![Ev::new(0xB0, ch, d1, (v >> 7) as u8), Ev::new(0xB0, ch, d1.saturating_add(32), (v & 127) as u8)]
        }
        2 => vec![Ev::new(0xB0, ch, d1, v7)],
        3 => vec![Ev::new(0xC0, ch, v7, 0)],
        4 => vec![Ev::new(0xD0, ch, v7, 0)],
        5 if fourteen => {
            let v = v.clamp(0, 16383);
            vec![Ev::new(0xE0, ch, (v & 127) as u8, (v >> 7) as u8)]
        }
        5 => vec![Ev::new(0xE0, ch, d1, v7)],
        6 => match d1 {
            // (cable switchers and alias assigners send nothing)
            48 | 46 => Vec::new(),
            n => vec![Ev::meta(n, v7)],
        },
        _ => Vec::new(),
    }
}

/// The fader's value shown as Value as says (Pan: 64 is 0; bpm adds 50).
pub fn value_text(o: &Obj, v: i32) -> String {
    match o.p("as") {
        1 => format!("{}", v - 64),
        2 => format!("{:.0} Hz", 20.0 * 1000f32.powf(v as f32 / 127.0)),
        3 => format!("{:+}", (v - 64) / 12),
        4 => format!("{:.1} dB", (v as f32 / 127.0).max(1e-4).log10() * 40.0),
        5 => format!("{} ms", v * 10),
        6 => format!("{}", v + 50),
        7 => {
            let names = o.names();
            let lo = o.p("min");
            let hi = o.p("max");
            if v < lo {
                "---".into()
            } else if v > hi {
                if lo == 0 && hi == 1 { names.get(1).cloned().unwrap_or_else(|| "1".into()) } else { "+++".into() }
            } else {
                names.get((v - lo) as usize).cloned().unwrap_or_else(|| v.to_string())
            }
        }
        _ => v.to_string(),
    }
}

/// A standard instrument's processing of an event: channel, transposition, velocity, key and
/// velocity limits (None: not played). `pre`: a multi-instrument subchannel's key prefix.
fn instrument_apply(o: &Obj, pre: &str, e: Ev) -> Option<Ev> {
    let p = |k: &str| if pre.is_empty() { o.p(k) } else { o.p(&format!("{pre}.{k}")) };
    let mut x = e;
    if pre.is_empty() && p("ch") > 0 && e.st != META {
        x.ch = (p("ch") - 1) as u8;
    }
    if x.is_note() || x.st == 0xA0 {
        let pitch = e.d1 as i32 + p("trans");
        if !(0..=127).contains(&pitch) || pitch < p("klo") || pitch > p("khi") {
            return None;
        }
        x.d1 = pitch as u8;
        if x.is_note_on() {
            let v = (e.d2 as i32 + p("velo")).clamp(1, 127);
            if v < p("vlo") || v > p("vhi") {
                return None;
            }
            x.d2 = v as u8;
        }
    }
    if e.st == META {
        return None;
    }
    Some(x)
}

/// What an instrument sends when its Program, Volume or Pan box is ticked (sent at once and when
/// the track is selected with Option).
pub fn instrument_setup(o: &Obj, pre: &str) -> Vec<Ev> {
    let p = |k: &str| if pre.is_empty() { o.p(k) } else { o.p(&format!("{pre}.{k}")) };
    let ch = if pre.is_empty() { (p("ch") - 1).max(0) as u8 } else { pre[1..].parse::<u8>().unwrap_or(1).saturating_sub(1) };
    let mut v = Vec::new();
    if p("prog_on") != 0 {
        let bank = p("bank");
        v.extend(bank_messages(o, bank, ch));
        v.push(Ev::new(0xC0, ch, p("prog") as u8, 0));
    }
    if p("vol_on") != 0 {
        v.push(Ev::new(0xB0, ch, 7, p("vol") as u8));
    }
    if p("pan_on") != 0 {
        v.push(Ev::new(0xB0, ch, 10, p("pan") as u8));
    }
    v
}

/// A bank's messages: the custom list (Options ▸ Define Custom Bank Messages; lines
/// "bank<TAB>status<TAB>d1<TAB>d2", in order), else a standard bank select (controller 0).
pub fn bank_messages(o: &Obj, bank: i32, ch: u8) -> Vec<Ev> {
    let custom: Vec<Ev> = o.texts.get("banks").map(|t| t.lines().filter_map(|l| {
        let f: Vec<&str> = l.split('\t').collect();
        if f.first()?.trim_end_matches(|c: char| c.is_alphabetic() || c == '(' || c == ')').parse::<i32>().ok()? != bank {
            return None;
        }
        let st = u8::from_str_radix(f.get(1)?.trim_start_matches("0x"), 16).ok()?;
        Some(Ev::new(st & 0xF0, ch, f.get(2)?.parse().ok()?, f.get(3)?.parse().ok()?))
    }).collect()).unwrap_or_default();
    if !custom.is_empty() {
        return custom;
    }
    vec![Ev::new(0xB0, ch, 0, bank as u8)]
}

/// The transformer's map (meta 122 / 123, Use Map): 128 values, identity by default.
pub fn transformer_map(o: &Obj) -> [u8; 128] {
    let mut m: [u8; 128] = std::array::from_fn(|i| i as u8);
    if let Some(t) = o.texts.get("map") {
        for (i, v) in t.split(',').enumerate().take(128) {
            if let Ok(v) = v.trim().parse() {
                m[i] = v;
            }
        }
    }
    m
}

fn status_of(i: i32) -> Option<u8> {
    Some(match i {
        1 => 0x90,
        2 => 0xA0,
        3 => 0xB0,
        4 => 0xC0,
        5 => 0xD0,
        6 => 0xE0,
        7 => META,
        _ => return None,
    })
}

fn cond_of(o: &Obj, k: &str) -> Cond {
    let (a, b) = (o.p(&format!("{k}a")) as i64, o.p(&format!("{k}b")) as i64);
    Cond::All.with_index(o.p(k).max(0) as usize).tap(a, b)
}

trait Tap {
    fn tap(self, a: i64, b: i64) -> Self;
}
impl Tap for Cond {
    fn tap(mut self, a: i64, b: i64) -> Cond {
        self.set_values(a, b);
        self
    }
}

/// Does an event meet the transformer's conditions?
pub fn transformer_matches(o: &Obj, e: Ev) -> bool {
    let est = if e.st == 0x80 { 0x90 } else { e.st };
    let st_ok = match o.p("c_st") {
        0 => true,
        1 => status_of(o.p("c_stv")) == Some(est),
        2 => status_of(o.p("c_stv")) != Some(est),
        _ => true,
    };
    st_ok && cond_of(o, "c_ch").test(e.ch as i64 + 1) && cond_of(o, "c_d1").test(e.d1 as i64) && cond_of(o, "c_d2").test(e.d2 as i64)
}

fn apply_op(o: &Obj, k: &str, v: i64, rng: &mut Rng, map: &[u8; 128]) -> i64 {
    let i = o.p(k).max(0) as usize;
    let (a, b) = (o.p(&format!("{k}a")) as i64, o.p(&format!("{k}b")) as i64);
    let f = o.p(&format!("{k}f")) as f64 / 100.0;
    if i == 16 {
        return map[v.clamp(0, 127) as usize] as i64;
    }
    let mut op = Op::Thru.with_index(i);
    op.set_values(a, b, f);
    match op {
        Op::Thru => v,
        Op::Fix(a) => a,
        Op::Add(a) => v + a,
        Op::Sub(a) => v - a,
        Op::Min(a) => v.max(a),
        Op::Max(a) => v.min(a),
        Op::Flip(a) => 2 * a - v,
        Op::Mul(f) => (v as f64 * f).round() as i64,
        Op::Div(f) => (v as f64 / f.max(0.001)).round() as i64,
        Op::Scale(f, a) => (v as f64 * f).round() as i64 + a,
        Op::Range(a, b) => v.clamp(a.min(b), a.max(b)),
        Op::Rand(a, b) => rng.range(a, b),
        Op::PlusMinusRand(a) => v + rng.range(-a, a),
        Op::Quantize(q) => ((v as f64 / q.max(1) as f64).round() as i64) * q.max(1),
        Op::QUp(q) => ((v + q.max(1) - 1) / q.max(1)) * q.max(1),
        Op::Expo(f) => (127.0 * (v as f64 / 127.0).powf(f.max(0.01))).round() as i64,
        _ => v,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clk(ms: f64) -> Clock {
        Clock { ms, bpm: 120.0, playing: true, song: 0.0 }
    }

    fn seq(out: &[Out]) -> Vec<Ev> {
        out.iter().filter_map(|o| if let Out::Seq { ev, .. } = o { Some(*ev) } else { None }).collect()
    }

    #[test]
    fn macros_pack_route_and_unpack() {
        let mut e = Env::new();
        let si = e.find(Kind::SequencerInput).unwrap();
        let a = e.add(Kind::Transformer, 3, 10.0, 10.0);
        let b = e.add(Kind::Monitor, 3, 100.0, 60.0);
        e.connect_next(a, b);
        // (no cables leave the selection: packed in place)
        let (m, copied) = e.make_macro(&[a, b]).unwrap();
        assert!(!copied);
        assert_eq!(e.members(m), vec![a, b]);
        assert_eq!(e.macro_in(m), Some(a));
        assert_eq!(e.macro_out(m), Some(b));
        assert!(e.obj(m).unwrap().assignable);
        e.connect_next(m, si);
        let out = e.deliver(m, Ev::note(0, 60, 100), clk(0.0));
        assert_eq!(seq(&out), vec![Ev::note(0, 60, 100)]);
        // Macro-In / Macro-Out names win; Show All off hides the transformer
        e.obj_mut(b).unwrap().name = "Macro-In".into();
        assert_eq!(e.macro_in(m), Some(b));
        e.obj_mut(m).unwrap().set("showall", 0);
        assert!(e.hidden_in_macro(a));
        assert!(!e.hidden_in_macro(b));
        // copies take members; nested macros; protected macros don't unpack
        let n = e.objects.len();
        let c = e.duplicate(&[m], 0.0, 300.0, None);
        assert_eq!(e.objects.len(), n + 3);
        assert_eq!(e.members(c[0]).len(), 2);
        let (outer, _) = e.make_macro(&[m, c[0]]).unwrap_or((0, false));
        assert!(outer != 0 && e.all_members(outer).len() == 6);
        e.obj_mut(outer).unwrap().set("protected", 1);
        assert!(!e.unpack_macro(outer));
        e.obj_mut(outer).unwrap().set("protected", 0);
        assert!(e.unpack_macro(outer));
        assert!(e.unpack_macro(m));
        assert!(e.macro_of(a).is_none());
        // the cable from the macro now leaves its output object
        assert!(e.cables.iter().any(|x| x.to == si && (x.from == a || x.from == b)));
    }

    #[test]
    fn macro_from_cabled_selection_is_a_copy() {
        let mut e = Env::new();
        let a = e.add(Kind::Transformer, 3, 10.0, 10.0);
        let b = e.add(Kind::Transformer, 3, 80.0, 10.0);
        let x = e.add(Kind::Monitor, 3, 300.0, 10.0);
        e.connect_next(a, b);
        e.connect_next(b, x);
        let (m, copied) = e.make_macro(&[a, b]).unwrap();
        assert!(copied);
        assert!(e.macro_of(a).is_none());
        assert_eq!(e.members(m).len(), 2);
        assert!(e.crossing_cables(&e.members(m)).is_empty());
        assert!(e.cables.iter().any(|c| c.from == b && c.to == x));
    }

    #[test]
    fn gm_mixer_reset_and_effects() {
        let r = gm_reset(1);
        assert_eq!(r[0], vec![0xF0, 0x41, 0x10, 0x42, 0x12, 0x40, 0x00, 0x7F, 0x00, 0x41, 0xF7]);
        assert_eq!(r.len(), 17);
        assert_eq!(r[16], vec![0xBF, 121, 0]);
        // GS reverb macro Hall 1 (4): 40 01 30 04, checksum 0x0B
        assert_eq!(gsxg_effect(1, 0, 4), Some(vec![0xF0, 0x41, 0x10, 0x42, 0x12, 0x40, 0x01, 0x30, 0x04, 0x0B, 0xF7]));
        assert_eq!(gsxg_effect(2, 2, 64), Some(vec![0xF0, 0x43, 0x10, 0x4C, 0x02, 0x01, 0x0C, 64, 0xF7]));
        assert_eq!(gsxg_effect(0, 0, 1), None);
        let o = Obj::new(1, Kind::GmMixer, 3, 0.0, 0.0);
        assert_eq!((o.p("k1"), o.p("k2"), o.p("k3")), (10, 91, 93));
    }

    #[test]
    fn sysex_mapper_transformer() {
        let mut e = Env::new();
        let t = e.add(Kind::Transformer, 3, 10.0, 10.0);
        let ins = e.add(Kind::Instrument, 3, 100.0, 10.0);
        e.obj_mut(ins).unwrap().port = "Synth".into();
        e.connect_next(t, ins);
        let o = e.obj_mut(t).unwrap();
        o.set("mode", 7);
        o.params.insert("sx_len".into(), 4);
        o.texts.insert("sxmap".into(), "67 16 0 0".into());
        // channel 1: byte 3 becomes 99 and is sent
        let out = e.deliver(t, Ev::new(0xB0, 0, 3, 99), clk(0.0));
        assert!(out.contains(&Out::Raw { port: "Synth".into(), bytes: vec![0xF0, 67, 16, 0, 99, 0xF7] }));
        // channel 2: changed, not sent; channel 3: sent unchanged
        assert!(e.deliver(t, Ev::new(0xB0, 1, 2, 5), clk(0.0)).iter().all(|x| !matches!(x, Out::Raw { .. })));
        let out = e.deliver(t, Ev::new(0xB0, 2, 2, 77), clk(0.0));
        assert!(out.contains(&Out::Raw { port: "Synth".into(), bytes: vec![0xF0, 67, 16, 5, 99, 0xF7] }));
        // non-matching events pass unless filtered
        e.obj_mut(t).unwrap().set("c_st", 1);
        e.obj_mut(t).unwrap().set("c_stv", 3);
        let seq_in = e.deliver(t, Ev::note(0, 60, 100), clk(0.0));
        let _ = seq_in;
        e.obj_mut(t).unwrap().params.insert("sx_filter".into(), 1);
        assert!(e.deliver(t, Ev::note(0, 60, 100), clk(0.0)).is_empty());
    }

    #[test]
    fn auto_fader_and_sysex_fader() {
        let mut e = Env::new();
        let si = e.find(Kind::SequencerInput).unwrap();
        let f1 = e.add(Kind::Fader, 3, 10.0, 10.0);
        let f2 = e.add(Kind::Fader, 3, 50.0, 10.0);
        e.obj_mut(f2).unwrap().set("out_d1", 10);
        let (m, _) = e.make_macro(&[f1, f2]).unwrap();
        e.obj_mut(m).unwrap().set("autofader", 1);
        e.connect_next(m, si);
        let out = e.move_fader(f1, 90, clk(0.0));
        assert_eq!(e.obj(f2).unwrap().p("val"), 90);
        assert!(seq(&out).contains(&Ev::new(0xB0, 0, 10, 90)));
        // SysEx output: raw bytes to the port its cables lead to
        let f = e.add(Kind::Fader, 3, 10.0, 200.0);
        let ins = e.add(Kind::Instrument, 3, 100.0, 200.0);
        e.obj_mut(ins).unwrap().port = "Synth".into();
        e.connect_next(f, ins);
        let o = e.obj_mut(f).unwrap();
        o.set("out_t", 7);
        o.params.insert("sx_sum".into(), 1);
        o.texts.insert("sysex".into(), "s\t1\t65 16 66 18 64 0 VAL SUM".to_string());
        let out = e.move_fader(f, 127, clk(0.0));
        assert!(out.contains(&Out::Raw { port: "Synth".into(), bytes: vec![0xF0, 65, 16, 66, 18, 64, 0, 127, 0x41, 0xF7] }));
        e.obj_mut(f).unwrap().set("in_t", 7);
        e.input_sysex(&[0xF0, 65, 16, 66, 18, 64, 0, 0xF7], clk(0.0));
        assert_eq!(e.obj(f).unwrap().p("val"), 127, "long messages aren't recognised as input");
    }

    #[test]
    fn default_routes_input_to_sequencer() {
        let mut e = Env::new();
        let out = e.input(0, Ev::note(0, 60, 100), clk(0.0));
        assert_eq!(seq(&out), vec![Ev::note(0, 60, 100)]);
        // the Input View monitor saw it and the keyboard shows it held
        let mon = e.objects.iter().find(|o| o.name == "Input View").unwrap().id;
        assert_eq!(e.run[&mon].log.len(), 1);
        let kb = e.objects.iter().find(|o| o.name == "Input Notes").unwrap().id;
        assert_eq!(e.run[&kb].held.len(), 1);
        // with the Physical Input's cable gone nothing reaches the Sequencer Input
        let pi = e.find(Kind::PhysicalInput).unwrap();
        let c = e.cables_from(pi)[0];
        e.disconnect(c);
        assert!(seq(&e.input(0, Ev::note(0, 61, 100), clk(1.0))).is_empty());
    }

    fn chain(e: &mut Env, k: Kind) -> Id {
        // Physical Input → k → Sequencer Input
        let pi = e.find(Kind::PhysicalInput).unwrap();
        let si = e.find(Kind::SequencerInput).unwrap();
        e.cables.clear();
        let id = e.add(k, 2, 100.0, 100.0);
        e.connect(pi, 0, id);
        e.connect_next(id, si);
        id
    }

    #[test]
    fn transformer_modes() {
        let mut e = Env::new();
        let t = chain(&mut e, Kind::Transformer);
        // notes: pitch + 12, others pass
        let o = e.obj_mut(t).unwrap();
        o.set("c_st", 1);
        o.set("c_stv", 1);
        o.set("o_d1", 2);
        o.set("o_d1a", 12);
        let out = e.input(0, Ev::note(0, 60, 90), clk(0.0));
        assert_eq!(seq(&out), vec![Ev::note(0, 72, 90)]);
        assert_eq!(seq(&e.input(0, Ev::new(0xB0, 0, 1, 5), clk(0.0))), vec![Ev::new(0xB0, 0, 1, 5)]);
        // filter non-matching
        e.obj_mut(t).unwrap().set("mode", 1);
        assert!(seq(&e.input(0, Ev::new(0xB0, 0, 1, 5), clk(0.0))).is_empty());
        // filter matching
        e.obj_mut(t).unwrap().set("mode", 2);
        assert!(seq(&e.input(0, Ev::note(0, 60, 90), clk(0.0))).is_empty());
        // copy and apply: original first
        e.obj_mut(t).unwrap().set("mode", 3);
        assert_eq!(seq(&e.input(0, Ev::note(0, 60, 90), clk(0.0))), vec![Ev::note(0, 60, 90), Ev::note(0, 72, 90)]);
        e.obj_mut(t).unwrap().set("mode", 4);
        assert_eq!(seq(&e.input(0, Ev::note(0, 60, 90), clk(0.0))), vec![Ev::note(0, 72, 90), Ev::note(0, 60, 90)]);
        // use map on data 2, set by meta events 123 (position) and 122 (value)
        let o = e.obj_mut(t).unwrap();
        o.set("mode", 0);
        o.set("o_d1", 0);
        o.set("o_d2", 16);
        e.inject_meta(t, 123, 90);
        e.inject_meta(t, 122, 33);
        assert_eq!(seq(&e.input(0, Ev::note(0, 60, 90), clk(0.0))), vec![Ev::note(0, 60, 33)]);
    }

    #[test]
    fn transformer_condition_splitter() {
        let mut e = Env::new();
        let t = chain(&mut e, Kind::Transformer);
        let mon = e.add(Kind::Monitor, 2, 0.0, 0.0);
        e.connect_next(t, mon);
        let o = e.obj_mut(t).unwrap();
        o.set("mode", 5);
        o.set("c_d1", 3); // <
        o.set("c_d1a", 60);
        assert_eq!(seq(&e.input(0, Ev::note(0, 50, 90), clk(0.0))).len(), 1);
        assert!(seq(&e.input(0, Ev::note(0, 70, 90), clk(0.0))).is_empty());
        assert_eq!(e.run[&mon].log.len(), 1);
    }

    #[test]
    fn chord_memorizer() {
        let mut e = Env::new();
        let c = chain(&mut e, Kind::ChordMemorizer);
        e.obj_mut(c).unwrap().set_chord(0, &[0, 4, 7]);
        let out = seq(&e.input(0, Ev::note(0, 48, 100), clk(0.0)));
        assert_eq!(out.iter().map(|x| x.d1).collect::<Vec<_>>(), vec![48, 52, 55]);
        // Transposition 1: C plays C♯ F G♯
        e.obj_mut(c).unwrap().set("trans", 1);
        let out = seq(&e.input(0, Ev::note(0, 60, 100), clk(0.0)));
        assert_eq!(out.iter().map(|x| x.d1).collect::<Vec<_>>(), vec![61, 65, 68]);
        // Key 1: C♯ maps to C♯ F G♯
        let o = e.obj_mut(c).unwrap();
        o.set("trans", 0);
        o.set("key", 1);
        let out = seq(&e.input(0, Ev::note(0, 61, 100), clk(0.0)));
        assert_eq!(out.iter().map(|x| x.d1).collect::<Vec<_>>(), vec![61, 65, 68]);
        // notes outside the key limit pass
        e.obj_mut(c).unwrap().set("khi", 50);
        assert_eq!(seq(&e.input(0, Ev::note(0, 61, 100), clk(0.0))).len(), 1);
    }

    #[test]
    fn delay_line_echoes_while_playing() {
        let mut e = Env::new();
        let d = chain(&mut e, Kind::DelayLine);
        let o = e.obj_mut(d).unwrap();
        o.set("rep", 2);
        o.set("delay", 480);
        o.set("trans", 12);
        o.set("velo", -10);
        let out = seq(&e.input(0, Ev::note(0, 60, 100), clk(0.0)));
        assert_eq!(out, vec![Ev::note(0, 60, 100)]);
        // 480 ticks at 120 bpm: 250 ms
        assert!(seq(&e.tick(clk(200.0))).is_empty());
        assert_eq!(seq(&e.tick(clk(260.0))), vec![Ev::note(0, 72, 90)]);
        assert_eq!(seq(&e.tick(clk(510.0))), vec![Ev::note(0, 84, 80)]);
        // stopped: no echoes; Thru Original off: nothing at all
        let mut c = clk(1000.0);
        c.playing = false;
        e.input(0, Ev::note(0, 60, 100), c);
        assert!(seq(&e.tick(clk(2000.0))).is_empty());
        e.obj_mut(d).unwrap().set("thru", 0);
        assert!(seq(&e.input(0, Ev::note(0, 60, 100), c)).is_empty());
    }

    #[test]
    fn voice_limiter_steals() {
        let mut e = Env::new();
        let v = chain(&mut e, Kind::VoiceLimiter);
        e.obj_mut(v).unwrap().set("voices", 2);
        e.input(0, Ev::note(0, 60, 100), clk(0.0));
        e.input(0, Ev::note(0, 64, 100), clk(0.0));
        let out = seq(&e.input(0, Ev::note(0, 67, 100), clk(0.0)));
        // Last: the earliest (60) is turned off
        assert_eq!(out, vec![Ev::new(0x80, 0, 60, 0), Ev::note(0, 67, 100)]);
        // Top priority: the lowest held note goes
        e.obj_mut(v).unwrap().set("prio", 1);
        let out = seq(&e.input(0, Ev::note(0, 70, 100), clk(0.0)));
        assert_eq!(out[0], Ev::new(0x80, 0, 64, 0));
    }

    #[test]
    fn arpeggiator_plays_up() {
        let mut e = Env::new();
        let a = chain(&mut e, Kind::Arpeggiator);
        e.obj_mut(a).unwrap().set("res", 8); // 1/16: 125 ms at 120
        let mut got = seq(&e.input(0, Ev::note(0, 60, 100), clk(0.0)));
        got.extend(seq(&e.input(0, Ev::note(0, 64, 100), clk(1.0))));
        got.extend(seq(&e.input(0, Ev::note(0, 67, 100), clk(2.0))));
        for k in 1..6 {
            got.extend(seq(&e.tick(clk(k as f64 * 125.0 + 1.0))));
        }
        let ons: Vec<u8> = got.iter().filter(|x| x.is_note_on()).map(|x| x.d1).collect();
        assert_eq!(&ons[..4], &[60, 64, 67, 60]);
        // Down
        let mut e = Env::new();
        let a = chain(&mut e, Kind::Arpeggiator);
        e.obj_mut(a).unwrap().set("dir", 1);
        let mut got = seq(&e.input(0, Ev::note(0, 60, 100), clk(0.0)));
        got.extend(seq(&e.input(0, Ev::note(0, 64, 100), clk(0.0))));
        for k in 1..4 {
            got.extend(seq(&e.tick(clk(k as f64 * 125.0 + 1.0))));
        }
        let ons: Vec<u8> = got.iter().filter(|x| x.is_note_on()).map(|x| x.d1).collect();
        assert!(ons.ends_with(&[64, 60]) || ons.contains(&64));
    }

    #[test]
    fn channel_splitter_and_cable_switcher() {
        let mut e = Env::new();
        let s = chain(&mut e, Kind::ChannelSplitter);
        let si = e.find(Kind::SequencerInput).unwrap();
        // channel 2 to a monitor; everything else to SUM (which goes nowhere now)
        e.cables.retain(|c| c.from != s);
        let mon = e.add(Kind::Monitor, 2, 0.0, 0.0);
        e.connect(s, 2, mon);
        e.connect(s, 0, si);
        e.input(0, Ev::note(1, 60, 100), clk(0.0));
        assert_eq!(e.run[&mon].log.len(), 1);
        assert_eq!(seq(&e.input(0, Ev::note(0, 60, 100), clk(0.0))).len(), 1);
        // a cable switcher: CC 1 picks the outlet
        let mut e = Env::new();
        let sw = chain(&mut e, Kind::Fader);
        let o = e.obj_mut(sw).unwrap();
        o.set("out_t", 6);
        o.set("out_d1", 48);
        o.set("in_t", 2);
        o.set("in_d1", 1);
        let m2 = e.add(Kind::Monitor, 2, 0.0, 0.0);
        e.connect_next(sw, m2);
        assert_eq!(seq(&e.input(0, Ev::note(0, 60, 1), clk(0.0))).len(), 1);
        e.input(0, Ev::new(0xB0, 0, 1, 1), clk(0.0));
        assert!(seq(&e.input(0, Ev::note(0, 60, 1), clk(0.0))).is_empty());
        assert_eq!(e.run[&m2].log.len(), 1);
        // 127 steps on and wraps through the last (uncabled) outlet
        e.input(0, Ev::new(0xB0, 0, 1, 127), clk(0.0));
        assert_eq!(e.obj(sw).unwrap().p("val"), 2);
        e.input(0, Ev::new(0xB0, 0, 1, 127), clk(0.0));
        assert_eq!(e.obj(sw).unwrap().p("val"), 0);
        e.click_switcher(sw);
        assert_eq!(e.obj(sw).unwrap().p("val"), 1);
    }

    #[test]
    fn fader_converts_and_filters() {
        let mut e = Env::new();
        let f = chain(&mut e, Kind::Fader);
        // CC 7 in → CC 11 out on channel 2
        let o = e.obj_mut(f).unwrap();
        o.set("out_d1", 11);
        o.set("out_ch", 2);
        assert_eq!(seq(&e.input(0, Ev::new(0xB0, 0, 7, 99), clk(0.0))), vec![Ev::new(0xB0, 1, 11, 99)]);
        assert_eq!(e.obj(f).unwrap().p("val"), 99);
        // Off passes non-matching; Other blocks them
        assert_eq!(seq(&e.input(0, Ev::note(0, 60, 9), clk(0.0))).len(), 1);
        e.obj_mut(f).unwrap().set("filter", 1);
        assert!(seq(&e.input(0, Ev::note(0, 60, 9), clk(0.0))).is_empty());
        // Thru blocks events from the Physical Input; the mouse still sends
        e.obj_mut(f).unwrap().set("filter", 4);
        assert!(seq(&e.input(0, Ev::new(0xB0, 0, 7, 50), clk(0.0))).is_empty());
        assert_eq!(seq(&e.move_fader(f, 20, clk(0.0))), vec![Ev::new(0xB0, 1, 11, 20)]);
        // 14 bit pitch bend
        let o = e.obj_mut(f).unwrap();
        o.set("filter", 6);
        o.set("out_t", 5);
        o.set("max", 16383);
        assert_eq!(seq(&e.move_fader(f, 8192, clk(0.0))), vec![Ev::new(0xE0, 1, 0, 64)]);
        // note on output sends on and off
        let o = e.obj_mut(f).unwrap();
        o.set("filter", 0);
        o.set("out_t", 0);
        o.set("out_d1", 36);
        assert_eq!(seq(&e.move_fader(f, 80, clk(0.0))), vec![Ev::note(1, 36, 80), Ev::new(0x80, 1, 36, 0)]);
        // meta 98 sets the value without sending; 99 Bang! resends
        e.inject_meta(f, 98, 42);
        assert_eq!(e.obj(f).unwrap().p("val"), 42);
        let out = e.inject_meta(f, 99, 0);
        assert_eq!(seq(&out), vec![Ev::note(1, 36, 42), Ev::new(0x80, 1, 36, 0)]);
    }

    #[test]
    fn instrument_object_to_port() {
        let mut e = Env::new();
        let i = chain(&mut e, Kind::Instrument);
        let o = e.obj_mut(i).unwrap();
        o.port = "Synth".into();
        o.set("ch", 3);
        o.set("trans", -12);
        o.set("velo", 10);
        o.set("khi", 70);
        let out = e.input(0, Ev::note(0, 60, 100), clk(0.0));
        assert!(out.contains(&Out::Port { port: "Synth".into(), ev: Ev::note(2, 48, 110) }));
        // outside the key limit: nothing
        assert!(e.input(0, Ev::note(0, 90, 100), clk(0.0)).is_empty());
        let o = e.obj_mut(i).unwrap();
        o.set("prog_on", 1);
        o.set("prog", 5);
        o.set("vol_on", 1);
        let s = instrument_setup(o, "");
        assert_eq!(s, vec![Ev::new(0xB0, 2, 0, 0), Ev::new(0xC0, 2, 5, 0), Ev::new(0xB0, 2, 7, 100)]);
    }

    #[test]
    fn mapped_instrument_gm_notation_defaults() {
        // kick in the bottom space (3.5 lines below the top line), hi-hat a cross above it
        let k = MapRow::default_for(36);
        assert_eq!((k.head, k.relpos, k.group.as_str()), (0, -7, "Kick"));
        let h = MapRow::default_for(42);
        assert_eq!((h.head, h.relpos, h.group.as_str()), (1, 1, "Hi-Hat"));
        assert_eq!(MapRow::default_for(38).group, "Snare");
        assert_eq!(MapRow::default_for(49).group, "Cymbals");
        assert_eq!(MapRow::default_for(45).group, "Toms");
        // outside the GM drum keys: no group (not shown in a mapped style)
        assert_eq!(MapRow::default_for(20).group, "");
    }

    #[test]
    fn mapped_instrument_rows() {
        let mut e = Env::new();
        let m = chain(&mut e, Kind::MappedInstrument);
        let o = e.obj_mut(m).unwrap();
        o.port.clear();
        let mut r = MapRow::default_for(36);
        r.out = 38;
        r.vel = -20;
        r.ch = 5;
        o.set_map_row(36, &r);
        assert_eq!(o.map_row(36).out, 38);
        assert_eq!(seq(&e.input(0, Ev::note(0, 36, 100), clk(0.0))), vec![Ev::note(4, 38, 80)]);
        // a cable that doesn't exist: not sent
        let o = e.obj_mut(m).unwrap();
        r.cable = 3;
        o.set_map_row(36, &r);
        assert!(seq(&e.input(0, Ev::note(0, 36, 100), clk(0.0))).is_empty());
    }

    #[test]
    fn layers_copy_and_save() {
        let mut e = Env::new();
        let l = e.create_layer(4);
        assert_eq!(e.layers[l], "(unnamed)");
        let a = e.add(Kind::Monitor, l, 10.0, 10.0);
        let b = e.add(Kind::Keyboard, l, 10.0, 80.0);
        e.connect_next(a, b);
        let copies = e.duplicate(&[a, b], 20.0, 0.0, None);
        assert_eq!(copies.len(), 2);
        assert!(e.cables.iter().any(|c| c.from == copies[0] && c.to == copies[1]));
        let t = e.to_text();
        let back = Env::from_text(&t).unwrap();
        assert_eq!(back, e);
        assert!(e.delete_layer(l));
        assert!(e.obj(a).is_none());
        assert!(!e.delete_layer(1));
        // align: a column under the upper-left object
        let x = e.add(Kind::Monitor, 2, 0.0, 0.0);
        let y = e.add(Kind::Monitor, 2, 5.0, 300.0);
        e.align(&[x, y]);
        assert_eq!(e.obj(y).unwrap().x, 0.0);
    }

    #[test]
    fn import_environments() {
        let mut src = Env::new();
        let l = src.create_layer(4);
        src.layers[l] = "Synths".into();
        let i = src.add(Kind::Instrument, l, 0.0, 0.0);
        let mon = src.add(Kind::Monitor, 2, 0.0, 0.0);
        src.connect_next(i, mon);
        let mut dst = Env::new();
        let n = dst.objects.len();
        dst.import_layer(&src, l);
        assert!(dst.layers.contains(&"Synths".to_string()));
        // the instrument and the monitor cabled from it
        assert_eq!(dst.objects.len(), n + 2);
        let mut d2 = Env::new();
        d2.update_from(&src);
        assert_eq!(d2.objects.iter().filter(|o| o.kind == Kind::Instrument).count(), 1);
        d2.total_replace(&src);
        assert_eq!(d2, src);
    }

    #[test]
    fn alias_rechannelizes() {
        let mut e = Env::new();
        let pi = e.find(Kind::PhysicalInput).unwrap();
        e.cables.clear();
        let ins = e.add(Kind::Instrument, 3, 0.0, 0.0);
        e.obj_mut(ins).unwrap().port = "X".into();
        let al = e.add(Kind::Alias, 2, 0.0, 0.0);
        let o = e.obj_mut(al).unwrap();
        o.set("ref", ins as i32);
        e.connect(pi, 0, al);
        let out = e.input(0, Ev::note(5, 60, 100), clk(0.0));
        assert!(out.contains(&Out::Port { port: "X".into(), ev: Ev::note(0, 60, 100) }));
    }

    #[test]
    fn value_as_text() {
        let mut o = Obj::new(1, Kind::Fader, 2, 0.0, 0.0);
        o.set("as", 1);
        assert_eq!(value_text(&o, 64), "0");
        o.set("as", 6);
        assert_eq!(value_text(&o, 70), "120");
        o.set("as", 7);
        o.set("max", 1);
        o.texts.insert("names".into(), "Off\nOn".into());
        assert_eq!(value_text(&o, 1), "On");
        assert_eq!(value_text(&o, 5), "On");
    }
}

#[cfg(test)]
impl Env {
    fn inject_meta(&mut self, id: Id, num: u8, val: u8) -> Vec<Out> {
        let mut out = Vec::new();
        self.receive(id, Tagged { ev: Ev::meta(num, val), from_pi: false }, Clock { ms: 0.0, bpm: 120.0, playing: true, song: 0.0 }, &mut out, &[]);
        out
    }
}
