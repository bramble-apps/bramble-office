//! Alchemy's pitch correction (the Pitch element of an additive or spectral source): On, Amount
//! (how strongly partial pitches are pulled), Speed (the delay before correction takes hold, 0–100
//! % of a second) and the Pitch rule each partial snaps to, measured from its own harmonic of the
//! played note: One Note (the harmonic itself), Octaves, Fifths, Octaves & Fifths, Chromatic.

use lg_dsp::{Param, Unit};

pub const PC: u32 = 32300;
pub const ON: u32 = 0;
pub const AMOUNT: u32 = 1;
pub const SPEED: u32 = 2;
pub const RULE: u32 = 3;
pub const RULES: &[&str] = &["One Note", "Octaves", "Fifths", "Octaves & Fifths", "Chromatic"];

pub fn id(s: usize, f: u32) -> u32 {
    PC + f * 4 + s as u32
}

pub fn params() -> Vec<Param> {
    let leak = |x: String| -> &'static str { Box::leak(x.into_boxed_str()) };
    let mut v = Vec::new();
    for (s, l) in ["A", "B", "C", "D"].iter().enumerate() {
        v.push(Param::new(id(s, ON), leak(format!("Pitch Correct On {l}")), 0.0, 1.0, 0.0, Unit::OnOff));
        v.push(Param::new(id(s, AMOUNT), leak(format!("Pitch Correct Amount {l}")), 0.0, 100.0, 100.0, Unit::Percent));
        v.push(Param::new(id(s, SPEED), leak(format!("Pitch Correct Speed {l}")), 0.0, 100.0, 0.0, Unit::Percent));
        v.push(Param::menu(id(s, RULE), leak(format!("Pitch Correct Pitch {l}")), RULES, 4.0));
    }
    v
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PitchCfg {
    pub on: bool,
    pub amount: f32,
    pub speed: f32,
    pub rule: u8,
}

impl PitchCfg {
    pub fn read(p: &dyn Fn(u32) -> f32, s: usize) -> PitchCfg {
        PitchCfg { on: p(id(s, ON)) >= 0.5, amount: p(id(s, AMOUNT)) / 100.0, speed: p(id(s, SPEED)) / 100.0, rule: p(id(s, RULE)).round().clamp(0.0, 4.0) as u8 }
    }
    /// How much correction applies `t` seconds into the note (Speed: the delay, up to 1 s).
    pub fn weight(&self, t: f32) -> f32 {
        if !self.on {
            return 0.0;
        }
        let ramp = if self.speed <= 0.0 { 1.0 } else { (t / self.speed).clamp(0.0, 1.0) };
        self.amount.clamp(0.0, 1.0) * ramp
    }
}

/// The allowed pitch nearest `f` for a partial whose harmonic is at `h` Hz.
pub fn snap(f: f32, h: f32, rule: u8) -> f32 {
    if f <= 0.0 || h <= 0.0 {
        return f;
    }
    let st = 12.0 * (f / h).log2();
    let to = match rule {
        0 => 0.0,
        1 => (st / 12.0).round() * 12.0,
        // the nearest fifth (a stack of fifths from the harmonic)
        2 => (st / 7.0).round() * 7.0,
        // the nearest octave or octave-and-a-fifth
        3 => {
            let o = (st / 12.0).floor() * 12.0;
            [o, o + 7.0, o + 12.0].into_iter().min_by(|a, b| (a - st).abs().total_cmp(&(b - st).abs())).unwrap_or(st)
        }
        _ => st.round(),
    };
    h * 2f32.powf(to / 12.0)
}

/// Pulls a partial's frequency `f` toward its snapped pitch by `w` (0–1).
pub fn correct(f: f32, h: f32, rule: u8, w: f32) -> f32 {
    if w <= 0.0 {
        return f;
    }
    let t = snap(f, h, rule);
    f * (t / f).powf(w)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        let h = 200.0;
        let up = |s: f32| h * 2f32.powf(s / 12.0);
        assert!((snap(up(0.4), h, 4) - h).abs() < 1e-3, "chromatic");
        assert!((snap(up(2.6), h, 4) - up(3.0)).abs() < 1e-2);
        assert!((snap(up(5.0), h, 0) - h).abs() < 1e-3, "one note");
        assert!((snap(up(10.0), h, 1) - up(12.0)).abs() < 1e-2, "octaves");
        assert!((snap(up(6.0), h, 2) - up(7.0)).abs() < 1e-2, "fifths");
        assert!((snap(up(17.0), h, 3) - up(19.0)).abs() < 1e-2, "octaves & fifths");
        // Amount and Speed
        let c = PitchCfg { on: true, amount: 0.5, speed: 0.2, rule: 4 };
        assert_eq!(c.weight(0.0), 0.0);
        assert!((c.weight(0.1) - 0.25).abs() < 1e-6 && c.weight(1.0) == 0.5);
        let f = correct(up(0.4), h, 4, 0.5);
        assert!((12.0 * (f / h).log2() - 0.2).abs() < 1e-3);
        assert_eq!(PitchCfg { on: false, ..c }.weight(1.0), 0.0);
    }
}
