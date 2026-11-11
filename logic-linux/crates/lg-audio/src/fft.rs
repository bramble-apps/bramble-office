//! A small in-place radix-2 FFT (power-of-two sizes) for the analysis.

/// Forward FFT of `re` / `im` (same power-of-two length), in place.
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);
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
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let a = start + k;
                let b = a + len / 2;
                let (xr, xi) = (re[b] as f64 * cr - im[b] as f64 * ci, re[b] as f64 * ci + im[b] as f64 * cr);
                re[b] = (re[a] as f64 - xr) as f32;
                im[b] = (im[a] as f64 - xi) as f32;
                re[a] = (re[a] as f64 + xr) as f32;
                im[a] = (im[a] as f64 + xi) as f32;
                let t = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = t;
            }
        }
        len <<= 1;
    }
}

/// The Hann window of `n` points.
pub fn hann(n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()).collect()
}

/// A planned FFT (twiddles and bit reversal computed once) for the frame-by-frame work of the
/// phase vocoder: forward and inverse, complex, power-of-two sizes.
pub struct Fft {
    n: usize,
    rev: Vec<u32>,
    cos: Vec<f32>,
    sin: Vec<f32>,
}

impl Fft {
    pub fn new(n: usize) -> Fft {
        assert!(n.is_power_of_two() && n >= 2);
        let bits = n.trailing_zeros();
        let rev = (0..n as u32).map(|i| i.reverse_bits() >> (32 - bits)).collect();
        let cos = (0..n / 2).map(|k| (2.0 * std::f64::consts::PI * k as f64 / n as f64).cos() as f32).collect();
        let sin = (0..n / 2).map(|k| -(2.0 * std::f64::consts::PI * k as f64 / n as f64).sin() as f32).collect();
        Fft { n, rev, cos, sin }
    }

    /// Forward transform in place (no scaling).
    pub fn forward(&self, re: &mut [f32], im: &mut [f32]) {
        let n = self.n;
        for i in 0..n {
            let j = self.rev[i] as usize;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let (wr, wi) = (self.cos[k * step], self.sin[k * step]);
                    let a = start + k;
                    let b = a + half;
                    let xr = re[b] * wr - im[b] * wi;
                    let xi = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - xr;
                    im[b] = im[a] - xi;
                    re[a] += xr;
                    im[a] += xi;
                }
            }
            len <<= 1;
        }
    }

    /// Inverse transform in place, scaled by 1/n.
    pub fn inverse(&self, re: &mut [f32], im: &mut [f32]) {
        im.iter_mut().for_each(|v| *v = -*v);
        self.forward(re, im);
        let k = 1.0 / self.n as f32;
        re.iter_mut().for_each(|v| *v *= k);
        im.iter_mut().for_each(|v| *v *= -k);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_sine_lands_in_its_bin() {
        let n = 256;
        let mut re: Vec<f32> = (0..n).map(|i| (2.0 * std::f32::consts::PI * 8.0 * i as f32 / n as f32).sin()).collect();
        let mut im = vec![0.0; n];
        super::fft(&mut re, &mut im);
        let mag: Vec<f32> = re.iter().zip(&im).map(|(a, b)| (a * a + b * b).sqrt()).collect();
        let best = (0..n / 2).max_by(|&a, &b| mag[a].total_cmp(&mag[b])).unwrap();
        assert_eq!(best, 8);
        assert!((mag[8] - n as f32 / 2.0).abs() < 0.5);
    }

    #[test]
    fn the_planned_fft_matches_and_inverts() {
        let n = 512;
        let x: Vec<f32> = (0..n).map(|i| ((i * 37 % 101) as f32 / 50.0) - 1.0).collect();
        let (mut r1, mut i1) = (x.clone(), vec![0.0; n]);
        super::fft(&mut r1, &mut i1);
        let p = super::Fft::new(n);
        let (mut r2, mut i2) = (x.clone(), vec![0.0; n]);
        p.forward(&mut r2, &mut i2);
        for k in 0..n {
            assert!((r1[k] - r2[k]).abs() < 1e-3 && (i1[k] - i2[k]).abs() < 1e-3, "{k}");
        }
        p.inverse(&mut r2, &mut i2);
        for k in 0..n {
            assert!((r2[k] - x[k]).abs() < 1e-4 && i2[k].abs() < 1e-4, "{k}");
        }
    }
}
