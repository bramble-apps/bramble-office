//! Logic's MIDI Transform window (Window ▸ Open MIDI Transform, ⌘9; Functions ▸ MIDI Transform):
//! Select Conditions and Operations on each note's Position, Pitch, Velocity, Length and
//! Channel, and Logic's presets (Fixed Velocity, Humanize, Crescendo, Half / Double Speed,
//! Reverse Position, Quantize Note Length…). Select Only picks the notes that meet the
//! conditions, Operate Only changes the selected notes that meet them, Select and Operate does
//! both.

use crate::events::{Note, PPQ};

/// The note parameters the window has rows for.
pub const PARAMS: [&str; 5] = ["Position", "Pitch", "Velocity", "Length", "Channel"];

/// A Select Condition (Logic's: All, =, ≠, <, >, Inside, Outside).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cond {
    All,
    Equal(i64),
    Unequal(i64),
    Smaller(i64),
    Greater(i64),
    Inside(i64, i64),
    Outside(i64, i64),
}

impl Cond {
    pub const NAMES: [&'static str; 7] = ["All", "=", "≠", "<", ">", "Inside", "Outside"];

    pub fn index(&self) -> usize {
        match self {
            Cond::All => 0,
            Cond::Equal(_) => 1,
            Cond::Unequal(_) => 2,
            Cond::Smaller(_) => 3,
            Cond::Greater(_) => 4,
            Cond::Inside(..) => 5,
            Cond::Outside(..) => 6,
        }
    }

    /// The condition `i` (NAMES order) keeping this one's values.
    pub fn with_index(&self, i: usize) -> Cond {
        let (a, b) = self.values();
        match i {
            1 => Cond::Equal(a),
            2 => Cond::Unequal(a),
            3 => Cond::Smaller(a),
            4 => Cond::Greater(a),
            5 => Cond::Inside(a, b),
            6 => Cond::Outside(a, b),
            _ => Cond::All,
        }
    }

    pub fn values(&self) -> (i64, i64) {
        match *self {
            Cond::All => (0, 0),
            Cond::Equal(a) | Cond::Unequal(a) | Cond::Smaller(a) | Cond::Greater(a) => (a, a),
            Cond::Inside(a, b) | Cond::Outside(a, b) => (a, b),
        }
    }

    pub fn set_values(&mut self, a: i64, b: i64) {
        *self = match *self {
            Cond::All => Cond::All,
            Cond::Equal(_) => Cond::Equal(a),
            Cond::Unequal(_) => Cond::Unequal(a),
            Cond::Smaller(_) => Cond::Smaller(a),
            Cond::Greater(_) => Cond::Greater(a),
            Cond::Inside(..) => Cond::Inside(a, b),
            Cond::Outside(..) => Cond::Outside(a, b),
        };
    }

    /// How many values it takes.
    pub fn arity(&self) -> usize {
        match self {
            Cond::All => 0,
            Cond::Inside(..) | Cond::Outside(..) => 2,
            _ => 1,
        }
    }

    pub fn test(&self, v: i64) -> bool {
        match *self {
            Cond::All => true,
            Cond::Equal(a) => v == a,
            Cond::Unequal(a) => v != a,
            Cond::Smaller(a) => v < a,
            Cond::Greater(a) => v > a,
            Cond::Inside(a, b) => v >= a.min(b) && v <= a.max(b),
            Cond::Outside(a, b) => v < a.min(b) || v > a.max(b),
        }
    }
}

/// An Operation (Logic's names). Values are in the parameter's own units (ticks, MIDI values);
/// Mul / Div / Scale / Expo take a factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Thru,
    Fix(i64),
    Add(i64),
    Sub(i64),
    Min(i64),
    Max(i64),
    Flip(i64),
    Mul(f64),
    Div(f64),
    Scale(f64, i64),
    Range(i64, i64),
    Rand(i64, i64),
    PlusMinusRand(i64),
    Quantize(i64),
    QUp(i64),
    Expo(f64),
    /// From the first selected note's value to the last's, by position.
    Crescendo(i64, i64),
    /// Mirrored within the selected notes' range.
    Reverse,
}

impl Op {
    pub const NAMES: [&'static str; 18] = ["Thru", "Fix", "Add", "Sub", "Min", "Max", "Flip", "Mul", "Div", "Scale", "Range", "Random", "+- Rand", "Quantize", "Q Up", "Expo", "Crescendo", "Reverse"];

    pub fn index(&self) -> usize {
        match self {
            Op::Thru => 0,
            Op::Fix(_) => 1,
            Op::Add(_) => 2,
            Op::Sub(_) => 3,
            Op::Min(_) => 4,
            Op::Max(_) => 5,
            Op::Flip(_) => 6,
            Op::Mul(_) => 7,
            Op::Div(_) => 8,
            Op::Scale(..) => 9,
            Op::Range(..) => 10,
            Op::Rand(..) => 11,
            Op::PlusMinusRand(_) => 12,
            Op::Quantize(_) => 13,
            Op::QUp(_) => 14,
            Op::Expo(_) => 15,
            Op::Crescendo(..) => 16,
            Op::Reverse => 17,
        }
    }

    /// The operation `i` (NAMES order), keeping this one's values where they fit.
    pub fn with_index(&self, i: usize) -> Op {
        let (a, b) = self.values();
        let f = self.factor();
        match i {
            1 => Op::Fix(a),
            2 => Op::Add(a),
            3 => Op::Sub(a),
            4 => Op::Min(a),
            5 => Op::Max(a),
            6 => Op::Flip(a),
            7 => Op::Mul(f),
            8 => Op::Div(f),
            9 => Op::Scale(f, a),
            10 => Op::Range(a, b),
            11 => Op::Rand(a, b),
            12 => Op::PlusMinusRand(a),
            13 => Op::Quantize(a.max(1)),
            14 => Op::QUp(a.max(1)),
            15 => Op::Expo(f),
            16 => Op::Crescendo(a, b),
            17 => Op::Reverse,
            _ => Op::Thru,
        }
    }

    pub fn values(&self) -> (i64, i64) {
        match *self {
            Op::Fix(a) | Op::Add(a) | Op::Sub(a) | Op::Min(a) | Op::Max(a) | Op::Flip(a) | Op::PlusMinusRand(a) | Op::Quantize(a) | Op::QUp(a) | Op::Scale(_, a) => (a, a),
            Op::Range(a, b) | Op::Rand(a, b) | Op::Crescendo(a, b) => (a, b),
            _ => (0, 0),
        }
    }

    pub fn factor(&self) -> f64 {
        match *self {
            Op::Mul(f) | Op::Div(f) | Op::Scale(f, _) | Op::Expo(f) => f,
            _ => 1.0,
        }
    }

    pub fn set_values(&mut self, a: i64, b: i64, f: f64) {
        *self = match *self {
            Op::Thru => Op::Thru,
            Op::Fix(_) => Op::Fix(a),
            Op::Add(_) => Op::Add(a),
            Op::Sub(_) => Op::Sub(a),
            Op::Min(_) => Op::Min(a),
            Op::Max(_) => Op::Max(a),
            Op::Flip(_) => Op::Flip(a),
            Op::Mul(_) => Op::Mul(f),
            Op::Div(_) => Op::Div(f),
            Op::Scale(..) => Op::Scale(f, a),
            Op::Range(..) => Op::Range(a, b),
            Op::Rand(..) => Op::Rand(a, b),
            Op::PlusMinusRand(_) => Op::PlusMinusRand(a),
            Op::Quantize(_) => Op::Quantize(a.max(1)),
            Op::QUp(_) => Op::QUp(a.max(1)),
            Op::Expo(_) => Op::Expo(f),
            Op::Crescendo(..) => Op::Crescendo(a, b),
            Op::Reverse => Op::Reverse,
        };
    }

    /// What values it shows: (integers, has a factor).
    pub fn shape(&self) -> (usize, bool) {
        match self {
            Op::Thru | Op::Reverse => (0, false),
            Op::Mul(_) | Op::Div(_) | Op::Expo(_) => (0, true),
            Op::Scale(..) => (1, true),
            Op::Range(..) | Op::Rand(..) | Op::Crescendo(..) => (2, false),
            _ => (1, false),
        }
    }
}

/// A small, seedable random source (xorshift), for Random and +- Rand.
#[derive(Clone, Debug)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// In a..=b.
    pub fn range(&mut self, a: i64, b: i64) -> i64 {
        let (lo, hi) = (a.min(b), a.max(b));
        lo + (self.next() % ((hi - lo + 1) as u64)) as i64
    }
}

/// A Transform set: a name, the conditions and the operations, one per parameter (PARAMS).
#[derive(Clone, Debug, PartialEq)]
pub struct Transform {
    pub name: String,
    pub conds: [Cond; 5],
    pub ops: [Op; 5],
}

impl Transform {
    pub fn new(name: &str) -> Transform {
        Transform { name: name.to_string(), conds: [Cond::All; 5], ops: [Op::Thru; 5] }
    }

    fn with(mut self, param: usize, op: Op) -> Transform {
        self.ops[param] = op;
        self
    }

    /// The note's value for a parameter (Position relative to `origin`).
    fn get(n: &Note, p: usize, origin: u32) -> i64 {
        match p {
            0 => n.pos as i64 - origin as i64,
            1 => n.pitch as i64,
            2 => n.velocity as i64,
            3 => n.len as i64,
            _ => n.channel as i64 + 1,
        }
    }

    fn put(n: &mut Note, p: usize, v: i64, origin: u32) {
        match p {
            0 => n.pos = (origin as i64 + v).max(0) as u32,
            1 => n.pitch = v.clamp(0, 127) as u8,
            2 => n.velocity = v.clamp(1, 127) as u8,
            3 => n.len = v.max(1) as u32,
            _ => n.channel = (v.clamp(1, 16) - 1) as u8,
        }
    }

    /// Whether a note meets every condition (Position counted from `origin`, the region's start).
    pub fn matches(&self, n: &Note, origin: u32) -> bool {
        (0..5).all(|p| self.conds[p].test(Self::get(n, p, origin)))
    }

    /// Select Only: the notes that meet the conditions.
    pub fn select(&self, notes: &[Note], origin: u32) -> Vec<usize> {
        (0..notes.len()).filter(|&i| self.matches(&notes[i], origin)).collect()
    }

    /// Operate Only: the operations on the notes in `sel` that meet the conditions (all notes
    /// when `sel` is empty).
    pub fn operate(&self, notes: &mut [Note], sel: &[usize], origin: u32, rng: &mut Rng) {
        let chosen: Vec<usize> = crate::functions::targets(notes, sel).into_iter().filter(|&i| self.matches(&notes[i], origin)).collect();
        if chosen.is_empty() {
            return;
        }
        let before: Vec<Note> = notes.to_vec();
        // the selection's first and last position (Crescendo), each parameter's range (Reverse)
        let first = chosen.iter().map(|&i| before[i].pos).min().unwrap_or(0) as f64;
        let last = chosen.iter().map(|&i| before[i].pos).max().unwrap_or(0) as f64;
        for p in 0..5 {
            let op = self.ops[p];
            if op == Op::Thru {
                continue;
            }
            let lo = chosen.iter().map(|&i| Self::get(&before[i], p, origin)).min().unwrap_or(0);
            let hi = chosen.iter().map(|&i| Self::get(&before[i], p, origin)).max().unwrap_or(0);
            for &i in &chosen {
                let v = Self::get(&before[i], p, origin);
                let out = match op {
                    Op::Thru => v,
                    Op::Fix(a) => a,
                    Op::Add(a) => v + a,
                    Op::Sub(a) => v - a,
                    Op::Min(a) => v.min(a),
                    Op::Max(a) => v.max(a),
                    Op::Flip(a) => 2 * a - v,
                    Op::Mul(f) => (v as f64 * f).round() as i64,
                    Op::Div(f) => if f == 0.0 { v } else { (v as f64 / f).round() as i64 },
                    Op::Scale(f, a) => (v as f64 * f).round() as i64 + a,
                    Op::Range(a, b) => v.clamp(a.min(b), a.max(b)),
                    Op::Rand(a, b) => rng.range(a, b),
                    Op::PlusMinusRand(a) => v + rng.range(-a.abs(), a.abs()),
                    Op::Quantize(q) => ((v as f64 / q.max(1) as f64).round() as i64) * q.max(1),
                    Op::QUp(q) => (v + q.max(1) - 1).div_euclid(q.max(1)) * q.max(1),
                    Op::Expo(f) => {
                        // a curve over the parameter's MIDI range (velocity, pitch: 0..127)
                        let top = 127.0;
                        (top * (v as f64 / top).clamp(0.0, 1.0).powf(f.max(0.01))).round() as i64
                    }
                    Op::Crescendo(a, b) => {
                        let t = if last > first { (before[i].pos as f64 - first) / (last - first) } else { 0.0 };
                        (a as f64 + (b - a) as f64 * t).round() as i64
                    }
                    Op::Reverse => lo + hi - v,
                };
                Self::put(&mut notes[i], p, out, origin);
            }
        }
    }

    /// Select and Operate: selects the matching notes, then operates on them. Returns the new
    /// selection.
    pub fn select_and_operate(&self, notes: &mut [Note], origin: u32, rng: &mut Rng) -> Vec<usize> {
        let sel = self.select(notes, origin);
        if !sel.is_empty() {
            self.operate(notes, &sel, origin, rng);
        }
        sel
    }
}

/// Logic's Transform presets (Functions ▸ MIDI Transform), in its menu order. Half / Double
/// Speed count positions from the selection's first note: see `origin_for`.
pub const PRESETS: [&str; 18] = [
    "Fixed Velocity",
    "Random Pitch",
    "Random Velocity",
    "Random Pitch, Velocity, Length",
    "Crescendo",
    "Scale 14bit PitchBd",
    "Double Speed",
    "Half Speed",
    "Humanize",
    "Reverse Position",
    "Reverse Pitch",
    "Transposition",
    "Exponential Velocity",
    "Velocity Limiter",
    "Fixed Note Length",
    "Maximum Note Length",
    "Minimum Note Length",
    "Quantize Note Length",
];

/// A preset's settings (the values are the window's starting values; change them there).
/// "Scale 14bit PitchBd" works on pitch bend, which Studio's regions don't keep yet: None.
pub fn preset(name: &str) -> Option<Transform> {
    let t = Transform::new(name);
    let sixteenth = (PPQ / 4) as i64;
    Some(match name {
        "Fixed Velocity" => t.with(2, Op::Fix(100)),
        "Random Pitch" => t.with(1, Op::Rand(36, 96)),
        "Random Velocity" => t.with(2, Op::Rand(40, 127)),
        "Random Pitch, Velocity, Length" => t.with(1, Op::PlusMinusRand(12)).with(2, Op::PlusMinusRand(20)).with(3, Op::PlusMinusRand(60)),
        "Crescendo" => t.with(2, Op::Crescendo(40, 120)),
        "Double Speed" => t.with(0, Op::Div(2.0)).with(3, Op::Div(2.0)),
        "Half Speed" => t.with(0, Op::Mul(2.0)).with(3, Op::Mul(2.0)),
        "Humanize" => t.with(0, Op::PlusMinusRand(10)).with(2, Op::PlusMinusRand(10)).with(3, Op::PlusMinusRand(10)),
        "Reverse Position" => t.with(0, Op::Reverse),
        "Reverse Pitch" => t.with(1, Op::Reverse),
        "Transposition" => t.with(1, Op::Add(12)),
        "Exponential Velocity" => t.with(2, Op::Expo(1.5)),
        "Velocity Limiter" => t.with(2, Op::Range(30, 110)),
        "Fixed Note Length" => t.with(3, Op::Fix(sixteenth)),
        "Maximum Note Length" => t.with(3, Op::Min(PPQ as i64)),
        "Minimum Note Length" => t.with(3, Op::Max(sixteenth)),
        "Quantize Note Length" => t.with(3, Op::Quantize(sixteenth)),
        _ => return None,
    })
}

/// Where a transform counts positions from: Half / Double Speed (Mul / Div on Position) from
/// the selection's first note, so the passage stays where it starts; the rest from the region's
/// start.
pub fn origin_for(t: &Transform, notes: &[Note], sel: &[usize], region_start: u32) -> u32 {
    if matches!(t.ops[0], Op::Mul(_) | Op::Div(_) | Op::Scale(..)) {
        crate::functions::targets(notes, sel).iter().map(|&i| notes[i].pos).min().unwrap_or(region_start)
    } else {
        region_start
    }
}

impl Transform {
    /// One line of text (user presets file): name, then each condition and operation by index
    /// and values.
    pub fn to_line(&self) -> String {
        let c: Vec<String> = self.conds.iter().map(|c| {
            let (a, b) = c.values();
            format!("{},{a},{b}", c.index())
        }).collect();
        let o: Vec<String> = self.ops.iter().map(|o| {
            let (a, b) = o.values();
            format!("{},{a},{b},{}", o.index(), o.factor())
        }).collect();
        format!("{}\t{}\t{}", self.name.replace('\t', " "), c.join(";"), o.join(";"))
    }

    pub fn from_line(line: &str) -> Option<Transform> {
        let mut it = line.split('\t');
        let mut t = Transform::new(it.next()?);
        for (k, c) in it.next()?.split(';').enumerate().take(5) {
            let v: Vec<i64> = c.split(',').filter_map(|x| x.parse().ok()).collect();
            let mut cond = Cond::All.with_index(*v.first()? as usize);
            cond.set_values(*v.get(1)?, *v.get(2)?);
            t.conds[k] = cond;
        }
        for (k, o) in it.next()?.split(';').enumerate().take(5) {
            let v: Vec<&str> = o.split(',').collect();
            let num = |i: usize| v.get(i).and_then(|x| x.parse::<i64>().ok());
            let mut op = Op::Thru.with_index(num(0)? as usize);
            op.set_values(num(1)?, num(2)?, v.get(3)?.parse().ok()?);
            t.ops[k] = op;
        }
        Some(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(pos: u32, len: u32, pitch: u8, velocity: u8) -> Note {
        Note { channel: 0, pitch, velocity, pos, len }
    }

    fn run(name: &str, notes: &mut Vec<Note>, sel: &[usize]) {
        let t = preset(name).unwrap();
        let o = origin_for(&t, notes, sel, 0);
        t.operate(notes, sel, o, &mut Rng(7));
    }

    #[test]
    fn every_preset_but_pitch_bend_exists() {
        for p in PRESETS {
            assert_eq!(preset(p).is_some(), p != "Scale 14bit PitchBd", "{p}");
        }
    }

    #[test]
    fn velocity_presets() {
        let mut v = vec![n(0, 10, 60, 20), n(480, 10, 62, 127), n(960, 10, 64, 60)];
        run("Fixed Velocity", &mut v, &[0, 2]);
        assert_eq!(v.iter().map(|x| x.velocity).collect::<Vec<_>>(), [100, 127, 100]);
        run("Velocity Limiter", &mut v, &[]);
        assert_eq!(v[1].velocity, 110);
        run("Crescendo", &mut v, &[]);
        assert_eq!(v.iter().map(|x| x.velocity).collect::<Vec<_>>(), [40, 80, 120]);
        let mut v = vec![n(0, 10, 60, 64)];
        run("Exponential Velocity", &mut v, &[]);
        assert!(v[0].velocity < 64);
        let mut v = vec![n(0, 10, 60, 64); 50];
        run("Random Velocity", &mut v, &[]);
        assert!(v.iter().all(|x| (40..=127).contains(&x.velocity)));
        assert!(v.iter().any(|x| x.velocity != v[0].velocity));
    }

    #[test]
    fn speed_and_reverse() {
        let mut v = vec![n(960, 480, 60, 80), n(1920, 480, 62, 80)];
        run("Double Speed", &mut v, &[]);
        assert_eq!(v.iter().map(|x| (x.pos, x.len)).collect::<Vec<_>>(), [(960, 240), (1440, 240)]);
        run("Half Speed", &mut v, &[]);
        assert_eq!(v.iter().map(|x| (x.pos, x.len)).collect::<Vec<_>>(), [(960, 480), (1920, 480)]);
        let mut v = vec![n(0, 10, 60, 80), n(480, 10, 62, 80), n(1920, 10, 72, 80)];
        run("Reverse Position", &mut v, &[]);
        assert_eq!(v.iter().map(|x| x.pos).collect::<Vec<_>>(), [1920, 1440, 0]);
        run("Reverse Pitch", &mut v, &[]);
        assert_eq!(v.iter().map(|x| x.pitch).collect::<Vec<_>>(), [72, 70, 60]);
        run("Transposition", &mut v, &[0]);
        assert_eq!(v[0].pitch, 84);
    }

    #[test]
    fn length_presets() {
        let mut v = vec![n(0, 100, 60, 80), n(0, 3000, 61, 80), n(0, 370, 62, 80)];
        run("Maximum Note Length", &mut v, &[]);
        assert_eq!(v[1].len, 960);
        run("Minimum Note Length", &mut v, &[]);
        assert_eq!(v[0].len, 240);
        run("Quantize Note Length", &mut v, &[]);
        assert_eq!(v[2].len, 480);
        run("Fixed Note Length", &mut v, &[]);
        assert!(v.iter().all(|x| x.len == 240));
    }

    #[test]
    fn humanize_stays_close() {
        let mut v: Vec<Note> = (0..40).map(|k| n(1000 + k * 240, 200, 60, 80)).collect();
        let before = v.clone();
        run("Humanize", &mut v, &[]);
        for (a, b) in before.iter().zip(&v) {
            assert!((a.pos as i64 - b.pos as i64).abs() <= 10);
            assert!((a.velocity as i64 - b.velocity as i64).abs() <= 10);
            assert!((a.len as i64 - b.len as i64).abs() <= 10);
        }
        assert_ne!(before, v);
    }

    #[test]
    fn conditions_select_and_operate() {
        let mut v = vec![n(0, 10, 60, 30), n(480, 10, 62, 90), n(960, 10, 64, 120)];
        let mut t = Transform::new("loud ones");
        t.conds[2] = Cond::Greater(80);
        t.ops[2] = Op::Sub(20);
        let sel = t.select_and_operate(&mut v, 0, &mut Rng(1));
        assert_eq!(sel, vec![1, 2]);
        assert_eq!(v.iter().map(|x| x.velocity).collect::<Vec<_>>(), [30, 70, 100]);
        t.conds[1] = Cond::Inside(61, 63);
        assert_eq!(t.select(&v, 0), Vec::<usize>::new(), "62 is now 70: not > 80");
        t.conds[2] = Cond::All;
        assert_eq!(t.select(&v, 0), vec![1]);
        // Operate Only: only the selected that match
        t.operate(&mut v, &[0, 1], 0, &mut Rng(1));
        assert_eq!(v[1].velocity, 50);
        assert_eq!(v[0].velocity, 30);
        assert!(Cond::Outside(10, 20).test(5) && !Cond::Outside(10, 20).test(15));
        assert_eq!(Op::Add(3).with_index(11), Op::Rand(3, 3));
        let mut o = Op::Thru.with_index(16);
        o.set_values(1, 127, 1.0);
        assert_eq!(o, Op::Crescendo(1, 127));
        assert_eq!(Op::QUp(240).with_index(14), Op::QUp(240));
    }

    #[test]
    fn user_preset_lines_round_trip() {
        let mut t = preset("Humanize").unwrap();
        t.name = "My Humanize".into();
        t.conds[1] = Cond::Inside(36, 60);
        t.ops[0] = Op::Scale(1.5, -3);
        assert_eq!(Transform::from_line(&t.to_line()), Some(t));
        assert_eq!(Transform::from_line("junk"), None);
    }
}
