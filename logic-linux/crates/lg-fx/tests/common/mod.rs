//! Shared test helpers: the reference test signal, its analysis (the same measurements as the
//! research's reference renders were summarised with), and access to the private research
//! folder (tests that need it skip quietly when it isn't on this machine).

#![allow(dead_code)]

use lg_dsp::Effect;
use std::path::PathBuf;

pub const RATE: f32 = 48000.0;

/// The private research folder's reference renders, when present.
pub fn reference_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let d = PathBuf::from(home).join("CascadeProjects/logic-research-private/plugins/reference");
    d.join("ANALYSIS.txt").exists().then_some(d)
}

pub fn plugins_dir() -> Option<PathBuf> {
    reference_dir().map(|d| d.parent().unwrap().to_path_buf())
}

/// The composite test signal (40 s, 48 kHz): impulse at 0, 1 kHz sine 0.5 peak at 8-10 s,
/// log sweep 20 Hz-20 kHz 0.5 peak at 16-22 s, uniform white noise ~-12 dBFS RMS at 30-32 s.
pub fn composite() -> Vec<f32> {
    let sr = RATE as usize;
    let mut x = vec![0.0f32; 40 * sr];
    x[0] = 0.5;
    for i in 0..2 * sr {
        x[8 * sr + i] = 0.5 * (std::f64::consts::TAU * 1000.0 * i as f64 / sr as f64).sin() as f32;
    }
    let (f1, f2, t) = (20.0f64, 20000.0f64, 6.0f64);
    let k = t / (f2 / f1).ln();
    for i in 0..6 * sr {
        let tt = i as f64 / sr as f64;
        x[16 * sr + i] = 0.5 * (std::f64::consts::TAU * f1 * k * ((tt / k).exp() - 1.0)).sin() as f32;
    }
    let mut s: u32 = 1;
    let a = 0.25 * 3f32.sqrt(); // uniform in ±a has RMS a/sqrt(3) = 0.25 (-12 dBFS)
    for i in 0..2 * sr {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        x[30 * sr + i] = (s as f32 / u32::MAX as f32 * 2.0 - 1.0) * a;
    }
    x
}

/// Runs a mono signal through an effect (dual mono in, stereo out) in 512-sample blocks.
pub fn run(fx: &mut dyn Effect, x: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let mut l = x.to_vec();
    let mut r = x.to_vec();
    for (a, b) in l.chunks_mut(512).zip(r.chunks_mut(512)) {
        fx.process(a, b);
    }
    (l, r)
}

fn rms(x: &[f32], a: usize, b: usize) -> f64 {
    let s = &x[a.min(x.len())..b.min(x.len())];
    (s.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / s.len().max(1) as f64).sqrt()
}

fn dbv(r: f64) -> f64 {
    if r > 1e-12 { 20.0 * r.log10() } else { -240.0 }
}

/// The reference summary of a render: sine gain, noise gain, impulse peak, sweep (Hz, dB).
#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub sine: f64,
    pub noise: f64,
    pub peak: f64,
    pub decay_ms: Option<u32>,
    pub sweep: Vec<(f64, f64)>,
}

pub fn analyse(x: &[f32], y: &[f32]) -> Summary {
    let sr = RATE as f64;
    let w = |a: f64, b: f64| ((a * sr) as usize, (b * sr) as usize);
    let (a, b) = w(8.5, 9.5);
    let sine = dbv(rms(y, a, b)) - dbv(rms(x, a, b));
    let (a, b) = w(30.2, 31.8);
    let noise = dbv(rms(y, a, b)) - dbv(rms(x, a, b));
    let n8 = 8 * sr as usize;
    let pk = (0..n8).max_by(|&i, &j| y[i].abs().partial_cmp(&y[j].abs()).unwrap()).unwrap();
    let hop = sr as usize / 100;
    let e: Vec<f64> = (0..n8 - hop).step_by(hop).map(|i| rms(y, i, i + hop)).collect();
    let refm = e.iter().cloned().fold(0.0, f64::max).max(1e-30);
    let pkms = 1000.0 * pk as f64 / sr;
    let decay_ms = e.iter().enumerate().find(|(k, v)| (*k * 10) as f64 > pkms && dbv(**v) - dbv(refm) < -60.0).map(|(k, _)| (k * 10) as u32);
    let (f1, f2, t) = (20.0f64, 20000.0f64, 6.0f64);
    let k = t / (f2 / f1).ln();
    let mut sweep = Vec::new();
    for i in 0..30 {
        let f = 20.0 * 2f64.powf(i as f64 / 3.0);
        if f > 19000.0 {
            break;
        }
        let tt = k * (f / f1).ln();
        let c = ((16.0 + tt) * sr) as usize;
        let h = 48usize.max((0.01 * sr) as usize);
        sweep.push((f.round(), dbv(rms(y, c - h, c + h)) - dbv(rms(x, c - h, c + h))));
    }
    Summary { sine, noise, peak: y[pk] as f64, decay_ms, sweep }
}

/// The reference summary for "<Plugin>/<setting>" from the research's ANALYSIS.txt.
pub fn reference(name: &str) -> Option<Summary> {
    let d = reference_dir()?;
    let text = std::fs::read_to_string(d.join("ANALYSIS.txt")).ok()?;
    let head = format!("== {name}");
    let mut lines = text.lines().skip_while(|l| l.trim() != head).skip(1);
    let mut s = Summary::default();
    let num = |l: &str| -> f64 { l.split_whitespace().find_map(|w| w.trim_end_matches("dB").parse::<f64>().ok()).unwrap_or(f64::NAN) };
    for _ in 0..4 {
        let l = lines.next()?;
        let l = l.trim();
        if l.starts_with("1 kHz") {
            s.sine = num(&l[10..]);
        } else if l.starts_with("noise") {
            s.noise = num(&l[14..]);
        } else if l.starts_with("impulse") {
            s.peak = l.split_whitespace().nth(2).and_then(|w| w.parse().ok()).unwrap_or(f64::NAN);
            s.decay_ms = l.split("after ").nth(1).and_then(|w| w.split_whitespace().next()).and_then(|w| w.parse().ok());
        } else if l.starts_with("sweep") {
            for p in l.split("):").nth(1)?.split_whitespace() {
                let mut it = p.split(':');
                let f: f64 = it.next()?.parse().ok()?;
                let v: f64 = it.next()?.parse().ok()?;
                s.sweep.push((f, v));
            }
        }
    }
    Some(s)
}

/// Floats of a factory preset from the research's decoded presets (preset-values/<plugin>.json),
/// as stored in the .pst (no leading on-switch for Channel EQ; a leading header float for most others).
pub fn preset_floats(plugin: &str, preset: &str) -> Option<Vec<f32>> {
    let p = plugins_dir()?.join("preset-values").join(format!("{plugin}.json"));
    let text = std::fs::read_to_string(p).ok()?;
    let at = text.find(&format!("\"{preset}\""))?;
    let rest = &text[at..];
    let v = rest.find("\"values\"")?;
    let rest = &rest[v..];
    let a = rest.find('[')?;
    let b = rest.find(']')?;
    Some(rest[a + 1..b].split(',').filter_map(|w| w.trim().parse::<f32>().ok()).collect())
}

/// A JSON number array by key in a settings.json of a reference render.
pub fn settings_array(rel: &str, key: &str) -> Option<Vec<f32>> {
    let text = std::fs::read_to_string(reference_dir()?.join(rel).join("settings.json")).ok()?;
    let at = text.find(&format!("\"{key}\""))?;
    let rest = &text[at..];
    let a = rest.find('[')?;
    let b = rest.find(']')?;
    Some(rest[a + 1..b].split(',').filter_map(|w| w.trim().parse::<f32>().ok()).collect())
}

pub fn print_compare(name: &str, ours: &Summary, theirs: &Summary) {
    eprintln!("== {name}");
    eprintln!("  sine  ours {:+6.2}  logic {:+6.2}", ours.sine, theirs.sine);
    eprintln!("  noise ours {:+6.2}  logic {:+6.2}", ours.noise, theirs.noise);
    eprintln!("  peak  ours {:.4}  logic {:.4}   decay ours {:?} logic {:?}", ours.peak, theirs.peak, ours.decay_ms, theirs.decay_ms);
    let row: Vec<String> = ours.sweep.iter().zip(theirs.sweep.iter()).map(|(a, b)| format!("{}:{:+.1}/{:+.1}", a.0, a.1, b.1)).collect();
    eprintln!("  sweep ours/logic {}", row.join(" "));
}

/// Largest sweep difference between `lo` and `hi` Hz.
pub fn sweep_error(ours: &Summary, theirs: &Summary, lo: f64, hi: f64) -> f64 {
    ours.sweep.iter().zip(theirs.sweep.iter()).filter(|(a, _)| a.0 >= lo && a.0 <= hi).map(|(a, b)| (a.1 - b.1).abs()).fold(0.0, f64::max)
}

/// Stability: feeds noise bursts, impulses and silence; every sample finite and bounded.
pub fn assert_stable(fx: &mut dyn Effect, bound: f32, what: &str) {
    let sr = RATE as usize;
    let mut x = vec![0.0f32; sr * 2];
    let mut s: u32 = 99;
    for v in x[..sr / 2].iter_mut() {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        *v = s as f32 / u32::MAX as f32 * 2.0 - 1.0;
    }
    x[sr] = 1.0;
    x[sr + 4000] = -1.0;
    let (l, r) = run(fx, &x);
    for (i, v) in l.iter().chain(r.iter()).enumerate() {
        assert!(v.is_finite() && v.abs() <= bound, "{what}: sample {i} = {v}");
    }
}

/// The left channel of a PCM WAV file (16 or 24 bit), as floats.
pub fn read_wav(path: &std::path::Path) -> Option<Vec<f32>> {
    let b = std::fs::read(path).ok()?;
    if b.get(0..4)? != b"RIFF" || b.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut at = 12;
    let (mut ch, mut bits) = (1usize, 16usize);
    while at + 8 <= b.len() {
        let id = &b[at..at + 4];
        let len = u32::from_le_bytes(b[at + 4..at + 8].try_into().ok()?) as usize;
        let body = b.get(at + 8..(at + 8 + len).min(b.len()))?;
        if id == b"fmt " {
            ch = u16::from_le_bytes(body[2..4].try_into().ok()?) as usize;
            bits = u16::from_le_bytes(body[14..16].try_into().ok()?) as usize;
        } else if id == b"data" {
            let bps = bits / 8;
            let frame = bps * ch;
            return Some(
                body.chunks_exact(frame)
                    .map(|f| match bps {
                        2 => i16::from_le_bytes([f[0], f[1]]) as f32 / 32768.0,
                        3 => (i32::from_le_bytes([0, f[0], f[1], f[2]]) >> 8) as f32 / 8388608.0,
                        _ => 0.0,
                    })
                    .collect(),
            );
        }
        at += 8 + len + (len & 1);
    }
    None
}

/// RMS envelope (dB) in `ms` windows from `from` to `to` seconds.
pub fn envelope(x: &[f32], from: f32, to: f32, ms: f32) -> Vec<f64> {
    let h = (RATE * ms / 1000.0) as usize;
    let a = (from * RATE) as usize;
    let b = (to * RATE) as usize;
    (a..b).step_by(h).map(|i| dbv(rms(x, i, i + h))).collect()
}

/// A text or number field of a reference render's settings.json (the first match of `"key":`).
pub fn settings_field(rel: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(reference_dir()?.join(rel).join("settings.json")).ok()?;
    let at = text.find(&format!("\"{key}\""))?;
    let rest = text[at + key.len() + 2..].trim_start().strip_prefix(':')?.trim_start();
    if let Some(r) = rest.strip_prefix('"') {
        Some(r[..r.find('"')?].to_string())
    } else {
        let end = rest.find([',', '\n', '}']).unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

pub fn settings_num(rel: &str, key: &str) -> Option<f32> {
    settings_field(rel, key)?.parse().ok()
}
