//! Control surfaces and controller assignments, as Logic's Controller Assignments window and its
//! control surface support work: assignments (an incoming MIDI message, its value range and
//! format, how it changes a parameter, and the parameter: a channel strip's, a global one, a key
//! command, a mode change…) in zones and modes, Learn, and the Mackie Control and HUI protocols
//! (faders, V-Pots, buttons, banking, transport, and their feedback: motor faders, LEDs, LCD,
//! time code display).

use std::collections::BTreeMap;

/// A token of an assignment's Value Change message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Byte(u8),
    /// The low 7 bits of the value.
    Lo7,
    /// The high 7 bits (14-bit messages).
    Hi7,
}

/// Parses "B0 08 Hi7 B0 28 Lo7" (hex bytes, Lo7, Hi7).
pub fn parse_msg(s: &str) -> Vec<Tok> {
    s.split_whitespace().filter_map(|w| match w.to_ascii_lowercase().as_str() {
        "lo7" => Some(Tok::Lo7),
        "hi7" => Some(Tok::Hi7),
        x => u8::from_str_radix(x, 16).ok().map(Tok::Byte),
    }).collect()
}

pub fn msg_text(m: &[Tok]) -> String {
    m.iter().map(|t| match t {
        Tok::Byte(b) => format!("{b:02X}"),
        Tok::Lo7 => "Lo7".into(),
        Tok::Hi7 => "Hi7".into(),
    }).collect::<Vec<_>>().join(" ")
}

/// A plain-language reading of a Value Change message ("Control 7 on channel 1").
pub fn msg_meaning(m: &[Tok]) -> String {
    let Some(Tok::Byte(s)) = m.first() else { return String::new() };
    let ch = (s & 15) + 1;
    let d1 = match m.get(1) {
        Some(Tok::Byte(b)) => b.to_string(),
        Some(_) => "value".into(),
        None => String::new(),
    };
    let what = match s & 0xF0 {
        0x80 => format!("Note Off {d1}"),
        0x90 => format!("Note {d1}"),
        0xA0 => format!("Poly Pressure {d1}"),
        0xB0 => format!("Control {d1}"),
        0xC0 => "Program Change".into(),
        0xD0 => "Channel Pressure".into(),
        0xE0 => "Pitch Bend".into(),
        _ => return "System message".into(),
    };
    let bits = if m.contains(&Tok::Hi7) { ", 14-bit" } else if m.contains(&Tok::Lo7) { "" } else { ", value 1 (button)" };
    format!("{what} on channel {ch}{bits}")
}

/// How negative values are encoded (Format).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Unsigned,
    TwosComplement,
    OnesComplement,
    SignMagnitude,
}

impl Format {
    pub const NAMES: [&'static str; 4] = ["Unsigned", "2's Complement", "1's Complement", "Sign Magnitude"];
    pub fn from_index(i: usize) -> Format {
        [Format::Unsigned, Format::TwosComplement, Format::OnesComplement, Format::SignMagnitude][i.min(3)]
    }
    pub fn index(&self) -> usize {
        *self as usize
    }

    /// A raw value of `bits` bits (7 or 14) read as this format.
    pub fn decode(&self, raw: i32, bits: u32) -> i32 {
        let msb = 1 << (bits - 1);
        let mask = (1 << bits) - 1;
        if raw & msb == 0 {
            return raw;
        }
        match self {
            Format::Unsigned => raw,
            Format::TwosComplement => -((!raw + 1) & mask),
            Format::OnesComplement => -(!raw & mask),
            Format::SignMagnitude => -(raw & !msb),
        }
    }
}

/// How an incoming value changes the parameter (Value Mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueMode {
    Direct,
    Toggle,
    Scaled,
    Relative,
    Rotate,
    XOr,
}

impl ValueMode {
    pub const NAMES: [&'static str; 6] = ["Direct", "Toggle", "Scaled", "Relative", "Rotate", "X-OR"];
    pub fn from_index(i: usize) -> ValueMode {
        [ValueMode::Direct, ValueMode::Toggle, ValueMode::Scaled, ValueMode::Relative, ValueMode::Rotate, ValueMode::XOr][i.min(5)]
    }
    pub fn index(&self) -> usize {
        *self as usize
    }
}

/// Feedback formats (LED rings, LCD bars).
pub const FEEDBACK: [&str; 11] = ["None", "Automatic", "Single Dot/Line", "Left to Right Bar", "Bar from Center", "Right to Left Bar", "Q/Spread", "Ascending Bar", "Descending Bar", "Text Only", "Default"];

/// What an assignment controls (the Class menu).
#[derive(Clone, Debug, PartialEq)]
pub enum Class {
    /// Activates a mode of a zone: Direct, Toggle (with the zone's first mode), Relative, Rotate.
    ModeChange { mode: String, how: u8 },
    /// A global parameter (Playhead, Cycle, Autopunch, Go to Marker n, …).
    Global { param: String, number: i32 },
    /// A channel strip parameter: the strip (Selected Track, Fader Bank n, Index n, Audio n,
    /// Software Instrument n, Aux n, Output n, Master, Pinned to Track n) and the parameter.
    Strip { strip: Strip, param: StripParam },
    /// A computer key press.
    Key(String),
    /// A key command.
    KeyCommand(String),
    /// A control surface group parameter (Fader Bank, Flip Mode, …).
    Group { param: String },
    /// An automation group parameter.
    AutomationGroup { param: String, group: i32 },
}

impl Class {
    pub const NAMES: [&'static str; 7] = ["Mode Change", "Global", "Channel Strip", "Key", "Key Command", "Control Surface Group", "Automation Group"];
    pub fn index(&self) -> usize {
        match self {
            Class::ModeChange { .. } => 0,
            Class::Global { .. } => 1,
            Class::Strip { .. } => 2,
            Class::Key(_) => 3,
            Class::KeyCommand(_) => 4,
            Class::Group { .. } => 5,
            Class::AutomationGroup { .. } => 6,
        }
    }
    pub fn with_index(i: usize) -> Class {
        match i {
            0 => Class::ModeChange { mode: String::new(), how: 0 },
            1 => Class::Global { param: GLOBALS[0].into(), number: 1 },
            3 => Class::Key(String::new()),
            4 => Class::KeyCommand(String::new()),
            5 => Class::Group { param: GROUP_PARAMS[0].into() },
            6 => Class::AutomationGroup { param: "Volume".into(), group: 0 },
            _ => Class::Strip { strip: Strip::Selected, param: StripParam::Volume },
        }
    }
}

/// Global class parameters Studio carries out.
pub const GLOBALS: [&str; 20] = ["Playhead", "Playhead (Beats)", "Playhead (Time Code)", "Playhead (Beats, Scrubbing)", "Move Locators", "Left Locator", "Right Locator", "Move Punch Locators", "Marker Position", "Nudge selected Regions/Events", "Any Solo", "Nudge Value", "Automation of all tracks", "Cycle", "Autopunch", "Go to Marker", "Group Clutch", "Shuttle Speed", "Horizontal Zoom", "Vertical Zoom"];
/// Control surface group parameters (Class Control Surface Group).
pub const GROUP_PARAMS: [&str; 9] = ["Fader Bank for Current View", "Flip Mode", "Display Mode", "Clock Display", "Channel Strip View Mode", "Channel Strip Parameter", "Track Lock", "Plug-in Parameter Page", "Send Slot"];

#[derive(Clone, Debug, PartialEq)]
pub enum Strip {
    Selected,
    FaderBank(i32),
    Index(i32),
    Audio(i32),
    Instrument(i32),
    Aux(i32),
    Output(i32),
    Master,
    Pinned(i32),
}

impl Strip {
    pub const NAMES: [&'static str; 9] = ["Selected Track", "Fader Bank", "Index", "Audio", "Software Instrument", "Auxiliary", "Output", "Master", "Pinned to Track"];
    pub fn index(&self) -> usize {
        match self {
            Strip::Selected => 0,
            Strip::FaderBank(_) => 1,
            Strip::Index(_) => 2,
            Strip::Audio(_) => 3,
            Strip::Instrument(_) => 4,
            Strip::Aux(_) => 5,
            Strip::Output(_) => 6,
            Strip::Master => 7,
            Strip::Pinned(_) => 8,
        }
    }
    pub fn number(&self) -> i32 {
        match self {
            Strip::FaderBank(n) | Strip::Index(n) | Strip::Audio(n) | Strip::Instrument(n) | Strip::Aux(n) | Strip::Output(n) | Strip::Pinned(n) => *n,
            _ => 0,
        }
    }
    pub fn with(i: usize, n: i32) -> Strip {
        match i {
            1 => Strip::FaderBank(n),
            2 => Strip::Index(n),
            3 => Strip::Audio(n),
            4 => Strip::Instrument(n),
            5 => Strip::Aux(n),
            6 => Strip::Output(n),
            7 => Strip::Master,
            8 => Strip::Pinned(n),
            _ => Strip::Selected,
        }
    }
}

/// Channel strip parameters.
#[derive(Clone, Debug, PartialEq)]
pub enum StripParam {
    Volume,
    Pan,
    Mute,
    Solo,
    RecordEnable,
    Select,
    Send(u8),
    /// A plug-in parameter: insert slot (0 = instrument), parameter index.
    Plugin(u8, u16),
    Automation,
}

impl StripParam {
    pub fn name(&self) -> String {
        match self {
            StripParam::Volume => "Volume".into(),
            StripParam::Pan => "Pan".into(),
            StripParam::Mute => "Mute".into(),
            StripParam::Solo => "Solo".into(),
            StripParam::RecordEnable => "Record Enable".into(),
            StripParam::Select => "Select".into(),
            StripParam::Send(n) => format!("Send {}", n + 1),
            StripParam::Plugin(0, p) => format!("Instrument Param {}", p + 1),
            StripParam::Plugin(s, p) => format!("Insert {s} Param {}", p + 1),
            StripParam::Automation => "Automation Mode".into(),
        }
    }
    pub fn parse(s: &str) -> StripParam {
        match s {
            "Volume" => StripParam::Volume,
            "Pan" => StripParam::Pan,
            "Mute" => StripParam::Mute,
            "Solo" => StripParam::Solo,
            "Record Enable" => StripParam::RecordEnable,
            "Select" => StripParam::Select,
            "Automation Mode" => StripParam::Automation,
            x if x.starts_with("Send ") => StripParam::Send(x[5..].parse::<u8>().unwrap_or(1).saturating_sub(1)),
            x if x.starts_with("Instrument Param ") => StripParam::Plugin(0, x[17..].parse::<u16>().unwrap_or(1).saturating_sub(1)),
            x if x.starts_with("Insert ") => {
                let w: Vec<&str> = x.split_whitespace().collect();
                StripParam::Plugin(w.get(1).and_then(|v| v.parse().ok()).unwrap_or(1), w.get(3).and_then(|v| v.parse::<u16>().ok()).unwrap_or(1).saturating_sub(1))
            }
            _ => StripParam::Volume,
        }
    }
    /// On/off parameters (Toggle by default).
    pub fn is_switch(&self) -> bool {
        matches!(self, StripParam::Mute | StripParam::Solo | StripParam::RecordEnable | StripParam::Select)
    }
}

/// A controller assignment.
#[derive(Clone, Debug, PartialEq)]
pub struct Assignment {
    pub zone: String,
    pub mode: String,
    pub control_name: String,
    pub label: String,
    pub flip_group: u32,
    pub exclusive: bool,
    /// The input port's name ("" any port).
    pub port: String,
    pub msg: Vec<Tok>,
    pub min: i32,
    pub max: i32,
    /// Only the minimum and maximum, nothing between.
    pub only: bool,
    pub format: Format,
    /// The Touch/Release message (empty: none).
    pub touch: Vec<Tok>,
    pub class: Class,
    pub multiply: f32,
    pub value_mode: ValueMode,
    pub resolution: i32,
    pub global_resolution: bool,
    pub key_repeat: bool,
    pub feedback: usize,
    pub text_feedback: bool,
    pub local_feedback: bool,
    /// Modifier keys held with the control (bit 0 Shift, 1 Control, 2 Option, 3 Command).
    pub modifiers: u8,
}

impl Assignment {
    pub fn learned(port: &str, msg: Vec<Tok>, class: Class) -> Assignment {
        let button = !msg.contains(&Tok::Lo7);
        let relative = false;
        let switch = matches!(&class, Class::Strip { param, .. } if param.is_switch()) || matches!(&class, Class::KeyCommand(_) | Class::Key(_));
        Assignment {
            zone: String::new(),
            mode: String::new(),
            control_name: "Learned".into(),
            label: String::new(),
            flip_group: 0,
            exclusive: false,
            port: port.into(),
            max: if msg.contains(&Tok::Hi7) { 16383 } else { 127 },
            msg,
            min: 0,
            only: false,
            format: Format::Unsigned,
            touch: Vec::new(),
            class,
            multiply: 1.0,
            // (the default: Toggle for on/off parameters, Scaled for absolute controls, Relative
            // for encoders)
            value_mode: if switch || button { ValueMode::Toggle } else if relative { ValueMode::Relative } else { ValueMode::Scaled },
            resolution: 128,
            global_resolution: true,
            key_repeat: false,
            feedback: 1,
            text_feedback: false,
            local_feedback: false,
            modifiers: 0,
        }
    }

    /// Does a MIDI message match the Value Change message? Its value when it does (1 for a
    /// message without Lo7/Hi7, as a button).
    pub fn value_of(&self, port: &str, bytes: &[u8]) -> Option<i32> {
        if !self.port.is_empty() && !port.is_empty() && self.port != port {
            return None;
        }
        match_msg(&self.msg, bytes).map(|(v, bits)| {
            if bits == 0 {
                1
            } else {
                self.format.decode(v, bits)
            }
        })
    }

    pub fn short_param(&self) -> String {
        match &self.class {
            Class::ModeChange { mode, .. } => format!("Mode: {mode}"),
            Class::Global { param, number } => if param == "Go to Marker" { format!("{param} {number}") } else { param.clone() },
            Class::Strip { strip, param } => {
                let s = match strip {
                    Strip::Selected => "Sel".to_string(),
                    Strip::Master => "Master".to_string(),
                    x => format!("{} {}", Strip::NAMES[x.index()], x.number()),
                };
                format!("{s} {}", param.name())
            }
            Class::Key(k) => format!("Key {k}"),
            Class::KeyCommand(c) => c.clone(),
            Class::Group { param } => param.clone(),
            Class::AutomationGroup { param, group } => format!("Group {group} {param}"),
        }
    }

    /// The new parameter value (0–1 range for continuous parameters, given as `cur`) for an
    /// incoming value, as Value Mode, Minimum / Maximum and Multiply say. `steps`: the number
    /// of steps across the parameter's range (relative changes move by one step per unit).
    pub fn apply(&self, incoming: i32, cur: f32, steps: f32) -> Option<f32> {
        if self.only && incoming != self.min && incoming != self.max {
            return None;
        }
        let v = incoming as f32 * self.multiply;
        let span = (self.max - self.min).max(1) as f32;
        Some(match self.value_mode {
            ValueMode::Direct => (v / steps.max(1.0)).clamp(0.0, 1.0),
            ValueMode::Toggle => {
                if incoming == 0 {
                    return None;
                }
                if cur > 0.5 { 0.0 } else { 1.0 }
            }
            ValueMode::Scaled => ((v - self.min as f32 * self.multiply) / (span * self.multiply.abs().max(1e-6))).clamp(0.0, 1.0),
            ValueMode::Relative => (cur + v / steps.max(1.0)).clamp(0.0, 1.0),
            ValueMode::Rotate => {
                let x = cur + v / steps.max(1.0);
                if x > 1.0 { 0.0 } else if x < 0.0 { 1.0 } else { x }
            }
            ValueMode::XOr => {
                let c = (cur * steps).round() as i32;
                ((c ^ v as i32) as f32 / steps.max(1.0)).clamp(0.0, 1.0)
            }
        })
    }

    pub fn to_line(&self) -> String {
        let class = match &self.class {
            Class::ModeChange { mode, how } => format!("mode\x1f{mode}\x1f{how}"),
            Class::Global { param, number } => format!("global\x1f{param}\x1f{number}"),
            Class::Strip { strip, param } => format!("strip\x1f{}\x1f{}\x1f{}", strip.index(), strip.number(), param.name()),
            Class::Key(k) => format!("key\x1f{k}"),
            Class::KeyCommand(c) => format!("cmd\x1f{c}"),
            Class::Group { param } => format!("group\x1f{param}"),
            Class::AutomationGroup { param, group } => format!("agroup\x1f{param}\x1f{group}"),
        };
        [
            self.zone.clone(), self.mode.clone(), self.control_name.clone(), self.label.clone(), self.flip_group.to_string(), (self.exclusive as u8).to_string(), self.port.clone(), msg_text(&self.msg),
            self.min.to_string(), self.max.to_string(), (self.only as u8).to_string(), self.format.index().to_string(), msg_text(&self.touch), class, self.multiply.to_string(), self.value_mode.index().to_string(),
            self.resolution.to_string(), (self.global_resolution as u8).to_string(), (self.key_repeat as u8).to_string(), self.feedback.to_string(), (self.text_feedback as u8).to_string(), (self.local_feedback as u8).to_string(), self.modifiers.to_string(),
        ].join("\t")
    }

    pub fn from_line(l: &str) -> Option<Assignment> {
        let f: Vec<&str> = l.split('\t').collect();
        if f.len() < 23 {
            return None;
        }
        let c: Vec<&str> = f[13].split('\x1f').collect();
        let num = |i: usize| c.get(i).and_then(|x| x.parse::<i32>().ok()).unwrap_or(0);
        let s = |i: usize| c.get(i).unwrap_or(&"").to_string();
        let class = match c.first().copied()? {
            "mode" => Class::ModeChange { mode: s(1), how: num(2) as u8 },
            "global" => Class::Global { param: s(1), number: num(2) },
            "strip" => Class::Strip { strip: Strip::with(num(1) as usize, num(2)), param: StripParam::parse(&s(3)) },
            "key" => Class::Key(s(1)),
            "cmd" => Class::KeyCommand(s(1)),
            "group" => Class::Group { param: s(1) },
            "agroup" => Class::AutomationGroup { param: s(1), group: num(2) },
            _ => return None,
        };
        Some(Assignment {
            zone: f[0].into(), mode: f[1].into(), control_name: f[2].into(), label: f[3].into(), flip_group: f[4].parse().unwrap_or(0), exclusive: f[5] == "1", port: f[6].into(), msg: parse_msg(f[7]),
            min: f[8].parse().unwrap_or(0), max: f[9].parse().unwrap_or(127), only: f[10] == "1", format: Format::from_index(f[11].parse().unwrap_or(0)), touch: parse_msg(f[12]), class, multiply: f[14].parse().unwrap_or(1.0),
            value_mode: ValueMode::from_index(f[15].parse().unwrap_or(2)), resolution: f[16].parse().unwrap_or(128), global_resolution: f[17] == "1", key_repeat: f[18] == "1", feedback: f[19].parse().unwrap_or(1), text_feedback: f[20] == "1", local_feedback: f[21] == "1", modifiers: f[22].parse().unwrap_or(0),
        })
    }
}

/// Matches a message pattern; (value, bits: 0 none, 7 or 14).
pub fn match_msg(pat: &[Tok], bytes: &[u8]) -> Option<(i32, u32)> {
    if pat.is_empty() || pat.len() != bytes.len() {
        // (a note on of velocity 0 learned as a note on still matches as a release)
        return None;
    }
    let (mut lo, mut hi) = (None, None);
    for (t, b) in pat.iter().zip(bytes) {
        match t {
            Tok::Byte(x) => {
                if x != b {
                    return None;
                }
            }
            Tok::Lo7 => lo = Some(*b as i32 & 127),
            Tok::Hi7 => hi = Some(*b as i32 & 127),
        }
    }
    Some(match (lo, hi) {
        (Some(l), Some(h)) => ((h << 7) | l, 14),
        (Some(l), None) => (l, 7),
        (None, Some(h)) => (h << 7, 14),
        (None, None) => (1, 0),
    })
}

/// The pattern learned from a message: status and data 1 kept, the value as Lo7 (pitch bend:
/// Lo7 Hi7; program change and channel pressure: Lo7 in data 1).
pub fn learn_pattern(bytes: &[u8]) -> Vec<Tok> {
    let Some(&s) = bytes.first() else { return Vec::new() };
    match s & 0xF0 {
        0xE0 => vec![Tok::Byte(s), Tok::Lo7, Tok::Hi7],
        0xC0 | 0xD0 => vec![Tok::Byte(s), Tok::Lo7],
        0x80 | 0x90 | 0xA0 | 0xB0 if bytes.len() >= 3 => vec![Tok::Byte(s), Tok::Byte(bytes[1]), Tok::Lo7],
        _ => bytes.iter().map(|b| Tok::Byte(*b)).collect(),
    }
}

/// Series assignment (learn the first and last pair): the patterns between two learned
/// controller messages, when they differ only in data 1 (controller number) or in the channel.
/// None when the counts don't match.
pub fn fill_series(first: &[Tok], last: &[Tok], params: usize) -> Option<Vec<Vec<Tok>>> {
    let (Some(Tok::Byte(s1)), Some(Tok::Byte(s2))) = (first.first(), last.first()) else { return None };
    if first.len() != last.len() || params < 2 {
        return None;
    }
    if s1 == s2 {
        let (Some(Tok::Byte(a)), Some(Tok::Byte(b))) = (first.get(1), last.get(1)) else { return None };
        let n = (*b as i32 - *a as i32).unsigned_abs() as usize + 1;
        if n != params {
            return None;
        }
        let step = if b >= a { 1 } else { -1 };
        return Some((0..n).map(|k| {
            let mut p = first.to_vec();
            p[1] = Tok::Byte((*a as i32 + step * k as i32) as u8);
            p
        }).collect());
    }
    if s1 & 0xF0 == s2 & 0xF0 {
        let n = ((s2 & 15) as i32 - (s1 & 15) as i32).unsigned_abs() as usize + 1;
        if n != params {
            return None;
        }
        let step = if s2 >= s1 { 1 } else { -1 };
        return Some((0..n).map(|k| {
            let mut p = first.to_vec();
            p[0] = Tok::Byte((*s1 as i32 + step * k as i32) as u8);
            p
        }).collect());
    }
    None
}

/// All assignments, zones and their active modes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Assignments {
    pub list: Vec<Assignment>,
    /// Zones (in order) and their modes.
    pub zones: Vec<(String, Vec<String>)>,
    /// The active mode of each zone.
    pub active: BTreeMap<String, String>,
}

impl Assignments {
    /// Is an assignment active (modeless, or its zone's active mode; an exclusive assignment
    /// for the same control silences other non-exclusive ones)?
    pub fn is_active(&self, i: usize) -> bool {
        let Some(a) = self.list.get(i) else { return false };
        let mode_ok = a.mode.is_empty() || self.active.get(&a.zone) == Some(&a.mode);
        if !mode_ok {
            return false;
        }
        if !a.exclusive {
            // (an active exclusive assignment of the same control takes over)
            let blocked = self.list.iter().enumerate().any(|(k, b)| k != i && b.exclusive && b.msg == a.msg && b.port == a.port && (b.mode.is_empty() || self.active.get(&b.zone) == Some(&b.mode)));
            if blocked {
                return false;
            }
        }
        true
    }

    /// The active assignments a message triggers, with their values.
    pub fn matching(&self, port: &str, bytes: &[u8], modifiers: u8) -> Vec<(usize, i32)> {
        (0..self.list.len()).filter(|&i| self.is_active(i) && self.list[i].modifiers == modifiers).filter_map(|i| self.list[i].value_of(port, bytes).map(|v| (i, v))).collect()
    }

    /// A Mode Change assignment pressed: the zone's active mode changes (Direct; Toggle between
    /// the first mode and this one; Relative by ±1; Rotate onwards).
    pub fn change_mode(&mut self, zone: &str, mode: &str, how: u8, value: i32) {
        let modes = self.zones.iter().find(|z| z.0 == zone).map(|z| z.1.clone()).unwrap_or_default();
        if modes.is_empty() {
            return;
        }
        let cur = self.active.get(zone).and_then(|m| modes.iter().position(|x| x == m)).unwrap_or(0);
        let next = match how {
            1 => {
                let target = modes.iter().position(|x| x == mode).unwrap_or(0);
                if cur == target { 0 } else { target }
            }
            2 => (cur as i32 + value.signum()).clamp(0, modes.len() as i32 - 1) as usize,
            3 => (cur + 1) % modes.len(),
            _ => modes.iter().position(|x| x == mode).unwrap_or(cur),
        };
        self.active.insert(zone.to_string(), modes[next].clone());
    }

    pub fn to_text(&self) -> String {
        let mut s = String::from("bramble-assignments 1\n");
        for (z, modes) in &self.zones {
            s += &format!("zone\t{z}\t{}\t{}\n", modes.join("\x1f"), self.active.get(z).cloned().unwrap_or_default());
        }
        for a in &self.list {
            s += &format!("a\t{}\n", a.to_line());
        }
        s
    }

    pub fn from_text(t: &str) -> Assignments {
        let mut a = Assignments::default();
        for l in t.lines().skip(1) {
            if let Some(rest) = l.strip_prefix("zone\t") {
                let f: Vec<&str> = rest.split('\t').collect();
                let modes: Vec<String> = f.get(1).map(|m| m.split('\x1f').filter(|x| !x.is_empty()).map(|x| x.to_string()).collect()).unwrap_or_default();
                if let Some(act) = f.get(2).filter(|x| !x.is_empty()) {
                    a.active.insert(f[0].to_string(), act.to_string());
                }
                a.zones.push((f[0].to_string(), modes));
            } else if let Some(rest) = l.strip_prefix("a\t") {
                if let Some(x) = Assignment::from_line(rest) {
                    a.list.push(x);
                }
            }
        }
        a
    }
}

/// A dynamic label: "@" placeholders (`@s#` send number, `@tn` track name, `@p p` …) expanded.
/// `get(kind, value)` gives the text for a placeholder's type and value characters.
pub fn expand_label(label: &str, get: impl Fn(char, char) -> String) -> String {
    let mut out = String::new();
    let ch: Vec<char> = label.chars().collect();
    let mut i = 0;
    while i < ch.len() {
        if ch[i] == '@' && i + 2 < ch.len() {
            out += &get(ch[i + 1], ch[i + 2]);
            i += 3;
        } else {
            out.push(ch[i]);
            i += 1;
        }
    }
    out
}

// ---- Mackie Control ----

/// What a Mackie Control (Logic Control) message means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mcu {
    /// A channel fader (0–7) or the master (8) moved: 0–1.
    Fader(u8, f32),
    /// A V-Pot turned: strip, signed ticks.
    VPot(u8, i32),
    /// The jog wheel: signed ticks.
    Jog(i32),
    /// A button pressed (true) or released.
    Button(u8, bool),
    /// A fader touched / released (strip, 8 = master).
    Touch(u8, bool),
}

/// Mackie Control button notes.
pub mod mcu {
    pub const REC: u8 = 0x00;
    pub const SOLO: u8 = 0x08;
    pub const MUTE: u8 = 0x10;
    pub const SELECT: u8 = 0x18;
    pub const VPOT_PUSH: u8 = 0x20;
    pub const ASSIGN_TRACK: u8 = 0x28;
    pub const ASSIGN_SEND: u8 = 0x29;
    pub const ASSIGN_PAN: u8 = 0x2A;
    pub const ASSIGN_PLUGIN: u8 = 0x2B;
    pub const ASSIGN_EQ: u8 = 0x2C;
    pub const ASSIGN_INSTRUMENT: u8 = 0x2D;
    pub const BANK_LEFT: u8 = 0x2E;
    pub const BANK_RIGHT: u8 = 0x2F;
    pub const CHANNEL_LEFT: u8 = 0x30;
    pub const CHANNEL_RIGHT: u8 = 0x31;
    pub const FLIP: u8 = 0x32;
    pub const GLOBAL: u8 = 0x33;
    pub const NAME_VALUE: u8 = 0x34;
    pub const SMPTE_BEATS: u8 = 0x35;
    pub const F1: u8 = 0x36;
    pub const F8: u8 = 0x3D;
    pub const SHIFT: u8 = 0x46;
    pub const OPTION: u8 = 0x47;
    pub const CONTROL: u8 = 0x48;
    pub const CMD_ALT: u8 = 0x49;
    pub const READ: u8 = 0x4A;
    pub const WRITE: u8 = 0x4B;
    pub const TRIM: u8 = 0x4C;
    pub const TOUCH: u8 = 0x4D;
    pub const LATCH: u8 = 0x4E;
    pub const GROUP: u8 = 0x4F;
    pub const SAVE: u8 = 0x50;
    pub const UNDO: u8 = 0x51;
    pub const CANCEL: u8 = 0x52;
    pub const ENTER: u8 = 0x53;
    pub const MARKER: u8 = 0x54;
    pub const NUDGE: u8 = 0x55;
    pub const CYCLE: u8 = 0x56;
    pub const DROP: u8 = 0x57;
    pub const REPLACE: u8 = 0x58;
    pub const CLICK: u8 = 0x59;
    pub const SOLO_BTN: u8 = 0x5A;
    pub const REWIND: u8 = 0x5B;
    pub const FORWARD: u8 = 0x5C;
    pub const STOP: u8 = 0x5D;
    pub const PLAY: u8 = 0x5E;
    pub const RECORD: u8 = 0x5F;
    pub const UP: u8 = 0x60;
    pub const DOWN: u8 = 0x61;
    pub const LEFT: u8 = 0x62;
    pub const RIGHT: u8 = 0x63;
    pub const ZOOM: u8 = 0x64;
    pub const SCRUB: u8 = 0x65;
    pub const FADER_TOUCH: u8 = 0x68;
    pub const SMPTE_LED: u8 = 0x71;
    pub const BEATS_LED: u8 = 0x72;
    pub const RUDE_SOLO: u8 = 0x73;
}

/// Reads a Mackie Control message.
pub fn mcu_parse(b: &[u8]) -> Option<Mcu> {
    let s = *b.first()?;
    match s & 0xF0 {
        0xE0 if b.len() >= 3 => {
            let v = ((b[2] as i32) << 7 | b[1] as i32) as f32 / 16383.0;
            Some(Mcu::Fader(s & 15, v.clamp(0.0, 1.0)))
        }
        0xB0 if b.len() >= 3 && (0x10..0x18).contains(&b[1]) => Some(Mcu::VPot(b[1] - 0x10, rel_signed(b[2]))),
        0xB0 if b.len() >= 3 && b[1] == 0x3C => Some(Mcu::Jog(rel_signed(b[2]))),
        0x90 if b.len() >= 3 && (mcu::FADER_TOUCH..=mcu::FADER_TOUCH + 8).contains(&b[1]) => Some(Mcu::Touch(b[1] - mcu::FADER_TOUCH, b[2] >= 64)),
        0x90 | 0x80 if b.len() >= 3 => Some(Mcu::Button(b[1], s & 0xF0 == 0x90 && b[2] >= 64)),
        _ => None,
    }
}

/// Mackie's relative encoding: bit 6 set = counter-clockwise, the low six bits the count.
fn rel_signed(v: u8) -> i32 {
    let n = (v & 0x3F) as i32;
    if v & 0x40 != 0 { -n } else { n }
}

/// Motor fader position (pitch bend on the strip's channel).
pub fn mcu_fader(strip: u8, v: f32) -> [u8; 3] {
    let x = (v.clamp(0.0, 1.0) * 16383.0).round() as u16;
    [0xE0 | (strip & 15), (x & 127) as u8, (x >> 7) as u8]
}

/// A button LED (off, on, flashing).
pub fn mcu_led(note: u8, state: u8) -> [u8; 3] {
    [0x90, note, match state {
        0 => 0,
        2 => 1,
        _ => 0x7F,
    }]
}

/// A V-Pot LED ring: mode (0 single dot, 1 boost/cut from centre, 2 wrap, 3 spread), value 0–1.
pub fn mcu_ring(strip: u8, mode: u8, v: f32, center_led: bool) -> [u8; 3] {
    let pos = 1 + (v.clamp(0.0, 1.0) * 10.0).round() as u8;
    [0xB0, 0x30 + (strip & 7), ((center_led as u8) << 6) | ((mode & 3) << 4) | pos.min(11)]
}

/// The LCD: text at an offset (0–111: top row 0–55, bottom row 56–111).
pub fn mcu_lcd(offset: u8, text: &str, extender: bool) -> Vec<u8> {
    let mut v = vec![0xF0, 0x00, 0x00, 0x66, if extender { 0x15 } else { 0x14 }, 0x12, offset];
    v.extend(text.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() { c as u8 } else { b'?' }));
    v.push(0xF7);
    v
}

/// The time code / beats display: ten digits from the right (CC 0x40–0x49), with a dot after
/// the digits marked.
pub fn mcu_timecode(text: &str) -> Vec<[u8; 3]> {
    let digits: Vec<char> = text.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ').collect();
    let mut out = Vec::new();
    for (k, c) in digits.iter().rev().take(10).enumerate() {
        let code = match c {
            '0'..='9' => 0x30 + (*c as u8 - b'0'),
            'A'..='Z' => *c as u8 - b'A' + 1,
            'a'..='z' => *c as u8 - b'a' + 1,
            _ => 0x20,
        };
        out.push([0xB0, 0x40 + k as u8, code]);
    }
    out
}

/// Seven characters of a strip's name for the LCD (Mackie shows 7 per strip).
pub fn lcd_cell(s: &str) -> String {
    let t: String = s.chars().filter(|c| c.is_ascii()).take(6).collect();
    format!("{t:<7}")
}

// ---- HUI ----

/// What a HUI message means.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hui {
    Fader(u8, f32),
    VPot(u8, i32),
    /// zone, port, pressed.
    Switch(u8, u8, bool),
    Ping,
}

/// Reads HUI: faders as controller pairs (0x00–07 high, 0x20–27 low), V-Pots 0x40–47, switches
/// as zone select (CC 0x0F) then port (CC 0x2F, bit 6 = on). `zone`: the last zone selected.
pub fn hui_parse(b: &[u8], zone: &mut u8, fader_hi: &mut [u8; 8]) -> Option<Hui> {
    if b.len() < 3 {
        return None;
    }
    match (b[0], b[1]) {
        (0x90, 0) if b[2] == 0x7F => Some(Hui::Ping),
        (0xB0, c) if c < 8 => {
            fader_hi[c as usize] = b[2];
            None
        }
        (0xB0, c) if (0x20..0x28).contains(&c) => {
            let i = (c - 0x20) as usize;
            let v = ((fader_hi[i] as u32) << 7 | b[2] as u32) as f32 / 16383.0;
            Some(Hui::Fader(i as u8, v.clamp(0.0, 1.0)))
        }
        (0xB0, c) if (0x40..0x48).contains(&c) => {
            let v = b[2] as i32;
            Some(Hui::VPot(c - 0x40, if v > 64 { v - 64 } else { -v }))
        }
        (0xB0, 0x0F) => {
            *zone = b[2];
            None
        }
        (0xB0, 0x2F) => Some(Hui::Switch(*zone, b[2] & 0x0F, b[2] & 0x40 != 0)),
        _ => None,
    }
}

/// HUI switch zones: channel strips 0–7 (ports: 0 fader touch, 1 select, 2 mute, 3 solo,
/// 4 auto, 5 V-Sel, 6 insert, 7 record ready) and transport (zone 0x0E: 1 rewind, 2 fast
/// forward, 3 stop, 4 play, 5 record).
pub const HUI_TRANSPORT: u8 = 0x0E;

/// HUI LED: zone select then port on/off.
pub fn hui_led(zone: u8, port: u8, on: bool) -> [[u8; 3]; 2] {
    [[0xB0, 0x0C, zone], [0xB0, 0x2C, (port & 15) | if on { 0x40 } else { 0 }]]
}

/// HUI fader position.
pub fn hui_fader(strip: u8, v: f32) -> [[u8; 3]; 2] {
    let x = (v.clamp(0.0, 1.0) * 16383.0).round() as u16;
    [[0xB0, strip & 7, (x >> 7) as u8], [0xB0, 0x20 + (strip & 7), (x & 127) as u8]]
}

/// The host's ping to a HUI (it answers 90 00 7F).
pub const HUI_PING: [u8; 3] = [0x90, 0x00, 0x00];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_change_messages() {
        let p = parse_msg("B0 07 Lo7");
        assert_eq!(match_msg(&p, &[0xB0, 7, 99]), Some((99, 7)));
        assert_eq!(match_msg(&p, &[0xB0, 8, 99]), None);
        // 14-bit with constant bytes between
        let p = parse_msg("B0 08 Hi7 B0 28 Lo7");
        assert_eq!(match_msg(&p, &[0xB0, 8, 1, 0xB0, 0x28, 2]), Some((130, 14)));
        // no placeholder: a button, value 1
        assert_eq!(match_msg(&parse_msg("90 3C 7F"), &[0x90, 0x3C, 0x7F]), Some((1, 0)));
        assert_eq!(msg_text(&parse_msg("b0 07 lo7")), "B0 07 Lo7");
        assert!(msg_meaning(&parse_msg("B0 07 Lo7")).starts_with("Control 7 on channel 1"));
    }

    #[test]
    fn formats() {
        assert_eq!(Format::Unsigned.decode(0x7F, 7), 127);
        assert_eq!(Format::TwosComplement.decode(0x7F, 7), -1);
        assert_eq!(Format::OnesComplement.decode(0x7F, 7), 0);
        assert_eq!(Format::SignMagnitude.decode(0x41, 7), -1);
        assert_eq!(Format::TwosComplement.decode(0x2000, 14), -8192);
    }

    #[test]
    fn value_modes() {
        let mut a = Assignment::learned("", parse_msg("B0 07 Lo7"), Class::Strip { strip: Strip::Selected, param: StripParam::Volume });
        assert_eq!(a.value_mode, ValueMode::Scaled);
        assert_eq!(a.apply(127, 0.2, 128.0), Some(1.0));
        assert_eq!(a.apply(0, 0.2, 128.0), Some(0.0));
        a.value_mode = ValueMode::Relative;
        a.multiply = -1.0;
        assert!((a.apply(1, 0.5, 100.0).unwrap() - 0.49).abs() < 1e-6);
        a.value_mode = ValueMode::Toggle;
        a.multiply = 1.0;
        assert_eq!(a.apply(1, 1.0, 1.0), Some(0.0));
        assert_eq!(a.apply(0, 1.0, 1.0), None);
        a.value_mode = ValueMode::Rotate;
        assert_eq!(a.apply(5, 0.99, 100.0), Some(0.0));
        a.value_mode = ValueMode::Scaled;
        a.only = true;
        assert_eq!(a.apply(50, 0.0, 128.0), None);
        // a mute learned from a button toggles
        let m = Assignment::learned("", parse_msg("90 10 7F"), Class::Strip { strip: Strip::Selected, param: StripParam::Mute });
        assert_eq!(m.value_mode, ValueMode::Toggle);
        let l = m.to_line();
        assert_eq!(Assignment::from_line(&l), Some(m));
    }

    #[test]
    fn zones_modes_exclusive() {
        let mut s = Assignments::default();
        s.zones.push(("Encoders".into(), vec!["Pan".into(), "Send".into()]));
        s.active.insert("Encoders".into(), "Pan".into());
        let mut pan = Assignment::learned("", parse_msg("B0 10 Lo7"), Class::Strip { strip: Strip::FaderBank(0), param: StripParam::Pan });
        pan.zone = "Encoders".into();
        pan.mode = "Pan".into();
        let mut send = pan.clone();
        send.mode = "Send".into();
        send.class = Class::Strip { strip: Strip::FaderBank(0), param: StripParam::Send(0) };
        s.list = vec![pan, send];
        assert_eq!(s.matching("", &[0xB0, 0x10, 5], 0), vec![(0, 5)]);
        s.change_mode("Encoders", "Send", 0, 1);
        assert_eq!(s.matching("", &[0xB0, 0x10, 5], 0), vec![(1, 5)]);
        // Toggle returns to the zone's first mode; Rotate goes round
        s.change_mode("Encoders", "Send", 1, 1);
        assert_eq!(s.active["Encoders"], "Pan");
        s.change_mode("Encoders", "", 3, 1);
        assert_eq!(s.active["Encoders"], "Send");
        // an exclusive modeless assignment takes the control over
        let mut ex = Assignment::learned("", parse_msg("B0 10 Lo7"), Class::Global { param: "Playhead".into(), number: 0 });
        ex.exclusive = true;
        s.list.push(ex);
        assert_eq!(s.matching("", &[0xB0, 0x10, 5], 0), vec![(2, 5)]);
        let t = s.to_text();
        assert_eq!(Assignments::from_text(&t), s);
    }

    #[test]
    fn series_fill() {
        let f = fill_series(&parse_msg("B0 10 Lo7"), &parse_msg("B0 17 Lo7"), 8).unwrap();
        assert_eq!(f.len(), 8);
        assert_eq!(f[3], parse_msg("B0 13 Lo7"));
        assert!(fill_series(&parse_msg("B0 10 Lo7"), &parse_msg("B0 17 Lo7"), 7).is_none());
        let c = fill_series(&parse_msg("B0 07 Lo7"), &parse_msg("B3 07 Lo7"), 4).unwrap();
        assert_eq!(c[2], parse_msg("B2 07 Lo7"));
    }

    #[test]
    fn mackie() {
        assert_eq!(mcu_parse(&[0xE2, 0x7F, 0x7F]), Some(Mcu::Fader(2, 1.0)));
        assert_eq!(mcu_parse(&[0xB0, 0x11, 0x43]), Some(Mcu::VPot(1, -3)));
        assert_eq!(mcu_parse(&[0xB0, 0x3C, 0x01]), Some(Mcu::Jog(1)));
        assert_eq!(mcu_parse(&[0x90, mcu::PLAY, 0x7F]), Some(Mcu::Button(mcu::PLAY, true)));
        assert_eq!(mcu_parse(&[0x90, 0x68, 0x7F]), Some(Mcu::Touch(0, true)));
        assert_eq!(mcu_fader(0, 1.0), [0xE0, 0x7F, 0x7F]);
        assert_eq!(mcu_lcd(0, "Ab", false), vec![0xF0, 0, 0, 0x66, 0x14, 0x12, 0, b'A', b'b', 0xF7]);
        assert_eq!(mcu_timecode("1"), vec![[0xB0, 0x40, 0x31]]);
        assert_eq!(lcd_cell("Vocals Lead"), "Vocals ");
    }

    #[test]
    fn hui() {
        let (mut z, mut hi) = (0u8, [0u8; 8]);
        assert_eq!(hui_parse(&[0xB0, 0x00, 0x7F], &mut z, &mut hi), None);
        assert_eq!(hui_parse(&[0xB0, 0x20, 0x7F], &mut z, &mut hi), Some(Hui::Fader(0, 1.0)));
        hui_parse(&[0xB0, 0x0F, HUI_TRANSPORT], &mut z, &mut hi);
        assert_eq!(hui_parse(&[0xB0, 0x2F, 0x44], &mut z, &mut hi), Some(Hui::Switch(HUI_TRANSPORT, 4, true)));
        assert_eq!(hui_parse(&[0x90, 0x00, 0x7F], &mut z, &mut hi), Some(Hui::Ping));
    }

    #[test]
    fn labels() {
        let l = expand_label("Send@s#", |t, v| if t == 's' && v == '#' { "1".into() } else { String::new() });
        assert_eq!(l, "Send1");
    }
}
