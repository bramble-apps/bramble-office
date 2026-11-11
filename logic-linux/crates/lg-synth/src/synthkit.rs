//! Pieces the synthesizers share: an LFO with the classic shapes, the tempo-synced lengths, a
//! note stack for monophonic playing, a ladder low-pass with 12 / 18 / 24 dB slopes, a
//! side chain buffer, and the parameter-table helper for MIDI remote control.

use crate::dsp::{fast_tanh, flush, Rng};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Triangle,
    SawDown,
    SawUp,
    Square,
    SampleHold,
    Lag,
    Sine,
    /// Square that only goes up from zero (ES2's unipolar rectangle).
    SquareUni,
}

/// A low-frequency oscillator, stepped once per control block. Output −1…1.
#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    pub phase: f32,
    hold: f32,
    next: f32,
    lag: f32,
}

impl Default for Lfo {
    fn default() -> Lfo {
        Lfo { phase: 0.0, hold: 0.0, next: 0.0, lag: 0.0 }
    }
}

impl Lfo {
    pub fn reset(&mut self, phase: f32) {
        self.phase = phase.rem_euclid(1.0);
    }
    /// Advances by `dphase` cycles and returns the value.
    pub fn step(&mut self, shape: Shape, dphase: f32, rng: &mut Rng) -> f32 {
        self.phase += dphase.max(0.0);
        if self.phase >= 1.0 {
            self.phase = self.phase.fract();
            self.hold = self.next;
            self.next = rng.bipolar();
        }
        let t = self.phase;
        match shape {
            Shape::Triangle => {
                if t < 0.25 {
                    4.0 * t
                } else if t < 0.75 {
                    2.0 - 4.0 * t
                } else {
                    4.0 * t - 4.0
                }
            }
            Shape::SawDown => 1.0 - 2.0 * t,
            Shape::SawUp => 2.0 * t - 1.0,
            Shape::Square => {
                if t < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Shape::SquareUni => {
                if t < 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Shape::SampleHold => self.next,
            Shape::Lag => {
                // smooth walk from the last random value to the next
                let s = t * t * (3.0 - 2.0 * t);
                self.lag = self.hold + (self.next - self.hold) * s;
                self.lag
            }
            Shape::Sine => crate::dsp::sin_cycles(t),
        }
    }
}

/// Tempo-synced lengths in bars (4/4), longest first: name and bars.
pub const SYNC: &[(&str, f32)] = &[
    ("32 bars", 32.0),
    ("16 bars", 16.0),
    ("8 bars", 8.0),
    ("4 bars", 4.0),
    ("2 bars", 2.0),
    ("1 bar", 1.0),
    ("1/2", 0.5),
    ("1/2 t", 1.0 / 3.0),
    ("1/4", 0.25),
    ("1/4 t", 1.0 / 6.0),
    ("1/8", 0.125),
    ("1/8 t", 1.0 / 12.0),
    ("1/16", 0.0625),
    ("1/16 t", 1.0 / 24.0),
    ("1/32", 1.0 / 32.0),
    ("1/32 t", 1.0 / 48.0),
    ("1/64", 1.0 / 64.0),
    ("1/96", 1.0 / 96.0),
];

/// Cycles per second of a length in bars at a tempo.
pub fn bars_hz(bars: f32, bpm: f32) -> f32 {
    bpm.max(1.0) / 240.0 / bars.max(1e-4)
}

/// A rate knob with tempo-synced lengths left of zero and free rates right of it
/// (`v` −1…1; `max_hz` at +1). Returns (Hz, name of the synced length).
pub fn bipolar_rate(v: f32, max_hz: f32, bpm: f32) -> (f32, Option<&'static str>) {
    if v < -0.001 {
        // −1 = slowest (32 bars) … just under 0 = fastest (1/96)
        let k = (((1.0 + v) * SYNC.len() as f32) as usize).min(SYNC.len() - 1);
        (bars_hz(SYNC[k].1, bpm), Some(SYNC[k].0))
    } else if v > 0.001 {
        (0.02 * (max_hz / 0.02).powf(v), None)
    } else {
        (0.0, None)
    }
}

/// Held notes of a monophonic synth, newest last.
#[derive(Clone, Copy, Debug)]
pub struct Stack {
    notes: [(u8, u8); 24],
    n: usize,
}

impl Default for Stack {
    fn default() -> Stack {
        Stack { notes: [(0, 0); 24], n: 0 }
    }
}

impl Stack {
    pub fn push(&mut self, note: u8, vel: u8) {
        self.remove(note);
        if self.n == self.notes.len() {
            self.notes.copy_within(1.., 0);
            self.n -= 1;
        }
        self.notes[self.n] = (note, vel);
        self.n += 1;
    }
    /// Removes the note; true when it was there.
    pub fn remove(&mut self, note: u8) -> bool {
        if let Some(i) = self.notes[..self.n].iter().position(|x| x.0 == note) {
            self.notes.copy_within(i + 1..self.n, i);
            self.n -= 1;
            true
        } else {
            false
        }
    }
    pub fn len(&self) -> usize {
        self.n
    }
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
    pub fn clear(&mut self) {
        self.n = 0;
    }
    /// The newest held note.
    pub fn last(&self) -> Option<(u8, u8)> {
        self.notes[..self.n].last().copied()
    }
    pub fn lowest(&self) -> Option<(u8, u8)> {
        self.notes[..self.n].iter().copied().min_by_key(|x| x.0)
    }
    pub fn highest(&self) -> Option<(u8, u8)> {
        self.notes[..self.n].iter().copied().max_by_key(|x| x.0)
    }
    pub fn notes(&self) -> &[(u8, u8)] {
        &self.notes[..self.n]
    }
}

/// Ladder low-pass with one to four poles in the loop (6 / 12 / 18 / 24 dB per octave),
/// resonance up to self-oscillation (three and four poles) and a saturating input.
#[derive(Clone, Copy, Debug)]
pub struct Lp {
    s: [f32; 4],
    g: f32,
    k: f32,
    pub poles: u8,
    /// Makes up for the low end the resonance takes away ("fat").
    pub fat: bool,
}

impl Default for Lp {
    fn default() -> Lp {
        Lp { s: [0.0; 4], g: 0.1, k: 0.0, poles: 4, fat: false }
    }
}

impl Lp {
    pub fn reset(&mut self) {
        self.s = [0.0; 4];
    }
    /// `res` 0…1 (1 = self-oscillation with three or four poles).
    pub fn set(&mut self, hz: f32, res: f32, rate: f32) {
        let wc = std::f32::consts::PI * (hz / rate).clamp(1e-5, 0.45);
        self.set_wc(wc, res);
    }
    /// The same from the pre-warped angle (π·f / rate): cheap enough for audio-rate cutoff
    /// modulation.
    #[inline]
    pub fn set_wc(&mut self, wc: f32, res: f32) {
        let wc = wc.clamp(1e-5, 1.4);
        // tan by its series near zero, exact enough below 0.45·rate
        let w2 = wc * wc;
        let g = wc * (1.0 + w2 * (1.0 / 3.0 + w2 * (2.0 / 15.0 + w2 * 17.0 / 315.0)));
        self.g = g / (1.0 + g);
        let r = res.clamp(0.0, 1.0);
        self.k = match self.poles {
            4 => r * 4.6,
            3 => r * 9.4,
            2 => r * r * 60.0,
            _ => r * 2.0,
        };
    }
    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        let n = self.poles.clamp(1, 4) as usize;
        let g = self.g;
        let mut big_g = 1.0;
        let mut sigma = 0.0;
        // Σ s[i]·(1−g)·g^(n−1−i), from the last stage back
        for i in (0..n).rev() {
            sigma += self.s[i] * (1.0 - g) * big_g;
            big_g *= g;
        }
        let comp = if self.fat { 1.0 + self.k * 0.5 } else { 1.0 };
        let u = (x * comp - self.k * sigma) / (1.0 + self.k * big_g);
        let mut v = fast_tanh(u);
        for i in 0..n {
            let y = g * (v - self.s[i]) + self.s[i];
            self.s[i] = flush(2.0 * y - self.s[i]);
            v = y;
        }
        v
    }
}

/// The side chain input of the block about to be rendered (fixed size: no allocation while
/// playing).
pub struct SideBuf {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
    pub n: usize,
    /// Rectified, smoothed level of the block (a modulation source).
    pub level: f32,
}

impl SideBuf {
    pub fn new() -> SideBuf {
        SideBuf { l: vec![0.0; 8192], r: vec![0.0; 8192], n: 0, level: 0.0 }
    }
    pub fn fill(&mut self, l: &[f32], r: &[f32]) {
        let n = l.len().min(r.len()).min(self.l.len());
        self.l[..n].copy_from_slice(&l[..n]);
        self.r[..n].copy_from_slice(&r[..n]);
        self.n = n;
        let mut pk = 0.0f32;
        for i in 0..n {
            pk = pk.max((l[i] + r[i]).abs() * 0.5);
        }
        self.level += (pk - self.level) * if pk > self.level { 0.6 } else { 0.1 };
    }
    /// The mono sample `i` of the block (0 past its end or when nothing was handed over).
    #[inline]
    pub fn at(&self, i: usize) -> f32 {
        if i < self.n {
            (self.l[i] + self.r[i]) * 0.5
        } else {
            0.0
        }
    }
    /// Called at the end of a render: the block is used up.
    pub fn done(&mut self) {
        self.n = 0;
    }
}

impl Default for SideBuf {
    fn default() -> SideBuf {
        SideBuf::new()
    }
}

/// A MIDI controller value (0…127) as the parameter's value across its range (menus and
/// switches land on whole steps).
pub fn cc_value(p: &lg_dsp::Param, v: u8) -> f32 {
    let x = p.min + (p.max - p.min) * v as f32 / 127.0;
    match p.unit {
        lg_dsp::Unit::Menu | lg_dsp::Unit::OnOff => x.round(),
        _ => x,
    }
}

/// Linear position between two values.
#[inline]
pub fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Normalized cutoff 0…1 as Hz (20 Hz … 20 kHz, ten octaves).
#[inline]
pub fn cutoff_hz(c: f32) -> f32 {
    20.0 * 1000f32.powf(c.clamp(0.0, 1.0))
}

/// The usual log time law of the synths' envelope sliders: 0…1 → 1 ms … `max` seconds.
#[inline]
pub fn env_time(v: f32, max: f32) -> f32 {
    0.001 * (max / 0.001).powf(v.clamp(0.0, 1.0))
}

/// A controller assignment menu: Off, MIDI controllers 1…119 (the index is the number), then
/// channel aftertouch.
pub const CTRLS: &[&str] = &[
    "Off", "1 Mod Wheel", "2 Breath", "3", "4 Foot", "5", "6", "7 Volume", "8", "9", "10 Pan", "11 Expression", "12", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22", "23", "24", "25", "26", "27", "28", "29", "30", "31", "32", "33", "34", "35", "36", "37", "38", "39", "40", "41", "42", "43", "44", "45", "46", "47", "48", "49", "50", "51", "52", "53", "54", "55", "56", "57", "58", "59", "60", "61", "62", "63", "64 Sustain", "65", "66", "67", "68", "69", "70", "71", "72", "73", "74", "75", "76", "77", "78", "79", "80", "81", "82", "83", "84", "85", "86", "87", "88", "89", "90", "91", "92", "93", "94", "95", "96", "97", "98", "99", "100", "101", "102", "103", "104", "105", "106", "107", "108", "109", "110", "111", "112", "113", "114", "115", "116", "117", "118", "119", "Aftertouch",
];
/// Index of Aftertouch in `CTRLS`.
pub const CTRL_AT: usize = 120;

/// A triangle wave from an oscillator's phase (starting at a rising zero crossing); no
/// integrator, so no offset to settle at a note's start.
#[inline]
pub fn tri(osc: &mut crate::dsp::Osc, inc: f32) -> f32 {
    let t = osc.phase;
    osc.advance(inc.clamp(1e-7, 0.5));
    if t < 0.25 {
        4.0 * t
    } else if t < 0.75 {
        2.0 - 4.0 * t
    } else {
        4.0 * t - 4.0
    }
}
