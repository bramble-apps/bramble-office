//! Alchemy stand-in ("Alchemist"): four sources (A–D), each a virtual-analog stack, an additive
//! partial bank, a spectral (noisy-partial + formant-noise) resynthesis, or a granular cloud over a
//! source buffer (our own synthesized texture, or the user's sample); per-source filters, two master
//! filters, two AHDSRs, LFOs, the eight Perform knobs, two XY pads, the Perform envelope and the
//! Transform pad (eight snapshots of the Perform section, interpolated by pad position), drive and a
//! delay + reverb. `load_preset_text` reads the plain-text preset Logic stores in projects.
//! Parameter ids = index in Logic's Alchemy Controls view.

use std::sync::{Arc, OnceLock};

use crate::dsp::{self, Adsr, CHUNK, Delay, Fade, Ladder, Osc, Rng, Room, Svf, SvfMode, Wave};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::retro::Lfo;
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const VA_SHAPES: &[&str] = &["Sine", "Square", "Saw", "Triangle"];
pub const SRC_FILTERS: &[&str] = &["Lowpass", "Bandpass", "Highpass", "Notch", "Comb"];
pub const MASTER_FILTERS: &[&str] = &[
    "LP 12dB",
    "LP 24dB Edgy",
    "LP 24dB Lush",
    "Bandpass",
    "Highpass",
    "Notch",
];
pub const PLAY_MODES: &[&str] = &["Poly", "Mono", "Legato"];

include!("alchemy_params.rs");

const NP: usize = 48; // partials
const NSPEC: usize = 8;
const GRAIN_ROOT: f32 = 130.812_78; // C3 (MIDI 48)

struct Banks {
    amp: [[f32; NP]; NSPEC],
    ratio: [[f32; NP]; NSPEC],
    /// Granular source: 2 s of evolving texture at GRAIN_ROOT.
    grain: Vec<f32>,
}

fn banks() -> &'static Banks {
    static B: OnceLock<Banks> = OnceLock::new();
    B.get_or_init(|| {
        let mut amp = [[0.0f32; NP]; NSPEC];
        let mut ratio = [[0.0f32; NP]; NSPEC];
        for s in 0..NSPEC {
            for k in 0..NP {
                let n = (k + 1) as f32;
                let (a, r) = match s {
                    0 => (if k == 0 { 1.0 } else { 0.0 }, n), // sine
                    1 => (
                        if [1, 2, 3, 4, 6, 8, 10, 12, 16].contains(&(k + 1)) {
                            0.7 / n.sqrt()
                        } else {
                            0.0
                        },
                        n,
                    ), // organ
                    2 => (if k % 2 == 0 { 1.0 / n } else { 0.0 }, n), // square / hollow
                    3 => (1.0 / n, n),                        // saw
                    4 => (
                        (-((n * 130.0 - 750.0) / 250.0).powi(2)).exp()
                            + 0.7 * (-((n * 130.0 - 1150.0) / 300.0).powi(2)).exp()
                            + 0.3 * (-((n * 130.0 - 2800.0) / 500.0).powi(2)).exp()
                            + 0.05 / n,
                        n,
                    ), // vocal "ah"
                    5 => (1.0 / (1.0 + ((n - 9.0) / 3.0).powi(2)), n), // glassy band
                    6 => (1.0 / n.powf(0.6), n * (1.0 + 0.0009 * n * n)), // stiff-string / metallic
                    _ => (0.8 / n.powf(0.8), n.powf(1.12)),   // bell (inharmonic)
                };
                amp[s][k] = a;
                ratio[s][k] = r;
            }
            let sum: f32 = amp[s].iter().sum();
            for a in &mut amp[s] {
                *a /= sum.max(1e-6) / 1.2;
            }
        }
        // granular texture: additive morph sine→vocal→glass with breath noise
        let rate = 48000.0f32;
        let n = (rate * 2.0) as usize;
        let mut grain = vec![0.0f32; n];
        let mut rng = Rng::new(77);
        let mut lp = 0.0f32;
        for (i, g) in grain.iter_mut().enumerate() {
            let t = i as f32 / rate;
            let x = t / 2.0;
            let mut s = 0.0;
            for k in 0..24 {
                let kf = (k + 1) as f32;
                let a = amp[4][k] * (1.0 - x)
                    + amp[5][k] * x
                    + amp[3][k] * 0.3 * (0.5 + 0.5 * (t * 1.3 + kf).sin());
                s += a * (std::f32::consts::TAU * GRAIN_ROOT * kf * t).sin();
            }
            lp += (rng.bipolar() - lp) * 0.2;
            *g = s * 0.8 + lp * 0.05;
        }
        let peak = grain.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        for g in &mut grain {
            *g /= peak.max(1e-6);
        }
        Banks { amp, ratio, grain }
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Kind {
    #[default]
    Off,
    Va,
    Additive,
    Spectral,
    Granular,
}

#[derive(Clone, Copy, Debug, Default)]
struct SourceCfg {
    kind: Kind,
    noise: bool,
    vol: f32,
    pan: f32,
    semis: f32,
    position: f32,
    speed: f32,
    density: f32,
    va_shape: u8,
    va_sym: f32,
    va_detune: f32,
    va_num: usize,
    va_sync: f32,
    cutoff: f32,
    res: f32,
    ftype: u8,
    keytrack: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AlchShared {
    rate: f32,
    src: [SourceCfg; 4],
    m_cut: f32,
    m_res: f32,
    m_type: u8,
    m_drive: f32,
    f2_on: bool,
    f2_cut: f32,
    f2_res: f32,
    env_amt: f32,
    amp_env: [f32; 4],
    filt_env: [f32; 4],
    vel_sens: f32,
    glide: f32,
    tune: f32,
    lfo1: f32,
    lfo2: f32,
    character: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Grain {
    pos: f32,
    inc: f32,
    age: f32,
    len: f32,
    pan: f32,
}

#[derive(Clone)]
struct SrcState {
    osc: [Osc; 4],
    sync: Osc,
    ph: [f32; NP],
    jit: [f32; NP],
    grains: [Grain; 12],
    gpos: f32,
    gnext: f32,
    filt: [Svf; 2],
    nf: Svf,
}

impl Default for SrcState {
    fn default() -> Self {
        SrcState {
            osc: [Osc::default(); 4],
            sync: Osc::default(),
            ph: [0.0; NP],
            jit: [1.0; NP],
            grains: [Grain::default(); 12],
            gpos: 0.0,
            gnext: 0.0,
            filt: [Svf::default(); 2],
            nf: Svf::default(),
        }
    }
}

pub(crate) struct AlchVoice {
    on: bool,
    note: u8,
    vel: f32,
    pitch: f32,
    target: f32,
    src: [SrcState; 4],
    amp: Adsr,
    fenv: Adsr,
    mf: [Ladder; 2],
    f2: [Svf; 2],
    rng: Rng,
    fade: Fade,
    killing: bool,
    level: f32,
    sample: Option<Arc<(Vec<f32>, f32)>>,
    mix: [(f32, f32); CHUNK],
}

impl AlchVoice {
    fn new(seed: u32) -> AlchVoice {
        AlchVoice {
            on: false,
            note: 60,
            vel: 1.0,
            pitch: 60.0,
            target: 60.0,
            src: Default::default(),
            amp: Adsr::default(),
            fenv: Adsr::default(),
            mf: [Ladder::new(); 2],
            f2: [Svf::default(); 2],
            rng: Rng::new(seed),
            fade: Fade::default(),
            killing: false,
            level: 0.0,
            sample: None,
            mix: [(0.0, 0.0); CHUNK],
        }
    }
}

fn spec_at(pos: f32) -> (usize, usize, f32) {
    let x = pos.clamp(0.0, 1.0) * (NSPEC - 1) as f32;
    let a = (x as usize).min(NSPEC - 2);
    (a, a + 1, x - a as f32)
}

impl Voice<AlchShared> for AlchVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &AlchShared, _ctl: &Ctl) {
        let rate = sh.rate;
        let was = self.on && !self.killing;
        self.on = true;
        self.killing = false;
        self.note = note;
        self.vel = velocity.max(1) as f32 / 127.0;
        self.target = note as f32;
        if !(was && sh.glide > 0.0) {
            self.pitch = self.target;
        }
        for (i, s) in self.src.iter_mut().enumerate() {
            for o in &mut s.osc {
                o.phase = self.rng.unit();
            }
            for k in 0..NP {
                s.ph[k] = self.rng.unit();
                s.jit[k] = 1.0;
            }
            s.grains = [Grain::default(); 12];
            s.gpos = sh.src[i].position;
            s.gnext = 0.0;
            let c = &sh.src[i];
            let mode = match c.ftype {
                1 => SvfMode::Band,
                2 => SvfMode::High,
                3 => SvfMode::Notch,
                _ => SvfMode::Low,
            };
            for f in &mut s.filt {
                f.reset();
                f.mode = mode;
            }
            s.nf = Svf::new(SvfMode::Band, 1200.0, 0.8, rate);
        }
        let [a, d, su, r] = sh.amp_env;
        self.amp.set(a, d, su, r, rate);
        let [a, d, su, r] = sh.filt_env;
        self.fenv.set(a, d, su, r, rate);
        if was {
            self.amp.gate_on();
            self.fenv.gate_on();
        } else {
            self.amp.trigger();
            self.fenv.trigger();
            for f in &mut self.mf {
                f.reset();
            }
        }
    }
    fn release(&mut self, _sh: &AlchShared, _ctl: &Ctl) {
        self.amp.gate_off();
        self.fenv.gate_off();
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.004, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &AlchShared, ctl: &Ctl) {
        let rate = sh.rate;
        let b = banks();
        if sh.glide > 0.0 {
            self.pitch = self.target
                + (self.pitch - self.target) * dsp::tau_coef(sh.glide * 0.3, rate / CHUNK as f32);
        } else {
            self.pitch = self.target;
        }
        let vib = sh.lfo1 * ctl.mod_wheel * 0.4;
        let base = self.pitch + sh.tune + ctl.bend + vib;
        let n = l.len();
        // master filter
        let fe = self.fenv.level;
        let key = (self.pitch - 60.0) / 12.0;
        let cut = (sh.m_cut * 9.97
            + sh.env_amt * fe * 5.5 * self.vel * self.vel
            + key * 0.5
            + sh.lfo2 * 0.3
            + (ctl.brightness - 0.5))
            .clamp(0.0, 10.0);
        let fc = (20.0 * 2f32.powf(cut)).min(rate * 0.45);
        for c in 0..2 {
            self.mf[c].set(fc, sh.m_res * 0.95, rate);
            self.mf[c].poles = if sh.m_type == 0 { 2 } else { 4 };
            self.mf[c].edgy = sh.m_type == 1;
            self.mf[c].drive = 1.0 + sh.m_drive * 4.0;
            self.f2[c].mode = SvfMode::Low;
            self.f2[c].set(
                (20.0 * 1000f32.powf(sh.f2_cut)).min(rate * 0.45),
                0.6 + sh.f2_res * 8.0,
                rate,
            );
        }
        // scratch mix of the four sources (stereo), at most CHUNK long
        let mut bl = [0.0f32; CHUNK];
        let mut br = [0.0f32; CHUNK];
        for si in 0..4 {
            let c = sh.src[si];
            if c.kind == Kind::Off || c.vol <= 0.0 {
                continue;
            }
            let hz = lg_dsp::midi_hz(base + c.semis);
            let st = &mut self.src[si];
            let sc = (20.0
                * 1000f32.powf(c.cutoff.clamp(0.0, 1.0))
                * if c.keytrack {
                    2f32.powf(key * 0.5)
                } else {
                    1.0
                })
            .min(rate * 0.45);
            for f in &mut st.filt {
                f.set(sc, 0.6 + c.res * 10.0, rate);
            }
            let filt_on = c.cutoff < 0.999 || c.ftype != 0;
            let (pl, pr) = ((1.0 - c.pan).sqrt(), (1.0 + c.pan).sqrt());
            let vol = c.vol;
            match c.kind {
                Kind::Va => {
                    let nos = c.va_num.clamp(1, 4);
                    let wave = match c.va_shape {
                        0 => Wave::Sine,
                        1 => Wave::Pulse,
                        3 => Wave::Triangle,
                        _ => Wave::Saw,
                    };
                    let g = vol / (nos as f32).sqrt();
                    for i in 0..n {
                        let mut sl = 0.0;
                        let mut sr = 0.0;
                        for u in 0..nos {
                            let d = if nos > 1 {
                                (u as f32 / (nos - 1) as f32 - 0.5) * c.va_detune * 0.6
                            } else {
                                0.0
                            };
                            let inc = hz * dsp::semis(d) / rate;
                            let x = if c.va_sync > 0.0 {
                                let wrapped = st.sync.advance(inc);
                                if wrapped {
                                    st.osc[u].phase = 0.0;
                                }
                                st.osc[u].next(wave, inc * (1.0 + c.va_sync * 4.0), c.va_sym)
                            } else {
                                st.osc[u].next(wave, inc, c.va_sym)
                            };
                            let p = if nos > 1 {
                                u as f32 / (nos - 1) as f32 * 2.0 - 1.0
                            } else {
                                0.0
                            } * 0.95;
                            sl += x * (1.0 - p).sqrt();
                            sr += x * (1.0 + p).sqrt();
                        }
                        bl[i] = sl * g;
                        br[i] = sr * g;
                    }
                }
                Kind::Additive | Kind::Spectral => {
                    let pos = (c.position + sh.character * 0.5).clamp(0.0, 1.0);
                    let (a0, a1, t) = spec_at(pos);
                    let spectral = c.kind == Kind::Spectral;
                    // partial amplitudes for this chunk
                    let mut amps = [0.0f32; NP];
                    let mut incs = [0.0f32; NP];
                    let mut sel = [0usize; NP];
                    let mut np = 0;
                    for k in 0..NP {
                        let ratio = b.ratio[a0][k] * (1.0 - t) + b.ratio[a1][k] * t;
                        let f = hz * ratio;
                        if f > rate * 0.45 {
                            break;
                        }
                        let mut a = b.amp[a0][k] * (1.0 - t) + b.amp[a1][k] * t;
                        if spectral {
                            // slowly wandering partial levels: the "alive" resynthesised sound
                            st.jit[k] +=
                                (self.rng.unit() * 2.0 - st.jit[k]) * (0.02 + 0.2 * c.speed);
                            a *= st.jit[k];
                        }
                        if a > 2e-3 && f < 14_000.0 {
                            // compact list of the audible partials
                            amps[np] = a * vol;
                            incs[np] = f / rate;
                            sel[np] = k;
                            np += 1;
                        }
                    }
                    st.nf.set((800.0 + 2500.0 * pos).min(rate * 0.4), 0.8, rate);
                    let noise_amt = if spectral { 0.15 * vol } else { 0.0 };
                    for i in 0..n {
                        let mut x = 0.0;
                        for j in 0..np {
                            let k = sel[j];
                            x += amps[j] * dsp::sin_cycles(st.ph[k]);
                            let ph = st.ph[k] + incs[j];
                            st.ph[k] = if ph >= 1.0 { ph - 1.0 } else { ph };
                        }
                        if noise_amt > 0.0 {
                            x += st.nf.run(self.rng.bipolar()) * noise_amt;
                        }
                        bl[i] = x * pl;
                        br[i] = x * pr;
                    }
                }
                Kind::Granular => {
                    let (buf, root): (&[f32], f32) = match &self.sample {
                        Some(s) => (&s.0, s.1),
                        None => (&b.grain, GRAIN_ROOT),
                    };
                    let len = buf.len() as f32;
                    if len < 1000.0 {
                        continue;
                    }
                    let ratio = hz / root;
                    let density = 8.0 + c.density * 80.0;
                    let glen = (0.12 - 0.08 * c.density) * rate;
                    for i in 0..n {
                        st.gnext -= 1.0;
                        if st.gnext <= 0.0 {
                            st.gnext = rate / density * (0.8 + 0.4 * self.rng.unit());
                            if let Some(g) = st.grains.iter_mut().find(|g| g.age >= g.len) {
                                let p = (st.gpos + self.rng.bipolar() * 0.02).rem_euclid(1.0);
                                *g = Grain {
                                    pos: p * (len - glen * ratio - 4.0).max(1.0),
                                    inc: ratio,
                                    age: 0.0,
                                    len: glen,
                                    pan: self.rng.bipolar() * 0.5,
                                };
                            }
                        }
                        let mut sl = 0.0;
                        let mut sr = 0.0;
                        for g in &mut st.grains {
                            if g.age >= g.len {
                                continue;
                            }
                            let w = 0.5 - 0.5 * dsp::sin_cycles(g.age / g.len + 0.25);
                            let ip = g.pos as usize;
                            let fr = g.pos - ip as f32;
                            let a = buf[ip.min(buf.len() - 2)];
                            let bb = buf[(ip + 1).min(buf.len() - 1)];
                            let x = (a + (bb - a) * fr) * w;
                            sl += x * (1.0 - g.pan);
                            sr += x * (1.0 + g.pan);
                            g.pos += g.inc;
                            g.age += 1.0;
                        }
                        let gn = vol * 0.5 / (density * glen / rate).max(1.0).sqrt();
                        bl[i] = sl * gn * pl;
                        br[i] = sr * gn * pr;
                    }
                    st.gpos = (st.gpos + c.speed * n as f32 / rate * 0.2).fract();
                }
                Kind::Off => {}
            }
            if c.noise {
                for i in 0..n {
                    let x = self.rng.bipolar() * 0.1 * vol;
                    bl[i] += x;
                    br[i] += x;
                }
            }
            if filt_on {
                for i in 0..n {
                    bl[i] = st.filt[0].run(bl[i]);
                    br[i] = st.filt[1].run(br[i]);
                }
            }
            for i in 0..n {
                self_mix_add(&mut self.mix, i, bl[i], br[i]);
            }
        }
        let vel_g = 1.0 - sh.vel_sens + sh.vel_sens * self.vel * self.vel;
        let mut peak = 0.0f32;
        for i in 0..n {
            let (mut xl, mut xr) = self.mix[i];
            self.mix[i] = (0.0, 0.0);
            xl = self.mf[0].run(xl);
            xr = self.mf[1].run(xr);
            if sh.f2_on {
                xl = self.f2[0].run(xl);
                xr = self.f2[1].run(xr);
            }
            self.fenv.next();
            let mut g = self.amp.next() * vel_g * 0.65;
            if self.killing {
                g *= self.fade.next();
            }
            let yl = xl * g;
            let yr = xr * g;
            peak = peak.max(yl.abs());
            l[i] += yl;
            r[i] += yr;
        }
        self.level = peak;
        if self.amp.idle() || (self.killing && self.fade.gain <= 0.0) {
            self.on = false;
        }
    }
    fn active(&self) -> bool {
        self.on
    }
    fn level(&self) -> f32 {
        self.level
    }
}

#[inline]
fn self_mix_add(mix: &mut [(f32, f32); CHUNK], i: usize, a: f32, b: f32) {
    mix[i].0 += a;
    mix[i].1 += b;
}

/// Perform section values: 8 knobs, 4 XY axes, 4 envelope values.
pub const PERFORM_IDS: [u32; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

pub struct Alchemy {
    params: Params,
    poly: Poly<AlchVoice>,
    ch: Channel,
    sh: AlchShared,
    rate: f32,
    lfo1: Lfo,
    lfo2: Lfo,
    rng: Rng,
    delay: [Delay; 2],
    room: Room,
    /// Transform pad snapshots (Perform values) and whether the pad drives the Perform section.
    pub snapshots: Vec<[f32; 16]>,
    transform: bool,
    defaults: [f32; 16],
    sample: Option<Arc<(Vec<f32>, f32)>>,
    pub name: String,
    /// The Arpeggiator, MSEG and effects rack (alchemy_extra).
    pub extra: crate::alchemy_extra::Extras,
    base_cut: f32,
    base_tune: f32,
    /// The settings before the modulation rack and MSEG move them (each block starts from it).
    base_sh: AlchShared,
}

/// Every parameter: Logic's Alchemy ids, then the Arpeggiator / MSEG / effects pages'.
pub fn all_params() -> &'static [Param] {
    static P: std::sync::OnceLock<Vec<Param>> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        let mut v = PARAMS.to_vec();
        v.extend(crate::alchemy_extra::params());
        v
    })
    .as_slice()
}

impl Alchemy {
    pub fn new(rate: f32) -> Alchemy {
        dsp::warm_tables();
        let _ = banks();
        let mut seed = 61;
        let params = Params::new(all_params());
        let mut defaults = [0.0; 16];
        for (k, &id) in PERFORM_IDS.iter().enumerate() {
            defaults[k] = params.get(id);
        }
        let sh0 = AlchShared {
                rate,
                src: [SourceCfg::default(); 4],
                m_cut: 0.6,
                m_res: 0.2,
                m_type: 1,
                m_drive: 0.0,
                f2_on: false,
                f2_cut: 1.0,
                f2_res: 0.0,
                env_amt: 0.5,
                amp_env: [0.005, 1.0, 0.7, 0.4],
                filt_env: [0.005, 0.6, 0.3, 0.4],
                vel_sens: 0.85,
                glide: 0.0,
                tune: 0.0,
                lfo1: 0.0,
                lfo2: 0.0,
                character: 0.0,
            };
        let mut a = Alchemy {
            params,
            poly: Poly::new(24, || {
                seed += 23;
                AlchVoice::new(seed)
            }),
            ch: Channel::new(rate),
            sh: sh0,
            rate,
            lfo1: Lfo::default(),
            lfo2: Lfo::default(),
            rng: Rng::new(17),
            delay: [
                Delay::new((rate * 1.0) as usize),
                Delay::new((rate * 1.0) as usize),
            ],
            room: Room::new(rate, 1.0),
            snapshots: Vec::new(),
            transform: false,
            defaults,
            sample: None,
            name: String::new(),
            extra: crate::alchemy_extra::Extras::new(rate),
            base_cut: 0.6,
            base_tune: 0.0,
            base_sh: sh0,
        };
        a.room.feedback = 0.82;
        a.update();
        a
    }

    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }

    /// Perform value k (0..16) after the Transform pad.
    fn perform(&self, k: usize) -> f32 {
        if self.transform && self.snapshots.len() >= 2 {
            let x = self.p(17);
            let y = self.p(18);
            let n = self.snapshots.len();
            let mut wsum = 0.0;
            let mut v = 0.0;
            for (i, s) in self.snapshots.iter().enumerate() {
                let ang = i as f32 / n as f32 * std::f32::consts::TAU;
                let (sx, sy) = (0.5 + 0.4 * ang.cos(), 0.5 + 0.4 * ang.sin());
                let d2 = (x - sx).powi(2) + (y - sy).powi(2);
                let w = 1.0 / (d2 + 1e-4);
                wsum += w;
                v += w * s[k];
            }
            v / wsum
        } else {
            self.p(PERFORM_IDS[k])
        }
    }

    fn update(&mut self) {
        let pf: Vec<f32> = (0..16).map(|k| self.perform(k)).collect();
        let d = |k: usize| pf[k] - self.defaults[k];
        let p = |id: u32| self.params.get(id);
        let envt = |x: f32, lo: f32, hi: f32| lo * (hi / lo).powf(x.clamp(0.0, 1.0));
        for s in 0..4 {
            let on = p(277 + s as u32) >= 0.5;
            let kind = if !on {
                Kind::Off
            } else if p(313 + s as u32) >= 0.5 {
                Kind::Granular
            } else if p(309 + s as u32) >= 0.5 {
                Kind::Spectral
            } else if p(293 + s as u32) >= 0.5 {
                Kind::Additive
            } else if p(297 + s as u32) >= 0.5 {
                Kind::Va
            } else {
                Kind::Off
            };
            let mut vol = p(19 + s as u32) * p(204 + s as u32);
            let mut semis = p(27 + s as u32) + p(31 + s as u32) / 100.0;
            let mut shape = p(301 + s as u32).round() as u8;
            let mut sym = p(208 + s as u32);
            let mut sync = p(250 + s as u32);
            let mut num = p(220 + s as u32).round() as usize;
            let mut det = p(216 + s as u32);
            // default Perform mapping (as on Logic's default patch): Octave Osc = source B level an
            // octave down, Square/Saw morphs A/C, unison and the B symmetry/sync on the XY pads
            if s == 1 {
                vol *= (0.25 + 0.5 * pf[3]).clamp(0.0, 1.0);
                if pf[3] > 0.5 && kind == Kind::Va && semis == 0.0 {
                    semis = -12.0;
                }
                sym = (sym + (pf[10] - 0.5) * 0.4).clamp(0.05, 0.95);
                sync = (sync + pf[11]).clamp(0.0, 1.0);
            }
            if s == 0 || s == 2 {
                shape = if pf[7] > 0.5 { 2 } else { 1 };
                num = (num as f32 + pf[8] * 3.0).round() as usize;
                det = (det + pf[9] * 0.6).min(1.0);
            }
            if shape > 3 {
                shape = 3;
            }
            self.sh.src[s] = SourceCfg {
                kind,
                noise: p(305 + s as u32) >= 0.5,
                vol,
                pan: p(23 + s as u32),
                semis,
                position: (p(35 + s as u32) + d(0) * 0.5).clamp(0.0, 1.0),
                speed: p(39 + s as u32),
                density: p(47 + s as u32),
                va_shape: shape,
                va_sym: sym,
                va_detune: det,
                va_num: num.clamp(1, 4),
                va_sync: sync,
                cutoff: p(51 + s as u32),
                res: p(55 + s as u32),
                ftype: p(333 + s as u32).round() as u8,
                keytrack: true,
            };
        }
        self.sh.character = d(0);
        self.sh.m_cut = (p(85) + d(1) * 0.5).clamp(0.0, 1.0);
        self.sh.m_res = p(86);
        self.sh.m_drive = (p(87) + pf[5] * 0.6).clamp(0.0, 1.0);
        self.sh.m_type = if p(364) >= 0.5 {
            p(365).round() as u8
        } else {
            0
        };
        self.sh.f2_on = p(367) >= 0.5;
        self.sh.f2_cut = p(89);
        self.sh.f2_res = p(90);
        self.sh.env_amt = (pf[2] * 0.8).clamp(-1.0, 1.0);
        self.sh.amp_env = [
            envt(pf[12], 0.001, 8.0),
            envt(pf[13], 0.02, 12.0),
            pf[14],
            envt(pf[15], 0.01, 10.0),
        ];
        self.sh.filt_env = [
            p(129).max(0.001),
            p(133) * envt(pf[6], 0.1, 10.0),
            p(135),
            p(137).max(0.01),
        ];
        self.sh.glide = p(98);
        self.sh.tune = p(96) + p(97) / 100.0;
        self.base_cut = self.sh.m_cut;
        self.base_tune = self.sh.tune;
        self.base_sh = self.sh;
        let params = &self.params;
        self.extra.read(&|i| params.get(i));
        self.poly.set_max(p(376).round() as usize);
        self.ch.bend_up = p(391);
        self.ch.bend_down = p(396);
        self.ch.update_bend();
    }

    /// Reads Logic's plain-text Alchemy preset (the project's AuCU text or an .acp file): the
    /// Perform knob values and the Transform snapshots.
    pub fn load_preset_text(&mut self, text: &str) {
        let mut section = String::new();
        let mut snaps: Vec<[f32; 16]> = Vec::new();
        let mut cur = [0.0f32; 16];
        let mut have = 0u32;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('<') && line.ends_with('>') {
                section = line[1..line.len() - 1].to_string();
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match section.as_str() {
                "alchemypreset" if k == "Name" => self.name = v.to_string(),
                "perform" => {
                    let Ok(x) = v.parse::<f32>() else { continue };
                    let idx = match k {
                        "_Contrl1" => Some(0),
                        "_Contrl2" => Some(1),
                        "_Contrl3" => Some(2),
                        "_Contrl4" => Some(3),
                        "_Contrl5" => Some(4),
                        "_Contrl6" => Some(5),
                        "_Contrl7" => Some(6),
                        "_Contrl8" => Some(7),
                        "_EnvAtt" => Some(12),
                        "_EnvDec" => Some(13),
                        "_EnvSust" => Some(14),
                        "_EnvRel" => Some(15),
                        "_XyPad1x" => Some(8),
                        "_XyPad1y" => Some(9),
                        "_XyPad2x" => Some(10),
                        "_XyPad2y" => Some(11),
                        "PerVol" => {
                            if have != 0 {
                                snaps.push(cur);
                                have = 0;
                            }
                            None
                        }
                        _ => None,
                    };
                    if let Some(i) = idx {
                        cur[i] = x;
                        have |= 1 << i;
                    }
                }
                _ => {}
            }
        }
        if have != 0 {
            snaps.push(cur);
        }
        if let Some(first) = snaps.first() {
            for (k, &id) in PERFORM_IDS.iter().enumerate() {
                self.params.set(id, first[k]);
            }
        }
        self.snapshots = snaps;
        self.update();
    }

    /// The user's sample for the granular sources (mono, its root frequency in Hz).
    pub fn set_granular_sample(&mut self, data: Vec<f32>, rate: f32, root_hz: f32) {
        let ratio = self.rate / rate;
        let s = Arc::new((data, root_hz * ratio.recip() * ratio));
        self.sample = Some(s.clone());
        for v in &mut self.poly.voices {
            v.sample = Some(s.clone());
        }
    }
}

impl Plugin for Alchemy {
    fn params(&self) -> &[Param] {
        all_params()
    }
    fn get(&self, id: u32) -> f32 {
        if (crate::alchemy_extra::FX_PARAM..crate::alchemy_extra::MOD).contains(&id) {
            return self.extra.fx_param(id).unwrap_or(0.0);
        }
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if (crate::alchemy_extra::FX_PARAM..crate::alchemy_extra::MOD).contains(&id) {
            self.extra.set_fx_param(id, v);
            return;
        }
        if self.params.set(id, v) {
            if id == 17 || id == 18 {
                self.transform = true;
            } else if PERFORM_IDS.contains(&id) {
                self.transform = false;
            }
            self.update();
        }
    }
    fn reset(&mut self) {
        for v in &mut self.poly.voices {
            v.on = false;
        }
        for d in &mut self.delay {
            d.clear();
        }
        self.room.reset();
        self.ch.reset();
        self.extra.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        self.sh.amp_env[3] + 2.0 + if self.extra.fx.iter().any(|f| f.is_some()) { 3.0 } else { 0.0 }
    }
}

impl Instrument for Alchemy {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        if self.extra.held == 0 {
            self.extra.mseg_t = 0.0;
        }
        self.extra.held += 1;
        if self.extra.arp_set.on {
            let s = self.extra.arp_set;
            self.extra.arp.note_on(&s, pitch, velocity);
            return;
        }
        self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
    }
    fn note_off(&mut self, pitch: u8) {
        self.extra.held = self.extra.held.saturating_sub(1);
        if self.extra.arp_set.on {
            let s = self.extra.arp_set;
            self.extra.arp.note_off(&s, pitch);
            return;
        }
        self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.extra.set_tempo(bpm);
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            return self.poly.all_notes_off(&self.sh, &self.ch.ctl);
        }
        if let Some(d) = self.ch.cc(cc, value) {
            self.poly.sustain(d, &self.sh, &self.ch.ctl);
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut sl = [0.0f32; CHUNK];
        let mut sr = [0.0f32; CHUNK];
        let (gl, gr) = self.ch.out_gains();
        let vol = self.p(94) / 0.8 * lg_dsp::db_to_gain(self.p(99));
        let fx = self.perform(4);
        let (dl, dr) = (0.375 * self.rate, 0.25 * self.rate);
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            let dt = m as f32 / self.rate;
            self.sh.lfo1 = self
                .lfo1
                .step(0, self.p(100).max(0.05) * 1.3 * dt, &mut self.rng);
            self.sh.lfo2 = self
                .lfo2
                .step(1, self.p(101).max(0.05) * 0.1 * dt, &mut self.rng);
            // the Arpeggiator plays the held keys
            if self.extra.arp_set.on {
                let s = self.extra.arp_set;
                let mut ev = [crate::arp::ArpEvent::Off(0); 16];
                let beats = dt as f64 * self.extra.bpm / 60.0;
                let k = self.extra.arp.advance(&s, beats, &mut ev);
                for e in &ev[..k] {
                    match *e {
                        crate::arp::ArpEvent::On(p, v) => self.poly.note_on(p, v, &self.sh, &self.ch.ctl),
                        crate::arp::ArpEvent::Off(p) => self.poly.note_off(p, &self.sh, &self.ch.ctl),
                    }
                }
            }
            // the MSEG and the modulation rack move the settings from where the knobs put them
            let mseg = self.extra.mseg_level();
            if self.extra.mseg_on || self.extra.nmods > 0 {
                let (l1, l2) = (self.sh.lfo1, self.sh.lfo2);
                self.sh = self.base_sh;
                self.sh.lfo1 = l1;
                self.sh.lfo2 = l2;
            }
            if self.extra.mseg_on {
                self.extra.mseg_t += dt;
                self.sh.m_cut = (self.base_cut + self.extra.mseg_amt[0] * mseg * 0.5).clamp(0.0, 1.0);
                self.sh.tune = self.base_tune + self.extra.mseg_amt[2] * mseg;
            }
            let mut mvol = if self.extra.mseg_on { (1.0 + self.extra.mseg_amt[1] * mseg).max(0.0) } else { 1.0 };
            for k in 0..self.extra.nmods {
                let (src, tg, depth) = self.extra.mods[k];
                let ctl = self.ch.ctl;
                let x = match src {
                    1 => self.sh.lfo1,
                    2 => self.sh.lfo2,
                    3 => mseg,
                    4 => ctl.mod_wheel,
                    5 => ctl.aftertouch,
                    6 => ctl.bend / 12.0,
                    _ => self.extra.arp_set.step_vel[self.extra.arp.current_step() % crate::arp::STEPS],
                } * depth;
                let sh = &mut self.sh;
                match tg {
                    1 => sh.m_cut = (sh.m_cut + x * 0.5).clamp(0.0, 1.0),
                    2 => sh.m_res = (sh.m_res + x * 0.5).clamp(0.0, 1.0),
                    3 => sh.m_drive = (sh.m_drive + x * 0.5).clamp(0.0, 1.0),
                    4 => sh.f2_cut = (sh.f2_cut + x * 0.5).clamp(0.0, 1.0),
                    5 => sh.tune += x * 12.0,
                    6 => mvol *= (1.0 + x).max(0.0),
                    7..=10 => sh.src[(tg - 7) as usize].vol = (sh.src[(tg - 7) as usize].vol * (1.0 + x)).max(0.0),
                    11..=14 => sh.src[(tg - 11) as usize].pan = (sh.src[(tg - 11) as usize].pan + x).clamp(-1.0, 1.0),
                    15..=18 => sh.src[(tg - 15) as usize].semis += x * 12.0,
                    19..=22 => sh.src[(tg - 19) as usize].position = (sh.src[(tg - 19) as usize].position + x * 0.5).clamp(0.0, 1.0),
                    _ => {}
                }
            }
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            if mvol != 1.0 {
                for i in 0..m {
                    sl[i] *= mvol;
                    sr[i] *= mvol;
                }
            }
            self.extra.process_fx(&mut sl[..m], &mut sr[..m]);
            if fx > 0.0 {
                for i in 0..m {
                    let wl = self.delay[0].tap(dl as usize);
                    let wr = self.delay[1].tap(dr as usize);
                    self.delay[0].push(sl[i] + wr * 0.35);
                    self.delay[1].push(sr[i] + wl * 0.35);
                    sl[i] += wl * fx * 0.5;
                    sr[i] += wr * fx * 0.5;
                }
                self.room.process(&mut sl[..m], &mut sr[..m], fx * 1.2);
            }
            for i in 0..m {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += m;
        }
    }
}
