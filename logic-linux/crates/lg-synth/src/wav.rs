//! Minimal WAV/AIFF reading and WAV writing (no dependencies).

use std::io::{self, Write};

/// Decoded audio: interleaved-free channels as f32.
#[derive(Clone, Debug, Default)]
pub struct Audio {
    pub rate: u32,
    pub channels: Vec<Vec<f32>>,
    /// Loop points from a 'smpl' (WAV) chunk, in frames.
    pub loop_points: Option<(u32, u32)>,
    /// Root note from a 'smpl' chunk.
    pub root: Option<u8>,
}

impl Audio {
    pub fn frames(&self) -> usize {
        self.channels.first().map(|c| c.len()).unwrap_or(0)
    }
}

/// Writes 24-bit PCM stereo WAV.
pub fn write_wav_24(path: &std::path::Path, rate: u32, l: &[f32], r: &[f32]) -> io::Result<()> {
    let n = l.len().min(r.len());
    let data_len = (n * 6) as u32;
    let mut out = Vec::with_capacity(44 + n * 6);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 6).to_le_bytes());
    out.extend_from_slice(&6u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..n {
        for s in [l[i], r[i]] {
            let v = (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
            out.extend_from_slice(&v.to_le_bytes()[..3]);
        }
    }
    std::fs::File::create(path)?.write_all(&out)
}

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u16be(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}
fn u32be(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn decode_pcm(data: &[u8], bits: u16, float: bool, big: bool, ch: usize) -> Vec<Vec<f32>> {
    let bps = (bits as usize).div_ceil(8);
    let frames = if bps * ch == 0 {
        0
    } else {
        data.len() / (bps * ch)
    };
    let mut out = vec![Vec::with_capacity(frames); ch];
    for f in 0..frames {
        for (c, o) in out.iter_mut().enumerate() {
            let p = (f * ch + c) * bps;
            let s = &data[p..p + bps];
            let v = match (bps, float) {
                (1, _) => (s[0] as f32 - 128.0) / 128.0,
                (2, _) => {
                    let x = if big {
                        i16::from_be_bytes([s[0], s[1]])
                    } else {
                        i16::from_le_bytes([s[0], s[1]])
                    };
                    x as f32 / 32768.0
                }
                (3, _) => {
                    let (a, b, c2) = if big {
                        (s[2], s[1], s[0])
                    } else {
                        (s[0], s[1], s[2])
                    };
                    let x = ((a as i32) | ((b as i32) << 8) | ((c2 as i32) << 16)) << 8 >> 8;
                    x as f32 / 8_388_608.0
                }
                (4, true) => {
                    let a = [s[0], s[1], s[2], s[3]];
                    if big {
                        f32::from_be_bytes(a)
                    } else {
                        f32::from_le_bytes(a)
                    }
                }
                (4, false) => {
                    let a = [s[0], s[1], s[2], s[3]];
                    (if big {
                        i32::from_be_bytes(a)
                    } else {
                        i32::from_le_bytes(a)
                    }) as f32
                        / 2_147_483_648.0
                }
                (8, true) => {
                    let mut a = [0u8; 8];
                    a.copy_from_slice(s);
                    (if big {
                        f64::from_be_bytes(a)
                    } else {
                        f64::from_le_bytes(a)
                    }) as f32
                }
                _ => 0.0,
            };
            o.push(v);
        }
    }
    out
}

/// Reads a WAV (PCM 8/16/24/32, float 32/64, WAVE_FORMAT_EXTENSIBLE) or AIFF/AIFC (PCM, 'sowt', 'fl32') file.
pub fn read_audio(path: &std::path::Path) -> io::Result<Audio> {
    let b = std::fs::read(path)?;
    decode_audio(&b).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported audio file {}", path.display()),
        )
    })
}

pub fn decode_audio(b: &[u8]) -> Option<Audio> {
    if b.len() < 12 {
        return None;
    }
    if &b[0..4] == b"RIFF" && &b[8..12] == b"WAVE" {
        let mut o = 12;
        let (mut fmt, mut ch, mut rate, mut bits) = (0u16, 0u16, 0u32, 0u16);
        let mut data: Option<&[u8]> = None;
        let mut a = Audio::default();
        while o + 8 <= b.len() {
            let id = &b[o..o + 4];
            let sz = u32le(b, o + 4) as usize;
            let body = &b[o + 8..(o + 8 + sz).min(b.len())];
            match id {
                b"fmt " if body.len() >= 16 => {
                    fmt = u16le(body, 0);
                    ch = u16le(body, 2);
                    rate = u32le(body, 4);
                    bits = u16le(body, 14);
                    if fmt == 0xFFFE && body.len() >= 26 {
                        fmt = u16le(body, 24);
                    }
                }
                b"data" => data = Some(body),
                b"smpl" if body.len() >= 36 => {
                    a.root = Some(u32le(body, 12).min(127) as u8);
                    if u32le(body, 28) > 0 && body.len() >= 60 {
                        a.loop_points = Some((u32le(body, 44), u32le(body, 48) + 1));
                    }
                }
                _ => {}
            }
            o += 8 + sz + (sz & 1);
        }
        let data = data?;
        if ch == 0 || !(fmt == 1 || fmt == 3) {
            return None;
        }
        a.rate = rate;
        a.channels = decode_pcm(data, bits, fmt == 3, false, ch as usize);
        return Some(a);
    }
    if &b[0..4] == b"FORM" && (&b[8..12] == b"AIFF" || &b[8..12] == b"AIFC") {
        let mut o = 12;
        let (mut ch, mut bits, mut rate) = (0u16, 0u16, 0f64);
        let mut little = false;
        let mut float = false;
        let mut data: Option<&[u8]> = None;
        while o + 8 <= b.len() {
            let id = &b[o..o + 4];
            let sz = u32be(b, o + 4) as usize;
            let body = &b[o + 8..(o + 8 + sz).min(b.len())];
            match id {
                b"COMM" if body.len() >= 18 => {
                    ch = u16be(body, 0);
                    bits = u16be(body, 6);
                    rate = ext80(&body[8..18]);
                    if body.len() >= 22 {
                        match &body[18..22] {
                            b"sowt" => little = true,
                            b"fl32" | b"FL32" => float = true,
                            _ => {}
                        }
                    }
                }
                b"SSND" if body.len() >= 8 => {
                    let off = u32be(body, 0) as usize;
                    data = Some(&body[(8 + off).min(body.len())..]);
                }
                _ => {}
            }
            o += 8 + sz + (sz & 1);
        }
        let data = data?;
        return Some(Audio {
            rate: rate as u32,
            channels: decode_pcm(
                data,
                if float { 32 } else { bits },
                float,
                !little,
                ch.max(1) as usize,
            ),
            loop_points: None,
            root: None,
        });
    }
    None
}

fn ext80(b: &[u8]) -> f64 {
    let exp = (((b[0] as i32) & 0x7f) << 8) | b[1] as i32;
    let mut mant: u64 = 0;
    for &x in &b[2..10] {
        mant = (mant << 8) | x as u64;
    }
    if exp == 0 && mant == 0 {
        return 0.0;
    }
    let v = mant as f64 * 2f64.powi(exp - 16383 - 63);
    if b[0] & 0x80 != 0 { -v } else { v }
}
