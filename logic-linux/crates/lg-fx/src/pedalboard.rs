//! Pedalboard, as the Effects guide describes it: pedals in slots, left to right; two buses
//! (Bus A, Bus B) with a Splitter (Split: both buses get the signal; Freq: lows to A, highs to B
//! at its Frequency) or a bus split point, and a Mixer (A / Mix / B, the Mix fader, Pan A / B)
//! where they meet; eight Macro controls (A–H), each a target parameter ("Slot n — Pedal —
//! Parameter") and a value.
//!
//! - A pedal moved to Bus B without a Mixer gets one after the last pedal, as Logic adds it.
//!   With no Splitter, the buses split at the Split Point (0 = at the board's input).
//! - Parameter ids: slot k (0..12) at 2000 + 32k: its pedal (a menu: Empty, every pedal,
//!   Splitter, Mixer), its bus, then the pedal's own parameters from +2. Macros at 2500..2515,
//!   the split point at 2520. (Logic's own board layout isn't decoded; a project's pedals are
//!   read with `from_aucu`, which recognises the five pedals the research decoded.)

use crate::pedals::{PEDALS, Pedal};
use crate::stomp;
use crate::util::{Biquad, Coefs};
use lg_dsp::{Effect, Param, Plugin, Transport, Unit};

pub const SLOTS: usize = 12;
pub const BASE: u32 = 2000;
pub const STRIDE: u32 = 32;
pub const MACROS: u32 = 2500;
pub const SPLIT_POINT: u32 = 2520;
const MAX_BLOCK: usize = 1024;


/// Every choice of a slot's pedal menu: Empty, the pedals (by category), Splitter, Mixer.
pub fn pedal_choices() -> &'static [&'static str] {
    static C: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
        let mut v = vec!["Empty"];
        let mut all: Vec<(&str, &str)> = PEDALS.iter().map(|(n, _, c)| (*c, *n)).collect();
        all.extend([("Delay", "Blue Echo"), ("Delay", "Tru-Tape Delay"), ("Modulation", "Robo Flanger"), ("Modulation", "Total Tremolo"), ("Distortion", "Vintage Drive")]);
        all.sort();
        v.extend(all.into_iter().map(|(_, n)| n));
        v.extend(["Splitter", "Mixer"]);
        v
    });
    C.as_slice()
}

/// A pedal's category ("Distortion", "Modulation"…), for the Pedal Browser's View menu.
pub fn category(name: &str) -> &'static str {
    if let Some((_, _, c)) = PEDALS.iter().find(|(n, ..)| *n == name) {
        return c;
    }
    match name {
        "Blue Echo" | "Tru-Tape Delay" => "Delay",
        "Robo Flanger" | "Total Tremolo" => "Modulation",
        "Vintage Drive" => "Distortion",
        "Splitter" | "Mixer" => "Utility",
        _ => "",
    }
}

/// A stand-alone pedal by name (the stomp.rs ones and pedals.rs ones).
pub fn make_pedal(name: &str, rate: f32) -> Option<Box<dyn Effect>> {
    Some(match name {
        "Blue Echo" => Box::new(stomp::BlueEcho::new(rate)),
        "Tru-Tape Delay" => Box::new(stomp::TruTapeDelay::new(rate)),
        "Robo Flanger" => Box::new(stomp::RoboFlanger::new(rate)),
        "Total Tremolo" => Box::new(stomp::TotalTremolo::new(rate)),
        "Vintage Drive" => Box::new(stomp::VintageDrive::new(rate)),
        _ => Box::new(Pedal::by_name(name, rate)?),
    })
}

static SPLITTER: [Param; 2] = [Param::menu(0, "Split/Freq", &["Split", "Freq"], 0.0), Param::new(1, "Frequency", 50.0, 10000.0, 800.0, Unit::Hz)];
static MIXER: [Param; 4] = [Param::new(0, "Mix", 0.0, 100.0, 50.0, Unit::Percent), Param::menu(1, "A/Mix/B", &["A", "Mix", "B"], 1.0), Param::new(2, "Pan A", -1.0, 1.0, 0.0, Unit::Pan), Param::new(3, "Pan B", -1.0, 1.0, 0.0, Unit::Pan)];

use crate::kit::leak;

enum Unit_ {
    Empty,
    Fx(Box<dyn Effect>),
    Splitter([f32; 2], [[Biquad; 4]; 2]),
    Mixer([f32; 4]),
}

struct Slot {
    kind: usize,
    bus_b: bool,
    unit: Unit_,
}

pub struct Board {
    rate: f32,
    bpm: f64,
    slots: Vec<Slot>,
    macros: [f32; 16],
    split_point: f32,
    defs: Vec<Param>,
    a: [Vec<f32>; 2],
    b: [Vec<f32>; 2],
}

impl Board {
    pub fn new(rate: f32) -> Board {
        let mut b = Board { rate, bpm: 120.0, slots: (0..SLOTS).map(|_| Slot { kind: 0, bus_b: false, unit: Unit_::Empty }).collect(), macros: [0.0; 16], split_point: 0.0, defs: Vec::new(), a: [vec![0.0; MAX_BLOCK], vec![0.0; MAX_BLOCK]], b: [vec![0.0; MAX_BLOCK], vec![0.0; MAX_BLOCK]] };
        b.rebuild();
        b
    }

    /// Builds the board from a project's Pedalboard (the pedals the research decoded, in series).
    pub fn from_aucu(payload: &[u8], rate: f32) -> Board {
        let mut b = Board::new(rate);
        for (k, floats) in stomp::Pedalboard::records(payload).into_iter().enumerate().take(SLOTS) {
            b.set_kind(k, pedal_choices().iter().position(|n| *n == stomp::Pedalboard::pedal_name_for(&floats)).unwrap_or(0));
            if let Unit_::Fx(fx) = &mut b.slots[k].unit {
                fx.load_floats(&floats);
            }
        }
        b.rebuild();
        b
    }

    /// The pedal names in the slots, left to right ("" for an empty slot).
    pub fn pedals(&self) -> Vec<&'static str> {
        self.slots.iter().map(|s| if s.kind == 0 { "" } else { pedal_choices()[s.kind] }).collect()
    }

    fn sub_params(&self, k: usize) -> &[Param] {
        match &self.slots[k].unit {
            Unit_::Empty => &[],
            Unit_::Fx(f) => f.params(),
            Unit_::Splitter(..) => &SPLITTER,
            Unit_::Mixer(_) => &MIXER,
        }
    }

    fn rebuild(&mut self) {
        let mut d = Vec::new();
        for k in 0..SLOTS {
            let base = BASE + STRIDE * k as u32;
            let kind = self.slots[k].kind;
            d.push(Param::menu(base, leak(format!("Slot {} Pedal", k + 1)), pedal_choices(), 0.0));
            if kind == 0 {
                continue;
            }
            let name = pedal_choices()[kind];
            d.push(Param::menu(base + 1, leak(format!("Slot {} — {name} — Bus", k + 1)), &["Bus A", "Bus B"], 0.0));
            for p in self.sub_params(k) {
                let mut q = p.clone();
                q.id = base + 2 + p.id;
                q.name = leak(format!("Slot {} — {name} — {}", k + 1, p.name));
                d.push(q);
            }
        }
        for m in 0..8 {
            let l = (b'A' + m as u8) as char;
            d.push(Param::new(MACROS + 2 * m, leak(format!("Macro {l} Target")), 0.0, (SLOTS as u32 * STRIDE) as f32, 0.0, Unit::None));
            d.push(Param::new(MACROS + 2 * m + 1, leak(format!("Macro {l} Value")), 0.0, 1.0, 0.0, Unit::None));
        }
        d.push(Param::new(SPLIT_POINT, "Split Point", 0.0, (SLOTS - 1) as f32, 0.0, Unit::None));
        self.defs = d;
    }

    fn set_kind(&mut self, k: usize, kind: usize) {
        let kind = kind.min(pedal_choices().len() - 1);
        if self.slots[k].kind == kind {
            return;
        }
        let name = pedal_choices()[kind];
        self.slots[k].kind = kind;
        self.slots[k].unit = match name {
            "Empty" => Unit_::Empty,
            "Splitter" => Unit_::Splitter([0.0, 800.0], [[Biquad::default(); 4]; 2]),
            "Mixer" => Unit_::Mixer([50.0, 1.0, 0.0, 0.0]),
            n => match make_pedal(n, self.rate) {
                Some(mut f) => {
                    f.set_tempo(self.bpm);
                    Unit_::Fx(f)
                }
                None => Unit_::Empty,
            },
        };
        if let Unit_::Splitter(v, bq) = &mut self.slots[k].unit {
            Board::split_coefs(self.rate, v[1], bq);
        }
        if self.slots.iter().all(|s| matches!(s.unit, Unit_::Empty | Unit_::Mixer(_) | Unit_::Splitter(..))) {
            // no pedals left: the Mixer and the second bus go
            for s in self.slots.iter_mut() {
                s.kind = 0;
                s.unit = Unit_::Empty;
                s.bus_b = false;
            }
        }
    }

    fn split_coefs(rate: f32, hz: f32, bq: &mut [[Biquad; 4]; 2]) {
        // Linkwitz-Riley 4th order: two Butterworth stages each way
        for s in 0..2 {
            for k in 0..2 {
                bq[s][k].c = Coefs::lowpass(rate, hz, std::f32::consts::FRAC_1_SQRT_2);
                bq[s][2 + k].c = Coefs::highpass(rate, hz, std::f32::consts::FRAC_1_SQRT_2);
            }
        }
    }

    /// Adds a Mixer after the last pedal when a pedal is on Bus B (or a Splitter is in) and there's
    /// none.
    fn ensure_mixer(&mut self) {
        let mixer = pedal_choices().iter().position(|n| *n == "Mixer").unwrap_or(0);
        let split = self.slots.iter().any(|s| s.bus_b || matches!(s.unit, Unit_::Splitter(..)));
        if !split || self.slots.iter().any(|s| s.kind == mixer) {
            return;
        }
        let last = self.slots.iter().rposition(|s| s.kind != 0).unwrap_or(0);
        if let Some(free) = (last + 1..SLOTS).find(|k| self.slots[*k].kind == 0) {
            self.set_kind(free, mixer);
        }
    }

    fn process_chunk(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        let rate = self.rate;
        let has_splitter = self.slots.iter().any(|s| matches!(s.unit, Unit_::Splitter(..)));
        let any_b = self.slots.iter().any(|s| s.bus_b);
        let split_at = if has_splitter { usize::MAX } else if any_b { self.split_point.round() as usize } else { usize::MAX };
        let mut split = false;
        let [al, ar] = &mut self.a;
        let [bl, br] = &mut self.b;
        al[..n].copy_from_slice(l);
        ar[..n].copy_from_slice(r);
        for k in 0..SLOTS {
            if !split && k == split_at {
                bl[..n].copy_from_slice(&al[..n]);
                br[..n].copy_from_slice(&ar[..n]);
                split = true;
            }
            let bus_b = self.slots[k].bus_b;
            match &mut self.slots[k].unit {
                Unit_::Empty => {}
                Unit_::Fx(fx) => {
                    if split && bus_b {
                        fx.process(&mut bl[..n], &mut br[..n]);
                    } else {
                        fx.process(&mut al[..n], &mut ar[..n]);
                    }
                }
                Unit_::Splitter(v, bq) => {
                    if split {
                        continue;
                    }
                    split = true;
                    if v[0] < 0.5 {
                        bl[..n].copy_from_slice(&al[..n]);
                        br[..n].copy_from_slice(&ar[..n]);
                    } else {
                        for i in 0..n {
                            for (s, buf) in [(0usize, &mut *al), (1, &mut *ar)] {
                                let x = buf[i];
                                let lo = bq[s][0].run(x);
                                let lo = bq[s][1].run(lo);
                                let hi = bq[s][2].run(x);
                                let hi = bq[s][3].run(hi);
                                buf[i] = lo;
                                if s == 0 { bl[i] = hi } else { br[i] = hi }
                            }
                        }
                    }
                }
                Unit_::Mixer(v) => {
                    if !split {
                        continue;
                    }
                    let mix = v[0] / 100.0;
                    let (ga, gb) = match v[1].round() as i32 {
                        0 => (mix * 2.0, 0.0),
                        2 => (0.0, mix * 2.0),
                        _ => ((1.0 - mix) * 2.0, mix * 2.0),
                    };
                    let pan = |p: f32| ((1.0 - p).min(1.0), (1.0 + p).min(1.0));
                    let (pal, par) = pan(v[2]);
                    let (pbl, pbr) = pan(v[3]);
                    for i in 0..n {
                        al[i] = 0.5 * (al[i] * ga * pal + bl[i] * gb * pbl);
                        ar[i] = 0.5 * (ar[i] * ga * par + br[i] * gb * pbr);
                    }
                    split = false;
                }
            }
        }
        if split {
            for i in 0..n {
                al[i] = 0.5 * (al[i] + bl[i]);
                ar[i] = 0.5 * (ar[i] + br[i]);
            }
        }
        l.copy_from_slice(&al[..n]);
        r.copy_from_slice(&ar[..n]);
        let _ = rate;
    }
}

impl Plugin for Board {
    fn params(&self) -> &[Param] {
        &self.defs
    }
    fn get(&self, id: u32) -> f32 {
        if id >= SPLIT_POINT {
            return self.split_point;
        }
        if id >= MACROS {
            return self.macros.get((id - MACROS) as usize).copied().unwrap_or(0.0);
        }
        let Some(rel) = id.checked_sub(BASE) else { return 0.0 };
        let (k, o) = ((rel / STRIDE) as usize, rel % STRIDE);
        let Some(s) = self.slots.get(k) else { return 0.0 };
        match o {
            0 => s.kind as f32,
            1 => s.bus_b as u8 as f32,
            _ => match &s.unit {
                Unit_::Fx(f) => f.get(o - 2),
                Unit_::Splitter(v, _) => v.get((o - 2) as usize).copied().unwrap_or(0.0),
                Unit_::Mixer(v) => v.get((o - 2) as usize).copied().unwrap_or(0.0),
                Unit_::Empty => 0.0,
            },
        }
    }
    fn set(&mut self, id: u32, value: f32) {
        if !value.is_finite() {
            return;
        }
        if id == SPLIT_POINT {
            self.split_point = value.clamp(0.0, (SLOTS - 1) as f32).round();
            return;
        }
        if (MACROS..MACROS + 16).contains(&id) {
            let m = (id - MACROS) as usize;
            self.macros[m] = value;
            // a Value drives its Target (encoded slot * 32 + offset + 1)
            let (t, v) = if m % 2 == 1 { (self.macros[m - 1], value) } else { (value, self.macros[m + 1]) };
            if m % 2 == 1 && t >= 1.0 {
                let target = BASE + t.round() as u32 - 1;
                if let Some(p) = self.defs.iter().find(|p| p.id == target).cloned() {
                    self.set(target, p.min + v.clamp(0.0, 1.0) * (p.max - p.min));
                }
            }
            return;
        }
        let Some(rel) = id.checked_sub(BASE) else { return };
        let (k, o) = ((rel / STRIDE) as usize, rel % STRIDE);
        if k >= SLOTS {
            return;
        }
        match o {
            0 => {
                self.set_kind(k, value.round().max(0.0) as usize);
                self.ensure_mixer();
                self.rebuild();
            }
            1 => {
                self.slots[k].bus_b = value >= 0.5 && !matches!(self.slots[k].unit, Unit_::Mixer(_) | Unit_::Splitter(..));
                self.ensure_mixer();
                self.rebuild();
            }
            _ => {
                let rate = self.rate;
                match &mut self.slots[k].unit {
                    Unit_::Fx(f) => f.set(o - 2, value),
                    Unit_::Splitter(v, bq) => {
                        let i = (o - 2) as usize;
                        if let Some(p) = SPLITTER.get(i) {
                            v[i] = value.clamp(p.min, p.max);
                            Board::split_coefs(rate, v[1], bq);
                        }
                    }
                    Unit_::Mixer(v) => {
                        let i = (o - 2) as usize;
                        if let Some(p) = MIXER.get(i) {
                            v[i] = value.clamp(p.min, p.max);
                        }
                    }
                    Unit_::Empty => {}
                }
            }
        }
    }
    fn reset(&mut self) {
        for s in self.slots.iter_mut() {
            if let Unit_::Fx(f) = &mut s.unit {
                f.reset();
            }
        }
    }
    fn tail(&self) -> f32 {
        self.slots.iter().map(|s| if let Unit_::Fx(f) = &s.unit { f.tail() } else { 0.0 }).sum()
    }
    fn load_floats(&mut self, floats: &[f32]) {
        // the pedals first (they decide the rest of the list), then everything else
        for k in 0..SLOTS {
            if let Some(v) = floats.get((BASE + STRIDE * k as u32) as usize) {
                self.set_kind(k, v.round().max(0.0) as usize);
            }
        }
        self.rebuild();
        let ids: Vec<u32> = self.defs.iter().map(|p| p.id).filter(|id| (id - BASE) % STRIDE != 0 || *id >= MACROS).collect();
        for id in ids {
            if let Some(v) = floats.get(id as usize) {
                self.set(id, *v);
            }
        }
    }
}

impl Effect for Board {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut at = 0;
        while at < n {
            let e = (at + MAX_BLOCK).min(n);
            let (l, r) = (&mut left[at..e], &mut right[at..e]);
            self.process_chunk(l, r);
            at = e;
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
        for s in self.slots.iter_mut() {
            if let Unit_::Fx(f) = &mut s.unit {
                f.set_tempo(bpm);
            }
        }
    }
    fn set_transport(&mut self, t: Transport) {
        for s in self.slots.iter_mut() {
            if let Unit_::Fx(f) = &mut s.unit {
                f.set_transport(t);
            }
        }
    }
    fn latency(&self) -> usize {
        self.slots.iter().map(|s| if let Unit_::Fx(f) = &s.unit { f.latency() } else { 0 }).sum()
    }
}


// ---- the Router's rules (the guide's): where a Splitter, a Mixer or a split point may go ----

/// The chain as the Router sees it: the used slots' names in order ("" never appears).
pub fn chain(names: &[&str]) -> Vec<String> {
    names.iter().filter(|n| !n.is_empty() && **n != "Empty").map(|n| n.to_string()).collect()
}

/// A Splitter may go before position `at` (0 = first) of the chain: not at the far right (no
/// pedal after it), not right after a Splitter, not right before a Mixer, not in empty space.
pub fn splitter_ok(chain: &[String], at: usize) -> bool {
    if at >= chain.len() {
        return false;
    }
    if at > 0 && chain[at - 1] == "Splitter" {
        return false;
    }
    chain[at] != "Mixer" && chain[at] != "Splitter"
}

/// A Mixer may go before position `at`: not in the first slot, not right after a Mixer, not
/// right after a Splitter (next to its split), not between a Splitter and its Mixer, not next
/// to the split point `split` (when there is no Splitter).
pub fn mixer_ok(chain: &[String], at: usize, split: Option<usize>) -> bool {
    if at == 0 || at > chain.len() {
        return false;
    }
    if chain[at - 1] == "Mixer" || chain[at - 1] == "Splitter" {
        return false;
    }
    if let Some(sp) = chain.iter().position(|n| n == "Splitter")
        && sp < at
        && chain[sp..].iter().any(|n| n == "Mixer")
        && chain[sp..at].iter().all(|n| n != "Mixer")
    {
        return false;
    }
    if let Some(s) = split
        && !chain.iter().any(|n| n == "Splitter")
        && (s == at || s + 1 == at)
    {
        return false;
    }
    true
}

/// A split point (before position `at`) may not sit directly before or after the Mixer.
pub fn split_point_ok(chain: &[String], at: usize) -> bool {
    let m = chain.iter().position(|n| n == "Mixer");
    match m {
        Some(m) => at < m && at + 1 != m && at != m + 1,
        None => at < chain.len(),
    }
}

/// Whether a second bus exists (a Splitter, a Mixer, or a pedal on Bus B).
pub fn two_buses(chain: &[String], any_b: bool) -> bool {
    any_b || chain.iter().any(|n| n == "Splitter" || n == "Mixer")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(n: &str) -> f32 {
        pedal_choices().iter().position(|c| *c == n).unwrap() as f32
    }

    #[test]
    fn slots_build_their_parameters() {
        let mut b = Board::new(48000.0);
        b.set(BASE, kind("Blue Echo"));
        assert!(b.params().iter().any(|p| p.name == "Slot 1 — Blue Echo — Repeats"));
        b.set(BASE, 0.0);
        assert!(!b.params().iter().any(|p| p.name.contains("Blue Echo")));
    }

    #[test]
    fn bus_b_adds_a_mixer_and_splitter_freq_splits() {
        let mut b = Board::new(48000.0);
        b.set(BASE + STRIDE, kind("Graphic EQ"));
        b.set(BASE, kind("Splitter"));
        b.set(BASE + STRIDE + 1, 1.0); // to Bus B
        assert_eq!(b.pedals()[2], "Mixer");
        // Freq split at 800 Hz, Bus B muted by the Mixer set to A: highs vanish
        b.set(BASE + 2, 1.0);
        b.set(BASE + 2 * STRIDE + 3, 0.0); // A only
        let mut l: Vec<f32> = (0..48000).map(|i| (std::f32::consts::TAU * 6000.0 * i as f32 / 48000.0).sin()).collect();
        let mut r = l.clone();
        b.process(&mut l, &mut r);
        let peak = l[24000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.05, "{peak}");
    }

    #[test]
    fn macros_drive_their_target() {
        let mut b = Board::new(48000.0);
        b.set(BASE, kind("Candy Fuzz"));
        // Slot 1 — Candy Fuzz — Drive = offset 2 + 1 → target 3 + 1
        b.set(MACROS, 4.0);
        b.set(MACROS + 1, 0.25);
        assert!((b.get(BASE + 3) - 25.0).abs() < 1e-3);
    }

    #[test]
    fn settings_round_trip() {
        let mut b = Board::new(48000.0);
        b.set(BASE + STRIDE, kind("Phaze 2"));
        b.set(BASE + STRIDE + 4, 1.3);
        let mut floats = vec![0.0; 2600];
        for p in b.params() {
            floats[p.id as usize] = b.get(p.id);
        }
        let mut c = Board::new(48000.0);
        c.load_floats(&floats);
        assert_eq!(c.pedals()[1], "Phaze 2");
        assert!((c.get(BASE + STRIDE + 4) - 1.3).abs() < 1e-4);
    }

    fn ch(v: &[&str]) -> Vec<String> {
        chain(v)
    }

    #[test]
    fn router_rules() {
        let c = ch(&["Rawk! Distortion", "", "Heavenly Chorus", "Blue Echo"]);
        assert_eq!(c.len(), 3);
        assert!(splitter_ok(&c, 0));
        assert!(!splitter_ok(&c, 3)); // far right / empty space
        let c2 = ch(&["Splitter", "Heavenly Chorus", "Mixer"]);
        assert!(!splitter_ok(&c2, 1)); // right after a Splitter
        assert!(!splitter_ok(&c2, 2)); // right before a Mixer
        assert!(!mixer_ok(&c, 0, None)); // first slot
        assert!(!mixer_ok(&c2, 3, None)); // right after a Mixer
        assert!(!mixer_ok(&c2, 1, None)); // next to its Splitter
        assert!(!mixer_ok(&c2, 2, None)); // between the Splitter and its Mixer
        assert!(mixer_ok(&c, 2, Some(0)));
        assert!(!mixer_ok(&c, 1, Some(0))); // next to the split point
        let c3 = ch(&["Rawk! Distortion", "Heavenly Chorus", "Blue Echo", "Mixer"]);
        assert!(split_point_ok(&c3, 0) && split_point_ok(&c3, 1));
        assert!(!split_point_ok(&c3, 2) && !split_point_ok(&c3, 4));
        assert!(two_buses(&c3, false) && !two_buses(&c, false));
    }
}
