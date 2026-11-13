//! Offline sound check for the patch library: renders a patch the way the Browser previews it
//! (its instrument plus its channel-strip effects, `patches::fx_chain`), and measures how far two
//! renders are apart (level-independent band spectrum and envelope), so the tests can prove that
//! patches with different names sound different.

use crate::patches::{self, Engine};
use lg_dsp::{Effect, Instrument};

pub const RATE: f32 = 44100.0;
/// Drum probe: one hit per piece (kick, snare, clap, closed hat, open hat, tom, crash, rim).
pub const DRUM_NOTES: [u8; 8] = [36, 38, 39, 42, 46, 45, 49, 37];
const DRUM_SEG: f32 = 0.35;
/// Melodic probe: a three-note chord held, then released.
const HOLD: f32 = 0.8;
const TAIL: f32 = 0.45;
const BANDS: usize = 20;
const ENV: usize = 8;

/// Is this spec a drum kit (played by the piece probe)?
pub fn is_kit(engine: Engine) -> bool {
    matches!(engine, Engine::DrumKit | Engine::DrumMachine)
}

/// The patch's effects as built units, in order.
pub fn build_fx(name: &str, rate: f32) -> Vec<Box<dyn Effect>> {
    patches::fx_chain(name)
        .iter()
        .filter_map(|f| {
            let mut e = lg_fx::by_logic_name(f.name, rate)?;
            for &(id, v) in &f.settings {
                e.set(id, v);
            }
            Some(e)
        })
        .collect()
}

fn run(ins: &mut dyn Instrument, fx: &mut [Box<dyn Effect>], events: &[(usize, u8, bool)], len: usize) -> Vec<f32> {
    let block = 256;
    let mut out = Vec::with_capacity(len);
    let (mut l, mut r) = (vec![0.0f32; block], vec![0.0f32; block]);
    let mut ev = 0;
    let mut t = 0;
    while t < len {
        while ev < events.len() && events[ev].0 <= t {
            let (_, p, on) = events[ev];
            if on { ins.note_on(p, 100) } else { ins.note_off(p) }
            ev += 1;
        }
        let n = block.min(len - t);
        l[..n].fill(0.0);
        r[..n].fill(0.0);
        ins.render(&mut l[..n], &mut r[..n]);
        for f in fx.iter_mut() {
            f.process(&mut l[..n], &mut r[..n]);
        }
        out.extend(l[..n].iter().zip(&r[..n]).map(|(a, b)| 0.5 * (a + b)));
        t += n;
    }
    out
}

/// Renders a patch's probe (mono) and returns it with the number of segments.
pub fn render(name: &str) -> Option<(Vec<f32>, usize)> {
    let spec = patches::patch(name)?;
    let mut ins = crate::for_patch(name, RATE)?;
    let mut fx = build_fx(name, RATE);
    if is_kit(spec.engine) {
        let seg = (DRUM_SEG * RATE) as usize;
        let mut ev = Vec::new();
        for (k, &n) in DRUM_NOTES.iter().enumerate() {
            ev.push((k * seg, n, true));
            ev.push((k * seg + seg / 4, n, false));
        }
        Some((run(ins.as_mut(), &mut fx, &ev, seg * DRUM_NOTES.len()), DRUM_NOTES.len()))
    } else {
        crate::poly_for_preview(ins.as_mut());
        let low = matches!(spec.category, Some(patches::Cat::Bass | patches::Cat::SynthBass));
        let chord: [u8; 3] = if low { [36, 43, 48] } else { [60, 64, 67] };
        let hold = (HOLD * RATE) as usize;
        let len = hold + (TAIL * RATE) as usize;
        let mut ev: Vec<(usize, u8, bool)> = chord.iter().map(|&p| (0, p, true)).collect();
        ev.extend(chord.iter().map(|&p| (hold, p, false)));
        // three segments: attack, body, release
        Some((run(ins.as_mut(), &mut fx, &ev, len), 3))
    }
}

fn band_edges() -> [f32; BANDS + 1] {
    let (lo, hi) = (40.0f32, 16000.0f32);
    std::array::from_fn(|k| lo * (hi / lo).powf(k as f32 / BANDS as f32))
}

/// Features of one segment: level-normalized band spectrum (dB) and envelope (dB below peak).
fn seg_features(x: &[f32], out: &mut Vec<f32>) {
    const N: usize = 1024;
    let edges = band_edges();
    let mut pow = [1e-12f64; BANDS];
    let mut start = 0;
    let win: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos()).collect();
    while start < x.len() {
        let mut re = vec![0.0f32; N];
        let mut im = vec![0.0f32; N];
        for i in 0..N.min(x.len() - start) {
            re[i] = x[start + i] * win[i];
        }
        crate::retro::fft(&mut re, &mut im, false);
        for b in 1..N / 2 {
            let f = b as f32 * RATE / N as f32;
            if let Some(k) = (0..BANDS).find(|&k| f >= edges[k] && f < edges[k + 1]) {
                pow[k] += (re[b] * re[b] + im[b] * im[b]) as f64;
            }
        }
        start += N / 2;
    }
    let db: Vec<f32> = pow.iter().map(|p| 10.0 * (*p as f32).log10()).collect();
    let mean = db.iter().sum::<f32>() / BANDS as f32;
    out.extend(db.iter().map(|d| (d - mean).max(-60.0)));
    // envelope: RMS per eighth, dB below the loudest
    let w = (x.len() / ENV).max(1);
    let env: Vec<f32> = (0..ENV)
        .map(|k| {
            let s = &x[(k * w).min(x.len())..((k + 1) * w).min(x.len())];
            let e = s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32;
            10.0 * (e + 1e-12).log10()
        })
        .collect();
    let peak = env.iter().cloned().fold(f32::MIN, f32::max);
    out.extend(env.iter().map(|e| (e - peak).max(-60.0)));
}

/// A render's features (per segment: BANDS spectrum values then ENV envelope values).
pub fn features(x: &[f32], segs: usize) -> Vec<f32> {
    let mut out = Vec::new();
    if segs == 3 {
        let a = (0.12 * RATE) as usize;
        let h = (HOLD * RATE) as usize;
        for (s, e) in [(0, a), (a, h), (h, x.len())] {
            seg_features(&x[s.min(x.len())..e.min(x.len())], &mut out);
        }
    } else {
        let w = x.len() / segs.max(1);
        for k in 0..segs {
            seg_features(&x[k * w..(k + 1) * w], &mut out);
        }
    }
    out
}

/// Distance in dB between two feature sets: RMS band-spectrum difference plus half the mean
/// envelope difference (0 = identical renders).
pub fn distance(a: &[f32], b: &[f32]) -> f32 {
    let per = BANDS + ENV;
    let segs = a.len().min(b.len()) / per;
    if segs == 0 {
        return f32::MAX;
    }
    let (mut spec, mut env) = (0.0f32, 0.0f32);
    for s in 0..segs {
        for k in 0..BANDS {
            let d = a[s * per + k] - b[s * per + k];
            spec += d * d;
        }
        for k in 0..ENV {
            env += (a[s * per + BANDS + k] - b[s * per + BANDS + k]).abs();
        }
    }
    (spec / (segs * BANDS) as f32).sqrt() + 0.5 * env / (segs * ENV) as f32
}

/// Peak level of a render.
pub fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

/// The closest pair of differently named patches in a family: (distance, a, b).
pub fn closest_pair(names: &[&str]) -> Option<(f32, String, String)> {
    let feats: Vec<(&str, Vec<f32>)> = names
        .iter()
        .filter_map(|n| render(n).map(|(x, s)| (*n, features(&x, s))))
        .collect();
    let mut best: Option<(f32, String, String)> = None;
    for i in 0..feats.len() {
        for j in i + 1..feats.len() {
            let d = distance(&feats[i].1, &feats[j].1);
            if best.as_ref().is_none_or(|b| d < b.0) {
                best = Some((d, feats[i].0.to_string(), feats[j].0.to_string()));
            }
        }
    }
    best
}
