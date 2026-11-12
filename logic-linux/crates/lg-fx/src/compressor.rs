//! Compressor: the stand-in for Logic's Compressor, with its seven circuit types.
//!
//! The parameter block follows a project's Compressor floats (Threshold first). The circuit types
//! are distinct models, not presets of one curve:
//!
//! | Circuit          | topology     | detector (Peak/RMS button aside)  | knee / character            |
//! |------------------|--------------|-----------------------------------|-----------------------------|
//! | Platinum Digital | feed-forward | sine-calibrated RMS, exact        | clean, the knee as set      |
//! | Studio VCA       | feed-forward | fast RMS                          | firm knee, quick release    |
//! | Studio FET       | feedback     | peak-leaning, very fast           | bright, slight 2nd harmonic |
//! | Classic VCA      | feed-forward | true RMS, program-dependent attack| wide "over easy" knee       |
//! | Vintage VCA      | feedback     | RMS, bus-compressor timing        | soft knee, glue             |
//! | Vintage FET      | feedback     | true RMS                          | FET colour (odd + even)     |
//! | Vintage Opto     | feedback     | slow optical cell                 | two-stage release, soft     |
//!
//! Feedback circuits look at their own output, so a ratio acts gentler there (as on the
//! hardware they echo). Auto Gain (0 dB / -12 dB) makes up the static gain reduction a steady
//! signal at that level would get. Auto Release adds a slow second release stage that holds
//! sustained compression while transients recover at the set release.
//! Levels and curves were matched to Logic renders of the default settings (sine, noise and
//! impulse gains); see tests/reference.rs.

use crate::util::{Biquad, Coefs, Values, coef, db, flush, plugin_impl};
use lg_dsp::{Param, Unit};

pub const CIRCUITS: &[&str] = &["Platinum Digital", "Studio VCA", "Studio FET", "Classic VCA", "Vintage VCA", "Vintage FET", "Vintage Opto"];
const SC_MODES: &[&str] = &["LP", "BP", "HP", "ParEQ", "HS"];

pub const THRESHOLD: u32 = 0;
pub const RATIO: u32 = 1;
pub const ATTACK: u32 = 2;
pub const RELEASE: u32 = 3;
pub const MAKEUP: u32 = 4;
pub const KNEE: u32 = 5;
pub const PEAK_RMS: u32 = 6;
pub const AUTO_GAIN: u32 = 7;
pub const DISTORTION: u32 = 8;
pub const CIRCUIT: u32 = 9;
pub const SC_DETECT: u32 = 10;
pub const LIMIT_THR: u32 = 11;
pub const LIMIT_ON: u32 = 12;
pub const AUTO_RELEASE: u32 = 13;
pub const SC_FILTER: u32 = 14;
pub const SC_MODE: u32 = 15;
pub const SC_FREQ: u32 = 17;
pub const SC_Q: u32 = 18;
pub const SC_GAIN: u32 = 19;
pub const MIX: u32 = 24;
pub const IN_GAIN: u32 = 25;
pub const OUT_GAIN: u32 = 26;

static PARAMS: [Param; 24] = [
    Param::new(THRESHOLD, "Threshold", -60.0, 0.0, -20.0, Unit::Db),
    Param::new(RATIO, "Ratio", 1.0, 30.0, 2.1, Unit::Ratio),
    Param::new(ATTACK, "Attack", 0.0, 200.0, 15.0, Unit::Ms),
    Param::new(RELEASE, "Release", 5.0, 5000.0, 51.0, Unit::Ms),
    Param::new(MAKEUP, "Make Up", -30.0, 30.0, 0.0, Unit::Db),
    Param::new(KNEE, "Knee", 0.0, 1.0, 0.7, Unit::None),
    Param::menu(PEAK_RMS, "Peak/RMS", &["Peak", "RMS"], 1.0),
    Param::menu(AUTO_GAIN, "Auto Gain", &["Off", "0 dB", "-12 dB"], 2.0),
    Param::menu(DISTORTION, "Distortion", &["Off", "Soft", "Hard", "Clip"], 0.0),
    Param::menu(CIRCUIT, "Circuit Type", CIRCUITS, 0.0),
    Param::menu(SC_DETECT, "Side Chain Detection", &["Max", "Sum"], 0.0),
    Param::new(LIMIT_THR, "Limiter Threshold", -20.0, 0.0, 0.0, Unit::Db),
    Param::new(LIMIT_ON, "Limiter On", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(AUTO_RELEASE, "Auto Release", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(SC_FILTER, "Side Chain Filter State", &["Off", "On"], 0.0),
    Param::menu(SC_MODE, "Side Chain Filter Mode", SC_MODES, 0.0),
    Param::new(SC_FREQ, "Side Chain Filter Frequency", 20.0, 20000.0, 7040.0, Unit::Hz),
    Param::new(SC_Q, "Side Chain Filter Q", 0.1, 100.0, 1.0, Unit::Ratio),
    Param::new(SC_GAIN, "Side Chain Filter Gain", -24.0, 24.0, 0.0, Unit::Db),
    Param::new(MIX, "Output Mix", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(IN_GAIN, "Input Gain", -30.0, 30.0, 0.0, Unit::Db),
    Param::new(OUT_GAIN, "Output Gain", -30.0, 30.0, 0.0, Unit::Db),
    // the gain-reduction meter's float (Logic stores the display state at 20-23)
    Param::new(16, "Side Chain Filter (reserved)", 0.0, 1.0, 0.0, Unit::None),
    // (our own id: Logic's Listen state isn't decoded)
    Param::new(LISTEN, "Listen", 0.0, 1.0, 0.0, Unit::OnOff),
];

/// The side chain section's Listen button: the output is the (filtered) side chain signal.
pub const LISTEN: u32 = 1000;

/// How one circuit type behaves.
#[derive(Clone, Copy, Debug)]
struct Circuit {
    /// Detects on its own output (gentler effective ratio).
    feedback: bool,
    /// dB added to the detected level: the circuit's threshold offset (always) and its RMS
    /// calibration (in RMS mode only; 3 dB reads a sine's RMS as its peak).
    det_offset: f32,
    rms_cal: f32,
    /// RMS averaging time (ms) when RMS is chosen; peak mode always uses a fast peak hold.
    rms_ms: f32,
    /// Knee width in dB for Knee = 1.
    knee_db: f32,
    /// Attack and release scale (the circuits' own timing laws).
    att_scale: f32,
    rel_scale: f32,
    /// Auto release: the slow stage's release (ms) and its share.
    slow_release: f32,
    /// Auto Gain's reference: the level (dBFS) whose static reduction "-12 dB" makes up.
    auto_ref: f32,
    /// Saturation: 2nd harmonic and 3rd harmonic amounts at full reduction.
    even: f32,
    odd: f32,
    /// Optical cell: attack and release slow down with how hard and how long it worked.
    opto: bool,
}

const CIRC: [Circuit; 7] = [
    // Platinum Digital
    Circuit { feedback: false, det_offset: 0.0, rms_cal: 3.0, rms_ms: 12.0, knee_db: 6.0, att_scale: 1.0, rel_scale: 1.0, slow_release: 400.0, auto_ref: -16.5, even: 0.0, odd: 0.0, opto: false },
    // Studio VCA
    Circuit { feedback: false, det_offset: 0.0, rms_cal: 1.5, rms_ms: 6.0, knee_db: 8.0, att_scale: 0.7, rel_scale: 0.8, slow_release: 300.0, auto_ref: -12.0, even: 0.002, odd: 0.004, opto: false },
    // Studio FET
    Circuit { feedback: true, det_offset: 0.0, rms_cal: 1.5, rms_ms: 3.0, knee_db: 4.0, att_scale: 0.3, rel_scale: 0.9, slow_release: 500.0, auto_ref: -12.0, even: 0.01, odd: 0.004, opto: false },
    // Classic VCA
    Circuit { feedback: false, det_offset: 0.0, rms_cal: 0.0, rms_ms: 20.0, knee_db: 18.0, att_scale: 1.2, rel_scale: 1.0, slow_release: 600.0, auto_ref: -12.0, even: 0.003, odd: 0.006, opto: false },
    // Vintage VCA
    Circuit { feedback: true, det_offset: -1.0, rms_cal: 0.0, rms_ms: 15.0, knee_db: 6.0, att_scale: 1.4, rel_scale: 1.0, slow_release: 800.0, auto_ref: -9.1, even: 0.004, odd: 0.006, opto: false },
    // Vintage FET
    Circuit { feedback: true, det_offset: -0.8, rms_cal: 0.0, rms_ms: 10.0, knee_db: 4.0, att_scale: 0.5, rel_scale: 1.0, slow_release: 600.0, auto_ref: -11.4, even: 0.02, odd: 0.01, opto: false },
    // Vintage Opto
    Circuit { feedback: true, det_offset: -0.8, rms_cal: 0.0, rms_ms: 25.0, knee_db: 14.0, att_scale: 1.0, rel_scale: 1.0, slow_release: 1500.0, auto_ref: -11.5, even: 0.006, odd: 0.003, opto: true },
];

pub struct Compressor {
    p: Values,
    rate: f32,
    c: Circuit,
    // settings derived on update
    thr: f32,
    slope: f32,
    knee: f32,
    att: f32,
    rel: f32,
    rel_slow: f32,
    att_slow: f32,
    rms_k: f32,
    peak_rel: f32,
    makeup: f32,
    in_gain: f32,
    out_gain: f32,
    mix: f32,
    lim_thr: f32,
    sc: [Biquad; 2],
    // state
    ms: [f32; 2],
    peak: [f32; 2],
    gr_fast: f32,
    gr_slow: f32,
    /// last linear gain (for feedback detection)
    g_last: f32,
    lim_gain: f32,
    lim_rel: f32,
    /// Opto cell memory: how worked-up the cell is (slows its release).
    opto_mem: f32,
    opto_up: f32,
    opto_down: f32,
    dc: [f32; 2],
    dc_k: f32,
    /// The current gain reduction in dB (positive), for a meter.
    pub reduction: f32,
    /// The external side chain's next block (header Side Chain pop-up): it only triggers.
    side: Option<(Vec<f32>, Vec<f32>)>,
}

impl Compressor {
    pub fn new(rate: f32) -> Compressor {
        let mut c = Compressor {
            p: Values::new(&PARAMS),
            rate,
            c: CIRC[0],
            thr: -20.0,
            slope: 0.5,
            knee: 0.0,
            att: 0.0,
            rel: 0.0,
            rel_slow: 0.0,
            att_slow: 0.0,
            rms_k: 0.0,
            peak_rel: 0.0,
            makeup: 1.0,
            in_gain: 1.0,
            out_gain: 1.0,
            mix: 1.0,
            lim_thr: 1.0,
            sc: [Biquad::default(); 2],
            ms: [0.0; 2],
            peak: [0.0; 2],
            gr_fast: 0.0,
            gr_slow: 0.0,
            g_last: 1.0,
            lim_gain: 1.0,
            lim_rel: 0.0,
            opto_mem: 0.0,
            opto_up: coef(rate, 300.0),
            opto_down: coef(rate, 4000.0),
            dc: [0.0; 2],
            dc_k: 1.0 - coef(rate, 1000.0 / (crate::util::TAU * 5.0)),
            reduction: 0.0,
            side: None,
        };
        c.update();
        c
    }

    /// The static gain reduction (dB, positive) for a detected level `x` dB.
    fn static_gr(&self, x: f32) -> f32 {
        let over = x - self.thr;
        let w = self.knee;
        if w > 0.0 && over > -w / 2.0 && over < w / 2.0 {
            self.slope * (over + w / 2.0) * (over + w / 2.0) / (2.0 * w)
        } else if over > 0.0 {
            self.slope * over
        } else {
            0.0
        }
    }

    /// The reduction the circuit settles on for a steady level (feedback circuits solve
    /// g = f(x - g)).
    fn steady_gr(&self, level_db: f32) -> f32 {
        let x = level_db + self.c.det_offset + if self.p.idx(PEAK_RMS) == 1 { self.c.rms_cal } else { 0.0 };
        if !self.c.feedback {
            return self.static_gr(x);
        }
        let mut g = 0.0;
        for _ in 0..40 {
            g = 0.5 * g + 0.5 * self.static_gr(x - g);
        }
        g
    }

    fn update(&mut self) {
        let p = &self.p;
        self.c = CIRC[p.idx(CIRCUIT).min(6)];
        let rate = self.rate;
        self.thr = p.get(THRESHOLD);
        let ratio = p.get(RATIO).max(1.0);
        self.slope = 1.0 - 1.0 / ratio;
        self.knee = p.get(KNEE) * self.c.knee_db;
        let att_ms = (p.get(ATTACK) * self.c.att_scale).max(0.05);
        let rel_ms = (p.get(RELEASE) * self.c.rel_scale).max(1.0);
        self.att = coef(rate, att_ms);
        self.rel = coef(rate, rel_ms);
        self.att_slow = coef(rate, att_ms * 6.0 + 20.0);
        self.rel_slow = coef(rate, self.c.slow_release.max(rel_ms * 2.0));
        let rms_ms = if p.idx(PEAK_RMS) == 1 { self.c.rms_ms } else { 0.0 };
        self.rms_k = coef(rate, rms_ms);
        self.peak_rel = coef(rate, (rel_ms * 0.5).clamp(5.0, 60.0));
        self.in_gain = db(p.get(IN_GAIN));
        self.out_gain = db(p.get(OUT_GAIN));
        self.mix = p.get(MIX) / 100.0;
        self.lim_thr = db(p.get(LIMIT_THR));
        self.lim_rel = coef(rate, 50.0);
        let auto = match p.idx(AUTO_GAIN) {
            1 => self.steady_gr(self.c.auto_ref + 12.0),
            2 => self.steady_gr(self.c.auto_ref),
            _ => 0.0,
        };
        self.makeup = db(p.get(MAKEUP) + auto);
        let (f, q, g) = (p.get(SC_FREQ), p.get(SC_Q), p.get(SC_GAIN));
        let c = if p.idx(SC_FILTER) == 0 {
            Coefs::THRU
        } else {
            match p.idx(SC_MODE) {
                0 => Coefs::lowpass(rate, f, q),
                1 => Coefs::bandpass(rate, f, q),
                2 => Coefs::highpass(rate, f, q),
                3 => Coefs::peak(rate, f, q, g),
                _ => Coefs::high_shelf(rate, f, q, g),
            }
        };
        self.sc[0].c = c;
        self.sc[1].c = c;
    }

    fn clear(&mut self) {
        self.ms = [0.0; 2];
        self.peak = [0.0; 2];
        self.gr_fast = 0.0;
        self.gr_slow = 0.0;
        self.g_last = 1.0;
        self.lim_gain = 1.0;
        self.opto_mem = 0.0;
        self.dc = [0.0; 2];
        self.sc[0].clear();
        self.sc[1].clear();
    }

    fn tail_secs(&self) -> f32 {
        0.0
    }

    #[inline]
    fn detect(&mut self, ch: usize, x: f32) -> f32 {
        let x = self.sc[ch].run(x);
        if self.rms_k > 0.0 {
            self.ms[ch] = flush(x * x + (self.ms[ch] - x * x) * self.rms_k);
            self.ms[ch]
        } else {
            let a = x.abs();
            self.peak[ch] = if a > self.peak[ch] { a } else { flush(a + (self.peak[ch] - a) * self.peak_rel) };
            self.peak[ch] * self.peak[ch]
        }
    }
}

/// Soft, hard and clip distortion stages of the Distortion menu.
#[inline]
fn distort(kind: usize, x: f32) -> f32 {
    match kind {
        1 => {
            // soft: a gentle tanh that leaves low levels nearly clean
            (x * 1.2).tanh() / 1.2f32.tanh()
        }
        2 => {
            // hard: a cubic that flattens sharply at full scale
            let x = x.clamp(-1.5, 1.5);
            x - x * x * x * 4.0 / 27.0
        }
        3 => x.clamp(-1.0, 1.0),
        _ => x,
    }
}

impl lg_dsp::Effect for Compressor {
    fn gain_reduction(&self) -> f32 {
        self.reduction
    }

    fn side_chain(&mut self, left: &[f32], right: &[f32]) {
        self.side = Some((left.to_vec(), right.to_vec()));
    }
    fn has_side_chain(&self) -> bool {
        true
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let side = self.side.take();
        let listen = self.p.on(LISTEN);
        let sum = self.p.idx(SC_DETECT) == 1;
        let dist = self.p.idx(DISTORTION);
        let limit = self.p.on(LIMIT_ON);
        let auto_rel = self.p.on(AUTO_RELEASE);
        let c = self.c;
        for i in 0..n {
            let (dl, dr) = (left[i] * self.in_gain, right[i] * self.in_gain);
            // feedback circuits listen after the gain stage; an external side chain only
            // triggers (its audio isn't processed)
            let (sl, sr) = if let Some((a, b)) = &side {
                (a.get(i).copied().unwrap_or(0.0) * self.in_gain, b.get(i).copied().unwrap_or(0.0) * self.in_gain)
            } else if c.feedback {
                (dl * self.g_last, dr * self.g_last)
            } else {
                (dl, dr)
            };
            if listen {
                // Listen: the side chain as the detector hears it (after its filter)
                left[i] = self.sc[0].run(sl);
                right[i] = self.sc[1].run(sr);
                continue;
            }
            let (ml, mr) = (self.detect(0, sl), self.detect(1, sr));
            let ms = if sum { 0.5 * (ml + mr) } else { ml.max(mr) };
            let level = 10.0 * ms.max(1e-12).log10() + c.det_offset + if self.rms_k > 0.0 { c.rms_cal } else { 0.0 };
            // (feedback circuits: the loop's own gain is already in the level)
            let target = self.static_gr(level);
            // gain reduction smoothing (dB): attack when reducing more, release when less
            let (att, rel) = if c.opto {
                // the cell gets lazier the more it has worked
                let slow = 1.0 + 3.0 * self.opto_mem;
                (self.att, self.rel.powf(1.0 / slow))
            } else {
                (self.att, self.rel)
            };
            let k = if target > self.gr_fast { att } else { rel };
            self.gr_fast = flush(target + (self.gr_fast - target) * k);
            let gr = if auto_rel {
                let k2 = if target > self.gr_slow { self.att_slow } else { self.rel_slow };
                self.gr_slow = flush(target + (self.gr_slow - target) * k2);
                // the slow stage holds about 2/3 of a sustained reduction
                self.gr_fast.max(self.gr_slow * 0.66)
            } else {
                self.gr_fast
            };
            if c.opto {
                // charge fast while working hard, discharge slowly (seconds)
                let t = (gr / 10.0).min(1.0);
                let k = if t > self.opto_mem { self.opto_up } else { self.opto_down };
                self.opto_mem = flush(t + (self.opto_mem - t) * k);
            }
            self.reduction = gr;
            let g = db(-gr);
            self.g_last = g;
            // circuit colour grows with the reduction (as the hardware's gain cells do)
            let col = (gr / 20.0).min(1.0);
            let (e, o) = (c.even * col * 4.0, c.odd * col * 4.0);
            // (bounded terms, so hot levels saturate instead of folding over)
            let shape = |x: f32| -> f32 { x + e * (x * x).min(4.0) - o * x * x * x / (1.0 + x * x) };
            let mut yl = shape(dl * g);
            let mut yr = shape(dr * g);
            if e > 0.0 || self.dc[0] != 0.0 || self.dc[1] != 0.0 {
                // the even term's DC, removed by a 5 Hz high-pass
                self.dc[0] = flush(self.dc[0] + (yl - self.dc[0]) * self.dc_k);
                self.dc[1] = flush(self.dc[1] + (yr - self.dc[1]) * self.dc_k);
                yl -= self.dc[0];
                yr -= self.dc[1];
            }
            yl *= self.makeup;
            yr *= self.makeup;
            if limit {
                let pk = yl.abs().max(yr.abs());
                let want = if pk > self.lim_thr { self.lim_thr / pk } else { 1.0 };
                self.lim_gain = if want < self.lim_gain { want } else { want + (self.lim_gain - want) * self.lim_rel };
                yl *= self.lim_gain;
                yr *= self.lim_gain;
            }
            yl = distort(dist, yl);
            yr = distort(dist, yr);
            left[i] = (left[i] * (1.0 - self.mix) + yl * self.mix) * self.out_gain;
            right[i] = (right[i] * (1.0 - self.mix) + yr * self.mix) * self.out_gain;
        }
    }
}

plugin_impl!(Compressor);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    /// An external side chain triggers the reduction; its audio isn't heard.
    #[test]
    fn side_chain_triggers_only() {
        let mut c = Compressor::new(48000.0);
        for (id, v) in [(RATIO, 10.0), (THRESHOLD, -30.0), (AUTO_GAIN, 0.0), (ATTACK, 1.0)] {
            c.set(id, v);
        }
        assert!(c.has_side_chain());
        let quiet = vec![0.01f32; 4800];
        let key: Vec<f32> = (0..4800).map(|i| 0.8 * (i as f32 * 0.05).sin()).collect();
        let (mut l, mut r) = (quiet.clone(), quiet.clone());
        c.side_chain(&key, &key);
        c.process(&mut l, &mut r);
        assert!(c.reduction > 10.0, "{}", c.reduction);
        assert!(l[4000..].iter().all(|v| v.abs() < 0.01));
        // without the key, the quiet input passes
        let (mut l, mut r) = (quiet.clone(), quiet.clone());
        c.reset();
        c.process(&mut l, &mut r);
        assert!((l[4700] - 0.01).abs() < 0.002);
        // Listen plays the side chain
        c.set(LISTEN, 1.0);
        let (mut l, mut r) = (quiet.clone(), quiet);
        c.side_chain(&key, &key);
        c.process(&mut l, &mut r);
        assert!((l[100] - key[100]).abs() < 1e-4);
    }

    /// A steady sine 14 dB over the threshold at 4:1 is turned down by 10.5 dB.
    #[test]
    fn reduction_at_a_known_level() {
        let mut c = Compressor::new(48000.0);
        for (id, v) in [(RATIO, 4.0), (KNEE, 0.0), (PEAK_RMS, 0.0), (AUTO_GAIN, 0.0), (ATTACK, 0.0), (RELEASE, 200.0), (AUTO_RELEASE, 0.0)] {
            c.set(id, v);
        }
        let x: Vec<f32> = (0..48000).map(|i| 0.5 * (crate::util::TAU * 1000.0 * i as f32 / 48000.0).sin()).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        c.process(&mut l, &mut r);
        let peak = l[24000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let gr = lg_dsp::gain_to_db(0.5 / peak);
        assert!((gr - 10.5).abs() < 0.5, "{gr}");
        assert!((c.reduction - 10.5).abs() < 0.6);
    }

    /// Below the threshold nothing happens (Auto Gain off).
    #[test]
    fn transparent_below_threshold() {
        let mut c = Compressor::new(48000.0);
        c.set(AUTO_GAIN, 0.0);
        c.set(KNEE, 0.0);
        let x: Vec<f32> = (0..9600).map(|i| 0.01 * (i as f32 * 0.05).sin()).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        c.process(&mut l, &mut r);
        assert!(l.iter().zip(x.iter()).all(|(a, b)| (a - b).abs() < 1e-3));
    }
}
