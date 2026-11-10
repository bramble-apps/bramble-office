//! Vintage Mellotron stand-in: tape-replay voices (eight-second tapes, wow and flutter, tape hiss
//! and roll-off) from the Ensemble engine, with the ten factory sounds (two tapes layered for the
//! combination frames, as on the original's dual-track tapes).

use crate::ensemble::StudioEnsemble;
use lg_dsp::{Instrument, Param, Plugin, Unit};

/// Logic's Vintage Mellotron factory settings (#default = 3 Violins).
pub const SOUNDS: &[&str] = &[
    "3 Violins",
    "8 Choir & Flute",
    "Boys Choir",
    "Brass",
    "Cello",
    "Flute",
    "Flutes",
    "GC 3 Brass",
    "Mixed Choir",
    "Strings & Flute",
];

pub static PARAMS: &[Param] = &[
    Param::menu(0, "Sound", SOUNDS, 0.0),
    Param::new(1, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(2, "Tone", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(4, "Balance", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(5, "Attack", 0.0, 1.0, 0.1, Unit::Ratio),
    Param::new(6, "Release", 0.0, 1.0, 0.25, Unit::Ratio),
];

/// (ensemble model, brightness, section size) for layer A and optional layer B.
fn layers(sound: usize) -> ((f32, f32, f32), Option<(f32, f32, f32)>) {
    match sound {
        1 => ((13.0, 0.5, 3.0), Some((12.0, 0.5, 1.0))),
        2 => ((18.0, 0.5, 3.0), None),
        3 => ((16.0, 0.5, 3.0), None),
        4 => ((17.0, 0.35, 1.0), None),
        5 => ((12.0, 0.5, 1.0), None),
        6 => ((12.0, 0.55, 3.0), None),
        7 => ((16.0, 0.75, 3.0), None),
        8 => ((13.0, 0.5, 3.0), None),
        9 => ((11.0, 0.5, 3.0), Some((12.0, 0.5, 1.0))),
        _ => ((11.0, 0.5, 3.0), None),
    }
}

pub struct VintageMellotron {
    params: crate::poly::Params,
    a: StudioEnsemble,
    b: StudioEnsemble,
    layered: bool,
}

impl VintageMellotron {
    pub fn new(rate: f32) -> VintageMellotron {
        let mut m = VintageMellotron {
            params: crate::poly::Params::new(PARAMS),
            a: StudioEnsemble::new(rate),
            b: StudioEnsemble::new(rate),
            layered: false,
        };
        m.apply();
        m
    }
    fn apply(&mut self) {
        let p = |i| self.params.get(i);
        let (la, lb) = layers(p(0).round() as usize);
        self.layered = lb.is_some();
        let bal = p(4);
        for (ens, l, gain) in [
            (
                &mut self.a,
                Some(la),
                1.0 - if lb.is_some() { bal * 0.5 } else { 0.0 },
            ),
            (&mut self.b, lb, bal),
        ] {
            let Some((model, bright, size)) = l else {
                continue;
            };
            ens.set(0, model);
            ens.set(3, (bright + (p(2) - 0.5)).clamp(0.0, 1.0));
            ens.set(4, size);
            ens.set(1, 0.005 + p(5) * 0.6);
            ens.set(2, 0.05 + p(6) * 1.2);
            ens.set(
                7,
                if p(0).round() as usize == 6 {
                    0.7
                } else {
                    0.15
                },
            );
            ens.set(8, 0.15);
            ens.set(10, p(1) + lg_dsp::gain_to_db(gain.max(0.01)));
        }
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
        if self.params.set(id, v) {
            self.apply();
        }
    }
    fn reset(&mut self) {
        self.a.reset();
        self.b.reset();
        self.apply();
    }
    fn tail(&self) -> f32 {
        2.0
    }
}

impl Instrument for VintageMellotron {
    fn note_on(&mut self, p: u8, v: u8) {
        self.a.note_on(p, v);
        if self.layered {
            self.b.note_on(p, v);
        }
    }
    fn note_off(&mut self, p: u8) {
        self.a.note_off(p);
        self.b.note_off(p);
    }
    fn controller(&mut self, cc: u8, v: u8) {
        self.a.controller(cc, v);
        self.b.controller(cc, v);
    }
    fn pitch_bend(&mut self, v: i16) {
        self.a.pitch_bend(v);
        self.b.pitch_bend(v);
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        // tape speed: a bend-like offset on top of the player's bend is approximated by the player's bend only
        self.a.render(l, r);
        self.b.render(l, r);
    }
}
