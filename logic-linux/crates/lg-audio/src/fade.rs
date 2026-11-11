//! Audio region fades as Logic stores them (LOGICX-FORMAT §8): Fade-In and Fade-Out lengths in
//! milliseconds, each with a curve of -99..+99 (0 straight; positive bends the fade-in to a
//! slow start and fast end), and the Fade-Out's type (Out, X crossfade, EqP equal power, X S
//! S-curve crossfade).
//!
//! The curve's exact formula isn't in the research; this uses a power law whose exponent
//! grows to 5 at ±99, which matches the screenshots' bend direction and rough depth.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FadeType {
    /// Out: a fade to silence.
    #[default]
    Out,
    /// X: a crossfade into the next region (linear).
    X,
    /// EqP: an equal-power crossfade.
    EqP,
    /// X S: an S-curve crossfade.
    XS,
}

impl FadeType {
    pub const ALL: [FadeType; 4] = [FadeType::Out, FadeType::X, FadeType::EqP, FadeType::XS];

    /// The Region inspector's pop-up names.
    pub fn name(self) -> &'static str {
        match self {
            FadeType::Out => "Out",
            FadeType::X => "X",
            FadeType::EqP => "EqP",
            FadeType::XS => "X S",
        }
    }
}

/// A fade's gain at `t` (0..1 through the fade, rising): `curve` -99..99.
pub fn curve_gain(t: f32, curve: i8) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let c = curve.clamp(-99, 99) as f32 / 99.0;
    let e = 1.0 + 4.0 * c.abs();
    if c >= 0.0 { t.powf(e) } else { 1.0 - (1.0 - t).powf(e) }
}

/// A fade's gain at `t` (0..1, rising) for a fade or crossfade type: X follows the curve (a
/// straight crossfade at 0), EqP keeps the power constant, X S is an S curve.
pub fn shaped(t: f32, kind: FadeType, curve: i8) -> f32 {
    let t = t.clamp(0.0, 1.0);
    match kind {
        FadeType::Out | FadeType::X => curve_gain(t, curve),
        FadeType::EqP => (t * std::f32::consts::FRAC_PI_2).sin(),
        FadeType::XS => t * t * (3.0 - 2.0 * t),
    }
}

/// A crossfade `ms` long between a region (`out`, its end) and the next (`into`, its start):
/// the first fades out with `kind`, the second fades in mirrored.
pub fn crossfade(out: &mut RegionFades, into: &mut RegionFades, ms: u16, kind: FadeType) {
    let kind = if kind == FadeType::Out { FadeType::X } else { kind };
    out.out_ms = ms;
    out.out_type = kind;
    out.out_curve = 0;
    into.in_ms = ms;
    into.in_type = kind;
    into.in_curve = 0;
}

/// Logic's region fades.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RegionFades {
    pub in_ms: u16,
    pub in_curve: i8,
    pub out_ms: u16,
    pub out_curve: i8,
    pub out_type: FadeType,
    /// The fade-in's shape when it is the second half of a crossfade (the region before's
    /// type mirrored; Out: the fade-in's own curve). Not in Logic's bytes: Logic keeps the
    /// type on the left region only.
    pub in_type: FadeType,
}

impl RegionFades {
    /// The gain at `sec` seconds into a region `len` seconds long.
    pub fn gain_at(&self, sec: f64, len: f64) -> f32 {
        let mut g = 1.0;
        let fin = self.in_ms as f64 / 1000.0;
        if fin > 0.0 && sec < fin {
            g *= shaped((sec / fin) as f32, self.in_type, self.in_curve);
        }
        let fout = (self.out_ms as f64 / 1000.0).min(len);
        if fout > 0.0 && sec > len - fout {
            let t = ((len - sec) / fout) as f32;
            g *= shaped(t, self.out_type, self.out_curve);
        }
        g.clamp(0.0, 1.0)
    }

    /// Applies the fades to a region's samples (interleaved), as it plays.
    pub fn apply(&self, data: &mut [f32], channels: usize, rate: u32) {
        let c = channels.max(1);
        let frames = data.len() / c;
        let len = frames as f64 / rate as f64;
        let fin = (self.in_ms as f64 / 1000.0 * rate as f64).ceil() as usize;
        let fout = (self.out_ms as f64 / 1000.0 * rate as f64).ceil() as usize;
        let touch = |f: usize, data: &mut [f32]| {
            let g = self.gain_at(f as f64 / rate as f64, len);
            for s in &mut data[f * c..f * c + c] {
                *s *= g;
            }
        };
        for f in 0..fin.min(frames) {
            touch(f, data);
        }
        for f in frames.saturating_sub(fout).max(fin.min(frames))..frames {
            touch(f, data);
        }
    }

    /// The 8 bytes of the region record's fifth unit, offsets +8..+15 (LOGICX-FORMAT §8).
    pub fn to_bytes(&self) -> [u8; 8] {
        let o = self.out_ms.to_le_bytes();
        let i = self.in_ms.to_le_bytes();
        let ty = match self.out_type {
            FadeType::Out => 0,
            FadeType::X => 1,
            FadeType::EqP => 2,
            FadeType::XS => 3,
        };
        [o[0], o[1], ty, self.out_curve as u8, i[0], i[1], 0, self.in_curve as u8]
    }

    pub fn from_bytes(b: [u8; 8]) -> RegionFades {
        let out_type = match b[2] {
            1 => FadeType::X,
            2 => FadeType::EqP,
            3 => FadeType::XS,
            _ => FadeType::Out,
        };
        RegionFades { out_ms: u16::from_le_bytes([b[0], b[1]]), out_type, out_curve: b[3] as i8, in_ms: u16::from_le_bytes([b[4], b[5]]), in_curve: b[7] as i8, in_type: FadeType::Out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_bend_the_right_way() {
        assert_eq!(curve_gain(0.0, 0), 0.0);
        assert_eq!(curve_gain(1.0, 50), 1.0);
        assert!((curve_gain(0.5, 0) - 0.5).abs() < 1e-6);
        // +50: slow start, fast end
        assert!(curve_gain(0.5, 50) < 0.3);
        assert!(curve_gain(0.5, -50) > 0.7);
        // rising for every curve
        for c in [-99i8, -30, 0, 30, 99] {
            let mut last = -1.0;
            for k in 0..=20 {
                let g = curve_gain(k as f32 / 20.0, c);
                assert!(g >= last);
                last = g;
            }
        }
    }

    #[test]
    fn logics_bytes_round_trip() {
        // R01's region: fade-in 1500 ms curve 50, fade-out 800 ms curve -30
        let f = RegionFades { in_ms: 1500, in_curve: 50, out_ms: 800, out_curve: -30, out_type: FadeType::Out, in_type: FadeType::Out };
        let b = f.to_bytes();
        assert_eq!(b, [0x20, 0x03, 0, 0xe2, 0xdc, 0x05, 0, 0x32]);
        assert_eq!(RegionFades::from_bytes(b), f);
    }

    #[test]
    fn fades_shape_the_region() {
        let rate = 1000;
        let mut data = vec![1.0f32; 2 * 2000];
        let f = RegionFades { in_ms: 500, in_curve: 0, out_ms: 250, out_curve: 0, out_type: FadeType::Out, in_type: FadeType::Out };
        f.apply(&mut data, 2, rate);
        assert_eq!(data[0], 0.0);
        assert!((data[2 * 250] - 0.5).abs() < 0.01);
        assert_eq!(data[2 * 1000], 1.0);
        assert!((data[2 * 1875] - 0.5).abs() < 0.01);
        assert!(data[2 * 1999] < 0.01);
        let eqp = RegionFades { out_ms: 1000, out_type: FadeType::EqP, ..Default::default() };
        assert!((eqp.gain_at(1.5, 2.0) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
    }

    #[test]
    fn crossfades_keep_the_level() {
        // over the overlap of two regions: the first's fade-out and the second's fade-in
        for (kind, power) in [(FadeType::X, false), (FadeType::EqP, true), (FadeType::XS, false)] {
            let (mut a, mut b) = (RegionFades::default(), RegionFades::default());
            crossfade(&mut a, &mut b, 1000, kind);
            for k in 0..=10 {
                let s = k as f64 / 10.0;
                // the first region is 4 s long: its fade-out spans 3..4 s; the second starts at 3 s
                let ga = a.gain_at(3.0 + s, 4.0);
                let gb = b.gain_at(s, 4.0);
                let sum = if power { ga * ga + gb * gb } else { ga + gb };
                assert!((sum - 1.0).abs() < 1e-4, "{kind:?} at {s}: {ga} + {gb}");
            }
        }
    }
}
