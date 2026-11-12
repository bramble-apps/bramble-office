//! Alchemy stand-in ("Alchemist"): four sources (A–D), each a virtual-analog stack, an additive
//! partial bank, a spectral (noisy-partial + formant-noise) resynthesis, or a granular cloud over a
//! source buffer (our own synthesized texture, or the user's sample); per-source filters, two master
//! filters, two AHDSRs, LFOs, the eight Perform knobs, two XY pads, the Perform envelope and the
//! Transform pad (eight snapshots of the Perform section, interpolated by pad position), drive and a
//! delay + reverb. `load_preset_text` reads the plain-text preset Logic stores in projects.
//! Parameter ids = index in Logic's Alchemy Controls view.

use std::sync::{Arc, OnceLock};

use crate::dsp::{self, Adsr, CHUNK, Delay, Fade, Osc, Rng, Room, Svf, SvfMode, Wave};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::retro::Lfo;
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const VA_SHAPES: &[&str] = &["Sine", "Square", "Saw", "Triangle"];
/// Source and main filter types (alchemy_filter).
pub const SRC_FILTERS: &[&str] = crate::alchemy_filter::FILTER_TYPES;
pub const MASTER_FILTERS: &[&str] = crate::alchemy_filter::FILTER_TYPES;
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
    /// How much of the source goes to main filter 2 rather than 1 (the F1/F2 knob).
    route: f32,
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
    keytrack: bool,
    /// The three source filters: (type, cutoff knob, res knob, drive knob, on); parallel or not.
    sf: [(u8, f32, f32, f32, bool); 3],
    sf_par: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AlchShared {
    rate: f32,
    src: [SourceCfg; 4],
    m_cut: f32,
    m_res: f32,
    m_type: u8,
    m_on: bool,
    f2_type: u8,
    /// Each main filter's FX amount (the rest goes straight to the outputs).
    m_fx: f32,
    f2_fx: f32,
    m_drive: f32,
    f2_on: bool,
    f2_cut: f32,
    f2_res: f32,
    f2_drive: f32,
    /// Main filters in series (0) or parallel (1).
    par: f32,
    env_amt: f32,
    amp_env: [f32; 4],
    filt_env: [f32; 4],
    vel_sens: f32,
    glide: f32,
    tune: f32,
    lfo1: f32,
    lfo2: f32,
    character: f32,
    /// Master Vol (as a gain) and Pan, per voice (modulation targets).
    vol: f32,
    pan: f32,
    /// The note counters the Note Property sources read (set before each note on).
    note_index: u32,
    arp_index: u32,
    speed: f32,
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
    sf: [[crate::alchemy_filter::AFilt; 2]; 3],
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
            sf: Default::default(),
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
    mf: [crate::alchemy_filter::AFilt; 2],
    f2: [crate::alchemy_filter::AFilt; 2],
    /// What goes past the effects (the main filters' FX amounts below 100 %), this block.
    pub(crate) dry: [(f32, f32); CHUNK],
    pub(crate) dry_n: usize,
    rng: Rng,
    fade: Fade,
    killing: bool,
    level: f32,
    sample: Option<Arc<(Vec<f32>, f32)>>,
    mix: [(f32, f32); CHUNK],
    /// What the sources send to main filter 2 (their F1/F2 knob).
    mix2: [(f32, f32); CHUNK],
    /// The voice's modulators, its modulated settings (when a rack moves them) and levels
    /// (sources A–D, main filters 1–2) for the envelope followers.
    pub(crate) mv: crate::alchemy_mod::ModVoice,
    legato: bool,
    own: Option<AlchShared>,
    ov: crate::alchemy_mod::Overlay,
    lv: [f32; 6],
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
            mf: Default::default(),
            f2: Default::default(),
            dry: [(0.0, 0.0); CHUNK],
            dry_n: 0,
            rng: Rng::new(seed),
            fade: Fade::default(),
            killing: false,
            level: 0.0,
            sample: None,
            mix: [(0.0, 0.0); CHUNK],
            mix2: [(0.0, 0.0); CHUNK],
            mv: Default::default(),
            legato: false,
            own: None,
            ov: Default::default(),
            lv: [0.0; 6],
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
        self.legato = was && sh.glide > 0.0;
        self.mv.fresh = true;
        self.mv.note = note;
        self.mv.vel = self.vel;
        self.mv.index = sh.note_index;
        self.mv.arp_index = sh.arp_index;
        self.mv.speed = sh.speed;
        if !self.legato {
            self.mv.key = note as f32;
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
            if !was {
                for f in s.sf.iter_mut().flatten() {
                    f.reset();
                }
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
        self.mv.release();
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
        let own;
        let sh = match self.own {
            Some(o) => {
                own = o;
                &own
            }
            None => sh,
        };
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
            self.mf[c].set(sh.m_type as usize, fc, sh.m_res, sh.m_drive, rate);
            self.f2[c].set(sh.f2_type as usize, crate::alchemy_filter::cutoff_hz(sh.f2_cut), sh.f2_res, sh.f2_drive, rate);
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
            let kt = if c.keytrack { 2f32.powf(key * 0.5) } else { 1.0 };
            let mut filt_on = [false; 3];
            for (k, &(ty, cut, res, drv, on)) in c.sf.iter().enumerate() {
                filt_on[k] = on && !crate::alchemy_filter::transparent(ty as usize, cut);
                if filt_on[k] {
                    for f in &mut st.sf[k] {
                        f.set(ty as usize, crate::alchemy_filter::cutoff_hz(cut) * kt, res, drv, rate);
                    }
                }
            }
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
            // the source filters: 1 → 2 → 3 in series, or side by side
            let nf = filt_on.iter().filter(|x| **x).count();
            if nf > 0 {
                for i in 0..n {
                    let (xl, xr) = (bl[i], br[i]);
                    let (mut yl, mut yr) = (xl, xr);
                    if c.sf_par {
                        let (mut sl, mut sr) = (0.0, 0.0);
                        for k in 0..3 {
                            if filt_on[k] {
                                sl += st.sf[k][0].run(xl);
                                sr += st.sf[k][1].run(xr);
                            }
                        }
                        yl = sl / nf as f32;
                        yr = sr / nf as f32;
                    } else {
                        for k in 0..3 {
                            if filt_on[k] {
                                yl = st.sf[k][0].run(yl);
                                yr = st.sf[k][1].run(yr);
                            }
                        }
                    }
                    bl[i] = yl;
                    br[i] = yr;
                }
            }
            let mut pk = 0.0f32;
            let (w1, w2) = (1.0 - c.route.clamp(0.0, 1.0), c.route.clamp(0.0, 1.0));
            for i in 0..n {
                pk = pk.max(bl[i].abs()).max(br[i].abs());
                if w1 > 0.0 {
                    self_mix_add(&mut self.mix, i, bl[i] * w1, br[i] * w1);
                }
                if w2 > 0.0 {
                    self_mix_add(&mut self.mix2, i, bl[i] * w2, br[i] * w2);
                }
            }
            self.lv[si] = pk;
        }
        let vel_g = 1.0 - sh.vel_sens + sh.vel_sens * self.vel * self.vel;
        let mut peak = 0.0f32;
        let (mut f1p, mut f2p) = (0.0f32, 0.0f32);
        let (pl, pr) = ((1.0 - sh.pan).clamp(0.0, 1.0).sqrt() * std::f32::consts::SQRT_2, (1.0 + sh.pan).clamp(0.0, 1.0).sqrt() * std::f32::consts::SQRT_2);
        let (pl, pr) = (pl.min(1.0), pr.min(1.0));
        for i in 0..n {
            let (mut xl, mut xr) = self.mix[i];
            let (bl2, br2) = self.mix2[i];
            self.mix[i] = (0.0, 0.0);
            self.mix2[i] = (0.0, 0.0);
            if sh.m_on {
                xl = self.mf[0].run(xl);
                xr = self.mf[1].run(xr);
            }
            f1p = f1p.max(xl.abs());
            // filter 1's FX share goes to the effects; the rest goes on: in series (Par/Ser
            // 100 %) into filter 2, in parallel (0 %) to the outputs
            let (e1l, e1r) = (xl * sh.m_fx, xr * sh.m_fx);
            let (m1l, m1r) = (xl - e1l, xr - e1r);
            let (il, ir) = (bl2 + m1l * sh.par, br2 + m1r * sh.par);
            let (yl, yr) = if sh.f2_on { (self.f2[0].run(il), self.f2[1].run(ir)) } else { (il, ir) };
            f2p = f2p.max(yl.abs());
            let (e2l, e2r) = (yl * sh.f2_fx, yr * sh.f2_fx);
            xl = e1l + e2l;
            xr = e1r + e2r;
            let dl = m1l * (1.0 - sh.par) + yl - e2l;
            let dr = m1r * (1.0 - sh.par) + yr - e2r;
            self.fenv.next();
            let mut g = self.amp.next() * vel_g * 0.65 * sh.vol;
            if self.killing {
                g *= self.fade.next();
            }
            let yl = xl * g * pl;
            let yr = xr * g * pr;
            self.dry[i] = (dl * g * pl, dr * g * pr);
            peak = peak.max(yl.abs()).max(self.dry[i].0.abs());
            l[i] += yl;
            r[i] += yr;
        }
        self.dry_n = n;
        self.level = peak;
        self.lv[4] = f1p;
        self.lv[5] = f2p;
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

/// Each source's F1/F2 knob (A–D), and the Name bar's Limiter.
pub const ROUTE_F2: u32 = 14610;
pub const LIMITER: u32 = 14614;
/// Each source's filter switches, types 2–3, drives and Ser/Par: SRC_FILT + source × 16 + (0–2 on,
/// 3–4 type of filter 2–3, 5–7 drive, 8 parallel).
pub const SRC_FILT: u32 = 14620;

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
    /// The modulation section (alchemy_mod): settings, instrument-wide state, what's in use.
    pub mods: crate::alchemy_mod::ModCfg,
    pub mg: crate::alchemy_mod::ModGlobal,
    used: crate::alchemy_mod::Used,
    /// Perform values now (after the Transform pad).
    pf: [f32; 16],
    /// An AHDSR (per voice) on Master Vol: it shapes the voices instead of the Perform envelope.
    amp_from: Option<usize>,
    bpm: f32,
    side: f32,
    limiter: bool,
}

/// Every parameter: Logic's Alchemy ids, then the Arpeggiator / MSEG / effects pages'.
pub fn all_params() -> &'static [Param] {
    static P: std::sync::OnceLock<Vec<Param>> = std::sync::OnceLock::new();
    P.get_or_init(|| {
        let mut v = PARAMS.to_vec();
        v.extend(crate::alchemy_extra::params());
        v.extend(crate::alchemy_mod::params());
        for (k, n) in ["F1/F2 A", "F1/F2 B", "F1/F2 C", "F1/F2 D"].iter().enumerate() {
            v.push(Param::new(ROUTE_F2 + k as u32, n, 0.0, 1.0, 0.0, Unit::Ratio));
        }
        v.push(Param::new(LIMITER, "Limiter", 0.0, 1.0, 1.0, Unit::OnOff));
        for (s, l) in ["A", "B", "C", "D"].iter().enumerate() {
            let b = SRC_FILT + s as u32 * 16;
            let leak = |x: String| -> &'static str { Box::leak(x.into_boxed_str()) };
            for k in 0..3u32 {
                v.push(Param::new(b + k, leak(format!("Filter {} On {l}", k + 1)), 0.0, 1.0, if k == 0 { 1.0 } else { 0.0 }, Unit::OnOff));
                v.push(Param::new(b + 5 + k, leak(format!("Filter {} Drive {l}", k + 1)), 0.0, 1.0, 0.0, Unit::Ratio));
            }
            for k in 1..3u32 {
                v.push(Param::menu(b + 2 + k, leak(format!("Filter {} Type {l}", k + 1)), SRC_FILTERS, crate::alchemy_filter::LP12_SMOOTH as f32));
            }
            v.push(Param::new(b + 8, leak(format!("Filter Par {l}")), 0.0, 1.0, 0.0, Unit::OnOff));
        }
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
                m_on: true,
                f2_type: 0,
                m_fx: 1.0,
                f2_fx: 1.0,
                m_drive: 0.0,
                f2_on: false,
                f2_cut: 1.0,
                f2_res: 0.0,
                f2_drive: 0.0,
                par: 0.0,
                env_amt: 0.5,
                amp_env: [0.005, 1.0, 0.7, 0.4],
                filt_env: [0.005, 0.6, 0.3, 0.4],
                vel_sens: 0.85,
                glide: 0.0,
                tune: 0.0,
                lfo1: 0.0,
                lfo2: 0.0,
                character: 0.0,
                vol: 1.0,
                pan: 0.0,
                note_index: 0,
                arp_index: 0,
                speed: 0.0,
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
            mods: Default::default(),
            mg: Default::default(),
            used: Default::default(),
            pf: [0.0; 16],
            amp_from: None,
            bpm: 120.0,
            side: 0.0,
            limiter: true,
        };
        a.room.feedback = 0.82;
        let params = &a.params;
        a.mods.read_all(&|i| params.get(i));
        a.used = crate::alchemy_mod::used(&a.mods);
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
                // the Transform pad's 4 × 2 cells (snapshot 1 at the top left)
                let _ = n;
                let (sx, sy) = ((i % 4) as f32 * 0.25 + 0.125, 1.0 - ((i / 4) % 2) as f32 * 0.5 - 0.25);
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

    /// The voices' settings from parameter values `p` (the knobs, or the knobs as modulated).
    fn build(p: &dyn Fn(u32) -> f32, pf: &[f32; 16], defaults: &[f32; 16], rate: f32, amp_from: Option<f32>) -> AlchShared {
        let d = |k: usize| pf[k] - defaults[k];
        let mut sh = AlchShared {
            rate,
            src: [SourceCfg::default(); 4],
            m_cut: 0.0,
            m_res: 0.0,
            m_type: 0,
            m_on: true,
            f2_type: 0,
            m_fx: 1.0,
            f2_fx: 1.0,
            m_drive: 0.0,
            f2_on: false,
            f2_cut: 1.0,
            f2_res: 0.0,
            f2_drive: 0.0,
            par: 0.0,
            env_amt: 0.0,
            amp_env: [0.0; 4],
            filt_env: [0.0; 4],
            vel_sens: 0.85,
            glide: 0.0,
            tune: 0.0,
            lfo1: 0.0,
            lfo2: 0.0,
            character: 0.0,
            vol: 1.0,
            pan: 0.0,
            note_index: 0,
            arp_index: 0,
            speed: 0.0,
        };
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
            sh.src[s] = SourceCfg {
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
                keytrack: true,
                sf: [0usize, 1, 2].map(|k| {
                    let b = SRC_FILT + s as u32 * 16;
                    let ty = if k == 0 { p(333 + s as u32) } else { p(b + 2 + k as u32) };
                    (ty.round().max(0.0) as u8, p(51 + k as u32 * 8 + s as u32), p(55 + k as u32 * 8 + s as u32), p(b + 5 + k as u32), p(b + k as u32) >= 0.5)
                }),
                sf_par: p(SRC_FILT + s as u32 * 16 + 8) >= 0.5,
                route: p(ROUTE_F2 + s as u32),
            };
        }
        sh.character = d(0);
        sh.m_cut = (p(85) + d(1) * 0.5).clamp(0.0, 1.0);
        sh.m_res = p(86);
        sh.m_drive = (p(87) + pf[5] * 0.6).clamp(0.0, 1.0);
        sh.m_on = p(364) >= 0.5;
        sh.m_type = p(365).round().max(0.0) as u8;
        sh.f2_type = p(368).round().max(0.0) as u8;
        sh.m_fx = p(88);
        sh.f2_fx = p(92);
        sh.f2_on = p(367) >= 0.5;
        sh.f2_cut = p(89);
        sh.f2_res = p(90);
        sh.f2_drive = p(91);
        // Par/Ser: 0 % parallel, 100 % series
        sh.par = p(93);
        sh.env_amt = (pf[2] * 0.8).clamp(-1.0, 1.0);
        sh.amp_env = [
            envt(pf[12], 0.001, 8.0),
            envt(pf[13], 0.02, 12.0),
            pf[14],
            envt(pf[15], 0.01, 10.0),
        ];
        sh.filt_env = [
            p(129).max(0.001),
            p(133) * envt(pf[6], 0.1, 10.0),
            p(135),
            p(137).max(0.01),
        ];
        sh.glide = p(98);
        sh.tune = p(96) + p(97) / 100.0;
        sh.vol = p(94) / 0.8;
        sh.pan = p(95);
        if let Some(r) = amp_from {
            sh.amp_env = [0.001, 0.001, 1.0, r.max(0.005)];
        }
        sh
    }

    fn update(&mut self) {
        for k in 0..16 {
            self.pf[k] = self.perform(k);
        }
        self.mg.perform = self.pf;
        let params = &self.params;
        let amp = self.amp_from.map(|i| self.mods.ahd[i].r);
        let lfo = (self.sh.lfo1, self.sh.lfo2);
        let ctr = (self.sh.note_index, self.sh.arp_index, self.sh.speed);
        self.sh = Self::build(&|i| params.get(i), &self.pf, &self.defaults, self.rate, amp);
        (self.sh.lfo1, self.sh.lfo2) = lfo;
        (self.sh.note_index, self.sh.arp_index, self.sh.speed) = ctr;
        let p = |id: u32| self.params.get(id);
        self.base_cut = self.sh.m_cut;
        self.base_tune = self.sh.tune;
        self.base_sh = self.sh;
        let params = &self.params;
        self.extra.read(&|i| params.get(i));
        self.poly.set_max(p(376).round() as usize);
        self.ch.bend_up = p(391);
        self.ch.bend_down = p(396);
        self.ch.update_bend();
        self.limiter = p(LIMITER) >= 0.5;
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

    /// The modulation section over one block: the instrument-wide modulators move the shared
    /// settings, each voice's own modulators (and every route, through the voice) its own.
    fn run_mods(&mut self, dt: f32) {
        use crate::alchemy_mod as am;
        let b = am::Block { dt, bpm: self.bpm, bend: self.ch.ctl.bend / self.ch.bend_up.max(self.ch.bend_down).max(1.0), wheel: self.ch.ctl.mod_wheel, aftertouch: self.ch.ctl.aftertouch, glide: self.sh.glide };
        self.mg.voices = self.poly.voices.iter().filter(|v| v.on && !v.killing).count();
        self.mg.max_voices = self.poly.max;
        self.mg.step(&self.mods, &b, &self.used);
        let params = &self.params;
        let range = |id: u32| params.def(id).map(|p| (p.min, p.max)).unwrap_or((0.0, 1.0));
        let amp = self.amp_from.map(|i| self.mods.ahd[i].r);
        // instrument-wide
        let mut ov = std::mem::take(&mut self.mg.ov);
        let mut sm = self.mg.smooth;
        am::build_overlay(&self.mods, None, &self.mg, &b, &mut sm, &mut ov, &range);
        self.mg.smooth = sm;
        if ov.n > 0 {
            let keep = (self.sh.lfo1, self.sh.lfo2, self.sh.note_index, self.sh.arp_index, self.sh.speed);
            self.sh = Self::build(&|id| ov.apply(id, params.get(id), range(id).0, range(id).1), &self.pf, &self.defaults, self.rate, amp);
            (self.sh.lfo1, self.sh.lfo2, self.sh.note_index, self.sh.arp_index, self.sh.speed) = keep;
        }
        self.mg.ov = ov;
        // per voice
        let (mods, mg, used, pf, defaults, rate) = (&self.mods, &self.mg, &self.used, &self.pf, &self.defaults, self.rate);
        let base = self.sh;
        for v in self.poly.voices.iter_mut() {
            if !v.on {
                v.own = None;
                continue;
            }
            if v.mv.fresh {
                v.mv.init(mods, mg, v.legato);
            }
            if !used.any_voice {
                v.own = None;
                continue;
            }
            v.mv.step(mods, &b, used);
            let mut sm = v.mv.smooth;
            am::build_overlay(mods, Some(&v.mv), mg, &b, &mut sm, &mut v.ov, &range);
            v.mv.smooth = sm;
            if v.ov.n == 0 {
                v.own = None;
                continue;
            }
            let ov = &v.ov;
            let mut o = Self::build(&|id| ov.apply(id, params.get(id), range(id).0, range(id).1), pf, defaults, rate, amp);
            (o.lfo1, o.lfo2, o.note_index, o.arp_index, o.speed) = (base.lfo1, base.lfo2, base.note_index, base.arp_index, base.speed);
            v.own = Some(o);
        }
    }

    /// What's in use after the modulation settings changed.
    fn mods_changed(&mut self) {
        use crate::alchemy_mod as am;
        self.used = am::used(&self.mods);
        self.amp_from = self.mods.routes[..self.mods.nroutes].iter().find(|r| r.target == 94 && r.kind == am::K_AHD && self.mods.per_voice(r.kind, r.idx)).map(|r| r.idx.min(am::N - 1));
    }

    /// The parameter's range (for modulation amounts).
    pub fn range(&self, id: u32) -> (f32, f32) {
        self.params.def(id).map(|p| (p.min, p.max)).unwrap_or((0.0, 1.0))
    }

    /// Before a note starts: the counters the Note Property sources read.
    fn count_note(&mut self) {
        let g = &mut self.mg;
        let gap = g.time - g.last_note_t;
        g.last_note_t = g.time;
        self.sh.note_index = g.note_count;
        self.sh.arp_index = g.arp_count;
        self.sh.speed = (gap / 2.0).clamp(0.0, 1.0);
        g.note_count = g.note_count.wrapping_add(1);
        g.arp_count = g.arp_count.wrapping_add(1);
        if self.mg.held == 0 {
            self.mg.first_note(&self.mods);
        }
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
            let params = &self.params;
            if self.mods.read_changed(id, &|i| params.get(i)) {
                self.mods_changed();
                // the knobs that are both a modulator's and the voices' (LFO 1–2, AHDSR 1–2)
                if !(100..=137).contains(&id) && !(crate::alchemy_mod::ROUTE..=crate::alchemy_mod::CTRL_CC + 4).contains(&id) && !(crate::alchemy_mod::AHD..crate::alchemy_mod::MSEG).contains(&id) {
                    return;
                }
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
        if self.extra.arp_set.on {
            if self.extra.held == 0 {
                self.mg.arp_count = 0;
            }
            self.extra.held += 1;
            let s = self.extra.arp_set;
            self.extra.arp.note_on(&s, pitch, velocity);
            return;
        }
        self.count_note();
        self.extra.held += 1;
        self.mg.held = self.extra.held;
        self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
    }
    fn note_off(&mut self, pitch: u8) {
        self.extra.held = self.extra.held.saturating_sub(1);
        if self.extra.held == 0 && self.mg.held > 0 {
            self.mg.held = 0;
            self.mg.all_released();
        }
        if self.extra.arp_set.on {
            let s = self.extra.arp_set;
            self.extra.arp.note_off(&s, pitch);
            return;
        }
        self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.extra.set_tempo(bpm);
        self.bpm = bpm as f32;
    }
    fn set_transport(&mut self, t: lg_dsp::Transport) {
        if t.playing {
            self.mg.beats = t.beats;
        }
        self.bpm = t.bpm as f32;
    }
    fn side_chain(&mut self, left: &[f32], right: &[f32]) {
        self.side = left.iter().chain(right).fold(0.0f32, |a, &b| a.max(b.abs()));
    }
    fn pressure(&mut self, value: u8) {
        self.ch.ctl.aftertouch = value as f32 / 127.0;
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
        self.mg.cc[(cc as usize).min(127)] = value as f32 / 127.0;
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
        let vol = lg_dsp::db_to_gain(self.p(99));
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
                        crate::arp::ArpEvent::On(p, v) => {
                            if self.extra.arp.current_step() == 0 {
                                self.mg.arp_count = 0;
                            }
                            let held = self.mg.held;
                            self.count_note();
                            self.mg.held = held.max(1);
                            self.poly.note_on(p, v, &self.sh, &self.ch.ctl)
                        }
                        crate::arp::ArpEvent::Off(p) => self.poly.note_off(p, &self.sh, &self.ch.ctl),
                    }
                }
            }
            // the MSEG and the modulation rack move the settings from where the knobs put them
            let mseg = self.extra.mseg_level();
            if self.extra.mseg_on || self.extra.nmods > 0 || self.used.any {
                let (l1, l2) = (self.sh.lfo1, self.sh.lfo2);
                let ctr = (self.sh.note_index, self.sh.arp_index, self.sh.speed);
                self.sh = self.base_sh;
                self.sh.lfo1 = l1;
                self.sh.lfo2 = l2;
                (self.sh.note_index, self.sh.arp_index, self.sh.speed) = ctr;
            }
            if self.used.any {
                self.run_mods(dt);
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
            // what the main filters send past the effects
            let mut drl = [0.0f32; CHUNK];
            let mut drr = [0.0f32; CHUNK];
            let mut any_dry = false;
            for v in self.poly.voices.iter_mut() {
                if v.dry_n == m {
                    for i in 0..m {
                        drl[i] += v.dry[i].0;
                        drr[i] += v.dry[i].1;
                    }
                    any_dry = true;
                }
                v.dry_n = 0;
            }
            if self.used.envf != 0 {
                let mut lv = [0.0f32; 6];
                for v in self.poly.voices.iter().filter(|v| v.on) {
                    for k in 0..6 {
                        lv[k] += v.lv[k];
                    }
                }
                self.mg.levels[..6].copy_from_slice(&lv);
                self.mg.levels[6] = sl[..m].iter().chain(&sr[..m]).fold(0.0f32, |a, &b| a.max(b.abs()));
                self.mg.levels[8] = self.side;
            }
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
            if self.used.envf != 0 {
                self.mg.levels[7] = sl[..m].iter().chain(&sr[..m]).fold(0.0f32, |a, &b| a.max(b.abs()));
            }
            if any_dry {
                for i in 0..m {
                    sl[i] += drl[i];
                    sr[i] += drr[i];
                }
            }
            if self.limiter {
                for i in 0..m {
                    sl[i] = dsp::soft_clip(sl[i] * vol) / vol.max(1e-6);
                    sr[i] = dsp::soft_clip(sr[i] * vol) / vol.max(1e-6);
                }
            }
            for i in 0..m {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += m;
        }
    }
}
