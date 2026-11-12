//! "Retro Synth" stand-in: a four-engine polyphonic synth (Analog, Sync, Table, FM) with Logic's
//! parameter list (ids = the order of plugins/RETRO-SYNTH-PARAM-MAP.txt; Logic's float block for
//! this plug-in is not yet aligned, see the report). Unison per voice, glide/autobend, mono/legato,
//! ladder and state-variable filters, two envelopes, LFO, vibrato, a small mod matrix
//! (velocity, mod wheel, aftertouch, a free controller) and the chorus/flanger effect.
//! Also the engine behind most synth patches (bass, lead, pad, pluck, brass, bells…).

use std::sync::OnceLock;

use crate::dsp::{self, Adsr, CHUNK, Chorus, Fade, Flanger, Ladder, Osc, Rng, Svf, SvfMode, Wave};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const ENGINES: &[&str] = &["Analog", "Sync", "Table", "FM"];
pub const MIDI_MODE: &[&str] = &["Poly", "Mono", "Legato"];
pub const MOD_TARGETS: &[&str] = &[
    "Off",
    "Cutoff",
    "Resonance",
    "Volume",
    "Osc Shape",
    "Osc Mix",
    "Pitch",
    "LFO Amount",
    "Vibrato",
    "Filter FM",
];
pub const FILTERS: &[&str] = &[
    "LP 24dB Lush",
    "LP 24dB Edgy",
    "LP 12dB",
    "Band Pass",
    "High Pass",
    "Peak",
    "Band Reject",
];
pub const EFFECTS: &[&str] = &["Chorus", "Flanger"];
pub const LFO_WAVES: &[&str] = &[
    "Sine",
    "Triangle",
    "Saw Up",
    "Saw Down",
    "Square",
    "Sample & Hold",
    "Random",
];
pub const VIA: &[&str] = &["Off", "Mod Wheel", "Aftertouch", "Velocity", "Ctrl X"];
pub const GLIDE_TYPE: &[&str] = &["Glide", "Autobend"];
pub const GLIDE_MODE: &[&str] = &["Always", "Legato"];

pub static PARAMS: &[Param] = &[
    Param::new(0, "Polyphony", 1.0, 32.0, 16.0, Unit::None),
    Param::new(1, "Unison", 1.0, 4.0, 1.0, Unit::None),
    Param::new(2, "Transpose", -36.0, 36.0, 0.0, Unit::Semitones),
    Param::new(3, "Tune", -100.0, 100.0, 0.0, Unit::Cents),
    Param::new(4, "Output Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::menu(5, "MIDI Mono Mode", MIDI_MODE, 0.0),
    Param::new(6, "Pitch Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(7, "Mono Mode Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(8, "Stereo Spread", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(9, "Voice Detune", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::menu(10, "Velocity To", MOD_TARGETS, 1.0),
    Param::new(11, "Velocity Mod Amount", -1.0, 1.0, 0.25, Unit::Ratio),
    Param::menu(12, "Mod Wheel To", MOD_TARGETS, 8.0),
    Param::new(13, "Mod Wheel Mod Amount", -1.0, 1.0, 0.5, Unit::Ratio),
    Param::menu(14, "Aftertouch To", MOD_TARGETS, 1.0),
    Param::new(15, "Aftertouch Mod Amount", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(16, "Ctrl X Source", 1.0, 127.0, 74.0, Unit::None),
    Param::menu(17, "Ctrl X To", MOD_TARGETS, 1.0),
    Param::new(18, "Ctrl X Mod Amount", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::menu(19, "Oscillator Type", ENGINES, 0.0),
    Param::new(20, "Vibrato Depth", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(21, "Glide/Autobend", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::menu(22, "Glide/AB Type", GLIDE_TYPE, 0.0),
    Param::menu(23, "Glide/AB Mode", GLIDE_MODE, 0.0),
    Param::new(24, "Glide/AB Time", 0.0, 3.0, 0.0, Unit::Seconds),
    Param::new(
        25,
        "Oscillator Shape by LFO/Env",
        -1.0,
        1.0,
        0.0,
        Unit::Ratio,
    ),
    Param::new(26, "Oscillator Mix", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(27, "Sine Level", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(28, "Oscillator 1 Waveform", 0.0, 3.0, 1.0, Unit::None),
    Param::new(29, "Oscillator 1 Pulse Width", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(30, "Oscillator 2 Waveform", 0.0, 3.0, 1.0, Unit::None),
    Param::new(31, "Oscillator 2 Pulse Width", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(
        32,
        "Oscillator 2 Semitone",
        -24.0,
        24.0,
        0.0,
        Unit::Semitones,
    ),
    Param::new(33, "Oscillator 2 Detune", -50.0, 50.0, 8.0, Unit::Cents),
    Param::new(34, "Filter State", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(35, "Filter Type", FILTERS, 0.0),
    Param::new(36, "Cutoff", 0.0, 1.0, 0.52, Unit::Ratio),
    Param::new(37, "Resonance", 0.0, 1.0, 0.2, Unit::Ratio),
    Param::new(38, "Filter Keyscale", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(39, "Cutoff by LFO", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(40, "Cutoff by Env", -1.0, 1.0, 0.5, Unit::Ratio),
    Param::new(41, "Filter FM/Drive", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(42, "Effect State", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::menu(43, "Effect Type", EFFECTS, 1.0),
    Param::new(44, "Effect Intensity", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(45, "Effect Rate", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::menu(46, "LFO Waveform", LFO_WAVES, 1.0),
    Param::new(47, "LFO Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(48, "LFO Rate", 0.05, 30.0, 4.0, Unit::Hz),
    Param::menu(49, "LFO Via Source", VIA, 0.0),
    Param::new(50, "LFO Via Amount", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::menu(51, "Vibrato Waveform", LFO_WAVES, 0.0),
    Param::new(52, "Vibrato Sync", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(53, "Vibrato Rate", 0.05, 20.0, 5.5, Unit::Hz),
    Param::menu(54, "Vibrato Via Source", VIA, 1.0),
    Param::new(55, "Vibrato Via Amount", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(56, "Filter Env Attack", 0.0, 10.0, 0.0, Unit::Seconds),
    Param::new(57, "Filter Env Decay", 0.0, 10.0, 1.2, Unit::Seconds),
    Param::new(58, "Filter Env Sustain", 0.0, 1.0, 0.4, Unit::Ratio),
    Param::new(59, "Filter Env Release", 0.0, 10.0, 0.05, Unit::Seconds),
    Param::new(60, "Filter Env Velocity", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::new(61, "Amp Env Attack", 0.0, 10.0, 0.003, Unit::Seconds),
    Param::new(62, "Amp Env Decay", 0.0, 10.0, 4.0, Unit::Seconds),
    Param::new(63, "Amp Env Sustain", 0.0, 1.0, 0.75, Unit::Ratio),
    Param::new(64, "Amp Env Release", 0.0, 10.0, 0.2, Unit::Seconds),
    Param::new(65, "Amp Env Velocity", 0.0, 1.0, 0.35, Unit::Ratio),
];

// ---------------------------------------------------------------------------------------------
// Wavetables (our own: additive spectra morphing from sine to bright formant/organ/bell shapes),
// mip-mapped per octave so they stay alias-free.

const TBL_LEN: usize = 2048;
const TBL_FRAMES: usize = 16;
const TBL_MIPS: usize = 11;

pub(crate) struct Tables {
    data: Vec<f32>,
}

pub(crate) fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let mut data = vec![0.0f32; TBL_FRAMES * TBL_MIPS * TBL_LEN];
        for f in 0..TBL_FRAMES {
            let x = f as f32 / (TBL_FRAMES - 1) as f32;
            // spectrum for this frame: harmonic k amplitude
            let spec = |k: usize| -> f32 {
                let kf = k as f32;
                let saw = 1.0 / kf;
                let square = if k % 2 == 1 { 1.0 / kf } else { 0.0 };
                let formant =
                    (-((kf - 1.0 - 14.0 * x) / (2.0 + 4.0 * x)).powi(2)).exp() / kf.sqrt();
                let organ = if [1usize, 2, 3, 4, 6, 8, 12, 16].contains(&k) {
                    0.6 / (kf.sqrt())
                } else {
                    0.0
                };
                let sine = if k == 1 { 1.0 } else { 0.0 };
                // walk: sine → organ → square → saw → formant sweep
                if x < 0.25 {
                    let t = x / 0.25;
                    sine * (1.0 - t) + organ * t
                } else if x < 0.5 {
                    let t = (x - 0.25) / 0.25;
                    organ * (1.0 - t) + square * t
                } else if x < 0.75 {
                    let t = (x - 0.5) / 0.25;
                    square * (1.0 - t) + saw * t
                } else {
                    let t = (x - 0.75) / 0.25;
                    saw * (1.0 - t) + formant * t * 1.5
                }
            };
            let amps: Vec<f32> = (0..=512)
                .map(|k| if k == 0 { 0.0 } else { spec(k) })
                .collect();
            for mip in 0..TBL_MIPS {
                // mip m is safe up to fundamental 20 Hz × 2^m
                let max_h = ((20000.0 / (20.0 * 2f32.powi(mip as i32))) as usize).clamp(1, 512);
                let base = (f * TBL_MIPS + mip) * TBL_LEN;
                let mut peak = 0.0f32;
                for i in 0..TBL_LEN {
                    let ph = i as f64 / TBL_LEN as f64 * std::f64::consts::TAU;
                    // sin(k·ph) by the Chebyshev recurrence
                    let c2 = 2.0 * ph.cos();
                    let (mut s1, mut s2) = (ph.sin(), 0.0f64);
                    let mut acc = 0.0f64;
                    for &a in &amps[1..=max_h] {
                        acc += a as f64 * s1;
                        let s0 = c2 * s1 - s2;
                        s2 = s1;
                        s1 = s0;
                    }
                    data[base + i] = acc as f32;
                    peak = peak.max(acc.abs() as f32);
                }
                if peak > 0.0 {
                    for v in &mut data[base..base + TBL_LEN] {
                        *v /= peak;
                    }
                }
            }
        }
        Tables { data }
    })
}

#[inline]
pub(crate) fn table_read(pos: f32, phase: f32, hz: f32) -> f32 {
    let t = tables();
    let mip = ((hz / 20.0).max(1.0).log2().ceil() as usize).min(TBL_MIPS - 1);
    let fp = pos.clamp(0.0, 1.0) * (TBL_FRAMES - 1) as f32;
    let f0 = (fp as usize).min(TBL_FRAMES - 2);
    let ff = fp - f0 as f32;
    let x = phase * TBL_LEN as f32;
    let i = (x as usize) % TBL_LEN;
    let fr = x - x.floor();
    let j = (i + 1) % TBL_LEN;
    let a0 = (f0 * TBL_MIPS + mip) * TBL_LEN;
    let a1 = ((f0 + 1) * TBL_MIPS + mip) * TBL_LEN;
    let s0 = t.data[a0 + i] + (t.data[a0 + j] - t.data[a0 + i]) * fr;
    let s1 = t.data[a1 + i] + (t.data[a1 + j] - t.data[a1 + i]) * fr;
    s0 + (s1 - s0) * ff
}

// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RetroShared {
    rate: f32,
    engine: u8,
    unison: usize,
    detune: f32,
    spread: f32,
    transpose: f32,
    tune: f32,
    mix: f32,
    sine: f32,
    w1: f32,
    pw1: f32,
    w2: f32,
    pw2: f32,
    semi2: f32,
    det2: f32,
    shape_mod: f32,
    filter_on: bool,
    ftype: u8,
    cutoff: f32,
    res: f32,
    keyscale: f32,
    cut_lfo: f32,
    cut_env: f32,
    drive: f32,
    fenv: [f32; 4],
    fenv_vel: f32,
    aenv: [f32; 4],
    aenv_vel: f32,
    vib_depth: f32,
    glide: f32,
    glide_time: f32,
    autobend: bool,
    vel_to: u8,
    vel_amt: f32,
    // per-chunk global modulation (filled by the instrument)
    lfo: f32,
    vib: f32,
    mods: [f32; 10],
}

pub(crate) struct RetroVoice {
    on: bool,
    note: u8,
    vel: f32,
    pitch: f32,
    target: f32,
    glide_coef: f32,
    bend_off: f32,
    bend_coef: f32,
    o1: [Osc; 4],
    o2: [Osc; 4],
    sine: Osc,
    fm_ph: [f32; 4],
    car_ph: [f32; 4],
    ladder: [Ladder; 2],
    svf: [Svf; 2],
    fenv: Adsr,
    aenv: Adsr,
    fade: Fade,
    killing: bool,
    pans: [(f32, f32); 4],
    level: f32,
    rng: Rng,
}

impl RetroVoice {
    pub fn new(seed: u32) -> RetroVoice {
        RetroVoice {
            on: false,
            note: 60,
            vel: 1.0,
            pitch: 60.0,
            target: 60.0,
            glide_coef: 0.0,
            bend_off: 0.0,
            bend_coef: 0.0,
            o1: [Osc::default(); 4],
            o2: [Osc::default(); 4],
            sine: Osc::default(),
            fm_ph: [0.0; 4],
            car_ph: [0.0; 4],
            ladder: [Ladder::new(); 2],
            svf: [Svf::default(); 2],
            fenv: Adsr::default(),
            aenv: Adsr::default(),
            fade: Fade::default(),
            killing: false,
            pans: [(1.0, 1.0); 4],
            level: 0.0,
            rng: Rng::new(seed),
        }
    }

    /// Mono/legato: move to a new note without retriggering.
    pub fn glide_to(&mut self, note: u8, sh: &RetroShared) {
        self.note = note;
        self.target = note as f32;
        if sh.glide_time <= 0.0 || sh.autobend {
            self.pitch = self.target;
        }
    }

    fn wave_of(w: f32) -> Wave {
        match w.round() as i32 {
            0 => Wave::Triangle,
            1 => Wave::Saw,
            _ => Wave::Pulse,
        }
    }
}

impl Voice<RetroShared> for RetroVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &RetroShared, _ctl: &Ctl) {
        let rate = sh.rate;
        let was_on = self.on && !self.killing;
        self.on = true;
        self.killing = false;
        self.note = note;
        self.vel = velocity.max(1) as f32 / 127.0;
        self.target = note as f32;
        if !(was_on && sh.glide_time > 0.0 && !sh.autobend) {
            self.pitch = self.target;
        }
        self.glide_coef = if sh.glide_time > 0.0 {
            dsp::tau_coef(sh.glide_time * 0.3, rate / CHUNK as f32)
        } else {
            0.0
        };
        if sh.autobend && sh.glide_time > 0.0 {
            self.bend_off = sh.glide * 12.0;
            self.bend_coef = dsp::tau_coef(sh.glide_time * 0.3, rate / CHUNK as f32);
        } else {
            self.bend_off = 0.0;
        }
        for u in 0..4 {
            // free-running analog-style phases
            self.o1[u].phase = self.rng.unit();
            self.o2[u].phase = self.rng.unit();
            self.fm_ph[u] = 0.0;
            self.car_ph[u] = 0.0;
            let p = if sh.unison > 1 {
                (u as f32 / (sh.unison - 1) as f32 * 2.0 - 1.0) * sh.spread
            } else {
                0.0
            };
            self.pans[u] = ((1.0 - p).sqrt(), (1.0 + p).sqrt());
        }
        for l in &mut self.ladder {
            l.reset();
        }
        for s in &mut self.svf {
            s.reset();
        }
        let [a, d, s, r] = sh.fenv;
        self.fenv.set(a, d, s, r, rate);
        let [a, d, s, r] = sh.aenv;
        self.aenv.set(a, d, s, r, rate);
        if was_on {
            self.fenv.gate_on();
            self.aenv.gate_on();
        } else {
            self.fenv.trigger();
            self.aenv.trigger();
        }
    }
    fn release(&mut self, sh: &RetroShared, _ctl: &Ctl) {
        let [a, d, s, r] = sh.aenv;
        self.aenv.set(a, d, s, r, sh.rate);
        self.fenv.gate_off();
        self.aenv.gate_off();
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.003, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &RetroShared, ctl: &Ctl) {
        let rate = sh.rate;
        let n = l.len();
        // control rate
        self.pitch = self.target + (self.pitch - self.target) * self.glide_coef;
        self.bend_off *= self.bend_coef;
        let m = &sh.mods;
        let velm = |t: usize| {
            if sh.vel_to as usize == t {
                sh.vel_amt * (self.vel - 0.5) * 2.0
            } else {
                0.0
            }
        };
        let vib = sh.vib * (sh.vib_depth + m[8].max(0.0)) * 0.5 + m[6] * 2.0;
        let pitch =
            self.pitch + sh.transpose + sh.tune / 100.0 + self.bend_off + vib + ctl.bend + velm(6);
        let hz = lg_dsp::midi_hz(pitch);
        // positive = by LFO, negative = by the filter envelope (FM depth envelope for EFM1-style use)
        let shape_src = if sh.shape_mod >= 0.0 {
            sh.lfo * sh.shape_mod
        } else {
            -sh.shape_mod * self.fenv.level - 0.5 * -sh.shape_mod
        };
        let shape = (shape_src + m[4] + velm(4)).clamp(-1.0, 1.0);
        let mix = (sh.mix + m[5] + velm(5)).clamp(0.0, 1.0);
        let uni = sh.unison.clamp(1, 4);
        // filter
        let fe = self.fenv.level;
        let vel_f = 1.0 - sh.fenv_vel + sh.fenv_vel * self.vel;
        let key = (pitch - 60.0) / 12.0 * sh.keyscale;
        let cut_oct = sh.cutoff * 10.0
            + key
            + sh.cut_env * fe * 7.0 * vel_f
            + sh.cut_lfo * sh.lfo * 3.0
            + m[1] * 4.0
            + velm(1) * 3.0
            + (ctl.brightness - 0.5) * 2.0;
        let fc = (20.0 * 2f32.powf(cut_oct)).clamp(20.0, rate * 0.45);
        let res = (sh.res + m[2] * 0.5 + velm(2)).clamp(0.0, 1.0);
        for ch in 0..2 {
            self.ladder[ch].set(fc, res * 0.98, rate);
            self.ladder[ch].poles = if sh.ftype == 2 { 2 } else { 4 };
            self.ladder[ch].edgy = sh.ftype == 1;
            self.ladder[ch].drive = 1.0 + sh.drive * 3.0;
            let mode = match sh.ftype {
                3 => SvfMode::Band,
                4 => SvfMode::High,
                5 => SvfMode::Peak,
                _ => SvfMode::Notch,
            };
            self.svf[ch].mode = mode;
            self.svf[ch].set(fc, 0.5 + res * 12.0, rate);
        }
        let amp_vel = 1.0 - sh.aenv_vel + sh.aenv_vel * self.vel;
        let vol = (1.0 + m[3] + velm(3)).max(0.0) * amp_vel * 0.38 / (uni as f32).sqrt();
        let det_c = sh.detune * 30.0;
        let mut peak = 0.0f32;
        for i in 0..n {
            let mut sl = 0.0;
            let mut sr = 0.0;
            for u in 0..uni {
                let ud = if uni > 1 {
                    (u as f32 / (uni - 1) as f32 - 0.5) * det_c
                } else {
                    0.0
                };
                let f1 = hz * dsp::semis(ud / 100.0);
                let f2 = f1 * dsp::semis(sh.semi2 + sh.det2 / 100.0);
                let inc1 = f1 / rate;
                let inc2 = f2 / rate;
                let s = match sh.engine {
                    0 => {
                        let pw1 = (sh.pw1 + 0.4 * shape).clamp(0.05, 0.95);
                        let pw2 = (sh.pw2 + 0.4 * shape).clamp(0.05, 0.95);
                        let a = self.o1[u].next(
                            Self::wave_of(sh.w1),
                            inc1,
                            if sh.w1 >= 2.5 { 0.5 } else { pw1 },
                        );
                        let b = self.o2[u].next(
                            Self::wave_of(sh.w2),
                            inc2,
                            if sh.w2 >= 2.5 { 0.5 } else { pw2 },
                        );
                        a * (1.0 - mix) + b * mix
                    }
                    1 => {
                        // osc 2 is hard-synced to osc 1; shape sweeps the sync ratio
                        let ratio = 1.0
                            + (sh.w2 / 3.0 + 0.5 * shape).clamp(0.0, 1.0) * 6.0
                            + sh.semi2.max(0.0) / 12.0;
                        let a = self.o1[u].next(Self::wave_of(sh.w1), inc1, sh.pw1);
                        let wrapped = self.o1[u].phase < inc1;
                        if wrapped {
                            self.o2[u].phase = self.o1[u].phase * ratio;
                        }
                        let b = self.o2[u].next(Wave::Saw, inc1 * ratio, 0.5);
                        a * (1.0 - mix) + b * mix
                    }
                    2 => {
                        let p1 = (sh.w1 / 3.0 + 0.5 * shape).clamp(0.0, 1.0);
                        let p2 = (sh.w2 / 3.0 + 0.5 * shape).clamp(0.0, 1.0);
                        self.o1[u].advance(inc1);
                        self.o2[u].advance(inc2);
                        let a = table_read(p1, self.o1[u].phase, f1);
                        let b = table_read(p2, self.o2[u].phase, f2);
                        a * (1.0 - mix) + b * mix
                    }
                    _ => {
                        // 2-operator FM: osc 2 modulates osc 1; mix = FM amount, osc 1 waveform = carrier ratio
                        let ratio_m = dsp::semis(sh.semi2) * (1.0 + (sh.w2.round()));
                        let ratio_c = 1.0 + sh.w1.round();
                        self.fm_ph[u] =
                            (self.fm_ph[u] + inc1 * ratio_m * dsp::semis(sh.det2 / 100.0)).fract();
                        self.car_ph[u] = (self.car_ph[u] + inc1 * ratio_c).fract();
                        let index = (mix + 0.5 * shape).clamp(0.0, 1.0) * 5.0;
                        let modu = dsp::sin_cycles(self.fm_ph[u]) * index;
                        dsp::sin_cycles(self.car_ph[u] + modu / std::f32::consts::TAU) * 0.5
                    }
                };
                sl += s * self.pans[u].0;
                sr += s * self.pans[u].1;
            }
            if sh.sine > 0.0 {
                let s = self.sine.next(Wave::Sine, hz / rate, 0.5) * sh.sine;
                sl += s;
                sr += s;
            }
            let fenv = self.fenv.next();
            let _ = fenv;
            let a = self.aenv.next();
            let (mut yl, mut yr) = (sl, sr);
            if sh.filter_on {
                if sh.ftype <= 2 {
                    yl = self.ladder[0].run(yl);
                    yr = if uni > 1 || sh.spread > 0.0 {
                        self.ladder[1].run(yr)
                    } else {
                        yl
                    };
                } else {
                    yl = self.svf[0].run(yl);
                    yr = if uni > 1 { self.svf[1].run(yr) } else { yl };
                }
            }
            let mut g = a * vol;
            if self.killing {
                g *= self.fade.next();
            }
            yl *= g;
            yr *= g;
            peak = peak.max(yl.abs());
            l[i] += yl;
            r[i] += yr;
        }
        self.level = peak;
        if self.aenv.idle() || (self.killing && self.fade.gain <= 0.0) {
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

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Lfo {
    phase: f32,
    hold: f32,
    last: f32,
    target: f32,
}

impl Lfo {
    pub(crate) fn step(&mut self, wave: u8, inc: f32, rng: &mut Rng) -> f32 {
        let p = self.phase;
        let wrapped = {
            self.phase += inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                true
            } else {
                false
            }
        };
        match wave {
            0 => dsp::sin_cycles(p),
            1 => 1.0 - 4.0 * (p - 0.5).abs(),
            2 => 2.0 * p - 1.0,
            3 => 1.0 - 2.0 * p,
            4 => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            5 => {
                if wrapped {
                    self.hold = rng.bipolar();
                }
                self.hold
            }
            _ => {
                if wrapped {
                    self.last = self.target;
                    self.target = rng.bipolar();
                }
                dsp::lerp(self.last, self.target, p)
            }
        }
    }
}

pub struct RetroSynth {
    params: Params,
    poly: Poly<RetroVoice>,
    ch: Channel,
    sh: RetroShared,
    rate: f32,
    lfo: Lfo,
    vib: Lfo,
    rng: Rng,
    chorus: Chorus,
    flanger: Flanger,
    stack: [u8; 16],
    nstack: usize,
    mono_note: Option<u8>,
    ctrl_x: f32,
    last_vel: f32,
    out: f32,
}

impl RetroSynth {
    pub fn new(rate: f32) -> RetroSynth {
        dsp::warm_tables();
        let _ = tables();
        let mut seed = 3;
        let mut s = RetroSynth {
            params: Params::new(PARAMS),
            poly: Poly::new(32, || {
                seed += 17;
                RetroVoice::new(seed)
            }),
            ch: Channel::new(rate),
            sh: RetroShared {
                rate,
                ..Default::default()
            },
            rate,
            lfo: Lfo::default(),
            vib: Lfo::default(),
            rng: Rng::new(99),
            chorus: Chorus::new(rate),
            flanger: Flanger::new(rate),
            stack: [0; 16],
            nstack: 0,
            mono_note: None,
            ctrl_x: 0.0,
            last_vel: 0.8,
            out: 1.0,
        };
        s.update();
        s
    }
    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn update(&mut self) {
        let mono = self.p(5).round() as u8;
        let unison = self.p(1).round().clamp(1.0, 4.0) as usize;
        self.poly.set_max(if mono > 0 {
            1
        } else {
            (self.p(0).round() as usize).clamp(1, 32)
        });
        self.poly.retrigger_kills = true;
        let sh = &mut self.sh;
        sh.engine = self.params.get(19).round() as u8;
        sh.unison = unison;
        sh.detune = self.params.get(9);
        sh.spread = self.params.get(8);
        sh.transpose = self.params.get(2);
        sh.tune = self.params.get(3);
        sh.mix = self.params.get(26);
        sh.sine = self.params.get(27);
        sh.w1 = self.params.get(28);
        sh.pw1 = self.params.get(29);
        sh.w2 = self.params.get(30);
        sh.pw2 = self.params.get(31);
        sh.semi2 = self.params.get(32);
        sh.det2 = self.params.get(33);
        sh.shape_mod = self.params.get(25);
        sh.filter_on = self.params.get(34) >= 0.5;
        sh.ftype = self.params.get(35).round() as u8;
        sh.cutoff = self.params.get(36);
        sh.res = self.params.get(37);
        sh.keyscale = self.params.get(38);
        sh.cut_lfo = self.params.get(39);
        sh.cut_env = self.params.get(40);
        sh.drive = self.params.get(41);
        sh.fenv = [
            self.params.get(56),
            self.params.get(57),
            self.params.get(58),
            self.params.get(59),
        ];
        sh.fenv_vel = self.params.get(60);
        sh.aenv = [
            self.params.get(61),
            self.params.get(62),
            self.params.get(63),
            self.params.get(64),
        ];
        sh.aenv_vel = self.params.get(65);
        sh.vib_depth = self.params.get(20);
        sh.glide = self.params.get(21);
        sh.glide_time = self.params.get(24)
            * self
                .params
                .get(21)
                .abs()
                .max(if self.params.get(22) < 0.5 { 1.0 } else { 0.0 });
        sh.autobend = self.params.get(22) >= 0.5;
        sh.vel_to = self.params.get(10).round() as u8;
        sh.vel_amt = self.params.get(11);
        let bend = if mono > 0 {
            self.params.get(7)
        } else {
            self.params.get(6)
        };
        self.ch.bend_up = bend;
        self.ch.bend_down = bend;
        self.ch.update_bend();
        self.out = lg_dsp::db_to_gain(self.params.get(4));
    }

    fn via(&self, src: u8) -> f32 {
        match src {
            1 => self.ch.ctl.mod_wheel,
            2 => self.ch.ctl.aftertouch,
            3 => self.last_vel,
            4 => self.ctrl_x,
            _ => 0.0,
        }
    }

    fn compute_mods(&mut self) {
        let mut m = [0.0f32; 10];
        let route = |m: &mut [f32; 10], t: f32, v: f32| {
            let t = t.round() as usize;
            if t > 0 && t < 10 {
                m[t] += v;
            }
        };
        route(&mut m, self.p(12), self.ch.ctl.mod_wheel * self.p(13));
        route(&mut m, self.p(14), self.ch.ctl.aftertouch * self.p(15));
        route(&mut m, self.p(17), self.ctrl_x * self.p(18));
        self.sh.mods = m;
    }
}

impl Plugin for RetroSynth {
    fn params(&self) -> &[Param] {
        PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            self.update();
        }
    }
    fn reset(&mut self) {
        for v in &mut self.poly.voices {
            v.on = false;
        }
        self.nstack = 0;
        self.mono_note = None;
        self.chorus.reset();
        self.flanger.reset();
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        self.p(64) + 0.3
    }
}

impl Instrument for RetroSynth {
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        self.last_vel = velocity as f32 / 127.0;
        let mono = self.p(5).round() as u8;
        if mono == 0 {
            self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
            return;
        }
        // note stack for mono/legato
        if self.nstack < self.stack.len() {
            self.stack[self.nstack] = pitch;
            self.nstack += 1;
        }
        let legato = mono == 2 || self.p(23) >= 0.5;
        if let (true, Some(_)) = (legato && self.nstack > 1, self.mono_note) {
            if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                v.glide_to(pitch, &self.sh);
                return;
            }
        }
        if let Some(n) = self.mono_note {
            // retrigger: keep the same voice so glide works from the last pitch
            if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                let sh = self.sh;
                v.start(pitch, velocity, &sh, &self.ch.ctl);
                v.note = pitch;
                let _ = n;
                return;
            }
        }
        self.mono_note = Some(pitch);
        self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
    }
    fn note_off(&mut self, pitch: u8) {
        let mono = self.p(5).round() as u8;
        if mono == 0 {
            self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
            return;
        }
        if let Some(i) = self.stack[..self.nstack].iter().position(|&n| n == pitch) {
            for j in i..self.nstack - 1 {
                self.stack[j] = self.stack[j + 1];
            }
            self.nstack -= 1;
        }
        if self.nstack > 0 {
            let top = self.stack[self.nstack - 1];
            if let Some(v) = self.poly.voices.iter_mut().find(|v| v.on && !v.killing) {
                v.glide_to(top, &self.sh);
            }
        } else if let Some(n) = self.mono_note.take() {
            self.poly.note_off(n, &self.sh, &self.ch.ctl);
            // the voice may have moved notes; release whatever is sounding
            let sh = self.sh;
            for v in self.poly.voices.iter_mut().filter(|v| v.on) {
                v.release(&sh, &self.ch.ctl);
            }
        }
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            self.nstack = 0;
            self.mono_note = None;
            return self.poly.all_notes_off(&self.sh, &self.ch.ctl);
        }
        if cc == self.p(16).round() as u8 {
            self.ctrl_x = value as f32 / 127.0;
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
        let fx_on = self.p(42) >= 0.5;
        let fx_type = self.p(43).round() as u8;
        let fx_int = self.p(44);
        let fx_rate = 0.05 * 100f32.powf(self.p(45));
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            self.compute_mods();
            let lfo_inc = self.p(48) * m as f32 / self.rate;
            let via_l = self.via(self.p(49).round() as u8);
            let lfo_amt = if self.p(49) < 0.5 {
                1.0
            } else {
                (1.0 + self.p(50) * (via_l * 2.0 - 1.0)).clamp(0.0, 2.0) * 0.5
            } + self.sh.mods[7];
            self.sh.lfo = self
                .lfo
                .step(self.p(46).round() as u8, lfo_inc, &mut self.rng)
                * lfo_amt;
            let vib_inc = self.p(53) * m as f32 / self.rate;
            let via_v = self.via(self.p(54).round() as u8);
            let vib_amt = if self.p(54) < 0.5 {
                1.0
            } else {
                via_v * (0.5 + self.p(55).max(0.0))
            };
            self.sh.vib = self
                .vib
                .step(self.p(51).round() as u8, vib_inc, &mut self.rng)
                * vib_amt;
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            if fx_on && fx_int > 0.0 {
                if fx_type == 0 {
                    self.chorus
                        .process(&mut sl[..m], &mut sr[..m], fx_rate, fx_int, fx_int);
                } else {
                    self.flanger
                        .process(&mut sl[..m], &mut sr[..m], fx_rate, fx_int, fx_int * 0.3);
                }
            }
            for i in 0..m {
                left[pos + i] += sl[i] * gl * self.out;
                right[pos + i] += sr[i] * gr * self.out;
            }
            pos += m;
        }
    }
}
