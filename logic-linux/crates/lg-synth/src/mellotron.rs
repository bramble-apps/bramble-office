//! Vintage Mellotron stand-in: tape-replay voices (wow and flutter, tape hiss and roll-off) from
//! the Ensemble engine. Two sounds, A and B, each with its own octave, blended with one knob;
//! Tape Speed moves the pitch and the tone of every note together, as varying the motor did.
//!
//! Parameter id 0 is the first version's factory setting (library patches use it): it chooses
//! sounds A and B and their blend together.

use crate::ensemble::StudioEnsemble;
use lg_dsp::{Instrument, Param, Plugin, Unit};

/// Logic's Vintage Mellotron factory settings (#default = 3 Violins).
pub const SOUNDS: &[&str] = &["3 Violins", "8 Choir & Flute", "Boys Choir", "Brass", "Cello", "Flute", "Flutes", "GC 3 Brass", "Mixed Choir", "Strings & Flute"];
/// The tape sounds.
pub const TAPES: &[&str] = &["3 Violins", "8 Choir", "Boys Choir", "Brass", "Cello", "Flute", "Flutes", "GC 3 Brass", "Mixed Choir", "Strings"];

pub static PARAMS: &[Param] = &[
    Param::menu(0, "Factory Setting", SOUNDS, 0.0).hide(),
    Param::new(1, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(2, "Tone", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(4, "Blend A/B", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(5, "Attack", 0.0, 1.0, 0.1, Unit::Ratio),
    Param::new(6, "Release", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::menu(7, "Sound A", TAPES, 0.0),
    Param::menu(8, "Sound B", TAPES, 5.0),
    Param::new(9, "Transpose A", -2.0, 2.0, 0.0, Unit::Octaves),
    Param::new(10, "Transpose B", -2.0, 2.0, 0.0, Unit::Octaves),
    Param::new(11, "Tape Speed", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(12, "Pitch Bend Range", 0.0, 12.0, 2.0, Unit::Semitones),
    Param::new(13, "Fixed Velocity", 0.0, 1.0, 0.0, Unit::OnOff),
];

/// A tape: (ensemble model, brightness, section size, ensemble width).
fn tape(k: usize) -> (f32, f32, f32, f32) {
    match k {
        1 => (13.0, 0.5, 3.0, 0.15),
        2 => (18.0, 0.5, 3.0, 0.15),
        3 => (16.0, 0.5, 3.0, 0.15),
        4 => (17.0, 0.35, 1.0, 0.15),
        5 => (12.0, 0.5, 1.0, 0.15),
        6 => (12.0, 0.55, 3.0, 0.7),
        7 => (16.0, 0.75, 3.0, 0.15),
        8 => (13.0, 0.55, 5.0, 0.3),
        9 => (11.0, 0.45, 5.0, 0.4),
        _ => (11.0, 0.5, 3.0, 0.15),
    }
}

/// A factory setting: (sound A, sound B, blend).
fn factory(k: usize) -> (f32, f32, f32) {
    match k {
        1 => (1.0, 5.0, 0.5),
        9 => (9.0, 5.0, 0.5),
        k => (k as f32, 5.0, 0.0),
    }
}

/// How far Tape Speed moves the pitch at either end, in semitones.
const SPEED_RANGE: f32 = 3.0;

pub struct VintageMellotron {
    params: crate::poly::Params,
    a: StudioEnsemble,
    b: StudioEnsemble,
    /// The notes each sound is playing for an incoming note (their transposition when struck).
    playing: [Option<(u8, u8)>; 128],
    bend: f32,
}

impl VintageMellotron {
    pub fn new(rate: f32) -> VintageMellotron {
        let mut m = VintageMellotron { params: crate::poly::Params::new(PARAMS), a: StudioEnsemble::new(rate), b: StudioEnsemble::new(rate), playing: [None; 128], bend: 0.0 };
        m.apply();
        m
    }
    /// The two sounds' levels for a blend: either end isolates one sound, the middle has both.
    pub fn blend_gains(blend: f32) -> (f32, f32) {
        ((2.0 * (1.0 - blend)).min(1.0), (2.0 * blend).min(1.0))
    }
    fn apply(&mut self) {
        let p = |i| self.params.get(i);
        let (ga, gb) = Self::blend_gains(p(4));
        let speed = p(11);
        // Tone: right thins the bass for a brighter, nasal sound; left takes brightness away.
        // A faster tape is brighter too.
        let tone = (p(2) - 0.5) + 0.2 * speed;
        for (ens, sound, gain) in [(&mut self.a, p(7), ga), (&mut self.b, p(8), gb)] {
            let (model, bright, size, wide) = tape(sound.round() as usize);
            ens.set(0, model);
            ens.set(3, (bright + tone).clamp(0.0, 1.0));
            ens.set(4, size);
            ens.set(1, 0.005 + p(5) * 0.6);
            ens.set(2, 0.05 + p(6) * 1.2);
            ens.set(7, wide);
            ens.set(8, 0.15);
            ens.set(11, 24.0);
            ens.set(10, if gain <= 0.001 { -40.0 } else { p(1) + lg_dsp::gain_to_db(gain) });
        }
        self.send_bend();
    }
    /// The player's bend (by the Pitch Bend Range) and the tape speed together.
    fn send_bend(&mut self) {
        let st = self.bend * self.params.get(12) + self.params.get(11) * SPEED_RANGE;
        let v = (st / 24.0 * 8192.0).clamp(-8192.0, 8191.0) as i16;
        self.a.pitch_bend(v);
        self.b.pitch_bend(v);
    }
    /// Whether each sound is heard at all (a sound blended out isn't played).
    fn heard(&self) -> (bool, bool) {
        let (ga, gb) = Self::blend_gains(self.params.get(4));
        (ga > 0.001, gb > 0.001)
    }
}

impl Plugin for VintageMellotron {
    fn params(&self) -> &[Param] {
        PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        let before = self.params.get(id);
        if self.params.set(id, v) {
            if id == 0 && self.params.get(0) != before {
                let (a, b, blend) = factory(self.params.get(0).round() as usize);
                self.params.set(7, a);
                self.params.set(8, b);
                self.params.set(4, blend);
            }
            self.apply();
        }
    }
    fn reset(&mut self) {
        self.a.reset();
        self.b.reset();
        self.playing = [None; 128];
        self.bend = 0.0;
        self.apply();
    }
    fn tail(&self) -> f32 {
        2.0
    }
}

impl Instrument for VintageMellotron {
    fn note_on(&mut self, p: u8, v: u8) {
        if v == 0 {
            return self.note_off(p);
        }
        let p = p.min(127);
        if self.playing[p as usize].is_some() {
            self.note_off(p);
        }
        let v = if self.params.get(13) >= 0.5 { 100 } else { v };
        let at = |oct: f32| (p as i32 + 12 * oct.round() as i32).clamp(0, 127) as u8;
        let (na, nb) = (at(self.params.get(9)), at(self.params.get(10)));
        let (ha, hb) = self.heard();
        if ha {
            self.a.note_on(na, v);
        }
        if hb {
            self.b.note_on(nb, v);
        }
        self.playing[p as usize] = Some((na, nb));
    }
    fn note_off(&mut self, p: u8) {
        if let Some((na, nb)) = self.playing[p.min(127) as usize].take() {
            self.a.note_off(na);
            self.b.note_off(nb);
        }
    }
    fn controller(&mut self, cc: u8, v: u8) {
        if cc == 123 || cc == 120 {
            self.playing = [None; 128];
        }
        self.a.controller(cc, v);
        self.b.controller(cc, v);
    }
    fn pitch_bend(&mut self, v: i16) {
        self.bend = v as f32 / 8192.0;
        self.send_bend();
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.a.render(l, r);
        self.b.render(l, r);
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

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }
    fn play(m: &mut VintageMellotron, secs: f32) -> Vec<f32> {
        let n = (48000.0 * secs) as usize;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            m.render(a, b);
        }
        l
    }
    fn zero_crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }

    #[test]
    fn blend_isolates_or_mixes_the_two_sounds() {
        assert_eq!(VintageMellotron::blend_gains(0.0), (1.0, 0.0));
        assert_eq!(VintageMellotron::blend_gains(1.0), (0.0, 1.0));
        assert_eq!(VintageMellotron::blend_gains(0.5), (1.0, 1.0));
        let level = |blend: f32| {
            let mut m = VintageMellotron::new(48000.0);
            m.set(4, blend);
            m.note_on(60, 100);
            play(&mut m, 0.4);
            rms(&play(&mut m, 0.4))
        };
        let (a, both, b) = (level(0.0), level(0.5), level(1.0));
        assert!(a > 1e-3 && b > 1e-3);
        assert!(both > a.max(b) * 1.05, "{a} {both} {b}");
    }

    #[test]
    fn each_sound_has_its_own_octave() {
        let mut m = VintageMellotron::new(48000.0);
        m.set(9, 1.0);
        m.set(10, -1.0);
        m.note_on(60, 100);
        assert_eq!(m.playing[60], Some((72, 48)));
        m.set(9, 0.0);
        m.note_off(60);
        assert_eq!(m.playing[60], None);
        play(&mut m, 3.0);
        assert!(rms(&play(&mut m, 0.2)) < 1e-4, "the note struck an octave up was released");
    }

    #[test]
    fn tape_speed_moves_the_pitch() {
        let count = |speed: f32| {
            let mut m = VintageMellotron::new(48000.0);
            m.set(7, 5.0);
            m.set(11, speed);
            m.note_on(69, 100);
            play(&mut m, 0.5);
            zero_crossings(&play(&mut m, 1.0)) as f32
        };
        let (slow, mid, fast) = (count(-1.0), count(0.0), count(1.0));
        assert!(slow < mid * 0.9 && fast > mid * 1.1, "{slow} {mid} {fast}");
    }

    #[test]
    fn fixed_velocity_and_bend_range() {
        let level = |fixed: f32, vel: u8| {
            let mut m = VintageMellotron::new(48000.0);
            m.set(13, fixed);
            m.note_on(60, vel);
            play(&mut m, 0.4);
            rms(&play(&mut m, 0.3))
        };
        assert!((level(1.0, 20) - level(1.0, 127)).abs() < level(1.0, 127) * 0.02);
        assert!(level(0.0, 20) < level(0.0, 127) * 0.9);
        let mut m = VintageMellotron::new(48000.0);
        m.set(12, 0.0);
        m.set(7, 5.0);
        m.note_on(69, 100);
        play(&mut m, 0.5);
        let a = zero_crossings(&play(&mut m, 0.5)) as f32;
        m.pitch_bend(8191);
        play(&mut m, 0.2);
        let b = zero_crossings(&play(&mut m, 0.5)) as f32;
        assert!((a - b).abs() < a * 0.02, "range 0: the wheel does nothing ({a} {b})");
    }

    #[test]
    fn the_factory_settings_choose_both_sounds() {
        let mut m = VintageMellotron::new(48000.0);
        m.set(0, 1.0);
        assert_eq!((m.get(7), m.get(8), m.get(4)), (1.0, 5.0, 0.5));
    }
}
