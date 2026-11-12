//! AutoFilter: the stand-in for Logic's AutoFilter — a resonant low-pass whose cutoff an LFO
//! and an input-driven envelope move, with pre / post distortion.
//!
//! The filter is a four-pole transistor-ladder style low-pass (each pole a one-pole stage with
//! soft saturation, resonance fed back from the last pole), tapped after 1-4 poles for
//! 6 / 12 / 18 / 24 dB/oct; Fatness puts back the low end resonance takes away. Cutoff (0-100 %)
//! maps 20 Hz-20 kHz on a log scale; the LFO (free Hz, or a synced note when its rate is
//! stored negative, like Tremolo) and the envelope (threshold-gated follower with attack,
//! decay and sustain) move it in octaves; Spread offsets left and right.
//!
//! Logic's sections and switches (our own ids 1000 up): Filter / Envelope / Distortion / LFO
//! on-off switches; State Variable HP / BP / LP / PK modes beside the 4-pole low-pass slopes
//! (choosing a slope turns State Variable off, like clicking a 4-Pole button); envelope
//! Retrigger, Dynamic and Release; LFO Sync (re-expresses the rate as a note value), Retrigger,
//! the five waveforms with Pulse Width; Classic / Tube / Scream distortion modes; Dry Signal
//! and Main Out.
//!
//! Positions confirmed from the research: Cutoff 2, Resonance 3, LFO intensity 12, Envelope
//! Threshold 15, LFO Rate 25, LFO Sync Phase 26, Spread 30. Fatness 4, slope 5, envelope
//! times 17-19, Stereo Phase 27 and the distortion levels 0 / 1 are the likely ones (unconfirmed).

use crate::util::{Noise, Phase, Values, beats_hz, coef, db, flush, sync_index_beats};
use lg_dsp::{Param, Unit};

const SV: &[&str] = &["Off", "HP", "BP", "LP", "PK"];
const WAVES: &[&str] = &["Saw Down", "Saw Up", "Triangle", "Pulse", "Random"];
const DIST: &[&str] = &["Classic", "Tube", "Scream"];

static PARAMS: [Param; 34] = [
    Param::new(0, "Distortion Input", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(1, "Distortion Output", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(2, "Cutoff", 0.0, 100.0, 70.0, Unit::Percent),
    Param::new(3, "Resonance", 0.0, 100.0, 10.0, Unit::Percent),
    Param::new(4, "Fatness", 0.0, 100.0, 0.0, Unit::Percent),
    Param::menu(5, "Slope", &["6 dB/Oct", "12 dB/Oct", "18 dB/Oct", "24 dB/Oct"], 2.0),
    Param::new(12, "LFO", -100.0, 100.0, 14.0, Unit::Percent),
    Param::new(15, "Threshold", -60.0, 0.0, -20.0, Unit::Db),
    Param::new(16, "Envelope", -100.0, 100.0, 0.0, Unit::Percent),
    Param::new(17, "Attack", 0.0, 2000.0, 52.0, Unit::Ms),
    Param::new(18, "Sustain", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(19, "Decay", 0.0, 5000.0, 52.0, Unit::Ms),
    Param::new(25, "LFO Rate", -26.0, 100.0, -11.0, Unit::None),
    Param::new(26, "LFO Sync Phase", 0.0, 360.0, 180.0, Unit::None),
    Param::new(27, "Stereo Phase", -180.0, 180.0, 0.0, Unit::None),
    Param::new(30, "Spread", 0.0, 1.0, 0.01, Unit::None),
    Param::new(1000, "Filter On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(1001, "State Variable", SV, 0.0),
    Param::new(1002, "Envelope On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(1003, "Env Retrigger", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(1004, "Dynamic", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(1005, "Release", 0.0, 10000.0, 200.0, Unit::Ms),
    Param::new(1006, "LFO On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(1007, "LFO Sync", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(1008, "LFO Retrigger", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(1009, "LFO Waveform", WAVES, 2.0),
    Param::new(1010, "Pulse Width", 5.0, 95.0, 50.0, Unit::Percent),
    Param::new(1011, "Distortion On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(1012, "Pre Mode", DIST, 0.0),
    Param::menu(1013, "Post Mode", DIST, 0.0),
    Param::new(1014, "Dry Signal", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(1015, "Main Out", -30.0, 12.0, 0.0, Unit::Db),
    Param::new(1016, "Fatness Comp", 0.0, 1.0, 0.0, Unit::None).hide(),
    Param::new(1017, "LFO Hz Memory", 0.0, 100.0, 0.0, Unit::None).hide(),
];

/// One distortion stage of the given mode (0 Classic, 1 Tube, 2 Scream) at drive 0-1.
fn distort(mode: usize, v: f32, d: f32) -> f32 {
    match mode {
        // Tube: asymmetric (even harmonics), softer knee
        1 => {
            let g = 1.0 + 6.0 * d;
            let b = 0.25 * d;
            ((v * g + b).tanh() - b.tanh()) / (1.0 + 1.5 * d)
        }
        // Scream: hard, bright clipping
        2 => {
            let g = 1.0 + 20.0 * d;
            (v * g).clamp(-1.0, 1.0) / (1.0 + 2.5 * d)
        }
        _ => (v * (1.0 + 8.0 * d)).tanh() / (1.0 + 2.0 * d),
    }
}

pub struct AutoFilter {
    /// The side chain's block: the envelope follows it when one is chosen.
    side: Option<(Vec<f32>, Vec<f32>)>,
    p: Values,
    rate: f32,
    bpm: f64,
    lfo: Phase,
    noise: Noise,
    /// The random waveform's held value and the phase it was taken at.
    held: f32,
    last_ph: f32,
    env: f32,
    /// The envelope's peak for this trigger (Dynamic scales it by the input level).
    peak: f32,
    above: bool,
    /// Peak-held input level the threshold looks at (so a waveform's zero crossings do not
    /// count as falling below it).
    det: f32,
    sv: [[f32; 2]; 2],
    stage: f32,
    pole: [[f32; 4]; 2],
    att: f32,
    dec: f32,
    rel: f32,
}

impl AutoFilter {
    pub fn new(rate: f32) -> AutoFilter {
        let mut a = AutoFilter { side: None, p: Values::new(&PARAMS), rate, bpm: 120.0, lfo: Phase::default(), noise: Noise(0x2545_F491), held: 0.0, last_ph: 0.0, env: 0.0, peak: 1.0, above: false, det: 0.0, sv: [[0.0; 2]; 2], stage: 2.0, pole: [[0.0; 4]; 2], att: 0.0, dec: 0.0, rel: 0.0 };
        a.update();
        a
    }

    fn lfo_hz(&self) -> f32 {
        let r = self.p.get(25);
        if r < 0.0 || (self.p.on(1007) && r < 0.5) { beats_hz(sync_index_beats(r), self.bpm) } else { r.max(0.01) }
    }

    fn update(&mut self) {
        self.att = coef(self.rate, self.p.get(17).max(0.1));
        self.dec = coef(self.rate, self.p.get(19).max(0.1));
        self.rel = coef(self.rate, self.p.get(1005).max(0.1));
    }

    fn clear(&mut self) {
        self.pole = [[0.0; 4]; 2];
        self.sv = [[0.0; 2]; 2];
        self.above = false;
        self.det = 0.0;
        self.env = 0.0;
        self.stage = 2.0;
    }

    fn tail_secs(&self) -> f32 {
        0.05
    }

    /// The LFO's value (-1..1) at a phase, for the chosen waveform.
    fn wave(&mut self, ph: f32) -> f32 {
        let ph = ph - ph.floor();
        match self.p.idx(1009) {
            0 => 1.0 - 2.0 * ph,
            1 => 2.0 * ph - 1.0,
            3 => if ph < self.p.get(1010) / 100.0 { 1.0 } else { -1.0 },
            4 => {
                if ph < self.last_ph {
                    self.held = self.noise.next();
                }
                self.last_ph = ph;
                self.held
            }
            _ => crate::util::tri(ph),
        }
    }

    /// A user edit (the plug-in window or automation), with Logic's linked buttons: a 4-pole
    /// slope turns State Variable off; LFO Sync re-expresses the rate as a note value or Hz.
    pub fn edit(&mut self, id: u32, value: f32) {
        match id {
            5 => {
                self.p.set(1001, 0.0);
            }
            1007 => {
                let was = self.p.on(1007) || self.p.get(25) < 0.0;
                let hz = self.lfo_hz();
                if value > 0.5 && !was {
                    // nearest note value to the free rate
                    let beats = (self.bpm as f32 / 60.0) / hz.max(1e-3);
                    let best = (2..=26).min_by(|a, b| {
                        let da = (sync_index_beats(-(*a as f32)) / beats).ln().abs();
                        let db_ = (sync_index_beats(-(*b as f32)) / beats).ln().abs();
                        da.total_cmp(&db_)
                    });
                    self.p.set(1017, hz);
                    self.p.set(25, -(best.unwrap_or(11) as f32));
                } else if value < 0.5 && was {
                    let back = if self.p.get(1017) > 0.01 { self.p.get(1017) } else { hz };
                    self.p.set(25, back.clamp(0.01, 100.0));
                }
            }
            _ => {}
        }
        if self.p.set(id, value) {
            self.update();
        }
    }
}

impl lg_dsp::Effect for AutoFilter {
    fn side_chain(&mut self, left: &[f32], right: &[f32]) {
        self.side = Some((left.to_vec(), right.to_vec()));
    }
    fn has_side_chain(&self) -> bool {
        true
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let side = self.side.take();
        let p = &self.p;
        let rate = self.rate;
        let base_oct = (p.get(2) / 100.0) * 1000f32.log2(); // octaves above 20 Hz
        let res = p.get(3) / 100.0;
        // (4.6: the ladder breaks into self-oscillation from about 93 % Resonance)
        let k = res * 4.6;
        let filt_on = p.on(1000);
        let svm = p.idx(1001);
        let env_on = p.on(1002);
        let env_retrig = p.on(1003);
        let dynamic = p.get(1004) / 100.0;
        let lfo_on = p.on(1006);
        let lfo_retrig = p.on(1008);
        let dist_on = p.on(1011);
        let (pre_m, post_m) = (p.idx(1012), p.idx(1013));
        let dry = p.get(1014) / 100.0;
        let out = db(p.get(1015));
        let thr_db = p.get(15);
        let fat = p.get(4) / 100.0;
        let poles = p.idx(5).min(3) + 1;
        let lfo_amt = p.get(12) / 100.0 * 5.0;
        let env_amt = p.get(16) / 100.0 * 6.0;
        let thr = db(p.get(15));
        let sustain = p.get(18) / 100.0;
        let spread = p.get(30) * 2.0;
        let (sync_ph, st_ph) = (p.get(26) / 360.0, p.get(27) / 360.0);
        let (din, dout) = if dist_on { (p.get(0) / 100.0, p.get(1) / 100.0) } else { (0.0, 0.0) };
        let lfo_amt = if lfo_on { lfo_amt } else { 0.0 };
        let env_amt = if env_on { env_amt } else { 0.0 };
        let hz = self.lfo_hz();
        let (att, dec, rel) = (self.att, self.dec, self.rel);
        let det_k = coef(rate, 30.0);
        for i in 0..left.len().min(right.len()) {
            // envelope: attack to 1 while the input is above threshold, then decay to sustain;
            // below threshold it releases to 0
            let lvl = match &side {
                Some((a, b)) => a.get(i).copied().unwrap_or(0.0).abs().max(b.get(i).copied().unwrap_or(0.0).abs()),
                None => left[i].abs().max(right[i].abs()),
            };
            self.det = flush(lvl.max(self.det * det_k));
            let lvl = self.det;
            if lvl > thr && !self.above {
                // a new trigger: with Retrigger off the envelope restarts only once it has
                // released fully; Dynamic scales the peak by how far the input is over threshold
                self.above = true;
                if env_retrig || self.env < 0.01 {
                    self.stage = 0.0;
                    let over = ((lg_dsp::gain_to_db(lvl) - thr_db) / (-thr_db).max(1.0)).clamp(0.0, 1.0);
                    self.peak = 1.0 - dynamic + dynamic * over;
                    if lfo_retrig {
                        self.lfo.p = 0.0;
                    }
                } else {
                    self.stage = 2.0; // ignored trigger: keep releasing
                }
            } else if lvl <= thr {
                self.above = false;
                self.stage = 2.0;
            }
            if self.stage < 0.5 {
                self.env = self.peak + (self.env - self.peak) * att;
                if self.env > self.peak * 0.98 {
                    self.stage = 1.0;
                }
            } else if self.stage < 1.5 {
                let s = sustain * self.peak;
                self.env = s + (self.env - s) * dec;
            } else {
                self.env = flush(self.env * rel);
            }
            let ph = self.lfo.step(rate, hz);
            let w = [self.wave(ph + sync_ph), self.wave(ph + sync_ph + st_ph)];
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let lp = w[s];
                let oct = base_oct + lfo_amt * lp + env_amt * self.env + if s == 1 { spread } else { -spread } * 0.5;
                let fc = (20.0 * 2f32.powf(oct)).clamp(20.0, rate * 0.45);
                let g = 1.0 - (-crate::util::TAU * fc / rate).exp();
                let x0 = *x;
                let mut v = x0;
                if din > 0.0 {
                    v = distort(pre_m, v, din);
                }
                let mut y = if !filt_on {
                    v
                } else if svm > 0 {
                    // State Variable (12 dB/oct, trapezoidal SVF): HP, BP (centred on the
                    // cutoff), LP, PK (a bell boost at the cutoff)
                    let kd = 2.0 - 1.98 * res;
                    let gg = (crate::util::PI * fc / rate).tan();
                    let [ic1, ic2] = &mut self.sv[s];
                    let a1 = 1.0 / (1.0 + gg * (gg + kd));
                    let v3 = v - *ic2;
                    let bp = a1 * *ic1 + gg * a1 * v3;
                    let lpo = *ic2 + gg * bp;
                    *ic1 = flush(2.0 * bp - *ic1);
                    *ic2 = flush(2.0 * lpo - *ic2);
                    let hp = v - kd * bp - lpo;
                    match svm {
                        1 => hp,
                        2 => kd * bp,
                        3 => lpo,
                        _ => v + kd * bp * (3.0 * res + 1.0),
                    }
                } else {
                    let st = &mut self.pole[s];
                    // resonance feedback from the fourth pole whatever the slope (the slope only
                    // picks the output tap); Fatness feeds the input back in step with it, so at
                    // full Fatness resonance no longer thins the band below the cutoff
                    let u = (v - k * (st[3] - fat * v)).tanh();
                    st[0] = flush(st[0] + g * (u - st[0]));
                    for j in 1..4 {
                        st[j] = flush(st[j] + g * (st[j - 1] - st[j]));
                    }
                    st[poles - 1]
                };
                if dout > 0.0 {
                    y = distort(post_m, y, dout);
                }
                *x = (y + dry * x0) * out;
            }
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        if bpm > 0.0 {
            self.bpm = bpm;
        }
    }
}

impl lg_dsp::Plugin for AutoFilter {
    fn params(&self) -> &[lg_dsp::Param] {
        self.p.defs
    }
    fn get(&self, id: u32) -> f32 {
        self.p.get(id)
    }
    fn set(&mut self, id: u32, value: f32) {
        self.edit(id, value);
    }
    fn reset(&mut self) {
        self.clear();
    }
    fn tail(&self) -> f32 {
        self.tail_secs()
    }
    fn load_floats(&mut self, floats: &[f32]) {
        for d in self.p.defs {
            if let Some(v) = floats.get(d.id as usize) {
                self.p.set(d.id, *v);
            }
        }
        self.update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    const R: f32 = 48000.0;

    fn sine(f: f32, a: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| a * (crate::util::TAU * f * i as f32 / R).sin()).collect()
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
    }

    fn run(set: &[(u32, f32)], x: &[f32]) -> Vec<f32> {
        let mut a = AutoFilter::new(R);
        // no modulation unless asked
        a.set(12, 0.0);
        for &(id, v) in set {
            a.set(id, v);
        }
        let (mut l, mut r) = (x.to_vec(), x.to_vec());
        for (cl, cr) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            a.process(cl, cr);
        }
        l
    }

    fn gain(set: &[(u32, f32)], f: f32) -> f32 {
        let y = run(set, &sine(f, 0.1, 24000));
        rms(&y[12000..]) / (0.1 / 2f32.sqrt())
    }

    #[test]
    fn has_logics_sections_and_switches() {
        let a = AutoFilter::new(R);
        let names: Vec<&str> = a.params().iter().map(|p| p.name).collect();
        for n in ["Filter On", "State Variable", "Envelope On", "Env Retrigger", "Dynamic", "Release", "LFO On", "LFO Sync", "LFO Retrigger", "LFO Waveform", "Pulse Width", "Distortion On", "Pre Mode", "Post Mode", "Dry Signal", "Main Out"] {
            assert!(names.contains(&n), "{n}");
        }
        assert_eq!(WAVES, &["Saw Down", "Saw Up", "Triangle", "Pulse", "Random"]);
        assert_eq!(DIST, &["Classic", "Tube", "Scream"]);
    }

    #[test]
    fn state_variable_modes() {
        // cutoff 50 % = about 630 Hz
        let c = [(2, 50.0), (3, 0.0)];
        let hp = [c[0], c[1], (1001, 1.0)];
        assert!(gain(&hp, 100.0) < 0.1 && gain(&hp, 5000.0) > 0.8);
        let lp = [c[0], c[1], (1001, 3.0)];
        assert!(gain(&lp, 100.0) > 0.8 && gain(&lp, 5000.0) < 0.1);
        // band-pass centred on the cutoff
        let bp = [c[0], c[1], (1001, 2.0)];
        let fc = 20.0 * 2f32.powf(0.5 * 1000f32.log2());
        assert!(gain(&bp, fc) > 0.9, "{}", gain(&bp, fc));
        assert!(gain(&bp, fc / 8.0) < 0.3 && gain(&bp, fc * 8.0) < 0.3);
        // peak: a boost at the cutoff, flat away from it
        let pk = [c[0], (3, 50.0), (1001, 4.0)];
        assert!(gain(&pk, fc) > 1.5 && (gain(&pk, 40.0) - 1.0).abs() < 0.2);
    }

    #[test]
    fn four_pole_button_overrides_state_variable() {
        let mut a = AutoFilter::new(R);
        a.set(1001, 1.0);
        assert_eq!(a.get(1001), 1.0);
        a.set(5, 1.0);
        assert_eq!(a.get(1001), 0.0);
        assert_eq!(a.get(5), 1.0);
    }

    #[test]
    fn section_switches() {
        // filter off: everything passes
        assert!(gain(&[(2, 30.0), (1000, 0.0)], 5000.0) > 0.95);
        // distortion switch off silences the drive
        let x = sine(220.0, 0.3, 24000);
        let on = run(&[(2, 100.0), (0, 100.0), (1000, 0.0)], &x);
        let off = run(&[(2, 100.0), (0, 100.0), (1000, 0.0), (1011, 0.0)], &x);
        assert!(off.iter().zip(&x).all(|(a, b)| (a - b).abs() < 1e-6));
        assert!(on.iter().zip(&x).any(|(a, b)| (a - b).abs() > 0.05));
        // Classic, Tube and Scream sound different
        let m: Vec<Vec<f32>> = (0..3).map(|k| run(&[(1000, 0.0), (0, 60.0), (1012, k as f32)], &x)).collect();
        assert!(rms(&m[0].iter().zip(&m[1]).map(|(a, b)| a - b).collect::<Vec<_>>()) > 0.005);
        assert!(rms(&m[0].iter().zip(&m[2]).map(|(a, b)| a - b).collect::<Vec<_>>()) > 0.005);
        // LFO off: a big LFO amount no longer moves the cutoff
        let still = run(&[(2, 40.0), (12, 100.0), (1006, 0.0)], &sine(1000.0, 0.1, 24000));
        let moving = run(&[(2, 40.0), (12, 100.0)], &sine(1000.0, 0.1, 24000));
        let var = |y: &[f32]| {
            let r: Vec<f32> = y[4800..].chunks(960).map(rms).collect();
            r.iter().cloned().fold(0.0, f32::max) - r.iter().cloned().fold(1.0, f32::min)
        };
        assert!(var(&still) < 0.002 && var(&moving) > 0.01, "{} {}", var(&still), var(&moving));
    }

    #[test]
    fn dry_signal_and_main_out() {
        // filter shut (cutoff 0): only the dry signal comes through
        let g = gain(&[(2, 0.0), (1014, 50.0)], 5000.0);
        assert!((g - 0.5).abs() < 0.05, "{g}");
        let g = gain(&[(1000, 0.0), (1015, -6.0)], 1000.0);
        assert!((g - 0.501).abs() < 0.02, "{g}");
    }

    #[test]
    fn full_fatness_keeps_the_low_end_under_resonance() {
        let thin = gain(&[(2, 60.0), (3, 80.0), (4, 0.0)], 40.0);
        let fat = gain(&[(2, 60.0), (3, 80.0), (4, 100.0)], 40.0);
        assert!(thin < 0.6 && (fat - 1.0).abs() < 0.1, "{thin} {fat}");
    }

    #[test]
    fn resonance_self_oscillates_below_maximum() {
        let mut x = vec![0.0; 48000];
        x[0] = 0.5;
        let y = run(&[(2, 50.0), (3, 97.0)], &x);
        assert!(rms(&y[40000..]) > 0.01, "{}", rms(&y[40000..]));
        let y = run(&[(2, 50.0), (3, 80.0)], &x);
        assert!(rms(&y[40000..]) < 1e-4);
    }

    /// The envelope's cutoff push over time for a burst input.
    fn env_trace(set: &[(u32, f32)], x: &[f32]) -> Vec<f32> {
        let mut a = AutoFilter::new(R);
        for &(id, v) in set {
            a.set(id, v);
        }
        let mut out = vec![];
        for c in x.chunks(48) {
            let (mut l, mut r) = (c.to_vec(), c.to_vec());
            a.process(&mut l, &mut r);
            out.push(a.env);
        }
        out
    }

    #[test]
    fn envelope_release_dynamic_and_retrigger() {
        let mut x = sine(200.0, 0.5, 9600);
        x.extend(vec![0.0; 24000]);
        // Release: slower release keeps the envelope up longer
        let fast = env_trace(&[(16, 50.0), (1005, 20.0), (17, 1.0)], &x);
        let slow = env_trace(&[(16, 50.0), (1005, 2000.0), (17, 1.0)], &x);
        // (the detector holds the peak about 50 ms before the level counts as gone)
        assert!(fast[200 + 150] < 0.01 && slow[200 + 150] > 0.3, "{} {}", fast[350], slow[350]);
        // Dynamic: a quiet input reaches a lower peak
        let quiet = sine(200.0, 0.05, 9600);
        let full = env_trace(&[(15, -40.0), (16, 50.0), (1004, 100.0), (17, 1.0), (18, 100.0)], &quiet);
        let flat = env_trace(&[(15, -40.0), (16, 50.0), (1004, 0.0), (17, 1.0), (18, 100.0)], &quiet);
        assert!(full[150] < 0.7 * flat[150], "{} {}", full[150], flat[150]);
        // Retrigger: a second burst while the envelope still releases restarts it only when on
        let mut two = sine(200.0, 0.5, 4800);
        two.extend(vec![0.0; 4800]);
        two.extend(sine(200.0, 0.5, 4800));
        let on = env_trace(&[(16, 50.0), (1005, 3000.0), (17, 1.0), (18, 50.0), (19, 20.0), (1003, 1.0)], &two);
        let off = env_trace(&[(16, 50.0), (1005, 3000.0), (17, 1.0), (18, 50.0), (19, 20.0), (1003, 0.0)], &two);
        assert!(on[205] > 0.9 && off[205] < 0.55, "{} {}", on[205], off[205]);
        // Envelope switch off: no push at all
        let mut a = AutoFilter::new(R);
        a.set(16, 100.0);
        a.set(1002, 0.0);
        let mut y = sine(1000.0, 0.5, 24000);
        let mut b = y.clone();
        a.set(12, 0.0);
        a.set(2, 40.0);
        a.process(&mut y, &mut b);
        let r = rms(&y[12000..]);
        let base = gain(&[(2, 40.0)], 1000.0) * 0.5 / 2f32.sqrt();
        assert!((r - base).abs() < 0.2 * base, "{r} {base}");
    }

    #[test]
    fn lfo_waveforms_sync_and_retrigger() {
        let mut a = AutoFilter::new(R);
        let mut at = |w: f32, ph: f32| {
            a.p.set(1009, w);
            a.wave(ph)
        };
        assert!((at(0.0, 0.1) - 0.8).abs() < 1e-5); // saw down
        assert!((at(1.0, 0.1) + 0.8).abs() < 1e-5); // saw up
        assert!((at(2.0, 0.25) - 1.0).abs() < 1e-5); // triangle peak
        // pulse width moves the edge
        a.p.set(1009, 3.0);
        a.p.set(1010, 25.0);
        assert_eq!(a.wave(0.2), 1.0);
        assert_eq!(a.wave(0.3), -1.0);
        a.p.set(1010, 75.0);
        assert_eq!(a.wave(0.7), 1.0);
        // random: held values, new one each cycle
        a.p.set(1009, 4.0);
        let v1 = a.wave(0.1);
        assert_eq!(a.wave(0.5), v1);
        let v2 = a.wave(0.05);
        assert_ne!(v1, v2);
        // Sync button: off gives a free rate in Hz, on a note value (bars / triplets reachable)
        let mut a = AutoFilter::new(R);
        a.set_tempo(120.0);
        assert!(a.get(25) < 0.0);
        a.set(1007, 0.0);
        assert!(a.get(25) > 0.0);
        let hz = a.lfo_hz();
        assert!((hz - 4.0).abs() < 0.01, "{hz}"); // 1/8 at 120 bpm
        a.set(25, 2.0);
        a.set(1007, 1.0);
        assert!(a.get(25) < 0.0 && (a.lfo_hz() - 2.0).abs() < 0.01, "{}", a.lfo_hz());
        assert!((sync_index_beats(-26.0) - 16.0).abs() < 1e-6 && (sync_index_beats(-12.0) - 2.0 / 3.0).abs() < 1e-6);
        // LFO Retrigger: a trigger resets the LFO phase
        let mut a = AutoFilter::new(R);
        a.set(1008, 1.0);
        a.set(16, 10.0);
        let mut z = vec![0.0; 4000];
        let mut zz = z.clone();
        a.process(&mut z, &mut zz);
        assert!(a.lfo.p > 0.01);
        let mut s = vec![0.9; 10];
        let mut ss = s.clone();
        a.process(&mut s, &mut ss);
        assert!(a.lfo.p < 0.01, "{}", a.lfo.p);
    }
}
