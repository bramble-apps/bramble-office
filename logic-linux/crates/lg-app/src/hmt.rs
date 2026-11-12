//! Hermode Tuning (Project Settings ▸ Tuning ▸ Hermode Tuning (HMT)): software instruments are
//! retuned as the music plays so the chord sounding has purer fifths and thirds (Pop/Jazz:
//! sevenths too). The engine hands each block's sounding pitch classes to `block`; the chord's
//! root stays put and its fifth, thirds and sevenths move toward just intonation (C major: E
//! about 14 cents down, G 2 cents up), scaled by Depth. Baroque tunes chords near the key centre
//! (Root Key) purely and distant ones less. Notes take the tuning when they start.

use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};

/// 0 off, 1 Classic, 2 Pop/Jazz, 3 Baroque.
static MODE: AtomicU8 = AtomicU8::new(0);
static DEPTH: AtomicU32 = AtomicU32::new(0);
static ROOT: AtomicU8 = AtomicU8::new(0);
static LAST: AtomicU32 = AtomicU32::new(u32::MAX);

pub const TYPES: [&str; 4] = ["Classic (3/5-all)", "Pop/Jazz (3/5/7-all)", "Baroque (3/5-adaptive)", "Off"];

/// From the project settings (projsettings::apply).
pub fn configure(v: &crate::projsettings::Values) {
    let on = v.get("Tuning /  / Software Instrument Tuning Table") == "Hermode Tuning (HMT)";
    let mode = if on { TYPES.iter().position(|t| *t == v.get("Tuning /  / HMT Type:")).map_or(1, |i| if i == 3 { 0 } else { i as u8 + 1 }) } else { 0 };
    MODE.store(mode, Ordering::Relaxed);
    DEPTH.store((v.num("Tuning /  / Depth:").clamp(0.0, 100.0) / 100.0).to_bits(), Ordering::Relaxed);
    let keys = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];
    ROOT.store(keys.iter().position(|k| *k == v.get("Tuning /  / Root Key:")).unwrap_or(0) as u8, Ordering::Relaxed);
    LAST.store(u32::MAX, Ordering::Relaxed);
    if mode == 0 {
        lg_dsp::set_hmt(&[0.0; 12]);
    }
}

pub fn active() -> bool {
    MODE.load(Ordering::Relaxed) != 0
}

/// Notes played live (counts per pitch class): they join the chord.
static LIVE: [AtomicU8; 12] = [const { AtomicU8::new(0) }; 12];

/// A live note on / off (before the instrument gets it).
pub fn live(pitch: u8, on: bool) {
    if !active() {
        return;
    }
    let c = &LIVE[pitch as usize % 12];
    let n = c.load(Ordering::Relaxed);
    c.store(if on { n.saturating_add(1) } else { n.saturating_sub(1) }, Ordering::Relaxed);
    if on {
        block(0);
    }
}

fn live_mask() -> u16 {
    (0..12).filter(|&k| LIVE[k].load(Ordering::Relaxed) > 0).fold(0, |m, k| m | 1 << k)
}

/// The engine's block: the pitch classes sounding (bit k = pitch class k).
pub fn block(mask: u16) {
    let mask = mask | live_mask();
    let mode = MODE.load(Ordering::Relaxed);
    if mode == 0 || mask == 0 || LAST.swap(mask as u32, Ordering::Relaxed) == mask as u32 {
        return;
    }
    let depth = f32::from_bits(DEPTH.load(Ordering::Relaxed));
    lg_dsp::set_hmt(&offsets(mask, mode, depth, ROOT.load(Ordering::Relaxed)));
}

/// Just intonation's deviation from equal temperament (cents) of the intervals HMT purifies.
const FIFTH: f32 = 1.96;
const MAJ3: f32 = -13.69;
const MIN3: f32 = 15.64;
const MIN7: f32 = -31.17;
const MAJ7: f32 = -11.73;

/// The offsets per pitch class for the chord in `mask` (mode 1 Classic, 2 Pop/Jazz, 3 Baroque).
pub fn offsets(mask: u16, mode: u8, depth: f32, key_root: u8) -> [f32; 12] {
    let has = |pc: usize| mask & (1 << (pc % 12)) != 0;
    // the root: the pitch class with the most chord tones above it (fifth counts double)
    let mut best: Option<(usize, i32)> = None;
    for r in (0..12).filter(|&r| has(r)) {
        let s = 2 * has(r + 7) as i32 + has(r + 4) as i32 + has(r + 3) as i32 + if mode == 2 { has(r + 10) as i32 } else { 0 };
        if s > 0 && best.is_none_or(|b| s > b.1) {
            best = Some((r, s));
        }
    }
    let mut out = [0.0f32; 12];
    let Some((r, _)) = best else { return out };
    let mut d = depth;
    if mode == 3 {
        // Baroque: steps round the circle of fifths from the key centre (0..6)
        let steps = (0..12).find(|k| (key_root as usize + k * 7) % 12 == r || (key_root as usize + 12 * 7 - k * 7) % 12 == r).unwrap_or(6).min(6);
        d *= 1.0 - steps as f32 / 8.0;
    }
    let mut put = |iv: usize, c: f32| {
        if has(r + iv) {
            out[(r + iv) % 12] = c * d;
        }
    };
    put(7, FIFTH);
    if has(r + 4) {
        put(4, MAJ3);
    } else {
        put(3, MIN3);
    }
    if mode == 2 {
        put(10, MIN7);
        put(11, MAJ7);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(pcs: &[usize]) -> u16 {
        pcs.iter().fold(0, |m, p| m | 1 << p)
    }

    #[test]
    fn c_major_purer_thirds_and_fifths() {
        let o = offsets(mask(&[0, 4, 7]), 1, 1.0, 0);
        assert_eq!(o[0], 0.0);
        assert!((o[4] + 13.69).abs() < 0.01 && (o[7] - 1.96).abs() < 0.01, "{o:?}");
        // half depth, half the correction
        let h = offsets(mask(&[0, 4, 7]), 1, 0.5, 0);
        assert!((h[4] + 6.845).abs() < 0.01);
        // A minor: C (its minor third) goes up
        let a = offsets(mask(&[9, 0, 4]), 1, 1.0, 0);
        assert!(a[0] > 15.0 && a[4] > 1.0 && a[9] == 0.0, "{a:?}");
    }

    #[test]
    fn sevenths_only_in_pop_jazz_baroque_less_far_from_the_key() {
        let g7 = mask(&[7, 11, 2, 5]);
        assert_eq!(offsets(g7, 1, 1.0, 0)[5], 0.0);
        assert!(offsets(g7, 2, 1.0, 0)[5] < -30.0);
        let near = offsets(mask(&[0, 4, 7]), 3, 1.0, 0)[4];
        let far = offsets(mask(&[6, 10, 1]), 3, 1.0, 0)[10];
        assert!(near.abs() > far.abs() && far != 0.0, "{near} {far}");
        assert_eq!(offsets(mask(&[0, 1]), 1, 1.0, 0), [0.0; 12]);
    }
}
