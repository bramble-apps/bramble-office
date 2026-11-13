//! Project Settings ▸ General ▸ Use musical grid off: the Tracks area's ruler reads in time
//! (minutes:seconds at round steps) instead of bar numbers.

use crate::app::App;

/// The step between labels (seconds) for at least `min_px` between them.
pub fn step_secs(px_per_sec: f64, min_px: f64) -> f64 {
    const STEPS: [f64; 14] = [0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0];
    STEPS.iter().copied().find(|s| s * px_per_sec >= min_px).unwrap_or(3600.0)
}

/// "0:05", "1:30", "1:00:00", or with tenths when the step is under a second ("0:02.5").
pub fn label(secs: f64, step: f64) -> String {
    let s = secs.max(0.0);
    let whole = s.floor() as u64;
    let (h, m, sec) = (whole / 3600, whole / 60 % 60, whole % 60);
    let base = if h > 0 { format!("{h}:{m:02}:{sec:02}") } else { format!("{m}:{sec:02}") };
    if step < 1.0 {
        let tenths = ((s - s.floor()) * 10.0).round() as u64 % 10;
        format!("{base}.{tenths}")
    } else {
        base
    }
}

/// The tick at `secs` from bar 1 (through the tempo changes).
pub fn tick_at(app: &App, secs: f64) -> f64 {
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    while app.seconds_at(hi) < secs && hi < 1e12 {
        hi *= 2.0;
    }
    for _ in 0..50 {
        let mid = (lo + hi) / 2.0;
        if app.seconds_at(mid) < secs { lo = mid } else { hi = mid }
    }
    (lo + hi) / 2.0
}

/// The labels between two ticks: (tick, text).
pub fn labels(app: &App, from_tick: f64, to_tick: f64, px_per_tick: f64) -> Vec<(f64, String)> {
    let (s0, s1) = (app.seconds_at(from_tick.max(0.0)), app.seconds_at(to_tick.max(0.0)));
    if s1 <= s0 {
        return Vec::new();
    }
    let px_per_sec = (to_tick - from_tick) * px_per_tick / (s1 - s0);
    let step = step_secs(px_per_sec, 56.0);
    let mut out = Vec::new();
    let mut s = (s0 / step).floor() * step;
    while s <= s1 && out.len() < 400 {
        if s >= 0.0 {
            out.push((tick_at(app, s), label(s, step)));
        }
        s += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_and_labels() {
        assert_eq!(step_secs(100.0, 56.0), 1.0);
        assert_eq!(step_secs(10.0, 56.0), 10.0);
        assert_eq!(label(65.0, 1.0), "1:05");
        assert_eq!(label(3725.0, 5.0), "1:02:05");
        assert_eq!(label(2.5, 0.5), "0:02.5");
    }
}
