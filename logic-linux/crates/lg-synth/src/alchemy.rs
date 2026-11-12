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
/// The master voice section's Trigger Mode (Logic's PlayMode, id 371).
pub const PLAY_MODES: &[&str] = &["Always", "Retrigger", "Legato"];
/// Master voice section: VOICE + (0 Voice Priority, 1 Glide Rate/Time, 2 Wide Unison).
pub const VOICE: u32 = 14960;
/// The VA element's noise oscillator per source: NOISE + s (type), + 4 + s (Vol),
/// + 8 + s (Low Cut), + 12 + s (High Cut).
pub const NOISE: u32 = 14964;
pub const PRIORITIES: &[&str] = &["Newest", "Oldest", "Lowest", "Highest"];
/// The Transform pad: snapshot k's (0–7) Perform value i (0–15) is SNAPV + k × 16 + i; its
/// Snap Vol (dB) SNAPVOL + k; then Snapshot Update, the keyswitch Octave, Rate (seconds, 0
/// Off) and the ModWheel target.
pub const SNAPV: u32 = 31000;
pub const SNAPVOL: u32 = 31128;
pub const SNAP_UPDATE: u32 = 31140;
pub const SNAP_OCTAVE: u32 = 31141;
pub const SNAP_RATE: u32 = 31142;
pub const SNAP_WHEEL: u32 = 31143;
/// Lock Control per Perform control (16): the Transform pad leaves it alone.
pub const SNAP_LOCK: u32 = 31150;
pub const SNAP_OCTAVES: &[&str] = &["Off", "C-2", "C-1", "C0", "C1", "C2", "C3", "C4", "C5", "C6", "C7", "C8"];
/// ModWheel pop-up: None, the 16 Perform controls, the same inverted.
pub fn wheel_targets() -> &'static [&'static str] {
    static T: OnceLock<Vec<&'static str>> = OnceLock::new();
    T.get_or_init(|| {
        let names = ["Control 1", "Control 2", "Control 3", "Control 4", "Control 5", "Control 6", "Control 7", "Control 8", "X1", "Y1", "X2", "Y2", "Attack", "Decay", "Sustain", "Release"];
        let mut v = vec!["None"];
        v.extend(names);
        for n in names {
            v.push(Box::leak(format!("{n} Inv").into_boxed_str()));
        }
        v
    })
}
pub const fn snapv(k: usize, i: usize) -> u32 {
    SNAPV + (k * 16 + i) as u32
}
/// The snapshot cell centres: snapshot 1 at the top left of the 4 × 2 pad.
pub fn snapshot_centre(k: usize) -> (f32, f32) {
    ((k % 4) as f32 * 0.25 + 0.125, 0.75 - (k / 4) as f32 * 0.5)
}
/// The selected snapshot: the one whose cell holds the pad position.
pub fn nearest_snapshot(x: f32, y: f32) -> usize {
    let col = ((x * 4.0).floor() as i32).clamp(0, 3) as usize;
    let row = if y >= 0.5 { 0 } else { 1 };
    row * 4 + col
}
/// Each snapshot's weight at pad position (x, y) (they sum to 1).
pub fn transform_weights(x: f32, y: f32) -> [f32; 8] {
    let mut w = [0.0f32; 8];
    let mut sum = 0.0;
    for (k, wk) in w.iter_mut().enumerate() {
        let (sx, sy) = snapshot_centre(k);
        let d2 = (x - sx).powi(2) + (y - sy).powi(2);
        *wk = 1.0 / (d2 * d2 + 1e-7);
        sum += *wk;
    }
    for wk in &mut w {
        *wk /= sum;
    }
    w
}
pub const GLIDE_MODES: &[&str] = &["Rate", "Time"];
/// Extended parameters: MIDI Mono Mode (MPE: a voice per channel) and its Bend Range.
pub const MONO_MODE: u32 = 14990;
pub const MONO_BEND: u32 = 14991;
pub const MONO_MODES: &[&str] = &["Off", "On (with common base channel 1)", "On (with common base channel 16)"];
pub const NOISE_TYPES: &[&str] = &["White", "Pink", "Brown"];
/// Noise Low Cut / High Cut knob (0–1) → Hz (20 Hz – 20 kHz).
pub fn noise_cut_hz(x: f32) -> f32 {
    20.0 * 1000f32.powf(x.clamp(0.0, 1.0))
}

include!("alchemy_params.rs");

const NP: usize = crate::alchemy_partials::NP; // partials
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
    /// VA Phase: a fixed start point (0–99.9 %), or random on each trigger (100 %).
    va_phase: f32,
    /// The noise oscillator: type (White, Pink, Brown), level and its band (Hz).
    noise_type: u8,
    noise_vol: f32,
    noise_lo: f32,
    noise_hi: f32,
    keytrack: bool,
    /// The three source filters: (type, cutoff knob, res knob, drive knob, on); parallel or not.
    sf: [(u8, f32, f32, f32, bool); 3],
    sf_par: bool,
    /// Keyscale (0 Key+PBend, 1 Key, 2 Off) and Wait (seconds before the source starts).
    keyscale: u8,
    wait: f32,
    /// The Send pop-up: 0 F1/F2, 1–4 F1 and FX A–D, 5–8 F2 and FX A–D.
    send: u8,
    /// The additive / spectral / formant settings (alchemy_partials).
    part: crate::alchemy_partials::PartCfg,
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
    /// Which effects rack each main filter's FX share goes to (0 Main, 1–4 A–D).
    m_rack: u8,
    f2_rack: u8,
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
    /// Glide Time (a fixed time whatever the interval) rather than Rate (a fixed speed).
    glide_time: bool,
    /// Wide unison mode (VA unison oscillators hard panned, detune up to ±2 semitones).
    wide: bool,
    /// Set per note by the instrument: restart the envelopes / sources (else play legato), and
    /// the pitch to glide from (NaN: no glide).
    retrig: bool,
    from: f32,
    /// The MIDI channel of the note being started (MIDI Mono Mode).
    chan: u8,
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
    /// The arpeggiator step's pan, Arp Mod values and playhead for the next note.
    note_pan: f32,
    arp_mod: [f32; 2],
    playhead: f32,
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
    osc: [Osc; 8],
    sync: Osc,
    ph: [f32; NP],
    jit: [f32; NP],
    grains: [Grain; 12],
    gpos: f32,
    gnext: f32,
    sf: [[crate::alchemy_filter::AFilt; 2]; 3],
    nf: Svf,
    ps: crate::alchemy_partials::PartState,
    /// The noise oscillator: pink (Kellet) / brown filter state, its Low Cut and High Cut.
    pink: [f32; 3],
    brown: f32,
    nlo: [dsp::OnePole; 2],
    nhi: [dsp::OnePole; 2],
}

impl Default for SrcState {
    fn default() -> Self {
        SrcState {
            osc: [Osc::default(); 8],
            sync: Osc::default(),
            ph: [0.0; NP],
            jit: [1.0; NP],
            grains: [Grain::default(); 12],
            gpos: 0.0,
            gnext: 0.0,
            sf: Default::default(),
            nf: Svf::default(),
            ps: Default::default(),
            pink: [0.0; 3],
            brown: 0.0,
            nlo: Default::default(),
            nhi: Default::default(),
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
    /// What goes to effects racks A–D this block (after the voice's level), and before it.
    pub(crate) rk: [[(f32, f32); CHUNK]; 4],
    pub(crate) rk_used: [bool; 4],
    rk_raw: [[(f32, f32); CHUNK]; 4],
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
    /// Glide speed in semitones per second (infinite: no glide).
    gspeed: f32,
    /// MIDI Mono Mode: the voice's channel and its own pitch bend (semitones).
    chan: u8,
    mpe_bend: f32,
    /// Seconds since the note started.
    age: f32,
    /// The arpeggiator step's pan for this note.
    npan: f32,
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
            rk: [[(0.0, 0.0); CHUNK]; 4],
            rk_used: [false; 4],
            rk_raw: [[(0.0, 0.0); CHUNK]; 4],
            rng: Rng::new(seed),
            fade: Fade::default(),
            killing: false,
            level: 0.0,
            sample: None,
            mix: [(0.0, 0.0); CHUNK],
            mix2: [(0.0, 0.0); CHUNK],
            mv: Default::default(),
            legato: false,
            gspeed: f32::INFINITY,
            chan: 0,
            mpe_bend: 0.0,
            age: 0.0,
            npan: 0.0,
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
        // a voice moved to a new note without a trigger plays on (legato): its envelopes,
        // sources and modulators carry on
        self.legato = was && !sh.retrig;
        self.chan = sh.chan;
        if !self.legato {
            self.mpe_bend = 0.0;
        }
        if sh.from.is_finite() && sh.glide > 0.0 {
            if !self.legato {
                self.pitch = sh.from;
            }
            // Rate: a fixed speed (an octave per Glide time), so wider intervals take longer;
            // Time: the same time whatever the interval
            let dist = (self.target - self.pitch).abs();
            self.gspeed = if sh.glide_time { dist.max(1e-3) / sh.glide } else { 12.0 / sh.glide };
        } else {
            self.pitch = self.target;
            self.gspeed = f32::INFINITY;
        }
        if !self.legato {
            self.age = 0.0;
        }
        self.mv.fresh = true;
        self.mv.note = note;
        self.mv.vel = self.vel;
        self.mv.index = sh.note_index;
        self.mv.arp_index = sh.arp_index;
        self.mv.speed = sh.speed;
        self.mv.arp_mod = sh.arp_mod;
        self.mv.playhead = sh.playhead;
        self.npan = sh.note_pan;
        if !self.legato {
            self.mv.key = note as f32;
        }
        if !self.legato {
            for (i, s) in self.src.iter_mut().enumerate() {
                // VA Phase: 0–99.9 % a fixed start point, 100 % random on each trigger
                let ph = sh.src[i].va_phase;
                for o in &mut s.osc {
                    o.phase = if ph >= 0.999 { self.rng.unit() } else { ph };
                }
                for k in 0..NP {
                    s.ph[k] = self.rng.unit();
                    s.jit[k] = 1.0;
                }
                s.grains = [Grain::default(); 12];
                if !was {
                    s.ps.start(self.rng.next_u32());
                }
                s.gpos = sh.src[i].position;
                s.gnext = 0.0;
                if !was {
                    for f in s.sf.iter_mut().flatten() {
                        f.reset();
                    }
                }
                s.nf = Svf::new(SvfMode::Band, 1200.0, 0.8, rate);
            }
        }
        let [a, d, su, r] = sh.amp_env;
        self.amp.set(a, d, su, r, rate);
        let [a, d, su, r] = sh.filt_env;
        self.fenv.set(a, d, su, r, rate);
        if self.legato {
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
        // glide: linear in semitones, at the speed the note started with
        let step = self.gspeed * l.len() as f32 / rate;
        if (self.target - self.pitch).abs() <= step || !step.is_finite() {
            self.pitch = self.target;
        } else {
            self.pitch += step.copysign(self.target - self.pitch);
        }
        let vib = sh.lfo1 * ctl.mod_wheel * 0.4;
        let base = self.pitch + sh.tune + ctl.bend + self.mpe_bend + vib;
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
            // Wait: the source starts a while after the key
            if c.kind == Kind::Off || c.vol <= 0.0 || self.age < c.wait {
                continue;
            }
            // Keyscale: Key+PBend follows notes and bend, Key only notes, Off neither (C3)
            let hz = lg_dsp::midi_hz(match c.keyscale {
                1 => base - ctl.bend,
                2 => 60.0 + sh.tune,
                _ => base,
            } + c.semis);
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
                    let nos = c.va_num.clamp(1, 8);
                    let wave = match c.va_shape {
                        0 => Wave::Sine,
                        1 => Wave::Pulse,
                        3 => Wave::Triangle,
                        _ => Wave::Saw,
                    };
                    let g = vol / (nos as f32).sqrt();
                    // the source Pan: a pan for one oscillator, a balance for a unison stack
                    let (bl_g, br_g) = if nos == 1 {
                        (pl, pr)
                    } else {
                        ((1.0 - c.pan).min(1.0), (1.0 + c.pan).min(1.0))
                    };
                    for i in 0..n {
                        let mut sl = 0.0;
                        let mut sr = 0.0;
                        for u in 0..nos {
                            let (d, p) = unison_spread(sh.wide, u, nos, c.va_detune);
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
                            sl += x * (1.0 - p).sqrt();
                            sr += x * (1.0 + p).sqrt();
                        }
                        bl[i] = sl * g * bl_g;
                        br[i] = sr * g * br_g;
                    }
                }
                Kind::Additive | Kind::Spectral => {
                    let pos = (c.position + sh.character * 0.5).clamp(0.0, 1.0);
                    let (a0, a1, t) = spec_at(pos);
                    let spectral = c.kind == Kind::Spectral;
                    // the content's partials (the bank morph), through the partial engine
                    let mut ba = [0.0f32; NP];
                    let mut bq = [0.0f32; NP];
                    for k in 0..NP {
                        bq[k] = b.ratio[a0][k] * (1.0 - t) + b.ratio[a1][k] * t;
                        let mut a = b.amp[a0][k] * (1.0 - t) + b.amp[a1][k] * t;
                        if spectral {
                            // slowly wandering partial levels: the "alive" resynthesised sound
                            st.jit[k] += (self.rng.unit() * 2.0 - st.jit[k]) * (0.02 + 0.2 * c.speed);
                            a *= st.jit[k];
                        }
                        ba[k] = a;
                    }
                    let mut pa = [0.0f32; NP];
                    let mut pf = [0.0f32; NP];
                    let mut pp = [0.0f32; NP];
                    let used = crate::alchemy_partials::process(&c.part, spectral, &ba, &bq, hz, n as f32 / rate, rate * 0.45, &mut st.ps, &mut pa, &mut pf, &mut pp);
                    if st.ps.lock_phase && st.ps.t <= n as f32 / rate * 1.5 {
                        st.ph = [0.0; NP];
                    }
                    let noise_mode = spectral && c.part.noise_mode;
                    let mut amps = [0.0f32; NP];
                    let mut gr = [0.0f32; NP];
                    let mut incs = [0.0f32; NP];
                    let mut sel = [0usize; NP];
                    let mut shaped = [false; NP];
                    let mut np = 0;
                    for k in 0..used {
                        if pa[k] > 2e-4 {
                            // Noise: each bin's level jumps about (filtered-noise-like bands)
                            let nz = if noise_mode { 0.2 + 1.6 * self.rng.unit() } else { 1.0 };
                            let pn = (c.pan + pp[k]).clamp(-1.0, 1.0);
                            amps[np] = pa[k] * vol * nz * (1.0 - pn).sqrt();
                            gr[np] = pa[k] * vol * nz * (1.0 + pn).sqrt();
                            incs[np] = pf[k] / rate;
                            shaped[np] = (c.part.complex || (c.part.sym - 0.5).abs() > 1e-3) && pf[k] * 8.0 < rate * 0.45;
                            sel[np] = k;
                            np += 1;
                        }
                    }
                    let (shape, sym) = (if c.part.complex { c.part.shape } else { 0 }, c.part.sym);
                    st.nf.set((800.0 + 2500.0 * pos).min(rate * 0.4), 0.8, rate);
                    let noise_amt = if spectral { 0.15 * vol } else { 0.0 };
                    for i in 0..n {
                        let (mut xl, mut xr) = (0.0, 0.0);
                        for j in 0..np {
                            let k = sel[j];
                            let w = if shaped[j] { crate::alchemy_partials::shape_at(shape, st.ph[k], sym) } else { dsp::sin_cycles(st.ph[k]) };
                            xl += amps[j] * w;
                            xr += gr[j] * w;
                            let ph = st.ph[k] + incs[j];
                            st.ph[k] = if ph >= 1.0 { ph - 1.0 } else { ph };
                        }
                        if noise_amt > 0.0 {
                            let z = st.nf.run(self.rng.bipolar()) * noise_amt;
                            xl += z * pl;
                            xr += z * pr;
                        }
                        bl[i] = xl;
                        br[i] = xr;
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
            if c.noise && c.noise_vol > 0.0 {
                // the noise oscillator: white, pink or brown, through its Low Cut and High Cut
                // (together a band-pass)
                let (lo, hi) = (c.noise_lo > 20.5, c.noise_hi < 19_900.0);
                if lo {
                    for f in &mut st.nlo {
                        f.lowpass_hz(c.noise_lo, rate);
                    }
                }
                if hi {
                    for f in &mut st.nhi {
                        f.lowpass_hz(c.noise_hi, rate);
                    }
                }
                let g = 0.1 * vol * c.noise_vol;
                for i in 0..n {
                    let w = self.rng.bipolar();
                    let mut x = match c.noise_type {
                        1 => {
                            let b = &mut st.pink;
                            b[0] = 0.99765 * b[0] + w * 0.099_046;
                            b[1] = 0.963 * b[1] + w * 0.296_516_4;
                            b[2] = 0.57 * b[2] + w * 1.052_691_3;
                            (b[0] + b[1] + b[2] + w * 0.1848) * 0.25
                        }
                        2 => {
                            st.brown = (st.brown + 0.02 * w) / 1.02;
                            st.brown * 3.5
                        }
                        _ => w,
                    };
                    if lo {
                        x = st.nlo[0].hp(x);
                        x = st.nlo[1].hp(x);
                    }
                    if hi {
                        x = st.nhi[0].lp(x);
                        x = st.nhi[1].lp(x);
                    }
                    bl[i] += x * g * pl;
                    br[i] += x * g * pr;
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
            // the Send: F1/F2 balance, or a main filter and an effects rack (bypassing the filters)
            let k = c.route.clamp(0.0, 1.0);
            let (w1, w2, rack) = match c.send {
                0 => (1.0 - k, k, None),
                1..=4 => (1.0 - k, 0.0, Some(c.send as usize - 1)),
                _ => (0.0, 1.0 - k, Some(c.send as usize - 5)),
            };
            for i in 0..n {
                pk = pk.max(bl[i].abs()).max(br[i].abs());
                if w1 > 0.0 {
                    self_mix_add(&mut self.mix, i, bl[i] * w1, br[i] * w1);
                }
                if w2 > 0.0 {
                    self_mix_add(&mut self.mix2, i, bl[i] * w2, br[i] * w2);
                }
            }
            if let Some(rk) = rack.filter(|_| k > 0.0) {
                for i in 0..n {
                    self_mix_add(&mut self.rk_raw[rk], i, bl[i] * k, br[i] * k);
                }
                self.rk_used[rk] = true;
            }
            self.lv[si] = pk;
        }
        let vel_g = 1.0 - sh.vel_sens + sh.vel_sens * self.vel * self.vel;
        let mut peak = 0.0f32;
        let (mut f1p, mut f2p) = (0.0f32, 0.0f32);
        let pan = (sh.pan + self.npan).clamp(-1.0, 1.0);
        let (pl, pr) = ((1.0 - pan).sqrt() * std::f32::consts::SQRT_2, (1.0 + pan).sqrt() * std::f32::consts::SQRT_2);
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
            // each filter's FX share goes to its rack (Main: the voice's output)
            xl = 0.0;
            xr = 0.0;
            for (rk, el, er) in [(sh.m_rack, e1l, e1r), (sh.f2_rack, e2l, e2r)] {
                if rk == 0 {
                    xl += el;
                    xr += er;
                } else {
                    self_mix_add(&mut self.rk_raw[rk as usize - 1], i, el, er);
                    self.rk_used[rk as usize - 1] = true;
                }
            }
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
            for k in 0..4 {
                if self.rk_used[k] {
                    let (a, b) = self.rk_raw[k][i];
                    self.rk[k][i] = (a * g * pl, b * g * pr);
                    self.rk_raw[k][i] = (0.0, 0.0);
                }
            }
            peak = peak.max(yl.abs()).max(self.dry[i].0.abs());
            l[i] += yl;
            r[i] += yr;
        }
        self.dry_n = n;
        self.level = peak;
        self.age += n as f32 / rate;
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

/// A VA unison oscillator's detune (semitones) and pan (-1 … 1), oscillator `u` of `n`.
/// Wide unison: 1 centred, 2 hard left / right, 3 centre + hard left / right, 4 or more
/// alternately hard left and right (an odd one centred), detune spread linearly up to ±2
/// semitones at 100 % (±1 at 50 %). The older mode: ±0.3 semitones, spread across the field.
pub fn unison_spread(wide: bool, u: usize, n: usize, detune: f32) -> (f32, f32) {
    if n <= 1 {
        return (0.0, 0.0);
    }
    let lin = u as f32 / (n - 1) as f32 * 2.0 - 1.0;
    if !wide {
        return (lin * 0.5 * detune * 0.6, lin * 0.95);
    }
    let pan = if n % 2 == 1 {
        if u == 0 { 0.0 } else if u % 2 == 1 { -1.0 } else { 1.0 }
    } else if u % 2 == 0 {
        -1.0
    } else {
        1.0
    };
    (lin * 2.0 * detune, pan)
}

#[inline]
fn self_mix_add(mix: &mut [(f32, f32); CHUNK], i: usize, a: f32, b: f32) {
    mix[i].0 += a;
    mix[i].1 += b;
}

/// Morph section: MORPH + (0 mode, 1 Elements, 2 group, 3 Auto Gain, 4 Fixed Pitch, 5 Auto Align,
/// 6–7 Pitch X / Y); Morph All X / Y are Logic's ids 75–76, the element knobs 77–84.
pub const MORPH: u32 = 14800;
/// Each source's Keyscale and Wait.
pub const KEYSCALE: u32 = 14810;
pub const WAIT: u32 = 14814;
pub const KEYSCALES: &[&str] = &["Key+PBend", "Key", "Off"];
pub const MORPH_MODES: &[&str] = &["Off", "XFade XY", "XFade Linear", "Morph XY", "Morph Linear"];
pub const MORPH_GROUPS: &[&str] = &["A-B", "C-D", "A-B-C-D"];

/// The four sources' weights at (x, y) for a morph mode and group.
pub fn morph_weights(mode: u8, group: u8, x: f32, y: f32) -> [f32; 4] {
    let (x, y) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
    match mode {
        1 | 3 => [(1.0 - x) * (1.0 - y), x * (1.0 - y), (1.0 - x) * y, x * y],
        2 | 4 => {
            let g = if mode == 2 { 2 } else { group };
            match g {
                0 => [1.0 - x, x, 0.0, 0.0],
                1 => [0.0, 0.0, 1.0 - x, x],
                _ => {
                    let t = x * 3.0;
                    let k = (t.floor() as usize).min(2);
                    let f = t - k as f32;
                    let mut w = [0.0; 4];
                    w[k] = 1.0 - f;
                    w[k + 1] = f;
                    w
                }
            }
        }
        _ => [1.0; 4],
    }
}

/// Crossfades the sources' levels, or morphs them into one source (interpolated settings, per
/// element when Elements is on: Additive/VA, Spectral/Granular, Pitch, Formant).
fn morph(sh: &mut AlchShared, p: &dyn Fn(u32) -> f32) {
    let mode = p(MORPH).round().clamp(0.0, 4.0) as u8;
    if mode == 0 {
        return;
    }
    let group = p(MORPH + 2).round().clamp(0.0, 2.0) as u8;
    let on: [bool; 4] = [0, 1, 2, 3].map(|s| sh.src[s].kind != Kind::Off);
    let norm = |mut w: [f32; 4]| {
        for s in 0..4 {
            if !on[s] {
                w[s] = 0.0;
            }
        }
        let t: f32 = w.iter().sum();
        if t > 1e-6 {
            for x in &mut w {
                *x /= t;
            }
        }
        w
    };
    let all = morph_weights(mode, group, p(75), p(76));
    if mode <= 2 {
        let w = if p(MORPH + 3) >= 0.5 { norm(all) } else { all };
        for s in 0..4 {
            sh.src[s].vol *= w[s];
        }
        return;
    }
    let elements = p(MORPH + 1) >= 0.5;
    let wsel = |xid: u32, yid: u32| if elements { norm(morph_weights(mode, group, p(xid), p(yid))) } else { norm(all) };
    let (w_add, w_spec, w_pitch, w_form) = (wsel(79, 80), wsel(81, 82), wsel(MORPH + 6, MORPH + 7), wsel(83, 84));
    let w_all = norm(all);
    let dom = (0..4).max_by(|&a, &b| w_all[a].total_cmp(&w_all[b])).unwrap_or(0);
    if !on[dom] {
        return;
    }
    let src = sh.src;
    let mix = |w: &[f32; 4], f: &dyn Fn(&SourceCfg) -> f32| -> f32 { (0..4).map(|s| w[s] * f(&src[s])).sum() };
    let mut m = src[dom];
    let w_el = match m.kind {
        Kind::Spectral | Kind::Granular => w_spec,
        _ => w_add,
    };
    m.vol = mix(&w_all, &|c| c.vol);
    m.pan = mix(&w_all, &|c| c.pan);
    // pitch glides between the sources' tunings (Fixed Pitch keeps the root)
    m.semis = if p(MORPH + 4) >= 0.5 { 0.0 } else { mix(&w_pitch, &|c| c.semis) };
    m.position = mix(&w_el, &|c| c.position);
    m.speed = mix(&w_el, &|c| c.speed);
    m.density = mix(&w_el, &|c| c.density);
    m.va_sym = mix(&w_add, &|c| c.va_sym);
    m.va_detune = mix(&w_add, &|c| c.va_detune);
    m.va_sync = mix(&w_add, &|c| c.va_sync);
    m.va_num = mix(&w_add, &|c| c.va_num as f32).round().max(1.0) as usize;
    m.route = mix(&w_all, &|c| c.route);
    // the formant knob also morphs the source filters' knobs
    for k in 0..3 {
        m.sf[k].1 = mix(&w_form, &|c| c.sf[k].1);
        m.sf[k].2 = mix(&w_form, &|c| c.sf[k].2);
        m.sf[k].3 = mix(&w_form, &|c| c.sf[k].3);
    }
    m.part.fm_shift = mix(&w_form, &|c| c.part.fm_shift);
    m.part.fm_size = mix(&w_form, &|c| c.part.fm_size);
    m.part.fm_select = mix(&w_form, &|c| c.part.fm_select);
    m.part.pvar = mix(&w_add, &|c| c.part.pvar);
    m.part.sym = mix(&w_add, &|c| c.part.sym);
    sh.src = [m, SourceCfg::default(), SourceCfg::default(), SourceCfg::default()];
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
    /// Whether the Transform pad (its eight snapshots, in parameters SNAPV…) drives the Perform
    /// section, and the pad's glide to a keyswitched snapshot (target x, y, seconds left).
    transform: bool,
    pad_to: Option<(f32, f32, f32)>,
    /// Keys that switched snapshots (not played).
    switch_keys: u128,
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
    /// The arpeggiator (alchemy_arp).
    pub arp_cfg: crate::alchemy_arp::ArpCfg,
    pub arp: crate::alchemy_arp::Arp,
    playing: bool,
    /// The effects racks (alchemy_fx).
    pub racks: crate::alchemy_fx::Racks,
    /// The keys down (pitch, velocity; oldest first), the note last played (glides start
    /// there) and, with Num 1, the note sounding.
    keys: [(u8, u8); 32],
    nkeys: usize,
    last: f32,
    sounding: Option<u8>,
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
        v.extend(crate::alchemy_partials::params());
        v.extend(crate::alchemy_arp::params());
        v.extend(crate::alchemy_fx::params());
        v.push(Param::menu(MORPH, "Morph Mode", MORPH_MODES, 0.0));
        for (s, l) in ["A", "B", "C", "D"].iter().enumerate() {
            let leak = |x: String| -> &'static str { Box::leak(x.into_boxed_str()) };
            v.push(Param::menu(KEYSCALE + s as u32, leak(format!("Keyscale {l}")), KEYSCALES, 0.0));
            v.push(Param::new(WAIT + s as u32, leak(format!("Wait {l}")), 0.0, 2.0, 0.0, Unit::Seconds));
        }
        for k in 0..8usize {
            let leak = |x: String| -> &'static str { Box::leak(x.into_boxed_str()) };
            for (i, q) in PARAMS.iter().filter(|q| (1..=16).contains(&q.id)).enumerate() {
                v.push(Param::new(snapv(k, i), leak(format!("Snapshot {} {}", k + 1, q.name)), 0.0, 1.0, q.default, Unit::Ratio).hide());
            }
            v.push(Param::new(SNAPVOL + k as u32, leak(format!("Snap Vol {}", k + 1)), -24.0, 12.0, 0.0, Unit::Db).hide());
        }
        v.push(Param::new(SNAP_UPDATE, "Snapshot Update", 0.0, 1.0, 1.0, Unit::OnOff));
        for (i, q) in PARAMS.iter().filter(|q| (1..=16).contains(&q.id)).enumerate() {
            let leak = |x: String| -> &'static str { Box::leak(x.into_boxed_str()) };
            v.push(Param::new(SNAP_LOCK + i as u32, leak(format!("Lock {}", q.name)), 0.0, 1.0, 0.0, Unit::OnOff).hide());
        }
        v.push(Param::menu(SNAP_OCTAVE, "Transform Octave", SNAP_OCTAVES, 0.0));
        v.push(Param::new(SNAP_RATE, "Transform Rate", 0.0, 5.0, 0.0, Unit::Seconds));
        v.push(Param::menu(SNAP_WHEEL, "ModWheel", wheel_targets(), 0.0));
        v.push(Param::menu(MONO_MODE, "MIDI Mono Mode", MONO_MODES, 0.0));
        v.push(Param::new(MONO_BEND, "Mono Mode Bend Range", 0.0, 96.0, 48.0, Unit::Semitones));
        v.push(Param::menu(VOICE, "Voice Priority", PRIORITIES, 0.0));
        v.push(Param::menu(VOICE + 1, "Glide Mode", GLIDE_MODES, 0.0));
        v.push(Param::new(VOICE + 2, "Wide Unison", 0.0, 1.0, 0.0, Unit::OnOff));
        for (s, l) in ["A", "B", "C", "D"].iter().enumerate() {
            let leak = |x: String| -> &'static str { Box::leak(x.into_boxed_str()) };
            let s = s as u32;
            v.push(Param::menu(NOISE + s, leak(format!("Noise Type {l}")), NOISE_TYPES, 0.0));
            v.push(Param::new(NOISE + 4 + s, leak(format!("Noise Vol {l}")), 0.0, 1.0, 1.0, Unit::Ratio));
            v.push(Param::new(NOISE + 8 + s, leak(format!("Noise Low Cut {l}")), 0.0, 1.0, 0.0, Unit::Ratio));
            v.push(Param::new(NOISE + 12 + s, leak(format!("Noise High Cut {l}")), 0.0, 1.0, 1.0, Unit::Ratio));
        }
        v.push(Param::new(MORPH + 1, "Morph Elements", 0.0, 1.0, 0.0, Unit::OnOff));
        v.push(Param::menu(MORPH + 2, "Morph Group", MORPH_GROUPS, 2.0));
        v.push(Param::new(MORPH + 3, "Auto Gain", 0.0, 1.0, 1.0, Unit::OnOff));
        v.push(Param::new(MORPH + 4, "Fixed Pitch", 0.0, 1.0, 0.0, Unit::OnOff));
        v.push(Param::new(MORPH + 5, "Auto Align", 0.0, 1.0, 1.0, Unit::OnOff));
        v.push(Param::new(MORPH + 6, "Morph Pitch X", 0.0, 1.0, 0.0, Unit::Ratio));
        v.push(Param::new(MORPH + 7, "Morph Pitch Y", 0.0, 1.0, 0.0, Unit::Ratio));
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
                m_rack: 0,
                f2_rack: 0,
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
                glide_time: false,
                wide: false,
                retrig: true,
                from: f32::NAN,
                chan: 0,
                tune: 0.0,
                lfo1: 0.0,
                lfo2: 0.0,
                character: 0.0,
                vol: 1.0,
                pan: 0.0,
                note_index: 0,
                arp_index: 0,
                speed: 0.0,
                note_pan: 0.0,
                arp_mod: [0.0; 2],
                playhead: 0.0,
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
            transform: false,
            pad_to: None,
            switch_keys: 0,
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
            arp_cfg: Default::default(),
            arp: Default::default(),
            playing: false,
            racks: crate::alchemy_fx::Racks::new(rate),
            keys: [(0, 0); 32],
            nkeys: 0,
            last: f32::NAN,
            sounding: None,
        };
        a.room.feedback = 0.82;
        let params = &a.params;
        a.mods.read_all(&|i| params.get(i));
        a.used = crate::alchemy_mod::used(&a.mods);
        let params = &a.params;
        a.arp_cfg.read_all(&|i| params.get(i));
        let params = &a.params;
        a.racks.read_all(&|i| params.get(i));
        a.update();
        a
    }

    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }

    /// Perform value k (0..16) after the Transform pad.
    fn perform(&self, k: usize) -> f32 {
        if self.transform && self.p(SNAP_LOCK + k as u32) < 0.5 {
            let w = transform_weights(self.p(17), self.p(18));
            (0..8).map(|i| w[i] * self.p(snapv(i, k))).sum()
        } else {
            self.p(PERFORM_IDS[k])
        }
    }

    /// The Snap Vol at the pad position (dB).
    fn snap_vol(&self) -> f32 {
        let w = transform_weights(self.p(17), self.p(18));
        (0..8).map(|i| w[i] * self.p(SNAPVOL + i as u32)).sum()
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
            m_rack: 0,
            f2_rack: 0,
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
            glide_time: false,
            wide: false,
            retrig: true,
            from: f32::NAN,
            chan: 0,
            tune: 0.0,
            lfo1: 0.0,
            lfo2: 0.0,
            character: 0.0,
            vol: 1.0,
            pan: 0.0,
            note_index: 0,
            arp_index: 0,
            speed: 0.0,
            note_pan: 0.0,
            arp_mod: [0.0; 2],
            playhead: 0.0,
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
                va_phase: p(212 + s as u32),
                noise_type: p(NOISE + s as u32).round().clamp(0.0, 2.0) as u8,
                noise_vol: p(NOISE + 4 + s as u32),
                noise_lo: noise_cut_hz(p(NOISE + 8 + s as u32)),
                noise_hi: noise_cut_hz(p(NOISE + 12 + s as u32)),
                keytrack: true,
                sf: [0usize, 1, 2].map(|k| {
                    let b = SRC_FILT + s as u32 * 16;
                    let ty = if k == 0 { p(333 + s as u32) } else { p(b + 2 + k as u32) };
                    (ty.round().max(0.0) as u8, p(51 + k as u32 * 8 + s as u32), p(55 + k as u32 * 8 + s as u32), p(b + 5 + k as u32), p(b + k as u32) >= 0.5)
                }),
                sf_par: p(SRC_FILT + s as u32 * 16 + 8) >= 0.5,
                send: p(crate::alchemy_fx::SEND + s as u32).round().clamp(0.0, 8.0) as u8,
                keyscale: p(KEYSCALE + s as u32).round().clamp(0.0, 2.0) as u8,
                wait: p(WAIT + s as u32),
                part: crate::alchemy_partials::PartCfg::read(p, s),
                route: p(ROUTE_F2 + s as u32),
            };
        }
        morph(&mut sh, p);
        sh.character = d(0);
        sh.m_cut = (p(85) + d(1) * 0.5).clamp(0.0, 1.0);
        sh.m_res = p(86);
        sh.m_drive = (p(87) + pf[5] * 0.6).clamp(0.0, 1.0);
        sh.m_on = p(364) >= 0.5;
        sh.m_type = p(365).round().max(0.0) as u8;
        sh.f2_type = p(368).round().max(0.0) as u8;
        sh.m_fx = p(88);
        sh.f2_fx = p(92);
        sh.m_rack = p(crate::alchemy_fx::SEND + 4).round().clamp(0.0, 4.0) as u8;
        sh.f2_rack = p(crate::alchemy_fx::SEND + 5).round().clamp(0.0, 4.0) as u8;
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
        sh.glide_time = p(VOICE + 1) >= 0.5;
        sh.wide = p(VOICE + 2) >= 0.5;
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
        self.poly.priority = p(VOICE).round().clamp(0.0, 3.0) as u8;
        self.ch.bend_up = p(391);
        self.ch.bend_down = p(396);
        self.ch.update_bend();
        self.limiter = p(LIMITER) >= 0.5;
    }

    /// The voices sounding (not fading out): note, pitch now (semitones) and seconds since
    /// their last trigger.
    pub fn voice_states(&self) -> Vec<(u8, f32, f32)> {
        self.poly.voices.iter().filter(|v| v.on && !v.killing).map(|v| (v.note, v.pitch, v.age)).collect()
    }

    /// One note's pitch bend in MIDI Mono Mode: a member channel bends its own voices by Mono
    /// Mode Bend Range; the base channel (or Mono Mode off) is the normal pitch wheel.
    pub fn pitch_bend_ch(&mut self, channel: u8, value: i16) {
        let base = match self.p(MONO_MODE).round() as i32 {
            1 => Some(0),
            2 => Some(15),
            _ => None,
        };
        match base {
            Some(b) if b != channel => {
                let st = value as f32 / 8192.0 * self.p(MONO_BEND);
                for v in self.poly.voices.iter_mut().filter(|v| v.on && v.chan == channel) {
                    v.mpe_bend = st;
                }
            }
            _ => self.pitch_bend(value),
        }
    }

    /// The held key Num 1 plays, by Voice Priority (Newest, Oldest, Lowest, Highest).
    fn choose(&self) -> Option<(u8, u8)> {
        let k = &self.keys[..self.nkeys];
        match self.poly.priority {
            1 => k.first().copied(),
            2 => k.iter().min_by_key(|e| e.0).copied(),
            3 => k.iter().max_by_key(|e| e.0).copied(),
            _ => k.last().copied(),
        }
    }

    /// A note starts, by the Trigger Mode (Always, Retrigger, Legato) and Num: with Num 1,
    /// Always triggers once per legato group but glides on every note, Retrigger triggers and
    /// glides on every note, Legato triggers and glides once per legato group; with more voices
    /// every note triggers, Legato gliding only for notes played while others are held.
    fn play_on(&mut self, pitch: u8, vel: u8) {
        let trig = self.p(371).round() as u8;
        let held_before = self.nkeys > 0;
        if let Some(i) = self.keys[..self.nkeys].iter().position(|e| e.0 == pitch) {
            self.keys.copy_within(i + 1..self.nkeys, i);
            self.nkeys -= 1;
        }
        if self.nkeys < self.keys.len() {
            self.keys[self.nkeys] = (pitch, vel);
            self.nkeys += 1;
        }
        if self.poly.max == 1 {
            let (want, wv) = self.choose().unwrap_or((pitch, vel));
            match self.sounding.filter(|_| held_before) {
                Some(cur) => {
                    if want != cur {
                        self.move_mono(cur, want, wv, trig == 1);
                    }
                }
                None => {
                    self.sh.from = if trig != 2 { self.last } else { f32::NAN };
                    self.poly.note_on(want, wv, &self.sh, &self.ch.ctl);
                    self.sounding = Some(want);
                    self.last = want as f32;
                }
            }
        } else {
            self.sh.from = if trig != 2 || held_before { self.last } else { f32::NAN };
            self.poly.note_on(pitch, vel, &self.sh, &self.ch.ctl);
            self.last = pitch as f32;
        }
        self.sh.from = f32::NAN;
        self.sh.retrig = true;
    }

    /// Num 1: the sounding voice moves from `cur` to `want`, gliding (legato, or retriggered).
    fn move_mono(&mut self, cur: u8, want: u8, vel: u8, retrig: bool) {
        self.sh.retrig = retrig;
        self.sh.from = self.last;
        if !self.poly.legato(cur, want, vel, &self.sh, &self.ch.ctl) {
            self.sh.retrig = true;
            self.poly.note_on(want, vel, &self.sh, &self.ch.ctl);
        }
        self.sounding = Some(want);
        self.last = want as f32;
        self.sh.from = f32::NAN;
        self.sh.retrig = true;
    }

    fn play_off(&mut self, pitch: u8) {
        if let Some(i) = self.keys[..self.nkeys].iter().position(|e| e.0 == pitch) {
            self.keys.copy_within(i + 1..self.nkeys, i);
            self.nkeys -= 1;
        }
        if self.poly.max == 1 && self.sounding.is_some() {
            if self.sounding != Some(pitch) {
                return;
            }
            if let Some((want, wv)) = self.choose() {
                // back to a key still held
                let trig = self.p(371).round() as u8;
                return self.move_mono(pitch, want, wv, trig == 1);
            }
            self.sounding = None;
        }
        self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
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
            // the eight snapshots (missing ones as snapshot 1)
            for n in 0..8 {
                let src = snaps.get(n).unwrap_or(first);
                for i in 0..16 {
                    self.params.set(snapv(n, i), src[i]);
                }
            }
        }
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
    fn value_text(&self, id: u32, value: f32) -> Option<String> {
        use crate::alchemy_fx as fx;
        if (fx::FXR..fx::FXR + fx::RACKS as u32 * 256).contains(&id) {
            let k = id - fx::FXR;
            let (r, sl, f) = ((k / 256) as usize, ((k % 256) / 24) as usize, (k % 256) % 24);
            let ty = self.params.get(fx::id(r, sl, 0)).round().max(0.0) as usize;
            return fx::knobs(ty).get((f as usize).wrapping_sub(2)).filter(|_| f >= 2).map(|s| fx::text(s, value));
        }
        // the Transform pad's Rate (0: Off) and Snap Vol
        if id == SNAP_RATE {
            return Some(if value <= 0.0 { "Off".to_string() } else { format!("{:.2} s", value) });
        }
        if (SNAPVOL..SNAPVOL + 8).contains(&id) {
            return Some(format!("{value:.2} dB"));
        }
        // the sources' Vol: a gain shown in dB (−∞ … 0 dB)
        if (19..=22).contains(&id) {
            return Some(if value <= 1e-5 { "-inf dB".to_string() } else { format!("{:.1} dB", 20.0 * value.log10()) });
        }
        crate::alchemy_arp::value_text(id, value)
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
            } else if let Some(k) = PERFORM_IDS.iter().position(|&x| x == id) {
                self.transform = false;
                // Snapshot Update: a Perform control's change goes to the selected snapshot
                if self.p(SNAP_UPDATE) >= 0.5 {
                    let n = nearest_snapshot(self.p(17), self.p(18));
                    self.params.set(snapv(n, k), v);
                }
            } else if (SNAPV..SNAPV + 128).contains(&id) {
                self.transform = true;
            }
            let params = &self.params;
            if self.arp_cfg.read_changed(id, &|i| params.get(i)) || self.racks.read_changed(id, &|i| params.get(i)) {
                return;
            }
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
        self.racks.reset();
        self.nkeys = 0;
        self.sounding = None;
        self.last = f32::NAN;
        let mut ev = [crate::alchemy_arp::ArpEv::Off(0); 32];
        self.arp.clear(&mut ev);
        self.update();
    }
    fn tail(&self) -> f32 {
        self.sh.amp_env[3] + 2.0 + if self.extra.fx.iter().any(|f| f.is_some()) || self.racks.tail() { 6.0 } else { 0.0 }
    }
}

impl Instrument for Alchemy {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        // the Transform pad's Octave: its first eight notes switch snapshots 1–8 (not played)
        let oct = self.p(SNAP_OCTAVE).round() as i32;
        if oct > 0 {
            let base = (oct - 1) * 12;
            let k = pitch as i32 - base;
            if (0..8).contains(&k) {
                let (x, y) = snapshot_centre(k as usize);
                let rate = self.p(SNAP_RATE);
                if rate <= 0.0 {
                    self.params.set(17, x);
                    self.params.set(18, y);
                    self.transform = true;
                    self.update();
                } else {
                    self.pad_to = Some((x, y, rate));
                }
                self.switch_keys |= 1u128 << pitch;
                return;
            }
        }
        if self.extra.held == 0 {
            self.extra.mseg_t = 0.0;
        }
        if self.arp_cfg.mode != 0 {
            if self.arp.held() == 0 {
                self.mg.arp_count = 0;
            }
            if self.arp.note_on(&self.arp_cfg, pitch, velocity, self.mg.beats, self.bpm, self.playing) {
                self.extra.held += 1;
                return;
            }
        }
        self.sh.note_pan = 0.0;
        self.sh.arp_mod = [0.0; 2];
        self.count_note();
        self.extra.held += 1;
        self.mg.held = self.extra.held;
        self.play_on(pitch, velocity);
    }
    fn note_off(&mut self, pitch: u8) {
        if self.switch_keys & (1u128 << pitch) != 0 {
            self.switch_keys &= !(1u128 << pitch);
            return;
        }
        self.extra.held = self.extra.held.saturating_sub(1);
        if self.extra.held == 0 && self.mg.held > 0 {
            self.mg.held = 0;
            self.mg.all_released();
        }
        if self.arp_cfg.mode != 0 && self.arp.note_off(&self.arp_cfg, pitch) {
            return;
        }
        self.play_off(pitch);
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.extra.set_tempo(bpm);
        self.bpm = bpm as f32;
    }
    fn set_transport(&mut self, t: lg_dsp::Transport) {
        if t.playing {
            self.mg.beats = t.beats;
        } else if self.playing {
            // stopping the transport stops the arpeggiator
            let mut ev = [crate::alchemy_arp::ArpEv::Off(0); 32];
            let k = self.arp.clear(&mut ev);
            for e in &ev[..k] {
                if let crate::alchemy_arp::ArpEv::Off(p) = *e {
                    self.play_off(p);
                }
            }
            self.extra.held = 0;
        }
        self.playing = t.playing;
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
            self.nkeys = 0;
            self.sounding = None;
            return self.poly.all_notes_off(&self.sh, &self.ch.ctl);
        }
        self.mg.cc[(cc as usize).min(127)] = value as f32 / 127.0;
        // the ModWheel pop-up: the wheel moves a Perform control (or the inverse)
        let wt = self.p(SNAP_WHEEL).round() as usize;
        if cc == 1 && wt > 0 {
            let (k, inv) = ((wt - 1) % 16, wt > 16);
            let x = value as f32 / 127.0;
            if self.params.set(PERFORM_IDS[k], if inv { 1.0 - x } else { x }) {
                self.transform = false;
                self.update();
            }
        }
        if let Some(d) = self.ch.cc(cc, value) {
            self.poly.sustain(d, &self.sh, &self.ch.ctl);
        }
    }
    fn note_on_ch(&mut self, channel: u8, pitch: u8, velocity: u8) {
        self.sh.chan = channel;
        self.note_on(pitch, velocity);
        self.sh.chan = 0;
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut sl = [0.0f32; CHUNK];
        let mut sr = [0.0f32; CHUNK];
        let (gl, gr) = self.ch.out_gains();
        // the pad gliding to a keyswitched snapshot at the Rate
        if let Some((tx, ty, left)) = self.pad_to {
            let dt = n as f32 / self.rate;
            let k = (dt / left.max(1e-4)).min(1.0);
            let (x, y) = (self.p(17), self.p(18));
            self.params.set(17, x + (tx - x) * k);
            self.params.set(18, y + (ty - y) * k);
            self.transform = true;
            self.pad_to = if k >= 1.0 { None } else { Some((tx, ty, left - dt)) };
            self.update();
        }
        let vol = lg_dsp::db_to_gain(self.p(99) + self.snap_vol());
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
            // the Arpeggiator plays the held keys (its knobs as the modulation section moves them)
            if self.arp_cfg.mode != 0 || self.arp.running {
                use crate::alchemy_arp as aa;
                let keep = (self.arp_cfg.mode, self.arp_cfg.pattern, self.arp_cfg.rate, self.arp_cfg.length, self.arp_cfg.swing, self.arp_cfg.octaves, self.arp_cfg.note_vel);
                let ov = &self.mg.ov;
                if ov.n > 0 {
                    let pr = &self.params;
                    let m = |id: u32| {
                        let (lo, hi) = pr.def(id).map(|d| (d.min, d.max)).unwrap_or((0.0, 1.0));
                        ov.apply(id, pr.get(id), lo, hi)
                    };
                    let c = &mut self.arp_cfg;
                    c.mode = m(aa::MODE).round() as u8;
                    c.pattern = (m(aa::PATTERN).round().clamp(1.0, 16.0) as usize) - 1;
                    c.rate = (m(aa::RATE).round().max(0.0) as usize).min(aa::RATES.len() - 1);
                    c.length = m(aa::LENGTH);
                    c.swing = m(aa::SWING);
                    c.octaves = m(aa::OCTAVE).round().clamp(1.0, 4.0) as u8;
                    c.note_vel = m(aa::NOTE_VEL);
                }
                let mut ev = [aa::ArpEv::Off(0); 32];
                let k = if self.arp_cfg.mode == 0 { self.arp.clear(&mut ev) } else { self.arp.advance(&self.arp_cfg, dt, self.bpm, &mut ev) };
                (self.arp_cfg.mode, self.arp_cfg.pattern, self.arp_cfg.rate, self.arp_cfg.length, self.arp_cfg.swing, self.arp_cfg.octaves, self.arp_cfg.note_vel) = keep;
                for e in &ev[..k] {
                    match *e {
                        aa::ArpEv::On(nt) => {
                            if nt.step == 0 {
                                self.mg.arp_count = 0;
                            }
                            let held = self.mg.held;
                            self.count_note();
                            self.mg.held = held.max(1);
                            self.sh.note_pan = nt.pan;
                            self.sh.arp_mod = [nt.m1, nt.m2];
                            self.sh.playhead = nt.step as f32 / nt.steps.max(1) as f32;
                            self.play_on(nt.pitch, nt.vel)
                        }
                        aa::ArpEv::Off(p) => self.play_off(p),
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
            let mut rkl = [[0.0f32; CHUNK]; 4];
            let mut rkr = [[0.0f32; CHUNK]; 4];
            let mut rk_any = [false; 4];
            for v in self.poly.voices.iter_mut() {
                if v.dry_n == m {
                    for i in 0..m {
                        drl[i] += v.dry[i].0;
                        drr[i] += v.dry[i].1;
                    }
                    any_dry = true;
                    for k in 0..4 {
                        if v.rk_used[k] {
                            for i in 0..m {
                                rkl[k][i] += v.rk[k][i].0;
                                rkr[k][i] += v.rk[k][i].1;
                            }
                            rk_any[k] = true;
                        }
                    }
                }
                v.dry_n = 0;
                v.rk_used = [false; 4];
            }
            // racks A–D: their effects, then mixed in with the dry part
            for k in 0..4 {
                if rk_any[k] || self.racks.used[k + 1] {
                    self.racks.process(k + 1, self.bpm, &mut rkl[k][..m], &mut rkr[k][..m]);
                    for i in 0..m {
                        drl[i] += rkl[k][i];
                        drr[i] += rkr[k][i];
                    }
                    any_dry = true;
                }
            }
            // the levels (envelope followers, and the window's source / filter meters)
            {
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
            if self.racks.used[0] {
                self.racks.process(0, self.bpm, &mut sl[..m], &mut sr[..m]);
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
