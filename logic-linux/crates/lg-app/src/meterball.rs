//! Settings ▸ View ▸ Mixer ▸ Peak Hold Time and Return Time: the channel strip meters'
//! ballistics. The meter falls at the Return Time's rate (dB per second) and a peak segment
//! stays at the highest level for the Peak Hold Time (800 ms, 2 s, 4 s or until reset);
//! clicking a peak level display clears the holds with the peaks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use crate::settings::Values;

pub const COMMANDS: &[&str] = &[];

pub const PEAK_HOLD: &str = "View / Mixer / Peak Hold Time:";
pub const RETURN_TIME: &str = "View / Mixer / Return Time:";

/// The Return Time's rate in dB/s ("IEC Type I (11.8 dB/s)—Recommended" → 11.8).
pub fn return_rate(s: &str) -> f32 {
    s.split('(').nth(1).and_then(|x| x.split_whitespace().next()).and_then(|x| x.parse().ok()).unwrap_or(11.8)
}

/// The Peak Hold Time in seconds (None: Infinite, until reset).
pub fn hold_secs(s: &str) -> Option<f32> {
    match s.trim() {
        "Infinite" => None,
        "2 s" => Some(2.0),
        "4 s" => Some(4.0),
        _ => Some(0.8),
    }
}

fn db(x: f32) -> f32 {
    if x <= 1e-6 { -120.0 } else { 20.0 * x.log10() }
}

fn lin(d: f32) -> f32 {
    if d <= -119.0 { 0.0 } else { 10f32.powf(d / 20.0) }
}

/// One meter's state: the level shown and the held peak (dB), times in seconds.
#[derive(Clone, Copy, Debug)]
pub struct Ball {
    pub shown: f32,
    pub peak: f32,
    peak_at: f64,
    at: f64,
}

impl Default for Ball {
    fn default() -> Ball {
        Ball { shown: -120.0, peak: -120.0, peak_at: 0.0, at: 0.0 }
    }
}

impl Ball {
    /// A new level `raw` (dB) at time `now`: rises at once, falls at `rate` dB/s; the peak holds
    /// for `hold` seconds, then falls with the meter.
    pub fn step(&mut self, raw: f32, now: f64, rate: f32, hold: Option<f32>) {
        let dt = (now - self.at).max(0.0) as f32;
        self.at = now;
        self.shown = raw.max(self.shown - rate * dt).max(-120.0);
        if raw >= self.peak {
            self.peak = raw;
            self.peak_at = now;
        } else if hold.is_some_and(|h| (now - self.peak_at) as f32 > h) {
            self.peak = self.shown;
            self.peak_at = now;
        }
    }
}

thread_local! {
    static BALLS: RefCell<HashMap<usize, Ball>> = RefCell::new(HashMap::new());
    static T0: Instant = Instant::now();
}

/// Strip `s`'s meter level this frame (linear, as the engine's), after the ballistics.
pub fn shown(v: &Values, s: usize, level: f32) -> f32 {
    let now = T0.with(|t| t.elapsed().as_secs_f64());
    let (rate, hold) = (return_rate(&v.get(RETURN_TIME)), hold_secs(&v.get(PEAK_HOLD)));
    BALLS.with(|b| {
        let mut b = b.borrow_mut();
        let ball = b.entry(s).or_default();
        ball.step(db(level), now, rate, hold);
        lin(ball.shown)
    })
}

/// Strip `s`'s held peak (linear; 0: none).
pub fn held(s: usize) -> f32 {
    BALLS.with(|b| b.borrow().get(&s).map_or(0.0, |x| lin(x.peak)))
}

/// Clicking a peak level display: every hold starts again.
pub fn reset_holds() {
    BALLS.with(|b| b.borrow_mut().values_mut().for_each(|x| x.peak = x.shown));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_parse() {
        assert_eq!(return_rate("IEC Type I (11.8 dB/s)—Recommended"), 11.8);
        assert_eq!(return_rate("Very Slow (4 dB/s)"), 4.0);
        assert_eq!(return_rate("Very Fast (50 dB/s)"), 50.0);
        assert_eq!(hold_secs("800 ms"), Some(0.8));
        assert_eq!(hold_secs("4 s"), Some(4.0));
        assert_eq!(hold_secs("Infinite"), None);
    }

    #[test]
    fn falls_at_the_return_time() {
        let mut b = Ball::default();
        b.step(0.0, 0.0, 11.8, Some(0.8));
        // silence: one second later 11.8 dB down
        b.step(-120.0, 1.0, 11.8, Some(0.8));
        assert!((b.shown + 11.8).abs() < 1e-4);
        let mut fast = Ball::default();
        fast.step(0.0, 0.0, 50.0, Some(0.8));
        fast.step(-120.0, 1.0, 50.0, Some(0.8));
        assert!((fast.shown + 50.0).abs() < 1e-4);
        // a louder level rises at once
        b.step(-3.0, 1.1, 11.8, Some(0.8));
        assert_eq!(b.shown, -3.0);
    }

    #[test]
    fn peak_holds() {
        let mut b = Ball::default();
        b.step(-1.0, 0.0, 11.8, Some(2.0));
        b.step(-120.0, 1.5, 11.8, Some(2.0));
        assert_eq!(b.peak, -1.0);
        b.step(-120.0, 2.5, 11.8, Some(2.0));
        assert!(b.peak < -1.0, "released after the hold time");
        // Infinite: stays until reset
        let mut c = Ball::default();
        c.step(-2.0, 0.0, 11.8, None);
        c.step(-120.0, 100.0, 11.8, None);
        assert_eq!(c.peak, -2.0);
    }

    #[test]
    fn reset_clears_holds() {
        let v = Values::default();
        let _ = shown(&v, 991, 1.0);
        BALLS.with(|b| b.borrow_mut().get_mut(&991).unwrap().shown = -30.0);
        assert!(held(991) > 0.9);
        reset_holds();
        assert!((held(991) - lin(-30.0)).abs() < 1e-4);
    }
}
