//! Zero-latency partitioned convolution: a direct FIR for the first taps, then two levels of
//! FFT partitions (short blocks near the start of the response, long blocks for the tail), each
//! level delivering its part exactly when it is due. Buffers are sized once for the longest
//! response; `process` never allocates. Also a small radix-2 FFT used by it.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct C {
    pub re: f32,
    pub im: f32,
}

impl C {
    #[inline]
    fn mul(self, o: C) -> C {
        C { re: self.re * o.re - self.im * o.im, im: self.re * o.im + self.im * o.re }
    }
}

/// An in-place radix-2 FFT of a fixed size.
pub struct Fft {
    n: usize,
    tw: Vec<C>,
    rev: Vec<u32>,
}

impl Fft {
    pub fn new(n: usize) -> Fft {
        assert!(n.is_power_of_two());
        let tw = (0..n / 2).map(|k| {
            let a = -std::f64::consts::TAU * k as f64 / n as f64;
            C { re: a.cos() as f32, im: a.sin() as f32 }
        }).collect();
        let bits = n.trailing_zeros();
        let rev = (0..n as u32).map(|i| if bits == 0 { 0 } else { i.reverse_bits() >> (32 - bits) }).collect();
        Fft { n, tw, rev }
    }

    /// Forward (inverse = false) or inverse (unscaled) transform.
    pub fn run(&self, x: &mut [C], inverse: bool) {
        let n = self.n;
        for i in 0..n {
            let j = self.rev[i] as usize;
            if j > i {
                x.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= n {
            let step = n / len;
            for s in (0..n).step_by(len) {
                for k in 0..len / 2 {
                    let mut w = self.tw[k * step];
                    if inverse {
                        w.im = -w.im;
                    }
                    let a = x[s + k];
                    let b = x[s + k + len / 2].mul(w);
                    x[s + k] = C { re: a.re + b.re, im: a.im + b.im };
                    x[s + k + len / 2] = C { re: a.re - b.re, im: a.im - b.im };
                }
            }
            len *= 2;
        }
    }
}

/// One level of uniformly partitioned convolution (overlap-save, FFT size 2B), for the taps
/// from `offset` on (offset >= B), into an output ring.
struct Level {
    b: usize,
    offset: usize,
    parts: usize,
    used: usize,
    fft: Fft,
    /// partition spectra (parts x 2B)
    h: Vec<C>,
    /// frequency-domain delay line of input spectra (parts x 2B), newest at `fdl_pos`
    fdl: Vec<C>,
    fdl_pos: usize,
    /// the last 2B input samples
    inbuf: Vec<f32>,
    fill: usize,
    work: Vec<C>,
    acc: Vec<C>,
}

impl Level {
    fn new(b: usize, offset: usize, max_taps: usize) -> Level {
        let parts = max_taps.saturating_sub(offset).div_ceil(b).max(1);
        Level {
            b,
            offset,
            parts,
            used: 0,
            fft: Fft::new(2 * b),
            h: vec![C::default(); parts * 2 * b],
            fdl: vec![C::default(); parts * 2 * b],
            fdl_pos: 0,
            inbuf: vec![0.0; 2 * b],
            fill: 0,
            work: vec![C::default(); 2 * b],
            acc: vec![C::default(); 2 * b],
        }
    }

    /// Loads the taps [offset, offset + parts*B) of `ir`.
    fn load(&mut self, ir: &[f32]) {
        let b = self.b;
        self.used = 0;
        for p in 0..self.parts {
            let start = self.offset + p * b;
            let seg = &mut self.h[p * 2 * b..(p + 1) * 2 * b];
            seg.iter_mut().for_each(|c| *c = C::default());
            if start >= ir.len() {
                continue;
            }
            let end = (start + b).min(ir.len());
            let mut any = false;
            for (k, v) in ir[start..end].iter().enumerate() {
                seg[k].re = *v;
                any |= *v != 0.0;
            }
            if any {
                self.used = p + 1;
            }
            self.fft.run(seg, false);
        }
    }

    fn clear(&mut self) {
        self.fdl.iter_mut().for_each(|c| *c = C::default());
        self.inbuf.iter_mut().for_each(|x| *x = 0.0);
        self.fill = 0;
    }

    /// Takes one input sample; when a block is complete, adds its contribution into `out`
    /// (a ring of output samples; `now` = the ring index of the next output sample).
    #[inline]
    fn push(&mut self, x: f32, out: &mut [f32], now: usize) {
        let b = self.b;
        self.inbuf[b + self.fill] = x;
        self.fill += 1;
        if self.fill < b {
            return;
        }
        self.fill = 0;
        if self.used == 0 {
            self.inbuf.copy_within(b.., 0);
            return;
        }
        // spectrum of the last 2B inputs into the delay line
        self.fdl_pos = (self.fdl_pos + self.parts - 1) % self.parts;
        let slot = &mut self.fdl[self.fdl_pos * 2 * b..(self.fdl_pos + 1) * 2 * b];
        for k in 0..2 * b {
            slot[k] = C { re: self.inbuf[k], im: 0.0 };
        }
        self.fft.run(slot, false);
        self.inbuf.copy_within(b.., 0);
        // sum of spectra times partitions
        self.acc.iter_mut().for_each(|c| *c = C::default());
        for p in 0..self.used {
            let xi = (self.fdl_pos + p) % self.parts;
            let xs = &self.fdl[xi * 2 * b..(xi + 1) * 2 * b];
            let hs = &self.h[p * 2 * b..(p + 1) * 2 * b];
            for k in 0..2 * b {
                let (x, h) = (xs[k], hs[k]);
                self.acc[k].re += x.re * h.re - x.im * h.im;
                self.acc[k].im += x.re * h.im + x.im * h.re;
            }
        }
        self.work.copy_from_slice(&self.acc);
        self.fft.run(&mut self.work, true);
        let scale = 1.0 / (2 * b) as f32;
        // the last B outputs belong to the block just taken in; with the taps offset by
        // `offset`, they sound from now + (offset - B) on
        let n = out.len();
        let start = now + self.offset - b;
        for k in 0..b {
            out[(start + k) % n] += self.work[b + k].re * scale;
        }
    }
}

/// A mono convolver for one channel.
pub struct Convolver {
    head: Vec<f32>,
    head_len: usize,
    hist: Vec<f32>,
    hpos: usize,
    l1: Level,
    l2: Level,
    out: Vec<f32>,
    now: usize,
}

const HEAD: usize = 128;
const B1: usize = 128;
const P1: usize = 16;
const B2: usize = 2048;

impl Convolver {
    /// Room for responses up to `max_taps` long.
    pub fn new(max_taps: usize) -> Convolver {
        let l2_off = HEAD + B1 * P1;
        Convolver {
            head: vec![0.0; HEAD],
            head_len: 0,
            hist: vec![0.0; HEAD],
            hpos: 0,
            l1: Level::new(B1, HEAD, l2_off),
            l2: Level::new(B2, l2_off, max_taps.max(l2_off + 1)),
            out: vec![0.0; (B2 * 4).next_power_of_two() * 2],
            now: 0,
        }
    }

    pub fn max_taps(&self) -> usize {
        self.l2.offset + self.l2.parts * B2
    }

    pub fn load(&mut self, ir: &[f32]) {
        let ir = &ir[..ir.len().min(self.max_taps())];
        self.head.iter_mut().for_each(|x| *x = 0.0);
        self.head_len = ir.len().min(HEAD);
        self.head[..self.head_len].copy_from_slice(&ir[..self.head_len]);
        self.l1.load(ir);
        self.l2.load(ir);
        self.clear();
    }

    pub fn clear(&mut self) {
        self.hist.iter_mut().for_each(|x| *x = 0.0);
        self.out.iter_mut().for_each(|x| *x = 0.0);
        self.l1.clear();
        self.l2.clear();
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f32 {
        // direct part
        self.hpos = (self.hpos + 1) % HEAD;
        self.hist[self.hpos] = x;
        let mut y = 0.0;
        for k in 0..self.head_len {
            y += self.head[k] * self.hist[(self.hpos + HEAD - k) % HEAD];
        }
        let n = self.out.len();
        let i = self.now % n;
        y += self.out[i];
        self.out[i] = 0.0;
        self.now = self.now.wrapping_add(1);
        // the partitions (their output lands from the next sample on)
        self.l1.push(x, &mut self.out, self.now);
        self.l2.push(x, &mut self.out, self.now);
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_direct_convolution() {
        let mut nz = crate::util::Noise(17);
        let ir: Vec<f32> = (0..9000).map(|i| nz.next() * (-(i as f32) / 3000.0).exp()).collect();
        let x: Vec<f32> = (0..12000).map(|_| nz.next()).collect();
        let mut c = Convolver::new(20000);
        c.load(&ir);
        let y: Vec<f32> = x.iter().map(|v| c.run(*v)).collect();
        for n in [0usize, 1, 127, 128, 129, 2175, 2176, 2177, 5000, 11999] {
            let mut d = 0.0f64;
            for k in 0..ir.len().min(n + 1) {
                d += ir[k] as f64 * x[n - k] as f64;
            }
            assert!((y[n] as f64 - d).abs() < 1e-3, "n {n}: {} vs {d}", y[n]);
        }
    }
}
