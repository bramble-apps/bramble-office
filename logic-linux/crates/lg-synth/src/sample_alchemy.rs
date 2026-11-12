//! Sample Alchemy stand-in (stub: filled in by its engine work).

use lg_dsp::{Instrument, Param, Plugin};

pub static PARAMS: &[Param] = &[];

pub struct SampleAlchemy {
    #[allow(dead_code)]
    rate: f32,
}

impl SampleAlchemy {
    pub fn new(rate: f32) -> SampleAlchemy {
        SampleAlchemy { rate }
    }
}

impl Plugin for SampleAlchemy {
    fn params(&self) -> &[Param] {
        PARAMS
    }
    fn get(&self, _id: u32) -> f32 {
        0.0
    }
    fn set(&mut self, _id: u32, _v: f32) {}
    fn reset(&mut self) {}
}

impl Instrument for SampleAlchemy {
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

impl SampleAlchemy {
    /// Loads the sample (mono) the instrument plays, recorded at `rate` Hz.
    pub fn set_sample(&mut self, _mono: Vec<f32>, _rate: f32) {}
}
