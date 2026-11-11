//! Surround effects (our own, by Logic's names): Multichannel Gain, Down Mixer, Surround
//! Compressor, Spatial Audio Monitoring (a surround mix to headphones), Binaural Post-Processing
//! (headphone compensation, speaker crosstalk cancellation), and Multi Mono — any stereo effect
//! run once per channel or stereo pair of a surround bus, with Logic's link groups A–E, per-group
//! on/off and Couple.
//!
//! Surround effects work through `Effect::process_surround` on one buffer per channel in Logic's
//! channel order; on a stereo strip they process L and R as a two-channel layout.

use std::sync::OnceLock;

use lg_dsp::surround::{self as sur, Ch, Format};
use lg_dsp::{Effect, Param, Plugin, Unit};

use crate::util::{coef, db, plugin_impl, Values};

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// The channels a per-channel control set covers (every speaker Logic has), in this order.
pub const CHANNELS: [Ch; 17] = Ch::ALL;

fn ch_slot(c: Ch) -> usize {
    CHANNELS.iter().position(|x| *x == c).unwrap_or(0)
}

/// Every surround effect here (Logic's names) with a native surround version.
pub const NATIVE: &[&str] = &["Multichannel Gain", "Down Mixer", "Surround Compressor", "Spatial Audio Monitoring"];

pub fn by_name(name: &str, rate: f32) -> Option<Box<dyn Effect>> {
    Some(match name {
        "Multichannel Gain" => Box::new(MultichannelGain::new(rate)),
        "Down Mixer" => Box::new(DownMixer::new()),
        "Surround Compressor" => Box::new(SurroundCompressor::new(rate)),
        "Spatial Audio Monitoring" => Box::new(SpatialMonitoring::new(rate)),
        "Binaural Post-Processing" => Box::new(BinauralPost::new(rate)),
        _ => return None,
    })
}

pub const NAMES: &[&str] = &["Multichannel Gain", "Down Mixer", "Surround Compressor", "Spatial Audio Monitoring", "Binaural Post-Processing"];

/// A stereo strip's two channels as a layout.
fn stereo_run(fx: &mut dyn Effect, l: &mut [f32], r: &mut [f32]) {
    let mut chans = vec![l.to_vec(), r.to_vec()];
    let mut layout = vec![Ch::L, Ch::R];
    fx.process_surround(&mut chans, &mut layout);
    l.copy_from_slice(&chans[0][..l.len()]);
    r.copy_from_slice(&chans[chans.len().min(2) - 1][..r.len()]);
}

// ---- Multichannel Gain ---------------------------------------------------------------------------

fn mcg_params() -> &'static [Param] {
    static P: OnceLock<Vec<Param>> = OnceLock::new();
    P.get_or_init(|| {
        let mut v = vec![Param::new(0, "Master", -96.0, 24.0, 0.0, Unit::Db)];
        for (k, c) in CHANNELS.iter().enumerate() {
            let b = 1 + 3 * k as u32;
            v.push(Param::new(b, leak(format!("{} Gain", c.label())), -96.0, 24.0, 0.0, Unit::Db));
            v.push(Param::new(b + 1, leak(format!("{} Phase Invert", c.label())), 0.0, 1.0, 0.0, Unit::OnOff));
            v.push(Param::new(b + 2, leak(format!("{} Mute", c.label())), 0.0, 1.0, 0.0, Unit::OnOff));
        }
        v
    })
}

/// Gain and phase per channel, mute per channel, and a master gain.
pub struct MultichannelGain {
    p: Values,
    peaks: Vec<f32>,
    _rate: f32,
}

impl MultichannelGain {
    pub fn new(rate: f32) -> MultichannelGain {
        MultichannelGain { p: Values::new(mcg_params()), peaks: vec![0.0; CHANNELS.len()], _rate: rate }
    }

    /// The gain applied to channel `c` (phase and mute included).
    pub fn gain_of(&self, c: Ch) -> f32 {
        let b = 1 + 3 * ch_slot(c) as u32;
        if self.p.on(b + 2) {
            return 0.0;
        }
        let g = |d: f32| if d <= -95.9 { 0.0 } else { db(d) };
        g(self.p.get(0)) * g(self.p.get(b)) * if self.p.on(b + 1) { -1.0 } else { 1.0 }
    }

    fn update(&mut self) {}
    fn clear(&mut self) {
        self.peaks.iter_mut().for_each(|x| *x = 0.0);
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl Effect for MultichannelGain {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        stereo_run(self, left, right);
    }
    fn process_surround(&mut self, chans: &mut Vec<Vec<f32>>, layout: &mut Vec<Ch>) -> bool {
        for (c, buf) in layout.iter().zip(chans.iter_mut()) {
            let g = self.gain_of(*c);
            let mut pk = 0f32;
            for x in buf.iter_mut() {
                *x *= g;
                pk = pk.max(x.abs());
            }
            let m = &mut self.peaks[ch_slot(*c)];
            *m = pk.max(*m * 0.95);
        }
        true
    }
    fn readouts(&self) -> Vec<(&'static str, f32)> {
        CHANNELS.iter().map(|c| (c.label(), self.peaks[ch_slot(*c)])).collect()
    }
}

plugin_impl!(MultichannelGain);

// ---- Down Mixer ----------------------------------------------------------------------------------

/// The Down Mixer's destinations (the formats a surround mix folds down to).
pub const DOWN_TO: [&str; 8] = ["Stereo", "Quadraphonic", "LCRS (Pro Logic)", "5.1 (ITU 775)", "6.1 (ES/EX)", "7.1", "7.1 (SDDS)", "Mono"];

static DM_PARAMS: [Param; 4] = [
    Param::menu(0, "Output Format", &DOWN_TO, 0.0),
    Param::new(1, "Center Level", -30.0, 6.0, 0.0, Unit::Db),
    Param::new(2, "Surround Level", -30.0, 6.0, 0.0, Unit::Db),
    Param::new(3, "LFE Level", -96.0, 6.0, -96.0, Unit::Db),
];

/// Folds a surround mix down to a narrower format (not for formats with overhead channels, as
/// Logic's). The level sliders it shows depend on the destination (`levels`).
pub struct DownMixer {
    p: Values,
}

impl DownMixer {
    pub fn new() -> DownMixer {
        DownMixer { p: Values::new(&DM_PARAMS) }
    }

    pub fn destination(&self) -> Vec<Ch> {
        match self.p.idx(0) {
            0 => vec![Ch::L, Ch::R],
            7 => vec![Ch::C],
            k => Format::from_name(DOWN_TO[k]).map_or(vec![Ch::L, Ch::R], |f| f.channels().to_vec()),
        }
    }

    /// The level sliders for this source and destination (names).
    pub fn levels(&self, from: &[Ch]) -> Vec<&'static str> {
        let to = self.destination();
        let mut v = Vec::new();
        if from.contains(&Ch::C) && !to.contains(&Ch::C) {
            v.push("Center Level");
        }
        if from.iter().any(|c| matches!(c, Ch::Ls | Ch::Rs | Ch::Lm | Ch::Rm | Ch::S) && !to.contains(c)) {
            v.push("Surround Level");
        }
        if from.contains(&Ch::Lfe) && !to.contains(&Ch::Lfe) {
            v.push("LFE Level");
        }
        v
    }

    fn update(&mut self) {}
    fn clear(&mut self) {}
    fn tail_secs(&self) -> f32 {
        0.0
    }
}

impl Default for DownMixer {
    fn default() -> Self {
        DownMixer::new()
    }
}

impl Effect for DownMixer {
    fn process(&mut self, _left: &mut [f32], _right: &mut [f32]) {}
    fn process_surround(&mut self, chans: &mut Vec<Vec<f32>>, layout: &mut Vec<Ch>) -> bool {
        let to = self.destination();
        // (as Logic's: not offered for formats with overhead channels, and only ever narrower)
        if layout.iter().any(|c| c.height()) || to.len() >= layout.len() {
            return true;
        }
        let from_fmt = Format::of_channels(layout).unwrap_or_default();
        let lv = |id: u32| if self.p.get(id) <= -95.9 { 0.0 } else { db(self.p.get(id)) };
        let m = sur::downmix_matrix(layout, &to, from_fmt, lv(1), lv(2), lv(3));
        let n = chans.first().map_or(0, |c| c.len());
        let mut out = vec![vec![0f32; n]; to.len()];
        if to == [Ch::C] {
            // mono: everything at −3 dB except the LFE
            for (s, c) in layout.iter().enumerate() {
                let g = if *c == Ch::Lfe { lv(3) } else { std::f32::consts::FRAC_1_SQRT_2 };
                for f in 0..n {
                    out[0][f] += chans[s][f] * g;
                }
            }
        } else {
            for (d, row) in m.iter().enumerate() {
                for (s, g) in row.iter().enumerate() {
                    if *g != 0.0 {
                        for f in 0..n {
                            out[d][f] += chans[s][f] * g;
                        }
                    }
                }
            }
        }
        *chans = out;
        *layout = to;
        true
    }
}

plugin_impl!(DownMixer);

// ---- Surround Compressor --------------------------------------------------------------------------

pub const CIRCUITS: [&str; 6] = ["Platinum", "Classic A_R", "Classic A_U", "VCA", "FET", "Opto"];
pub const GROUPS: [&str; 4] = ["-", "A", "B", "C"];

// ids: 0 circuit, 1 detection; main 2..8: ratio, knee, attack, release, auto, limiter, limiter
// threshold; LFE 9..15 the same; per channel k (CHANNELS order) 20 + 4k: threshold, output,
// group, bypass.
fn sc_params() -> &'static [Param] {
    static P: OnceLock<Vec<Param>> = OnceLock::new();
    P.get_or_init(|| {
        let mut v = vec![Param::menu(0, "Circuit Type", &CIRCUITS, 0.0), Param::menu(1, "Detection", &["Max", "Sum"], 0.0)];
        for (b, part) in [(2u32, "Main"), (9, "LFE")] {
            v.push(Param::new(b, leak(format!("{part} Ratio")), 1.0, 30.0, 2.0, Unit::Ratio));
            v.push(Param::new(b + 1, leak(format!("{part} Knee")), 0.0, 1.0, 0.2, Unit::None));
            v.push(Param::new(b + 2, leak(format!("{part} Attack")), 0.0, 200.0, 15.0, Unit::Ms));
            v.push(Param::new(b + 3, leak(format!("{part} Release")), 5.0, 5000.0, 150.0, Unit::Ms));
            v.push(Param::new(b + 4, leak(format!("{part} Auto")), 0.0, 1.0, 0.0, Unit::OnOff));
            v.push(Param::new(b + 5, leak(format!("{part} Limiter")), 0.0, 1.0, 0.0, Unit::OnOff));
            v.push(Param::new(b + 6, leak(format!("{part} Limiter Threshold")), -20.0, 0.0, 0.0, Unit::Db));
        }
        for (k, c) in CHANNELS.iter().enumerate() {
            let b = 20 + 4 * k as u32;
            v.push(Param::new(b, leak(format!("{} Threshold", c.label())), -50.0, 0.0, 0.0, Unit::Db));
            v.push(Param::new(b + 1, leak(format!("{} Output Level", c.label())), -30.0, 30.0, 0.0, Unit::Db));
            v.push(Param::menu(b + 2, leak(format!("{} Group", c.label())), &GROUPS, 1.0));
            v.push(Param::new(b + 3, leak(format!("{} Bypass", c.label())), 0.0, 1.0, 0.0, Unit::OnOff));
        }
        // (the LFE starts ungrouped)
        let lfe = 20 + 4 * ch_slot(Ch::Lfe) as u32 + 2;
        if let Some(p) = v.iter_mut().find(|p| p.id == lfe) {
            p.default = 0.0;
        }
        v
    })
}

/// Compressor for whole surround mixes: Main and LFE sections, per-channel threshold and output,
/// groups A–C whose detection is linked (Max or Sum), per-channel limiter, six circuit types.
pub struct SurroundCompressor {
    p: Values,
    rate: f32,
    env: Vec<f32>,
    gr: Vec<f32>,
    fast: Vec<f32>,
}

impl SurroundCompressor {
    pub fn new(rate: f32) -> SurroundCompressor {
        SurroundCompressor { p: Values::new(sc_params()), rate, env: vec![0.0; 17], gr: vec![0.0; 17], fast: vec![0.0; 17] }
    }

    fn base(c: Ch) -> u32 {
        20 + 4 * ch_slot(c) as u32
    }

    /// Group of a channel (0 none, 1–3 A–C).
    pub fn group(&self, c: Ch) -> usize {
        self.p.idx(Self::base(c) + 2)
    }

    /// Sets a channel's threshold or output (`which` 0 / 1), the rest of its group following —
    /// unless `unlink` (Command-Option while dragging).
    pub fn set_linked(&mut self, c: Ch, which: u32, v: f32, unlink: bool) {
        let g = self.group(c);
        let ids: Vec<u32> = if g == 0 || unlink { vec![Self::base(c) + which] } else { CHANNELS.iter().filter(|x| self.group(**x) == g).map(|x| Self::base(*x) + which).collect() };
        for id in ids {
            self.p.set(id, v);
        }
    }

    /// Bypasses a channel, or (`whole`) its group.
    pub fn bypass(&mut self, c: Ch, on: bool, whole: bool) {
        let g = self.group(c);
        let list: Vec<Ch> = if whole && g != 0 { CHANNELS.iter().copied().filter(|x| self.group(*x) == g).collect() } else { vec![c] };
        for x in list {
            self.p.set(Self::base(x) + 3, on as u8 as f32);
        }
    }

    fn update(&mut self) {}
    fn clear(&mut self) {
        self.env.iter_mut().for_each(|x| *x = 0.0);
        self.fast.iter_mut().for_each(|x| *x = 0.0);
        self.gr.iter_mut().for_each(|x| *x = 0.0);
    }
    fn tail_secs(&self) -> f32 {
        0.0
    }

    /// Gain reduction (dB, ≥ 0) for a detector level `lvl` (dB) over `thr`.
    fn curve(ratio: f32, knee: f32, thr: f32, lvl: f32) -> f32 {
        let w = knee * 20.0;
        let over = lvl - thr;
        let o = if w > 0.0 && over.abs() < w / 2.0 { (over + w / 2.0).powi(2) / (2.0 * w) } else if over > 0.0 { over } else { 0.0 };
        o * (1.0 - 1.0 / ratio.max(1.0))
    }
}

impl Effect for SurroundCompressor {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        stereo_run(self, left, right);
    }

    fn process_surround(&mut self, chans: &mut Vec<Vec<f32>>, layout: &mut Vec<Ch>) -> bool {
        let n = chans.first().map_or(0, |c| c.len());
        let p = &self.p;
        let circuit = p.idx(0);
        let sum = p.idx(1) == 1;
        // circuit character: attack / release scaling and soft saturation of the gain element
        let (atk_k, rel_k, colour) = match circuit {
            1 | 2 => (1.3, 1.2, 0.05),
            3 => (0.7, 0.9, 0.0),
            4 => (0.35, 0.6, 0.08),
            5 => (2.0, 2.5, 0.03),
            _ => (1.0, 1.0, 0.0),
        };
        let sec = |lfe: bool| -> (f32, f32, f32, f32, bool, bool, f32) {
            let b = if lfe { 9 } else { 2 };
            (p.get(b), p.get(b + 1), coef(self.rate, p.get(b + 2) * atk_k), coef(self.rate, p.get(b + 3) * rel_k), p.on(b + 4), p.on(b + 5), db(p.get(b + 6)))
        };
        let main = sec(false);
        let lfe = sec(true);
        let idx: Vec<usize> = layout.iter().map(|c| ch_slot(*c)).collect();
        for f in 0..n {
            // detectors: each channel's own envelope
            let mut lv = [0f32; 17];
            for (k, c) in layout.iter().enumerate() {
                let x = chans[k][f].abs();
                let (_, _, a, r, auto, ..) = if *c == Ch::Lfe { lfe } else { main };
                let s = idx[k];
                // Auto: release follows the material (a fast and a slow detector)
                self.fast[s] = if x > self.fast[s] { x } else { self.fast[s] * coef(self.rate, 30.0) };
                let rel = if auto && self.fast[s] < self.env[s] * 0.5 { r.powf(0.25) } else { r };
                self.env[s] = if x > self.env[s] { a * self.env[s] + (1.0 - a) * x } else { rel * self.env[s] + (1.0 - rel) * x };
                lv[s] = self.env[s];
            }
            // groups: Max (any channel over) or Sum (the combined level)
            let mut glv = [0f32; 4];
            for (k, c) in layout.iter().enumerate() {
                let g = p.idx(Self::base(*c) + 2);
                if g > 0 {
                    let v = lv[idx[k]];
                    glv[g] = if sum { (glv[g] * glv[g] + v * v).sqrt() } else { glv[g].max(v) };
                }
            }
            for (k, c) in layout.iter().enumerate() {
                let b = Self::base(*c);
                if p.on(b + 3) {
                    continue;
                }
                let g = p.idx(b + 2);
                let level = if g > 0 { glv[g] } else { lv[idx[k]] };
                let (ratio, knee, _, _, _, lim, lim_thr) = if *c == Ch::Lfe { lfe } else { main };
                let red = Self::curve(ratio, knee, p.get(b), lg_dsp::gain_to_db(level));
                self.gr[idx[k]] = red;
                let mut y = chans[k][f] * db(-red) * db(p.get(b + 1));
                if colour > 0.0 {
                    y = y - colour * y * y * y;
                }
                if lim && y.abs() > lim_thr {
                    // the integrated limiter: a soft knee onto the threshold
                    let over = y.abs() - lim_thr;
                    y = y.signum() * (lim_thr + over / (1.0 + over * 8.0)).min(lim_thr * 1.05);
                }
                chans[k][f] = y;
            }
        }
        true
    }

    fn gain_reduction(&self) -> f32 {
        self.gr.iter().copied().fold(0.0, f32::max)
    }

    fn readouts(&self) -> Vec<(&'static str, f32)> {
        CHANNELS.iter().map(|c| (c.label(), self.gr[ch_slot(*c)])).collect()
    }
}

plugin_impl!(SurroundCompressor);

// ---- Spatial Audio Monitoring ---------------------------------------------------------------------

static SAM_PARAMS: [Param; 4] = [
    Param::menu(0, "Monitoring Format", &["Apple Renderer", "Renderer for Built-in Speakers", "Renderer for Display Speakers"], 0.0),
    Param::new(1, "Head Tracking", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(2, "Personalized Spatial Audio Profile", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(3, "Mode", &["Music Mode", "Movie Mode"], 0.0),
];

/// A surround mix rendered to two channels for headphones (or a laptop's speakers): our own head
/// model standing in for Apple's renderer. Head tracking and personalized profiles need Apple's
/// hardware: their buttons are kept but do nothing.
pub struct SpatialMonitoring {
    p: Values,
    rate: f32,
    hp: Option<(usize, sur::Headphones)>,
}

impl SpatialMonitoring {
    pub fn new(rate: f32) -> SpatialMonitoring {
        SpatialMonitoring { p: Values::new(&SAM_PARAMS), rate, hp: None }
    }
    fn update(&mut self) {
        self.hp = None;
    }
    fn clear(&mut self) {
        self.hp = None;
    }
    fn tail_secs(&self) -> f32 {
        0.1
    }
}

impl Effect for SpatialMonitoring {
    fn process(&mut self, _left: &mut [f32], _right: &mut [f32]) {}
    fn process_surround(&mut self, chans: &mut Vec<Vec<f32>>, layout: &mut Vec<Ch>) -> bool {
        let Some(f) = Format::of_channels(layout) else { return true };
        let roomy = match (self.p.idx(0), self.p.idx(3)) {
            (0, 1) => 1.6,
            (0, _) => 1.0,
            _ => 0.6,
        };
        let key = self.p.idx(0) * 2 + self.p.idx(3);
        if self.hp.as_ref().is_none_or(|h| h.0 != key) {
            self.hp = Some((key, sur::Headphones::new(self.rate, roomy)));
        }
        // (the input in Logic's order for this format)
        let ordered: Vec<Vec<f32>> = f.channels().iter().map(|c| layout.iter().position(|x| x == c).map(|k| chans[k].clone()).unwrap_or_default()).collect();
        let n = chans.first().map_or(0, |c| c.len());
        let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
        if let Some((_, h)) = self.hp.as_mut() {
            h.run(f, &ordered, &mut l, &mut r);
        }
        *chans = vec![l, r];
        *layout = vec![Ch::L, Ch::R];
        true
    }
}

plugin_impl!(SpatialMonitoring);

// ---- Binaural Post-Processing -----------------------------------------------------------------------

pub const COMPENSATION: [&str; 4] = ["Headphone FF", "Headphone HB", "Headphone DF", "Speaker CTC"];

static BPP_PARAMS: [Param; 2] = [Param::menu(0, "Compensation Mode", &COMPENSATION, 2.0), Param::new(1, "CTC-Speaker Angle", 5.0, 90.0, 30.0, Unit::None)];

/// Conditioning for binaurally panned signals: headphone equalisation (free-field, horizontal,
/// diffuse-field) or crosstalk cancellation for two speakers at the CTC-Speaker Angle.
pub struct BinauralPost {
    p: Values,
    rate: f32,
    eq: [[crate::util::Biquad; 2]; 2],
    // crosstalk canceller: per side delay line of the other side's output, and its shadow filter
    buf: [Vec<f32>; 2],
    w: usize,
    lp: [f32; 2],
    delay: usize,
}

impl BinauralPost {
    pub fn new(rate: f32) -> BinauralPost {
        let mut b = BinauralPost { p: Values::new(&BPP_PARAMS), rate, eq: Default::default(), buf: [vec![0.0; 256], vec![0.0; 256]], w: 0, lp: [0.0; 2], delay: 1 };
        b.update();
        b
    }

    fn update(&mut self) {
        use crate::util::{Biquad, Coefs};
        // (Our own compensation curves: gentle, so a neutral source stays near flat.)
        let (f1, g1, f2, g2) = match self.p.idx(0) {
            0 => (3000.0, -4.0, 9000.0, -2.0),
            1 => (2500.0, -2.5, 8000.0, -1.5),
            2 => (3500.0, -2.0, 10000.0, -1.0),
            _ => (3000.0, 0.0, 9000.0, 0.0),
        };
        for ch in 0..2 {
            self.eq[ch][0] = Biquad::new(Coefs::peak(self.rate, f1, 1.0, g1));
            self.eq[ch][1] = Biquad::new(Coefs::high_shelf(self.rate, f2, 0.7, g2));
        }
        // the extra path to the far ear for speakers at ± angle / 2 (Woodworth)
        let half = (self.p.get(1) / 2.0).to_radians();
        let itd = 0.0875 / 343.0 * (half + half.sin());
        self.delay = ((itd * self.rate).round() as usize).clamp(1, 200);
    }

    fn clear(&mut self) {
        for e in self.eq.iter_mut().flatten() {
            e.clear();
        }
        for b in self.buf.iter_mut() {
            b.iter_mut().for_each(|x| *x = 0.0);
        }
        self.lp = [0.0; 2];
    }

    fn tail_secs(&self) -> f32 {
        0.05
    }
}

impl Effect for BinauralPost {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let ctc = self.p.idx(0) == 3;
        let n = self.buf[0].len();
        for i in 0..left.len().min(right.len()) {
            let (mut l, mut r) = (left[i], right[i]);
            if ctc {
                // recursive canceller: each side subtracts the other side's delayed, shadowed
                // output so the far ear's crosstalk cancels
                let d = self.delay;
                let xl = self.buf[1][(self.w + n - d) % n];
                let xr = self.buf[0][(self.w + n - d) % n];
                self.lp[0] += 0.45 * (xl - self.lp[0]);
                self.lp[1] += 0.45 * (xr - self.lp[1]);
                l -= 0.7 * self.lp[0];
                r -= 0.7 * self.lp[1];
                self.buf[0][self.w] = l;
                self.buf[1][self.w] = r;
                self.w = (self.w + 1) % n;
            } else {
                let a = self.eq[0][0].run(l);
                l = self.eq[0][1].run(a);
                let b = self.eq[1][0].run(r);
                r = self.eq[1][1].run(b);
            }
            left[i] = l;
            right[i] = r;
        }
    }
}

plugin_impl!(BinauralPost);

// ---- Multi Mono ------------------------------------------------------------------------------------

/// Logic's Link groups.
pub const LINKS: [&str; 6] = ["-", "A", "B", "C", "D", "E"];
/// Parameter ids of Multi Mono's own controls (after any Logic parameter block).
pub const MM_PANE: u32 = 9000;
pub const MM_COUPLE: u32 = 9001;
pub const MM_LINK: u32 = 9100;
pub const MM_ON: u32 = 9200;

/// A stereo effect run Multi Mono on a surround layout: one instance per channel, except that
/// left/right pairs (L-R, Ls-Rs, Lm-Rm, Lt-Rt…) share a stereo instance when linked together.
/// Channels in the same Link group (A–E) share parameter values; each group can be switched
/// off; Couple applies a change to every pane.
pub struct MultiMono {
    make: Box<dyn Fn() -> Option<Box<dyn Effect>> + Send>,
    layout: Vec<Ch>,
    /// Per channel of `layout`: its link group (0 none, 1–5 A–E) and whether its effect is on.
    pub link: Vec<usize>,
    pub on: Vec<bool>,
    /// Instances: the channels each runs (one, or a left/right pair) and the unit.
    units: Vec<(Vec<usize>, Box<dyn Effect>)>,
    /// The channel button shown (a pane: a group, or an unlinked channel), and Couple.
    pub pane: usize,
    pub couple: bool,
    params: &'static [Param],
    base_ids: Vec<u32>,
    configured: bool,
}

impl MultiMono {
    /// `make` builds one (stereo) instance; the layout is set on the first block (or `configure`).
    pub fn new(make: Box<dyn Fn() -> Option<Box<dyn Effect>> + Send>) -> Option<MultiMono> {
        let probe = make()?;
        let base: Vec<Param> = probe.params().to_vec();
        let base_ids = base.iter().map(|p| p.id).collect();
        let mut m = MultiMono { make, layout: Vec::new(), link: Vec::new(), on: Vec::new(), units: Vec::new(), pane: 0, couple: false, params: Box::leak(Vec::new().into_boxed_slice()), base_ids, configured: false };
        m.params = m.build_params(&base);
        Some(m)
    }

    /// Logic's preconfiguration for a layout: left/right pairs linked as stereo groups, centre
    /// and LFE on their own (5.1: L-R, Ls-Rs, C, LFE).
    pub fn configure(&mut self, layout: &[Ch]) {
        let keep: Vec<(Ch, usize, bool)> = self.layout.iter().zip(self.link.iter().zip(self.on.iter())).map(|(c, (l, o))| (*c, *l, *o)).collect();
        let values = self.units.first().map(|(_, u)| self.base_ids.iter().map(|id| (*id, u.get(*id))).collect::<Vec<_>>());
        self.layout = layout.to_vec();
        self.link.clear();
        self.on.clear();
        let mut next = 1usize;
        let mut pair_group: std::collections::HashMap<Ch, usize> = Default::default();
        for c in layout {
            if let Some(k) = keep.iter().find(|k| k.0 == *c) {
                self.link.push(k.1);
                self.on.push(k.2);
                continue;
            }
            let m = c.mirror();
            let g = if m != *c && layout.contains(&m) {
                *pair_group.entry(if (*c as u8) < (m as u8) { *c } else { m }).or_insert_with(|| {
                    let g = next.min(5);
                    next += 1;
                    g
                })
            } else {
                0
            };
            self.link.push(g);
            self.on.push(true);
        }
        self.rebuild();
        if let Some(vals) = values {
            for (_, u) in self.units.iter_mut() {
                for (id, v) in &vals {
                    u.set(*id, *v);
                }
            }
        }
        self.configured = true;
    }

    fn rebuild(&mut self) {
        self.units.clear();
        let mut done = vec![false; self.layout.len()];
        for k in 0..self.layout.len() {
            if done[k] {
                continue;
            }
            let c = self.layout[k];
            // a linked left/right pair runs as one stereo instance
            let partner = (self.link[k] != 0).then(|| self.layout.iter().position(|x| *x == c.mirror() && *x != c)).flatten().filter(|&j| self.link[j] == self.link[k] && !done[j]);
            let chans = match partner {
                Some(j) => {
                    done[j] = true;
                    if (c as u8) < (self.layout[j] as u8) { vec![k, j] } else { vec![j, k] }
                }
                None => vec![k],
            };
            done[k] = true;
            if let Some(u) = (self.make)() {
                self.units.push((chans, u));
            }
        }
        // channels joining a group take the group's values (from its first member); a group
        // nobody used before takes the front left channel's
        let mut first: [Option<usize>; 6] = [None; 6];
        for ui in 0..self.units.len() {
            let ch = self.units[ui].0[0];
            let g = self.link[ch];
            if g == 0 {
                continue;
            }
            match first[g] {
                None => first[g] = Some(ui),
                Some(f) => {
                    let vals: Vec<(u32, f32)> = self.base_ids.iter().map(|id| (*id, self.units[f].1.get(*id))).collect();
                    for (id, v) in vals {
                        self.units[ui].1.set(id, v);
                    }
                }
            }
        }
    }

    /// The channel buttons: (label, channels). Stereo instance channels join with "-", mono
    /// ones with "|"; everything in one group shows a single "Surround" button.
    pub fn panes(&self) -> Vec<(String, Vec<usize>)> {
        if !self.layout.is_empty() && self.link.iter().all(|g| *g != 0 && *g == self.link[0]) {
            return vec![("Surround".into(), (0..self.layout.len()).collect())];
        }
        let mut out: Vec<(String, Vec<usize>)> = Vec::new();
        let mut seen = [false; 6];
        for (k, g) in self.link.iter().enumerate() {
            if *g == 0 {
                out.push((self.layout[k].label().to_string(), vec![k]));
                continue;
            }
            if seen[*g] {
                continue;
            }
            seen[*g] = true;
            let members: Vec<usize> = (0..self.layout.len()).filter(|j| self.link[*j] == *g).collect();
            // label: instances' channels, stereo pairs with "-", instances separated by "|"
            let mut parts: Vec<String> = Vec::new();
            for (chans, _) in &self.units {
                if chans.iter().any(|c| members.contains(c)) {
                    parts.push(chans.iter().map(|c| self.layout[*c].label()).collect::<Vec<_>>().join("-"));
                }
            }
            out.push((parts.join("|"), members));
        }
        out
    }

    fn pane_channels(&self) -> Vec<usize> {
        let p = self.panes();
        if self.couple {
            return (0..self.layout.len()).collect();
        }
        p.get(self.pane.min(p.len().saturating_sub(1))).map(|x| x.1.clone()).unwrap_or_default()
    }

    fn build_params(&self, base: &[Param]) -> &'static [Param] {
        let mut v: Vec<Param> = base.to_vec();
        let labels: Vec<&'static str> = if self.layout.is_empty() { vec!["Surround"] } else { self.panes().into_iter().map(|(l, _)| leak(l)).collect() };
        let choices: &'static [&'static str] = Box::leak(labels.into_boxed_slice());
        v.push(Param::menu(MM_PANE, "Channel", choices, 0.0));
        v.push(Param::new(MM_COUPLE, "Couple", 0.0, 1.0, 0.0, Unit::OnOff));
        for (k, c) in self.layout.iter().enumerate() {
            v.push(Param::menu(MM_LINK + k as u32, leak(format!("{} Link", c.label())), &LINKS, 0.0).hide());
            v.push(Param::new(MM_ON + k as u32, leak(format!("{} Effect On", c.label())), 0.0, 1.0, 1.0, Unit::OnOff).hide());
        }
        Box::leak(v.into_boxed_slice())
    }

    fn refresh_params(&mut self) {
        let base: Vec<Param> = self.params.iter().filter(|p| self.base_ids.contains(&p.id)).cloned().collect();
        self.params = self.build_params(&base);
    }

    pub fn layout(&self) -> &[Ch] {
        &self.layout
    }

    pub fn instances(&self) -> usize {
        self.units.len()
    }
}

impl Plugin for MultiMono {
    fn params(&self) -> &[Param] {
        self.params
    }

    fn get(&self, id: u32) -> f32 {
        match id {
            MM_PANE => self.pane as f32,
            MM_COUPLE => self.couple as u8 as f32,
            _ if (MM_LINK..MM_LINK + 17).contains(&id) => self.link.get((id - MM_LINK) as usize).copied().unwrap_or(0) as f32,
            _ if (MM_ON..MM_ON + 17).contains(&id) => self.on.get((id - MM_ON) as usize).copied().unwrap_or(true) as u8 as f32,
            _ => {
                let chans = self.pane_channels();
                self.units.iter().find(|(c, _)| c.iter().any(|x| chans.contains(x))).or(self.units.first()).map_or(0.0, |(_, u)| u.get(id))
            }
        }
    }

    fn set(&mut self, id: u32, value: f32) {
        match id {
            MM_PANE => self.pane = value.round().max(0.0) as usize,
            MM_COUPLE => self.couple = value > 0.5,
            _ if (MM_LINK..MM_LINK + 17).contains(&id) => {
                let k = (id - MM_LINK) as usize;
                if k < self.link.len() {
                    self.link[k] = (value.round().max(0.0) as usize).min(5);
                    let vals: Vec<(u32, f32)> = self.units.first().map(|(_, u)| self.base_ids.iter().map(|i| (*i, u.get(*i))).collect()).unwrap_or_default();
                    self.rebuild();
                    // (a group nobody used takes the front left channel's settings)
                    if let Some((_, u)) = self.units.iter_mut().find(|(c, _)| c.contains(&k)) {
                        if self.link.iter().filter(|g| **g == self.link[k]).count() == 1 {
                            for (i, v) in &vals {
                                u.set(*i, *v);
                            }
                        }
                    }
                    self.refresh_params();
                }
            }
            _ if (MM_ON..MM_ON + 17).contains(&id) => {
                // (turning one channel's effect off turns off its whole group)
                let k = (id - MM_ON) as usize;
                if k < self.on.len() {
                    let g = self.link[k];
                    for j in 0..self.on.len() {
                        if j == k || (g != 0 && self.link[j] == g) {
                            self.on[j] = value > 0.5;
                        }
                    }
                }
            }
            _ => {
                let chans = self.pane_channels();
                for (c, u) in self.units.iter_mut() {
                    if c.iter().any(|x| chans.contains(x)) {
                        u.set(id, value);
                    }
                }
            }
        }
    }

    fn reset(&mut self) {
        for (_, u) in self.units.iter_mut() {
            u.reset();
        }
    }

    fn tail(&self) -> f32 {
        self.units.iter().map(|(_, u)| u.tail()).fold(0.0, f32::max)
    }

    fn load_floats(&mut self, floats: &[f32]) {
        for (_, u) in self.units.iter_mut() {
            u.load_floats(floats);
        }
        for (k, g) in self.link.iter_mut().enumerate() {
            if let Some(v) = floats.get((MM_LINK as usize) + k).filter(|v| **v > 0.0) {
                *g = (*v as usize).min(5);
            }
        }
    }
}

impl Effect for MultiMono {
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        // on a stereo strip: one stereo instance
        if let Some((_, u)) = self.units.first_mut() {
            u.process(left, right);
        } else if let Some(mut u) = (self.make)() {
            u.process(left, right);
            self.units.push((vec![0, 1], u));
        }
    }

    fn process_surround(&mut self, chans: &mut Vec<Vec<f32>>, layout: &mut Vec<Ch>) -> bool {
        if !self.configured || self.layout != *layout {
            self.configure(layout);
            self.refresh_params();
        }
        let n = chans.first().map_or(0, |c| c.len());
        let mut scratch = vec![0f32; n];
        for (cs, u) in self.units.iter_mut() {
            if cs.iter().any(|c| !self.on[*c]) {
                continue;
            }
            match cs.as_slice() {
                [a, b] => {
                    let (lo, hi) = (*a.min(b), *a.max(b));
                    let (x, y) = chans.split_at_mut(hi);
                    let (l, r) = if a < b { (&mut x[lo], &mut y[0]) } else { (&mut y[0], &mut x[lo]) };
                    u.process(l, r);
                }
                [a] => {
                    // a mono instance: the channel on both inputs, the left output kept
                    scratch.copy_from_slice(&chans[*a]);
                    u.process(&mut chans[*a], &mut scratch);
                }
                _ => {}
            }
        }
        true
    }

    fn latency(&self) -> usize {
        self.units.iter().map(|(_, u)| u.latency()).max().unwrap_or(0)
    }

    fn gain_reduction(&self) -> f32 {
        self.units.iter().map(|(_, u)| u.gain_reduction()).fold(0.0, f32::max)
    }

    fn set_tempo(&mut self, bpm: f64) {
        for (_, u) in self.units.iter_mut() {
            u.set_tempo(bpm);
        }
    }

    fn set_transport(&mut self, t: lg_dsp::Transport) {
        for (_, u) in self.units.iter_mut() {
            u.set_transport(t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(n: usize, chans: usize, v: f32) -> Vec<Vec<f32>> {
        vec![vec![v; n]; chans]
    }

    #[test]
    fn multichannel_gain_sets_gain_phase_and_mute_per_channel() {
        let mut g = MultichannelGain::new(48000.0);
        let f = Format::S51;
        g.set(1 + 3 * ch_slot(Ch::C) as u32, -6.0);
        g.set(1 + 3 * ch_slot(Ch::Ls) as u32 + 1, 1.0);
        g.set(1 + 3 * ch_slot(Ch::Lfe) as u32 + 2, 1.0);
        let mut c = block(8, 6, 1.0);
        let mut lay = f.channels().to_vec();
        assert!(g.process_surround(&mut c, &mut lay));
        assert!((c[1][0] - 0.501).abs() < 0.01 && c[3][0] == -1.0 && c[5][0] == 0.0 && c[0][0] == 1.0);
        g.set(0, -96.0);
        let mut c = block(8, 6, 1.0);
        g.process_surround(&mut c, &mut lay);
        assert!(c.iter().all(|x| x[0] == 0.0), "Master");
    }

    #[test]
    fn down_mixer_folds_5_1_to_stereo_and_skips_height_formats() {
        let mut d = DownMixer::new();
        let mut c = block(4, 6, 0.0);
        c[1] = vec![1.0; 4];
        let mut lay = Format::S51.channels().to_vec();
        d.process_surround(&mut c, &mut lay);
        assert_eq!(lay, vec![Ch::L, Ch::R]);
        assert!((c[0][0] - 0.708).abs() < 0.01, "centre −3 dB on the left: {}", c[0][0]);
        assert_eq!(d.levels(Format::S51.channels()), vec!["Center Level", "Surround Level", "LFE Level"]);
        let mut c = block(4, 12, 1.0);
        let mut lay = Format::S714.channels().to_vec();
        d.process_surround(&mut c, &mut lay);
        assert_eq!(lay.len(), 12, "not for formats with overhead channels");
        d.set(0, 3.0);
        let mut c = block(4, 8, 1.0);
        let mut lay = Format::S71.channels().to_vec();
        d.process_surround(&mut c, &mut lay);
        assert_eq!(lay, Format::S51.channels().to_vec());
    }

    #[test]
    fn surround_compressor_links_groups() {
        let mut s = SurroundCompressor::new(48000.0);
        let f = Format::S51;
        for c in f.channels() {
            s.set_linked(*c, 0, -20.0, true);
        }
        s.set(2, 10.0); // main ratio
        s.set(4, 0.1); // fast attack
        // only L is loud; R is in the same group (A), so it is turned down too (Max detection)
        let n = 9600;
        let mut c = block(n, 6, 0.0);
        c[0] = vec![0.9; n];
        c[2] = vec![0.05; n];
        let mut lay = f.channels().to_vec();
        s.process_surround(&mut c, &mut lay);
        assert!(c[2][n - 1] < 0.03, "grouped R follows L: {}", c[2][n - 1]);
        // ungroup R: it keeps its level
        s.set(SurroundCompressor::base(Ch::R) + 2, 0.0);
        let mut c = block(n, 6, 0.0);
        c[0] = vec![0.9; n];
        c[2] = vec![0.05; n];
        s.reset();
        s.process_surround(&mut c, &mut lay);
        assert!((c[2][n - 1] - 0.05).abs() < 0.005);
        // threshold moves the whole group
        s.set_linked(Ch::Ls, 0, -10.0, false);
        assert_eq!(s.get(SurroundCompressor::base(Ch::Rs)), -10.0);
        assert!(s.gain_reduction() > 1.0);
    }

    #[test]
    fn multi_mono_preconfigures_pairs_and_groups() {
        let mut m = MultiMono::new(Box::new(|| crate::by_logic_name("Gain", 48000.0))).unwrap();
        let mut c = block(4800, 6, 0.5);
        let mut lay = Format::S51.channels().to_vec();
        m.process_surround(&mut c, &mut lay);
        // 5.1: two stereo pairs plus mono C and LFE
        assert_eq!(m.instances(), 4);
        let labels: Vec<String> = m.panes().into_iter().map(|p| p.0).collect();
        assert_eq!(labels, vec!["L-R", "C", "Ls-Rs", "LFE"]);
        // a change in the L-R pane reaches only L and R
        m.set(MM_PANE, 0.0);
        m.set(4, -96.0);
        let mut c = block(4800, 6, 0.5);
        m.process_surround(&mut c, &mut lay);
        assert!(c[0][4799].abs() < 1e-3 && c[2][4799].abs() < 1e-3 && (c[1][4799] - 0.5).abs() < 1e-3);
        // Couple: everything
        m.set(MM_COUPLE, 1.0);
        m.set(4, -6.0);
        assert!((m.units.iter().all(|(_, u)| (u.get(4) + 6.0).abs() < 1e-3)));
        // link all in group A: one Surround button
        for k in 0..6 {
            m.set(MM_LINK + k, 1.0);
        }
        assert_eq!(m.panes()[0].0, "Surround");
        // a group's effect switched off
        m.set(MM_ON, 0.0);
        assert!(m.on.iter().all(|o| !o));
    }

    #[test]
    fn spatial_monitoring_renders_two_channels() {
        let mut s = SpatialMonitoring::new(48000.0);
        let mut c = block(480, 6, 0.0);
        c[0] = (0..480).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut lay = Format::S51.channels().to_vec();
        s.process_surround(&mut c, &mut lay);
        assert_eq!(lay, vec![Ch::L, Ch::R]);
        let e = |v: &Vec<f32>| v.iter().map(|x| x * x).sum::<f32>();
        assert!(e(&c[0]) > e(&c[1]));
    }

    #[test]
    fn binaural_post_processing_cancels_crosstalk() {
        let mut b = BinauralPost::new(48000.0);
        b.set(0, 3.0);
        let mut l = vec![0f32; 400];
        let mut r = vec![0f32; 400];
        l[0] = 1.0;
        b.process(&mut l, &mut r);
        // the right channel gets an inverted, delayed copy that cancels the left's crosstalk
        assert!(r.iter().any(|x| *x < -0.05), "{:?}", &r[..40]);
        b.set(0, 2.0);
        let mut l = vec![0.5f32; 4800];
        let mut r = vec![0.5f32; 4800];
        b.process(&mut l, &mut r);
        assert!((l[4799] - 0.5).abs() < 0.05, "DF: near neutral at low frequency");
    }

    #[test]
    fn surround_effects_stay_bounded_at_every_extreme() {
        for name in NAMES {
            for extreme in [0, 1] {
                let mut fx = by_name(name, 48000.0).unwrap();
                let ps: Vec<(u32, f32)> = fx.params().iter().map(|p| (p.id, if extreme == 0 { p.min } else { p.max })).collect();
                for (id, v) in ps {
                    fx.set(id, v);
                }
                let n = 48000;
                let mut seed: u32 = 7;
                let noise: Vec<f32> = (0..n).map(|i| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    if i < n / 2 { seed as f32 / u32::MAX as f32 * 2.0 - 1.0 } else { 0.0 }
                }).collect();
                let (mut l, mut r) = (noise.clone(), noise.clone());
                fx.process(&mut l, &mut r);
                let mut chans = vec![noise.clone(); 12];
                let mut lay = Format::S714.channels().to_vec();
                fx.process_surround(&mut chans, &mut lay);
                for v in l.iter().chain(r.iter()).chain(chans.iter().flatten()) {
                    // (Multichannel Gain at +24 dB twice: 48 dB is its range)
                    assert!(v.is_finite() && v.abs() < 300.0, "{name} extreme {extreme}: {v}");
                }
                let tail = &chans[0][n - 4800..];
                if extreme == 0 && *name != "Multichannel Gain" {
                    assert!(tail.iter().all(|x| x.abs() < 1e-2), "{name} falls silent");
                }
            }
        }
    }
}
