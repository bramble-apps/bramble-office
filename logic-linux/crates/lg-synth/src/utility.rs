//! Logic's Utility instruments (the instrument slot's Utility submenu): Klopfgeist, the click
//! instrument that also makes Studio's metronome sound, and Test Oscillator as an instrument.
//! External Instrument: the track's MIDI goes to the module (lg-app sends it, from the track's
//! External settings); the module's audio comes back through the side-chain input (Input 1-2)
//! at the Input Volume, into the instrument channel strip and its inserts and sends.
//!
//! Klopfgeist, as Logic's: Trigger Mode Mono (one voice) or Poly (four), Tune in semitones,
//! Detune in cents, Tonality from a short click to a pitched woodblock / claves, Damp the release
//! (maximum = the shortest), and Level via Vel: the levels at the lowest and the highest velocity.

use lg_dsp::{midi_hz, Effect, Instrument, Param, Plugin, Unit};

pub const KLOPFGEIST: &str = "Klopfgeist";
pub const TEST_OSCILLATOR: &str = "Test Oscillator";
pub const EXTERNAL_INSTRUMENT: &str = "External Instrument";

pub const K_MODE: u32 = 0;
pub const K_TUNE: u32 = 1;
pub const K_DETUNE: u32 = 2;
pub const K_TONALITY: u32 = 3;
pub const K_DAMP: u32 = 4;
pub const K_VEL_MIN: u32 = 5;
pub const K_VEL_MAX: u32 = 6;

pub static K_PARAMS: &[Param] = &[
    Param::menu(K_MODE, "Trigger Mode", &["Mono", "Poly"], 1.0),
    Param::new(K_TUNE, "Tune", -24.0, 24.0, 0.0, Unit::Semitones),
    Param::new(K_DETUNE, "Detune", -50.0, 50.0, 0.0, Unit::Cents),
    Param::new(K_TONALITY, "Tonality", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::new(K_DAMP, "Damp", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(K_VEL_MIN, "Level via Vel (min)", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::new(K_VEL_MAX, "Level via Vel (max)", 0.0, 1.0, 1.0, Unit::Ratio),
];

/// Klopfgeist's sound at `t` seconds after the strike: a noise-and-sine click whose body turns,
/// with Tonality, into the inharmonic modes of a struck wooden bar (a pitched woodblock /
/// claves). Damp shortens the decay. Deterministic (the metronome uses it too).
pub fn tone(t: f32, freq: f32, tonality: f32, damp: f32) -> f32 {
    if t < 0.0 {
        return 0.0;
    }
    let tau = std::f32::consts::TAU;
    let ton = tonality.clamp(0.0, 1.0);
    // decay rate (1/s): a click dies in a few ms, a woodblock rings ~100 ms; Damp shortens it
    let decay = (160.0 - 135.0 * ton) * (0.6 + 0.8 * damp.clamp(0.0, 1.0));
    let body = (tau * freq * t).sin()
        + ton * 0.45 * (tau * freq * 2.76 * t).sin() * (-t * decay * 1.6).exp()
        + ton * 0.2 * (tau * freq * 5.40 * t).sin() * (-t * decay * 2.4).exp();
    // the strike: a 1 ms noise burst, louder for a short click
    let n = ((t * 48_000.0).floor() * 12.9898).sin() * 43_758.547;
    let noise = (n - n.floor()) * 2.0 - 1.0;
    let strike = noise * (-t * 1500.0).exp() * (1.0 - 0.7 * ton);
    (body * (-t * decay).exp() + strike) * 0.7
}

/// How long a Klopfgeist strike sounds (seconds) before it is inaudible.
pub fn tone_length(tonality: f32, damp: f32) -> f32 {
    let decay = (160.0 - 135.0 * tonality.clamp(0.0, 1.0)) * (0.6 + 0.8 * damp.clamp(0.0, 1.0));
    (9.0 / decay).max(0.02)
}

#[derive(Clone, Copy)]
struct KVoice {
    note: u8,
    t: f32,
    freq: f32,
    amp: f32,
    /// 1 while held, falls after the note-off at the Damp rate.
    rel: f32,
    held: bool,
    age: u64,
}

pub struct Klopfgeist {
    params: crate::poly::Params,
    rate: f32,
    voices: Vec<KVoice>,
    clock: u64,
}

impl Klopfgeist {
    pub fn new(rate: f32) -> Klopfgeist {
        Klopfgeist { params: crate::poly::Params::new(K_PARAMS), rate, voices: Vec::new(), clock: 0 }
    }
    /// Voices Trigger Mode allows: Mono one, Poly four.
    pub fn max_voices(&self) -> usize {
        if self.params.get(K_MODE) >= 0.5 {
            4
        } else {
            1
        }
    }
    pub fn sounding(&self) -> usize {
        self.voices.len()
    }
    /// The level for a velocity: the min slider at velocity 1, the max slider at 127.
    pub fn level(&self, vel: u8) -> f32 {
        let (lo, hi) = (self.params.get(K_VEL_MIN), self.params.get(K_VEL_MAX));
        lo + (hi - lo) * (vel.clamp(1, 127) - 1) as f32 / 126.0
    }
    /// A note's frequency after Tune (semitones) and Detune (cents).
    pub fn freq(&self, note: u8) -> f32 {
        midi_hz(note as f32 + self.params.get(K_TUNE).round() + self.params.get(K_DETUNE) / 100.0)
    }
    /// Release time (seconds) after the note-off: the maximum Damp is the shortest.
    pub fn release(&self) -> f32 {
        0.002 + 0.4 * (1.0 - self.params.get(K_DAMP)).powi(2)
    }
}

impl Plugin for Klopfgeist {
    fn params(&self) -> &[Param] {
        K_PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        self.params.set(id, v);
        let max = self.max_voices();
        while self.voices.len() > max {
            self.voices.remove(0);
        }
    }
    fn reset(&mut self) {
        self.voices.clear();
    }
    fn tail(&self) -> f32 {
        tone_length(self.params.get(K_TONALITY), self.params.get(K_DAMP)) + self.release()
    }
}

impl Instrument for Klopfgeist {
    fn note_on(&mut self, pitch: u8, vel: u8) {
        if vel == 0 {
            return self.note_off(pitch);
        }
        self.clock += 1;
        let v = KVoice { note: pitch, t: 0.0, freq: self.freq(pitch), amp: self.level(vel), rel: 1.0, held: true, age: self.clock };
        let max = self.max_voices();
        if self.voices.len() >= max {
            // steal the oldest
            if let Some(i) = self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i) {
                self.voices.remove(i);
            }
        }
        self.voices.push(v);
    }
    fn note_off(&mut self, pitch: u8) {
        for v in self.voices.iter_mut().filter(|v| v.note == pitch) {
            v.held = false;
        }
    }
    fn controller(&mut self, cc: u8, _v: u8) {
        if cc == 120 || cc == 123 {
            self.voices.clear();
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let dt = 1.0 / self.rate;
        let (ton, damp) = (self.params.get(K_TONALITY), self.params.get(K_DAMP));
        let len = tone_length(ton, damp);
        let fall = (-dt / self.release()).exp();
        for v in &mut self.voices {
            for i in 0..l.len().min(r.len()) {
                if !v.held {
                    v.rel *= fall;
                }
                let y = tone(v.t, v.freq, ton, damp) * v.amp * v.rel;
                l[i] += y;
                r[i] += y;
                v.t += dt;
            }
        }
        self.voices.retain(|v| v.t < len && v.rel > 1e-4);
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// Test Oscillator on an instrument channel strip: the same generator as the effect (lg-fx), fed
/// silence. Test Tone sounds as soon as it is inserted; in Sine Sweep mode a note (or Trigger)
/// starts the sweep.
pub struct TestOscInstrument {
    pub osc: lg_fx::more_fx::TestOscillator,
}

/// The Test Oscillator's parameter ids (lg-fx numbers them from 1000).
pub const TO_GENERATOR: u32 = 1000;
pub const TO_TRIGGER: u32 = 1011;

impl TestOscInstrument {
    pub fn new(rate: f32) -> TestOscInstrument {
        TestOscInstrument { osc: lg_fx::more_fx::TestOscillator::new(rate) }
    }
    /// The sweep's current frequency (Hz), for the window's readout.
    pub fn sweep_hz(&self) -> f32 {
        self.osc.sweep_hz
    }
}

impl Plugin for TestOscInstrument {
    fn params(&self) -> &[Param] {
        self.osc.params()
    }
    fn get(&self, id: u32) -> f32 {
        self.osc.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        self.osc.set(id, v)
    }
    fn reset(&mut self) {
        self.osc.reset()
    }
}

impl Instrument for TestOscInstrument {
    fn note_on(&mut self, _pitch: u8, vel: u8) {
        if vel > 0 && self.osc.get(TO_GENERATOR).round() as usize == 5 {
            self.osc.set(TO_TRIGGER, 0.0);
            self.osc.set(TO_TRIGGER, 1.0);
        }
    }
    fn note_off(&mut self, _pitch: u8) {}
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len());
        let (mut a, mut b) = (vec![0.0; n], vec![0.0; n]);
        self.osc.process(&mut a, &mut b);
        for i in 0..n {
            l[i] += a[i];
            r[i] += b[i];
        }
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

pub const EI_VOLUME: u32 = 0;
pub const EI_INPUT: u32 = 1;
pub static EI_PARAMS: &[Param] = &[
    Param::new(EI_VOLUME, "Input Volume", -60.0, 12.0, 0.0, Unit::Db),
    Param::menu(EI_INPUT, "Input", &["Off", "Input 1-2"], 1.0),
];

/// External Instrument: the module's audio (the side-chain input) at the Input Volume.
pub struct ExternalInstrument {
    params: crate::poly::Params,
    inl: Vec<f32>,
    inr: Vec<f32>,
}

impl ExternalInstrument {
    pub fn new(_rate: f32) -> ExternalInstrument {
        ExternalInstrument { params: crate::poly::Params::new(EI_PARAMS), inl: Vec::new(), inr: Vec::new() }
    }
}

impl Plugin for ExternalInstrument {
    fn params(&self) -> &[Param] {
        EI_PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        self.params.set(id, v);
    }
    fn reset(&mut self) {
        self.inl.clear();
        self.inr.clear();
    }
}

impl Instrument for ExternalInstrument {
    fn note_on(&mut self, _pitch: u8, _vel: u8) {}
    fn note_off(&mut self, _pitch: u8) {}
    fn side_chain(&mut self, l: &[f32], r: &[f32]) {
        self.inl.clear();
        self.inl.extend_from_slice(l);
        self.inr.clear();
        self.inr.extend_from_slice(r);
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        if self.params.get(EI_INPUT) < 0.5 {
            return;
        }
        let g = if self.params.get(EI_VOLUME) <= -60.0 { 0.0 } else { lg_dsp::db_to_gain(self.params.get(EI_VOLUME)) };
        for i in 0..l.len().min(self.inl.len()) {
            l[i] += self.inl[i] * g;
        }
        for i in 0..r.len().min(self.inr.len()) {
            r[i] += self.inr[i] * g;
        }
        // (each block's input is used once)
        self.inl.clear();
        self.inr.clear();
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(k: &mut Klopfgeist, n: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        k.render(&mut l, &mut r);
        l
    }
    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |a, v| a.max(v.abs()))
    }

    #[test]
    fn poly_has_four_voices_mono_one() {
        let mut k = Klopfgeist::new(48_000.0);
        for n in 60..66 {
            k.note_on(n, 100);
        }
        assert_eq!(k.sounding(), 4);
        k.set(K_MODE, 0.0);
        assert_eq!(k.sounding(), 1);
        k.note_on(70, 100);
        k.note_on(72, 100);
        assert_eq!(k.sounding(), 1);
    }

    #[test]
    fn tune_in_semitones_detune_in_cents() {
        let mut k = Klopfgeist::new(48_000.0);
        let f0 = k.freq(69);
        k.set(K_TUNE, 12.0);
        assert!((k.freq(69) / f0 - 2.0).abs() < 1e-3);
        k.set(K_TUNE, 0.4); // semitone steps
        assert!((k.freq(69) - f0).abs() < 1e-3);
        k.set(K_TUNE, 0.0);
        k.set(K_DETUNE, 50.0);
        assert!((k.freq(69) / f0 - 2f32.powf(0.5 / 12.0)).abs() < 1e-4);
    }

    #[test]
    fn tonality_rings_longer_damp_releases_faster() {
        assert!(tone_length(1.0, 0.5) > 3.0 * tone_length(0.0, 0.5));
        let mut k = Klopfgeist::new(48_000.0);
        k.set(K_DAMP, 1.0);
        let short = k.release();
        k.set(K_DAMP, 0.0);
        assert!(k.release() > 50.0 * short);
        // a released note at full Damp dies within a few ms
        k.set(K_DAMP, 1.0);
        k.set(K_TONALITY, 1.0);
        k.note_on(60, 127);
        run(&mut k, 48);
        k.note_off(60);
        run(&mut k, 960);
        let after = run(&mut k, 480);
        assert!(peak(&after) < 1e-3, "{}", peak(&after));
    }

    #[test]
    fn level_via_velocity() {
        let mut k = Klopfgeist::new(48_000.0);
        k.set(K_VEL_MIN, 0.2);
        k.set(K_VEL_MAX, 0.8);
        assert!((k.level(1) - 0.2).abs() < 1e-6);
        assert!((k.level(127) - 0.8).abs() < 1e-6);
        k.note_on(60, 127);
        let loud = peak(&run(&mut k, 2400));
        k.reset();
        k.note_on(60, 1);
        let soft = peak(&run(&mut k, 2400));
        assert!(loud > 3.0 * soft && soft > 0.0);
    }

    #[test]
    fn test_oscillator_instrument_sounds_and_sweeps_on_a_note() {
        let mut t = TestOscInstrument::new(48_000.0);
        let (mut l, mut r) = (vec![0.0; 480], vec![0.0; 480]);
        t.render(&mut l, &mut r);
        assert!(peak(&l) > 0.05, "Test Tone sounds when inserted");
        t.set(TO_GENERATOR, 5.0);
        let (mut l, mut r) = (vec![0.0; 480], vec![0.0; 480]);
        t.render(&mut l, &mut r);
        assert_eq!(peak(&l), 0.0);
        t.note_on(60, 100);
        t.render(&mut l, &mut r);
        assert!(peak(&l) > 0.05 && t.sweep_hz() > 0.0);
    }

    #[test]
    fn external_instrument_returns_the_input_at_its_volume() {
        let mut e = ExternalInstrument::new(48_000.0);
        e.set(EI_VOLUME, -6.0);
        e.side_chain(&[0.5; 64], &[0.25; 64]);
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        e.render(&mut l, &mut r);
        assert!((l[10] - 0.5 * 0.501_187).abs() < 1e-4 && (r[10] - 0.25 * 0.501_187).abs() < 1e-4);
        e.side_chain(&[0.5; 64], &[0.5; 64]);
        e.set(EI_INPUT, 0.0);
        let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
        e.render(&mut l, &mut r);
        assert_eq!(l[10], 0.0);
    }
}
