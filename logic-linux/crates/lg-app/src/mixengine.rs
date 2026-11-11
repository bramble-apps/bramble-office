//! The engine's side of the channel strip settings in lg-core trackmix: the input format (the
//! Channel Mode button), the panners (Pan with the project's Pan Law on mono strips, Balance on
//! stereo ones, Stereo Pan, the Binaural Panner), solo with solo-safe strips and signal flow,
//! tracks turned off, frozen tracks playing their freeze file, send modes (Post Fader before
//! pan, Post Pan, Independent Pan, bypassed, Low Latency Safe), Low Latency Monitoring, Dim on
//! the master, and the held peak levels with their clipping flags.

use std::sync::Arc;

use lg_core::trackmix::{Binaural, InputFormat, PanMode, SendSet, StereoPan};

use crate::app::App;
use crate::audio::{self, Engine, Out, RATE};

/// The project's Pan Law: the level at the centre, and whether the sides are raised to make up
/// for it ("Compensated").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanLaw {
    pub center_db: f32,
    pub compensated: bool,
}

impl Default for PanLaw {
    fn default() -> Self {
        // (what Studio's mixer did before the choice was wired: centre at unity)
        PanLaw { center_db: -3.0, compensated: true }
    }
}

impl PanLaw {
    pub fn parse(s: &str) -> PanLaw {
        let db = if s.starts_with("-6") { -6.0 } else if s.starts_with("-4.5") { -4.5 } else if s.starts_with("-3") { -3.0 } else { 0.0 };
        PanLaw { center_db: db, compensated: s.contains("Compensated") }
    }

    /// Left and right gain of a mono signal at `pan` (-1…1).
    pub fn gains(self, pan: f32) -> (f32, f32) {
        let x = (pan.clamp(-1.0, 1.0) + 1.0) / 2.0;
        if self.center_db == 0.0 {
            // 0 dB: full level at the centre, the far side fades
            return ((2.0 * (1.0 - x)).min(1.0), (2.0 * x).min(1.0));
        }
        // a power law with the chosen level at the centre: cos^k / sin^k, k from the centre level
        let k = self.center_db / (20.0 * std::f32::consts::FRAC_1_SQRT_2.log10());
        let a = x * std::f32::consts::FRAC_PI_2;
        let (mut l, mut r) = (a.cos().powf(k), a.sin().powf(k));
        if self.compensated {
            let c = 10f32.powf(-self.center_db / 20.0);
            l *= c;
            r *= c;
        }
        (l, r)
    }
}

/// Balance on a stereo strip: the side turned away from fades, the other keeps its level.
pub fn balance_gains(pan: f32, compensate: Option<PanLaw>) -> (f32, f32) {
    if let Some(law) = compensate {
        return law.gains(pan);
    }
    let p = pan.clamp(-1.0, 1.0);
    let fade = |x: f32| (x * std::f32::consts::FRAC_PI_2).cos();
    (if p > 0.0 { fade(p) } else { 1.0 }, if p < 0.0 { fade(-p) } else { 1.0 })
}

const BIN_BUF: usize = 16384;

/// One source through the binaural model: a distance delay (Doppler when it moves), a delay
/// and a head-shadow low-pass per ear.
#[derive(Clone)]
struct BinSource {
    buf: Vec<f32>,
    w: usize,
    /// The smoothed distance delay in samples.
    dist: f32,
    lp_l: f32,
    lp_r: f32,
    gl: f32,
    gr: f32,
}

impl Default for BinSource {
    fn default() -> Self {
        BinSource { buf: vec![0.0; BIN_BUF], w: 0, dist: -1.0, lp_l: 0.0, lp_r: 0.0, gl: 0.0, gr: 0.0 }
    }
}

impl BinSource {
    fn read(&self, back: f32) -> f32 {
        let pos = self.w as f32 - back + BIN_BUF as f32;
        let i = pos.floor();
        let f = pos - i;
        let a = self.buf[i as usize % BIN_BUF];
        let b = self.buf[(i as usize + 1) % BIN_BUF];
        a + (b - a) * f
    }

    /// Adds the source `x` as heard from `angle` into `ol` / `or`.
    fn process(&mut self, b: &Binaural, angle: f32, x: &[f32], ol: &mut [f32], or: &mut [f32]) {
        let e = b.ears(angle);
        let rate = RATE as f32;
        // Doppler: the sound travels the distance; a moving source slides the delay (pitch)
        let want = if b.doppler { (b.distance / 343.0 * rate).min(BIN_BUF as f32 - 256.0) } else { 0.0 };
        if self.dist < 0.0 {
            self.dist = want;
            (self.gl, self.gr) = (e.gain_l, e.gain_r);
        }
        let (dl, dr) = (e.delay_l * rate, e.delay_r * rate);
        // the far ear is duller; a source behind is duller in both
        let coef = |hz: f32| 1.0 - (-std::f32::consts::TAU * hz / rate).exp();
        let near_hz = if e.behind { 9000.0 } else { 20000.0 };
        let (cl, cr) = if e.delay_l > e.delay_r { (coef(e.far_lowpass), coef(near_hz)) } else if e.delay_r > e.delay_l { (coef(near_hz), coef(e.far_lowpass)) } else { (coef(near_hz), coef(near_hz)) };
        // (without Diffuse-Field Compensation the ears' own colouring is left in: a brighter
        // near side, as the raw response; with it the sound stays neutral for headphones)
        let tilt = if b.diffuse_field { 0.0 } else { 0.35 };
        for k in 0..x.len() {
            self.buf[self.w] = x[k];
            self.dist += (want - self.dist) * 0.0005;
            self.gl += (e.gain_l - self.gl) * 0.002;
            self.gr += (e.gain_r - self.gr) * 0.002;
            let a = self.read(self.dist + dl + 1.0);
            let c = self.read(self.dist + dr + 1.0);
            self.lp_l += cl * (a - self.lp_l);
            self.lp_r += cr * (c - self.lp_r);
            ol[k] += (self.lp_l + tilt * (a - self.lp_l)) * self.gl;
            or[k] += (self.lp_r + tilt * (c - self.lp_r)) * self.gr;
            self.w = (self.w + 1) % BIN_BUF;
        }
    }
}

/// A strip's settings as the engine uses them.
#[derive(Clone, Default)]
pub struct StripDsp {
    pub format: InputFormat,
    pub pan_mode: PanMode,
    pub stereo: StereoPan,
    pub binaural: Binaural,
    pub solo_safe: bool,
    /// An aux strip's Solo button.
    pub solo: bool,
    pub off: bool,
    /// The freeze file (interleaved stereo from tick 0 at the project's tempo), played instead
    /// of the track; `freeze_skips_fx`: Pre Fader (the effects are in the file).
    pub frozen: Option<Arc<Vec<f32>>>,
    pub freeze_skips_fx: bool,
    pub sends: Vec<SendSet>,
    pub phase_invert: bool,
    /// Input gain as a factor.
    pub input_gain: f32,
    /// Low Latency Monitoring applies to this strip (focused, record-enabled or monitored).
    pub low_latency: bool,
    bin: [BinSource; 2],
}

/// The engine's extension (Engine::ext).
pub struct EngineExt {
    pub tracks: Vec<StripDsp>,
    pub auxes: Vec<StripDsp>,
    pub pan_law: PanLaw,
    /// Apply Pan Law compensation to stereo balancers.
    pub law_on_balance: bool,
    /// The master's Dim button and the Dim Level as a gain.
    pub dim: bool,
    pub dim_gain: f32,
    /// Peak levels held until reset (the peak level displays), and the clipping flags.
    pub peaks: Vec<f32>,
    pub aux_peaks: Vec<f32>,
    pub master_peak: f32,
    /// Low Latency Monitoring Mode and its limit in samples.
    pub low_latency: bool,
    pub ll_limit: usize,
    /// Which inserts Low Latency Monitoring bypasses now, by track (for the slots' look).
    pub ll_bypassed: Vec<Vec<bool>>,
    /// The freeze being rendered leaves this off; set when the transport starts.
    dry_l: Vec<f32>,
    dry_r: Vec<f32>,
    tmp_l: Vec<f32>,
    tmp_r: Vec<f32>,
}

impl Default for EngineExt {
    fn default() -> Self {
        EngineExt { tracks: Vec::new(), auxes: Vec::new(), pan_law: PanLaw::default(), law_on_balance: true, dim: false, dim_gain: 0.1, peaks: Vec::new(), aux_peaks: Vec::new(), master_peak: 0.0, low_latency: false, ll_limit: 240, ll_bypassed: Vec::new(), dry_l: Vec::new(), dry_r: Vec::new(), tmp_l: Vec::new(), tmp_r: Vec::new() }
    }
}

/// How a send is taken from the strip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Conn {
    pub gl: f32,
    pub gr: f32,
    /// From the signal before the panner (kept aside when the panner works in place).
    pub dry: bool,
}

impl EngineExt {
    fn strip(&self, node: usize, n_tracks: usize) -> Option<&StripDsp> {
        if node < n_tracks { self.tracks.get(node) } else { self.auxes.get(node - n_tracks) }
    }

    /// A track's sound before its effects: a frozen track plays its freeze file; then the input
    /// format, the phase and the input gain. True when a freeze file put sound in.
    pub fn source(&mut self, i: usize, playing: bool, start_tick: f64, tps: f64, l: &mut [f32], r: &mut [f32]) -> bool {
        let Some(s) = self.tracks.get(i) else { return false };
        let mut any = false;
        if let (Some(f), true) = (&s.frozen, playing) {
            let first = (start_tick / tps).round().max(0.0) as usize;
            for k in 0..l.len() {
                let fr = first + k;
                if 2 * fr + 1 >= f.len() {
                    break;
                }
                l[k] = f[2 * fr];
                r[k] = f[2 * fr + 1];
                any = true;
            }
        }
        s.format.apply(l, r);
        if s.phase_invert || s.input_gain != 1.0 {
            let g = if s.phase_invert { -s.input_gain } else { s.input_gain };
            for x in l.iter_mut().chain(r.iter_mut()) {
                *x *= g;
            }
        }
        any
    }

    /// The inserts a strip skips: its own bypasses, all of them on a track frozen Pre Fader,
    /// and in Low Latency Monitoring those that push the strip over the limit.
    pub fn bypass(&mut self, i: usize, mut bypass: Vec<bool>, latencies: &[(usize, usize)]) -> Vec<bool> {
        let Some(s) = self.tracks.get(i) else { return bypass };
        if s.frozen.is_some() && s.freeze_skips_fx {
            let n = latencies.iter().map(|x| x.0 + 1).max().unwrap_or(0).max(bypass.len());
            return vec![true; n];
        }
        if self.ll_bypassed.len() <= i {
            self.ll_bypassed.resize(i + 1, Vec::new());
        }
        self.ll_bypassed[i].clear();
        if self.low_latency && s.low_latency {
            let mut total = 0usize;
            for &(slot, lat) in latencies {
                if bypass.get(slot).copied().unwrap_or(false) {
                    continue;
                }
                if total + lat > self.ll_limit {
                    if bypass.len() <= slot {
                        bypass.resize(slot + 1, false);
                    }
                    bypass[slot] = true;
                    if self.ll_bypassed[i].len() <= slot {
                        self.ll_bypassed[i].resize(slot + 1, false);
                    }
                    self.ll_bypassed[i][slot] = true;
                } else {
                    total += lat;
                }
            }
        }
        bypass
    }

    /// The panner of strip `node`: returns the output's left and right gains. Stereo Pan and
    /// the Binaural Panner work on the block in place (the unpanned sound kept for the sends).
    pub fn pan(&mut self, node: usize, n_tracks: usize, vol: f32, pan: f32, surround: bool, l: &mut [f32], r: &mut [f32]) -> (f32, f32) {
        let (law, law_bal) = (self.pan_law, self.law_on_balance);
        let s = if node < n_tracks { self.tracks.get_mut(node) } else { self.auxes.get_mut(node - n_tracks) };
        let Some(s) = s else {
            let (a, b) = PanLaw::default().gains(pan);
            return (vol * a, vol * b);
        };
        if surround {
            return (vol, vol);
        }
        match s.pan_mode {
            PanMode::Balance => {
                let (a, b) = if s.format.is_stereo() { balance_gains(pan, law_bal.then_some(law)) } else { law.gains(pan) };
                (vol * a, vol * b)
            }
            PanMode::StereoPan => {
                self.dry_l.clear();
                self.dry_l.extend_from_slice(l);
                self.dry_r.clear();
                self.dry_r.extend_from_slice(r);
                let m = s.stereo.matrix();
                for k in 0..l.len() {
                    let (a, b) = (l[k], r[k]);
                    l[k] = a * m[0][0] + b * m[0][1];
                    r[k] = a * m[1][0] + b * m[1][1];
                }
                (vol, vol)
            }
            PanMode::Binaural => {
                self.dry_l.clear();
                self.dry_l.extend_from_slice(l);
                self.dry_r.clear();
                self.dry_r.extend_from_slice(r);
                self.tmp_l.clear();
                self.tmp_l.resize(l.len(), 0.0);
                self.tmp_r.clear();
                self.tmp_r.resize(l.len(), 0.0);
                let b = s.binaural;
                if s.format.is_stereo() && b.spread > 0.0 {
                    let (al, ar) = b.sides();
                    s.bin[0].process(&b, al, &self.dry_l, &mut self.tmp_l, &mut self.tmp_r);
                    s.bin[1].process(&b, ar, &self.dry_r, &mut self.tmp_l, &mut self.tmp_r);
                } else {
                    // one puck: the mono sum from the direction
                    for k in 0..l.len() {
                        self.dry_l[k] = (l[k] + r[k]) * 0.5;
                    }
                    s.bin[0].process(&b, b.angle, &self.dry_l, &mut self.tmp_l, &mut self.tmp_r);
                    self.dry_l.copy_from_slice(l);
                }
                l.copy_from_slice(&self.tmp_l);
                r.copy_from_slice(&self.tmp_r);
                (vol, vol)
            }
        }
    }

    /// Send `k` of strip `node`: its gains and where it is taken. None: off (bypassed, or put
    /// away by Low Latency Monitoring).
    pub fn send(&self, node: usize, n_tracks: usize, k: usize, snd: &audio::Send, vol: f32, gl: f32, gr: f32) -> Option<Conn> {
        let Some(s) = self.strip(node, n_tracks) else {
            return Some(Conn { gl: if snd.pre { snd.gain } else { snd.gain * gl }, gr: if snd.pre { snd.gain } else { snd.gain * gr }, dry: false });
        };
        let set = s.sends.get(k).copied().unwrap_or_default();
        if set.bypass || (self.low_latency && s.low_latency && !set.low_latency_safe) {
            return None;
        }
        let in_place = s.pan_mode != PanMode::Balance;
        if let Some(p) = set.independent_pan {
            // Independent Pan: the send's own place, from the unpanned sound
            let (a, b) = if s.format.is_stereo() { balance_gains(p, None) } else { self.pan_law.gains(p) };
            let lvl = if snd.pre { snd.gain } else { snd.gain * vol };
            return Some(Conn { gl: lvl * a, gr: lvl * b, dry: in_place });
        }
        Some(if snd.pre {
            Conn { gl: snd.gain, gr: snd.gain, dry: in_place }
        } else if set.post_pan {
            Conn { gl: snd.gain * gl, gr: snd.gain * gr, dry: false }
        } else {
            // Post Fader: after the fader, before pan
            Conn { gl: snd.gain * vol, gr: snd.gain * vol, dry: in_place }
        })
    }

    /// The block a connection is taken from: the panner's output, or the sound before it.
    pub fn tap<'a>(&'a self, dry: bool, l: &'a [f32], r: &'a [f32]) -> (&'a [f32], &'a [f32]) {
        if dry && self.dry_l.len() == l.len() { (&self.dry_l, &self.dry_r) } else { (l, r) }
    }

    pub fn meter(&mut self, node: usize, n_tracks: usize, peak: f32) {
        let v = if node < n_tracks { &mut self.peaks } else { &mut self.aux_peaks };
        let k = if node < n_tracks { node } else { node - n_tracks };
        if v.len() <= k {
            v.resize(k + 1, 0.0);
        }
        v[k] = v[k].max(peak);
    }

    /// The master's gain with Dim.
    pub fn master_gain(&self) -> f32 {
        if self.dim { self.dim_gain } else { 1.0 }
    }

    /// Clicking a peak level display: all of them start again.
    pub fn reset_peaks(&mut self) {
        self.peaks.iter_mut().for_each(|p| *p = 0.0);
        self.aux_peaks.iter_mut().for_each(|p| *p = 0.0);
        self.master_peak = 0.0;
    }
}

/// Whether track `i` sounds: not muted, not turned off; with a solo anywhere, soloed itself,
/// solo-safe, or feeding a soloed aux (its signal flow).
pub fn audible(e: &Engine, i: usize) -> bool {
    let t = &e.tracks[i];
    let s = e.ext.tracks.get(i);
    if t.mute || s.is_some_and(|s| s.off) {
        return false;
    }
    let track_solo = e.tracks.iter().any(|t| t.solo);
    let aux_solo = e.ext.auxes.iter().any(|a| a.solo);
    if !track_solo && !aux_solo {
        return true;
    }
    if t.solo || s.is_some_and(|s| s.solo_safe) {
        return true;
    }
    // feeding a soloed aux: by its output or a send
    let feeds = |bus: usize| e.auxes.iter().zip(e.ext.auxes.iter()).any(|(a, x)| x.solo && a.bus == bus);
    matches!(e.track_out.get(i), Some(Out::Bus(b)) if feeds(*b)) || t.sends.iter().any(|s| feeds(s.bus))
}

/// The engine's strips from the project and the Mixer (after mixstate::to_engine): keeps the
/// panners' running state where the strips stayed.
pub fn to_engine(app: &App, e: &mut Engine) {
    let Some(p) = app.project.as_ref() else { return };
    let old_t = std::mem::take(&mut e.ext.tracks);
    let old_a = std::mem::take(&mut e.ext.auxes);
    let focused = app.selected_track;
    let make = |set: &lg_core::trackmix::StripSet, old: Option<&StripDsp>| {
        let mut d = StripDsp { format: set.format, pan_mode: set.pan_mode, stereo: set.stereo_pan, binaural: set.binaural, solo_safe: set.solo_safe, sends: set.sends.clone(), phase_invert: set.phase_invert, input_gain: crate::app::db_to_gain(set.input_gain), ..Default::default() };
        if let Some(o) = old {
            d.bin = o.bin.clone();
        }
        d
    };
    e.ext.tracks = p
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let set = &t.extra.set;
            let mut d = make(&set.strip, old_t.get(i));
            d.off = set.off || set.assign != lg_core::trackmix::Assign::Strip;
            if set.frozen {
                d.frozen = app.tm.freeze.files.get(&t.extra.id).map(|f| f.samples.clone());
                d.freeze_skips_fx = set.freeze_mode == lg_core::trackmix::FreezeMode::PreFader;
            }
            d.low_latency = (focused == Some(i) && t.kind != lg_core::TrackKind::Audio) || app.armed.contains(&i) || app.monitoring.contains(&i);
            d
        })
        .collect();
    e.ext.auxes = app
        .auxes
        .iter()
        .enumerate()
        .map(|(k, a)| {
            let set = app.tm.strips.get(&(crate::mixer::AUX + k)).cloned().unwrap_or_default();
            let mut d = make(&set, old_a.get(k));
            d.solo = a.solo;
            d
        })
        .collect();
    // solo on auxes: an aux muted only because another is soloed stays on when it is solo-safe,
    // or when a soloed track feeds it
    let aux_solo = app.auxes.iter().any(|a| a.solo);
    if aux_solo {
        for (k, a) in app.auxes.iter().enumerate() {
            let safe = e.ext.auxes.get(k).is_some_and(|x| x.solo_safe);
            let (_, vca_mute) = crate::mixstate::vca_offset(app, crate::mixer::AUX + k);
            if let Some(x) = e.auxes.get_mut(k) {
                x.mute = a.mute || vca_mute || (!a.solo && !safe);
            }
        }
    }
    e.ext.pan_law = PanLaw::parse(&app.proj.get("Audio / General / Pan Law:"));
    e.ext.law_on_balance = app.proj.on("Audio / General / Apply Pan Law compensation to stereo balancers");
    e.ext.dim = app.tm.mx.dim;
    let dim_db = app.logic_settings.get("Audio / General / Dim Level:").parse::<f32>().unwrap_or(-20.0).clamp(-30.0, 0.0);
    e.ext.dim_gain = crate::app::db_to_gain(dim_db);
    e.ext.low_latency = app.low_latency || app.logic_settings.on("Audio / General / Low Latency Monitoring Mode");
    let limit_ms = app.logic_settings.get("Audio / General / Limit:").parse::<f32>().unwrap_or(5.0).clamp(0.0, 30.0);
    e.ext.ll_limit = (limit_ms / 1000.0 * RATE as f32) as usize;
}

/// The settings alone into the running engine (a knob moved: no rebuild).
pub fn sync(app: &App) {
    let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
    to_engine(app, &mut e);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_laws() {
        let l = PanLaw::parse("-3 dB");
        let (a, b) = l.gains(0.0);
        assert!((a - 0.7071).abs() < 1e-3 && (b - 0.7071).abs() < 1e-3);
        let (a, b) = l.gains(-1.0);
        assert!((a - 1.0).abs() < 1e-4 && b.abs() < 1e-4);
        let c = PanLaw::parse("-3 dB Compensated");
        let (a, _) = c.gains(0.0);
        assert!((a - 1.0).abs() < 1e-3, "compensated: unity at the centre");
        assert!((c.gains(-1.0).0 - 1.4125).abs() < 1e-3, "+3 dB at the sides");
        let z = PanLaw::parse("0 dB");
        assert_eq!(z.gains(0.0), (1.0, 1.0));
        assert_eq!(z.gains(1.0), (0.0, 1.0));
        let six = PanLaw::parse("-6 dB").gains(0.0).0;
        assert!((six - 0.5).abs() < 2e-3, "{six}");
        let q = PanLaw::parse("-4.5 dB").gains(0.0).0;
        assert!((20.0 * q.log10() + 4.5).abs() < 0.05);
    }

    #[test]
    fn balance_keeps_the_near_side() {
        assert_eq!(balance_gains(0.0, None), (1.0, 1.0));
        let (l, r) = balance_gains(1.0, None);
        assert!(l.abs() < 1e-6 && r == 1.0);
        let (l, r) = balance_gains(-0.5, None);
        assert!(l == 1.0 && r < 1.0 && r > 0.5);
    }

    fn ext(n: usize) -> EngineExt {
        EngineExt { tracks: (0..n).map(|_| StripDsp { input_gain: 1.0, ..Default::default() }).collect(), ..Default::default() }
    }

    #[test]
    fn stereo_pan_swaps_in_place_and_sends_stay_dry() {
        let mut x = ext(1);
        x.tracks[0].pan_mode = PanMode::StereoPan;
        x.tracks[0].stereo.swap();
        let (mut l, mut r) = (vec![1.0; 8], vec![0.0; 8]);
        let (gl, gr) = x.pan(0, 1, 0.5, 0.0, false, &mut l, &mut r);
        assert_eq!((gl, gr), (0.5, 0.5));
        assert!(l[0].abs() < 1e-6 && (r[0] - 1.0).abs() < 1e-6, "left comes out right");
        let snd = audio::Send { bus: 0, gain: 2.0, pre: false };
        let c = x.send(0, 1, 0, &snd, 0.5, gl, gr).unwrap();
        assert!(c.dry && c.gl == 1.0, "Post Fader: after the fader, before the panner");
        let (tl, tr) = x.tap(c.dry, &l, &r);
        assert!(tl[0] == 1.0 && tr[0] == 0.0);
        x.tracks[0].sends = vec![SendSet { post_pan: true, ..Default::default() }];
        let c = x.send(0, 1, 0, &snd, 0.5, gl, gr).unwrap();
        assert!(!c.dry);
        x.tracks[0].sends = vec![SendSet { independent_pan: Some(1.0), ..Default::default() }];
        let c = x.send(0, 1, 0, &snd, 0.5, gl, gr).unwrap();
        assert!(c.gl.abs() < 1e-6 && (c.gr - 1.0).abs() < 1e-6 && c.dry, "its own pan, hard right");
        x.tracks[0].sends = vec![SendSet { bypass: true, ..Default::default() }];
        assert!(x.send(0, 1, 0, &snd, 0.5, gl, gr).is_none());
    }

    #[test]
    fn binaural_puts_a_source_at_the_right_in_the_right_ear_first() {
        let mut x = ext(1);
        x.tracks[0].pan_mode = PanMode::Binaural;
        x.tracks[0].format = InputFormat::Mono;
        x.tracks[0].binaural.angle = 90.0;
        let n = 512;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        l[10] = 1.0;
        r[10] = 1.0;
        x.pan(0, 1, 1.0, 0.0, false, &mut l, &mut r);
        let first = |v: &[f32]| v.iter().position(|s| s.abs() > 0.02).unwrap();
        assert!(first(&r) < first(&l), "the far ear hears it later");
        let e = |v: &[f32]| v.iter().map(|s| s * s).sum::<f32>();
        assert!(e(&r) > e(&l) * 2.0, "and softer: {} vs {}", e(&r), e(&l));
    }

    #[test]
    fn frozen_tracks_play_their_file_and_skip_effects() {
        let mut x = ext(1);
        let file: Vec<f32> = (0..200).map(|k| k as f32).collect();
        x.tracks[0].frozen = Some(Arc::new(file));
        x.tracks[0].freeze_skips_fx = true;
        let (mut l, mut r) = (vec![0.0; 4], vec![0.0; 4]);
        assert!(x.source(0, true, 20.0, 2.0, &mut l, &mut r));
        assert_eq!((l[0], r[0], l[1]), (20.0, 21.0, 22.0), "frame 10 of the file at tick 20");
        assert!(!x.source(0, false, 20.0, 2.0, &mut l, &mut r), "stopped: silent");
        assert_eq!(x.bypass(0, vec![false, false], &[(0, 0), (1, 0)]), vec![true, true]);
    }

    #[test]
    fn low_latency_bypasses_what_goes_over_the_limit() {
        let mut x = ext(2);
        x.low_latency = true;
        x.ll_limit = 100;
        x.tracks[0].low_latency = true;
        assert_eq!(x.bypass(0, vec![false; 3], &[(0, 60), (1, 80), (2, 30)]), vec![false, true, false]);
        assert_eq!(x.ll_bypassed[0], vec![false, true]);
        assert_eq!(x.bypass(1, vec![false; 2], &[(0, 600), (1, 80)]), vec![false, false], "other tracks keep theirs");
        let snd = audio::Send { bus: 0, gain: 1.0, pre: false };
        assert!(x.send(0, 2, 0, &snd, 1.0, 1.0, 1.0).is_none(), "its sends go off");
        x.tracks[0].sends = vec![SendSet { low_latency_safe: true, ..Default::default() }];
        assert!(x.send(0, 2, 0, &snd, 1.0, 1.0, 1.0).is_some(), "unless Low Latency Safe");
    }

    #[test]
    fn peaks_hold_until_reset_and_dim_lowers_the_master() {
        let mut x = ext(1);
        x.meter(0, 1, 0.5);
        x.meter(0, 1, 0.2);
        x.meter(2, 1, 1.2);
        assert_eq!(x.peaks[0], 0.5);
        assert_eq!(x.aux_peaks[1], 1.2);
        x.reset_peaks();
        assert_eq!((x.peaks[0], x.aux_peaks[1]), (0.0, 0.0));
        assert_eq!(x.master_gain(), 1.0);
        x.dim = true;
        x.dim_gain = 0.1;
        assert_eq!(x.master_gain(), 0.1);
    }
}
