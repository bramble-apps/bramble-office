//! Recording: a WAV file written while the audio comes in (the header's sizes filled in when it
//! ends, so a recording cut short is still a file most readers open), and joining a comp's pieces
//! into one file (Flatten and Merge) with short crossfades at the joins.

use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::file::{put_sample, AudioFile};

pub struct WavWriter {
    file: io::BufWriter<File>,
    pub path: PathBuf,
    pub rate: u32,
    pub channels: usize,
    bits: u16,
    float: bool,
    frames: u64,
    buf: Vec<u8>,
}

impl WavWriter {
    /// A new file: `bits` 16 or 24 (PCM) or 32 (float).
    pub fn create(path: &Path, rate: u32, channels: usize, bits: u16) -> io::Result<WavWriter> {
        let float = bits == 32;
        let bits = if float { 32 } else if bits <= 16 { 16 } else { 24 };
        let mut w = WavWriter { file: io::BufWriter::new(File::create(path)?), path: path.to_path_buf(), rate, channels: channels.max(1), bits, float, frames: 0, buf: Vec::new() };
        let header = w.header(0);
        w.file.write_all(&header)?;
        Ok(w)
    }

    fn header(&self, data_bytes: u32) -> Vec<u8> {
        let bps = self.bits as usize / 8;
        let mut h = Vec::with_capacity(44);
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        h.extend_from_slice(b"WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&(if self.float { 3u16 } else { 1 }).to_le_bytes());
        h.extend_from_slice(&(self.channels as u16).to_le_bytes());
        h.extend_from_slice(&self.rate.to_le_bytes());
        h.extend_from_slice(&((self.rate as usize * self.channels * bps) as u32).to_le_bytes());
        h.extend_from_slice(&((self.channels * bps) as u16).to_le_bytes());
        h.extend_from_slice(&self.bits.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&data_bytes.to_le_bytes());
        h
    }

    /// Interleaved samples (whole frames).
    pub fn write(&mut self, samples: &[f32]) -> io::Result<()> {
        self.buf.clear();
        for &s in samples {
            put_sample(&mut self.buf, s, self.bits, self.float, false);
        }
        self.frames += (samples.len() / self.channels) as u64;
        self.file.write_all(&self.buf)
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn seconds(&self) -> f64 {
        self.frames as f64 / self.rate.max(1) as f64
    }

    /// Ends the file: the sizes written into the header. Returns its length in frames.
    pub fn finish(mut self) -> io::Result<u64> {
        let data = (self.frames * (self.channels * self.bits as usize / 8) as u64).min(u32::MAX as u64 - 64) as u32;
        if data % 2 == 1 {
            self.file.write_all(&[0])?;
        }
        let header = self.header(data);
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header)?;
        self.file.flush()?;
        Ok(self.frames)
    }
}

/// One piece of a comp: `frames` frames of `src` from `from`, placed at `at` (frames from the
/// comp's start).
pub struct Piece<'a> {
    pub src: &'a AudioFile,
    pub from: usize,
    pub frames: usize,
    pub at: usize,
}

/// The pieces as one file of `total` frames (rate and channels of the first piece; silence
/// where none plays), each join crossfaded over `fade` frames (equal gain).
pub fn join(pieces: &[Piece], total: usize, fade: usize) -> AudioFile {
    let (rate, ch) = pieces.first().map_or((48_000, 1), |p| (p.src.rate, p.src.channels));
    let mut data = vec![0f32; total * ch];
    for (i, p) in pieces.iter().enumerate() {
        // a piece after another starts its fade a little early, under the end of the one before
        let lead = if i > 0 { fade.min(p.at).min(p.from) } else { 0 };
        let n = p.frames + lead;
        for k in 0..n {
            let dst = p.at - lead + k;
            if dst >= total {
                break;
            }
            let src = p.from - lead + k;
            let g_in = if lead > 0 && k < lead { k as f32 / lead as f32 } else { 1.0 };
            for c in 0..ch {
                let x = p.src.at(src, c) * g_in;
                let d = &mut data[dst * ch + c];
                // under the fade, the piece before fades out as this one fades in
                *d = if k < lead { *d * (1.0 - g_in) + x } else { x };
            }
        }
    }
    let mut out = AudioFile::new(rate, ch, data);
    if let Some(p) = pieces.first() {
        out.bits = p.src.bits;
        out.float = p.src.float;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("lg-record-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    #[test]
    fn writes_a_wav_as_it_comes_in() {
        let p = tmp("a.wav");
        let mut w = WavWriter::create(&p, 48_000, 2, 24).unwrap();
        let sine: Vec<f32> = (0..2400).flat_map(|i| {
            let x = (i as f32 * 0.05).sin() * 0.5;
            [x, -x]
        }).collect();
        for c in sine.chunks(130) {
            w.write(c).unwrap();
        }
        assert_eq!(w.frames(), 2400);
        assert_eq!(w.finish().unwrap(), 2400);
        let a = AudioFile::read(&p).unwrap();
        assert_eq!((a.rate, a.channels, a.bits, a.frames()), (48_000, 2, 24, 2400));
        for (x, y) in a.data.iter().zip(&sine) {
            assert!((x - y).abs() < 1e-6);
        }
    }

    #[test]
    fn sixteen_bit_and_float() {
        for (bits, tol) in [(16u16, 1e-4f32), (32, 0.0)] {
            let p = tmp(&format!("b{bits}.wav"));
            let mut w = WavWriter::create(&p, 44_100, 1, bits).unwrap();
            w.write(&[0.25, -0.5, 0.75]).unwrap();
            w.finish().unwrap();
            let a = AudioFile::read(&p).unwrap();
            assert_eq!((a.rate, a.channels, a.float, a.frames()), (44_100, 1, bits == 32, 3));
            assert!((a.data[1] + 0.5).abs() <= tol);
        }
    }

    #[test]
    fn an_empty_recording_is_still_a_file() {
        let p = tmp("c.wav");
        WavWriter::create(&p, 48_000, 1, 24).unwrap().finish().unwrap();
        assert_eq!(AudioFile::read(&p).unwrap().frames(), 0);
    }

    #[test]
    fn joins_pieces_with_crossfades() {
        let a = AudioFile::new(48_000, 1, vec![1.0; 1000]);
        let b = AudioFile::new(48_000, 1, vec![-1.0; 1000]);
        let out = join(&[Piece { src: &a, from: 0, frames: 500, at: 0 }, Piece { src: &b, from: 500, frames: 300, at: 500 }], 1000, 10);
        assert_eq!(out.frames(), 1000);
        assert_eq!(out.data[0], 1.0);
        assert_eq!(out.data[489], 1.0);
        // the fade: from a to b over 10 frames
        assert!(out.data[495].abs() < 1.0);
        assert_eq!(out.data[500], -1.0);
        assert_eq!(out.data[799], -1.0);
        // nothing after the last piece
        assert_eq!(out.data[800], 0.0);
    }
}
