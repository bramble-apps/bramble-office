//! WAV and AIFF / AIFC files as samples (interleaved f32, -1..1) at their own rate, channels and
//! bit depth, written back the same way. Chunks other than the format and the samples are kept
//! and written again (Logic's 'LGBM' Smart Tempo beat map, markers, 'bext'…), except Logic's
//! waveform overviews ('LGWV', 'ovvw'), which a changed file makes stale.

use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Format {
    #[default]
    Wav,
    Aiff,
}

impl Format {
    /// By the file name's extension (.aif, .aiff, .aifc: AIFF; else WAV).
    pub fn of(path: &Path) -> Format {
        match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("aif" | "aiff" | "aifc") => Format::Aiff,
            _ => Format::Wav,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioFile {
    pub rate: u32,
    pub channels: usize,
    /// Bits per sample as stored (8, 16, 24, 32; 32 or 64 with `float`).
    pub bits: u16,
    pub float: bool,
    pub format: Format,
    /// Interleaved samples, -1..1.
    pub data: Vec<f32>,
    /// Other chunks, in file order (id, bytes).
    pub extra: Vec<([u8; 4], Vec<u8>)>,
}

/// Chunks dropped when writing (Logic's overview caches: rebuilt from the samples).
const STALE: &[&[u8; 4]] = &[b"LGWV", b"ovvw"];

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

impl AudioFile {
    pub fn new(rate: u32, channels: usize, data: Vec<f32>) -> AudioFile {
        AudioFile { rate, channels: channels.max(1), bits: 24, float: false, format: Format::Wav, data, extra: Vec::new() }
    }

    pub fn frames(&self) -> usize {
        self.data.len() / self.channels.max(1)
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / self.rate.max(1) as f64
    }

    /// One sample of a frame (channel clamped).
    pub fn at(&self, frame: usize, ch: usize) -> f32 {
        self.data.get(frame * self.channels + ch.min(self.channels - 1)).copied().unwrap_or(0.0)
    }

    /// The channels mixed to one.
    pub fn mono(&self) -> Vec<f32> {
        let c = self.channels.max(1);
        self.data.chunks(c).map(|f| f.iter().sum::<f32>() / c as f32).collect()
    }

    /// Frames `range` as a new file of the same kind.
    pub fn slice(&self, range: std::ops::Range<usize>) -> AudioFile {
        let r = range.start.min(self.frames())..range.end.min(self.frames());
        let data = self.data[r.start * self.channels..r.end * self.channels].to_vec();
        AudioFile { data, extra: Vec::new(), ..self.clone() }
    }

    /// Stereo at 48 kHz, as Studio's engine plays (linear resampling; mono doubled).
    pub fn stereo_48k(&self) -> Vec<f32> {
        let out_rate = 48_000.0;
        let ratio = self.rate as f64 / out_rate;
        let n = (self.frames() as f64 / ratio) as usize;
        let mut out = Vec::with_capacity(n * 2);
        for k in 0..n {
            let src = k as f64 * ratio;
            let i = src as usize;
            let t = (src - i as f64) as f32;
            for c in 0..2 {
                let a = self.at(i, c);
                let b = if i + 1 < self.frames() { self.at(i + 1, c) } else { a };
                out.push(a + (b - a) * t);
            }
        }
        out
    }

    pub fn read(path: &Path) -> io::Result<AudioFile> {
        AudioFile::parse(&std::fs::read(path)?)
    }

    pub fn parse(b: &[u8]) -> io::Result<AudioFile> {
        if b.len() >= 12 && (&b[0..4] == b"RIFF" || &b[0..4] == b"RF64") && &b[8..12] == b"WAVE" {
            parse_wav(b)
        } else if b.len() >= 12 && &b[0..4] == b"FORM" && (&b[8..12] == b"AIFF" || &b[8..12] == b"AIFC") {
            parse_aiff(b)
        } else {
            Err(bad("not a WAV or AIFF file"))
        }
    }

    /// Writes the file in its format (the extension's when it has none set).
    pub fn write(&self, path: &Path) -> io::Result<()> {
        let bytes = match self.format {
            Format::Wav => self.wav_bytes(),
            Format::Aiff => self.aiff_bytes(),
        };
        // write beside, then rename: a failed write never leaves half a file
        let tmp = path.with_extension("lgaudio-tmp");
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, path)
    }

    pub fn wav_bytes(&self) -> Vec<u8> {
        let (bits, float) = if self.float { (32, true) } else { (self.bits.clamp(8, 32) / 8 * 8, false) };
        let bps = bits as usize / 8;
        let mut out = Vec::with_capacity(self.data.len() * bps + 64);
        out.extend_from_slice(b"RIFF\0\0\0\0WAVE");
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&(if float { 3u16 } else { 1 }).to_le_bytes());
        fmt.extend_from_slice(&(self.channels as u16).to_le_bytes());
        fmt.extend_from_slice(&self.rate.to_le_bytes());
        fmt.extend_from_slice(&((self.rate as usize * self.channels * bps) as u32).to_le_bytes());
        fmt.extend_from_slice(&((self.channels * bps) as u16).to_le_bytes());
        fmt.extend_from_slice(&(bits as u16).to_le_bytes());
        chunk(&mut out, b"fmt ", &fmt, false);
        let mut data = Vec::with_capacity(self.data.len() * bps);
        for &s in &self.data {
            put_sample(&mut data, s, bits, float, false);
        }
        chunk(&mut out, b"data", &data, false);
        for (id, body) in &self.extra {
            if !STALE.contains(&id) {
                chunk(&mut out, id, body, false);
            }
        }
        let n = (out.len() - 8) as u32;
        out[4..8].copy_from_slice(&n.to_le_bytes());
        out
    }

    pub fn aiff_bytes(&self) -> Vec<u8> {
        let float = self.float;
        let bits = if float { 32 } else { self.bits.clamp(8, 32) / 8 * 8 };
        let bps = bits as usize / 8;
        let mut out = Vec::with_capacity(self.data.len() * bps + 64);
        out.extend_from_slice(b"FORM\0\0\0\0");
        out.extend_from_slice(if float { b"AIFC" } else { b"AIFF" });
        if float {
            chunk(&mut out, b"FVER", &0xA280_5140u32.to_be_bytes(), true);
        }
        let mut comm = Vec::new();
        comm.extend_from_slice(&(self.channels as u16).to_be_bytes());
        comm.extend_from_slice(&(self.frames() as u32).to_be_bytes());
        comm.extend_from_slice(&(bits as u16).to_be_bytes());
        comm.extend_from_slice(&extended(self.rate as f64));
        if float {
            comm.extend_from_slice(b"fl32");
            comm.extend_from_slice(&[0, 0]);
        }
        chunk(&mut out, b"COMM", &comm, true);
        let mut ssnd = vec![0u8; 8];
        for &s in &self.data {
            put_sample(&mut ssnd, s, bits, float, true);
        }
        chunk(&mut out, b"SSND", &ssnd, true);
        for (id, body) in &self.extra {
            if !STALE.contains(&id) {
                chunk(&mut out, id, body, true);
            }
        }
        let n = (out.len() - 8) as u32;
        out[4..8].copy_from_slice(&n.to_be_bytes());
        out
    }
}

fn chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8], be: bool) {
    out.extend_from_slice(id);
    let n = body.len() as u32;
    out.extend_from_slice(&if be { n.to_be_bytes() } else { n.to_le_bytes() });
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
}

fn put_sample(out: &mut Vec<u8>, s: f32, bits: u16, float: bool, be: bool) {
    if float {
        out.extend_from_slice(&if be { s.to_be_bytes() } else { s.to_le_bytes() });
        return;
    }
    let s = s.clamp(-1.0, 1.0) as f64;
    match bits {
        8 => {
            let v = (s * 127.0).round() as i8;
            // WAV's 8 bits are unsigned, AIFF's signed
            out.push(if be { v as u8 } else { (v as i16 + 128) as u8 });
        }
        16 => {
            let v = (s * 32767.0).round() as i16;
            out.extend_from_slice(&if be { v.to_be_bytes() } else { v.to_le_bytes() });
        }
        24 => {
            let v = (s * 8_388_607.0).round() as i32;
            let b = v.to_le_bytes();
            if be {
                out.extend_from_slice(&[b[2], b[1], b[0]]);
            } else {
                out.extend_from_slice(&b[..3]);
            }
        }
        _ => {
            let v = (s * 2_147_483_647.0).round() as i32;
            out.extend_from_slice(&if be { v.to_be_bytes() } else { v.to_le_bytes() });
        }
    }
}

fn get_sample(b: &[u8], bits: u16, float: bool, be: bool) -> f32 {
    if float {
        return match bits {
            64 => {
                let a: [u8; 8] = b[..8].try_into().unwrap_or_default();
                (if be { f64::from_be_bytes(a) } else { f64::from_le_bytes(a) }) as f32
            }
            _ => {
                let a: [u8; 4] = b[..4].try_into().unwrap_or_default();
                if be { f32::from_be_bytes(a) } else { f32::from_le_bytes(a) }
            }
        };
    }
    match bits {
        8 => {
            if be {
                b[0] as i8 as f32 / 128.0
            } else {
                (b[0] as f32 - 128.0) / 128.0
            }
        }
        16 => {
            let v = if be { i16::from_be_bytes([b[0], b[1]]) } else { i16::from_le_bytes([b[0], b[1]]) };
            v as f32 / 32768.0
        }
        24 => {
            let v = if be { i32::from_be_bytes([b[0], b[1], b[2], 0]) } else { i32::from_le_bytes([0, b[0], b[1], b[2]]) } >> 8;
            v as f32 / 8_388_608.0
        }
        _ => {
            let v = if be { i32::from_be_bytes([b[0], b[1], b[2], b[3]]) } else { i32::from_le_bytes([b[0], b[1], b[2], b[3]]) };
            v as f32 / 2_147_483_648.0
        }
    }
}

fn chunks(b: &[u8], start: usize, be: bool) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut i = start;
    while i + 8 <= b.len() {
        let id: [u8; 4] = b[i..i + 4].try_into().unwrap_or_default();
        let raw = [b[i + 4], b[i + 5], b[i + 6], b[i + 7]];
        let n = if be { u32::from_be_bytes(raw) } else { u32::from_le_bytes(raw) } as usize;
        let body = &b[i + 8..(i + 8 + n).min(b.len())];
        out.push((id, body));
        i += 8 + n + n % 2;
    }
    out
}

fn parse_wav(b: &[u8]) -> io::Result<AudioFile> {
    let mut fmt = None;
    let mut data: &[u8] = &[];
    let mut extra = Vec::new();
    for (id, body) in chunks(b, 12, false) {
        match &id {
            b"fmt " if body.len() >= 16 => {
                let tag = u16::from_le_bytes([body[0], body[1]]);
                let ch = u16::from_le_bytes([body[2], body[3]]) as usize;
                let rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
                let bits = u16::from_le_bytes([body[14], body[15]]);
                // WAVE_FORMAT_EXTENSIBLE: the sub-format's first two bytes are the tag
                let tag = if tag == 0xFFFE && body.len() >= 26 { u16::from_le_bytes([body[24], body[25]]) } else { tag };
                fmt = Some((tag, ch, rate, bits));
            }
            b"data" => data = body,
            b"ds64" => {}
            _ => extra.push((id, body.to_vec())),
        }
    }
    let (tag, ch, rate, bits) = fmt.ok_or_else(|| bad("WAV without a format chunk"))?;
    let float = tag == 3;
    if !(tag == 1 || float) || ch == 0 || bits == 0 {
        return Err(bad("unsupported WAV encoding"));
    }
    let bps = (bits as usize).div_ceil(8);
    let samples = data.chunks_exact(bps).map(|s| get_sample(s, bits.div_ceil(8) * 8, float, false)).collect();
    Ok(AudioFile { rate, channels: ch, bits, float, format: Format::Wav, data: samples, extra })
}

fn parse_aiff(b: &[u8]) -> io::Result<AudioFile> {
    let mut comm = None;
    let mut ssnd: &[u8] = &[];
    let mut extra = Vec::new();
    for (id, body) in chunks(b, 12, true) {
        match &id {
            b"COMM" if body.len() >= 18 => {
                let ch = u16::from_be_bytes([body[0], body[1]]) as usize;
                let bits = u16::from_be_bytes([body[6], body[7]]);
                let rate = from_extended(body[8..18].try_into().unwrap_or_default());
                let kind: [u8; 4] = if body.len() >= 22 { body[18..22].try_into().unwrap_or(*b"NONE") } else { *b"NONE" };
                comm = Some((ch, bits, rate, kind));
            }
            b"SSND" if body.len() >= 8 => {
                let off = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
                ssnd = &body[(8 + off).min(body.len())..];
            }
            b"FVER" => {}
            _ => extra.push((id, body.to_vec())),
        }
    }
    let (ch, bits, rate, kind) = comm.ok_or_else(|| bad("AIFF without a COMM chunk"))?;
    let (float, be, bits) = match &kind {
        b"NONE" | b"twos" => (false, true, bits),
        b"sowt" => (false, false, bits),
        b"fl32" | b"FL32" => (true, true, 32),
        b"fl64" | b"FL64" => (true, true, 64),
        _ => return Err(bad("unsupported AIFC compression")),
    };
    if ch == 0 || bits == 0 {
        return Err(bad("bad AIFF format"));
    }
    let bps = (bits as usize).div_ceil(8);
    let samples = ssnd.chunks_exact(bps).map(|s| get_sample(s, bits.div_ceil(8) * 8, float, be)).collect();
    Ok(AudioFile { rate: rate.round() as u32, channels: ch, bits, float, format: Format::Aiff, data: samples, extra })
}

/// An 80-bit IEEE extended number (AIFF's sample rate).
fn extended(v: f64) -> [u8; 10] {
    let mut out = [0u8; 10];
    if v <= 0.0 {
        return out;
    }
    let exp = v.log2().floor() as i32;
    let mant = (v / 2f64.powi(exp - 63)) as u64;
    let e = (exp + 16383) as u16;
    out[..2].copy_from_slice(&e.to_be_bytes());
    out[2..].copy_from_slice(&mant.to_be_bytes());
    out
}

fn from_extended(b: [u8; 10]) -> f64 {
    let e = u16::from_be_bytes([b[0], b[1]]) & 0x7fff;
    let mant = u64::from_be_bytes(b[2..10].try_into().unwrap_or_default());
    if e == 0 && mant == 0 {
        return 0.0;
    }
    mant as f64 * 2f64.powi(e as i32 - 16383 - 63)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, ch: usize, secs: f64) -> AudioFile {
        let n = (rate as f64 * secs) as usize;
        let mut data = Vec::new();
        for i in 0..n {
            for c in 0..ch {
                data.push(0.5 * ((i as f32 * 0.05) + c as f32).sin());
            }
        }
        AudioFile::new(rate, ch, data)
    }

    #[test]
    fn extended_rates_round_trip() {
        for r in [8000.0, 44100.0, 48000.0, 96000.0, 192000.0] {
            assert_eq!(from_extended(extended(r)), r);
        }
        // 44.1 kHz as every AIFF writer stores it
        assert_eq!(extended(44100.0), [0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn wav_and_aiff_round_trip_at_every_depth() {
        for format in [Format::Wav, Format::Aiff] {
            for (bits, float) in [(8, false), (16, false), (24, false), (32, false), (32, true)] {
                let mut a = sine(44100, 2, 0.05);
                a.bits = bits;
                a.float = float;
                a.format = format;
                a.extra.push((*b"LGBM", vec![1, 2, 3]));
                a.extra.push((*b"LGWV", vec![9; 4]));
                let bytes = if format == Format::Wav { a.wav_bytes() } else { a.aiff_bytes() };
                let b = AudioFile::parse(&bytes).unwrap();
                assert_eq!((b.rate, b.channels, b.bits, b.float, b.format), (44100, 2, bits, float, format));
                assert_eq!(b.frames(), a.frames());
                let tol = if bits == 8 { 0.01 } else { 1e-4 };
                assert!(a.data.iter().zip(&b.data).all(|(x, y)| (x - y).abs() < tol), "{format:?} {bits}");
                // the beat map kept, the overview dropped
                assert_eq!(b.extra, vec![(*b"LGBM", vec![1, 2, 3])]);
            }
        }
    }

    #[test]
    fn reads_logics_test_files() {
        let base = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("CascadeProjects/logic-research-private/sample-files");
        let wav = base.join("S05-audio-region.logicx/Media/Audio Files/tone-440hz-2s.wav");
        if let Ok(a) = AudioFile::read(&wav) {
            assert!((a.seconds() - 2.0).abs() < 0.01, "{}", a.seconds());
        }
        let aif = base.join("E23-live-loops-base.logicx/Media/Audio Files/composite-40s_Bass.aif");
        if let Ok(a) = AudioFile::read(&aif) {
            assert_eq!(a.rate, 48000);
            assert!((a.seconds() - 40.0).abs() < 0.01);
        }
    }

    #[test]
    fn writes_beside_then_renames() {
        let dir = std::env::temp_dir().join(format!("lg-audio-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.aif");
        let mut a = sine(48000, 1, 0.01);
        a.format = Format::of(&p);
        a.write(&p).unwrap();
        let b = AudioFile::read(&p).unwrap();
        assert_eq!(b.format, Format::Aiff);
        assert!(!dir.join("x.lgaudio-tmp").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
