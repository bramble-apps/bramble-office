//! SysEx faders (Environment): a fader whose Output (or Input) is SysEx sends a list of events
//! entered in the SysEx Fader Editor. Selected events take the fader's value; SysEx strings
//! carry a VAL slot (the value, in the chosen Value format) and an optional SUM slot (the
//! checksum, in the chosen Checksum format). F0 and the EOX end byte (F7) are implicit.
//!
//! Saved in the fader's texts["sysex"] (one event per line) and params sx_sum / sx_pos / sx_fmt.

/// A SysEx string's byte: a data byte, the value slot or the checksum slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    B(u8),
    Val,
    Sum,
}

/// One event of a SysEx fader's list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SxEv {
    /// A channel message: status kind (0x90 note, 0xA0 poly aftertouch, 0xB0 control, 0xC0
    /// program, 0xD0 channel aftertouch, 0xE0 pitch bend), channel 1–16, data bytes.
    Chan { st: u8, ch: u8, d1: u8, d2: u8, sel: bool },
    /// A meta event: number, value.
    Meta { num: u8, val: u8, sel: bool },
    /// A System Exclusive string (without F0 and F7).
    Sysex { toks: Vec<Tok>, sel: bool },
}

/// The eight event type buttons of the SysEx fader window (Command-click makes a new event).
pub const EVENT_TYPES: [&str; 8] = ["Note", "Program Number", "Pitch Bend", "Controller", "Channel Aftertouch", "Poly Aftertouch", "SysEx", "Meta Event"];
pub const CHECKSUMS: [&str; 6] = ["Off", "Roland", "Yamaha", "Regular Checksum", "2's Complement", "1's Complement"];
pub const FORMATS: [&str; 15] = ["Auto", "One Byte", "MSB/LSB", "LSB/MSB", "BCD 4 LSB", "BCD 4 MSB", "2 Nibbles L", "3 Nibbles L", "4 Nibbles L", "2 Nibbles M", "3 Nibbles M", "4 Nibbles M", "2 ASCII M", "3 ASCII M", "4 ASCII M"];
/// Value positions: Auto, then Last, Last-1 … Last-14 (counted from the end, before EOX).
pub fn positions() -> Vec<String> {
    let mut v = vec!["Auto".to_string(), "Last".to_string()];
    for k in 1..15 {
        v.push(format!("Last-{k}"));
    }
    v
}

impl SxEv {
    /// A new event of a button's type (index into EVENT_TYPES), selected.
    pub fn new_of(kind: usize) -> SxEv {
        match kind {
            0 => SxEv::Chan { st: 0x90, ch: 1, d1: 60, d2: 100, sel: true },
            1 => SxEv::Chan { st: 0xC0, ch: 1, d1: 0, d2: 0, sel: true },
            2 => SxEv::Chan { st: 0xE0, ch: 1, d1: 0, d2: 64, sel: true },
            3 => SxEv::Chan { st: 0xB0, ch: 1, d1: 7, d2: 100, sel: true },
            4 => SxEv::Chan { st: 0xD0, ch: 1, d1: 0, d2: 0, sel: true },
            5 => SxEv::Chan { st: 0xA0, ch: 1, d1: 60, d2: 0, sel: true },
            7 => SxEv::Meta { num: 0, val: 0, sel: true },
            // (Syst. Exclusive: a generic message, manufacturer 0 and the value)
            _ => SxEv::Sysex { toks: vec![Tok::B(0), Tok::Val], sel: true },
        }
    }
    pub fn sel(&self) -> bool {
        match self {
            SxEv::Chan { sel, .. } | SxEv::Meta { sel, .. } | SxEv::Sysex { sel, .. } => *sel,
        }
    }
    pub fn set_sel(&mut self, on: bool) {
        match self {
            SxEv::Chan { sel, .. } | SxEv::Meta { sel, .. } | SxEv::Sysex { sel, .. } => *sel = on,
        }
    }
    /// The event type's name (the Status column).
    pub fn status_name(&self) -> &'static str {
        match self {
            SxEv::Chan { st: 0x90, .. } => "Note",
            SxEv::Chan { st: 0xA0, .. } => "Poly Aftertouch",
            SxEv::Chan { st: 0xB0, .. } => "Control",
            SxEv::Chan { st: 0xC0, .. } => "Program",
            SxEv::Chan { st: 0xD0, .. } => "Aftertouch",
            SxEv::Chan { .. } => "Pitch Bend",
            SxEv::Meta { .. } => "Meta",
            SxEv::Sysex { .. } => "SysEx",
        }
    }

    pub fn to_line(&self) -> String {
        match self {
            SxEv::Chan { st, ch, d1, d2, sel } => format!("c\t{}\t{st}\t{ch}\t{d1}\t{d2}", *sel as u8),
            SxEv::Meta { num, val, sel } => format!("m\t{}\t{num}\t{val}", *sel as u8),
            SxEv::Sysex { toks, sel } => format!("s\t{}\t{}", *sel as u8, toks_text(toks, false)),
        }
    }
    pub fn from_line(l: &str) -> Option<SxEv> {
        let f: Vec<&str> = l.split('\t').collect();
        let n = |i: usize| f.get(i).and_then(|x| x.parse::<u8>().ok()).unwrap_or(0);
        let sel = f.get(1) == Some(&"1");
        Some(match *f.first()? {
            "c" => SxEv::Chan { st: n(2), ch: n(3).clamp(1, 16), d1: n(4) & 127, d2: n(5) & 127, sel },
            "m" => SxEv::Meta { num: n(2) & 127, val: n(3) & 127, sel },
            "s" => SxEv::Sysex { toks: parse_toks(f.get(2).unwrap_or(&""))?, sel },
            _ => return None,
        })
    }
}

/// A data byte typed in the Num/data columns: decimal, or hex with a $ prefix.
pub fn parse_byte(s: &str) -> Option<u8> {
    let s = s.trim();
    let v = if let Some(h) = s.strip_prefix('$') { u32::from_str_radix(h, 16).ok()? } else { s.parse::<u32>().ok()? };
    (v < 128).then_some(v as u8)
}

/// A byte as shown: decimal, or "$7F" with View ▸ SysEx in Hex Format.
pub fn byte_text(b: u8, hex: bool) -> String {
    if hex { format!("${b:02X}") } else { b.to_string() }
}

pub fn toks_text(t: &[Tok], hex: bool) -> String {
    t.iter().map(|x| match x {
        Tok::B(b) => byte_text(*b, hex),
        Tok::Val => "VAL".into(),
        Tok::Sum => "SUM".into(),
    }).collect::<Vec<_>>().join(" ")
}

/// Parses a SysEx string: bytes (decimal or $hex), VAL, SUM; F0 / F7 at the ends are dropped.
pub fn parse_toks(s: &str) -> Option<Vec<Tok>> {
    let mut v = Vec::new();
    for w in s.split_whitespace() {
        match w.to_ascii_uppercase().as_str() {
            "VAL" => v.push(Tok::Val),
            "SUM" => v.push(Tok::Sum),
            "$F0" | "240" | "SYSEX" if v.is_empty() => {}
            "$F7" | "247" | "EOX" => {}
            _ => v.push(Tok::B(parse_byte(w)?)),
        }
    }
    Some(v)
}

/// The value as the bytes of a Value format (index into FORMATS).
pub fn encode_value(fmt: usize, v: i32) -> Vec<u8> {
    let v = v.max(0) as u32;
    let nib_l = |n: usize| (0..n).map(|k| ((v >> (4 * k)) & 15) as u8).collect::<Vec<u8>>();
    let nib_m = |n: usize| (0..n).rev().map(|k| ((v >> (4 * k)) & 15) as u8).collect::<Vec<u8>>();
    let ascii = |b: u8| if b < 10 { b'0' + b } else { b'A' + b - 10 };
    let bcd = || {
        let d = v.min(9999);
        [d % 10, d / 10 % 10, d / 100 % 10, d / 1000 % 10].iter().map(|x| *x as u8).collect::<Vec<u8>>()
    };
    match fmt {
        // Auto: one byte up to 127, else two, MSB first
        0 if v <= 127 => vec![v as u8],
        0 | 2 => vec![((v >> 7) & 127) as u8, (v & 127) as u8],
        1 => vec![(v & 127) as u8],
        3 => vec![(v & 127) as u8, ((v >> 7) & 127) as u8],
        4 => bcd(),
        5 => bcd().into_iter().rev().collect(),
        6..=8 => nib_l(fmt - 4),
        9..=11 => nib_m(fmt - 7),
        12..=14 => nib_m(fmt - 10).into_iter().map(ascii).collect(),
        _ => vec![(v & 127) as u8],
    }
}

/// The checksum (index into CHECKSUMS) of a message's bytes (without F0, up to the SUM slot):
/// Roland and Yamaha: the 2's complement of the address and data bytes (after the four- or
/// three-byte header); Regular: the sum; 2's / 1's complement of the bytes after the
/// manufacturer ID. Always 7 bits.
pub fn checksum(kind: usize, bytes: &[u8]) -> Option<u8> {
    let sum = |from: usize| bytes.iter().skip(from).fold(0u32, |a, b| a + *b as u32);
    Some(match kind {
        1 => ((128 - sum(4) % 128) % 128) as u8,
        2 => ((128 - sum(3) % 128) % 128) as u8,
        3 => (sum(1) & 127) as u8,
        4 => ((128 - sum(1) % 128) % 128) as u8,
        5 => (!(sum(1) as u8)) & 127,
        _ => return None,
    })
}

/// Moves the VAL slot (and adds or removes SUM) as the Position and Checksum menus say:
/// Auto puts the value last, or second to last when a checksum is used; Last-n counts back
/// from the byte before EOX.
pub fn place(toks: &mut Vec<Tok>, pos: usize, sum: usize) {
    toks.retain(|t| *t != Tok::Val && *t != Tok::Sum);
    if sum != 0 {
        toks.push(Tok::Sum);
    }
    let from_end = if pos == 0 { if sum != 0 { 1 } else { 0 } } else { pos - 1 };
    let at = toks.len().saturating_sub(from_end);
    toks.insert(at, Tok::Val);
}

/// One message a SysEx fader sends: a channel or meta event, or raw SysEx bytes (F0 … F7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    Chan { st: u8, ch: u8, d1: u8, d2: u8 },
    Meta { num: u8, val: u8 },
    Raw(Vec<u8>),
}

/// The fader's list for value `v`: selected events take the value, others go as entered.
pub fn render(list: &[SxEv], v: i32, fmt: usize, sum: usize) -> Vec<Msg> {
    let v7 = v.clamp(0, 127) as u8;
    list.iter().map(|e| match e {
        SxEv::Chan { st, ch, d1, d2, sel } => {
            let (mut a, mut b) = (*d1, *d2);
            if *sel {
                match *st {
                    0xC0 | 0xD0 => a = v7,
                    0xE0 => {
                        let w = v.clamp(0, 16383) as u32;
                        let w = if v <= 127 { w << 7 } else { w };
                        a = (w & 127) as u8;
                        b = (w >> 7) as u8;
                    }
                    _ => b = v7,
                }
            }
            Msg::Chan { st: *st, ch: ch.saturating_sub(1), d1: a, d2: b }
        }
        SxEv::Meta { num, val, sel } => Msg::Meta { num: *num, val: if *sel { v7 } else { *val } },
        SxEv::Sysex { toks, sel } => {
            let mut b = vec![0xF0];
            for t in toks {
                match t {
                    Tok::B(x) => b.push(*x),
                    Tok::Val => b.extend(if *sel { encode_value(fmt, v) } else { vec![0] }),
                    Tok::Sum => {
                        if let Some(c) = checksum(sum, &b[1..]) {
                            b.push(c);
                        }
                    }
                }
            }
            b.push(0xF7);
            Msg::Raw(b)
        }
    }).collect()
}

pub fn list_from(text: Option<&String>) -> Vec<SxEv> {
    text.map(|t| t.lines().filter_map(SxEv::from_line).collect()).unwrap_or_default()
}
pub fn list_text(list: &[SxEv]) -> String {
    list.iter().map(|e| e.to_line()).collect::<Vec<_>>().join("\n")
}

/// A captured SysEx message (MIDI In in the editor): F0 … F7 as a new selected event.
pub fn capture(bytes: &[u8]) -> Option<SxEv> {
    if bytes.first() != Some(&0xF0) {
        return None;
    }
    let toks = bytes[1..].iter().take_while(|b| **b != 0xF7).map(|b| Tok::B(*b & 127)).collect();
    Some(SxEv::Sysex { toks, sel: true })
}

/// SysEx as a fader's Input: only very short messages (up to 4 data bytes, with the value in
/// the last) are recognised; the value they carry.
pub fn short_input(bytes: &[u8], list: &[SxEv]) -> Option<i32> {
    if bytes.first() != Some(&0xF0) || bytes.len() > 7 {
        return None;
    }
    let data: Vec<u8> = bytes[1..].iter().copied().take_while(|b| *b != 0xF7).collect();
    let (val, head) = data.split_last()?;
    let pat = list.iter().find_map(|e| match e {
        SxEv::Sysex { toks, .. } => Some(toks.iter().filter_map(|t| if let Tok::B(b) = t { Some(*b) } else { None }).collect::<Vec<u8>>()),
        _ => None,
    })?;
    (pat == head).then_some(*val as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_formats() {
        assert_eq!(encode_value(0, 100), vec![100]);
        assert_eq!(encode_value(0, 300), vec![2, 44]);
        assert_eq!(encode_value(1, 300), vec![44]);
        assert_eq!(encode_value(3, 300), vec![44, 2]);
        assert_eq!(encode_value(4, 1234), vec![4, 3, 2, 1]);
        assert_eq!(encode_value(5, 1234), vec![1, 2, 3, 4]);
        assert_eq!(encode_value(6, 127), vec![15, 7]);
        assert_eq!(encode_value(9, 127), vec![7, 15]);
        assert_eq!(encode_value(10, 127), vec![0, 7, 15]);
        assert_eq!(encode_value(12, 127), vec![b'7', b'F']);
        assert_eq!(encode_value(8, 0x1234), vec![4, 3, 2, 1]);
    }

    #[test]
    fn checksums_and_positions() {
        // Roland DT1: F0 41 10 42 12 40 00 7F 00 41 F7 (the GS reset's checksum is 0x41)
        let b = [0x41, 0x10, 0x42, 0x12, 0x40, 0x00, 0x7F, 0x00];
        assert_eq!(checksum(1, &b), Some(0x41));
        assert_eq!(checksum(0, &b), None);
        assert_eq!(checksum(3, &[1, 2, 3]), Some(5));
        assert_eq!(checksum(4, &[1, 2, 3]), Some(123));
        assert_eq!(checksum(5, &[1, 2, 3]), Some(!5u8 & 127));
        let mut t = vec![Tok::B(65), Tok::B(16), Tok::B(66)];
        place(&mut t, 0, 0);
        assert_eq!(t.last(), Some(&Tok::Val));
        place(&mut t, 0, 1);
        assert_eq!(&t[3..], &[Tok::Val, Tok::Sum]);
        place(&mut t, 3, 0);
        assert_eq!(t, vec![Tok::B(65), Tok::Val, Tok::B(16), Tok::B(66)]);
    }

    #[test]
    fn render_selected_only() {
        let list = vec![
            SxEv::Sysex { toks: vec![Tok::B(0x41), Tok::B(0x10), Tok::B(0x42), Tok::B(0x12), Tok::B(0x40), Tok::B(0), Tok::Val, Tok::Sum], sel: true },
            SxEv::Chan { st: 0xB0, ch: 2, d1: 7, d2: 99, sel: false },
            SxEv::Chan { st: 0xB0, ch: 2, d1: 10, d2: 0, sel: true },
        ];
        let m = render(&list, 127, 0, 1);
        assert_eq!(m[0], Msg::Raw(vec![0xF0, 0x41, 0x10, 0x42, 0x12, 0x40, 0x00, 0x7F, 0x41, 0xF7]));
        assert_eq!(m[1], Msg::Chan { st: 0xB0, ch: 1, d1: 7, d2: 99 });
        assert_eq!(m[2], Msg::Chan { st: 0xB0, ch: 1, d1: 10, d2: 127 });
        let t = list_text(&list);
        assert_eq!(list_from(Some(&t)), list);
    }

    #[test]
    fn typing_and_capture() {
        assert_eq!(parse_byte("$7F"), Some(127));
        assert_eq!(parse_byte("65"), Some(65));
        assert_eq!(parse_byte("200"), None);
        assert_eq!(parse_toks("$41 16 VAL SUM"), Some(vec![Tok::B(65), Tok::B(16), Tok::Val, Tok::Sum]));
        assert_eq!(toks_text(&[Tok::B(65), Tok::Val], true), "$41 VAL");
        assert_eq!(capture(&[0xF0, 0x43, 0x10, 0xF7]), Some(SxEv::Sysex { toks: vec![Tok::B(0x43), Tok::B(0x10)], sel: true }));
        let l = vec![SxEv::Sysex { toks: vec![Tok::B(0x7D), Tok::B(1), Tok::Val], sel: true }];
        assert_eq!(short_input(&[0xF0, 0x7D, 1, 55, 0xF7], &l), Some(55));
        assert_eq!(short_input(&[0xF0, 0x7D, 2, 55, 0xF7], &l), None);
    }
}
