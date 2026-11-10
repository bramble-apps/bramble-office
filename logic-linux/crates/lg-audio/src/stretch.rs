//! Time stretch and pitch shift (Flex Time / Flex Pitch groundwork, the Time and Pitch Machine).
//!
//! - [`stretch`]: WSOLA (waveform-similarity overlap-add): Hann frames of ~30 ms laid half a
//!   frame apart in the output, each read from near its nominal input place where it best
//!   continues the one before (cross-correlation on a 4x decimated mono mix, so every channel
//!   moves together and stereo stays put);
//! - [`pitch_shift`]: stretched by the pitch ratio, then resampled back to the length;
//! - [`warp`]: piecewise stretching between anchor pairs (Flex markers);
//! - [`detect_pitch`]: YIN's fundamental frequency of a stretch of samples.

/// Stretches interleaved samples by `ratio` (2.0: twice as long, same pitch).
pub fn stretch(data: &[f32], channels: usize, rate: u32, ratio: f64) -> Vec<f32> {
    let c = channels.max(1);
    let frames = data.len() / c;
    let ratio = ratio.clamp(0.1, 10.0);
    if (ratio - 1.0).abs() < 1e-9 || frames == 0 {
        return data.to_vec();
    }
    let n = ((rate as f64 * 0.03) as usize).max(64) & !1;
    let hs = n / 2;
    let ha = hs as f64 / ratio;
    let tol = n / 4;
    let out_frames = (frames as f64 * ratio).round() as usize;
    let win: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()).collect();
    // the decimated mono mix the search reads
    const DEC: usize = 4;
    let mono: Vec<f32> = (0..frames.div_ceil(DEC)).map(|k| {
        let f = k * DEC;
        (0..c).map(|ch| data.get(f * c + ch).copied().unwrap_or(0.0)).sum::<f32>()
    }).collect();
    let m = |f: isize| if f >= 0 { mono.get(f as usize / DEC).copied().unwrap_or(0.0) } else { 0.0 };
    let mut out = vec![0f32; (out_frames + n) * c];
    let mut norm = vec![0f32; out_frames + n];
    let mut prev: isize = 0;
    let mut k = 0usize;
    loop {
        let out_pos = k * hs;
        if out_pos >= out_frames {
            break;
        }
        let nominal = (k as f64 * ha).round() as isize;
        let chosen = if k == 0 {
            0
        } else {
            // the natural continuation of the last frame
            let nat = prev + hs as isize;
            let mut best = nominal;
            let mut best_v = f32::MIN;
            let mut d = -(tol as isize);
            while d <= tol as isize {
                let cand = nominal + d;
                if cand >= 0 {
                    let mut s = 0.0;
                    let mut i = 0;
                    while i < n {
                        s += m(nat + i as isize) * m(cand + i as isize);
                        i += DEC;
                    }
                    if s > best_v {
                        best_v = s;
                        best = cand;
                    }
                }
                d += DEC as isize;
            }
            best
        };
        for i in 0..n {
            let src = chosen + i as isize;
            if src < 0 || src as usize >= frames {
                continue;
            }
            let w = win[i];
            for ch in 0..c {
                out[(out_pos + i) * c + ch] += data[src as usize * c + ch] * w;
            }
            norm[out_pos + i] += w;
        }
        prev = chosen;
        k += 1;
    }
    out.truncate(out_frames * c);
    for (f, w) in norm.iter().take(out_frames).enumerate() {
        if *w > 1e-3 {
            for ch in 0..c {
                out[f * c + ch] /= w;
            }
        }
    }
    out
}

/// Resamples interleaved samples to `frames` frames (cubic interpolation).
pub fn resample(data: &[f32], channels: usize, frames: usize) -> Vec<f32> {
    let c = channels.max(1);
    let n = data.len() / c;
    if n == 0 || frames == 0 {
        return vec![0.0; frames * c];
    }
    let step = n as f64 / frames as f64;
    let get = |f: isize, ch: usize| data[(f.clamp(0, n as isize - 1) as usize) * c + ch];
    let mut out = Vec::with_capacity(frames * c);
    for k in 0..frames {
        let x = k as f64 * step;
        let i = x.floor() as isize;
        let t = (x - i as f64) as f32;
        for ch in 0..c {
            let (p0, p1, p2, p3) = (get(i - 1, ch), get(i, ch), get(i + 1, ch), get(i + 2, ch));
            // Catmull-Rom
            let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
            let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
            let cc = -0.5 * p0 + 0.5 * p2;
            out.push(((a * t + b) * t + cc) * t + p1);
        }
    }
    out
}

/// Shifts the pitch by `cents` keeping the length.
pub fn pitch_shift(data: &[f32], channels: usize, rate: u32, cents: f64) -> Vec<f32> {
    let c = channels.max(1);
    let frames = data.len() / c;
    if cents.abs() < 0.01 {
        return data.to_vec();
    }
    let f = 2f64.powf(cents / 1200.0);
    let long = stretch(data, c, rate, f);
    resample(&long, c, frames)
}

/// Flex Time: the samples warped so each anchor's input frame lands on its output frame
/// (`anchors`: (input frame, output frame), rising in both; the file's start and end are
/// added). Each stretch between anchors is time-stretched on its own.
pub fn warp(data: &[f32], channels: usize, rate: u32, anchors: &[(usize, usize)]) -> Vec<f32> {
    let c = channels.max(1);
    let frames = data.len() / c;
    let mut pts = vec![(0usize, 0usize)];
    pts.extend(anchors.iter().copied().filter(|&(i, o)| i > 0 && i < frames && o > 0));
    let last_out = pts.last().map_or(0, |p| p.1) + frames.saturating_sub(pts.last().map_or(0, |p| p.0));
    pts.push((frames, last_out));
    let mut out = Vec::with_capacity(last_out * c);
    for w in pts.windows(2) {
        let ((i0, o0), (i1, o1)) = (w[0], w[1]);
        if i1 <= i0 || o1 <= o0 {
            continue;
        }
        let seg = &data[i0 * c..i1 * c];
        let ratio = (o1 - o0) as f64 / (i1 - i0) as f64;
        let mut s = stretch(seg, c, rate, ratio);
        s.resize((o1 - o0) * c, 0.0);
        out.extend_from_slice(&s);
    }
    out
}

/// YIN: the fundamental (Hz) of mono samples, None when unpitched (aperiodicity over 0.2).
pub fn detect_pitch(mono: &[f32], rate: u32, min_hz: f32, max_hz: f32) -> Option<f32> {
    let max_lag = (rate as f32 / min_hz.max(20.0)) as usize;
    let min_lag = ((rate as f32 / max_hz.max(min_hz + 1.0)) as usize).max(2);
    let w = mono.len().saturating_sub(max_lag + 2);
    if w < max_lag || max_lag <= min_lag {
        return None;
    }
    let mut d = vec![0f32; max_lag + 2];
    for (tau, dv) in d.iter_mut().enumerate().skip(1) {
        let mut s = 0.0;
        for i in 0..w {
            let x = mono[i] - mono[i + tau];
            s += x * x;
        }
        *dv = s;
    }
    // cumulative mean normalised difference
    let mut cm = vec![1f32; d.len()];
    let mut run = 0.0;
    for tau in 1..d.len() {
        run += d[tau];
        cm[tau] = if run > 0.0 { d[tau] * tau as f32 / run } else { 1.0 };
    }
    let mut tau = None;
    for t in min_lag..=max_lag {
        if cm[t] < 0.15 {
            let mut t = t;
            while t + 1 <= max_lag && cm[t + 1] < cm[t] {
                t += 1;
            }
            tau = Some(t);
            break;
        }
    }
    let t = tau.or_else(|| (min_lag..=max_lag).min_by(|&a, &b| cm[a].total_cmp(&cm[b])).filter(|&t| cm[t] < 0.2))?;
    let (a, b, c) = (cm[t - 1], cm[t], cm[t + 1]);
    let den = a - 2.0 * b + c;
    let off = if den.abs() > 1e-9 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
    Some(rate as f32 / (t as f32 + off))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, hz: f32, secs: f32, ch: usize) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n).flat_map(|i| std::iter::repeat_n(0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin(), ch)).collect()
    }

    fn mono_of(x: &[f32], ch: usize) -> Vec<f32> {
        x.chunks(ch).map(|f| f[0]).collect()
    }

    #[test]
    fn yin_finds_the_pitch() {
        let rate = 16000;
        for hz in [110.0, 220.0, 440.0, 880.0] {
            let f = detect_pitch(&sine(rate, hz, 0.1, 1), rate, 60.0, 1500.0).unwrap();
            assert!((f - hz).abs() / hz < 0.01, "{hz}: {f}");
        }
        assert!(detect_pitch(&vec![0.0; 3200], rate, 60.0, 1500.0).is_none());
    }

    #[test]
    fn stretch_keeps_the_pitch() {
        let rate = 16000;
        let x = sine(rate, 300.0, 1.0, 2);
        for ratio in [0.5, 1.5, 2.0] {
            let y = stretch(&x, 2, rate, ratio);
            assert_eq!(y.len() / 2, (16000.0 * ratio) as usize);
            let m = mono_of(&y, 2);
            let mid = m.len() / 2;
            let f = detect_pitch(&m[mid - 1600..mid + 1600], rate, 60.0, 1500.0).unwrap();
            assert!((f - 300.0).abs() < 4.0, "{ratio}: {f}");
            // no gaps: the level stays even
            let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
            assert!((rms(&m[1000..m.len() - 1000]) - 0.3535).abs() < 0.06, "{ratio}");
        }
    }

    #[test]
    fn pitch_shift_keeps_the_length() {
        let rate = 16000;
        let x = sine(rate, 220.0, 0.8, 1);
        let y = pitch_shift(&x, 1, rate, 1200.0);
        assert_eq!(y.len(), x.len());
        let mid = y.len() / 2;
        let f = detect_pitch(&y[mid - 1600..mid + 1600], rate, 60.0, 1500.0).unwrap();
        assert!((f - 440.0).abs() < 6.0, "{f}");
        let down = pitch_shift(&x, 1, rate, -700.0);
        let f = detect_pitch(&down[mid - 1600..mid + 1600], rate, 60.0, 1500.0).unwrap();
        assert!((f - 220.0 * 2f32.powf(-7.0 / 12.0)).abs() < 3.0, "{f}");
    }

    #[test]
    fn warp_moves_anchors() {
        let rate = 8000;
        let x = sine(rate, 200.0, 1.0, 1);
        // the middle moved later by 0.25 s: first half longer, second half shorter
        let y = warp(&x, 1, rate, &[(4000, 6000)]);
        assert_eq!(y.len(), 8000 + 2000);
        let y2 = warp(&x, 1, rate, &[]);
        assert_eq!(y2.len(), 8000);
    }
}
