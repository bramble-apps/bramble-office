//! The waveform overview: each channel's minimum and maximum per block of frames, at every
//! power-of-two block size from 32 frames up (a mipmap), so any zoom of any length of file
//! draws from about one entry per pixel. Closer than 32 frames a pixel reads the samples.

#[derive(Clone, Debug, Default)]
pub struct Overview {
    pub channels: usize,
    pub frames: usize,
    /// levels[l][ch]: (min, max) per block of `BASE << l` frames.
    levels: Vec<Vec<Vec<(f32, f32)>>>,
}

pub const BASE: usize = 32;

impl Overview {
    /// From interleaved samples.
    pub fn new(data: &[f32], channels: usize) -> Overview {
        let channels = channels.max(1);
        let frames = data.len() / channels;
        let mut level0 = vec![Vec::with_capacity(frames / BASE + 1); channels];
        for block in data.chunks(BASE * channels) {
            for (c, l) in level0.iter_mut().enumerate() {
                let mut mn = f32::MAX;
                let mut mx = f32::MIN;
                for s in block.iter().skip(c).step_by(channels) {
                    mn = mn.min(*s);
                    mx = mx.max(*s);
                }
                l.push((mn, mx));
            }
        }
        let mut levels = vec![level0];
        while levels.last().is_some_and(|l| l[0].len() > 1) {
            let prev = levels.last().unwrap();
            let next: Vec<Vec<(f32, f32)>> = prev.iter().map(|ch| ch.chunks(2).map(|p| p.iter().fold((f32::MAX, f32::MIN), |a, b| (a.0.min(b.0), a.1.max(b.1)))).collect()).collect();
            levels.push(next);
        }
        Overview { channels, frames, levels }
    }

    /// Min and max of frames `start..end` in channel `ch` (None: all channels together),
    /// read from the coarsest level that still resolves the span (`data` for closer looks).
    pub fn range(&self, data: &[f32], ch: Option<usize>, start: f64, end: f64) -> (f32, f32) {
        let s = start.max(0.0) as usize;
        let e = (end.max(start + 1.0).ceil() as usize).min(self.frames);
        if s >= e {
            return (0.0, 0.0);
        }
        let chans: Vec<usize> = match ch {
            Some(c) => vec![c.min(self.channels - 1)],
            None => (0..self.channels).collect(),
        };
        let span = e - s;
        let mut mn = f32::MAX;
        let mut mx = f32::MIN;
        if span < BASE * 2 || self.levels.is_empty() {
            for f in s..e {
                for &c in &chans {
                    let v = data.get(f * self.channels + c).copied().unwrap_or(0.0);
                    mn = mn.min(v);
                    mx = mx.max(v);
                }
            }
            return (mn, mx);
        }
        // the level whose block is at most half the span
        let mut l = 0;
        while l + 1 < self.levels.len() && (BASE << (l + 1)) * 2 <= span {
            l += 1;
        }
        let block = BASE << l;
        for &c in &chans {
            let lv = &self.levels[l][c];
            for b in s / block..e.div_ceil(block).min(lv.len()) {
                mn = mn.min(lv[b].0);
                mx = mx.max(lv[b].1);
            }
        }
        (mn, mx)
    }

    /// `n` pixel columns from frame `start`, `frames_per_px` frames each.
    pub fn columns(&self, data: &[f32], ch: Option<usize>, start: f64, frames_per_px: f64, n: usize) -> Vec<(f32, f32)> {
        (0..n).map(|i| {
            let a = start + i as f64 * frames_per_px;
            if a >= self.frames as f64 || a + frames_per_px <= 0.0 {
                return (0.0, 0.0);
            }
            self.range(data, ch, a, a + frames_per_px)
        }).collect()
    }

    /// The file's peak (largest absolute sample).
    pub fn peak(&self) -> f32 {
        self.levels.last().map_or(0.0, |l| l.iter().map(|c| c.first().map_or(0.0, |p| p.0.abs().max(p.1.abs()))).fold(0.0, f32::max))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_agree_with_the_samples() {
        let n = 100_003;
        let data: Vec<f32> = (0..n * 2).map(|i| ((i as f32 * 0.0137).sin() * (i % 7) as f32 / 7.0)).collect();
        let o = Overview::new(&data, 2);
        assert_eq!(o.frames, n);
        for (s, e) in [(0.0, 10.0), (5.0, 70.0), (1000.0, 9000.0), (0.0, n as f64), (99_000.0, 100_003.0)] {
            let (mn, mx) = o.range(&data, None, s, e);
            let raw = data[s as usize * 2..e as usize * 2].iter().fold((f32::MAX, f32::MIN), |a, b| (a.0.min(*b), a.1.max(*b)));
            // coarse levels may reach a little past the span, never miss a peak inside it
            assert!(mn <= raw.0 + 1e-6 && mx >= raw.1 - 1e-6, "{s}..{e}");
        }
        assert!((o.peak() - data.iter().fold(0.0f32, |a, b| a.max(b.abs()))).abs() < 1e-6);
        let cols = o.columns(&data, Some(1), 0.0, 1000.0, 120);
        assert_eq!(cols.len(), 120);
        assert_eq!(cols[110], (0.0, 0.0));
    }
}
