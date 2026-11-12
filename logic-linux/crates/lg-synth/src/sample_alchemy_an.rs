//! Sample Alchemy's analysis of the loaded sample: a short-time spectrum cut into log-spaced
//! bands (spectral resynthesis), the harmonic partials with their pitch wobble (additive
//! resynthesis) and the onsets (the Snap to Transients choice). Done once when a sample is
//! loaded; the instrument only reads the tables while it plays.

use std::f32::consts::PI;

/// Spectral bands (log-spaced from 40 Hz upward), each refilled with a sine or filtered noise.
pub const BANDS: usize = 128;
/// Harmonics the additive engine follows.
pub const PARTIALS: usize = 64;

/// Radix-2 FFT in place (re, im), `n` a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (i + k, i + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
            i += len;
        }
        len <<= 1;
    }
}

#[derive(Clone, Default)]
pub struct Analysis {
    pub rate: f32,
    /// Samples between frames.
    pub hop: usize,
    pub frames: usize,
    /// Band centres in Hz.
    pub band_hz: Vec<f32>,
    /// Band amplitude per frame (frames × BANDS).
    pub bands: Vec<f32>,
    /// Fundamental per frame in Hz (the median where the frame is unpitched).
    pub f0: Vec<f32>,
    /// Harmonic amplitudes per frame (frames × PARTIALS).
    pub amp: Vec<f32>,
    /// Measured partial frequency over k·f0 (≈1; the inharmonic wobble).
    pub dev: Vec<f32>,
    /// Loudness of each frame (rms of the partial amplitudes).
    pub level: Vec<f32>,
    pub f0_med: f32,
    /// Onsets as positions 0…1 of the sample.
    pub onsets: Vec<f32>,
}

impl Analysis {
    pub fn band_ratio(&self) -> f32 {
        if self.band_hz.len() < 2 {
            return 1.1;
        }
        self.band_hz[1] / self.band_hz[0]
    }

    /// Analyses `x` (mono at `rate`); at most the first 60 seconds.
    pub fn new(x: &[f32], rate: f32) -> Analysis {
        let x = &x[..x.len().min((rate * 60.0) as usize)];
        let mut n = 1024usize;
        while (n as f32) < rate * 0.08 && n < 8192 {
            n <<= 1;
        }
        let hop = n / 4;
        let frames = x.len() / hop + 1;
        let lo = 40.0f32;
        let hi = (rate * 0.45).min(16000.0).max(lo * 4.0);
        let ratio = (hi / lo).powf(1.0 / (BANDS as f32 - 1.0));
        let band_hz: Vec<f32> = (0..BANDS).map(|k| lo * ratio.powi(k as i32)).collect();
        let mut a = Analysis {
            rate,
            hop,
            frames,
            band_hz,
            bands: vec![0.0; frames * BANDS],
            f0: vec![0.0; frames],
            amp: vec![0.0; frames * PARTIALS],
            dev: vec![1.0; frames * PARTIALS],
            level: vec![0.0; frames],
            f0_med: 0.0,
            onsets: Vec::new(),
        };
        if x.len() < 64 {
            a.f0_med = 220.0;
            a.f0.fill(220.0);
            return a;
        }
        let win: Vec<f32> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos()).collect();
        let wsum: f32 = win.iter().sum::<f32>() / 2.0;
        let (mut re, mut im) = (vec![0.0f32; n], vec![0.0f32; n]);
        let mut mag = vec![0.0f32; n / 2 + 1];
        let bin_hz = rate / n as f32;
        let mut voiced = vec![false; frames];
        let mut flux = vec![0.0f32; frames];
        let mut prev_total = 0.0f32;
        for f in 0..frames {
            let start = f as i64 * hop as i64 - (n / 2) as i64;
            for i in 0..n {
                let k = start + i as i64;
                let v = if k >= 0 && (k as usize) < x.len() { x[k as usize] } else { 0.0 };
                re[i] = v * win[i];
                im[i] = 0.0;
            }
            fft(&mut re, &mut im);
            for b in 0..=n / 2 {
                mag[b] = (re[b] * re[b] + im[b] * im[b]).sqrt() / wsum;
            }
            // bands
            let row = &mut a.bands[f * BANDS..(f + 1) * BANDS];
            let half = ratio.sqrt();
            let mut total = 0.0f32;
            for (k, out) in row.iter_mut().enumerate() {
                let c = a.band_hz[k];
                let (b0, b1) = ((c / half / bin_hz).floor() as usize, (c * half / bin_hz).ceil() as usize);
                let b1 = b1.min(n / 2);
                *out = if b1 > b0 + 1 {
                    (mag[b0.max(1)..=b1].iter().map(|m| m * m).sum::<f32>()).sqrt() * 0.816
                } else {
                    let p = c / bin_hz;
                    let i = (p as usize).min(n / 2 - 1);
                    mag[i] + (mag[i + 1] - mag[i]) * (p - i as f32)
                };
                total += *out * *out;
            }
            let total = total.sqrt();
            flux[f] = (total - prev_total).max(0.0);
            prev_total = total;
            // fundamental: harmonic product spectrum over the lower half
            let (fl, fh) = ((60.0 / bin_hz).ceil() as usize, ((1500.0 / bin_hz) as usize).min(n / 16));
            let mut best = (0usize, 0.0f32);
            if total > 1e-4 {
                for b in fl.max(2)..fh.max(3) {
                    let mut s = 0.0f32;
                    for h in 1..=4 {
                        s += (mag[(b * h).min(n / 2)] + 1e-7).ln();
                    }
                    if s > best.1 || best.0 == 0 {
                        best = (b, s);
                    }
                }
            }
            let mut f0 = 0.0;
            if best.0 > 0 {
                let b = best.0;
                // is it a real peak? compare against the average level near it
                let peak = mag[b - 1..=b + 1].iter().cloned().fold(0.0, f32::max);
                let lo_b = b.saturating_sub(8).max(1);
                let avg = mag[lo_b..=(b + 8).min(n / 2)].iter().sum::<f32>() / ((b + 8).min(n / 2) - lo_b + 1) as f32;
                if peak > 2.5 * avg && peak > 1e-3 {
                    f0 = peak_freq(&mag, b, bin_hz);
                }
            }
            let f0_use = if f0 > 0.0 { f0 } else { 0.0 };
            // partials around k·f0 (voiced) or read off the envelope at k·f0_med later
            if f0_use > 0.0 {
                let (mut wsum2, mut fsum) = (0.0f32, 0.0f32);
                let mut pf = [0.0f32; PARTIALS];
                for k in 1..=PARTIALS {
                    let fk = f0_use * k as f32;
                    if fk > rate * 0.45 {
                        break;
                    }
                    let c = (fk / bin_hz).round() as usize;
                    let w = ((f0_use / bin_hz * 0.45) as usize).max(1);
                    let (b0, b1) = (c.saturating_sub(w).max(1), (c + w).min(n / 2 - 1));
                    let mut pb = b0;
                    for b in b0..=b1 {
                        if mag[b] > mag[pb] {
                            pb = b;
                        }
                    }
                    let fr = peak_freq(&mag, pb, bin_hz);
                    pf[k - 1] = fr;
                    a.amp[f * PARTIALS + k - 1] = mag[pb];
                    if k <= 6 {
                        wsum2 += mag[pb];
                        fsum += mag[pb] * fr / k as f32;
                    }
                }
                let f0r = if wsum2 > 0.0 { fsum / wsum2 } else { f0_use };
                a.f0[f] = f0r;
                for k in 1..=PARTIALS {
                    if pf[k - 1] > 0.0 {
                        a.dev[f * PARTIALS + k - 1] = (pf[k - 1] / (k as f32 * f0r)).clamp(0.9, 1.1);
                    }
                }
                voiced[f] = true;
            }
            // keep the magnitude spectrum for unvoiced frames' partials
            if f0_use <= 0.0 {
                // stash: amplitudes sampled at multiples of 100 Hz-ish comb are filled below
                for k in 0..PARTIALS {
                    let p = (k + 1) as f32 * 110.0 / bin_hz;
                    let i = (p as usize).min(n / 2 - 1);
                    a.amp[f * PARTIALS + k] = mag[i] + (mag[i + 1] - mag[i]) * (p - i as f32);
                }
            }
        }
        // median fundamental of the pitched frames
        let mut v: Vec<f32> = (0..frames).filter(|&f| voiced[f]).map(|f| a.f0[f]).collect();
        v.sort_by(|p, q| p.partial_cmp(q).unwrap());
        a.f0_med = if v.is_empty() { 110.0 } else { v[v.len() / 2] };
        for f in 0..frames {
            if !voiced[f] {
                a.f0[f] = a.f0_med;
                // the amplitudes were taken at a 110 Hz comb: resample them at k·f0_med
                let tmp: Vec<f32> = a.amp[f * PARTIALS..(f + 1) * PARTIALS].to_vec();
                for k in 0..PARTIALS {
                    let fk = (k + 1) as f32 * a.f0_med;
                    let p = (fk / 110.0 - 1.0).clamp(0.0, PARTIALS as f32 - 1.0);
                    let i = (p as usize).min(PARTIALS - 2);
                    a.amp[f * PARTIALS + k] = if fk / 110.0 > PARTIALS as f32 { 0.0 } else { tmp[i] + (tmp[i + 1] - tmp[i]) * (p - i as f32) };
                }
            }
            let e: f32 = a.amp[f * PARTIALS..(f + 1) * PARTIALS].iter().map(|v| v * v).sum();
            a.level[f] = (e / PARTIALS as f32).sqrt();
        }
        // onsets: peaks of the spectral flux above a moving threshold
        let mean = flux.iter().sum::<f32>() / frames as f32;
        let mut last = -100i64;
        for f in 1..frames.saturating_sub(1) {
            if flux[f] > mean * 2.5 && flux[f] >= flux[f - 1] && flux[f] > flux[f + 1] && f as i64 - last > 3 {
                a.onsets.push((f * hop) as f32 / x.len() as f32);
                last = f as i64;
            }
        }
        a
    }
}

/// Parabolic peak position of bin `b` in Hz.
fn peak_freq(mag: &[f32], b: usize, bin_hz: f32) -> f32 {
    if b == 0 || b + 1 >= mag.len() {
        return b as f32 * bin_hz;
    }
    let (l, c, r) = ((mag[b - 1] + 1e-9).ln(), (mag[b] + 1e-9).ln(), (mag[b + 1] + 1e-9).ln());
    let d = l - 2.0 * c + r;
    let off = if d.abs() > 1e-9 { (0.5 * (l - r) / d).clamp(-0.5, 0.5) } else { 0.0 };
    (b as f32 + off) * bin_hz
}
