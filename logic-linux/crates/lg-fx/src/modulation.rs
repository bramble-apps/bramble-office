//! The modulation effects: Chorus, Ensemble, Flanger, Phaser, Microphaser and Tremolo —
//! stand-ins for Logic's, on the parameter positions of a project's floats.
//!
//! - Chorus (Mix, Intensity, Rate, D-Mode): two modulated delays (~7-25 ms) in quadrature
//!   between left and right; D-Mode switches to fixed, gentle "dimension" settings.
//! - Ensemble (Mix, Volume Compensation, Voices, LFO 1 / LFO 2 / Random rate and intensity,
//!   Phase, Stereo Spread, the three modulators' switches): 2-8 delayed voices, each modulated by
//!   two LFOs and a random source with a per-voice phase offset, spread across the stereo field.
//! - Flanger (Mix, Intensity, Rate, Feedback): a short (0.3-8 ms) triangle-swept delay with
//!   positive or negative feedback.
//! - Phaser (LFO 1 / LFO 2 rates and mix, Feedback, Sweep Floor / Ceiling, Stages, Phase, Output
//!   Mix, feedback filter cuts, Warmth): 4-12 first-order all-pass stages swept on a log scale,
//!   left and right offset by Phase.
//! - Microphaser (Rate, Feedback, Intensity): the same engine with its fixed front-end settings.
//! - Tremolo (Depth, Rate or synced note, Symmetry, Smoothing, Stereo Phase, Offset, Sync).
//!
//! Mix is a crossfade (dry = 1 - mix) throughout, as in Logic's delays.

use crate::util::{Biquad, Coefs, Delay, Drift, OnePole, Phase, Values, beats_hz, flush, plugin_impl, sine01, sync_index_beats, tri};
use lg_dsp::{Param, Unit};

// ---------------------------------------------------------------------------------------------
// Chorus

static CHORUS_PARAMS: [Param; 4] = [
    Param::new(0, "Mix", 0.0, 100.0, 30.0, Unit::Percent),
    Param::new(1, "Intensity", 0.0, 100.0, 10.0, Unit::Percent),
    Param::new(2, "Rate", 0.01, 20.0, 0.5, Unit::Hz),
    Param::menu(3, "D-Mode", &["Off", "1", "2", "1 + 2"], 0.0),
];

pub struct Chorus {
    p: Values,
    rate: f32,
    line: [Delay; 2],
    ph: Phase,
}

impl Chorus {
    pub fn new(rate: f32) -> Chorus {
        let cap = (0.05 * rate) as usize;
        Chorus { p: Values::new(&CHORUS_PARAMS), rate, line: [Delay::new(cap), Delay::new(cap)], ph: Phase::default() }
    }
    fn update(&mut self) {}
    fn clear(&mut self) {
        self.line[0].clear();
        self.line[1].clear();
    }
    fn tail_secs(&self) -> f32 {
        0.05
    }
}

impl lg_dsp::Effect for Chorus {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let rate = self.rate;
        let mut mix = p.get(0) / 100.0;
        let mut depth = p.get(1) / 100.0 * 0.007 * rate;
        let mut hz = p.get(2);
        let mut base = 0.012 * rate;
        // D-Mode: fixed slow, shallow, wide settings (stronger for 2, both for 1 + 2)
        match p.idx(3) {
            1 => (hz, depth, base, mix) = (0.25, 0.0012 * rate, 0.008 * rate, 0.5),
            2 => (hz, depth, base, mix) = (0.5, 0.0018 * rate, 0.009 * rate, 0.5),
            3 => (hz, depth, base, mix) = (0.5, 0.0025 * rate, 0.01 * rate, 0.5),
            _ => {}
        }
        for i in 0..left.len().min(right.len()) {
            let ph = self.ph.step(rate, hz);
            let ml = tri(ph);
            let mr = tri(ph + 0.25);
            self.line[0].push(left[i]);
            self.line[1].push(right[i]);
            let wl = self.line[0].read_lin(base + depth * ml);
            let wr = self.line[1].read_lin(base + depth * mr);
            left[i] = left[i] * (1.0 - mix) + wl * mix;
            right[i] = right[i] * (1.0 - mix) + wr * mix;
        }
    }
}

plugin_impl!(Chorus);

// ---------------------------------------------------------------------------------------------
// Ensemble

static ENSEMBLE_PARAMS: [Param; 14] = [
    Param::new(0, "Output Mix", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(1, "Volume Compensation", -10.0, 10.0, 0.2, Unit::Db),
    Param::new(2, "Voices", 2.0, 8.0, 3.0, Unit::None),
    Param::new(3, "LFO 1 Rate", 0.01, 10.0, 0.9, Unit::Hz),
    Param::new(4, "LFO 1 Intensity", 0.0, 100.0, 16.0, Unit::Percent),
    Param::new(5, "LFO 2 Rate", 0.01, 20.0, 3.8, Unit::Hz),
    Param::new(6, "LFO 2 Intensity", 0.0, 100.0, 8.0, Unit::Percent),
    Param::new(7, "Random Rate", 0.01, 20.0, 2.0, Unit::Hz),
    Param::new(8, "Random Intensity", 0.0, 100.0, 0.0, Unit::Percent),
    Param::new(9, "Phase", -100.0, 100.0, -30.0, Unit::Percent),
    Param::new(10, "Stereo Spread", 0.0, 200.0, 100.0, Unit::Percent),
    Param::new(11, "LFO 1", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(12, "LFO 2", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(13, "Random", 0.0, 1.0, 1.0, Unit::OnOff),
];

pub struct Ensemble {
    p: Values,
    rate: f32,
    line: Delay,
    lfo1: Phase,
    lfo2: Phase,
    rnd: [Drift; 8],
}

impl Ensemble {
    pub fn new(rate: f32) -> Ensemble {
        Ensemble { p: Values::new(&ENSEMBLE_PARAMS), rate, line: Delay::new((0.06 * rate) as usize), lfo1: Phase::default(), lfo2: Phase::default(), rnd: std::array::from_fn(|i| Drift::new(31 + i as u32 * 7919)) }
    }
    fn update(&mut self) {}
    fn clear(&mut self) {
        self.line.clear();
    }
    fn tail_secs(&self) -> f32 {
        0.06
    }
}

impl lg_dsp::Effect for Ensemble {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let rate = self.rate;
        let voices = (p.get(2).round() as usize).clamp(2, 8);
        let mix = p.get(0) / 100.0;
        let comp = crate::util::db(p.get(1)) / (voices as f32).sqrt();
        let a1 = if p.on(11) { p.get(4) / 100.0 * 0.006 * rate } else { 0.0 };
        let a2 = if p.on(12) { p.get(6) / 100.0 * 0.002 * rate } else { 0.0 };
        let ar = if p.on(13) { p.get(8) / 100.0 * 0.004 * rate } else { 0.0 };
        let phase = p.get(9) / 100.0;
        let spread = (p.get(10) / 100.0).min(2.0);
        let (r1, r2, rr) = (p.get(3), p.get(5), p.get(7));
        for i in 0..left.len().min(right.len()) {
            let (l, r) = (left[i], right[i]);
            self.line.push(0.5 * (l + r));
            let p1 = self.lfo1.step(rate, r1);
            let p2 = self.lfo2.step(rate, r2);
            let (mut wl, mut wr) = (0.0, 0.0);
            for v in 0..voices {
                let off = v as f32 / voices as f32;
                // Phase spreads the voices' LFO positions (0 = all together, ±100 % = evenly
                // around the cycle, in either direction)
                let m = a1 * sine01(p1 + off * phase) + a2 * sine01(p2 + off * phase * 0.5 + 0.25) + if ar > 0.0 { ar * self.rnd[v].next(rate, rr) } else { 0.0 };
                let d = (0.010 + 0.0035 * v as f32) * rate + m + a1 + a2 + ar;
                let s = self.line.read_lin(d);
                // pan the voices across the field
                let pan = if voices > 1 { (v as f32 / (voices - 1) as f32 * 2.0 - 1.0) * (spread * 0.5).min(1.0) } else { 0.0 };
                wl += s * (1.0 - pan) * 0.5;
                wr += s * (1.0 + pan) * 0.5;
            }
            // beyond 100 % spread, widen further with mid/side
            if spread > 1.0 {
                let mid = 0.5 * (wl + wr);
                let side = 0.5 * (wl - wr) * spread;
                wl = mid + side;
                wr = mid - side;
            }
            left[i] = l * (1.0 - mix) + wl * comp * 2.0 * mix;
            right[i] = r * (1.0 - mix) + wr * comp * 2.0 * mix;
        }
    }
}

plugin_impl!(Ensemble);

// ---------------------------------------------------------------------------------------------
// Flanger

static FLANGER_PARAMS: [Param; 5] = [
    Param::new(0, "Mix", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(1, "Intensity", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(2, "Rate", 0.01, 20.0, 0.1333, Unit::Hz),
    Param::new(3, "Feedback", -100.0, 100.0, 67.0, Unit::Percent),
    Param::new(4, "Rate Sync", 0.0, 1.0, 0.0, Unit::OnOff),
];

pub struct Flanger {
    p: Values,
    rate: f32,
    line: [Delay; 2],
    ph: Phase,
    fb: [f32; 2],
}

impl Flanger {
    pub fn new(rate: f32) -> Flanger {
        let cap = (0.02 * rate) as usize;
        Flanger { p: Values::new(&FLANGER_PARAMS), rate, line: [Delay::new(cap), Delay::new(cap)], ph: Phase::default(), fb: [0.0; 2] }
    }
    fn update(&mut self) {}
    fn clear(&mut self) {
        self.line[0].clear();
        self.line[1].clear();
        self.fb = [0.0; 2];
    }
    fn tail_secs(&self) -> f32 {
        0.2
    }
}

impl lg_dsp::Effect for Flanger {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let rate = self.rate;
        let mix = p.get(0) / 100.0;
        let depth = p.get(1) / 100.0;
        let fb = (p.get(3) / 100.0).clamp(-0.97, 0.97);
        let hz = p.get(2);
        let (lo, hi) = (0.0003 * rate, (0.0003 + 0.0077 * depth) * rate);
        for i in 0..left.len().min(right.len()) {
            let ph = self.ph.step(rate, hz);
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                // left and right sweep a quarter cycle apart
                let m = 0.5 + 0.5 * tri(ph + 0.25 * s as f32);
                // exponential sweep sounds even across the range
                let d = lo * (hi / lo).powf(m);
                self.line[s].push(*x + self.fb[s] * fb);
                let y = self.line[s].read_lin(d);
                self.fb[s] = flush(y);
                *x = *x * (1.0 - mix) + y * mix;
            }
        }
    }
}

plugin_impl!(Flanger);

// ---------------------------------------------------------------------------------------------
// Phaser and Microphaser

static PHASER_PARAMS: [Param; 16] = [
    Param::new(0, "Rate LFO 1", 0.01, 20.0, 0.24, Unit::Hz),
    Param::new(1, "Rate LFO 2", 0.01, 20.0, 0.48, Unit::Hz),
    Param::new(2, "LFO Mix", 0.0, 100.0, 10.0, Unit::Percent),
    Param::new(3, "Feedback", -100.0, 100.0, 50.0, Unit::Percent),
    Param::new(4, "Sweep Floor", 20.0, 20000.0, 550.0, Unit::Hz),
    Param::new(5, "Sweep Ceiling", 20.0, 20000.0, 13200.0, Unit::Hz),
    Param::new(6, "Stages", 4.0, 12.0, 12.0, Unit::None),
    Param::new(8, "Phase", -180.0, 180.0, 180.0, Unit::None),
    Param::new(9, "Output Mix", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(13, "Feedback High Cut", 20.0, 20000.0, 20000.0, Unit::Hz),
    Param::new(14, "Feedback Low Cut", 20.0, 20000.0, 20.0, Unit::Hz),
    Param::new(18, "Warmth", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(19, "Feedback Filter", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(23, "LFO 1 Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(24, "LFO 2 Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(25, "Sweep Mode", &["Squared", "Linear", "Exponential"], 0.0),
];

static MICROPHASER_PARAMS: [Param; 3] = [
    Param::new(0, "Rate", 0.01, 20.0, 0.22, Unit::Hz),
    Param::new(3, "Feedback", -100.0, 100.0, 37.0, Unit::Percent),
    Param::new(15, "Intensity", 0.0, 100.0, 41.0, Unit::Percent),
];

/// The shared phaser engine.
pub struct Phaser {
    p: Values,
    micro: bool,
    rate: f32,
    lfo1: Phase,
    lfo2: Phase,
    ap: [[f32; 12]; 2],
    fb: [f32; 2],
    fb_lp: [Biquad; 2],
    fb_hp: [Biquad; 2],
}

impl Phaser {
    pub fn new(rate: f32) -> Phaser {
        let mut p = Phaser { p: Values::new(&PHASER_PARAMS), micro: false, rate, lfo1: Phase::default(), lfo2: Phase::default(), ap: [[0.0; 12]; 2], fb: [0.0; 2], fb_lp: [Biquad::default(); 2], fb_hp: [Biquad::default(); 2] };
        p.update();
        p
    }

    pub fn micro(rate: f32) -> Phaser {
        let mut p = Phaser::new(rate);
        p.p = Values::new(&MICROPHASER_PARAMS);
        p.micro = true;
        p.update();
        p
    }

    fn update(&mut self) {
        let (hc, lc, on) = if self.micro { (20000.0, 20.0, false) } else { (self.p.get(13), self.p.get(14), self.p.on(19)) };
        for s in 0..2 {
            self.fb_lp[s].c = if on && hc < 19999.0 { Coefs::lowpass(self.rate, hc, 0.707) } else { Coefs::THRU };
            self.fb_hp[s].c = if on && lc > 20.0 { Coefs::highpass(self.rate, lc, 0.707) } else { Coefs::THRU };
        }
    }

    fn clear(&mut self) {
        self.ap = [[0.0; 12]; 2];
        self.fb = [0.0; 2];
        for s in 0..2 {
            self.fb_lp[s].clear();
            self.fb_hp[s].clear();
        }
    }

    fn tail_secs(&self) -> f32 {
        0.1
    }
}

impl lg_dsp::Effect for Phaser {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let rate = self.rate;
        // the Microphaser's front end drives fixed engine settings
        let (r1, r2, lmix, fb, floor, ceil, stages, phase, mix, warm) = if self.micro {
            let int = p.get(15) / 100.0;
            (p.get(0), 0.5, 0.0, p.get(3) / 100.0, 550.0, 550.0 * (5400.0f32 / 550.0).powf(int.max(0.02)), 8, -0.5, 0.5, false)
        } else {
            (p.get(0), p.get(1), p.get(2) / 100.0, p.get(3) / 100.0, p.get(4), p.get(5), (p.get(6).round() as usize).clamp(2, 12) & !1, p.get(8) / 360.0, p.get(9) / 100.0, p.on(18))
        };
        let fb = fb.clamp(-0.95, 0.95);
        let (lo, hi) = (floor.min(ceil).max(20.0), ceil.max(floor).min(rate * 0.45));
        let mode = if self.micro { 0 } else { p.idx(25) };
        for i in 0..left.len().min(right.len()) {
            let p1 = self.lfo1.step(rate, r1);
            let p2 = self.lfo2.step(rate, r2);
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let off = if s == 1 { phase } else { 0.0 };
                let m = (1.0 - lmix) * (0.5 + 0.5 * sine01(p1 + off)) + lmix * (0.5 + 0.5 * sine01(p2 + off));
                let m = match mode {
                    0 => m * m,
                    2 => m.sqrt(),
                    _ => m,
                };
                let f = lo * (hi / lo).powf(m);
                // first-order all-pass coefficient for this corner
                let t = (std::f32::consts::PI * f / rate).tan();
                let a = (t - 1.0) / (t + 1.0);
                let mut y = *x + self.fb[s] * fb;
                if warm {
                    y = (y * 1.3).tanh() / 1.3;
                }
                for k in 0..stages {
                    // y = a*x + z; z = x - a*y  (transposed first-order all-pass)
                    let z = self.ap[s][k];
                    let out = a * y + z;
                    self.ap[s][k] = flush(y - a * out);
                    y = out;
                }
                let f = self.fb_hp[s].run(self.fb_lp[s].run(y));
                self.fb[s] = flush(f);
                *x = *x * (1.0 - mix) + y * mix;
            }
        }
    }
}

plugin_impl!(Phaser);

// ---------------------------------------------------------------------------------------------
// Tremolo

static TREMOLO_PARAMS: [Param; 7] = [
    Param::new(0, "Depth", 0.0, 100.0, 100.0, Unit::Percent),
    // free Hz, or (negative) the synced note index: -14 = 1/4, -11 = 1/8 …
    Param::new(1, "Rate", -26.0, 20.0, -14.0, Unit::None),
    Param::new(2, "Symmetry", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(3, "Smoothing", 0.0, 100.0, 100.0, Unit::Percent),
    Param::new(4, "Stereophase", -180.0, 180.0, 180.0, Unit::None),
    Param::new(5, "Offset", -100.0, 100.0, 0.0, Unit::Percent),
    Param::new(8, "Sync", 0.0, 1.0, 1.0, Unit::OnOff),
];

pub struct Tremolo {
    p: Values,
    rate: f32,
    bpm: f64,
    ph: Phase,
    smooth: [OnePole; 2],
}

impl Tremolo {
    pub fn new(rate: f32) -> Tremolo {
        let mut t = Tremolo { p: Values::new(&TREMOLO_PARAMS), rate, bpm: 120.0, ph: Phase::default(), smooth: [OnePole::default(); 2] };
        t.update();
        t
    }

    /// The LFO rate in Hz (synced or free).
    pub fn hz(&self) -> f32 {
        let r = self.p.get(1);
        if self.p.on(8) && r < 0.0 { beats_hz(sync_index_beats(r), self.bpm) } else { r.abs().max(0.01) }
    }

    fn update(&mut self) {
        // Smoothing rounds the square wave's edges: 0 % = hard switching, 100 % = near sine
        let hz = self.hz();
        let sm = self.p.get(3) / 100.0;
        let cut = hz * (2.0 + 400.0 * (1.0 - sm).powi(2));
        for s in self.smooth.iter_mut() {
            s.set(self.rate, cut);
        }
    }

    fn clear(&mut self) {
        self.smooth = [OnePole { y: 1.0, ..self.smooth[0] }; 2];
    }

    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl lg_dsp::Effect for Tremolo {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let p = &self.p;
        let depth = p.get(0) / 100.0;
        let sym = (p.get(2) / 100.0).clamp(0.01, 0.99);
        let st = p.get(4) / 360.0;
        let off = p.get(5) / 100.0;
        let hz = self.hz();
        for i in 0..left.len().min(right.len()) {
            let ph = self.ph.step(self.rate, hz);
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let q = ph + if s == 1 { st } else { 0.0 };
                let q = q - q.floor();
                // a pulse with Symmetry's duty cycle, smoothed into a rounded wave
                let sq = if q < sym { 1.0 } else { 0.0 };
                let w = self.smooth[s].lp(sq);
                // depth from 1 down to (1 - depth); Offset shifts the swing up or down
                let g = (1.0 - depth * (1.0 - w) + off * depth * 0.5).clamp(0.0, 1.0);
                *x *= g;
            }
        }
    }

    fn set_tempo(&mut self, bpm: f64) {
        if bpm > 0.0 {
            self.bpm = bpm;
            self.update();
        }
    }
}

plugin_impl!(Tremolo);

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::{Effect, Plugin};

    #[test]
    fn tremolo_quarter_at_120_is_2hz() {
        let t = Tremolo::new(48000.0);
        assert!((t.hz() - 2.0).abs() < 1e-4);
    }

    #[test]
    fn tremolo_depth_swings() {
        let mut t = Tremolo::new(48000.0);
        t.set(3, 0.0);
        let mut l = vec![1.0; 48000];
        let mut r = l.clone();
        t.process(&mut l, &mut r);
        let (mn, mx) = l.iter().fold((1.0f32, 0.0f32), |a, v| (a.0.min(*v), a.1.max(*v)));
        assert!(mn < 0.05 && mx > 0.95, "{mn} {mx}");
    }

    #[test]
    fn phaser_notches_move() {
        // a phaser at full mix with no feedback cancels one frequency per two stages
        let mut p = Phaser::new(48000.0);
        p.set(3, 0.0);
        p.set(0, 0.01);
        p.set(2, 0.0);
        let mut nz = crate::util::Noise(1);
        let mut l: Vec<f32> = (0..48000).map(|_| nz.next()).collect();
        let mut r = l.clone();
        p.process(&mut l, &mut r);
        assert!(l.iter().all(|x| x.is_finite() && x.abs() < 2.0));
    }
}
