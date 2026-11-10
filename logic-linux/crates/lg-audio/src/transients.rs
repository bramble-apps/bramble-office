//! Transients (Logic's transient markers, Flex Time's slice points): an onset-strength curve
//! (log spectral flux, half-wave rectified) peak-picked against a moving threshold, each onset
//! then placed on the sample where the sound starts to rise.

use crate::fft::{fft, hann};

/// The analysis frame and hop (frames of audio) for a sample rate: about 21 ms / 5 ms.
pub fn frame_hop(rate: u32) -> (usize, usize) {
    let n = ((rate as f64 * 0.0213) as usize).next_power_of_two();
    (n, n / 4)
}

/// The onset-strength curve of mono samples: one value per hop (see [`frame_hop`]).
pub fn onset_strength(mono: &[f32], rate: u32) -> Vec<f32> {
    let (n, hop) = frame_hop(rate);
    let w = hann(n);
    let bins = n / 2;
    let mut prev = vec![0f32; bins];
    let mut out = Vec::with_capacity(mono.len() / hop + 1);
    let mut re = vec![0f32; n];
    let mut im = vec![0f32; n];
    let mut pos = 0usize;
    while pos < mono.len() {
        // frame centred on pos
        for i in 0..n {
            let k = pos as isize + i as isize - n as isize / 2;
            re[i] = if k >= 0 && (k as usize) < mono.len() { mono[k as usize] * w[i] } else { 0.0 };
            im[i] = 0.0;
        }
        fft(&mut re, &mut im);
        let mut flux = 0.0;
        for b in 1..bins {
            let m = (1.0 + 100.0 * (re[b] * re[b] + im[b] * im[b]).sqrt()).ln();
            let d = m - prev[b];
            if d > 0.0 {
                flux += d;
            }
            prev[b] = m;
        }
        out.push(flux / bins as f32);
        pos += hop;
    }
    out
}

/// Peaks of an onset curve: local maxima over ±`wide` hops that stand `delta` above the mean
/// around them, at least `gap` hops apart. Returns hop indexes.
pub fn pick_peaks(env: &[f32], delta: f32, wide: usize, gap: usize) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    let max = env.iter().copied().fold(0.0f32, f32::max);
    if max <= 0.0 {
        return out;
    }
    for i in 0..env.len() {
        let a = i.saturating_sub(wide);
        let b = (i + wide + 1).min(env.len());
        let v = env[i];
        if env[a..b].iter().any(|&x| x > v) {
            continue;
        }
        let ma = i.saturating_sub(wide * 3);
        let mb = (i + wide + 1).min(env.len());
        let mean = env[ma..mb].iter().sum::<f32>() / (mb - ma) as f32;
        if v < mean + delta * max {
            continue;
        }
        if let Some(&last) = out.last() {
            if i - last < gap {
                if env[last] < v {
                    out.pop();
                } else {
                    continue;
                }
            }
        }
        out.push(i);
    }
    out
}

/// Transients of interleaved samples, as frame positions. `sensitivity` 0..1 (Logic's
/// "Increase / Decrease Number of Transients"; 0.5 is the default).
pub fn detect(data: &[f32], channels: usize, rate: u32, sensitivity: f32) -> Vec<usize> {
    let c = channels.max(1);
    let mono: Vec<f32> = data.chunks(c).map(|f| f.iter().sum::<f32>() / c as f32).collect();
    detect_mono(&mono, rate, sensitivity)
}

pub fn detect_mono(mono: &[f32], rate: u32, sensitivity: f32) -> Vec<usize> {
    let (n, hop) = frame_hop(rate);
    let mut env = onset_strength(mono, rate);
    // the frames reaching past the end (the cut there isn't an onset)
    let whole = mono.len().saturating_sub(n / 2) / hop;
    env.truncate(whole.max(1));
    let s = sensitivity.clamp(0.0, 1.0);
    let delta = 0.30 - 0.27 * s;
    let gap = ((0.045 * rate as f64) as usize / hop).max(1);
    let peaks = pick_peaks(&env, delta, 3, gap);
    let mut out: Vec<usize> = peaks.into_iter().map(|p| refine(mono, p * hop, hop, rate)).collect();
    out.dedup();
    out
}

/// The sample where an onset near `at` starts: the quiet point before the rise to its peak.
fn refine(mono: &[f32], at: usize, hop: usize, rate: u32) -> usize {
    let lo = at.saturating_sub(hop * 3);
    let hi = (at + hop * 3).min(mono.len());
    if lo >= hi {
        return at.min(mono.len());
    }
    let peak = mono[lo..hi].iter().fold(0.0f32, |a, b| a.max(b.abs()));
    if peak <= 0.0 {
        return at;
    }
    // the level before it (its floor), the first sample clearly above that floor
    let pre = &mono[lo.saturating_sub(hop * 2)..lo];
    let floor = pre.iter().fold(0.0f32, |a, b| a.max(b.abs()));
    let th = floor + (peak - floor) * 0.1;
    let first = (lo..hi).find(|&i| mono[i].abs() > th).unwrap_or(at);
    // back to the zero crossing before it (at most 1 ms), as Logic draws the marker
    let back = (rate as usize / 1000).max(1);
    let mut i = first;
    while i > first.saturating_sub(back) && i > 0 && mono[i - 1].signum() == mono[i].signum() && mono[i - 1] != 0.0 {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decaying noise bursts at the given seconds.
    pub fn clicks(rate: u32, secs: f64, at: &[f64]) -> Vec<f32> {
        let n = (rate as f64 * secs) as usize;
        let mut out = vec![0f32; n];
        let mut seed = 12345u32;
        for &t in at {
            let s = (t * rate as f64) as usize;
            for k in 0..(rate as usize / 10) {
                if s + k >= n {
                    break;
                }
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let noise = ((seed >> 16) as f32 / 32768.0) - 1.0;
                out[s + k] += 0.8 * noise * (-(k as f32) / (rate as f32 * 0.015)).exp();
            }
        }
        out
    }

    #[test]
    fn finds_clicks_within_a_few_ms() {
        let rate = 48000;
        let at = [0.25, 0.71, 1.0, 1.5, 1.62, 2.4];
        let x = clicks(rate, 3.0, &at);
        let found = detect_mono(&x, rate, 0.5);
        assert_eq!(found.len(), at.len(), "{found:?}");
        for (f, t) in found.iter().zip(at) {
            let err = (*f as f64 / rate as f64 - t).abs();
            assert!(err < 0.004, "{f} vs {t}: {err}");
        }
    }

    #[test]
    fn silence_and_a_steady_tone_have_none() {
        let rate = 44100;
        assert!(detect_mono(&vec![0.0; rate as usize], rate, 0.5).is_empty());
        // a sine that's on from the start: only its start
        let tone: Vec<f32> = (0..rate as usize * 2).map(|i| 0.5 * (i as f32 * 0.06).sin()).collect();
        let f = detect_mono(&tone, rate, 0.5);
        assert!(f.len() <= 1, "{f:?} {:?}", onset_strength(&tone, rate).iter().take(12).collect::<Vec<_>>());
    }

    #[test]
    fn sensitivity_changes_the_count() {
        let rate = 48000;
        // loud and soft hits
        let mut x = clicks(rate, 2.0, &[0.2, 1.2]);
        let soft = clicks(rate, 2.0, &[0.7, 1.7]);
        for (a, b) in x.iter_mut().zip(soft) {
            *a += b * 0.08;
        }
        let few = detect_mono(&x, rate, 0.0).len();
        let many = detect_mono(&x, rate, 1.0).len();
        assert!(few <= many && many >= 3, "{few} {many}");
    }
}
