//! Smart Tempo's analysis: the tempo, every beat, the downbeats and the meter of a recording.
//!
//! 1. the onset-strength curve ([`crate::transients::onset_strength`]);
//! 2. the tempo: its autocorrelation, weighted towards 120 BPM (a log-Gaussian over octaves,
//!    as Ellis 2007), with the double period's support added;
//! 3. the beats: dynamic programming over the curve (each beat the best onset about one period
//!    after the last), then each beat moved onto a transient within 25 ms;
//! 4. constant tempo: the beats fitted with a straight line (one period), then extended over
//!    the whole file; variable: the tracked beats as they are (the tempo curve), extended;
//! 5. the meter and downbeat: which beats in 3 or in 4 are louder (accent contrast).

use crate::transients::{detect_mono, frame_hop, onset_strength};

#[derive(Clone, Debug, PartialEq)]
pub struct BeatMap {
    /// Every beat, in seconds from the file's start (rising).
    pub beats: Vec<f64>,
    /// The beat (index into `beats`) that is bar 1, beat 1.
    pub downbeat: usize,
    /// The signature: beats in a bar, and the beat's note value.
    pub beats_per_bar: u8,
    pub beat_unit: u8,
    /// Tempo: Variable (the beats as played) or Constant (evenly spaced).
    pub variable: bool,
    /// Hints and locked ranges (hints.rs), kept with the map so undo covers them.
    pub hints: crate::hints::Hints,
}

impl BeatMap {
    /// Evenly spaced beats over `seconds`, from `first` (a downbeat).
    pub fn constant(bpm: f64, first: f64, seconds: f64, beats_per_bar: u8) -> BeatMap {
        let p = 60.0 / bpm.max(1.0);
        let mut beats = Vec::new();
        let back = (first / p).floor() as i64;
        let mut k = -back;
        loop {
            let t = first + k as f64 * p;
            if t > seconds + p * 0.5 {
                break;
            }
            beats.push(t);
            k += 1;
        }
        BeatMap { beats, downbeat: back.max(0) as usize, beats_per_bar: beats_per_bar.max(1), beat_unit: 4, variable: false, hints: Default::default() }
    }

    /// The average tempo (BPM).
    pub fn tempo(&self) -> f64 {
        if self.beats.len() < 2 {
            return 120.0;
        }
        let span = self.beats[self.beats.len() - 1] - self.beats[0];
        60.0 * (self.beats.len() - 1) as f64 / span.max(1e-9)
    }

    /// The tempo at a moment (the beat interval around it).
    pub fn tempo_at(&self, sec: f64) -> f64 {
        if self.beats.len() < 2 {
            return self.tempo();
        }
        let i = self.beats.partition_point(|&b| b <= sec).clamp(1, self.beats.len() - 1);
        60.0 / (self.beats[i] - self.beats[i - 1]).max(1e-9)
    }

    /// The tempo curve: (second, BPM) at each beat.
    pub fn curve(&self) -> Vec<(f64, f64)> {
        self.beats.windows(2).map(|w| (w[0], 60.0 / (w[1] - w[0]).max(1e-9))).collect()
    }

    /// A beat's bar and beat (bar 1 beat 1 at the downbeat; the beats before it in bar 0 or
    /// earlier: Logic's "0. 2.").
    pub fn bar_beat(&self, i: usize) -> (i64, u32) {
        let m = self.beats_per_bar.max(1) as i64;
        let rel = i as i64 - self.downbeat as i64;
        (rel.div_euclid(m) + 1, rel.rem_euclid(m) as u32 + 1)
    }

    /// The position in beats (fractional, beat 0 = the file's first beat) of a moment, the beat
    /// grid extended at its ends.
    pub fn beats_at(&self, sec: f64) -> f64 {
        let b = &self.beats;
        match b.len() {
            0 => sec * 2.0,
            1 => (sec - b[0]) * 2.0,
            n => {
                let i = b.partition_point(|&x| x <= sec).clamp(1, n - 1);
                (i - 1) as f64 + (sec - b[i - 1]) / (b[i] - b[i - 1]).max(1e-9)
            }
        }
    }

    /// The moment of a fractional beat position (inverse of [`beats_at`](Self::beats_at)).
    pub fn sec_at(&self, beat: f64) -> f64 {
        let b = &self.beats;
        match b.len() {
            0 => beat / 2.0,
            1 => b[0] + beat / 2.0,
            n => {
                let i = (beat.floor().max(0.0) as usize).min(n - 2);
                b[i] + (beat - i as f64) * (b[i + 1] - b[i])
            }
        }
    }

    /// x2: twice the tempo (a beat between every two).
    pub fn double(&mut self) {
        if self.beats.len() < 2 {
            return;
        }
        let mut out = Vec::with_capacity(self.beats.len() * 2);
        for w in self.beats.windows(2) {
            out.push(w[0]);
            out.push((w[0] + w[1]) / 2.0);
        }
        out.push(*self.beats.last().unwrap());
        self.beats = out;
        self.downbeat *= 2;
    }

    /// /2: half the tempo (every other beat, the downbeat kept).
    pub fn halve(&mut self) {
        if self.beats.len() < 3 {
            return;
        }
        let keep = self.downbeat % 2;
        self.beats = self.beats.iter().enumerate().filter(|(i, _)| i % 2 == keep).map(|(_, b)| *b).collect();
        self.downbeat /= 2;
    }

    /// Tempo: Constant — the beats fitted to one period (least squares), kept in place.
    pub fn make_constant(&mut self) {
        let n = self.beats.len();
        if n < 2 {
            self.variable = false;
            return;
        }
        let (a, p) = fit(&self.beats);
        self.beats = (0..n).map(|i| a + p * i as f64).collect();
        self.variable = false;
    }

    /// Moves a beat marker (the editor's drag). Constant: the whole grid's tempo changes so the
    /// beat lands there, the downbeat staying (dragging the downbeat moves the grid). Variable:
    /// only that beat, kept between its neighbours.
    pub fn move_beat(&mut self, i: usize, sec: f64) {
        let n = self.beats.len();
        if i >= n {
            return;
        }
        if !self.variable && n >= 2 {
            let d = self.downbeat.min(n - 1);
            if i == d {
                let shift = sec - self.beats[i];
                self.beats.iter_mut().for_each(|b| *b += shift);
            } else {
                let p = (sec - self.beats[d]) / (i as f64 - d as f64);
                if p > 0.05 {
                    let a = self.beats[d];
                    self.beats = (0..n).map(|k| a + p * (k as f64 - d as f64)).collect();
                }
            }
            return;
        }
        let lo = if i > 0 { self.beats[i - 1] + 0.02 } else { f64::MIN };
        let hi = if i + 1 < n { self.beats[i + 1] - 0.02 } else { f64::MAX };
        if lo < hi {
            self.beats[i] = sec.clamp(lo, hi);
        }
    }

    /// Set Downbeat: this beat is bar 1 beat 1 (or the nearest beat to a moment).
    pub fn set_downbeat(&mut self, i: usize) {
        self.downbeat = i.min(self.beats.len().saturating_sub(1));
    }

    pub fn nearest_beat(&self, sec: f64) -> Option<usize> {
        (0..self.beats.len()).min_by(|&a, &b| (self.beats[a] - sec).abs().total_cmp(&(self.beats[b] - sec).abs()))
    }

    /// The downbeats (bar starts): (second, bar number).
    pub fn bars(&self) -> Vec<(f64, i64)> {
        (0..self.beats.len()).filter(|&i| self.bar_beat(i).1 == 1).map(|i| (self.beats[i], self.bar_beat(i).0)).collect()
    }

    /// Average tempo over beats `a..=b` set evenly (Set Average Tempo for Selection).
    pub fn even_out(&mut self, a: usize, b: usize) {
        let (a, b) = (a.min(b), a.max(b).min(self.beats.len().saturating_sub(1)));
        if b <= a + 1 {
            return;
        }
        let (s, e) = (self.beats[a], self.beats[b]);
        for k in a..=b {
            self.beats[k] = s + (e - s) * (k - a) as f64 / (b - a) as f64;
        }
    }
}

/// Least-squares line through beats: (first beat's time, period).
fn fit(beats: &[f64]) -> (f64, f64) {
    let n = beats.len() as f64;
    let mx = (n - 1.0) / 2.0;
    let my = beats.iter().sum::<f64>() / n;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    for (i, b) in beats.iter().enumerate() {
        let dx = i as f64 - mx;
        sxy += dx * (b - my);
        sxx += dx * dx;
    }
    let p = if sxx > 0.0 { sxy / sxx } else { 0.5 };
    (my - p * mx, p)
}

/// The tempo (BPM, 40..240) of an onset curve sampled at `env_rate` per second.
pub fn estimate_tempo(env: &[f32], env_rate: f64) -> Option<f64> {
    let mean = env.iter().sum::<f32>() / env.len().max(1) as f32;
    let x: Vec<f32> = env.iter().map(|v| v - mean).collect();
    let lag_of = |bpm: f64| 60.0 / bpm * env_rate;
    let (lo, hi) = (lag_of(240.0).floor() as usize, (lag_of(40.0).ceil() as usize).min(x.len() / 2));
    if hi <= lo + 2 {
        return None;
    }
    let acf = |l: usize| -> f32 {
        if l >= x.len() {
            return 0.0;
        }
        let mut s = 0.0;
        for i in 0..x.len() - l {
            s += x[i] * x[i + l];
        }
        s / (x.len() - l) as f32
    };
    let raw: Vec<f32> = (0..=hi * 2).map(|l| if l >= lo / 2 { acf(l) } else { 0.0 }).collect();
    let zero = acf(0).max(1e-12);
    let mut best = None;
    let mut best_v = f32::MIN;
    for l in lo..=hi {
        let bpm = 60.0 * env_rate / l as f64;
        let w = (-0.5 * ((bpm / 120.0).log2() / 0.9).powi(2)).exp() as f32;
        let v = w * (raw[l] + 0.5 * raw.get(2 * l).copied().unwrap_or(0.0));
        if v > best_v {
            best_v = v;
            best = Some(l);
        }
    }
    let l = best?;
    if raw[l] / zero < 0.02 {
        return None;
    }
    // parabolic refinement of the peak
    let (a, b, c) = (raw[l - 1], raw[l], raw[l + 1]);
    let den = a - 2.0 * b + c;
    let off = if den.abs() > 1e-12 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
    Some(60.0 * env_rate / (l as f64 + off as f64))
}

/// Beat tracking by dynamic programming (Ellis 2007): hop indexes of the beats.
pub fn track_beats(env: &[f32], period: f64, tightness: f64) -> Vec<usize> {
    let n = env.len();
    if n == 0 || period < 2.0 {
        return Vec::new();
    }
    let sd = {
        let m = env.iter().sum::<f32>() / n as f32;
        (env.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / n as f32).sqrt().max(1e-9)
    };
    let e: Vec<f64> = env.iter().map(|v| (*v / sd) as f64).collect();
    let mut score = e.clone();
    let mut back = vec![usize::MAX; n];
    let lo = (period / 2.0).round() as usize;
    let hi = (period * 2.0).round() as usize;
    for t in 0..n {
        let mut best = f64::MIN;
        let mut arg = usize::MAX;
        for d in lo.max(1)..=hi {
            if d > t {
                break;
            }
            let tau = t - d;
            let pen = (d as f64 / period).ln();
            let v = score[tau] - tightness * pen * pen;
            if v > best {
                best = v;
                arg = tau;
            }
        }
        if arg != usize::MAX && best > 0.0 {
            score[t] = e[t] + best;
            back[t] = arg;
        }
    }
    // the end: the best score in the last period
    let tail = n.saturating_sub(period.ceil() as usize);
    let mut t = (tail..n).max_by(|&a, &b| score[a].total_cmp(&score[b])).unwrap_or(n - 1);
    let mut beats = vec![t];
    while back[t] != usize::MAX {
        t = back[t];
        beats.push(t);
    }
    beats.reverse();
    beats
}

/// Smart Tempo's analysis of a recording (mono samples). With no rhythm to find (silence, one
/// hit, a steady tone) the grid is Logic's default, 120 BPM in 4/4 from the first sound; None
/// only for an empty file.
pub fn analyze(mono: &[f32], rate: u32, variable: bool) -> Option<BeatMap> {
    if mono.is_empty() || rate == 0 {
        return None;
    }
    analyze_rhythm(mono, rate, variable).or_else(|| {
        let first = detect_mono(mono, rate, 0.6).first().map_or(0.0, |f| *f as f64 / rate as f64);
        let mut m = BeatMap::constant(120.0, first, mono.len() as f64 / rate as f64, 4);
        m.variable = variable;
        Some(m)
    })
}

/// The analysis proper: None when there's no rhythm in it.
pub fn analyze_rhythm(mono: &[f32], rate: u32, variable: bool) -> Option<BeatMap> {
    let (_, hop) = frame_hop(rate);
    let env_rate = rate as f64 / hop as f64;
    let seconds = mono.len() as f64 / rate as f64;
    let tr: Vec<f64> = detect_mono(mono, rate, 0.6).into_iter().map(|f| f as f64 / rate as f64).collect();
    // a few hits only (no groove to follow): the slowest steady grid they all sit on
    if tr.len() < 8 {
        let (bpm, first) = sparse_grid(&tr)?;
        let mut map = BeatMap::constant(bpm, first, seconds, 4);
        map.variable = variable;
        let accents: Vec<f64> = map.beats.iter().map(|&b| accent(mono, rate, b)).collect();
        let (m, phase) = meter(&accents);
        map.beats_per_bar = m;
        map.downbeat = phase;
        return Some(map);
    }
    let env = onset_strength(mono, rate);
    let bpm = estimate_tempo(&env, env_rate)?;
    let period = 60.0 / bpm * env_rate;
    let idx = track_beats(&env, period, if variable { 40.0 } else { 400.0 });
    if idx.len() < 3 {
        return None;
    }
    let mut beats: Vec<f64> = idx.iter().map(|&i| i as f64 / env_rate).collect();
    // onto the transients (within 25 ms)
    if !tr.is_empty() {
        for b in beats.iter_mut() {
            let k = tr.partition_point(|&x| x < *b);
            let near = [k.checked_sub(1), Some(k)].into_iter().flatten().filter_map(|j| tr.get(j)).min_by(|x, y| (*x - *b).abs().total_cmp(&(*y - *b).abs()));
            if let Some(&t) = near.filter(|t| (*t - *b).abs() < 0.025) {
                *b = t;
            }
        }
        beats.dedup_by(|a, b| (*a - *b).abs() < 0.03);
    }
    let mut map = BeatMap { beats, downbeat: 0, beats_per_bar: 4, beat_unit: 4, variable, hints: Default::default() };
    if !variable {
        map.make_constant();
    }
    extend(&mut map, seconds);
    // the meter and downbeat from the beats' accents
    let accents: Vec<f64> = map.beats.iter().map(|&b| accent(mono, rate, b)).collect();
    let (m, phase) = meter(&accents);
    map.beats_per_bar = m;
    map.downbeat = phase;
    Some(map)
}

/// The tempo (60..180 BPM) and phase of the steadiest grid a few onsets (seconds) sit on:
/// the one they miss least, the slowest of equals.
pub fn sparse_grid(onsets: &[f64]) -> Option<(f64, f64)> {
    if onsets.len() < 2 {
        return None;
    }
    let o0 = onsets[0];
    let err_at = |k: usize| {
        let p = 600.0 / k as f64;
        onsets.iter().map(|o| {
            let x = (o - o0) / p;
            (x - x.round()).abs()
        }).sum::<f64>() / onsets.len() as f64
    };
    let errs: Vec<f64> = (600..=1800).map(err_at).collect();
    let min = errs.iter().copied().fold(f64::MAX, f64::min);
    // the slowest near the best, then down into its own minimum
    let mut i = errs.iter().position(|e| *e <= min + 0.01)?;
    while i + 1 < errs.len() && errs[i + 1] < errs[i] {
        i += 1;
    }
    let (bpm, err) = ((600 + i) as f64 / 10.0, errs[i]);
    (err < 0.1).then_some((bpm, o0))
}

/// The beat grid continued to the file's start and end (the first and last intervals).
fn extend(map: &mut BeatMap, seconds: f64) {
    let b = &mut map.beats;
    if b.len() < 2 {
        return;
    }
    let p0 = b[1] - b[0];
    let mut front = Vec::new();
    let mut t = b[0] - p0;
    while t >= -p0 * 0.01 && p0 > 0.0 {
        front.push(t.max(0.0));
        t -= p0;
    }
    front.reverse();
    let added = front.len();
    b.splice(0..0, front);
    let pn = b[b.len() - 1] - b[b.len() - 2];
    while pn > 0.0 && *b.last().unwrap() + pn < seconds {
        let t = *b.last().unwrap() + pn;
        b.push(t);
    }
    map.downbeat += added;
}

/// A beat's loudness: the RMS of the 70 ms after it over the 30 ms before.
fn accent(mono: &[f32], rate: u32, sec: f64) -> f64 {
    let at = (sec * rate as f64) as usize;
    let w = (rate as f64 * 0.07) as usize;
    let pre = (rate as f64 * 0.03) as usize;
    let rms = |a: usize, b: usize| {
        let b = b.min(mono.len());
        if b <= a {
            return 0.0;
        }
        (mono[a..b].iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / (b - a) as f64).sqrt()
    };
    (rms(at, at + w) - 0.5 * rms(at.saturating_sub(pre), at)).max(0.0)
}

/// The meter (3 or 4) and the first downbeat's index from the beats' accents.
pub fn meter(accents: &[f64]) -> (u8, usize) {
    let mean = accents.iter().sum::<f64>() / accents.len().max(1) as f64;
    if accents.len() < 6 || mean <= 0.0 {
        return (4, 0);
    }
    let contrast = |m: usize, p: usize| {
        let (mut on, mut n_on, mut off, mut n_off) = (0.0, 0, 0.0, 0);
        for (i, a) in accents.iter().enumerate() {
            if i % m == p {
                on += a;
                n_on += 1;
            } else {
                off += a;
                n_off += 1;
            }
        }
        if n_on == 0 || n_off == 0 {
            return 0.0;
        }
        (on / n_on as f64 - off / n_off as f64) / mean
    };
    let best = |m: usize| (0..m).map(|p| (contrast(m, p), p)).max_by(|a, b| a.0.total_cmp(&b.0)).unwrap_or((0.0, 0));
    let (c4, p4) = best(4);
    let (c3, p3) = best(3);
    // no accents to tell by: 4/4 from the first beat (Logic's default)
    if c3.max(c4) < 0.08 {
        return (4, 0);
    }
    if c3 > c4 * 1.15 { (3, p3) } else { (4, p4) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A click track: noise clicks at the beat times, every `m`th louder and lower.
    fn clicks(rate: u32, beats: &[f64], m: usize, first_down: usize, secs: f64) -> Vec<f32> {
        let n = (rate as f64 * secs) as usize;
        let mut out = vec![0f32; n];
        let mut seed = 99u32;
        for (k, &t) in beats.iter().enumerate() {
            let down = k >= first_down && (k - first_down) % m == 0 || (k < first_down && (first_down - k) % m == 0);
            let amp = if down { 0.9 } else { 0.35 };
            let s = (t * rate as f64) as usize;
            for j in 0..(rate as usize / 12) {
                if s + j >= n {
                    break;
                }
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let noise = ((seed >> 16) as f32 / 32768.0) - 1.0;
                let tone = (j as f32 * if down { 0.03 } else { 0.09 }).sin();
                out[s + j] += amp * (0.5 * noise + 0.5 * tone) * (-(j as f32) / (rate as f32 * 0.02)).exp();
            }
        }
        out
    }

    #[test]
    fn finds_a_constant_tempo_meter_and_downbeat() {
        let rate = 22050;
        for (bpm, m, down) in [(100.0, 4usize, 1usize), (128.0, 4, 0), (90.0, 3, 2)] {
            let p = 60.0 / bpm;
            let beats: Vec<f64> = (0..).map(|k| 0.3 + k as f64 * p).take_while(|t| *t < 14.0).collect();
            let x = clicks(rate, &beats, m, down, 14.5);
            let map = analyze_rhythm(&x, rate, false).expect("a tempo");
            assert!((map.tempo() - bpm).abs() < 1.0, "{bpm}: {}", map.tempo());
            assert_eq!(map.beats_per_bar as usize, m, "{bpm}");
            // the downbeat on an accented click
            let d = map.beats[map.downbeat];
            let k = ((d - 0.3) / p).round() as usize;
            assert_eq!((k + m - down % m) % m, 0, "{bpm}: downbeat at {d}");
            // the beats on the clicks
            for b in map.beats.iter().filter(|b| **b > 0.3 && **b < 13.5) {
                let off = ((b - 0.3) / p - ((b - 0.3) / p).round()).abs() * p;
                assert!(off < 0.02, "{bpm}: beat {b} off by {off}");
            }
        }
    }

    #[test]
    fn variable_tempo_follows_a_speed_up() {
        let rate = 22050;
        // 90 → 120 BPM over 16 s
        let mut beats = vec![0.5];
        while *beats.last().unwrap() < 16.0 {
            let t = *beats.last().unwrap();
            let bpm = 90.0 + 30.0 * t / 16.0;
            beats.push(t + 60.0 / bpm);
        }
        let x = clicks(rate, &beats, 4, 0, 16.5);
        let map = analyze_rhythm(&x, rate, true).expect("a tempo");
        assert!(map.variable);
        let early = map.tempo_at(3.0);
        let late = map.tempo_at(14.0);
        assert!(early < 100.0 && late > 110.0, "{early} {late}");
        // Constant from it: one tempo in between
        let mut c = map.clone();
        c.make_constant();
        assert!((c.tempo_at(3.0) - c.tempo_at(14.0)).abs() < 1e-6);
    }

    #[test]
    fn a_few_hits_sit_on_the_slowest_grid() {
        // Logic's E23 composite-40s: hits at 0, 8, 10, 22 and 30 s → 60 BPM
        assert_eq!(sparse_grid(&[0.0, 8.0, 10.0, 22.0, 30.0]).map(|g| g.0), Some(60.0));
        assert_eq!(sparse_grid(&[1.0, 1.5, 2.5]).map(|g| g.0), Some(120.0));
        assert!(sparse_grid(&[1.0]).is_none());
    }

    #[test]
    fn silence_gets_the_default_grid() {
        assert!(analyze_rhythm(&vec![0.0; 22050 * 4], 22050, false).is_none());
        let m = analyze(&vec![0.0; 22050 * 4], 22050, false).unwrap();
        assert!((m.tempo() - 120.0).abs() < 1e-9 && m.beats_per_bar == 4 && m.beats[0] == 0.0);
        assert!(analyze(&[], 22050, false).is_none());
    }

    #[test]
    fn double_halve_bars_and_moves() {
        let mut m = BeatMap::constant(60.0, 2.0, 20.0, 3);
        assert_eq!(m.beats[0], 0.0);
        assert_eq!(m.beats[m.downbeat], 2.0);
        // the beats before the downbeat: bar 0 (Logic's "0. 2.")
        assert_eq!(m.bar_beat(1), (0, 3));
        assert_eq!(m.bar_beat(0), (0, 2));
        assert_eq!(m.bar_beat(m.downbeat + 3), (2, 1));
        assert!((m.tempo() - 60.0).abs() < 1e-9);
        m.double();
        assert!((m.tempo() - 120.0).abs() < 1e-9);
        assert_eq!(m.beats[m.downbeat], 2.0);
        m.halve();
        assert!((m.tempo() - 60.0).abs() < 1e-9);
        assert_eq!(m.beats[m.downbeat], 2.0);
        // constant: dragging beat 4 after the downbeat to 6.4 s → 0.4 s → 1.1 s per beat
        let d = m.downbeat;
        m.move_beat(d + 4, 6.4);
        assert!((m.tempo_at(10.0) - 60.0 / 1.1).abs() < 1e-6);
        assert_eq!(m.beats[d], 2.0);
        // variable: only that beat
        m.variable = true;
        let before = m.beats.clone();
        m.move_beat(d + 1, before[d + 1] + 0.2);
        assert_eq!(m.beats.iter().zip(&before).filter(|(a, b)| a != b).count(), 1);
        assert!((m.sec_at(m.beats_at(7.3)) - 7.3).abs() < 1e-9);
        assert_eq!(m.bars()[0], (2.0, 1));
    }
}
