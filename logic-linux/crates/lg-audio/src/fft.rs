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
}
