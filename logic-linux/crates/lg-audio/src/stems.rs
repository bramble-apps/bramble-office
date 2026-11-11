//! Stem Splitter's separation, done with signal processing only (no trained model: nothing to
//! download, fully offline). It is an honest approximation of Logic's machine-learning splitter:
//!
//! - A short-time Fourier transform (2048-point Hann frames, 512 hop) of both channels.
//! - Harmonic / percussive separation by median filtering (along time for sustained partials,
//!   along frequency for broadband hits), giving soft masks.
//! - Vocals: harmonic content panned to the centre (left and right alike in level and phase) in
//!   the voice's range; Bass: harmonic content below about 250 Hz; Drums: the percussive part;
//!   Piano: harmonic partials that start with a hammer-like attack and decay; Guitar: other
//!   harmonic content in the guitar's range; Other: what remains.
//! - Drum kit pieces from the drum part: Kick (lows), Snare (broadband bursts with a 150–300 Hz
//!   body), Tom (pitched lows-mids between hits), Hi-Hat (short highs), Cymbal (sustained highs),
//!   Other.
//!
//! The masks of every split sum to one, so the stems always add back up to the original
//! exactly. Quality is far from a trained separator: expect bleed between stems.

use crate::fft::Fft;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stem {
    Vocals,
    Drums,
    Bass,
    Guitar,
    Piano,
    Other,
    Kick,
    Snare,
    Tom,
    HiHat,
    Cymbal,
    DrumOther,
}

impl Stem {
    pub const MAIN: [Stem; 6] = [Stem::Vocals, Stem::Drums, Stem::Bass, Stem::Guitar, Stem::Piano, Stem::Other];
    pub const KIT: [Stem; 6] = [Stem::Kick, Stem::Snare, Stem::Tom, Stem::HiHat, Stem::Cymbal, Stem::DrumOther];
    pub fn name(self) -> &'static str {
        match self {
            Stem::Vocals => "Vocals",
            Stem::Drums => "Drums",
            Stem::Bass => "Bass",
            Stem::Guitar => "Guitar",
            Stem::Piano => "Piano",
            Stem::Other => "Other",
            Stem::Kick => "Kick",
            Stem::Snare => "Snare",
            Stem::Tom => "Tom",
            Stem::HiHat => "Hi-Hat",
            Stem::Cymbal => "Cymbal",
            Stem::DrumOther => "Drums Other",
        }
    }
}

const N: usize = 2048;
const HOP: usize = 512;
const BINS: usize = N / 2 + 1;
const MED_T: usize = 17;
const MED_F: usize = 17;

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn median(v: &mut [f32]) -> f32 {
    let m = v.len() / 2;
    *v.select_nth_unstable_by(m, |a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)).1
}

/// Splits a stereo signal (interleaved, `rate`). Returns every main stem (and with `kit`, the
/// six kit pieces in place of Drums), each interleaved stereo of the input's length.
/// `progress` gets 0..1.
pub fn split(x: &[f32], rate: f32, kit: bool, progress: &mut dyn FnMut(f32)) -> Vec<(Stem, Vec<f32>)> {
    let frames_in = x.len() / 2;
    let stems: Vec<Stem> = if kit { vec![Stem::Vocals, Stem::Bass, Stem::Guitar, Stem::Piano, Stem::Other, Stem::Kick, Stem::Snare, Stem::Tom, Stem::HiHat, Stem::Cymbal, Stem::DrumOther] } else { Stem::MAIN.to_vec() };
    let ns = stems.len();
    let pad = N;
    let total = frames_in + 2 * pad;
    let get = |i: isize, c: usize| -> f32 {
        let j = i - pad as isize;
        if j < 0 || j as usize >= frames_in { 0.0 } else { x[2 * j as usize + c] }
    };
    let nframes = total.div_ceil(HOP);
    let fft = Fft::new(N);
    // square-root Hann for analysis and synthesis: their product, the Hann, sums to 2 at 75 %
    let win: Vec<f32> = (0..N).map(|i| (0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos()).sqrt()).collect();
    let mut out: Vec<Vec<f32>> = vec![vec![0.0; total * 2]; ns];
    // a ring of spectra (MED_T frames) for the time medians
    let half_t = MED_T / 2;
    let mut ring_re = vec![[vec![0f32; BINS], vec![0f32; BINS]]; MED_T];
    let mut ring_im = vec![[vec![0f32; BINS], vec![0f32; BINS]]; MED_T];
    let mut ring_mag = vec![vec![0f32; BINS]; MED_T];
    let mut prev_mag = vec![0f32; BINS];
    let mut onset_env = vec![0f32; BINS];
    let mut decay_env = vec![0f32; BINS];
    let mut tmp = vec![0f32; MED_T.max(MED_F)];
    let bin_hz: Vec<f32> = (0..BINS).map(|k| k as f32 * rate / N as f32).collect();
    let mut flux_avg = 1e-6f32;
    let analyse = |t: usize, re: &mut [Vec<f32>; 2], im: &mut [Vec<f32>; 2]| {
        let mut br = vec![0f32; N];
        let mut bi = vec![0f32; N];
        for c in 0..2 {
            for i in 0..N {
                br[i] = get((t * HOP + i) as isize, c) * win[i];
                bi[i] = 0.0;
            }
            fft.forward(&mut br, &mut bi);
            re[c].copy_from_slice(&br[..BINS]);
            im[c].copy_from_slice(&bi[..BINS]);
        }
    };
    // frames are analysed half_t ahead of the one being separated
    for t in 0..nframes + half_t {
        if t < nframes {
            let slot = t % MED_T;
            let (mut re, mut im) = (std::mem::take(&mut ring_re[slot]), std::mem::take(&mut ring_im[slot]));
            analyse(t, &mut re, &mut im);
            for k in 0..BINS {
                ring_mag[slot][k] = (re[0][k] * re[0][k] + im[0][k] * im[0][k] + re[1][k] * re[1][k] + im[1][k] * im[1][k]).sqrt();
            }
            ring_re[slot] = re;
            ring_im[slot] = im;
        } else {
            let slot = t % MED_T;
            ring_mag[slot].iter_mut().for_each(|v| *v = 0.0);
        }
        if t < half_t {
            continue;
        }
        let ft = t - half_t;
        if ft >= nframes {
            break;
        }
        let cs = ft % MED_T;
        let mag = ring_mag[cs].clone();
        // per-frame features: broadband flux (for snare hits)
        let mut flux = 0.0;
        let mut body = 0.0;
        for k in 0..BINS {
            let f = bin_hz[k];
            if (1000.0..6000.0).contains(&f) {
                flux += (mag[k] - prev_mag[k]).max(0.0);
            }
            if (150.0..300.0).contains(&f) {
                body += (mag[k] - prev_mag[k]).max(0.0);
            }
        }
        flux_avg = 0.98 * flux_avg + 0.02 * flux;
        let snare_hit = flux > 2.5 * flux_avg && body > 0.0;
        let mut masks = vec![vec![0f32; BINS]; ns];
        for k in 0..BINS {
            // harmonic: median along time; percussive: median along frequency
            for (j, v) in tmp.iter_mut().take(MED_T).enumerate() {
                *v = ring_mag[(ft + MED_T - half_t + j) % MED_T][k];
            }
            let h = median(&mut tmp[..MED_T]);
            let lo = k.saturating_sub(MED_F / 2);
            let hi = (k + MED_F / 2 + 1).min(BINS);
            let w = hi - lo;
            tmp[..w].copy_from_slice(&mag[lo..hi]);
            let p = median(&mut tmp[..w]);
            let (h2, p2) = (h * h, p * p);
            let harm = if h2 + p2 > 1e-20 { h2 / (h2 + p2) } else { 0.5 };
            let perc = 1.0 - harm;
            let f = bin_hz[k];
            // centre: left and right alike (level and phase)
            let (lr, li, rr, ri) = (ring_re[cs][0][k], ring_im[cs][0][k], ring_re[cs][1][k], ring_im[cs][1][k]);
            let (ml, mr) = ((lr * lr + li * li).sqrt(), (rr * rr + ri * ri).sqrt());
            let dot = lr * rr + li * ri;
            let centre = if ml * mr > 1e-12 { (2.0 * ml * mr / (ml * ml + mr * mr)) * (dot / (ml * mr)).max(0.0) } else { 0.0 };
            // piano: partials that begin with an attack (a jump in this bin) and then decay
            if mag[k] > 2.0 * prev_mag[k] + 1e-6 {
                onset_env[k] = 1.0;
            } else {
                onset_env[k] *= 0.93;
            }
            // highs that keep sounding (cymbals) vs. short ones (hi-hats)
            decay_env[k] = if mag[k] > decay_env[k] { mag[k] } else { decay_env[k] * 0.9 + mag[k] * 0.1 };
            let sustained = if decay_env[k] > 1e-9 { (mag[k] / decay_env[k]).min(1.0) } else { 0.0 };
            let bass_shape = 1.0 - smoothstep(180.0, 320.0, f);
            let voice_band = smoothstep(120.0, 220.0, f) * (1.0 - smoothstep(7000.0, 11000.0, f));
            let bass = harm * bass_shape * smoothstep(25.0, 40.0, f);
            let vocal = harm * (1.0 - bass_shape) * voice_band * centre.powi(3);
            let rest = (harm - bass - vocal).max(0.0);
            let piano = rest * onset_env[k] * smoothstep(40.0, 60.0, f) * (1.0 - smoothstep(4000.0, 6000.0, f));
            let guitar = (rest - piano).max(0.0) * 0.7 * smoothstep(70.0, 100.0, f) * (1.0 - smoothstep(5000.0, 7000.0, f));
            let other = (rest - piano - guitar).max(0.0);
            let drums = perc;
            let sum = bass + vocal + piano + guitar + other + drums;
            let n = if sum > 1e-9 { 1.0 / sum } else { 0.0 };
            let main = [vocal * n, drums * n, bass * n, guitar * n, piano * n, other * n];
            if !kit {
                for (s, m) in main.iter().enumerate() {
                    masks[s][k] = if sum > 1e-9 { *m } else if s == 5 { 1.0 } else { 0.0 };
                }
            } else {
                let d = main[1];
                let kick = 1.0 - smoothstep(90.0, 160.0, f);
                let tom = smoothstep(80.0, 120.0, f) * (1.0 - smoothstep(350.0, 500.0, f)) * if snare_hit { 0.2 } else { 0.8 };
                let snare = if snare_hit { smoothstep(140.0, 180.0, f) * (1.0 - smoothstep(9000.0, 12000.0, f)) } else { 0.05 };
                let highs = smoothstep(4500.0, 6500.0, f);
                let hat = highs * (1.0 - sustained * 0.8);
                let cym = highs * sustained * 0.8;
                let oth = 0.05;
                let ks = kick + tom + snare + hat + cym + oth;
                let pieces = [kick, snare, tom, hat, cym, oth];
                let others = [main[0], main[2], main[3], main[4], main[5]];
                for (s, m) in others.iter().enumerate() {
                    masks[s][k] = *m;
                }
                for (j, pc) in pieces.iter().enumerate() {
                    masks[5 + j][k] = d * pc / ks;
                }
                if sum <= 1e-9 {
                    masks[4][k] = 1.0;
                }
            }
        }
        prev_mag.copy_from_slice(&mag);
        // resynthesise each stem's frame and overlap-add it
        let mut br = vec![0f32; N];
        let mut bi = vec![0f32; N];
        for (s, m) in masks.iter().enumerate() {
            for c in 0..2 {
                for k in 0..BINS {
                    br[k] = ring_re[cs][c][k] * m[k];
                    bi[k] = ring_im[cs][c][k] * m[k];
                }
                for k in 1..N / 2 {
                    br[N - k] = br[k];
                    bi[N - k] = -bi[k];
                }
                fft.inverse(&mut br, &mut bi);
                let o = &mut out[s];
                for i in 0..N {
                    let at = ft * HOP + i;
                    if at < total {
                        o[2 * at + c] += br[i] * win[i];
                    }
                }
            }
        }
        if ft % 64 == 0 {
            progress(ft as f32 / nframes as f32);
        }
    }
    progress(1.0);
    // sqrt-Hann² at 75 % overlap sums to 2: halve, and drop the padding
    stems.into_iter().zip(out).map(|(s, o)| {
        let v: Vec<f32> = o[2 * pad..2 * (pad + frames_in)].iter().map(|v| v * 0.5).collect();
        (s, v)
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn mix(rate: f32, secs: f32) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>) {
        let n = (rate * secs) as usize;
        let mut bass = vec![0f32; n * 2];
        let mut voice = vec![0f32; n * 2];
        let mut drums = vec![0f32; n * 2];
        let mut nz = 12345u32;
        for i in 0..n {
            let t = i as f32 / rate;
            // bass: 55 Hz sustained, both channels
            let b = 0.3 * (std::f32::consts::TAU * 55.0 * t).sin();
            // "voice": a sustained 440 Hz tone with harmonics, centred
            let v = 0.2 * ((std::f32::consts::TAU * 440.0 * t).sin() + 0.5 * (std::f32::consts::TAU * 880.0 * t).sin() + 0.3 * (std::f32::consts::TAU * 1320.0 * t).sin());
            // drums: noise bursts every 0.5 s decaying in 30 ms
            nz ^= nz << 13;
            nz ^= nz >> 17;
            nz ^= nz << 5;
            let ph = t % 0.5;
            let d = (nz as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.6 * (-ph / 0.03).exp();
            bass[2 * i] = b;
            bass[2 * i + 1] = b;
            voice[2 * i] = v;
            voice[2 * i + 1] = v;
            drums[2 * i] = d;
            drums[2 * i + 1] = d * 0.8;
        }
        let m: Vec<f32> = (0..n * 2).map(|i| bass[i] + voice[i] + drums[i]).collect();
        (m, bass, voice, drums)
    }

    #[test]
    fn stems_sum_back_to_the_mix() {
        let rate = 48000.0;
        let (m, ..) = mix(rate, 2.0);
        let st = split(&m, rate, false, &mut |_| {});
        let sum: Vec<f32> = (0..m.len()).map(|i| st.iter().map(|s| s.1[i]).sum::<f32>()).collect();
        let err: Vec<f32> = sum.iter().zip(&m).map(|(a, b)| a - b).collect();
        assert!(rms(&err) < 1e-3 * rms(&m).max(1e-6) + 1e-5, "{}", rms(&err));
        let st = split(&m, rate, true, &mut |_| {});
        let sum: Vec<f32> = (0..m.len()).map(|i| st.iter().map(|s| s.1[i]).sum::<f32>()).collect();
        let err: Vec<f32> = sum.iter().zip(&m).map(|(a, b)| a - b).collect();
        assert!(rms(&err) < 1e-3 * rms(&m) + 1e-5);
    }

    #[test]
    fn each_part_lands_mostly_in_its_stem() {
        let rate = 48000.0;
        let (m, bass, voice, drums) = mix(rate, 3.0);
        let st = split(&m, rate, false, &mut |_| {});
        let get = |s: Stem| &st.iter().find(|x| x.0 == s).unwrap().1;
        // correlation of each stem with its source
        let corr = |a: &[f32], b: &[f32]| {
            let (mut ab, mut aa, mut bb) = (0f64, 0f64, 0f64);
            for (x, y) in a.iter().zip(b) {
                ab += (*x as f64) * (*y as f64);
                aa += (*x as f64).powi(2);
                bb += (*y as f64).powi(2);
            }
            ab / (aa * bb).sqrt().max(1e-20)
        };
        let cb = corr(get(Stem::Bass), &bass);
        let cv = corr(get(Stem::Vocals), &voice);
        let cd = corr(get(Stem::Drums), &drums);
        assert!(cb > 0.8, "bass {cb}");
        assert!(cv > 0.6, "vocals {cv}");
        assert!(cd > 0.6, "drums {cd}");
    }

    #[test]
    fn kick_goes_to_kick_hats_to_hihat() {
        let rate = 48000.0;
        let n = (rate * 2.0) as usize;
        let mut x = vec![0f32; n * 2];
        let mut nz = 99u32;
        for i in 0..n {
            let t = i as f32 / rate;
            let ph = t % 0.5;
            // kick: 55 Hz thump at the beat; hats: high noise ticks between
            let kick = 0.6 * (std::f32::consts::TAU * 55.0 * ph).sin() * (-ph / 0.08).exp();
            nz ^= nz << 13;
            nz ^= nz >> 17;
            nz ^= nz << 5;
            let ph2 = (t + 0.25) % 0.5;
            let noise = nz as f32 / u32::MAX as f32 * 2.0 - 1.0;
            // crude high-pass by differencing
            let hat = 0.3 * noise * (-ph2 / 0.015).exp();
            x[2 * i] = kick + hat;
            x[2 * i + 1] = kick + hat;
        }
        let st = split(&x, rate, true, &mut |_| {});
        let e = |s: Stem| rms(&st.iter().find(|y| y.0 == s).unwrap().1);
        assert!(e(Stem::Kick) > e(Stem::Snare), "{} {}", e(Stem::Kick), e(Stem::Snare));
        assert!(e(Stem::HiHat) + e(Stem::Cymbal) > 0.01, "{}", e(Stem::HiHat));
    }
}
