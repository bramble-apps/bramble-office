//! Delay Designer: Logic's multi-tap delay with up to 26 taps (A–Z), all fed from the input,
//! as the Effects guide describes it.
//!
//! - Each tap: delay time (up to 10 s), filter on / off with HP and LP cutoffs, a shared 6 or
//!   12 dB slope and resonance, pitch on / off with Transpose (semitones) and Fine (cents), Flip
//!   (swaps left and right), Pan (balance for stereo input), Spread, Mute and Level.
//! - Sync snaps every tap to the Grid (a note value) with Swing moving every second grid step
//!   (50 % straight); the taps keep their grid positions at a new tempo. Taps past 10 s are kept
//!   but don't play.
//! - Master: Feedback on / off, the feedback tap and its level, and the Dry / Wet mix.
//!
//! Tap A's delay, cutoffs and level, the grid, swing, sync, feedback and mix sit at the floats
//! the research decoded (21, 34, 35, 29, 0, 1, 10, 3, 4, 26, 28). Logic stores further taps as
//! variable-length records that aren't decoded, so taps B–Z and Tap A's other fields are our own
//! ids from 1000 (tap k at 1000 + 32 k, hidden: the window's Tap parameter bar edits them through
//! the "Selected Tap" controls at 1950).

use crate::kit::{Shifter, Svf, leak};
use crate::util::{Delay, Values, db, flush};
use lg_dsp::{Effect, Param, Plugin, Unit};

pub const TAPS: usize = 26;
pub const GRID: u32 = 0;
pub const SWING: u32 = 1;
pub const FEEDBACK: u32 = 3;
pub const FEEDBACK_LEVEL: u32 = 4;
pub const SYNC: u32 = 10;
pub const MIX_DRY: u32 = 26;
pub const MIX_WET: u32 = 28;
pub const FEEDBACK_TAP: u32 = 1900;
pub const SELECTED: u32 = 1901;
/// The selected tap's fields (FIELDS order) at 1950 + field.
pub const SEL: u32 = 1950;

pub const FIELDS: &[(&str, f32, f32, f32, Unit)] = &[
    ("On", 0.0, 1.0, 0.0, Unit::OnOff),
    ("Delay", 1.0, 10000.0, 1000.0, Unit::Ms),
    ("Filter", 0.0, 1.0, 0.0, Unit::OnOff),
    ("HP Cutoff", 10.0, 20000.0, 19.711, Unit::Hz),
    ("LP Cutoff", 10.0, 20480.0, 20290.0, Unit::Hz),
    ("Slope 12 dB", 0.0, 1.0, 1.0, Unit::OnOff),
    ("Resonance", 0.0, 100.0, 0.0, Unit::Percent),
    ("Pitch", 0.0, 1.0, 0.0, Unit::OnOff),
    ("Transpose", -24.0, 24.0, 0.0, Unit::Semitones),
    ("Fine", -50.0, 50.0, 0.0, Unit::Cents),
    ("Flip", 0.0, 1.0, 0.0, Unit::OnOff),
    ("Pan", -100.0, 100.0, 0.0, Unit::Percent),
    ("Spread", 0.0, 100.0, 100.0, Unit::Percent),
    ("Mute", 0.0, 1.0, 0.0, Unit::OnOff),
    ("Level", 0.0, 100.0, 70.0, Unit::Percent),
];
pub const T_ON: usize = 0;
pub const T_DELAY: usize = 1;
pub const T_FILTER: usize = 2;
pub const T_HP: usize = 3;
pub const T_LP: usize = 4;
pub const T_SLOPE: usize = 5;
pub const T_RESO: usize = 6;
pub const T_PITCH: usize = 7;
pub const T_TRANSP: usize = 8;
pub const T_FINE: usize = 9;
pub const T_FLIP: usize = 10;
pub const T_PAN: usize = 11;
pub const T_SPREAD: usize = 12;
pub const T_MUTE: usize = 13;
pub const T_LEVEL: usize = 14;

/// A tap field's id: Tap A's decoded fields at Logic's floats, the rest ours.
pub const fn tap_id(k: usize, f: usize) -> u32 {
    if k == 0 {
        match f {
            T_DELAY => return 21,
            T_HP => return 34,
            T_LP => return 35,
            T_LEVEL => return 29,
            _ => {}
        }
    }
    1000 + 32 * k as u32 + f as u32
}

pub fn tap_letter(k: usize) -> char {
    (b'A' + k.min(25) as u8) as char
}

const GRIDS: &[&str] = &["1/1", "1/2", "1/4", "1/8", "1/16", "1/32", "1/64", "1/4 T", "1/8 T", "1/16 T"];
/// Grid choices as a share of a whole note (the decoded float stores the share).
const GRID_SHARE: [f32; 10] = [1.0, 0.5, 0.25, 0.125, 0.0625, 0.03125, 0.015625, 1.0 / 6.0, 1.0 / 12.0, 1.0 / 24.0];

static PARAMS: std::sync::LazyLock<Vec<Param>> = std::sync::LazyLock::new(|| {
    let mut v = vec![
        Param::new(GRID, "Grid", 0.015625, 1.0, 0.0625, Unit::None),
        Param::new(SWING, "Swing", 0.0, 100.0, 50.0, Unit::Percent),
        Param::new(FEEDBACK, "Feedback", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(FEEDBACK_LEVEL, "Feedback Level", -60.0, 0.0, -6.0, Unit::Db),
        Param::new(SYNC, "Sync", 0.0, 1.0, 1.0, Unit::OnOff),
        Param::new(MIX_DRY, "Mix Original", 0.0, 100.0, 100.0, Unit::Percent),
        Param::new(MIX_WET, "Mix Effect", 0.0, 100.0, 90.0, Unit::Percent),
        Param::menu(FEEDBACK_TAP, "Feedback Tap", &["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z"], 0.0),
        Param::menu(SELECTED, "Selected Tap", &["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T", "U", "V", "W", "X", "Y", "Z"], 0.0),
    ];
    for (f, (n, lo, hi, d, u)) in FIELDS.iter().enumerate() {
        if f == T_ON {
            continue;
        }
        let mut p = Param::new(SEL + f as u32, leak(format!("Tap {n}")), *lo, *hi, *d, *u);
        p.choices = &[];
        v.push(p);
    }
    for k in 0..TAPS {
        for (f, (n, lo, hi, d, u)) in FIELDS.iter().enumerate() {
            let d = if f == T_ON && k == 0 { 1.0 } else { *d };
            let p = Param::new(tap_id(k, f), leak(format!("Tap {} {n}", tap_letter(k))), *lo, *hi, d, *u);
            // Tap A's decoded fields stay visible (as before); everything else is the window's
            v.push(if k == 0 && matches!(f, T_DELAY | T_HP | T_LP | T_LEVEL) { p } else { p.hide() });
        }
    }
    v
});

struct TapState {
    hp: [Svf; 2],
    lp: [Svf; 2],
    hp1: [f32; 2],
    lp1: [f32; 2],
    shift: [Shifter; 2],
}

pub struct DelayDesigner {
    p: Values,
    rate: f32,
    bpm: f64,
    line: [Delay; 2],
    taps: Vec<TapState>,
    fb: [f32; 2],
    /// Per tap (computed on change): delay samples, playing, gains (LL, RL, LR, RR), pitch ratio.
    plan: Vec<(f32, bool, [f32; 4], f32)>,
}

impl DelayDesigner {
    pub fn new(rate: f32) -> DelayDesigner {
        let mut d = DelayDesigner {
            p: Values::new(PARAMS.as_slice()),
            rate,
            bpm: 120.0,
            line: [Delay::new((10.1 * rate) as usize), Delay::new((10.1 * rate) as usize)],
            taps: (0..TAPS).map(|_| TapState { hp: [Svf::default(); 2], lp: [Svf::default(); 2], hp1: [0.0; 2], lp1: [0.0; 2], shift: [Shifter::new(rate, 30.0), Shifter::new(rate, 30.0)] }).collect(),
            fb: [0.0; 2],
            plan: vec![(1.0, false, [0.0; 4], 1.0); TAPS],
        };
        d.update();
        d
    }

    pub fn tap(&self, k: usize, f: usize) -> f32 {
        self.p.get(tap_id(k, f))
    }

    /// The taps that exist, by delay time: (tap index, delay ms as played).
    pub fn taps_in_time(&self) -> Vec<(usize, f32)> {
        let mut v: Vec<(usize, f32)> = (0..TAPS).filter(|k| self.tap(*k, T_ON) >= 0.5).map(|k| (k, self.played_ms(k))).collect();
        v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        v
    }

    /// The grid step in ms at the tempo.
    pub fn grid_ms(&self) -> f32 {
        self.p.get(GRID) * 4.0 * 60000.0 / self.bpm as f32
    }

    /// A delay snapped to the grid with swing (every second step moved), as Sync plays it.
    pub fn snap_ms(&self, ms: f32) -> f32 {
        if !self.p.on(SYNC) {
            return ms;
        }
        let g = self.grid_ms().max(0.01);
        let swing = (self.p.get(SWING) / 100.0).clamp(0.0, 1.0);
        // pairs of grid steps: the second step sits at swing × 2 steps into the pair
        let pair = 2.0 * g;
        let n = (ms / pair).floor();
        let within = ms - n * pair;
        let cands = [0.0, swing * pair, pair];
        let best = cands.iter().copied().min_by(|a, b| (a - within).abs().partial_cmp(&(b - within).abs()).unwrap()).unwrap();
        (n * pair + best).max(g.min(ms.max(1.0)))
    }

    fn played_ms(&self, k: usize) -> f32 {
        self.snap_ms(self.tap(k, T_DELAY))
    }

    /// Adds a tap at a delay time (the first free letter); its index.
    pub fn add_tap(&mut self, ms: f32) -> Option<usize> {
        let k = (0..TAPS).find(|k| self.tap(*k, T_ON) < 0.5)?;
        for (f, (_, _, _, d, _)) in FIELDS.iter().enumerate() {
            self.p.set(tap_id(k, f), *d);
        }
        self.p.set(tap_id(k, T_ON), 1.0);
        self.p.set(tap_id(k, T_DELAY), ms);
        self.update();
        Some(k)
    }

    pub fn delete_tap(&mut self, k: usize) {
        if self.taps_in_time().len() > 1 {
            self.p.set(tap_id(k, T_ON), 0.0);
            self.update();
        }
    }

    /// Replaces every tap with taps at these times (the Tap pads: Start, Tap…, Last Tap); the
    /// last becomes the feedback tap.
    pub fn set_taps(&mut self, times_ms: &[f32]) {
        for k in 0..TAPS {
            self.p.set(tap_id(k, T_ON), 0.0);
        }
        let mut last = 0;
        for t in times_ms.iter().take(TAPS) {
            if let Some(k) = self.add_tap(*t) {
                last = k;
            }
        }
        self.p.set(FEEDBACK_TAP, last as f32);
        self.update();
    }

    fn update(&mut self) {
        let rate = self.rate;
        let stereo_pan = |pan: f32, spread: f32, flip: bool| {
            // balance and spread: spread 100 % keeps L and R apart, 0 % folds them to the pan
            let pan = if flip { -pan } else { pan } / 100.0;
            let s = spread / 100.0;
            let (ll, rr) = (0.5 + 0.5 * s, 0.5 + 0.5 * s);
            let (rl, lr) = (0.5 - 0.5 * s, 0.5 - 0.5 * s);
            // pan: +100 % is full left (as the guide's field counts)
            let gl = (1.0 + pan).min(1.0);
            let gr = (1.0 - pan).min(1.0);
            let m = if flip { [lr * gl, rr * gl, ll * gr, rl * gr] } else { [ll * gl, rl * gl, lr * gr, rr * gr] };
            m
        };
        for k in 0..TAPS {
            let on = self.tap(k, T_ON) >= 0.5 && self.tap(k, T_MUTE) < 0.5;
            let ms = self.played_ms(k);
            let samples = ms * 0.001 * rate;
            let plays = on && ms <= 10000.0;
            let lvl = self.tap(k, T_LEVEL) / 100.0;
            let g = stereo_pan(self.tap(k, T_PAN), self.tap(k, T_SPREAD), self.tap(k, T_FLIP) >= 0.5).map(|x| x * lvl);
            let ratio = if self.tap(k, T_PITCH) >= 0.5 { 2f32.powf((self.tap(k, T_TRANSP) + self.tap(k, T_FINE) / 100.0) / 12.0) } else { 1.0 };
            self.plan[k] = (samples.clamp(1.0, 10.0 * rate), plays, g, ratio);
            let q = 0.5 + 0.08 * self.tap(k, T_RESO);
            let (hp, lp) = (self.tap(k, T_HP), self.tap(k, T_LP));
            let t = &mut self.taps[k];
            for s in 0..2 {
                t.hp[s].set(rate, hp, q);
                t.lp[s].set(rate, lp.min(rate * 0.45), q);
            }
        }
    }

    fn clear(&mut self) {
        self.line[0].clear();
        self.line[1].clear();
        self.fb = [0.0; 2];
        for t in self.taps.iter_mut() {
            for s in 0..2 {
                t.hp[s].clear();
                t.lp[s].clear();
                t.shift[s].clear();
            }
            t.hp1 = [0.0; 2];
            t.lp1 = [0.0; 2];
        }
    }
}

impl Effect for DelayDesigner {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rate = self.rate;
        let fb_on = self.p.on(FEEDBACK);
        let fb_g = if fb_on { db(self.p.get(FEEDBACK_LEVEL)).min(0.98) } else { 0.0 };
        let fb_tap = self.p.idx(FEEDBACK_TAP);
        let (dry, wet) = (self.p.get(MIX_DRY) / 100.0, self.p.get(MIX_WET) / 100.0);
        let filt: Vec<(bool, bool, f32, f32)> = (0..TAPS).map(|k| (self.tap(k, T_FILTER) >= 0.5, self.tap(k, T_SLOPE) >= 0.5, self.tap(k, T_HP), self.tap(k, T_LP))).collect();
        for i in 0..left.len().min(right.len()) {
            let (xl, xr) = (left[i], right[i]);
            let (mut wl, mut wr) = (0.0, 0.0);
            let mut fbs = [0f32; 2];
            for k in 0..TAPS {
                let (d, plays, g, ratio) = self.plan[k];
                if !plays {
                    continue;
                }
                let t = &mut self.taps[k];
                let mut y = [self.line[0].read(d), self.line[1].read(d)];
                let (fon, steep, hp, lp) = filt[k];
                if fon {
                    for s in 0..2 {
                        if steep {
                            if hp > 20.0 {
                                y[s] = t.hp[s].run(y[s]).high;
                            }
                            if lp < 20000.0 {
                                y[s] = t.lp[s].run(y[s]).low;
                            }
                        } else {
                            let a_hp = 1.0 - (-std::f32::consts::TAU * hp / rate).exp();
                            let a_lp = 1.0 - (-std::f32::consts::TAU * lp.min(rate * 0.45) / rate).exp();
                            t.hp1[s] = flush(t.hp1[s] + a_hp * (y[s] - t.hp1[s]));
                            let h = y[s] - t.hp1[s];
                            t.lp1[s] = flush(t.lp1[s] + a_lp * (h - t.lp1[s]));
                            y[s] = t.lp1[s];
                        }
                    }
                }
                if ratio != 1.0 {
                    y[0] = t.shift[0].run(y[0], ratio);
                    y[1] = t.shift[1].run(y[1], ratio);
                }
                let ol = y[0] * g[0] + y[1] * g[1];
                let or = y[0] * g[2] + y[1] * g[3];
                if k == fb_tap {
                    fbs = [ol, or];
                }
                wl += ol;
                wr += or;
            }
            self.fb = [flush(fbs[0] * fb_g), flush(fbs[1] * fb_g)];
            self.line[0].push(xl + self.fb[0]);
            self.line[1].push(xr + self.fb[1]);
            left[i] = xl * dry + wl * wet;
            right[i] = xr * dry + wr * wet;
        }
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm.max(1.0);
        self.update();
    }
}

impl Plugin for DelayDesigner {
    fn params(&self) -> &[Param] {
        self.p.defs
    }
    fn get(&self, id: u32) -> f32 {
        if (SEL..SEL + FIELDS.len() as u32).contains(&id) {
            return self.tap(self.p.idx(SELECTED), (id - SEL) as usize);
        }
        self.p.get(id)
    }
    fn set(&mut self, id: u32, value: f32) {
        if (SEL..SEL + FIELDS.len() as u32).contains(&id) {
            let k = self.p.idx(SELECTED);
            let f = (id - SEL) as usize;
            self.p.set(tap_id(k, f), value);
            // editing a filter or pitch value turns that module on (the first time)
            if matches!(f, T_HP | T_LP | T_RESO) {
                self.p.set(tap_id(k, T_FILTER), 1.0);
            }
            if matches!(f, T_TRANSP | T_FINE) {
                self.p.set(tap_id(k, T_PITCH), 1.0);
            }
            self.update();
            return;
        }
        if id == GRID {
            // the grid menu's shares (any stored value snaps to the nearest)
            let v = GRID_SHARE.iter().copied().min_by(|a, b| (a - value).abs().partial_cmp(&(b - value).abs()).unwrap()).unwrap_or(value);
            self.p.set(id, v);
            self.update();
            return;
        }
        if self.p.set(id, value) {
            self.update();
        }
    }
    fn reset(&mut self) {
        self.clear();
    }
    fn tail(&self) -> f32 {
        let last = self.taps_in_time().last().map_or(1000.0, |t| t.1) / 1000.0;
        if self.p.on(FEEDBACK) { (last * (1.0 + 60.0 / (-self.p.get(FEEDBACK_LEVEL)).max(0.5))).min(60.0) } else { last + 0.1 }
    }
    fn load_floats(&mut self, floats: &[f32]) {
        for d in self.p.defs {
            if let Some(v) = floats.get(d.id as usize) {
                self.p.set(d.id, *v);
            }
        }
        // Tap A always exists (a project's block has no on switch for it)
        if floats.len() < 1000 {
            self.p.set(tap_id(0, T_ON), 1.0);
        }
        self.update();
    }
}

pub fn grid_names() -> &'static [&'static str] {
    GRIDS
}
pub fn grid_share(i: usize) -> f32 {
    GRID_SHARE[i.min(GRID_SHARE.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse(dd: &mut DelayDesigner, n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0; n];
        l[0] = 1.0;
        let mut r = l.clone();
        dd.process(&mut l, &mut r);
        (l, r)
    }

    #[test]
    fn default_tap_a_at_one_second() {
        let mut dd = DelayDesigner::new(48000.0);
        dd.set(SYNC, 0.0);
        let (l, _) = impulse(&mut dd, 50000);
        let at = (100..50000).max_by(|a, b| l[*a].abs().partial_cmp(&l[*b].abs()).unwrap()).unwrap();
        assert!((at as i64 - 48000).abs() <= 1, "{at}");
        assert!((l[at] - 0.63).abs() < 0.02, "{}", l[at]);
    }

    #[test]
    fn three_taps_sync_and_swing() {
        let mut dd = DelayDesigner::new(48000.0);
        dd.set_tempo(120.0);
        // 1/16 at 120 = 125 ms; 380 ms snaps to 375
        dd.set(SELECTED, 0.0);
        dd.set(SEL + T_DELAY as u32, 380.0);
        assert!((dd.played_ms(0) - 375.0).abs() < 0.01);
        let b = dd.add_tap(130.0).unwrap();
        assert_eq!(tap_letter(b), 'B');
        assert!((dd.played_ms(b) - 125.0).abs() < 0.01);
        // swing 75 %: the second step of each pair moves later (125 → 187.5)
        dd.set(SWING, 75.0);
        assert!((dd.played_ms(b) - 187.5).abs() < 0.01, "{}", dd.played_ms(b));
        let (l, _) = impulse(&mut dd, 24000);
        let peaks: Vec<usize> = (1..24000).filter(|i| l[*i].abs() > 0.3).collect();
        assert!(peaks.contains(&9000) && peaks.contains(&21000), "{peaks:?}");
    }

    #[test]
    fn pan_and_flip() {
        let mut dd = DelayDesigner::new(48000.0);
        dd.set(SYNC, 0.0);
        dd.set(SEL + T_DELAY as u32, 10.0);
        dd.set(SEL + T_PAN as u32, 100.0); // full left
        dd.set(SEL + T_SPREAD as u32, 0.0);
        let mut l = vec![0.0; 1000];
        let mut r = vec![0.0; 1000];
        l[0] = 1.0;
        r[0] = 1.0;
        dd.process(&mut l, &mut r);
        assert!(l[480].abs() > 0.5 && r[480].abs() < 1e-6, "{} {}", l[480], r[480]);
        dd.reset();
        dd.set(SEL + T_FLIP as u32, 1.0);
        let mut l = vec![0.0; 1000];
        let mut r = vec![0.0; 1000];
        l[0] = 1.0;
        r[0] = 1.0;
        dd.process(&mut l, &mut r);
        assert!(r[480].abs() > 0.5 && l[480].abs() < 1e-6);
    }

    #[test]
    fn feedback_tap_repeats() {
        let mut dd = DelayDesigner::new(48000.0);
        dd.set(SYNC, 0.0);
        dd.set(SEL + T_DELAY as u32, 100.0);
        dd.set(FEEDBACK, 1.0);
        dd.set(FEEDBACK_LEVEL, -6.0);
        let (l, _) = impulse(&mut dd, 20000);
        assert!(l[4800].abs() > 0.5 && l[9600].abs() > 0.15 && l[9600].abs() < l[4800].abs());
    }

    #[test]
    fn tap_filter_and_pitch() {
        let mut dd = DelayDesigner::new(48000.0);
        dd.set(SYNC, 0.0);
        dd.set(MIX_DRY, 0.0);
        dd.set(SEL + T_DELAY as u32, 5.0);
        dd.set(SEL + T_LP as u32, 500.0);
        let mut l: Vec<f32> = (0..48000).map(|i| (std::f32::consts::TAU * 6000.0 * i as f32 / 48000.0).sin()).collect();
        let mut r = l.clone();
        dd.process(&mut l, &mut r);
        let p = l[24000..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(p < 0.02, "{p}");
        let mut dd = DelayDesigner::new(48000.0);
        dd.set(SYNC, 0.0);
        dd.set(MIX_DRY, 0.0);
        dd.set(SEL + T_DELAY as u32, 5.0);
        dd.set(SEL + T_TRANSP as u32, 12.0);
        let mut l: Vec<f32> = (0..24000).map(|i| (std::f32::consts::TAU * 200.0 * i as f32 / 48000.0).sin()).collect();
        let mut r = l.clone();
        dd.process(&mut l, &mut r);
        let f = crate::kit::yin(&l[12000..16096], 48000.0, 50.0, 2000.0).unwrap();
        assert!((f - 400.0).abs() < 10.0, "{f}");
    }

    #[test]
    fn tap_pads_replace_taps() {
        let mut dd = DelayDesigner::new(48000.0);
        dd.set_taps(&[200.0, 400.0, 650.0]);
        assert_eq!(dd.taps_in_time().len(), 3);
        assert_eq!(dd.p.idx(FEEDBACK_TAP), 2);
    }
}
