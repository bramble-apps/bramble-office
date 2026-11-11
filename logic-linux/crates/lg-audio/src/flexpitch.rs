//! Flex Pitch (Logic Pro 13): a monophonic recording's pitch tracked, cut into notes, and each
//! note's edits played: pitch (whole semitones and Fine Pitch in cents), Pitch Drift (the glide
//! into and out of the note), Vibrato, Gain and Formant Shift; Pitch Correction moves notes to
//! a scale.
//!
//! - [`track`]: YIN on a 4x decimated copy (a windowed-sinc low-pass first) every 5 ms, each
//!   period then refined on the full-rate samples (sub-sample, parabolic), voiced where YIN's
//!   aperiodicity is low and the level is within 40 dB of the loudest;
//! - [`notes`]: voiced runs cut where the pitch moves to a new level and stays (vibrato and
//!   drift don't cut), at transients, and at gaps; each note's pitch is its median;
//! - [`render`]: TD-PSOLA: grains two periods long at pitch marks one period apart, laid at the
//!   new period (the formants stay where they were); Formant Shift resamples each grain; the
//!   pitch curve inside a note is its centre + drift + vibrato, each scaled by the edit.

use crate::transients;

/// The pitch track: one value per `hop` frames, centred on `k * hop`.
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub rate: u32,
    pub hop: usize,
    /// Fundamental in Hz, 0 where unvoiced.
    pub f0: Vec<f32>,
    /// RMS level (linear) around each point.
    pub level: Vec<f32>,
}

impl Track {
    /// The pitch (MIDI note number, fractional) at a frame, None where unvoiced.
    pub fn midi_at(&self, frame: f64) -> Option<f32> {
        let k = (frame / self.hop as f64).round() as usize;
        self.f0.get(k).copied().filter(|&f| f > 0.0).map(hz_to_midi)
    }

    fn hz_at(&self, frame: f64) -> f32 {
        let x = frame / self.hop as f64;
        let i = x.floor() as usize;
        let t = (x - i as f64) as f32;
        let a = self.f0.get(i).copied().unwrap_or(0.0);
        let b = self.f0.get(i + 1).copied().unwrap_or(0.0);
        match (a > 0.0, b > 0.0) {
            (true, true) => a + (b - a) * t,
            (true, false) => a,
            (false, true) => b,
            _ => 0.0,
        }
    }
}

pub fn hz_to_midi(hz: f32) -> f32 {
    69.0 + 12.0 * (hz / 440.0).log2()
}

pub fn midi_to_hz(m: f32) -> f32 {
    440.0 * 2f32.powf((m - 69.0) / 12.0)
}

/// A note of a Flex Pitch region: what was found (span, pitch, level) and its edits.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// Frames (of the analysed audio).
    pub start: usize,
    pub end: usize,
    /// As played: the median pitch (MIDI, fractional) and level (dBFS RMS).
    pub pitch: f32,
    pub level_db: f32,
    /// Edits: the pitch moved by (cents; whole semitones from dragging, Fine Pitch adds the
    /// rest), Pitch Drift and Vibrato (1 = as played, 0 = flat), Gain (dB), Formant Shift (cents).
    pub shift_cents: f32,
    pub drift: f32,
    pub vibrato: f32,
    pub gain_db: f32,
    pub formant_cents: f32,
}

impl Note {
    pub fn new(start: usize, end: usize, pitch: f32, level_db: f32) -> Note {
        Note { start, end, pitch, level_db, shift_cents: 0.0, drift: 1.0, vibrato: 1.0, gain_db: 0.0, formant_cents: 0.0 }
    }

    /// Unedited.
    pub fn is_neutral(&self) -> bool {
        self.shift_cents.abs() < 0.05 && (self.drift - 1.0).abs() < 1e-3 && (self.vibrato - 1.0).abs() < 1e-3 && self.gain_db.abs() < 0.01 && self.formant_cents.abs() < 0.05
    }

    /// The pitch it plays at (MIDI, fractional).
    pub fn target(&self) -> f32 {
        self.pitch + self.shift_cents / 100.0
    }
}

/// A low-pass for decimating by `d` (windowed sinc, Blackman).
fn lowpass_decimate(x: &[f32], d: usize) -> Vec<f32> {
    if d <= 1 {
        return x.to_vec();
    }
    let m = 8 * d;
    let fc = 0.45 / d as f64;
    let taps: Vec<f32> = (0..=2 * m).map(|i| {
        let k = i as f64 - m as f64;
        let s = if k == 0.0 { 2.0 * fc } else { (2.0 * std::f64::consts::PI * fc * k).sin() / (std::f64::consts::PI * k) };
        let w = 0.42 - 0.5 * (std::f64::consts::PI * i as f64 / m as f64).cos() + 0.08 * (2.0 * std::f64::consts::PI * i as f64 / m as f64).cos();
        (s * w) as f32
    }).collect();
    (0..x.len().div_ceil(d)).map(|k| {
        let c = (k * d) as isize;
        let mut s = 0.0;
        for (i, h) in taps.iter().enumerate() {
            let j = c + i as isize - m as isize;
            if j >= 0 && (j as usize) < x.len() {
                s += h * x[j as usize];
            }
        }
        s
    }).collect()
}

/// The pitch track of mono samples (50..1500 Hz).
pub fn track(mono: &[f32], rate: u32) -> Track {
    let hop = ((rate as f64 * 0.005) as usize).max(16);
    let d = (rate as usize / 11025).max(1);
    let x = lowpass_decimate(mono, d);
    let rd = rate as f32 / d as f32;
    let max_lag = (rd / 50.0) as usize;
    let min_lag = ((rd / 1500.0) as usize).max(2);
    let w = ((rd * 0.025) as usize).max(max_lag);
    let n = mono.len().div_ceil(hop);
    let mut f0 = vec![0f32; n];
    let mut level = vec![0f32; n];
    let mut dif = vec![0f32; max_lag + 2];
    let mut cm = vec![1f32; max_lag + 2];
    for k in 0..n {
        let centre = k * hop;
        // the level over ±hop
        let (a, b) = (centre.saturating_sub(hop), (centre + hop).min(mono.len()));
        level[k] = if b > a { (mono[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32).sqrt() } else { 0.0 };
        let s = (centre / d) as isize - (w / 2) as isize;
        if s < 0 || s as usize + w + max_lag + 1 >= x.len() {
            continue;
        }
        let s = s as usize;
        for (tau, dv) in dif.iter_mut().enumerate().skip(1) {
            let mut acc = 0.0;
            for i in 0..w {
                let e = x[s + i] - x[s + i + tau];
                acc += e * e;
            }
            *dv = acc;
        }
        let mut run = 0.0;
        for tau in 1..dif.len() {
            run += dif[tau];
            cm[tau] = if run > 0.0 { dif[tau] * tau as f32 / run } else { 1.0 };
        }
        let mut found = None;
        for t in min_lag..=max_lag {
            if cm[t] < 0.15 {
                let mut t = t;
                while t < max_lag && cm[t + 1] < cm[t] {
                    t += 1;
                }
                found = Some(t);
                break;
            }
        }
        let Some(t) = found.or_else(|| (min_lag..=max_lag).min_by(|&a, &b| cm[a].total_cmp(&cm[b])).filter(|&t| cm[t] < 0.25)) else { continue };
        // refined on the full-rate samples
        let wf = w * d;
        let sf = centre as isize - (wf / 2) as isize;
        let lag0 = t * d;
        let lo = lag0.saturating_sub(d + 1).max(2);
        let hi = lag0 + d + 1;
        if sf < 0 || sf as usize + wf + hi + 2 >= mono.len() {
            f0[k] = rd / t as f32;
            continue;
        }
        let sf = sf as usize;
        let diff = |lag: usize| -> f32 {
            let mut acc = 0.0;
            for i in 0..wf {
                let e = mono[sf + i] - mono[sf + i + lag];
                acc += e * e;
            }
            acc
        };
        let vals: Vec<f32> = (lo - 1..=hi + 1).map(diff).collect();
        let best = (1..vals.len() - 1).min_by(|&a, &b| vals[a].total_cmp(&vals[b])).unwrap_or(1);
        let (a, b, c) = (vals[best - 1], vals[best], vals[best + 1]);
        let den = a - 2.0 * b + c;
        let off = if den.abs() > 1e-12 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
        let lag = (lo - 1 + best) as f32 + off;
        f0[k] = rate as f32 / lag;
    }
    // voiced only within 40 dB of the loudest
    let top = level.iter().copied().fold(0.0f32, f32::max);
    for k in 0..n {
        if level[k] < top * 0.01 || level[k] < 1e-4 {
            f0[k] = 0.0;
        }
    }
    Track { rate, hop, f0, level }
}

/// The notes of a pitch track (`onsets`: transients, frames, that may start a note).
pub fn notes(tr: &Track, onsets: &[usize]) -> Vec<Note> {
    let n = tr.f0.len();
    let mut m: Vec<Option<f32>> = tr.f0.iter().map(|&f| (f > 0.0).then(|| hz_to_midi(f))).collect();
    // a median of 5 against octave slips and single-frame errors
    let raw = m.clone();
    for k in 0..n {
        if raw[k].is_none() {
            continue;
        }
        let mut v: Vec<f32> = (k.saturating_sub(2)..(k + 3).min(n)).filter_map(|j| raw[j]).collect();
        v.sort_by(f32::total_cmp);
        m[k] = Some(v[v.len() / 2]);
    }
    let hop = tr.hop;
    let min_frames = ((0.05 * tr.rate as f64) as usize / hop).max(3);
    let onset_k: Vec<usize> = onsets.iter().map(|&o| (o + hop / 2) / hop).collect();
    let mut out = Vec::new();
    let push = |a: usize, b: usize, out: &mut Vec<Note>| {
        if b <= a || b - a < min_frames {
            return;
        }
        // the median of the middle (the ends glide)
        let cut = (b - a) / 8;
        let mut v: Vec<f32> = (a + cut..b - cut).filter_map(|j| m[j]).collect();
        if v.is_empty() {
            return;
        }
        v.sort_by(f32::total_cmp);
        let lv = (a..b).map(|j| tr.level[j] * tr.level[j]).sum::<f32>() / (b - a) as f32;
        out.push(Note::new(a * hop, (b * hop).min(n * hop), v[v.len() / 2], 10.0 * lv.max(1e-12).log10()));
    };
    // the slow curve (over about a vibrato cycle): notes are cut where it moves, not the vibrato
    let smooth = ((0.075 * tr.rate as f64) as usize / hop).max(1);
    let ms: Vec<Option<f32>> = (0..n).map(|k| {
        m[k]?;
        let v: Vec<f32> = (k.saturating_sub(smooth)..(k + smooth + 1).min(n)).filter_map(|j| m[j]).collect();
        Some(v.iter().sum::<f32>() / v.len() as f32)
    }).collect();
    let mut k = 0;
    while k < n {
        if m[k].is_none() {
            k += 1;
            continue;
        }
        let a = k;
        let mut hist: Vec<f32> = Vec::new();
        let mut away = 0usize;
        let mut gap = 0usize;
        let mut end = n;
        let mut j = k;
        while j < n {
            match (m[j], ms[j]) {
                (Some(_), Some(p)) => {
                    gap = 0;
                    // a transient well inside: a new note
                    if j > a + min_frames && onset_k.contains(&j) {
                        end = j;
                        break;
                    }
                    if hist.len() >= 4 {
                        let mut v = hist.clone();
                        v.sort_by(f32::total_cmp);
                        let med = v[v.len() / 2];
                        if (p - med).abs() > 0.6 {
                            away += 1;
                            if away >= 4 {
                                // the cut: where the pitch itself crosses halfway to the new one
                                let first = j + 1 - away;
                                let mid = (med + p) / 2.0;
                                let up = p > med;
                                let from = first.saturating_sub(smooth).max(a + 1);
                                end = (from..=j).find(|&q| m[q].is_some_and(|x| if up { x > mid } else { x < mid })).unwrap_or(first);
                                break;
                            }
                        } else {
                            away = 0;
                        }
                    }
                    if away == 0 {
                        hist.push(p);
                        if hist.len() > 40 {
                            hist.remove(0);
                        }
                    }
                }
                _ => {
                    gap += 1;
                    if gap >= 3 {
                        end = j + 1 - gap;
                        break;
                    }
                }
            }
            j += 1;
        }
        if j >= n {
            end = n - gap.min(n);
        }
        push(a, end, &mut out);
        k = end.max(a + 1);
    }
    out
}

/// Pitch Correction: each note moved `amount` (0..1) of the way to the nearest pitch of the
/// scale (`scale[pc]`: pitch class pc in the scale, 0 = C).
pub fn correct(notes: &mut [Note], scale: &[bool; 12], amount: f32) {
    if !scale.iter().any(|&b| b) {
        return;
    }
    for n in notes.iter_mut() {
        let p = n.pitch;
        let mut best = p.round();
        let mut dist = f32::MAX;
        for k in (p.floor() as i32 - 6)..=(p.ceil() as i32 + 6) {
            if scale[k.rem_euclid(12) as usize] && (k as f32 - p).abs() < dist {
                dist = (k as f32 - p).abs();
                best = k as f32;
            }
        }
        n.shift_cents = (best - p) * 100.0 * amount.clamp(0.0, 1.0);
    }
}

/// The scale of a key: major (or natural minor) pitch classes from `root` (0 = C).
pub fn key_scale(root: u8, minor: bool) -> [bool; 12] {
    let steps: [u8; 7] = if minor { [0, 2, 3, 5, 7, 8, 10] } else { [0, 2, 4, 5, 7, 9, 11] };
    let mut s = [false; 12];
    for st in steps {
        s[((root + st) % 12) as usize] = true;
    }
    s
}

/// The notes as MIDI: (start frame, end frame, key, velocity) — Create MIDI Track from Flex
/// Pitch Data.
pub fn to_midi(notes: &[Note]) -> Vec<(usize, usize, u8, u8)> {
    let top = notes.iter().map(|n| n.level_db + n.gain_db).fold(-200.0f32, f32::max);
    notes.iter().map(|n| {
        let rel = n.level_db + n.gain_db - top;
        let vel = (127.0 + rel * 2.5).clamp(20.0, 127.0) as u8;
        (n.start, n.end, n.target().round().clamp(0.0, 127.0) as u8, vel)
    }).collect()
}

/// Plays the notes' edits on interleaved `data` (its pitch track `tr`); the length stays.
pub fn render(data: &[f32], channels: usize, tr: &Track, notes: &[Note]) -> Vec<f32> {
    let c = channels.max(1);
    let frames = data.len() / c;
    if notes.iter().all(Note::is_neutral) || frames == 0 {
        return data.to_vec();
    }
    let rate = tr.rate;
    let hop = tr.hop;
    let nk = tr.f0.len();
    // per hop: the pitch ratio, the formant factor, the gain
    let mut ratio = vec![1f32; nk + 1];
    let mut formant = vec![1f32; nk + 1];
    let mut gain = vec![1f32; nk + 1];
    let smooth = ((0.075 * rate as f64) as usize / hop).max(1);
    for n in notes.iter().filter(|n| !n.is_neutral()) {
        let (a, b) = (n.start / hop, (n.end / hop).min(nk));
        if b <= a {
            continue;
        }
        let centre = n.pitch * 100.0;
        let cents: Vec<Option<f32>> = (a..b).map(|k| (tr.f0[k] > 0.0).then(|| hz_to_midi(tr.f0[k]) * 100.0)).collect();
        for k in a..b {
            let i = k - a;
            let here = cents[i].unwrap_or(centre);
            // the slow curve (drift) and what's left (vibrato)
            let lo = i.saturating_sub(smooth);
            let hi = (i + smooth + 1).min(cents.len());
            let v: Vec<f32> = cents[lo..hi].iter().flatten().copied().collect();
            let slow = if v.is_empty() { centre } else { v.iter().sum::<f32>() / v.len() as f32 };
            let new = centre + n.shift_cents + n.drift * (slow - centre) + n.vibrato * (here - slow);
            ratio[k] = 2f32.powf((new - here) / 1200.0);
            formant[k] = 2f32.powf(n.formant_cents / 1200.0);
            gain[k] = 10f32.powf(n.gain_db / 20.0);
        }
    }
    let at = |v: &[f32], f: f64| -> f32 {
        let x = f / hop as f64;
        let i = (x.floor() as usize).min(v.len() - 1);
        let j = (i + 1).min(v.len() - 1);
        let t = (x - i as f64).clamp(0.0, 1.0) as f32;
        v[i] + (v[j] - v[i]) * t
    };
    // the analysis marks: a period apart where voiced, 5 ms elsewhere
    let unvoiced = rate as f64 * 0.005;
    let mut marks: Vec<f64> = Vec::new();
    let mut t = 0.0;
    while t < frames as f64 {
        marks.push(t);
        let hz = tr.hz_at(t) as f64;
        let p = if hz > 0.0 { (rate as f64 / hz).clamp(rate as f64 / 1500.0, rate as f64 / 50.0) } else { unvoiced };
        t += p;
    }
    marks.push(t);
    let sample = |f: f64, ch: usize| -> f32 {
        let i = f.floor();
        let fr = (f - i) as f32;
        let i = i as isize;
        let g = |k: isize| if k >= 0 && (k as usize) < frames { data[k as usize * c + ch] } else { 0.0 };
        g(i) + (g(i + 1) - g(i)) * fr
    };
    let mut out = vec![0f32; data.len()];
    let mut ts = 0.0f64;
    let mut j = 0usize;
    while ts < frames as f64 {
        while j + 2 < marks.len() && (marks[j + 1] - ts).abs() <= (marks[j] - ts).abs() {
            j += 1;
        }
        let ta = marks[j];
        let pa = marks[j + 1] - marks[j];
        let voiced = tr.hz_at(ta) > 0.0;
        let r = if voiced { at(&ratio, ts) as f64 } else { 1.0 };
        let f = at(&formant, ts) as f64;
        let ps = pa / r.max(0.25);
        // a grain two analysis periods long, weighted for the new density
        let h = pa;
        let wgt = (ps / pa) as f32;
        let lo = (ts - h).ceil() as isize;
        let hi = (ts + h).floor() as isize;
        for o in lo.max(0)..=hi.min(frames as isize - 1) {
            let x = (o as f64 - ts) / h;
            let w = (0.5 + 0.5 * (std::f64::consts::PI * x).cos()) as f32 * wgt;
            let src = ta + (o as f64 - ts) * f;
            for ch in 0..c {
                out[o as usize * c + ch] += sample(src, ch) * w;
            }
        }
        ts += ps;
    }
    // gain, ramped between hops
    for fr in 0..frames {
        let g = at(&gain, fr as f64);
        if g != 1.0 {
            for ch in 0..c {
                out[fr * c + ch] *= g;
            }
        }
    }
    out
}

/// Tracks and cuts in one go (the analysis Studio keeps per file).
pub fn analyze(data: &[f32], channels: usize, rate: u32) -> (Track, Vec<Note>) {
    let c = channels.max(1);
    let mono: Vec<f32> = data.chunks(c).map(|f| f.iter().sum::<f32>() / c as f32).collect();
    let tr = track(&mono, rate);
    let on = transients::detect_mono(&mono, rate, 0.5);
    let ns = notes(&tr, &on);
    (tr, ns)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A voice-like tone: 10 harmonics, falling off, at `hz(t)`.
    fn tone(rate: u32, secs: f32, hz: impl Fn(f32) -> f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        let mut ph = 0.0f64;
        (0..n).map(|i| {
            let t = i as f32 / rate as f32;
            ph += hz(t) as f64 / rate as f64;
            let p = 2.0 * std::f64::consts::PI * ph;
            (1..=10).map(|h| (0.25 / h as f64) * (p * h as f64).sin()).sum::<f64>() as f32
        }).collect()
    }

    fn pitch_of(x: &[f32], rate: u32) -> f32 {
        let tr = track(x, rate);
        let mut v: Vec<f32> = tr.f0[tr.f0.len() / 4..tr.f0.len() * 3 / 4].iter().copied().filter(|&f| f > 0.0).collect();
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    }

    #[test]
    fn tracks_the_pitch_finely() {
        for rate in [44_100, 48_000] {
            for hz in [98.0, 220.0, 440.0, 659.25] {
                let f = pitch_of(&tone(rate, 0.4, |_| hz), rate);
                let cents = 1200.0 * (f / hz).log2();
                assert!(cents.abs() < 3.0, "{rate} {hz}: {f} ({cents} cents)");
            }
        }
        let tr = track(&vec![0.0; 9600], 48_000);
        assert!(tr.f0.iter().all(|&f| f == 0.0));
    }

    #[test]
    fn finds_the_notes_of_a_melody() {
        let rate = 44_100;
        let mut x = Vec::new();
        let melody = [220.0, 261.63, 329.63, 293.66];
        for hz in melody {
            x.extend(tone(rate, 0.3, |_| hz));
            x.extend(vec![0.0; (rate as f32 * 0.04) as usize]);
        }
        let (_, ns) = analyze(&x, 1, rate);
        assert_eq!(ns.len(), 4, "{ns:?}");
        let step = (rate as f32 * 0.34) as usize;
        for (k, (n, hz)) in ns.iter().zip(melody).enumerate() {
            assert!((n.pitch - hz_to_midi(hz)).abs() < 0.1, "{k}: {}", n.pitch);
            assert!((n.start as i64 - (k * step) as i64).abs() < (rate as i64 / 40), "{k}: {}", n.start);
            assert!((n.end as i64 - (k * step + (rate as f32 * 0.3) as usize) as i64).abs() < (rate as i64 / 25), "{k}: {}", n.end);
        }
        // legato: no gap, the pitch steps a whole tone and stays
        let mut y = tone(rate, 0.3, |_| 220.0);
        y.extend(tone(rate, 0.3, |_| 246.94));
        let (_, ns) = analyze(&y, 1, rate);
        assert_eq!(ns.len(), 2, "{ns:?}");
        // vibrato doesn't cut
        let v = tone(rate, 0.8, |t| 220.0 * 2f32.powf(0.5 / 12.0 * (2.0 * std::f32::consts::PI * 5.5 * t).sin()));
        let (_, ns) = analyze(&v, 1, rate);
        assert_eq!(ns.len(), 1, "{ns:?}");
    }

    #[test]
    fn shifted_notes_hit_their_target() {
        let rate = 44_100;
        let x = tone(rate, 0.8, |_| 220.0);
        let (tr, mut ns) = analyze(&x, 1, rate);
        assert_eq!(ns.len(), 1);
        for (cents, want) in [(200.0, 246.94), (-700.0, 146.83), (1200.0, 440.0), (-1200.0, 110.0), (37.0, 224.75)] {
            ns[0].shift_cents = cents;
            let y = render(&x, 1, &tr, &ns);
            assert_eq!(y.len(), x.len());
            let f = pitch_of(&y, rate);
            let err = 1200.0 * (f / want).log2();
            assert!(err.abs() < 6.0, "{cents}: {f} ({err} cents)");
            // the level stays about the same (an octave keeps only every other harmonic)
            if (cents as f32).abs() > 1000.0 {
                continue;
            }
            let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
            let k = rms(&y[8000..30000]) / rms(&x[8000..30000]);
            assert!((0.7..1.4).contains(&k), "{cents}: level {k}");
        }
        // unedited: the samples as they were
        ns[0].shift_cents = 0.0;
        assert_eq!(render(&x, 1, &tr, &ns), x);
    }

    #[test]
    fn stereo_edits_keep_both_channels() {
        let rate = 44_100;
        let m = tone(rate, 0.5, |_| 196.0);
        let x: Vec<f32> = m.iter().flat_map(|v| [*v, *v * 0.5]).collect();
        let (tr, mut ns) = analyze(&x, 2, rate);
        ns[0].shift_cents = 500.0;
        let y = render(&x, 2, &tr, &ns);
        let l: Vec<f32> = y.chunks(2).map(|f| f[0]).collect();
        let r: Vec<f32> = y.chunks(2).map(|f| f[1]).collect();
        assert!((pitch_of(&l, rate) - 261.63).abs() < 2.0);
        for i in 0..l.len() {
            assert!((r[i] - l[i] * 0.5).abs() < 1e-4);
        }
    }

    #[test]
    fn vibrato_and_drift_flatten() {
        let rate = 44_100;
        let x = tone(rate, 1.0, |t| 220.0 * 2f32.powf(0.5 / 12.0 * (2.0 * std::f32::consts::PI * 5.5 * t).sin()));
        let (tr, mut ns) = analyze(&x, 1, rate);
        let spread = |s: &[f32]| {
            let t = track(s, rate);
            let v: Vec<f32> = t.f0[30..t.f0.len() - 30].iter().filter(|&&f| f > 0.0).map(|&f| hz_to_midi(f) * 100.0).collect();
            let mean = v.iter().sum::<f32>() / v.len() as f32;
            (v.iter().map(|c| (c - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt()
        };
        let before = spread(&x);
        assert!(before > 25.0, "{before}");
        ns[0].vibrato = 0.0;
        let after = spread(&render(&x, 1, &tr, &ns));
        assert!(after < before * 0.35, "{before} → {after}");
        // a glide into the note: drift 0 holds it at the note's pitch
        let g = tone(rate, 0.8, |t| 220.0 * 2f32.powf(-(1.0 - (t / 0.25).min(1.0)) * 1.5 / 12.0));
        let (tr, mut ns) = analyze(&g, 1, rate);
        assert_eq!(ns.len(), 1, "{ns:?}");
        ns[0].drift = 0.0;
        let y = render(&g, 1, &tr, &ns);
        let t2 = track(&y, rate);
        let early: Vec<f32> = t2.f0[12..30].iter().filter(|&&f| f > 0.0).copied().collect();
        let mean = early.iter().sum::<f32>() / early.len() as f32;
        assert!((1200.0 * (mean / 220.0).log2()).abs() < 30.0, "{mean}");
    }

    #[test]
    fn formant_shift_moves_the_spectrum_not_the_pitch() {
        let rate = 44_100;
        // a 150 Hz voice with one formant at 900 Hz
        let n = rate as usize;
        let x: Vec<f32> = (0..n).map(|i| {
            let t = i as f64 / rate as f64;
            (1..40).map(|h| {
                let f = 150.0 * h as f64;
                let a = (-((f - 900.0) / 250.0).powi(2)).exp() * 0.2;
                a * (2.0 * std::f64::consts::PI * f * t).sin()
            }).sum::<f64>() as f32
        }).collect();
        let (tr, mut ns) = analyze(&x, 1, rate);
        assert_eq!(ns.len(), 1, "{ns:?}");
        let centroid = |s: &[f32]| {
            let (mut num, mut den) = (0.0f64, 0.0f64);
            for h in 1..40 {
                let f = 150.0 * h as f64;
                let (mut a, mut b) = (0.0f64, 0.0f64);
                for (i, v) in s[10000..30000].iter().enumerate() {
                    let ph = 2.0 * std::f64::consts::PI * f * (i + 10000) as f64 / rate as f64;
                    a += *v as f64 * ph.cos();
                    b += *v as f64 * ph.sin();
                }
                let m = (a * a + b * b).sqrt();
                num += m * f;
                den += m;
            }
            num / den
        };
        let c0 = centroid(&x);
        ns[0].formant_cents = 700.0;
        let y = render(&x, 1, &tr, &ns);
        let c1 = centroid(&y);
        assert!(c1 / c0 > 1.3, "{c0} → {c1}");
        assert!((pitch_of(&y, rate) - 150.0).abs() < 2.0, "{}", pitch_of(&y, rate));
    }

    #[test]
    fn gain_and_correction() {
        let rate = 44_100;
        let x = tone(rate, 0.5, |_| 233.08); // A#3
        let (tr, mut ns) = analyze(&x, 1, rate);
        ns[0].gain_db = -6.0206;
        let y = render(&x, 1, &tr, &ns);
        let rms = |s: &[f32]| (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt();
        assert!((rms(&y[5000..15000]) / rms(&x[5000..15000]) - 0.5).abs() < 0.02);
        // C major: 58.3 (A#3, a little sharp) goes to B3 (59), the nearer
        ns[0].gain_db = 0.0;
        ns[0].pitch = 58.3;
        correct(&mut ns, &key_scale(0, false), 1.0);
        assert!((ns[0].shift_cents - 70.0).abs() < 0.01, "{}", ns[0].shift_cents);
        correct(&mut ns, &key_scale(0, false), 0.5);
        assert!((ns[0].shift_cents - 35.0).abs() < 0.01);
        let midi = to_midi(&ns);
        assert_eq!(midi[0].2, 59);
    }
}
