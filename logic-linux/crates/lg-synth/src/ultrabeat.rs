//! Ultrabeat stand-in (stub: filled in by its engine work).

use lg_dsp::{Instrument, Param, Plugin};

pub static PARAMS: &[Param] = &[];

pub struct Ultrabeat {
    #[allow(dead_code)]
    rate: f32,
}

impl Ultrabeat {
    pub fn new(rate: f32) -> Ultrabeat {
        Ultrabeat { rate }
    }
}

impl Plugin for Ultrabeat {
    fn params(&self) -> &[Param] {
        PARAMS
    }
    fn get(&self, _id: u32) -> f32 {
        0.0
    }
    fn set(&mut self, _id: u32, _v: f32) {}
    fn reset(&mut self) {}
}

impl Instrument for Ultrabeat {
    fn note_on(&mut self, _pitch: u8, _velocity: u8) {}
    fn note_off(&mut self, _pitch: u8) {}
    fn render(&mut self, _left: &mut [f32], _right: &mut [f32]) {}
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
