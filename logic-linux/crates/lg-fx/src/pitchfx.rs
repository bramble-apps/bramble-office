//! The pitch effects: Pitch Shifter, Pitch Correction and Vocal Transformer, with the guide's
//! controls.
//!
//! The shifting is pitch-synchronous where it matters: a detector (YIN) follows the input's
//! period and the grains are sized to it, which keeps a voice's formants in place (like PSOLA).
//! Vocal Transformer moves the formants separately: the whole signal is shifted by the formant
//! amount (a long-grain shifter moves the spectral envelope with it) and the pitch is then put
//! where Pitch says with the period-synchronous shifter.
//!
//! Ids are our own from 1000 (the blocks aren't decoded).

use crate::kit::{Shifter, yin};
use crate::util::{Smooth, Values, plugin_impl};
use lg_dsp::{Effect, Param, Unit};

const B: u32 = 1000;

/// Follows the input's pitch on a sliding window, a few times per second.
struct Tracker {
    buf: Vec<f32>,
    at: usize,
    count: usize,
    hop: usize,
    pub hz: Option<f32>,
    lo: f32,
    hi: f32,
}

impl Tracker {
    fn new(rate: f32) -> Tracker {
        Tracker { buf: vec![0.0; 1536], at: 0, count: 0, hop: (rate / 40.0) as usize, hz: None, lo: 70.0, hi: 1000.0 }
    }
    #[inline]
    fn push(&mut self, x: f32, rate: f32) {
        self.buf[self.at] = x;
        self.at = (self.at + 1) % self.buf.len();
        self.count += 1;
        if self.count >= self.hop {
            self.count = 0;
            let n = self.buf.len();
            let w: Vec<f32> = (0..n).map(|i| self.buf[(self.at + i) % n]).collect();
            self.hz = yin(&w, rate, self.lo, self.hi);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Pitch Shifter

static PS_P: [Param; 8] = [
    Param::new(B, "Semi Tones", -24.0, 24.0, 0.0, Unit::Semitones),
    Param::new(B + 1, "Cents", -100.0, 100.0, 0.0, Unit::Cents),
    Param::new(B + 2, "Mix", 0.0, 100.0, 100.0, Unit::Percent),
    // Drums / Speech / Vocals: fixed grain sizes for groove, balance or intonation; Manual:
    // Grain Size, Delay and Crossfade; Pitch Tracking: grains follow the detected pitch
    Param::menu(B + 3, "Timing", &["Drums", "Speech", "Vocals", "Manual", "Pitch Tracking"], 4.0),
    Param::new(B + 4, "Grain Size", 5.0, 100.0, 30.0, Unit::Ms),
    Param::new(B + 5, "Delay", 0.0, 100.0, 0.0, Unit::Ms),
    Param::new(B + 6, "Crossfade", 0.0, 100.0, 50.0, Unit::Percent),
    Param::menu(B + 7, "Stereo Link", &["Normal", "Inv"], 0.0),
];

pub struct PitchShifter {
    p: Values,
    rate: f32,
    sh: [Shifter; 2],
    tr: Tracker,
    pre: [crate::util::Delay; 2],
}

impl PitchShifter {
    pub fn new(rate: f32) -> PitchShifter {
        PitchShifter { p: Values::new(&PS_P), rate, sh: [Shifter::new(rate, 100.0), Shifter::new(rate, 100.0)], tr: Tracker::new(rate), pre: [crate::util::Delay::new((0.11 * rate) as usize), crate::util::Delay::new((0.11 * rate) as usize)] }
    }
    fn update(&mut self) {}
    fn clear(&mut self) {
        self.sh[0].clear();
        self.sh[1].clear();
    }
    fn tail_secs(&self) -> f32 {
        0.15
    }
}

impl Effect for PitchShifter {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let st = self.p.get(B) + self.p.get(B + 1) / 100.0;
        let ratio = 2f32.powf(st / 12.0);
        let mix = self.p.get(B + 2) / 100.0;
        let timing = self.p.idx(B + 3);
        let manual = timing == 3;
        let inv = self.p.idx(B + 7) == 1;
        // Delay and Crossfade only work in Manual
        let delay = if manual { self.p.get(B + 5) * 0.001 * self.rate } else { 0.0 };
        let fade = if manual { self.p.get(B + 6) / 100.0 } else { 1.0 };
        for s in 0..2 {
            self.sh[s].set_fade(fade);
        }
        for i in 0..left.len().min(right.len()) {
            self.tr.push(0.5 * (left[i] + right[i]), self.rate);
            let ms = match timing {
                // short grains keep the groove, long ones the intonation
                0 => 12.0,
                1 => 30.0,
                2 => 60.0,
                3 => self.p.get(B + 4),
                // Pitch Tracking: grains of two periods of the detected pitch
                _ => self.tr.hz.map_or(30.0, |f| (2000.0 / f).clamp(5.0, 60.0)),
            };
            for s in 0..2 {
                self.sh[s].set_window(self.rate, ms);
            }
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let dry = *x;
                self.pre[s].push(dry);
                let inp = if delay >= 1.0 { self.pre[s].read_lin(delay) } else { dry };
                let r = if inv && s == 1 { 1.0 / ratio } else { ratio };
                let y = self.sh[s].run(inp, r);
                *x = dry * (1.0 - mix) + y * mix;
            }
        }
    }
}
plugin_impl!(PitchShifter);

// ---------------------------------------------------------------------------------------------
// Pitch Correction

pub const SCALES: &[(&str, [bool; 12])] = &[
    ("Chromatic", [true; 12]),
    ("Major", [true, false, true, false, true, true, false, true, false, true, false, true]),
    ("Minor", [true, false, true, true, false, true, false, true, true, false, true, false]),
    ("Harmonic Minor", [true, false, true, true, false, true, false, true, true, false, false, true]),
    ("Major Pentatonic", [true, false, true, false, true, false, false, true, false, true, false, false]),
    ("Minor Pentatonic", [true, false, false, true, false, true, false, true, false, false, true, false]),
    ("Dorian", [true, false, true, true, false, true, false, true, false, true, true, false]),
    ("Mixolydian", [true, false, true, false, true, true, false, true, false, true, true, false]),
    ("Blues", [true, false, false, true, false, true, true, true, false, false, true, false]),
    ("Major Chord", [true, false, false, false, true, false, false, true, false, false, false, false]),
    ("Minor Chord", [true, false, false, true, false, false, false, true, false, false, false, false]),
    ("sus4", [true, false, false, false, false, true, false, true, false, false, false, false]),
    ("Drone", [true, false, false, false, false, false, false, true, false, false, false, false]),
    ("Single", [true, false, false, false, false, false, false, false, false, false, false, false]),
];
const NOTES: &[&str] = &["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

/// Edit Scale (notes left out of the grid) at 1020 + pitch class; Bypass Notes at 1040 + class.
pub const PC_EDIT: u32 = B + 20;
pub const PC_BYPASS: u32 = B + 40;

static PC_P: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let names: &'static [&'static str] = Box::leak(SCALES.iter().map(|s| s.0).collect::<Vec<_>>().into_boxed_slice());
    let mut v = vec![
        Param::menu(B, "Root Note", NOTES, 0.0),
        Param::menu(B + 1, "Scale/Chord", names, 0.0),
        Param::new(B + 2, "Bypass All", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(B + 3, "Pitch Range", &["Normal", "Low"], 0.0),
        Param::new(B + 4, "Response", 0.0, 400.0, 60.0, Unit::Ms),
        Param::new(B + 5, "Tolerance", 0.0, 100.0, 0.0, Unit::Cents),
        Param::new(B + 6, "Global Tuning", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(B + 7, "Reference", -50.0, 50.0, 0.0, Unit::Cents),
        Param::new(B + 8, "Detune", -100.0, 100.0, 0.0, Unit::Cents),
        Param::new(B + 9, "Neural Pitch Detection", 0.0, 1.0, 1.0, Unit::OnOff),
    ];
    for k in 0..12 {
        v.push(Param::new(PC_EDIT + k, crate::kit::leak(format!("Exclude {}", NOTES[k as usize])), 0.0, 1.0, 0.0, Unit::OnOff));
    }
    for k in 0..12 {
        v.push(Param::new(PC_BYPASS + k, crate::kit::leak(format!("Bypass {}", NOTES[k as usize])), 0.0, 1.0, 0.0, Unit::OnOff));
    }
    v
});

pub struct PitchCorrection {
    p: Values,
    rate: f32,
    sh: [Shifter; 2],
    tr: Tracker,
    ratio: Smooth,
    pub correction_cents: f32,
}

impl PitchCorrection {
    pub fn new(rate: f32) -> PitchCorrection {
        let mut p = PitchCorrection { p: Values::new(PC_P.as_slice()), rate, sh: [Shifter::new(rate, 60.0), Shifter::new(rate, 60.0)], tr: Tracker::new(rate), ratio: Smooth::new(rate, 60.0, 1.0), correction_cents: 0.0 };
        p.update();
        p
    }
    /// The notes of the grid (pitch classes) after Edit Scale.
    pub fn grid(&self) -> [bool; 12] {
        let root = self.p.idx(B);
        let sc = SCALES[self.p.idx(B + 1).min(SCALES.len() - 1)].1;
        std::array::from_fn(|pc| sc[(pc + 12 - root) % 12] && !self.p.on(PC_EDIT + pc as u32))
    }
    /// The corrected pitch (MIDI note, fractional) for a detected one; None to leave it.
    pub fn target(&self, note: f32) -> Option<f32> {
        let pc = (note.round() as i32).rem_euclid(12) as usize;
        if self.p.on(PC_BYPASS + pc as u32) {
            return None;
        }
        let grid = self.grid();
        if !grid.iter().any(|g| *g) {
            return None;
        }
        let base = note.round() as i32;
        let best = (-6..=6).map(|d| base + d).filter(|n| grid[n.rem_euclid(12) as usize]).min_by(|a, b| (*a as f32 - note).abs().partial_cmp(&(*b as f32 - note).abs()).unwrap())?;
        let tol = self.p.get(B + 5) / 100.0;
        if (best as f32 - note).abs() <= tol {
            return None;
        }
        Some(best as f32)
    }
    fn update(&mut self) {
        self.ratio.set_time(self.rate, self.p.get(B + 4).max(0.5));
        self.tr.lo = if self.p.idx(B + 3) == 1 { 40.0 } else { 70.0 };
        self.tr.hi = if self.p.idx(B + 3) == 1 { 500.0 } else { 1200.0 };
    }
    fn clear(&mut self) {
        self.sh[0].clear();
        self.sh[1].clear();
    }
    fn tail_secs(&self) -> f32 {
        0.1
    }
}

impl Effect for PitchCorrection {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let bypass = self.p.on(B + 2);
        let refc = if self.p.on(B + 6) { 0.0 } else { self.p.get(B + 7) };
        let detune = self.p.get(B + 8);
        for i in 0..left.len().min(right.len()) {
            self.tr.push(0.5 * (left[i] + right[i]), self.rate);
            if bypass {
                continue;
            }
            let want = match self.tr.hz {
                Some(f) => {
                    let note = 69.0 + 12.0 * (f / 440.0).log2() - refc / 100.0;
                    let t = self.target(note).map_or(note, |t| t);
                    self.correction_cents = (t - note) * 100.0;
                    2f32.powf((t - note + detune / 100.0) / 12.0)
                }
                None => 2f32.powf(detune / 1200.0),
            };
            self.ratio.target = want;
            let r = self.ratio.next();
            let ms = self.tr.hz.map_or(30.0, |f| (2000.0 / f).clamp(8.0, 60.0));
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                self.sh[s].set_window(self.rate, ms);
                *x = if (r - 1.0).abs() < 1e-4 { *x } else { self.sh[s].run(*x, r) };
            }
        }
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        let mut v = vec![("Correction", self.correction_cents)];
        if let Some(f) = self.tr.hz {
            v.push(("Input Pitch", 69.0 + 12.0 * (f / 440.0).log2()));
        }
        v
    }
}
plugin_impl!(PitchCorrection);

// ---------------------------------------------------------------------------------------------
// Vocal Transformer

static VT_P: [Param; 14] = [
    Param::new(B, "Pitch", -24.0, 24.0, 0.0, Unit::Semitones),
    Param::new(B + 1, "Robotize", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 2, "Pitch Base", -24.0, 24.0, 0.0, Unit::Semitones),
    Param::new(B + 3, "Tracking", -100.0, 200.0, 100.0, Unit::Percent),
    Param::new(B + 4, "Mix", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(B + 5, "Formant", -24.0, 24.0, 0.0, Unit::Semitones),
    Param::new(B + 6, "Glide", 0.0, 1000.0, 0.0, Unit::Ms),
    // 0 = Auto (two periods of the detected pitch)
    Param::new(B + 7, "Grain Size", 0.0, 8.0, 4.0, Unit::None),
    Param::menu(B + 8, "Formants", &["Process always", "Keep unvoiced formants"], 0.0),
    Param::new(B + 9, "Detune", -100.0, 100.0, 0.0, Unit::Cents),
    // the Tracking buttons: -1, 0, 1 and 2 set Tracking to -100, 0, 100 and 200 % (momentary)
    Param::new(B + 10, "Tracking -1", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 11, "Tracking 0", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 12, "Tracking 1", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 13, "Tracking 2", 0.0, 1.0, 0.0, Unit::OnOff),
];

pub struct VocalTransformer {
    p: Values,
    rate: f32,
    formant: [Shifter; 2],
    pitch: [Shifter; 2],
    tr: Tracker,
    ratio: Smooth,
}

impl VocalTransformer {
    pub fn new(rate: f32) -> VocalTransformer {
        let mut v = VocalTransformer { p: Values::new(&VT_P), rate, formant: [Shifter::new(rate, 4.0), Shifter::new(rate, 4.0)], pitch: [Shifter::new(rate, 60.0), Shifter::new(rate, 60.0)], tr: Tracker::new(rate), ratio: Smooth::new(rate, 1.0, 1.0) };
        v.update();
        v
    }
    fn update(&mut self) {
        self.ratio.set_time(self.rate, self.p.get(B + 6).max(0.5));
    }
    fn clear(&mut self) {
        for s in self.formant.iter_mut().chain(self.pitch.iter_mut()) {
            s.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        0.1
    }
}

impl Effect for VocalTransformer {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rf = 2f32.powf(self.p.get(B + 5) / 12.0);
        let mix = self.p.get(B + 4) / 100.0;
        let robot = self.p.on(B + 1);
        let keep_unvoiced = self.p.idx(B + 8) == 1;
        let detune = self.p.get(B + 9) / 100.0;
        for i in 0..left.len().min(right.len()) {
            self.tr.push(0.5 * (left[i] + right[i]), self.rate);
            // Robotize: the melody mirrored / scaled around Pitch Base by Tracking
            let st = if robot {
                match self.tr.hz {
                    Some(f) => {
                        let note = 69.0 + 12.0 * (f / 440.0).log2();
                        let base = 60.0 + self.p.get(B + 2);
                        let k = self.p.get(B + 3) / 100.0;
                        (base + (note - base) * k) - note + self.p.get(B)
                    }
                    None => self.p.get(B),
                }
            } else {
                self.p.get(B)
            } + detune;
            self.ratio.target = 2f32.powf(st / 12.0);
            let rp = self.ratio.next();
            let voiced = self.tr.hz.is_some();
            let gs = self.p.get(B + 7);
            let gs = if gs < 0.5 { 4.0 } else { gs };
            let ms = self.tr.hz.map_or(30.0, |f| (1000.0 * gs / 2.0 / f).clamp(5.0, 60.0));
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let dry = *x;
                // formants: a very short grain shift moves the spectral envelope (and pitch);
                // the period-synchronous stage then sets the pitch to rp
                let use_f = (rf - 1.0).abs() > 1e-4 && (voiced || !keep_unvoiced);
                let a = if use_f { self.formant[s].run(dry, rf) } else { dry };
                let total = if use_f { rp / rf } else { rp };
                self.pitch[s].set_window(self.rate, ms);
                let y = if (total - 1.0).abs() < 1e-4 { a } else { self.pitch[s].run(a, total) };
                *x = dry * (1.0 - mix) + y * mix;
            }
        }
    }
}
impl VocalTransformer {
    pub fn edit(&mut self, id: u32, value: f32) {
        if (B + 10..=B + 13).contains(&id) {
            if value > 0.5 {
                self.p.set(B + 3, [-100.0, 0.0, 100.0, 200.0][(id - B - 10) as usize]);
            }
            return;
        }
        if self.p.set(id, value) {
            self.update();
        }
    }
}
crate::util::plugin_impl_edit!(VocalTransformer);

pub fn by_name(name: &str, rate: f32) -> Option<Box<dyn Effect>> {
    Some(match name {
        "Pitch Shifter" | "PShft" => Box::new(PitchShifter::new(rate)),
        "Pitch Correction" => Box::new(PitchCorrection::new(rate)),
        "Vocal Transformer" => Box::new(VocalTransformer::new(rate)),
        _ => return None,
    })
}

pub const NAMES: &[&str] = &["Pitch Shifter", "Pitch Correction", "Vocal Transformer"];

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::Plugin;

    fn tone(n: usize, f: f32) -> Vec<f32> {
        (0..n).map(|i| {
            let t = i as f32 / 48000.0;
            0.4 * (crate::util::TAU * f * t).sin() + 0.2 * (crate::util::TAU * 2.0 * f * t).sin()
        }).collect()
    }

    #[test]
    fn pitch_shifter_fifth_up() {
        let mut p = PitchShifter::new(48000.0);
        p.set(B, 7.0);
        let mut l = tone(48000, 220.0);
        let mut r = l.clone();
        p.process(&mut l, &mut r);
        let f = yin(&l[30000..34096], 48000.0, 60.0, 1500.0).unwrap();
        assert!((f - 220.0 * 2f32.powf(7.0 / 12.0)).abs() < 6.0, "{f}");
    }

    #[test]
    fn pitch_shifter_timing_modes() {
        assert_eq!(PS_P[3].choices, &["Drums", "Speech", "Vocals", "Manual", "Pitch Tracking"]);
        let run = |set: &[(u32, f32)]| {
            let mut p = PitchShifter::new(48000.0);
            p.set(B, 7.0);
            for &(id, v) in set {
                p.set(id, v);
            }
            let mut l = tone(48000, 220.0);
            let mut r = l.clone();
            p.process(&mut l, &mut r);
            (l, p.sh[0].window)
        };
        // every mode shifts a fifth; the grain size follows the mode
        let mut sizes = vec![];
        for m in 0..5 {
            let (l, w) = run(&[(B + 3, m as f32)]);
            // (Drums' short grains are for percussive material: a tone only has to come out)
            if m == 0 {
                assert!(l[30000..].iter().any(|x| x.abs() > 0.05));
            } else {
                let f = yin(&l[30000..34096], 48000.0, 60.0, 1500.0).unwrap();
                assert!((f - 220.0 * 2f32.powf(7.0 / 12.0)).abs() < 8.0, "mode {m}: {f}");
            }
            sizes.push(w);
        }
        assert!(sizes[0] < sizes[1] && sizes[1] < sizes[2]);
        // Delay and Crossfade only count in Manual
        let (a, _) = run(&[(B + 3, 1.0), (B + 5, 50.0), (B + 6, 10.0)]);
        let (b, _) = run(&[(B + 3, 1.0)]);
        assert_eq!(a, b);
        let (c, _) = run(&[(B + 3, 3.0), (B + 4, 30.0), (B + 6, 10.0)]);
        let (d, _) = run(&[(B + 3, 3.0), (B + 4, 30.0), (B + 6, 100.0)]);
        assert_ne!(c, d);
        let (e, _) = run(&[(B + 3, 3.0), (B + 4, 30.0), (B + 5, 50.0)]);
        let (g, _) = run(&[(B + 3, 3.0), (B + 4, 30.0)]);
        assert!(e[..2000].iter().map(|x| x.abs()).sum::<f32>() < g[..2000].iter().map(|x| x.abs()).sum::<f32>());
    }

    #[test]
    fn vocal_transformer_tracking_buttons_and_auto_grain() {
        let mut v = VocalTransformer::new(48000.0);
        for (b, want) in [(B + 10, -100.0), (B + 11, 0.0), (B + 13, 200.0), (B + 12, 100.0)] {
            v.set(b, 1.0);
            assert_eq!(v.get(B + 3), want);
            assert_eq!(v.get(b), 0.0);
        }
        // 200 %: an interval doubled around Pitch Base (C4 = 60): A3 (57) -> 54
        v.set(B + 13, 1.0);
        v.set(B + 1, 1.0);
        let f = 440.0 * 2f32.powf((57.0 - 69.0) / 12.0);
        let mut l: Vec<f32> = (0..48000).map(|i| 0.4 * (crate::util::TAU * f * i as f32 / 48000.0).sin()).collect();
        let mut r = l.clone();
        v.process(&mut l, &mut r);
        let out = yin(&l[36000..40096], 48000.0, 60.0, 1500.0).unwrap();
        let want = 440.0 * 2f32.powf((54.0 - 69.0) / 12.0);
        assert!((out - want).abs() < 4.0, "{out} {want}");
        // Grain Size 0 = Auto
        v.set(B + 7, 0.0);
        assert_eq!(v.get(B + 7), 0.0);
    }

    #[test]
    fn pitch_correction_pulls_to_the_scale() {
        let mut p = PitchCorrection::new(48000.0);
        p.set(B + 4, 5.0);
        // 225 Hz is about 39 cents above A3 (220): C major corrects it to A
        let mut l = tone(48000, 225.0);
        let mut r = l.clone();
        p.process(&mut l, &mut r);
        let f = yin(&l[36000..40096], 48000.0, 60.0, 1500.0).unwrap();
        assert!((f - 220.0).abs() < 2.5, "{f}");
        // with A bypassed nothing changes
        let mut p = PitchCorrection::new(48000.0);
        p.set(PC_BYPASS + 9, 1.0);
        let mut l = tone(48000, 225.0);
        let mut r = l.clone();
        p.process(&mut l, &mut r);
        let f = yin(&l[36000..40096], 48000.0, 60.0, 1500.0).unwrap();
        assert!((f - 225.0).abs() < 2.0, "{f}");
    }

    #[test]
    fn scale_grid_follows_root_and_edits() {
        let mut p = PitchCorrection::new(48000.0);
        p.set(B, 2.0); // D
        p.set(B + 1, 1.0); // major: D E F# G A B C#
        let g = p.grid();
        assert!(g[2] && g[6] && g[1] && !g[0]);
        p.set(PC_EDIT + 6, 1.0);
        assert!(!p.grid()[6]);
    }

    #[test]
    fn vocal_transformer_pitch_without_formant() {
        let mut v = VocalTransformer::new(48000.0);
        v.set(B, -12.0);
        let mut l = tone(48000, 300.0);
        let mut r = l.clone();
        v.process(&mut l, &mut r);
        let f = yin(&l[30000..34096], 48000.0, 60.0, 1500.0).unwrap();
        assert!((f - 150.0).abs() < 5.0, "{f}");
    }
}
