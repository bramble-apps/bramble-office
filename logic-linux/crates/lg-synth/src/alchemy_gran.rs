//! Alchemy's granular element settings: Size (grain length, 2–230 ms) and Density (grains per
//! Size: Density 2 starts a grain every Size / 2), or — with Sync on — pitch-synchronous grains
//! and Formant Shift (±36 semitones); RTime (random offset of where grains are taken, default
//! 3 %), RPan (random grain pan, stereo only), the Grain Shape (each grain's envelope), Volume,
//! and the taps: Num Taps parallel grain streams, Tap Interval (±12 semitones per tap), Tap Delay
//! (up to 0.3 s per tap), Tap Spacing (position offset, % of the sample) and StOffset (stereo
//! spread, stereo only); tap levels are fixed (TAP_VOL).

use lg_dsp::{Param, Unit};

pub const GRAINS: usize = 48;
pub const GR: u32 = 32200;
/// Field f of source s is GR + f × 4 + s.
pub const SIZE: u32 = 0;
pub const DENSITY: u32 = 1;
pub const RTIME: u32 = 2;
pub const RPAN: u32 = 3;
pub const SYNC: u32 = 4;
pub const FORMANT: u32 = 5;
pub const SHAPE: u32 = 6;
pub const TAPS: u32 = 7;
pub const INTERVAL: u32 = 8;
pub const DELAY: u32 = 9;
pub const SPACING: u32 = 10;
pub const STOFFSET: u32 = 11;
pub const VOL: u32 = 12;
pub const TAP_VOL: [f32; 8] = [1.0, 0.92, 0.87, 0.78, 0.71, 0.65, 0.57, 0.50];
pub const SHAPES: &[&str] = &["Hann", "Triangle", "Gauss", "Tukey", "Rectangle", "Exp Attack", "Exp Decay"];

pub fn id(s: usize, f: u32) -> u32 {
    GR + f * 4 + s as u32
}

pub fn params() -> Vec<Param> {
    let leak = |x: String| -> &'static str { Box::leak(x.into_boxed_str()) };
    let mut v = Vec::new();
    for (s, l) in ["A", "B", "C", "D"].iter().enumerate() {
        let p = |f: u32, n: &str, lo: f32, hi: f32, d: f32, u: Unit| Param::new(id(s, f), leak(format!("Granular {n} {l}")), lo, hi, d, u);
        v.push(p(SIZE, "Size", 2.0, 230.0, 60.0, Unit::Ms));
        v.push(p(DENSITY, "Density", 0.5, 20.0, 4.0, Unit::None));
        v.push(p(RTIME, "RTime", 0.0, 100.0, 3.0, Unit::Percent));
        v.push(p(RPAN, "RPan", 0.0, 100.0, 0.0, Unit::Percent));
        v.push(p(SYNC, "Sync", 0.0, 1.0, 0.0, Unit::OnOff));
        v.push(p(FORMANT, "Formant Shift", -36.0, 36.0, 0.0, Unit::Semitones));
        v.push(Param::menu(id(s, SHAPE), leak(format!("Granular Grain Shape {l}")), SHAPES, 0.0));
        v.push(p(TAPS, "Num Taps", 1.0, 8.0, 1.0, Unit::None));
        v.push(p(INTERVAL, "Tap Interval", -12.0, 12.0, 0.0, Unit::Semitones));
        v.push(p(DELAY, "Tap Delay", 0.0, 0.3, 0.0, Unit::Seconds));
        v.push(p(SPACING, "Tap Spacing", 0.0, 100.0, 0.0, Unit::Percent));
        v.push(p(STOFFSET, "StOffset", 0.0, 100.0, 0.0, Unit::Percent));
        v.push(p(VOL, "Granular Volume", 0.0, 1.0, 1.0, Unit::Ratio));
    }
    v
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GranCfg {
    pub size_ms: f32,
    pub density: f32,
    /// RTime and RPan as fractions; Tap Spacing and StOffset too.
    pub rtime: f32,
    pub rpan: f32,
    pub sync: bool,
    pub formant: f32,
    pub shape: u8,
    pub taps: u8,
    pub interval: f32,
    pub tap_delay: f32,
    pub spacing: f32,
    pub st_offset: f32,
    pub vol: f32,
}

impl GranCfg {
    pub fn read(p: &dyn Fn(u32) -> f32, s: usize) -> GranCfg {
        let q = |f: u32| p(id(s, f));
        GranCfg {
            size_ms: q(SIZE).clamp(2.0, 230.0),
            density: q(DENSITY).clamp(0.5, 20.0),
            rtime: q(RTIME) / 100.0,
            rpan: q(RPAN) / 100.0,
            sync: q(SYNC) >= 0.5,
            formant: q(FORMANT).clamp(-36.0, 36.0),
            shape: q(SHAPE).round().max(0.0) as u8,
            taps: q(TAPS).round().clamp(1.0, 8.0) as u8,
            interval: q(INTERVAL).clamp(-12.0, 12.0),
            tap_delay: q(DELAY).clamp(0.0, 0.3),
            spacing: q(SPACING) / 100.0,
            st_offset: q(STOFFSET) / 100.0,
            vol: q(VOL),
        }
    }
    /// Samples between grain starts (classic mode) at `rate`.
    pub fn every(&self, rate: f32) -> f32 {
        self.size_ms / 1000.0 * rate / self.density.max(0.1)
    }
}

/// A grain's envelope at x (0–1 through the grain).
pub fn window(shape: u8, x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    match shape {
        1 => 1.0 - (2.0 * x - 1.0).abs(),
        2 => (-((x - 0.5) * 6.0).powi(2) / 2.0).exp(),
        3 => {
            if x < 0.2 {
                0.5 - 0.5 * (std::f32::consts::PI * x / 0.2).cos()
            } else if x > 0.8 {
                0.5 - 0.5 * (std::f32::consts::PI * (1.0 - x) / 0.2).cos()
            } else {
                1.0
            }
        }
        4 => 1.0,
        5 => x * x * (1.0 - x).min(0.05) * 20.0,
        6 => (1.0 - x).powi(2) * x.min(0.05) * 20.0,
        _ => 0.5 - 0.5 * (2.0 * std::f32::consts::PI * x).cos(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_and_size_set_the_grain_rate() {
        let mut c = GranCfg { size_ms: 100.0, density: 1.0, ..Default::default() };
        assert_eq!(c.every(1000.0), 100.0);
        c.density = 2.0;
        assert_eq!(c.every(1000.0), 50.0);
    }

    #[test]
    fn defaults_and_shapes() {
        let ps = params();
        let d = |f: u32| ps.iter().find(|p| p.id == id(0, f)).unwrap();
        assert_eq!(d(RTIME).default, 3.0);
        assert_eq!((d(SIZE).min, d(SIZE).max), (2.0, 230.0));
        assert_eq!((d(FORMANT).min, d(FORMANT).max), (-36.0, 36.0));
        assert_eq!(d(DELAY).max, 0.3);
        assert_eq!(TAP_VOL, [1.0, 0.92, 0.87, 0.78, 0.71, 0.65, 0.57, 0.50]);
        for k in 0..SHAPES.len() as u8 {
            assert!(window(k, 0.5) > 0.0, "{k}");
        }
        assert_eq!(window(0, 0.0), 0.0);
    }
}
