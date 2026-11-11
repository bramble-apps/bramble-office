//! Flex Time (Logic Pro 13's Flex modes): audio played to a time map made of flex markers, each
//! mode its own algorithm, as Logic describes them.
//!
//! - **Polyphonic** (and Automatic): a phase vocoder (~85 ms Hann frames, 4x overlap) with
//!   identity phase locking (Laroche–Dolson: peaks advance at their measured frequency, the bins
//!   around them keep their phase to the peak), phases reset at transients so attacks stay
//!   sharp, and every channel locked to the mono mix's phases so stereo stays put;
//! - **Monophonic** (and Flex Pitch's time part): WSOLA following the map, short frames;
//! - **Slicing**: cut at the transients, each slice played as it is at its new place; when
//!   slices move apart the gaps are silent, when they close up a slice ends with a short fade;
//! - **Rhythmic**: as Slicing, but a gap is filled by looping the end of the slice before it;
//! - **Speed (FX)**: played faster or slower, the pitch following (tape);
//! - **Tempophone (FX)**: fixed grains repeated or skipped without searching for a good join,
//!   the tempophone's granular sound.
//!
//! A time map is the flex markers: (source frame, destination frame) pairs, rising in both,
//! from the region's start; before the first marker it runs from (0, 0), after the last at
//! speed 1.

use crate::fft::{hann, Fft};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Mode {
    Slicing,
    Rhythmic,
    Monophonic,
    #[default]
    Polyphonic,
    Speed,
    Tempophone,
    /// Flex Pitch: Monophonic for time, its notes' edits for pitch (see `flexpitch`).
    Pitch,
}

impl Mode {
    /// In the order of Logic's Flex mode pop-up.
    pub const ALL: [Mode; 7] = [Mode::Pitch, Mode::Monophonic, Mode::Slicing, Mode::Rhythmic, Mode::Polyphonic, Mode::Speed, Mode::Tempophone];

    /// Logic's pop-up label.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Pitch => "Flex Pitch",
            Mode::Monophonic => "Flex Time - Monophonic",
            Mode::Slicing => "Flex Time - Slicing",
            Mode::Rhythmic => "Flex Time - Rhythmic",
            Mode::Polyphonic => "Flex Time - Polyphonic",
            Mode::Speed => "Flex Time - Speed (FX)",
            Mode::Tempophone => "Flex Time - Tempophone (FX)",
        }
    }

    /// The short name (the track header and the editor's pop-up).
    pub fn name(self) -> &'static str {
        match self {
            Mode::Pitch => "Flex Pitch",
            Mode::Monophonic => "Monophonic",
            Mode::Slicing => "Slicing",
            Mode::Rhythmic => "Rhythmic",
            Mode::Polyphonic => "Polyphonic",
            Mode::Speed => "Speed (FX)",
            Mode::Tempophone => "Tempophone (FX)",
        }
    }

    pub fn from_label(s: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.label() == s || m.name() == s).or_else(|| (s == "Flex Time - Automatic (Polyphonic)").then_some(Mode::Polyphonic))
    }
}

/// Source ↔ destination frames through the flex markers.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeMap {
    pts: Vec<(f64, f64)>,
}

impl TimeMap {
    pub fn new(anchors: &[(usize, usize)]) -> TimeMap {
        let mut a: Vec<(usize, usize)> = anchors.to_vec();
        a.sort_by_key(|p| p.0);
        let mut pts = vec![(0.0, 0.0)];
        for (s, d) in a {
            let last = *pts.last().unwrap();
            if s as f64 > last.0 && d as f64 > last.1 {
                pts.push((s as f64, d as f64));
            }
        }
        TimeMap { pts }
    }

    pub fn is_identity(&self) -> bool {
        self.pts.iter().all(|p| (p.0 - p.1).abs() < 0.5)
    }

    /// The source frame playing at destination frame `d`.
    pub fn src(&self, d: f64) -> f64 {
        for w in self.pts.windows(2) {
            let ((s0, d0), (s1, d1)) = (w[0], w[1]);
            if d <= d1 {
                return s0 + (d - d0) * (s1 - s0) / (d1 - d0);
            }
        }
        let (s, dd) = *self.pts.last().unwrap();
        s + (d - dd)
    }

    /// Where source frame `s` plays.
    pub fn dst(&self, s: f64) -> f64 {
        for w in self.pts.windows(2) {
            let ((s0, d0), (s1, d1)) = (w[0], w[1]);
            if s <= s1 {
                return d0 + (s - s0) * (d1 - d0) / (s1 - s0);
            }
        }
        let (ss, d) = *self.pts.last().unwrap();
        d + (s - ss)
    }

    /// The markers' source frames (after the start).
    pub fn sources(&self) -> impl Iterator<Item = f64> + '_ {
        self.pts.iter().skip(1).map(|p| p.0)
    }
}

/// Renders `out_frames` frames of interleaved `data` through the flex markers in `mode`.
/// `transients`: the source's transients (frames), the slice points and phase resets.
pub fn render(data: &[f32], channels: usize, rate: u32, anchors: &[(usize, usize)], out_frames: usize, mode: Mode, transients: &[usize]) -> Vec<f32> {
    let c = channels.max(1);
    let map = TimeMap::new(anchors);
    if map.is_identity() {
        let mut v: Vec<f32> = data.iter().copied().take(out_frames * c).collect();
        v.resize(out_frames * c, 0.0);
        return v;
    }
    match mode {
        Mode::Polyphonic => vocoder(data, c, rate, &map, out_frames, transients),
        Mode::Monophonic | Mode::Pitch => wsola(data, c, rate, &map, out_frames, 0.024),
        Mode::Slicing => slices(data, c, rate, &map, out_frames, transients, false),
        Mode::Rhythmic => slices(data, c, rate, &map, out_frames, transients, true),
        Mode::Speed => speed(data, c, &map, out_frames),
        Mode::Tempophone => tempophone(data, c, rate, &map, out_frames, 0.05, 0.25),
    }
}

fn mono_of(data: &[f32], c: usize) -> Vec<f32> {
    data.chunks(c).map(|f| f.iter().sum::<f32>() / c as f32).collect()
}

/// WSOLA following a time map: frames laid half a frame apart, each read near the map's source
/// where it best continues the one before.
fn wsola(data: &[f32], c: usize, rate: u32, map: &TimeMap, out_frames: usize, secs: f64) -> Vec<f32> {
    let frames = data.len() / c;
    let n = ((rate as f64 * secs) as usize).max(64) & !1;
    let hs = n / 2;
    let tol = n / 4;
    let win = hann(n);
    const DEC: usize = 2;
    let mono = mono_of(data, c);
    let m = |f: isize| if f >= 0 { mono.get(f as usize).copied().unwrap_or(0.0) } else { 0.0 };
    let mut out = vec![0f32; (out_frames + n) * c];
    let mut norm = vec![0f32; out_frames + n];
    let mut prev: isize = 0;
    let mut k = 0usize;
    while k * hs < out_frames {
        let out_pos = k * hs;
        // the frame's centre follows the map
        let nominal = (map.src((out_pos + hs) as f64) - hs as f64).round() as isize;
        let chosen = if k == 0 {
            nominal
        } else {
            let nat = prev + hs as isize;
            let mut best = nominal;
            let mut best_v = f32::MIN;
            let mut d = -(tol as isize);
            while d <= tol as isize {
                let cand = nominal + d;
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
                d += 1;
            }
            best
        };
        for i in 0..n {
            let src = chosen + i as isize;
            let w = win[i];
            if src >= 0 && (src as usize) < frames {
                for ch in 0..c {
                    out[(out_pos + i) * c + ch] += data[src as usize * c + ch] * w;
                }
            }
            norm[out_pos + i] += w;
        }
        prev = chosen;
        k += 1;
    }
    finish(out, &norm, c, out_frames, 1e-3)
}

fn finish(mut out: Vec<f32>, norm: &[f32], c: usize, out_frames: usize, floor: f32) -> Vec<f32> {
    out.truncate(out_frames * c);
    out.resize(out_frames * c, 0.0);
    for (f, w) in norm.iter().take(out_frames).enumerate() {
        if *w > floor {
            for ch in 0..c {
                out[f * c + ch] /= w;
            }
        }
    }
    out
}

fn wrap(x: f64) -> f64 {
    let tau = std::f64::consts::TAU;
    x - tau * ((x + std::f64::consts::PI) / tau).floor()
}

/// The phase vocoder (Polyphonic).
fn vocoder(data: &[f32], c: usize, rate: u32, map: &TimeMap, out_frames: usize, transients: &[usize]) -> Vec<f32> {
    let frames = data.len() / c;
    let n = ((rate as f64 * 0.085) as usize).next_power_of_two().clamp(256, 8192);
    let hs = n / 4;
    let half = n as isize / 2;
    let fft = Fft::new(n);
    let win = hann(n);
    let bins = n / 2 + 1;
    let mono = mono_of(data, c);
    let get = |sig: &[f32], stride: usize, ch: usize, f: isize| -> f32 {
        if f >= 0 && (f as usize) < frames { sig[f as usize * stride + ch] } else { 0.0 }
    };
    // two real frames in one complex FFT: a + ib → A, B (the spectra, bins 0..=n/2)
    let spectra = |a: &dyn Fn(usize) -> f32, b: &dyn Fn(usize) -> f32, re: &mut Vec<f32>, im: &mut Vec<f32>| -> [(Vec<f32>, Vec<f32>); 2] {
        for i in 0..n {
            re[i] = a(i) * win[i];
            im[i] = b(i) * win[i];
        }
        fft.forward(re, im);
        let mut ar = vec![0f32; bins];
        let mut ai = vec![0f32; bins];
        let mut br = vec![0f32; bins];
        let mut bi = vec![0f32; bins];
        for k in 0..bins {
            let j = (n - k) % n;
            ar[k] = 0.5 * (re[k] + re[j]);
            ai[k] = 0.5 * (im[k] - im[j]);
            br[k] = 0.5 * (im[k] + im[j]);
            bi[k] = -0.5 * (re[k] - re[j]);
        }
        [(ar, ai), (br, bi)]
    };
    let mut re = vec![0f32; n];
    let mut im = vec![0f32; n];
    let mut out = vec![0f32; (out_frames + n) * c];
    let mut norm = vec![0f32; out_frames + n];
    let mut syn = vec![0f64; bins];
    let omega: Vec<f64> = (0..bins).map(|k| std::f64::consts::TAU * k as f64 / n as f64).collect();
    let mut tr: Vec<usize> = transients.to_vec();
    tr.sort_unstable();
    let mut prev_ac = f64::NEG_INFINITY;
    let mut k = 0usize;
    loop {
        let oc = (k * hs) as isize;
        if oc - half >= out_frames as isize {
            break;
        }
        let ac = map.src(oc as f64);
        let a0 = ac.round() as isize - half;
        let a1 = a0 - hs as isize;
        let [(mr, mi), (pr, pi)] = spectra(&|i| get(&mono, 1, 0, a0 + i as isize), &|i| get(&mono, 1, 0, a1 + i as isize), &mut re, &mut im);
        let phase: Vec<f64> = (0..bins).map(|b| (mi[b] as f64).atan2(mr[b] as f64)).collect();
        let mag: Vec<f32> = (0..bins).map(|b| (mr[b] * mr[b] + mi[b] * mi[b]).sqrt()).collect();
        // a transient passed since the last frame: the phases start again from the source's
        let lo = prev_ac.max(-1.0);
        let reset = k == 0 || tr.iter().any(|&t| (t as f64) > lo && (t as f64) <= ac);
        if reset {
            syn.copy_from_slice(&phase);
        } else {
            // the peaks and the bins each belongs to
            let mut peaks: Vec<usize> = Vec::new();
            let top = mag.iter().copied().fold(0.0f32, f32::max);
            for b in 1..bins - 1 {
                if mag[b] > mag[b - 1] && mag[b] >= mag[b + 1] && mag[b] > top * 1e-4 {
                    peaks.push(b);
                }
            }
            let mut next = syn.clone();
            if peaks.is_empty() {
                for b in 0..bins {
                    let pp = (pi[b] as f64).atan2(pr[b] as f64);
                    let inst = omega[b] + wrap(phase[b] - pp - omega[b] * hs as f64) / hs as f64;
                    next[b] = syn[b] + inst * hs as f64;
                }
            } else {
                let mut start = 0;
                for (pk, &p) in peaks.iter().enumerate() {
                    let end = peaks.get(pk + 1).map_or(bins, |&q| {
                        // the lowest bin between two peaks splits them
                        (p..q).min_by(|&a, &b| mag[a].total_cmp(&mag[b])).unwrap_or(q)
                    });
                    let pp = (pi[p] as f64).atan2(pr[p] as f64);
                    let inst = omega[p] + wrap(phase[p] - pp - omega[p] * hs as f64) / hs as f64;
                    let yp = syn[p] + inst * hs as f64;
                    for b in start..end {
                        next[b] = yp + (phase[b] - phase[p]);
                    }
                    start = end;
                }
            }
            syn = next;
        }
        // each channel: its own magnitudes and its phase offset from the mono mix
        let chans: Vec<(Vec<f32>, Vec<f32>)> = if c == 1 {
            vec![(mr.clone(), mi.clone())]
        } else {
            let mut v = Vec::new();
            let mut ch = 0;
            while ch < c {
                let c2 = (ch + 1).min(c - 1);
                let [x, y] = spectra(&|i| get(data, c, ch, a0 + i as isize), &|i| get(data, c, c2, a0 + i as isize), &mut re, &mut im);
                v.push(x);
                if ch + 1 < c {
                    v.push(y);
                }
                ch += 2;
            }
            v
        };
        // back to time, two channels per inverse FFT
        let mut ch = 0;
        while ch < c {
            let pair = [Some(ch), (ch + 1 < c).then_some(ch + 1)];
            let mut yr = [vec![0f32; bins], vec![0f32; bins]];
            let mut yi = [vec![0f32; bins], vec![0f32; bins]];
            for (s, q) in pair.iter().enumerate() {
                let Some(q) = *q else { continue };
                let (xr, xi) = &chans[q];
                for b in 0..bins {
                    let m = (xr[b] * xr[b] + xi[b] * xi[b]).sqrt();
                    let off = if c == 1 { 0.0 } else { (xi[b] as f64).atan2(xr[b] as f64) - phase[b] };
                    let ph = syn[b] + off;
                    yr[s][b] = m * ph.cos() as f32;
                    yi[s][b] = m * ph.sin() as f32;
                }
            }
            // Z = Y0 + i·Y1 over the full (Hermitian) spectrum
            for b in 0..n {
                let (k2, conj) = if b < bins { (b, false) } else { (n - b, true) };
                let (r0, i0) = (yr[0][k2], if conj { -yi[0][k2] } else { yi[0][k2] });
                let (r1, i1) = (yr[1][k2], if conj { -yi[1][k2] } else { yi[1][k2] });
                re[b] = r0 - i1;
                im[b] = i0 + r1;
            }
            fft.inverse(&mut re, &mut im);
            for i in 0..n {
                let o = oc - half + i as isize;
                if o < 0 || o as usize >= out_frames + n {
                    continue;
                }
                let o = o as usize;
                out[o * c + ch] += re[i] * win[i];
                if ch + 1 < c {
                    out[o * c + ch + 1] += im[i] * win[i];
                }
                if ch == 0 {
                    norm[o] += win[i] * win[i];
                }
            }
            ch += 2;
        }
        prev_ac = ac;
        k += 1;
    }
    finish(out, &norm, c, out_frames, 1e-2)
}

/// Slicing and Rhythmic: slices at the transients and the markers, each kept at its speed.
fn slices(data: &[f32], c: usize, rate: u32, map: &TimeMap, out_frames: usize, transients: &[usize], fill: bool) -> Vec<f32> {
    let frames = data.len() / c;
    let mut cuts: Vec<usize> = vec![0];
    cuts.extend(transients.iter().copied().filter(|&t| t > 0 && t < frames));
    cuts.extend(map.sources().map(|s| s.round() as usize).filter(|&t| t > 0 && t < frames));
    cuts.sort_unstable();
    cuts.dedup();
    let fade = ((rate as f64 * 0.003) as usize).max(8);
    let mut out = vec![0f32; out_frames * c];
    for (i, &s0) in cuts.iter().enumerate() {
        let s1 = cuts.get(i + 1).copied().unwrap_or(frames);
        let o0 = map.dst(s0 as f64).round() as usize;
        let o1 = if i + 1 < cuts.len() { map.dst(s1 as f64).round() as usize } else { out_frames };
        let room = o1.saturating_sub(o0).min(out_frames.saturating_sub(o0));
        let len = s1 - s0;
        if room == 0 {
            continue;
        }
        let put = |out: &mut Vec<f32>, o: usize, s: usize, g: f32| {
            if o < out_frames && s < frames {
                for ch in 0..c {
                    out[o * c + ch] += data[s * c + ch] * g;
                }
            }
        };
        if len >= room {
            // closed up: the slice cut short with a fade (the last one runs to the end)
            let last = i + 1 == cuts.len();
            for j in 0..room {
                let g = if last { 1.0 } else { ((room - j) as f32 / fade as f32).min(1.0) };
                put(&mut out, o0 + j, s0 + j, g);
            }
        } else {
            for j in 0..len {
                put(&mut out, o0 + j, s0 + j, 1.0);
            }
            if fill && len > 4 * fade {
                // Rhythmic: the slice's end looped into the gap, crossfaded
                let lp = (len / 2).min((rate as f64 * 0.06) as usize).max(2 * fade);
                let ls = s1 - lp;
                let mut o = o0 + len - fade;
                while o < o0 + room {
                    for j in 0..lp {
                        let at = o + j;
                        if at >= o0 + room {
                            break;
                        }
                        let mut g = 1.0;
                        if j < fade {
                            g = j as f32 / fade as f32;
                        }
                        if lp - j < fade {
                            g = g.min((lp - j) as f32 / fade as f32);
                        }
                        let end = o0 + room - at;
                        if end < fade && i + 1 < cuts.len() {
                            g *= end as f32 / fade as f32;
                        }
                        // where the loop's fade-in overlaps the slice's tail, that fades out
                        if j < fade && o + j < o0 + len && at < out_frames {
                            for ch in 0..c {
                                out[at * c + ch] *= 1.0 - j as f32 / fade as f32;
                            }
                        }
                        put(&mut out, at, ls + j, g);
                    }
                    o += lp - fade;
                }
            } else if !fill && i + 1 < cuts.len() {
                // Slicing: a short fade before the silence
                for j in 0..fade.min(len) {
                    let at = o0 + len - 1 - j;
                    if at < out_frames {
                        for ch in 0..c {
                            out[at * c + ch] *= j as f32 / fade as f32;
                        }
                    }
                }
            }
        }
    }
    out
}

/// Speed (FX): read at the map's speed, the pitch following.
fn speed(data: &[f32], c: usize, map: &TimeMap, out_frames: usize) -> Vec<f32> {
    let frames = data.len() / c;
    let get = |f: isize, ch: usize| if f >= 0 && (f as usize) < frames { data[f as usize * c + ch] } else { 0.0 };
    let mut out = Vec::with_capacity(out_frames * c);
    for o in 0..out_frames {
        let x = map.src(o as f64);
        let i = x.floor() as isize;
        let t = (x - i as f64) as f32;
        for ch in 0..c {
            let (p0, p1, p2, p3) = (get(i - 1, ch), get(i, ch), get(i + 1, ch), get(i + 2, ch));
            let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
            let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
            let cc = -0.5 * p0 + 0.5 * p2;
            out.push(((a * t + b) * t + cc) * t + p1);
        }
    }
    out
}

/// Tempophone (FX): grains of `grain` seconds read where the map says, crossfaded over `xfade`
/// of their length, without looking for a good join.
fn tempophone(data: &[f32], c: usize, rate: u32, map: &TimeMap, out_frames: usize, grain: f64, xfade: f64) -> Vec<f32> {
    let frames = data.len() / c;
    let g = ((rate as f64 * grain) as usize).max(32);
    let x = ((g as f64 * xfade) as usize).max(4);
    let hop = g - x;
    let mut out = vec![0f32; (out_frames + g) * c];
    let mut norm = vec![0f32; out_frames + g];
    let mut o = 0usize;
    while o < out_frames {
        let s = map.src(o as f64).round() as isize;
        for j in 0..g {
            let w = if j < x { j as f32 / x as f32 } else if g - j <= x { (g - j) as f32 / x as f32 } else { 1.0 };
            let f = s + j as isize;
            if f >= 0 && (f as usize) < frames {
                for ch in 0..c {
                    out[(o + j) * c + ch] += data[f as usize * c + ch] * w;
                }
            }
            norm[o + j] += w;
        }
        o += hop;
    }
    finish(out, &norm, c, out_frames, 1e-3)
}

/// Quantize by transients: a flex marker for each transient that's within `range` frames of a
/// grid line, moved `strength` (0..1) of the way there. `grid` gives the nearest grid line's
/// destination frame for a destination frame. Returns markers (source, destination), rising.
pub fn quantize(transients: &[usize], map: &TimeMap, grid: impl Fn(f64) -> f64, strength: f64, range: f64) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &t in transients {
        let d = map.dst(t as f64);
        let q = grid(d);
        if (q - d).abs() > range {
            continue;
        }
        let nd = (d + (q - d) * strength.clamp(0.0, 1.0)).round().max(1.0) as usize;
        if t == 0 {
            continue;
        }
        if out.last().is_some_and(|l| l.1 >= nd || l.0 >= t) {
            continue;
        }
        out.push((t, nd));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stretch::detect_pitch;

    fn sine(rate: u32, hz: f32, frames: usize, ch: usize) -> Vec<f32> {
        (0..frames).flat_map(|i| std::iter::repeat_n(0.5 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin(), ch)).collect()
    }

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32).sqrt()
    }

    fn left(x: &[f32], c: usize) -> Vec<f32> {
        x.chunks(c).map(|f| f[0]).collect()
    }

    #[test]
    fn the_map_goes_both_ways() {
        let m = TimeMap::new(&[(1000, 1500), (3000, 3000)]);
        assert_eq!(m.src(750.0), 500.0);
        assert_eq!(m.dst(500.0), 750.0);
        assert_eq!(m.src(4000.0), 4000.0);
        assert!(TimeMap::new(&[]).is_identity());
        // a marker that doesn't rise is dropped
        assert_eq!(TimeMap::new(&[(1000, 1500), (2000, 1400)]).pts.len(), 2);
    }

    #[test]
    fn stretched_sines_keep_their_pitch_in_every_time_mode() {
        let rate = 16000;
        let x = sine(rate, 300.0, 16000, 2);
        for mode in [Mode::Polyphonic, Mode::Monophonic, Mode::Tempophone] {
            for ratio in [0.6, 1.5, 2.0] {
                let out_frames = (16000.0 * ratio) as usize;
                let y = render(&x, 2, rate, &[(16000, out_frames)], out_frames, mode, &[]);
                assert_eq!(y.len(), out_frames * 2);
                let m = left(&y, 2);
                let mid = m.len() / 2;
                let f = detect_pitch(&m[mid - 1600..mid + 1600], rate, 60.0, 1500.0).unwrap();
                // (Tempophone's grains join anywhere: its warble is the effect)
                let tol = if mode == Mode::Tempophone { 12.0 } else { 3.0 };
                assert!((f - 300.0).abs() < tol, "{mode:?} {ratio}: {f}");
                let r = rms(&m[2000..m.len() - 2000]);
                assert!((r - 0.3535).abs() < 0.05, "{mode:?} {ratio}: rms {r}");
            }
        }
    }

    #[test]
    fn the_vocoder_keeps_stereo_and_chords() {
        let rate = 22050;
        let n = rate as usize;
        // a chord, left a fifth above right
        let x: Vec<f32> = (0..n).flat_map(|i| {
            let t = i as f32 / rate as f32;
            let tau = 2.0 * std::f32::consts::PI;
            [0.3 * (tau * 440.0 * t).sin() + 0.2 * (tau * 554.37 * t).sin(), 0.3 * (tau * 293.66 * t).sin()]
        }).collect();
        let out = (n as f64 * 1.7) as usize;
        let y = render(&x, 2, rate, &[(n, out)], out, Mode::Polyphonic, &[]);
        let r: Vec<f32> = y.chunks(2).map(|f| f[1]).collect();
        let mid = r.len() / 2;
        let f = detect_pitch(&r[mid - 2000..mid + 2000], rate, 60.0, 1500.0).unwrap();
        assert!((f - 293.66).abs() < 3.0, "{f}");
        // the left's two partials both there: its spectrum peaks at 440 and 554
        let l: Vec<f32> = left(&y, 2)[mid - 4096..mid + 4096].to_vec();
        let mag = |hz: f32| {
            let (mut a, mut b) = (0.0f32, 0.0f32);
            for (i, v) in l.iter().enumerate() {
                let ph = 2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32;
                a += v * ph.cos();
                b += v * ph.sin();
            }
            (a * a + b * b).sqrt() / l.len() as f32
        };
        assert!(mag(440.0) > 0.1 && mag(554.37) > 0.07, "{} {}", mag(440.0), mag(554.37));
        assert!(mag(500.0) < 0.02, "{}", mag(500.0));
    }

    #[test]
    fn transients_land_where_the_markers_put_them() {
        let rate = 16000;
        // clicks every 4000 frames over a quiet tone
        let n = 16000;
        let mut x = sine(rate, 200.0, n, 1).iter().map(|v| v * 0.05).collect::<Vec<f32>>();
        let clicks = [2000usize, 6000, 10000, 14000];
        for &k in &clicks {
            for j in 0..40 {
                x[k + j] += 0.9 * (1.0 - j as f32 / 40.0);
            }
        }
        // the second click moved 1000 frames later, the third kept
        let anchors = [(6000, 7000), (10000, 10000)];
        for mode in [Mode::Slicing, Mode::Rhythmic, Mode::Polyphonic, Mode::Monophonic] {
            let y = render(&x, 1, rate, &anchors, n, mode, &clicks);
            let peak_near = |at: usize| (at - 600..at + 600).max_by(|&a, &b| y[a].abs().total_cmp(&y[b].abs())).unwrap();
            let p = peak_near(7000);
            assert!((p as i64 - 7000).abs() < 160, "{mode:?}: {p}");
            assert!(y[p].abs() > 0.5, "{mode:?}: {}", y[p]);
            let p3 = peak_near(10000);
            assert!((p3 as i64 - 10000).abs() < 160, "{mode:?}: {p3}");
            // nothing left where the click was
            assert!(y[6000..6040].iter().all(|v| v.abs() < 0.3), "{mode:?}");
        }
    }

    #[test]
    fn slicing_leaves_gaps_rhythmic_fills_them() {
        let rate = 16000;
        let n = 16000;
        let x = sine(rate, 250.0, n, 1);
        // the slice from 4000 moved to 6000: 2000 frames of gap after the first slice
        let anchors = [(4000, 6000)];
        let s = render(&x, 1, rate, &anchors, n + 2000, Mode::Slicing, &[4000]);
        assert!(rms(&s[4200..5800]) < 0.01, "{}", rms(&s[4200..5800]));
        assert!(rms(&s[6200..8000]) > 0.3);
        let r = render(&x, 1, rate, &anchors, n + 2000, Mode::Rhythmic, &[4000]);
        assert!(rms(&r[4200..5800]) > 0.25, "{}", rms(&r[4200..5800]));
        // Slicing keeps each slice's speed: the pitch is the same
        let f = detect_pitch(&s[7000..9000], rate, 60.0, 1500.0).unwrap();
        assert!((f - 250.0).abs() < 3.0);
    }

    #[test]
    fn speed_changes_the_pitch_with_the_tempo() {
        let rate = 16000;
        let x = sine(rate, 300.0, 16000, 1);
        // twice as long: an octave down
        let y = render(&x, 1, rate, &[(16000, 32000)], 32000, Mode::Speed, &[]);
        let f = detect_pitch(&y[10000..13000], rate, 60.0, 1500.0).unwrap();
        assert!((f - 150.0).abs() < 2.0, "{f}");
    }

    #[test]
    fn quantize_moves_transients_toward_the_grid() {
        let map = TimeMap::new(&[]);
        let grid = |d: f64| (d / 1000.0).round() * 1000.0;
        let q = quantize(&[1100, 2950, 4500], &map, grid, 1.0, 400.0);
        assert_eq!(q, vec![(1100, 1000), (2950, 3000)]);
        let half = quantize(&[1100], &map, grid, 0.5, 400.0);
        assert_eq!(half, vec![(1100, 1050)]);
    }
}
