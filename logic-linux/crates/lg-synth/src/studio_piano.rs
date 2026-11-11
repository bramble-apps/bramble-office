//! Studio Piano stand-in: the bundled sampled grand when the sample library is installed (the
//! modelled piano otherwise) heard through three microphone sets that are mixed like faders:
//! Stereo Mic A (a condenser pair: open and wide), Stereo Mic B (a ribbon pair: darker, closer
//! together) and the Mono Mic (one ribbon). Around the notes: key noise, the dampers returning
//! (Release Samples), pedal noise and the sympathetic resonance of undamped strings.

use crate::dsp::{self, Biquad, CHUNK, Delay, OnePole, Rng, Room};
use crate::piano::StudioPiano;
use crate::sampled::SampledInstrument;
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const INSTRUMENTS: &[&str] = &["Studio Grand", "Concert Grand", "Vintage Upright", "Studio Grand (Mono Mic)"];

pub static PARAMS: &[Param] = &[
    Param::menu(0, "Instrument", INSTRUMENTS, 0.0),
    Param::new(1, "Stereo Mic A", 0.0, 1.0, 0.8, Unit::Ratio),
    Param::new(2, "Stereo Mic B", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(3, "Mono Mic", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(4, "Stereo Mic A On", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(5, "Stereo Mic B On", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(6, "Mono Mic On", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(7, "Main Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(8, "Pedal Noise", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(9, "Key Noise", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(10, "Release Samples", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(11, "Sympathetic Resonance", 0.0, 1.0, 0.4, Unit::Ratio),
];

pub struct StudioPianoInst {
    params: crate::poly::Params,
    inner: Box<dyn Instrument>,
    sampled: bool,
    use_samples: bool,
    instrument: usize,
    rate: f32,
    pedal: bool,
    volume_cc: f32,
    expression: f32,
    // the instrument's character
    body: [Biquad; 2],
    air: [Biquad; 2],
    // microphones
    a_shelf: [Biquad; 2],
    b_lp: [OnePole; 2],
    b_delay: [Delay; 2],
    mono_lp: OnePole,
    mono_shelf: Biquad,
    // noises and resonance
    rng: Rng,
    key: f32,
    key_f: Biquad,
    felt: f32,
    felt_f: Biquad,
    thud: f32,
    thud_f: Biquad,
    strings: Room,
    res_send: f32,
}

impl StudioPianoInst {
    pub fn new(rate: f32) -> StudioPianoInst {
        Self::with_samples(rate, true)
    }
    pub fn with_samples(rate: f32, use_samples: bool) -> StudioPianoInst {
        let d = || Delay::new((rate * 0.02) as usize);
        let mut strings = Room::new(rate, 1.7);
        strings.feedback = 0.93;
        strings.damp = 0.45;
        let mut p = StudioPianoInst {
            params: crate::poly::Params::new(PARAMS),
            inner: Box::new(StudioPiano::new(rate)),
            sampled: false,
            use_samples,
            instrument: usize::MAX,
            rate,
            pedal: false,
            volume_cc: 1.0,
            expression: 1.0,
            body: [Biquad::default(); 2],
            air: [Biquad::default(); 2],
            a_shelf: [Biquad::default(); 2],
            b_lp: [OnePole::default(); 2],
            b_delay: [d(), d()],
            mono_lp: OnePole::default(),
            mono_shelf: Biquad::default(),
            rng: Rng::new(88),
            key: 0.0,
            key_f: Biquad::default(),
            felt: 0.0,
            felt_f: Biquad::default(),
            thud: 0.0,
            thud_f: Biquad::default(),
            strings,
            res_send: 0.0,
        };
        p.key_f.bandpass(900.0, 0.8, rate);
        p.felt_f.bandpass(350.0, 0.7, rate);
        p.thud_f.bandpass(90.0, 1.0, rate);
        p.mono_shelf.low_shelf(160.0, 2.0, rate);
        p.mono_lp.lowpass_hz(5000.0, rate);
        for c in 0..2 {
            p.a_shelf[c].high_shelf(6000.0, 2.5, rate);
            p.b_lp[c].lowpass_hz(5500.0, rate);
        }
        p.apply();
        p
    }
    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    /// The three microphone sets' levels as they are heard (the Mono Mic instrument has only
    /// its one microphone).
    pub fn mic_levels(&self) -> (f32, f32, f32) {
        let on = |id: u32| if self.p(id) >= 0.5 { 1.0 } else { 0.0 };
        if self.instrument == 3 {
            (0.0, 0.0, self.p(3))
        } else {
            (self.p(1) * on(4), self.p(2) * on(5), self.p(3) * on(6))
        }
    }
    fn apply(&mut self) {
        let rate = self.rate;
        let inst = (self.p(0).round() as usize).min(INSTRUMENTS.len() - 1);
        if inst != self.instrument {
            self.instrument = inst;
            let sampled = if self.use_samples { SampledInstrument::load("piano-grand", rate) } else { None };
            match sampled {
                Some(s) => {
                    self.inner = Box::new(s);
                    self.sampled = true;
                }
                None => {
                    let mut m = StudioPiano::new(rate);
                    m.set(13, [1.0, 0.0, 2.0, 1.0][inst]);
                    // (its own pedal noise and resonance are replaced by this instrument's)
                    m.set(10, 0.0);
                    m.set(5, 0.0);
                    m.set(7, 0.1);
                    self.inner = Box::new(m);
                    self.sampled = false;
                }
            }
            // the instruments' characters: the concert grand fuller and more open, the upright boxy
            let (body, air): ((f32, f32), f32) = match inst {
                1 => ((110.0, 2.5), 2.5),
                2 => ((450.0, 4.5), -6.0),
                _ => ((200.0, 0.0), 0.0),
            };
            for c in 0..2 {
                self.body[c].peaking(body.0, 0.8, body.1, rate);
                self.air[c].high_shelf(5500.0, air, rate);
            }
        }
        if !self.sampled {
            self.inner.set(11, 0.0);
        }
    }
}

impl Plugin for StudioPianoInst {
    fn params(&self) -> &[Param] {
        PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            self.apply();
        }
    }
    fn reset(&mut self) {
        self.inner.reset();
        self.strings.reset();
        for d in &mut self.b_delay {
            d.clear();
        }
        self.pedal = false;
        self.expression = 1.0;
        self.key = 0.0;
        self.felt = 0.0;
        self.thud = 0.0;
    }
    fn tail(&self) -> f32 {
        3.0
    }
    fn load_floats(&mut self, _floats: &[f32]) {}
}

impl Instrument for StudioPianoInst {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        self.inner.note_on(pitch, velocity);
        // the key going down
        self.key += self.p(9) * (0.3 + 0.7 * velocity as f32 / 127.0) * 0.5;
    }
    fn note_off(&mut self, pitch: u8) {
        self.inner.note_off(pitch);
        if !self.pedal {
            // the damper coming back onto the strings, the key coming up
            self.felt += self.p(10) * 0.5 * if pitch < 90 { 1.0 } else { 0.2 };
            self.key += self.p(9) * 0.15;
        }
    }
    fn controller(&mut self, cc: u8, value: u8) {
        match cc {
            7 => self.volume_cc = (value as f32 / 127.0).powi(2),
            11 => self.expression = value as f32 / 127.0,
            64 => {
                let down = value >= 64;
                if down != self.pedal {
                    self.pedal = down;
                    self.thud += self.p(8) * if down { 0.7 } else { 1.0 };
                    if !down {
                        self.felt += self.p(8) * 0.6;
                    }
                }
                self.inner.controller(cc, value);
            }
            _ => self.inner.controller(cc, value),
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.inner.pitch_bend(value);
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.inner.set_tempo(bpm);
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let rate = self.rate;
        let (a, b, mono) = self.mic_levels();
        let out = lg_dsp::db_to_gain(self.p(7)) * self.volume_cc * self.expression;
        // undamped strings ring along: all of them with the pedal down, a little otherwise
        let send = self.p(11) * if self.pedal { 1.0 } else { 0.12 };
        let b_gap = [rate * 0.0031, rate * 0.0043];
        let mut dl = [0.0f32; CHUNK];
        let mut dr = [0.0f32; CHUNK];
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            dl[..m].fill(0.0);
            dr[..m].fill(0.0);
            self.inner.render(&mut dl[..m], &mut dr[..m]);
            for i in 0..m {
                // key, damper and pedal noises are part of what the microphones hear
                let w = self.rng.bipolar();
                let mut noise = 0.0;
                if self.key > 1e-4 {
                    noise += self.key_f.run(w) * self.key * 0.06;
                    self.key *= 0.994;
                }
                if self.felt > 1e-4 {
                    noise += self.felt_f.run(w) * self.felt * 0.05;
                    self.felt *= 0.9988;
                }
                if self.thud > 1e-4 {
                    noise += self.thud_f.run(w) * self.thud * 0.12;
                    self.thud *= 0.9993;
                }
                dl[i] = self.air[0].run(self.body[0].run(dl[i])) + noise;
                dr[i] = self.air[1].run(self.body[1].run(dr[i])) + noise;
            }
            if self.p(11) > 0.0 {
                self.res_send += (send - self.res_send) * 0.02;
                self.strings.process(&mut dl[..m], &mut dr[..m], self.res_send * 1.2);
            }
            for i in 0..m {
                let (l, r) = (dl[i], dr[i]);
                // A: the condenser pair, open and wide
                let (al, ar) = (self.a_shelf[0].run(l), self.a_shelf[1].run(r));
                // B: the ribbon pair, darker, closer together and a little further back
                self.b_delay[0].push(self.b_lp[0].lp(0.7 * l + 0.3 * r));
                self.b_delay[1].push(self.b_lp[1].lp(0.7 * r + 0.3 * l));
                let (bl, br) = (self.b_delay[0].read(b_gap[0]), self.b_delay[1].read(b_gap[1]));
                // Mono: one ribbon in front of the instrument
                let mm = self.mono_shelf.run(self.mono_lp.lp(0.5 * (l + r)));
                left[pos + i] += (al * a + bl * b + mm * mono) * out;
                right[pos + i] += (ar * a + br * b + mm * mono) * out;
            }
            pos += m;
        }
        let _ = dsp::CHUNK;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }
    fn play(p: &mut StudioPianoInst, secs: f32) -> (Vec<f32>, Vec<f32>) {
        let n = (48000.0 * secs) as usize;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            p.render(a, b);
        }
        (l, r)
    }
    fn piano(set: &[(u32, f32)]) -> StudioPianoInst {
        let mut p = StudioPianoInst::with_samples(48000.0, false);
        for (id, v) in set {
            p.set(*id, *v);
        }
        p
    }
    fn tone(set: &[(u32, f32)]) -> (f32, f32, f32) {
        let mut p = piano(set);
        p.note_on(48, 100);
        let (l, r) = play(&mut p, 0.5);
        let diff: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a - b).collect();
        (rms(&l), rms(&r), rms(&diff))
    }

    #[test]
    fn the_three_microphones_mix_and_switch() {
        let quiet = [(8, 0.0), (9, 0.0), (10, 0.0), (11, 0.0)];
        let with = |more: &[(u32, f32)]| {
            let mut v = quiet.to_vec();
            v.extend_from_slice(more);
            tone(&v)
        };
        let a = with(&[]);
        assert!(a.0 > 1e-3);
        let off = with(&[(4, 0.0)]);
        assert!(off.0 < 1e-6, "every microphone off: silence");
        let b = with(&[(4, 0.0), (5, 1.0), (2, 1.0)]);
        assert!(b.0 > 1e-3 && (b.0 - a.0).abs() > 1e-5);
        let mono = with(&[(4, 0.0), (6, 1.0), (3, 1.0)]);
        assert!(mono.0 > 1e-3 && mono.2 < 1e-6, "the mono microphone is the same on both sides");
        assert!(a.2 > mono.2);
        // a level slider
        let half = with(&[(1, 0.4)]);
        assert!((half.0 / a.0 - 0.5).abs() < 0.02);
        // the Mono Mic instrument has just its one microphone
        let mm = with(&[(0, 3.0)]);
        assert!(mm.0 > 1e-3 && mm.2 < 1e-6);
    }

    #[test]
    fn instruments_sound_different() {
        let (studio, concert, upright) = (tone(&[]).0, tone(&[(0, 1.0)]).0, tone(&[(0, 2.0)]).0);
        assert!(studio != concert && concert != upright && studio != upright);
    }

    #[test]
    fn noises_and_resonance_follow_their_sliders() {
        // key noise at the attack
        let attack = |key: f32| {
            let mut p = piano(&[(9, key), (8, 0.0), (10, 0.0), (11, 0.0), (4, 0.0), (6, 1.0)]);
            p.note_on(100, 1);
            rms(&play(&mut p, 0.03).0)
        };
        assert!(attack(1.0) > attack(0.0) * 1.5);
        // pedal noise without any note
        let pedal = |level: f32| {
            let mut p = piano(&[(8, level)]);
            p.controller(64, 127);
            rms(&play(&mut p, 0.2).0)
        };
        assert!(pedal(1.0) > 1e-5);
        assert!(pedal(0.0) < 1e-7);
        // the damper returning when a key is let go
        let release = |level: f32| {
            let mut p = piano(&[(10, level), (9, 0.0), (11, 0.0)]);
            p.note_on(60, 100);
            play(&mut p, 0.2);
            p.note_off(60);
            p.felt
        };
        assert!(release(1.0) > 0.0 && release(0.0) == 0.0);
        // sympathetic resonance: more ring after a staccato chord with the pedal down
        let ring = |level: f32| {
            let mut p = piano(&[(11, level), (8, 0.0), (9, 0.0), (10, 0.0)]);
            p.controller(64, 127);
            p.note_on(48, 110);
            play(&mut p, 0.3);
            p.controller(64, 0);
            p.note_off(48);
            play(&mut p, 0.3);
            rms(&play(&mut p, 0.4).0)
        };
        assert!(ring(1.0) > ring(0.0) * 1.2, "{} vs {}", ring(1.0), ring(0.0));
    }
}
