//! EnVerb, SilverVerb and Quantec Room Simulator, with the guide's controls.
//!
//! - EnVerb: a reverb whose tail follows an envelope you draw (Attack, Decay, Sustain, Hold,
//!   Release), with Dry Signal Delay, Predelay, Density, Spread, High Cut, Crossover and Low Freq
//!   Level: built as a synthesised impulse response with that envelope, convolved (zero latency).
//! - SilverVerb: Predelay, Reflectivity, Size, Density/Time, Low and High Cut, a modulation LFO
//!   (Rate, Phase, Intensity), Dry / Wet — a feedback delay network.
//! - Quantec Room Simulator: the QRS and YardStick models; Reverb Time with Room Size (1 m³ to
//!   1,000,000 m³), Low / High time multipliers (YardStick: with their crossovers), Reverb Delay,
//!   Dry and Reverb levels, Freeze (Add, Clear), Enhance (QRS), the AD/DA converter drive (QRS),
//!   the YardStick's Mode (Complex / Medium / Simple), Density, High Cut, Bass Gain and Crossover,
//!   Correlation, Dry Source (L R / R L) and Subsonic.
//!
//! Ids are our own from 1000 (the blocks aren't decoded).

use crate::convolve::Convolver;
use crate::kit::{Crossover, Fdn};
use crate::util::{Biquad, Coefs, Delay, Noise, OnePole, Phase, Values, db, plugin_impl, sine01};
use lg_dsp::{Effect, Param, Unit};

const B: u32 = 1000;

// ---------------------------------------------------------------------------------------------
// EnVerb

static EV_P: [Param; 14] = [
    Param::new(B, "Attack", 0.0, 2000.0, 20.0, Unit::Ms),
    Param::new(B + 1, "Decay", 0.0, 4000.0, 300.0, Unit::Ms),
    Param::new(B + 2, "Sustain", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 3, "Hold", 0.0, 4000.0, 300.0, Unit::Ms),
    Param::new(B + 4, "Release", 10.0, 4000.0, 800.0, Unit::Ms),
    Param::new(B + 5, "Dry Signal Delay", 0.0, 200.0, 0.0, Unit::Ms),
    Param::new(B + 6, "Predelay", 0.0, 200.0, 10.0, Unit::Ms),
    Param::new(B + 7, "Density", 0.0, 100.0, 80.0, Unit::Percent),
    Param::new(B + 8, "Spread", 0.0, 200.0, 100.0, Unit::Percent),
    Param::new(B + 9, "High Cut", 500.0, 20000.0, 9000.0, Unit::Hz),
    Param::new(B + 10, "Crossover", 50.0, 2000.0, 300.0, Unit::Hz),
    Param::new(B + 11, "Low Freq Level", -24.0, 6.0, -3.0, Unit::Db),
    Param::new(B + 12, "Dry", -96.0, 6.0, 0.0, Unit::Db),
    Param::new(B + 13, "Wet", -96.0, 6.0, -10.0, Unit::Db),
];

const EV_MAX: f32 = 6.0;

pub struct EnVerb {
    p: Values,
    rate: f32,
    conv: [Convolver; 2],
    ir: [Vec<f32>; 2],
    n: usize,
    dry_line: [Delay; 2],
}

impl EnVerb {
    pub fn new(rate: f32) -> EnVerb {
        let max = (EV_MAX * rate) as usize;
        let mut e = EnVerb { p: Values::new(&EV_P), rate, conv: [Convolver::new(max), Convolver::new(max)], ir: [vec![0.0; max], vec![0.0; max]], n: 0, dry_line: [Delay::new((0.21 * rate) as usize), Delay::new((0.21 * rate) as usize)] };
        e.update();
        e
    }

    /// The envelope's level at time t (s): Attack up, Decay to Sustain, Hold, Release to zero
    /// (after the Predelay).
    pub fn envelope(&self, t: f32) -> f32 {
        let g = |id| self.p.get(id) * 0.001;
        let (a, d, s, h, r) = (g(B), g(B + 1), self.p.get(B + 2) / 100.0, g(B + 3), g(B + 4));
        if t < a {
            t / a.max(1e-4)
        } else if t < a + d {
            1.0 - (1.0 - s) * (t - a) / d.max(1e-4)
        } else if t < a + d + h {
            s
        } else if t < a + d + h + r {
            s * (1.0 - (t - a - d - h) / r)
        } else {
            0.0
        }
    }

    fn update(&mut self) {
        let rate = self.rate;
        let total = (self.p.get(B) + self.p.get(B + 1) + self.p.get(B + 3) + self.p.get(B + 4)) * 0.001;
        let pre = (self.p.get(B + 6) * 0.001 * rate) as usize;
        let n = ((total * rate) as usize + pre).min(self.ir[0].len());
        self.n = n;
        let dens = self.p.get(B + 7) / 100.0;
        let spread = (self.p.get(B + 8) / 100.0).min(2.0);
        let mut nz = [Noise(0x1357_9bdf), Noise(0x2468_ace1), Noise(0x0f1e_2d3c)];
        let mut hc = [OnePole::new(rate, self.p.get(B + 9)); 2];
        let mut xo = [Crossover::new(rate, self.p.get(B + 10)); 2];
        let low = db(self.p.get(B + 11));
        let sparse = (dens * dens).max(0.01);
        for i in 0..n {
            let t = i.saturating_sub(pre) as f32 / rate;
            let env = if i < pre { 0.0 } else { self.envelope(t) };
            let keep = (nz[2].next() * 0.5 + 0.5) < sparse;
            let common = nz[2].next();
            for c in 0..2 {
                let own = nz[c].next();
                // Spread: 0 = mono, 1 = independent, 2 = anti-correlated widening
                let x = if keep { own * spread.min(1.0) + common * (1.0 - spread.min(1.0)) - if spread > 1.0 { (spread - 1.0) * common * if c == 0 { 1.0 } else { -1.0 } } else { 0.0 } } else { 0.0 } / sparse.sqrt();
                let x = hc[c].lp(x * env);
                let (l, h) = xo[c].run(x);
                self.ir[c][i] = l * low + h;
            }
        }
        for c in 0..2 {
            self.ir[c][n..].iter_mut().for_each(|v| *v = 0.0);
        }
        let e: f64 = self.ir.iter().map(|ch| ch[..n].iter().map(|v| (*v as f64).powi(2)).sum::<f64>()).sum::<f64>() / 2.0;
        let g = (1.2 / e.max(1e-20).sqrt()) as f32;
        for c in 0..2 {
            self.ir[c][..n].iter_mut().for_each(|v| *v *= g);
            self.conv[c].load(&self.ir[c][..n.max(1)]);
        }
    }
    fn clear(&mut self) {
        for c in self.conv.iter_mut() {
            c.clear();
        }
        self.dry_line[0].clear();
        self.dry_line[1].clear();
    }
    fn tail_secs(&self) -> f32 {
        self.n as f32 / self.rate
    }
}

impl Effect for EnVerb {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let dd = self.p.get(B + 5) * 0.001 * self.rate;
        let dry = if self.p.get(B + 12) <= -95.9 { 0.0 } else { db(self.p.get(B + 12)) };
        let wet = if self.p.get(B + 13) <= -95.9 { 0.0 } else { db(self.p.get(B + 13)) };
        for i in 0..left.len().min(right.len()) {
            for (s, x) in [&mut left[i], &mut right[i]].into_iter().enumerate() {
                let w = self.conv[s].run(*x);
                self.dry_line[s].push(*x);
                let d = if dd >= 1.0 { self.dry_line[s].read_lin(dd) } else { *x };
                *x = d * dry + w * wet;
            }
        }
    }
}
plugin_impl!(EnVerb);

// ---------------------------------------------------------------------------------------------
// SilverVerb

static SV_P: [Param; 11] = [
    Param::new(B, "Predelay", 0.0, 200.0, 10.0, Unit::Ms),
    Param::new(B + 1, "Reflectivity", 0.0, 100.0, 60.0, Unit::Percent),
    Param::new(B + 2, "Room Size", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 3, "Density/Time", 0.0, 100.0, 50.0, Unit::Percent),
    Param::new(B + 4, "Low Cut", 20.0, 2000.0, 100.0, Unit::Hz),
    Param::new(B + 5, "High Cut", 500.0, 20000.0, 8000.0, Unit::Hz),
    Param::new(B + 6, "Modulation", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(B + 7, "Rate", 0.01, 5.0, 0.4, Unit::Hz),
    Param::new(B + 8, "Phase", 0.0, 360.0, 90.0, Unit::None),
    Param::new(B + 9, "Intensity", 0.0, 100.0, 20.0, Unit::Percent),
    Param::new(B + 10, "Dry/Wet", 0.0, 100.0, 30.0, Unit::Percent),
];

pub struct SilverVerb {
    p: Values,
    rate: f32,
    fdn: Fdn,
    lc: [Biquad; 2],
    hc: [Biquad; 2],
    lfo: Phase,
    mod_line: [Delay; 2],
}

impl SilverVerb {
    pub fn new(rate: f32) -> SilverVerb {
        let mut s = SilverVerb { p: Values::new(&SV_P), rate, fdn: Fdn::new(rate), lc: [Biquad::default(); 2], hc: [Biquad::default(); 2], lfo: Phase::default(), mod_line: [Delay::new((0.02 * rate) as usize), Delay::new((0.02 * rate) as usize)] };
        s.update();
        s
    }
    fn update(&mut self) {
        let size = 0.4 + 1.4 * self.p.get(B + 2) / 100.0;
        let time = 0.3 + 6.0 * (self.p.get(B + 3) / 100.0).powi(2) * (0.4 + 0.6 * self.p.get(B + 1) / 100.0);
        let damp = 2000.0 + 14000.0 * self.p.get(B + 1) / 100.0;
        self.fdn.set(time, size, damp);
        self.fdn.predelay = self.p.get(B) * 0.001;
        for s in 0..2 {
            self.lc[s].c = Coefs::highpass(self.rate, self.p.get(B + 4), 0.7);
            self.hc[s].c = Coefs::lowpass(self.rate, self.p.get(B + 5).min(self.rate * 0.45), 0.7);
        }
    }
    fn clear(&mut self) {
        self.fdn.clear();
    }
    fn tail_secs(&self) -> f32 {
        6.0
    }
}

impl Effect for SilverVerb {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let mix = self.p.get(B + 10) / 100.0;
        let on = self.p.on(B + 6);
        let ph_off = self.p.get(B + 8) / 360.0;
        let int = self.p.get(B + 9) / 100.0;
        for i in 0..left.len().min(right.len()) {
            let (mut a, mut b) = self.fdn.run(left[i], right[i]);
            a = self.hc[0].run(self.lc[0].run(a));
            b = self.hc[1].run(self.lc[1].run(b));
            if on && int > 0.0 {
                let ph = self.lfo.step(self.rate, self.p.get(B + 7));
                for (s, v) in [&mut a, &mut b].into_iter().enumerate() {
                    let m = sine01(ph + ph_off * s as f32);
                    self.mod_line[s].push(*v);
                    *v = self.mod_line[s].read_lin(1.0 + 0.004 * self.rate * int * (0.5 + 0.5 * m));
                }
            }
            left[i] = left[i] * (1.0 - mix) + a * mix * 0.5;
            right[i] = right[i] * (1.0 - mix) + b * mix * 0.5;
        }
    }
}
plugin_impl!(SilverVerb);

// ---------------------------------------------------------------------------------------------
// Quantec Room Simulator

static QRS_P: [Param; 24] = [
    Param::menu(B, "Model", &["QRS", "YardStick"], 1.0),
    Param::new(B + 1, "Reverb Time", 0.1, 100.0, 2.0, Unit::Seconds),
    Param::new(B + 2, "Freeze", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 3, "Add", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 4, "Clear", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 5, "Enhance", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 6, "Low", 0.25, 4.0, 1.0, Unit::None),
    Param::new(B + 7, "High", 0.25, 4.0, 0.6, Unit::None),
    Param::new(B + 8, "Room Size", 1.0, 1_000_000.0, 2000.0, Unit::None),
    Param::new(B + 9, "Reverb Delay", 0.0, 200.0, 20.0, Unit::Ms),
    Param::new(B + 10, "Dry Level", -96.0, 6.0, 0.0, Unit::Db),
    Param::new(B + 11, "Reverb Level", -96.0, 6.0, -8.0, Unit::Db),
    Param::new(B + 12, "AD/DA Converters", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(B + 13, "AD/DA Drive", 0.0, 30.0, 0.0, Unit::Db),
    Param::menu(B + 14, "Mode", &["Complex", "Medium", "Simple"], 0.0),
    Param::new(B + 15, "Low Xover", 50.0, 2000.0, 300.0, Unit::Hz),
    Param::new(B + 16, "High Xover", 1000.0, 16000.0, 5000.0, Unit::Hz),
    Param::new(B + 17, "Reverb Density", 0.0, 200.0, 100.0, Unit::Percent),
    Param::new(B + 18, "Reverb High Cut", 1000.0, 20000.0, 16000.0, Unit::Hz),
    Param::new(B + 19, "Bass Gain", -12.0, 12.0, 0.0, Unit::Db),
    Param::new(B + 20, "Bass Crossover", 50.0, 1000.0, 200.0, Unit::Hz),
    Param::new(B + 21, "Reverb Correlation", 0.0, 100.0, 50.0, Unit::Percent),
    Param::menu(B + 22, "Dry Source", &["L R", "R L"], 0.0),
    Param::new(B + 23, "Subsonic", 0.0, 1.0, 1.0, Unit::OnOff),
];

pub struct Quantec {
    p: Values,
    rate: f32,
    lo: Fdn,
    mid: Fdn,
    xo: [Crossover; 2],
    xo_hi: [Crossover; 2],
    hc: [Biquad; 2],
    bass: [Biquad; 2],
    sub: [Biquad; 2],
    frozen: bool,
}

impl Quantec {
    pub fn new(rate: f32) -> Quantec {
        let mut q = Quantec { p: Values::new(&QRS_P), rate, lo: Fdn::new(rate), mid: Fdn::new(rate), xo: [Crossover::new(rate, 300.0); 2], xo_hi: [Crossover::new(rate, 5000.0); 2], hc: [Biquad::default(); 2], bass: [Biquad::default(); 2], sub: [Biquad::default(); 2], frozen: false };
        q.update();
        q
    }
    /// The reverb time range a room size allows (s): bigger rooms ring longer.
    pub fn time_range(size_m3: f32) -> (f32, f32) {
        let s = size_m3.max(1.0).log10() / 6.0;
        (0.1 + 0.9 * s, 2.0 + 98.0 * s * s)
    }
    fn update(&mut self) {
        if self.p.on(B + 4) {
            self.p.set(B + 4, 0.0);
            self.lo.clear();
            self.mid.clear();
        }
        let (lo, hi) = Quantec::time_range(self.p.get(B + 8));
        let t = self.p.get(B + 1).clamp(lo, hi);
        let size = (0.4 + 0.27 * self.p.get(B + 8).max(1.0).log10()).min(2.0);
        let (ml, mh) = (self.p.get(B + 6), self.p.get(B + 7));
        let yard = self.p.idx(B) == 1;
        self.frozen = self.p.on(B + 2);
        let damp = if yard { 6000.0 * mh.sqrt() * 2.0 } else { 7000.0 * mh.sqrt() };
        let ft = if self.frozen { 1e6 } else { t };
        self.lo.set(if self.frozen { 1e6 } else { t * ml }, size, 8000.0);
        self.mid.set(ft, size, damp);
        let pd = self.p.get(B + 9) * 0.001;
        self.lo.predelay = pd;
        self.mid.predelay = pd;
        // YardStick's Mode: less modulation and diffusion from Complex to Simple
        let m = [0.8, 0.4, 0.0][self.p.idx(B + 14)];
        self.lo.modulation = if yard { m } else { 0.3 };
        self.mid.modulation = if yard { m } else { 0.3 };
        let r = self.rate;
        let xo = if yard { self.p.get(B + 15) } else { 300.0 };
        for s in 0..2 {
            self.xo[s].set(r, xo);
            self.xo_hi[s].set(r, if yard { self.p.get(B + 16) } else { 5000.0 });
            self.hc[s].c = if yard { Coefs::lowpass(r, self.p.get(B + 18).min(r * 0.45), 0.7) } else { Coefs::THRU };
            self.bass[s].c = if yard && self.p.get(B + 19).abs() > 0.01 { Coefs::low_shelf(r, self.p.get(B + 20), 0.7, self.p.get(B + 19)) } else { Coefs::THRU };
            self.sub[s].c = if self.p.on(B + 23) { Coefs::THRU } else { Coefs::highpass(r, 25.0, 0.7) };
        }
    }
    fn clear(&mut self) {
        if !self.frozen {
            self.lo.clear();
            self.mid.clear();
        }
    }
    fn tail_secs(&self) -> f32 {
        self.p.get(B + 1).min(30.0)
    }
}

impl Effect for Quantec {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let dry = if self.p.get(B + 10) <= -95.9 { 0.0 } else { db(self.p.get(B + 10)) };
        let wet = if self.p.get(B + 11) <= -95.9 { 0.0 } else { db(self.p.get(B + 11)) };
        let yard = self.p.idx(B) == 1;
        let enhance = !yard && self.p.on(B + 5);
        // Freeze: the tail is held; nothing new goes in unless Add is held
        let feed = if self.frozen { if self.p.on(B + 3) { 0.3 } else { 0.0 } } else { 1.0 };
        let adda = !yard && self.p.on(B + 12);
        let drive = db(self.p.get(B + 13));
        let corr = self.p.get(B + 21) / 100.0;
        let density = self.p.get(B + 17) / 100.0;
        let swap = self.p.idx(B + 22) == 1;
        let hi_mult = self.p.get(B + 7);
        for i in 0..left.len().min(right.len()) {
            let (mut l, mut r) = (self.sub[0].run(left[i]), self.sub[1].run(right[i]));
            if adda {
                l = (l * drive).tanh() / drive.max(1.0).sqrt();
                r = (r * drive).tanh() / drive.max(1.0).sqrt();
            }
            let (ll, lh) = self.xo[0].run(l * feed);
            let (rl, rh) = self.xo[1].run(r * feed);
            let (a1, b1) = self.lo.run(ll, rl);
            let (a2, b2) = self.mid.run(lh, rh);
            let (mut a, mut b) = (a1 + a2, b1 + b2);
            if yard {
                // highs above the High crossover shortened or lengthened by the High multiplier
                let (_, ah) = self.xo_hi[0].run(a);
                let (_, bh) = self.xo_hi[1].run(b);
                a += ah * (hi_mult - 1.0).clamp(-1.0, 1.0) * 0.5;
                b += bh * (hi_mult - 1.0).clamp(-1.0, 1.0) * 0.5;
                a = self.bass[0].run(self.hc[0].run(a));
                b = self.bass[1].run(self.hc[1].run(b));
                a *= density.clamp(0.2, 2.0).sqrt();
                b *= density.clamp(0.2, 2.0).sqrt();
            }
            if enhance {
                // a dense cluster of early reflections instead of a long tail
                a = a * 0.3 + (l + r) * 0.0;
                b *= 0.3;
            }
            // Correlation: 0 = mono, 100 = fully decorrelated
            let m = 0.5 * (a + b);
            a = m + (a - m) * corr * 2.0;
            b = m + (b - m) * corr * 2.0;
            let (dl, dr) = if swap { (right[i], left[i]) } else { (left[i], right[i]) };
            left[i] = dl * dry + a * wet * 0.5;
            right[i] = dr * dry + b * wet * 0.5;
        }
    }
}
plugin_impl!(Quantec);

pub fn by_name(name: &str, rate: f32) -> Option<Box<dyn Effect>> {
    Some(match name {
        "EnVerb" => Box::new(EnVerb::new(rate)),
        "SilverVerb" => Box::new(SilverVerb::new(rate)),
        "Quantec Room Simulator" | "Quantec" | "QRS" => Box::new(Quantec::new(rate)),
        _ => return None,
    })
}

pub const NAMES: &[&str] = &["EnVerb", "SilverVerb", "Quantec Room Simulator"];

#[cfg(test)]
mod tests {
    use super::*;
    use lg_dsp::Plugin;

    fn impulse(fx: &mut dyn Effect, n: usize) -> Vec<f32> {
        let mut l = vec![0.0; n];
        l[0] = 1.0;
        let mut r = l.clone();
        fx.process(&mut l, &mut r);
        l
    }

    fn energy(x: &[f32]) -> f32 {
        x.iter().map(|v| v * v).sum()
    }

    #[test]
    fn enverb_tail_follows_its_envelope() {
        let mut e = EnVerb::new(48000.0);
        e.set(B + 12, -96.0);
        e.set(B, 500.0); // a slow 0.5 s attack
        e.set(B + 6, 0.0);
        let y = impulse(&mut e, 96000);
        // the swell: more energy at 0.4–0.5 s than at 0–0.1 s
        assert!(energy(&y[19200..24000]) > 4.0 * energy(&y[0..4800]));
        // and nothing after the envelope ends
        let end = ((0.5 + 0.3 + 0.3 + 0.8) * 48000.0) as usize + 500;
        assert!(energy(&y[end..]) < 1e-9);
    }

    #[test]
    fn silververb_rings_then_decays() {
        let mut s = SilverVerb::new(48000.0);
        s.set(B + 10, 100.0);
        let y = impulse(&mut s, 96000 * 2);
        assert!(energy(&y[4800..14400]) > 10.0 * energy(&y[150000..170000]));
    }

    #[test]
    fn quantec_freeze_holds_the_tail() {
        let mut q = Quantec::new(48000.0);
        q.set(B + 10, -96.0);
        q.set(B + 1, 1.0);
        q.set(B + 8, 100.0);
        let mut l = vec![0.0; 24000];
        for (i, v) in l.iter_mut().enumerate().take(4800) {
            *v = ((i as f32) * 0.05).sin() * 0.5;
        }
        let mut r = l.clone();
        q.process(&mut l, &mut r);
        q.set(B + 2, 1.0);
        let mut l2 = vec![0.0; 96000];
        let mut r2 = l2.clone();
        q.process(&mut l2, &mut r2);
        // two seconds later it's still sounding at about the same level
        let a = energy(&l2[0..9600]);
        let b = energy(&l2[86400..96000]);
        assert!(b > a * 0.3, "{a} {b}");
        q.set(B + 4, 1.0); // Clear
        let mut l3 = vec![0.0; 9600];
        let mut r3 = l3.clone();
        q.process(&mut l3, &mut r3);
        assert!(energy(&l3[4800..]) < 1e-6);
    }

    #[test]
    fn room_size_sets_the_time_range() {
        let (a, b) = Quantec::time_range(1.0);
        let (c, d) = Quantec::time_range(1_000_000.0);
        assert!(a < c && b < d && d >= 99.0);
    }
}
