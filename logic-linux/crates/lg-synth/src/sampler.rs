//! "Sampler": plays the user's own multi-sample instruments.
//!
//! One zone engine (key/velocity ranges, round robin, loops, per-zone ADSR, pitch, pan, filter,
//! exclusive groups, release triggers) fed by several loaders:
//! - SFZ (`Sampler::from_sfz`), WAV/AIFF/CAF(PCM) samples;
//! - SoundFont 2 "Apple-style" (`Sampler::from_sf2`): every preset region × instrument region
//!   becomes a Sampler zone, as Logic does when it converts a SoundFont into a Sampler instrument;
//! - SoundFont 2 "native" (`SoundFontPlayer`): the .sf2 played directly by rustysynth's engine;
//! - EXS24 (`Sampler::from_exs` / `from_exs_bytes`, also Logic's Sampler state, which embeds
//!   the same chunks): decoded from our reading of the chunk layout (zones, groups, samples);
//!   unverified against Apple's factory files, see the report.
//!
//! Sample data is loaded when the instrument is built; rendering never allocates.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::dsp::{self, Adsr, CHUNK, Fade, Svf, SvfMode};
use crate::sampler_synth::{self, ENVS, Env, FilterUnit, LFOS, Lfo, Source, Synth, target};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use crate::wav;
use lg_dsp::{Instrument, Param, Plugin, Unit};

// ---------------------------------------------------------------------------------------------
// Data

/// PCM storage (16-bit for SoundFonts, float otherwise).
#[derive(Debug)]
pub enum Pcm {
    F32(Vec<f32>),
    I16(Vec<i16>),
}

impl Pcm {
    #[inline]
    fn at(&self, i: usize) -> f32 {
        match self {
            Pcm::F32(v) => v.get(i).copied().unwrap_or(0.0),
            Pcm::I16(v) => v.get(i).map(|&x| x as f32 / 32768.0).unwrap_or(0.0),
        }
    }
    fn len(&self) -> usize {
        match self {
            Pcm::F32(v) => v.len(),
            Pcm::I16(v) => v.len(),
        }
    }
}

#[derive(Debug)]
pub struct SampleData {
    pub rate: f32,
    /// One or two channels.
    pub ch: Vec<Pcm>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    NoLoop,
    OneShot,
    /// Loops while sounding, including the release.
    Continuous,
    /// Loops until note-off, then plays on to the end (Logic's Play to End on Release).
    Sustain,
}

/// Logic's Loop Mode: Forward, Reverse (end to start) or Alternate (back and forth).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LoopDir {
    #[default]
    Forward,
    Reverse,
    Alternate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Attack,
    Release,
}

#[derive(Clone, Debug)]
pub struct Zone {
    pub sample: Arc<SampleData>,
    /// Sample rate of this zone's audio (SoundFont zones share one pool with per-sample rates).
    pub rate: f32,
    pub lokey: u8,
    pub hikey: u8,
    pub lovel: u8,
    pub hivel: u8,
    /// Root key (MIDI) plus fine tune in cents.
    pub root: u8,
    pub tune_cents: f32,
    /// Cents per key (100 = normal).
    pub keytrack: f32,
    pub gain: f32,
    pub pan: f32,
    pub start: usize,
    pub end: usize,
    pub loop_mode: LoopMode,
    pub loop_start: usize,
    pub loop_end: usize,
    /// delay, attack, hold, decay (s), sustain (0..1), release (s)
    pub env: [f32; 6],
    pub cutoff: Option<f32>,
    pub resonance: f32,
    pub filter: SvfMode,
    pub vel_track: f32,
    pub group: i32,
    pub off_by: i32,
    pub seq_length: u32,
    pub seq_position: u32,
    pub trigger: Trigger,
    pub lorand: f32,
    pub hirand: f32,
    /// Plays from the end marker back to the start marker.
    pub reverse: bool,
    pub loop_dir: LoopDir,
    /// Loop crossfade (samples) and its curve (equal power, else linear).
    pub loop_xfade: usize,
    pub loop_eq_power: bool,
    /// Retunes the looped part (cents).
    pub loop_tune: f32,
    /// Fade in / out (samples) at the sample start / end.
    pub fade_in: usize,
    pub fade_out: usize,
    /// Envelope 1 of the Synth pane shapes this zone (instruments made in the Sampler editor);
    /// off: the zone's own `env` (SFZ, SoundFont, EXS), scaled by the Attack… Release Scale knobs.
    pub synth_env: bool,
    /// The group's Envelope 1 offsets: attack, decay, release (seconds added), sustain (added).
    pub env_offset: [f32; 4],
    /// The group's filter offsets: cutoff (octaves), resonance (added).
    pub cutoff_offset: f32,
    pub res_offset: f32,
    /// The Sampler group (index into the instrument's group rules), for round robin,
    /// articulations, keyswitches, voice limits and exclusive classes.
    pub layer: u16,
    /// Group key / velocity crossfades: (low, high, width, type 0 dB / 1 gain / 2 equal power).
    pub key_xfade: (u8, u8, u8, u8),
    pub vel_xfade: (u8, u8, u8, u8),
    /// Scale: the level across the key range (−1 … 1: low keys softer … louder).
    pub scale: f32,
}

impl Zone {
    pub fn new(sample: Arc<SampleData>) -> Zone {
        let end = sample.ch.first().map(|c| c.len()).unwrap_or(0);
        Zone {
            rate: sample.rate,
            sample,
            lokey: 0,
            hikey: 127,
            lovel: 1,
            hivel: 127,
            root: 60,
            tune_cents: 0.0,
            keytrack: 100.0,
            gain: 1.0,
            pan: 0.0,
            start: 0,
            end,
            loop_mode: LoopMode::NoLoop,
            loop_start: 0,
            loop_end: end,
            env: [0.0, 0.001, 0.0, 0.0, 1.0, 0.05],
            cutoff: None,
            resonance: 0.0,
            filter: SvfMode::Low,
            vel_track: 1.0,
            group: 0,
            off_by: 0,
            seq_length: 1,
            seq_position: 1,
            trigger: Trigger::Attack,
            lorand: 0.0,
            hirand: 1.0,
            reverse: false,
            loop_dir: LoopDir::Forward,
            loop_xfade: 0,
            loop_eq_power: false,
            loop_tune: 0.0,
            fade_in: 0,
            fade_out: 0,
            synth_env: false,
            env_offset: [0.0; 4],
            cutoff_offset: 0.0,
            res_offset: 0.0,
            layer: 0,
            key_xfade: (0, 127, 0, 0),
            vel_xfade: (0, 127, 0, 0),
            scale: 0.0,
        }
    }
    fn matches(&self, key: u8, vel: u8) -> bool {
        key >= self.lokey && key <= self.hikey && vel >= self.lovel && vel <= self.hivel
    }
    /// The group crossfade gain for this key and velocity.
    fn xfade(&self, key: u8, vel: u8) -> f32 {
        let one = |(lo, hi, w, kind): (u8, u8, u8, u8), x: u8| {
            if w == 0 {
                return 1.0;
            }
            let d = (x as f32 - lo as f32 + 1.0).min(hi as f32 - x as f32 + 1.0);
            sampler_synth::xfade_gain(kind, d / (w as f32 + 1.0))
        };
        one(self.key_xfade, key) * one(self.vel_xfade, vel)
    }
}

/// How a Sampler group is chosen when a note plays (Group view's selection columns), its voice
/// limit and exclusive class, mute and solo.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GroupRule {
    pub mute: bool,
    pub solo: bool,
    /// Enable by Articulation (the note's or the Articulation ID target's).
    pub articulation: Option<u8>,
    /// Enable by Note: this keyswitch turns the group on and the other keyswitched ones off.
    pub on_note: Option<u8>,
    /// Enable by Control: (controller, low, high).
    pub on_control: Option<(u8, u8, u8)>,
    /// Enable by Bend: (low, high) of the bend (−64…63).
    pub on_bend: Option<(i8, i8)>,
    /// Enable by Channel (Studio plays every track on channel 1).
    pub on_channel: Option<u8>,
    /// Enable by Tempo: (low, high) BPM.
    pub on_tempo: Option<(f32, f32)>,
    /// Round robin: (cycle, this group's place, groups in the cycle).
    pub round_robin: Option<(u8, u8, u8)>,
    /// Voices the group may play at once (0: any).
    pub voices: u8,
    /// Exclusive class (0: none): a note in one group of the class stops the others.
    pub exclusive: u8,
    /// Release Trigger Decay: a release sample fades over this time.
    pub release_decay: Option<f32>,
}

// ---------------------------------------------------------------------------------------------
// Engine

pub static PARAMS: &[Param] = &[
    Param::new(0, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(1, "Attack Scale", 0.0, 10.0, 1.0, Unit::Ratio),
    Param::new(2, "Decay Scale", 0.0, 10.0, 1.0, Unit::Ratio),
    Param::new(3, "Sustain Scale", 0.0, 1.0, 1.0, Unit::Ratio),
    Param::new(4, "Release Scale", 0.0, 10.0, 1.0, Unit::Ratio),
    Param::new(5, "Filter", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(6, "Cutoff", 20.0, 20000.0, 20000.0, Unit::Hz),
    Param::new(7, "Resonance", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(8, "Velocity to Cutoff", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(9, "Velocity Sensitivity", 0.0, 1.0, 1.0, Unit::Ratio),
    Param::new(10, "Transpose", -48.0, 48.0, 0.0, Unit::Semitones),
    Param::new(11, "Tune", -100.0, 100.0, 0.0, Unit::Cents),
    Param::new(12, "Pitch Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
    Param::new(13, "Polyphony", 1.0, 64.0, 32.0, Unit::None),
    Param::new(14, "Mod Wheel to Vibrato", 0.0, 1.0, 0.3, Unit::Ratio),
];

/// Every parameter: the first fifteen and the synth's (sampler_synth).
pub fn all_params() -> &'static [Param] {
    static P: std::sync::OnceLock<Vec<Param>> = std::sync::OnceLock::new();
    P.get_or_init(|| sampler_synth::params(PARAMS)).as_slice()
}

/// Control-rate block (samples): envelopes, LFOs, modulation and filter settings move per block.
const CTL: usize = 16;

/// What the instrument hands the next voice it starts.
#[derive(Clone, Copy, Debug, Default)]
struct Pending {
    zone: usize,
    detune: f32,
    pan: f32,
    xfade: f32,
    start_mod: f32,
    times: [f32; 4],
    /// The LFO Phase targets' offsets at note on.
    lfo_phase: [f32; LFOS],
    glide_from: Option<f32>,
    random: f32,
    vel: f32,
    key_shift: f32,
}

pub(crate) struct SamplerShared {
    rate: f32,
    zones: Vec<Zone>,
    groups: Vec<GroupRule>,
    env_scale: [f32; 4],
    vel_cut: f32,
    vel_sens: f32,
    transpose: f32,
    tune: f32,
    vib: f32,
    vib_phase: f32,
    syn: Synth,
    /// The mono LFOs (one for every voice).
    lfo_mono: [Lfo; LFOS],
    lfo_mono_val: [f32; LFOS],
    cc: [f32; 128],
    pending: Pending,
    npending: usize,
}

pub(crate) struct SamplerVoice {
    on: bool,
    zone: usize,
    note: u8,
    pos: f64,
    dir: f64,
    looped: bool,
    released: bool,
    zenv: Adsr,
    env: [Env; ENVS],
    lfo: [Lfo; LFOS],
    lfo_val: [f32; LFOS],
    delay: u32,
    gain: f32,
    pan_l: f32,
    pan_r: f32,
    zsvf: [Svf; 2],
    zfilt: bool,
    filt: [FilterUnit; 2],
    fade: Fade,
    killing: bool,
    level: f32,
    pub group: i32,
    pub off_by: i32,
    pub layer: u16,
    glide: f32,
    glide_target: f32,
    vel: f32,
    key: f32,
    random: f32,
    pan_off: f32,
    amp_prev: f32,
    rel_decay: f32,
    age: u64,
}

impl SamplerVoice {
    fn new() -> SamplerVoice {
        SamplerVoice {
            on: false,
            zone: 0,
            note: 0,
            pos: 0.0,
            dir: 1.0,
            looped: false,
            released: false,
            zenv: Adsr::default(),
            env: [Env::default(); ENVS],
            lfo: [Lfo::default(); LFOS],
            lfo_val: [0.0; LFOS],
            delay: 0,
            gain: 0.0,
            pan_l: 1.0,
            pan_r: 1.0,
            zsvf: [Svf::default(); 2],
            zfilt: false,
            filt: [FilterUnit::default(); 2],
            fade: Fade::default(),
            killing: false,
            level: 0.0,
            group: 0,
            off_by: 0,
            layer: 0,
            glide: 0.0,
            glide_target: 0.0,
            vel: 0.0,
            key: 60.0,
            random: 0.0,
            pan_off: 0.0,
            amp_prev: 0.0,
            rel_decay: 0.0,
            age: 0,
        }
    }
    /// A legato note: the voice glides to the new key without restarting.
    fn retarget(&mut self, note: u8, sh: &SamplerShared) {
        let z = &sh.zones[self.zone];
        let semis = (note as f32 - z.root as f32) * z.keytrack / 100.0 + (z.tune_cents + sh.tune) / 100.0 + sh.transpose;
        self.glide_target = semis;
        if sh.syn.glide <= 0.0 {
            self.glide = semis;
        }
        self.note = note;
        self.key = note as f32;
        self.released = false;
        self.env[0].retrigger();
        self.zenv.gate_on();
    }
    /// The value of a modulation source for this voice now.
    #[inline]
    fn source(&self, s: Source, sh: &SamplerShared, ctl: &Ctl) -> f32 {
        match s {
            Source::None | Source::SideChain => 0.0,
            Source::Maximum => 1.0,
            Source::Random => self.random,
            Source::Lfo(k) => self.lfo_val[k as usize],
            Source::Env(k) => self.env[k as usize].out,
            Source::Key => (self.key - 60.0) / 60.0,
            Source::Velocity | Source::ReleaseVelocity => self.vel,
            Source::PitchBend => ctl.bend / 12.0,
            Source::Aftertouch => ctl.aftertouch,
            Source::ModWheel => ctl.mod_wheel,
            Source::Cc(c) => sh.cc[c as usize],
        }
    }
}

/// A Mod Matrix source's value at note-on (before any envelope or LFO runs).
fn note_on_source(s: Source, key: u8, vel: f32, random: f32, sh: &SamplerShared, ctl: &Ctl) -> f32 {
    match s {
        Source::Maximum => 1.0,
        Source::Random => random,
        Source::Key => (key as f32 - 60.0) / 60.0,
        Source::Velocity | Source::ReleaseVelocity => vel,
        Source::PitchBend => ctl.bend / 12.0,
        Source::Aftertouch => ctl.aftertouch,
        Source::ModWheel => ctl.mod_wheel,
        Source::Cc(c) => sh.cc[c as usize],
        _ => 0.0,
    }
}

/// The sum of the note-on routings into `target`.
fn note_on_mod(target: usize, key: u8, vel: f32, random: f32, sh: &SamplerShared, ctl: &Ctl) -> f32 {
    let mut m = 0.0;
    for rt in &sh.syn.routes[..sh.syn.nroutes] {
        if rt.target != target {
            continue;
        }
        let s = note_on_source(rt.src, key, vel, random, sh, ctl);
        let via = (rt.via != Source::None).then(|| note_on_source(rt.via, key, vel, random, sh, ctl));
        m += sampler_synth::route_value(rt, s, via);
    }
    m
}

impl Voice<SamplerShared> for SamplerVoice {
    fn start(&mut self, note: u8, _velocity: u8, sh: &SamplerShared, _ctl: &Ctl) {
        if sh.npending == 0 {
            self.on = false;
            return;
        }
        // the instrument queued the zone for this voice
        let pd = sh.pending;
        let zi = pd.zone;
        let z = &sh.zones[zi];
        let rate = sh.rate;
        self.zone = zi;
        self.note = note;
        self.key = note as f32;
        self.layer = z.layer;
        let semis = (note as f32 - z.root as f32) * z.keytrack / 100.0
            + (z.tune_cents + sh.tune + pd.detune) / 100.0
            + sh.transpose
            + pd.key_shift;
        self.glide_target = semis;
        self.glide = pd.glide_from.unwrap_or(semis);
        let len = z.end.saturating_sub(z.start).max(1) as f64;
        self.pos = z.start as f64 + (pd.start_mod.clamp(0.0, 0.99) as f64 * len).floor();
        self.dir = 1.0;
        self.looped = false;
        self.released = false;
        let [delay, a, hold, d, s, r] = z.env;
        let _ = hold;
        self.zenv.set(
            a * sh.env_scale[0],
            d.max(0.001) * sh.env_scale[1] * if d <= 0.0 { 1000.0 } else { 1.0 },
            s * sh.env_scale[2],
            r * sh.env_scale[3],
            rate,
        );
        self.zenv.trigger();
        // the synth's envelopes (Envelope 1 with the group's offsets)
        let mut e1 = sh.syn.env[0];
        if z.synth_env {
            e1.attack = (e1.attack + z.env_offset[0]).max(0.0);
            e1.decay = (e1.decay + z.env_offset[1]).max(0.0);
            e1.release = (e1.release + z.env_offset[2]).max(0.0);
            e1.sustain = (e1.sustain + z.env_offset[3]).clamp(0.0, 1.0);
        }
        self.env[0].start(&e1, pd.times, rate);
        for k in 1..ENVS {
            if sh.syn.env[k].on {
                self.env[k].start(&sh.syn.env[k], [1.0, 1.0, 1.0, pd.times[3]], rate);
            } else {
                self.env[k] = Env::default();
            }
        }
        self.random = pd.random;
        for k in 0..LFOS {
            let ls = &sh.syn.lfo[k];
            if ls.key_trigger || !self.on {
                self.lfo[k].restart(ls, (note as u32 + 1) * 7919 + k as u32 * 104729 + (pd.random * 1000.0) as u32);
                self.lfo[k].phase = (self.lfo[k].phase + pd.lfo_phase[k]).rem_euclid(1.0);
            }
            self.lfo_val[k] = 0.0;
        }
        self.delay = if z.synth_env { 0 } else { (delay * rate) as u32 };
        let v = pd.vel;
        self.vel = v;
        let vel_gain = if z.synth_env {
            let vt = sh.syn.env[0].vel;
            let c = sh.syn.vel_curve;
            let vc = v.powf(2f32.powf(c * 1.5));
            1.0 - vt + vt * vc
        } else {
            let vt = z.vel_track * sh.vel_sens;
            1.0 - vt + vt * v * v
        };
        let mut key_db = sh.syn.key_scale * (note as f32 - 60.0) / 60.0 * 12.0;
        if z.scale != 0.0 && z.hikey > z.lokey {
            let mid = (z.lokey as f32 + z.hikey as f32) * 0.5;
            key_db += z.scale * 12.0 * (note as f32 - mid) / ((z.hikey - z.lokey) as f32 * 0.5);
        }
        self.gain = z.gain * vel_gain * pd.xfade * lg_dsp::db_to_gain(key_db);
        let p = (z.pan + pd.pan).clamp(-1.0, 1.0);
        self.pan_off = pd.pan;
        self.pan_l = (1.0 - p).sqrt();
        self.pan_r = (1.0 + p).sqrt();
        self.zfilt = z.cutoff.is_some();
        self.age = 0;
        let fc = z.cutoff.unwrap_or(20000.0);
        let q = 0.707 + (z.resonance / 24.0) * 10.0;
        for f in &mut self.zsvf {
            *f = Svf::new(z.filter, fc.clamp(20.0, rate * 0.45), q, rate);
        }
        for f in &mut self.filt {
            f.reset();
        }
        self.group = z.group;
        self.off_by = z.off_by;
        self.killing = false;
        self.on = true;
        self.level = self.gain;
        self.amp_prev = 0.0;
        self.rel_decay = 0.0;
        if z.trigger == Trigger::Release
            && let Some(t) = sh.groups.get(z.layer as usize).and_then(|g| g.release_decay)
        {
            self.rel_decay = dsp::t60_coef(t.max(0.01), rate);
        }
    }
    fn release(&mut self, sh: &SamplerShared, _ctl: &Ctl) {
        let z = &sh.zones[self.zone];
        if z.loop_mode == LoopMode::OneShot {
            return;
        }
        self.released = true;
        self.zenv.gate_off();
        for e in &mut self.env {
            e.gate_off();
        }
        if z.loop_mode == LoopMode::Sustain {
            // Play to End on Release: on toward the end marker
            self.dir = 1.0;
        }
    }
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.004, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &SamplerShared, ctl: &Ctl) {
        let z = &sh.zones[self.zone];
        let data = &z.sample;
        let stereo = data.ch.len() > 1;
        let rate = sh.rate;
        let syn = &sh.syn;
        let n_all = l.len();
        let looping_mode = match z.loop_mode {
            LoopMode::Continuous => true,
            LoopMode::Sustain => !self.released,
            _ => false,
        } && z.loop_end > z.loop_start + 1;
        // logical positions run start → end; a reversed zone reads the audio mirrored
        let (s0, e0) = (z.start.min(data.ch[0].len()), z.end.min(data.ch[0].len()));
        if e0 <= s0 + 1 {
            self.on = false;
            return;
        }
        let (ls0, le0) = if z.reverse {
            (s0 + e0 - z.loop_end.min(e0), s0 + e0 - z.loop_start.max(s0))
        } else {
            (z.loop_start.max(s0), z.loop_end.min(e0))
        };
        let (mut ls, mut le) = (ls0, le0);
        let mut looping = looping_mode && le > ls + 1;
        let mut llen = (le - ls) as f64;
        let phys = |k: usize| if z.reverse { s0 + e0 - 1 - k } else { k };
        let mut peak = 0.0f32;
        let mut i = 0;
        while i < n_all {
            let m = (n_all - i).min(CTL);
            let dt = m as f32 / rate;
            // ---- control rate: envelopes, LFOs, the Mod Matrix
            for k in 0..ENVS {
                if k == 0 || syn.env[k].on {
                    self.env[k].advance(m);
                }
            }
            let mut lfo_rate_mod = [0.0f32; LFOS];
            let mut md = [0.0f32; target::COUNT];
            for rt in &syn.routes[..syn.nroutes] {
                let s = self.source(rt.src, sh, ctl);
                let via = (rt.via != Source::None).then(|| self.source(rt.via, sh, ctl));
                md[rt.target] += sampler_synth::route_value(rt, s, via);
            }
            // the loop targets: Loop Start / End move one point, Loop Position both
            if syn.has_target[target::LOOP_START] || syn.has_target[target::LOOP_END] || syn.has_target[target::LOOP_POS] {
                (ls, le) = moved_loop(ls0, le0, s0, e0, md[target::LOOP_START], md[target::LOOP_END], md[target::LOOP_POS]);
                looping = looping_mode && le > ls + 1;
                llen = (le - ls) as f64;
            }
            for k in 0..LFOS {
                lfo_rate_mod[k] = md[target::LFO_RATE + k];
                let ls_ = &syn.lfo[k];
                if !ls_.on {
                    self.lfo_val[k] = 0.0;
                    continue;
                }
                if ls_.poly {
                    let hz = ls_.rate(syn.bpm) * 2f32.powf(lfo_rate_mod[k] * 4.0);
                    // LFO Fade: the fade (ramp) time scaled by the target
                    let faded;
                    let ls_ = if syn.has_target[target::LFO_FADE + k] {
                        faded = sampler_synth::LfoSet { fade: ls_.fade * 2f32.powf(md[target::LFO_FADE + k] * 4.0), ..*ls_ };
                        &faded
                    } else {
                        ls_
                    };
                    self.lfo_val[k] = self.lfo[k].advance(ls_, hz, dt);
                } else {
                    self.lfo_val[k] = sh.lfo_mono_val[k];
                }
            }
            // glide toward the key
            let gt = (syn.glide * (1.0 + md[target::GLIDE] * 2.0).max(0.0)).max(0.0);
            if gt > 0.0 && (self.glide - self.glide_target).abs() > 1e-4 {
                let step = dt / gt * 12.0;
                let d = self.glide_target - self.glide;
                self.glide += d.clamp(-step, step);
            } else {
                self.glide = self.glide_target;
            }
            let vib = 1.0 + (sh.vib * ctl.mod_wheel * 0.006 * dsp::sin_cycles(sh.vib_phase)) as f64;
            let semis = self.glide + md[target::PITCH] * 24.0;
            let mut inc = self.base_inc(z, rate) * dsp::semis(semis) as f64 * ctl.bend_ratio as f64 * vib;
            if self.looped && z.loop_tune != 0.0 {
                inc *= dsp::semis(z.loop_tune / 100.0) as f64;
            }
            // filters for this block
            let fon = [syn.f[0].on, syn.f[1].on];
            let vel_cut = 1.0 + 3.0 * sh.vel_cut * (self.vel - 0.5);
            let mut drive = [0.0f32; 2];
            for k in 0..2 {
                if !fon[k] {
                    continue;
                }
                let fs = &syn.f[k];
                let (cm, rm, dm) = if k == 0 { (md[target::F1_CUT], md[target::F1_RES], md[target::F1_DRIVE]) } else { (md[target::F2_CUT], md[target::F2_RES], md[target::F2_DRIVE]) };
                let hz = fs.cutoff * 2f32.powf((cm * 5.0) + z.cutoff_offset) * if k == 0 { vel_cut.max(0.1) } else { 1.0 };
                self.filt[k].set(fs.kind, hz, fs.res + rm + z.res_offset, rate);
                drive[k] = (fs.drive + dm).clamp(0.0, 1.0);
            }
            let blend = ((syn.blend + md[target::BLEND] * 2.0).clamp(-1.0, 1.0) + 1.0) * 0.5;
            // amplitude: Envelope 1 (or the zone's own), Volume and Pan targets
            let amp_env = if z.synth_env { self.env[0].out } else { 1.0 };
            let vol = (1.0 + md[target::VOLUME]).max(0.0);
            if self.rel_decay > 0.0 {
                self.gain *= self.rel_decay.powf(m as f32);
            }
            let amp = amp_env * vol * self.gain;
            let pan = (z.pan + self.pan_off + syn.pan + md[target::PAN]).clamp(-1.0, 1.0);
            let (pl, pr) = ((1.0 - pan).sqrt(), (1.0 + pan).sqrt());
            self.pan_l = pl;
            self.pan_r = pr;
            let bits = md[target::BITS];
            let quant = if syn.has_target[target::BITS] { 2f32.powf((24.0 - bits.clamp(0.0, 1.0) * 22.0).max(1.0)) } else { 0.0 };
            // ---- audio rate
            let a0 = self.amp_prev;
            let mut stopped = false;
            for j in 0..m {
                let idx = i + j;
                if self.delay > 0 {
                    self.delay -= 1;
                    continue;
                }
                // loop and end handling in logical positions
                if looping && (self.looped || self.pos >= ls as f64) {
                    match z.loop_dir {
                        LoopDir::Forward => {
                            if self.pos >= le as f64 {
                                self.pos -= llen;
                                self.looped = true;
                            }
                        }
                        LoopDir::Reverse => {
                            if self.dir > 0.0 && self.pos >= le as f64 {
                                self.dir = -1.0;
                                self.pos = le as f64 - 1.0;
                                self.looped = true;
                            } else if self.dir < 0.0 && self.pos <= ls as f64 {
                                self.pos += llen - 1.0;
                            }
                        }
                        LoopDir::Alternate => {
                            if self.dir > 0.0 && self.pos >= le as f64 {
                                self.dir = -1.0;
                                self.pos = 2.0 * le as f64 - self.pos - 1.0;
                                self.looped = true;
                            } else if self.dir < 0.0 && self.pos <= ls as f64 {
                                self.dir = 1.0;
                                self.pos = 2.0 * ls as f64 - self.pos;
                            }
                        }
                    }
                }
                if !looping && ((self.dir > 0.0 && self.pos >= e0 as f64 - 1.0) || (self.dir < 0.0 && self.pos <= s0 as f64)) {
                    stopped = true;
                    break;
                }
                let ip = self.pos as usize;
                let f = (self.pos - ip as f64) as f32;
                let wrap = looping && z.loop_dir == LoopDir::Forward;
                let at = |c: &Pcm, k: usize| -> f32 {
                    let mut k = k.max(s0).min(e0 - 1);
                    if wrap && k >= le {
                        k -= le - ls;
                    }
                    c.at(phys(k))
                };
                let read = |c: &Pcm| {
                    let xm1 = at(c, ip.saturating_sub(1));
                    dsp::hermite(xm1, at(c, ip), at(c, ip + 1), at(c, ip + 2), f)
                };
                let mut a = read(&data.ch[0]);
                let mut b = if stereo { read(&data.ch[1]) } else { a };
                // the loop crossfade: the audio before the loop start blends in toward the loop end
                if wrap && z.loop_xfade > 0 && self.pos >= (le - z.loop_xfade.min(le - ls)) as f64 {
                    let xf = z.loop_xfade.min(le - ls) as f32;
                    let t = ((self.pos - (le as f64 - xf as f64)) as f32 / xf).clamp(0.0, 1.0);
                    let (go, gi) = if z.loop_eq_power { ((t * std::f32::consts::FRAC_PI_2).cos(), (t * std::f32::consts::FRAC_PI_2).sin()) } else { (1.0 - t, t) };
                    let back = (self.pos - llen).max(s0 as f64);
                    let bi = back as usize;
                    let bf = (back - bi as f64) as f32;
                    let rb = |c: &Pcm| dsp::lerp(c.at(phys(bi.min(e0 - 1))), c.at(phys((bi + 1).min(e0 - 1))), bf);
                    a = a * go + rb(&data.ch[0]) * gi;
                    b = if stereo { b * go + rb(&data.ch[1]) * gi } else { a };
                }
                // fades at the sample start and end
                let q = self.pos as usize;
                let mut fg = 1.0;
                if z.fade_in > 0 && !self.looped && q < s0 + z.fade_in {
                    fg *= (q - s0) as f32 / z.fade_in as f32;
                }
                if z.fade_out > 0 && !looping && q + z.fade_out > e0 {
                    fg *= (e0 - q) as f32 / z.fade_out as f32;
                }
                if self.zfilt {
                    a = self.zsvf[0].run(a);
                    b = if stereo { self.zsvf[1].run(b) } else { a };
                }
                // the Synth pane's filters
                if fon[0] || fon[1] {
                    let (fa, fb) = if syn.parallel && fon[0] && fon[1] {
                        let mono = (a + b) * 0.5;
                        let y1 = self.filt[0].run(0, mono, drive[0]);
                        let y2 = self.filt[1].run(0, mono, drive[1]);
                        let y = y1 * (1.0 - blend) + y2 * blend;
                        (y, y)
                    } else if fon[0] && fon[1] {
                        let y1a = self.filt[0].run(0, a, drive[0]);
                        let y1b = if stereo { self.filt[0].run(1, b, drive[0]) } else { y1a };
                        let y2a = self.filt[1].run(0, y1a, drive[1]);
                        let y2b = if stereo { self.filt[1].run(1, y1b, drive[1]) } else { y2a };
                        (dsp::lerp(y1a, y2a, blend), dsp::lerp(y1b, y2b, blend))
                    } else {
                        let k = if fon[0] { 0 } else { 1 };
                        let ya = self.filt[k].run(0, a, drive[k]);
                        let yb = if stereo { self.filt[k].run(1, b, drive[k]) } else { ya };
                        // with one filter Blend crossfades it with the dry sound
                        let wet = if k == 0 { 1.0 - blend } else { blend };
                        (dsp::lerp(a, ya, wet), dsp::lerp(b, yb, wet))
                    };
                    a = fa;
                    b = fb;
                }
                if quant > 0.0 {
                    a = (a * quant).round() / quant;
                    b = (b * quant).round() / quant;
                }
                let ramp = a0 + (amp - a0) * (j as f32 + 1.0) / m as f32;
                let mut g = ramp * fg * if z.synth_env { 1.0 } else { self.zenv.next() };
                if self.killing {
                    g *= self.fade.next();
                }
                let yl = a * g * pl;
                let yr = b * g * pr;
                peak = peak.max(yl.abs().max(yr.abs()));
                l[idx] += yl;
                r[idx] += yr;
                self.pos += inc * self.dir;
            }
            self.amp_prev = amp;
            if stopped {
                self.on = false;
                break;
            }
            i += m;
        }
        self.level = peak;
        let env_done = if z.synth_env { self.env[0].idle() } else { self.zenv.idle() };
        if env_done || (self.killing && self.fade.gain <= 0.0) {
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

impl SamplerVoice {
    #[inline]
    fn base_inc(&self, z: &Zone, rate: f32) -> f64 {
        (z.rate / rate) as f64
    }
}

pub struct Sampler {
    params: Params,
    poly: Poly<SamplerVoice>,
    ch: Channel,
    sh: SamplerShared,
    rate: f32,
    rr: [u32; 128],
    rng: dsp::Rng,
    held_vel: [u8; 128],
    has_release_zones: bool,
    /// Mono / Legato: the keys held, last on top.
    held: [u8; 16],
    nheld: usize,
    /// The last Enable by Note keyswitch played; the note's articulation (0: none).
    switch_note: Option<u8>,
    articulation: u8,
    counter: u64,
    /// The keys notes were shifted to (Transpose), by the key played.
    shifted: [u8; 128],
    pub name: String,
    /// The editor's instrument revision these zones came from (0: not from the editor).
    pub doc_rev: u64,
}

impl Sampler {
    pub fn new(zones: Vec<Zone>, rate: f32) -> Sampler {
        dsp::warm_tables();
        let has_release_zones = zones.iter().any(|z| z.trigger == Trigger::Release);
        let mut s = Sampler {
            params: Params::new(all_params()),
            poly: Poly::new(64, SamplerVoice::new),
            ch: Channel::new(rate),
            sh: SamplerShared {
                rate,
                zones,
                groups: Vec::new(),
                env_scale: [1.0; 4],
                vel_cut: 0.0,
                vel_sens: 1.0,
                transpose: 0.0,
                tune: 0.0,
                vib: 0.3,
                vib_phase: 0.0,
                syn: Synth::default(),
                lfo_mono: [Lfo::default(); LFOS],
                lfo_mono_val: [0.0; LFOS],
                cc: [0.0; 128],
                pending: Pending::default(),
                npending: 0,
            },
            rate,
            rr: [0; 128],
            rng: dsp::Rng::new(4242),
            held_vel: [0; 128],
            has_release_zones,
            held: [0; 16],
            nheld: 0,
            switch_note: None,
            articulation: 0,
            counter: 0,
            shifted: std::array::from_fn(|i| i as u8),
            name: String::new(),
            doc_rev: 0,
        };
        s.poly.retrigger_kills = false;
        s.update();
        s
    }
    /// Replaces the zones (voices are stopped first; not for the audio thread).
    pub fn set_zones(&mut self, zones: Vec<Zone>) {
        for v in &mut self.poly.voices {
            v.on = false;
        }
        self.has_release_zones = zones.iter().any(|z| z.trigger == Trigger::Release);
        self.sh.zones = zones;
    }
    /// Adds a zone while voices play on (Quick Sampler's Flex makes one per key as notes
    /// come); returns its index.
    pub fn add_zone(&mut self, zone: Zone) -> usize {
        self.has_release_zones |= zone.trigger == Trigger::Release;
        self.sh.zones.push(zone);
        self.sh.zones.len() - 1
    }
    /// The group rules zones refer to by `layer`.
    pub fn set_groups(&mut self, groups: Vec<GroupRule>) {
        self.sh.groups = groups;
    }
    pub fn zones(&self) -> &[Zone] {
        &self.sh.zones
    }
    /// Where the sounding voices are: (zone index, frame), the newest last.
    pub fn play_positions(&self) -> Vec<(usize, f64)> {
        self.poly.voices.iter().filter(|v| v.on).map(|v| (v.zone, v.pos)).collect()
    }
    /// Voices sounding now (the Synth Details' Used Voices).
    pub fn used_voices(&self) -> usize {
        self.poly.voices.iter().filter(|v| v.on).count()
    }
    /// Every voice fades out at once (a choke: an exclusive group, a pad cutting another).
    pub fn choke(&mut self) {
        for v in &mut self.poly.voices {
            v.kill(self.rate);
        }
    }
    /// The articulation of the next notes (a note event's articulation ID; 0 none).
    pub fn set_articulation(&mut self, id: u8) {
        self.articulation = id;
    }
    fn update(&mut self) {
        let p = |i| self.params.get(i);
        self.sh.env_scale = [p(1), p(2), p(3), p(4)];
        self.sh.vel_cut = p(8);
        self.sh.vel_sens = p(9);
        self.sh.transpose = p(10);
        self.sh.tune = p(11);
        self.ch.bend_up = p(12);
        let down = p(sampler_synth::BEND_DOWN);
        self.ch.bend_down = if down < 0.0 { p(12) } else { down };
        self.ch.update_bend();
        self.sh.vib = p(14);
        let max = p(13) as usize;
        self.poly.set_max(max);
        let bpm = self.sh.syn.bpm;
        let params = &self.params;
        self.sh.syn.read(&|i| params.get(i));
        self.sh.syn.bpm = bpm;
    }

    /// Whether a group may play this note now (mute / solo, selection filters, round robin).
    fn group_ok(&self, layer: u16, counter: u32) -> bool {
        let Some(g) = self.sh.groups.get(layer as usize) else { return true };
        if g.mute {
            return false;
        }
        if !g.solo && self.sh.groups.iter().any(|x| x.solo) {
            return false;
        }
        if let Some(a) = g.articulation {
            let cur = if self.articulation > 0 { self.articulation } else { self.sh.groups.iter().filter_map(|x| x.articulation).min().unwrap_or(0) };
            if a != cur {
                return false;
            }
        }
        if let Some(n) = g.on_note {
            let cur = self.switch_note.or_else(|| self.sh.groups.iter().filter_map(|x| x.on_note).min());
            if Some(n) != cur {
                return false;
            }
        }
        if let Some((cc, lo, hi)) = g.on_control {
            let v = (self.sh.cc[cc as usize & 127] * 127.0).round() as u8;
            if v < lo || v > hi {
                return false;
            }
        }
        if let Some((lo, hi)) = g.on_bend {
            let b = (self.ch.ctl.bend / self.ch.bend_up.max(0.01) * 64.0).round().clamp(-64.0, 63.0) as i8;
            if b < lo || b > hi {
                return false;
            }
        }
        if let Some(c) = g.on_channel
            && c != 1
        {
            return false;
        }
        if let Some((lo, hi)) = g.on_tempo
            && (self.sh.syn.bpm < lo || self.sh.syn.bpm > hi)
        {
            return false;
        }
        if let Some((_, place, count)) = g.round_robin
            && count > 1
            && counter % count as u32 != place as u32
        {
            return false;
        }
        true
    }

    fn trigger(&mut self, note: u8, vel: u8, trig: Trigger, glide_from: Option<f32>, key_shift: f32) {
        let rand = self.rng.unit();
        let counter = self.rr[note as usize];
        let random = self.rng.unit();
        // velocity: offset and random, then the Sample Select value (Velocity → Sample Select by default)
        let syn = self.sh.syn;
        if trig == Trigger::Attack {
            for k in 0..LFOS {
                let s = syn.lfo[k];
                if s.on && !s.poly && s.key_trigger {
                    self.sh.lfo_mono[k].restart(&s, 99 + k as u32);
                }
            }
        }
        let vr = if syn.vel_random > 0.0 { self.rng.bipolar() * syn.vel_random } else { 0.0 };
        let v = (vel as f32 + syn.vel_offset + vr).clamp(1.0, 127.0);
        let vn = v / 127.0;
        let ctl = self.ch.ctl;
        let sel = if syn.has_target[target::SAMPLE_SELECT] {
            let s = note_on_mod(target::SAMPLE_SELECT, note, vn, random, &self.sh, &ctl) * 127.0;
            let r = if syn.sel_random > 0.0 { self.rng.bipolar() * syn.sel_random } else { 0.0 };
            (s + r).round().clamp(1.0, 127.0) as u8
        } else {
            v as u8
        };
        if syn.has_target[target::ARTICULATION] {
            let a = note_on_mod(target::ARTICULATION, note, vn, random, &self.sh, &ctl);
            let mut ids: [u8; 32] = [0; 32];
            let mut n = 0;
            for g in &self.sh.groups {
                if let Some(x) = g.articulation
                    && n < 32
                    && !ids[..n].contains(&x)
                {
                    ids[n] = x;
                    n += 1;
                }
            }
            ids[..n].sort_unstable();
            if n > 0 {
                self.articulation = ids[((a.clamp(0.0, 1.0) * (n - 1) as f32).round() as usize).min(n - 1)];
            }
        }
        let start_mod = if syn.has_target[target::SAMPLE_START] { note_on_mod(target::SAMPLE_START, note, vn, random, &self.sh, &ctl).max(0.0) } else { 0.0 };
        let tm = |t: usize, sh: &SamplerShared| if sh.syn.has_target[t] { 2f32.powf(note_on_mod(t, note, vn, random, sh, &ctl) * 4.0) } else { 1.0 };
        let times = [tm(target::ENV1_ATTACK, &self.sh), tm(target::ENV1_DECAY, &self.sh), tm(target::ENV1_RELEASE, &self.sh), tm(target::ENV_ALL, &self.sh)];
        let lfo_phase: [f32; LFOS] = std::array::from_fn(|k| if syn.has_target[target::LFO_PHASE + k] { note_on_mod(target::LFO_PHASE + k, note, vn, random, &self.sh, &ctl) } else { 0.0 });
        let unison = self.sh.syn.unison.max(1) as usize;
        let detune = self.sh.syn.detune;
        let mut chosen = [0usize; 16];
        let mut n = 0;
        for (i, z) in self.sh.zones.iter().enumerate() {
            if z.trigger != trig || !z.matches(note, sel) {
                continue;
            }
            if z.seq_length > 1 && (counter % z.seq_length) + 1 != z.seq_position {
                continue;
            }
            if rand < z.lorand || rand >= z.hirand.max(z.lorand + 1e-6) && z.hirand < 1.0 {
                continue;
            }
            if !self.group_ok(z.layer, counter) {
                continue;
            }
            if n < chosen.len() {
                chosen[n] = i;
                n += 1;
            }
        }
        self.rr[note as usize] = counter.wrapping_add(1);
        self.poly.stacking = false;
        for &zi in &chosen[..n] {
            // exclusive groups: this zone cuts voices of the groups it names
            let z = &self.sh.zones[zi];
            let grp = z.group;
            if grp != 0 {
                for v in &mut self.poly.voices {
                    if v.on && v.off_by == grp {
                        v.kill(self.rate);
                    }
                }
            }
            let layer = z.layer;
            let xfade = z.xfade(note, sel);
            if let Some(g) = self.sh.groups.get(layer as usize).copied() {
                // exclusive classes between groups, voice limits within one
                if g.exclusive != 0 {
                    for v in &mut self.poly.voices {
                        if v.on && v.layer != layer && self.sh.groups.get(v.layer as usize).is_some_and(|o| o.exclusive == g.exclusive) {
                            v.kill(self.rate);
                        }
                    }
                }
                if g.voices > 0 {
                    loop {
                        let live = self.poly.voices.iter().filter(|v| v.on && !v.killing && v.layer == layer).count();
                        if live < g.voices as usize {
                            break;
                        }
                        let oldest = self.poly.voices.iter_mut().filter(|v| v.on && !v.killing && v.layer == layer).min_by_key(|v| v.age);
                        match oldest {
                            Some(v) => v.kill(self.rate),
                            None => break,
                        }
                    }
                }
            }
            for u in 0..unison {
                let (dt, pan) = if unison > 1 {
                    let x = u as f32 / (unison - 1) as f32 * 2.0 - 1.0;
                    (x * (10.0 + detune * 0.5) + self.rng.bipolar() * detune * 0.5, x)
                } else if detune > 0.0 {
                    (self.rng.bipolar() * detune, 0.0)
                } else {
                    (0.0, 0.0)
                };
                self.sh.pending = Pending { zone: zi, detune: dt, pan, xfade, start_mod, times, lfo_phase, glide_from, random, vel: vn, key_shift };
                self.sh.npending = 1;
                self.counter += 1;
                self.poly.note_on(note, vel, &self.sh, &self.ch.ctl);
                // the other layers and unison voices join this one (the editor's instruments;
                // SFZ / SoundFont layers keep the engine's older one-voice-per-key handover)
                self.poly.stacking = self.sh.zones[zi].synth_env || unison > 1;
                let c = self.counter;
                for v in &mut self.poly.voices {
                    if v.on && v.age == 0 {
                        v.age = c;
                    }
                }
                self.sh.npending = 0;
            }
        }
        self.poly.stacking = false;
    }

    fn mono(&self) -> bool {
        self.sh.syn.mode != 0
    }
}

impl Plugin for Sampler {
    fn params(&self) -> &[Param] {
        all_params()
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
        self.ch.reset();
        self.nheld = 0;
        self.update();
    }
    fn tail(&self) -> f32 {
        let zone_tail = self.sh.zones.iter().filter(|z| !z.synth_env).map(|z| z.env[5]).fold(0.0, f32::max).min(10.0) * self.params.get(4);
        let synth_tail = if self.sh.zones.iter().any(|z| z.synth_env) { self.sh.syn.env[0].release.min(20.0) } else { 0.0 };
        zone_tail.max(synth_tail)
    }
}

impl Instrument for Sampler {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        let shift = self.params.get(sampler_synth::TRANSPOSE_NOTES).round() as i32;
        let played = pitch;
        let pitch = (pitch as i32 + shift).clamp(0, 127) as u8;
        self.shifted[played as usize] = pitch;
        // Coarse Tune Remote: keys within an octave of its key transpose the instrument
        let remote = self.sh.syn.coarse_remote;
        if remote >= 0 && (pitch as i32 - remote).abs() <= 12 {
            self.params.set(10, (pitch as i32 - remote) as f32);
            self.update();
            return;
        }
        if self.sh.groups.iter().any(|g| g.on_note == Some(pitch)) {
            self.switch_note = Some(pitch);
        }
        self.held_vel[pitch as usize] = velocity;
        if !self.mono() {
            self.trigger(pitch, velocity, Trigger::Attack, None, 0.0);
            return;
        }
        // Mono / Legato: one note sounds; the last key held wins
        let others = self.nheld > 0;
        if self.nheld < self.held.len() {
            self.held[self.nheld] = pitch;
            self.nheld += 1;
        }
        let sounding = self.poly.voices.iter().any(|v| v.on && !v.released && !v.killing);
        if self.sh.syn.mode == 2 && others && sounding {
            for v in &mut self.poly.voices {
                if v.on && !v.killing {
                    v.retarget(pitch, &self.sh);
                }
            }
            return;
        }
        // glide from the last note's pitch (Mono: always; Legato: only tied notes, above)
        let from = if self.sh.syn.mode == 1 && self.sh.syn.glide > 0.0 {
            self.poly.voices.iter().find(|v| v.on && !v.killing).map(|v| v.glide)
        } else {
            None
        };
        for v in &mut self.poly.voices {
            if v.on {
                v.kill(self.rate);
            }
        }
        self.trigger(pitch, velocity, Trigger::Attack, from, 0.0);
    }
    fn note_off(&mut self, pitch: u8) {
        let pitch = std::mem::replace(&mut self.shifted[pitch as usize], pitch);
        if self.mono() {
            let was_top = self.nheld > 0 && self.held[self.nheld - 1] == pitch;
            let mut k = 0;
            for i in 0..self.nheld {
                if self.held[i] != pitch {
                    self.held[k] = self.held[i];
                    k += 1;
                }
            }
            self.nheld = k;
            if was_top && self.nheld > 0 {
                let back = self.held[self.nheld - 1];
                if self.sh.syn.mode == 2 {
                    for v in &mut self.poly.voices {
                        if v.on && !v.killing {
                            v.retarget(back, &self.sh);
                        }
                    }
                } else {
                    let vel = self.held_vel[back as usize].max(1);
                    for v in &mut self.poly.voices {
                        if v.on {
                            v.kill(self.rate);
                        }
                    }
                    self.trigger(back, vel, Trigger::Attack, None, 0.0);
                }
                return;
            }
            // the voices may play another key now (a legato retarget): release them all
            if self.nheld == 0 {
                for v in &mut self.poly.voices {
                    if v.on && !v.released {
                        v.release(&self.sh, &self.ch.ctl);
                    }
                }
            }
        }
        self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
        // release samples (key-up noises) play when the key really lets go of the string: with
        // the sustain pedal down there is none (and a new voice on the key would cut the held one)
        if self.has_release_zones && !self.ch.ctl.sustain {
            let v = self.held_vel[pitch as usize].max(1);
            self.trigger(pitch, v, Trigger::Release, None, 0.0);
        }
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            self.nheld = 0;
            return self.poly.all_notes_off(&self.sh, &self.ch.ctl);
        }
        self.sh.cc[cc as usize & 127] = value as f32 / 127.0;
        if let Some(d) = self.ch.cc(cc, value) {
            self.poly.sustain(d, &self.sh, &self.ch.ctl);
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn set_tempo(&mut self, bpm: f64) {
        self.sh.syn.bpm = bpm as f32;
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
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut sl = [0.0f32; CHUNK];
        let mut sr = [0.0f32; CHUNK];
        let (gl, gr) = self.ch.out_gains();
        let vol = lg_dsp::db_to_gain(self.params.get(0));
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            self.sh.vib_phase = (self.sh.vib_phase + 5.5 * m as f32 / self.rate).fract();
            // the mono LFOs: one for every voice
            let dt = m as f32 / self.rate;
            for k in 0..LFOS {
                let s = self.sh.syn.lfo[k];
                if s.on && !s.poly {
                    let hz = s.rate(self.sh.syn.bpm);
                    self.sh.lfo_mono_val[k] = self.sh.lfo_mono[k].advance(&s, hz, dt);
                }
            }
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            for i in 0..m {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += m;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Loading helpers

type SampleEntry = (Arc<SampleData>, Option<(u32, u32)>, Option<u8>);

type SampleCache = std::collections::HashMap<
    PathBuf,
    (std::sync::Weak<SampleData>, Option<(u32, u32)>, Option<u8>),
>;

/// Samples already in memory, shared by every instrument that uses them (weak: a sample is freed
/// when no instrument holds it).
fn sample_cache() -> &'static std::sync::Mutex<SampleCache> {
    static C: std::sync::OnceLock<std::sync::Mutex<SampleCache>> = std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

pub(crate) fn load_sample(path: &Path) -> std::io::Result<SampleEntry> {
    if let Ok(c) = sample_cache().lock()
        && let Some((w, lp, root)) = c.get(path)
        && let Some(a) = w.upgrade()
    {
        return Ok((a, *lp, *root));
    }
    let bytes = std::fs::read(path)?;
    let e = decode_sample(&bytes).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("unsupported sample format: {}", path.display()),
        )
    })?;
    if let Ok(mut c) = sample_cache().lock() {
        c.insert(path.to_path_buf(), (Arc::downgrade(&e.0), e.1, e.2));
    }
    Ok(e)
}

fn decode_sample(bytes: &[u8]) -> Option<SampleEntry> {
    if bytes.starts_with(b"fLaC") {
        let (rate, ch) = decode_flac(bytes)?;
        return Some((Arc::new(SampleData { rate: rate as f32, ch }), None, None));
    }
    let a = wav::decode_audio(bytes).or_else(|| decode_caf(bytes))?;
    let ch = a.channels.into_iter().take(2).map(Pcm::F32).collect();
    Some((
        Arc::new(SampleData {
            rate: a.rate as f32,
            ch,
        }),
        a.loop_points,
        a.root,
    ))
}

/// Loads several samples on all cores (the bundled instruments have hundreds of files).
pub(crate) fn load_samples(paths: &[PathBuf]) -> Vec<Option<SampleEntry>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let n = paths.len();
    let threads = std::thread::available_parallelism()
        .map(|t| t.get())
        .unwrap_or(4)
        .clamp(1, 8)
        .min(n.max(1));
    if threads <= 1 || n < 4 {
        return paths.iter().map(|p| load_sample(p).ok()).collect();
    }
    let next = AtomicUsize::new(0);
    let mut out: Vec<Option<SampleEntry>> = vec![None; n];
    let results: Vec<Vec<(usize, Option<SampleEntry>)>> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .map(|_| {
                sc.spawn(|| {
                    let mut got = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= n {
                            break;
                        }
                        got.push((i, load_sample(&paths[i]).ok()));
                    }
                    got
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap_or_default()).collect()
    });
    for (i, e) in results.into_iter().flatten() {
        out[i] = e;
    }
    out
}

/// FLAC (any bit depth): 16-bit and below stay 16-bit in memory, deeper files become float.
fn decode_flac(bytes: &[u8]) -> Option<(u32, Vec<Pcm>)> {
    use symphonia::core::audio::{AudioBufferRef, Signal};
    use symphonia::core::codecs::{Decoder, DecoderOptions};
    use symphonia::core::formats::{FormatOptions, FormatReader};
    use symphonia::core::io::MediaSourceStream;
    let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes.to_vec())), Default::default());
    let mut reader = symphonia::default::formats::FlacReader::try_new(mss, &FormatOptions::default()).ok()?;
    let track = reader.default_track()?.clone();
    let params = &track.codec_params;
    let rate = params.sample_rate?;
    let nch = params.channels.map(|c| c.count()).unwrap_or(1).clamp(1, 2);
    let bits = params.bits_per_sample.unwrap_or(16);
    let frames = params.n_frames.unwrap_or(0) as usize;
    let mut dec = symphonia::default::codecs::FlacDecoder::try_new(params, &DecoderOptions::default()).ok()?;
    let mut i16s: Vec<Vec<i16>> = (0..nch).map(|_| Vec::with_capacity(frames)).collect();
    let mut f32s: Vec<Vec<f32>> = Vec::new();
    let wide = bits > 16;
    if wide {
        f32s = (0..nch).map(|_| Vec::with_capacity(frames)).collect();
    }
    while let Ok(packet) = reader.next_packet() {
        let buf = match dec.decode(&packet) {
            Ok(b) => b,
            Err(_) => continue,
        };
        match buf {
            AudioBufferRef::S16(b) => {
                for c in 0..nch.min(b.spec().channels.count()) {
                    if wide {
                        f32s[c].extend(b.chan(c).iter().map(|&x| x as f32 / 32768.0));
                    } else {
                        i16s[c].extend_from_slice(b.chan(c));
                    }
                }
            }
            // symphonia's FLAC decoder gives left-justified 32-bit samples
            AudioBufferRef::S32(b) => {
                for c in 0..nch.min(b.spec().channels.count()) {
                    let src = b.chan(c);
                    if wide {
                        f32s[c].extend(src.iter().map(|&x| x as f32 / 2_147_483_648.0));
                    } else {
                        i16s[c].extend(src.iter().map(|&x| (x >> 16) as i16));
                    }
                }
            }
            _ => return None,
        }
    }
    let ch: Vec<Pcm> = if wide {
        f32s.into_iter().map(Pcm::F32).collect()
    } else {
        i16s.into_iter().map(Pcm::I16).collect()
    };
    if ch.first().map(|c| c.len()).unwrap_or(0) == 0 {
        return None;
    }
    Some((rate, ch))
}

/// Core Audio Format with linear PCM ('lpcm') data.
fn decode_caf(b: &[u8]) -> Option<wav::Audio> {
    if b.len() < 8 || &b[0..4] != b"caff" {
        return None;
    }
    let mut o = 8;
    let mut rate = 0f64;
    let (mut fmt, mut flags, mut ch, mut bits) = ([0u8; 4], 0u32, 0u32, 0u32);
    let mut data: Option<&[u8]> = None;
    while o + 12 <= b.len() {
        let id = &b[o..o + 4];
        let sz = i64::from_be_bytes(b[o + 4..o + 12].try_into().ok()?);
        let start = o + 12;
        let end = if sz < 0 {
            b.len()
        } else {
            (start + sz as usize).min(b.len())
        };
        let body = &b[start..end];
        match id {
            b"desc" if body.len() >= 32 => {
                rate = f64::from_be_bytes(body[0..8].try_into().ok()?);
                fmt.copy_from_slice(&body[8..12]);
                flags = u32::from_be_bytes(body[12..16].try_into().ok()?);
                ch = u32::from_be_bytes(body[24..28].try_into().ok()?);
                bits = u32::from_be_bytes(body[28..32].try_into().ok()?);
            }
            b"data" if body.len() >= 4 => data = Some(&body[4..]),
            _ => {}
        }
        o = end;
    }
    if &fmt != b"lpcm" || ch == 0 {
        return None;
    }
    let float = flags & 1 != 0;
    let little = flags & 2 != 0;
    let data = data?;
    let bps = (bits as usize).div_ceil(8);
    let frames = data.len() / (bps * ch as usize);
    let mut out = vec![Vec::with_capacity(frames); ch as usize];
    for f in 0..frames {
        for (c, o) in out.iter_mut().enumerate() {
            let p = (f * ch as usize + c) * bps;
            let s = &data[p..p + bps];
            let v = match (bps, float) {
                (2, _) => {
                    (if little {
                        i16::from_le_bytes([s[0], s[1]])
                    } else {
                        i16::from_be_bytes([s[0], s[1]])
                    }) as f32
                        / 32768.0
                }
                (3, _) => {
                    let (a, b2, c2) = if little {
                        (s[0], s[1], s[2])
                    } else {
                        (s[2], s[1], s[0])
                    };
                    ((((a as i32) | ((b2 as i32) << 8) | ((c2 as i32) << 16)) << 8) >> 8) as f32
                        / 8_388_608.0
                }
                (4, true) => {
                    let a = [s[0], s[1], s[2], s[3]];
                    if little {
                        f32::from_le_bytes(a)
                    } else {
                        f32::from_be_bytes(a)
                    }
                }
                (4, false) => {
                    let a = [s[0], s[1], s[2], s[3]];
                    (if little {
                        i32::from_le_bytes(a)
                    } else {
                        i32::from_be_bytes(a)
                    }) as f32
                        / 2_147_483_648.0
                }
                _ => 0.0,
            };
            o.push(v);
        }
    }
    Some(wav::Audio {
        rate: rate as u32,
        channels: out,
        loop_points: None,
        root: None,
    })
}

/// SFZ note names (c4 = 60) or numbers.
fn sfz_note(v: &str) -> Option<u8> {
    let v = v.trim().to_ascii_lowercase();
    if let Ok(n) = v.parse::<i32>() {
        return Some(n.clamp(0, 127) as u8);
    }
    let b = v.as_bytes();
    let base = match b.first()? {
        b'c' => 0,
        b'd' => 2,
        b'e' => 4,
        b'f' => 5,
        b'g' => 7,
        b'a' => 9,
        b'b' => 11,
        _ => return None,
    };
    let mut i = 1;
    let mut acc = 0;
    if i < b.len() && (b[i] == b'#' || b[i] == b's') {
        acc = 1;
        i += 1;
    } else if i < b.len() && b[i] == b'b' && b.len() > 2 {
        acc = -1;
        i += 1;
    }
    let oct: i32 = v[i..].parse().ok()?;
    Some(((oct + 1) * 12 + base + acc).clamp(0, 127) as u8)
}

impl Sampler {
    /// Loads an SFZ instrument (samples relative to the file / `default_path`).
    pub fn from_sfz(path: &Path, rate: f32) -> std::io::Result<Sampler> {
        let text = std::fs::read_to_string(path)?;
        let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let zones = parse_sfz(&text, &base)?;
        if zones.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "SFZ has no playable regions",
            ));
        }
        let mut s = Sampler::new(zones, rate);
        s.name = path
            .file_stem()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(s)
    }
}

pub fn parse_sfz(text: &str, base: &Path) -> std::io::Result<Vec<Zone>> {
    use std::collections::HashMap;
    // strip comments, tokenise into headers and opcode=value pairs (values may contain spaces)
    let mut clean = String::with_capacity(text.len());
    for line in text.lines() {
        let l = match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        };
        if l.trim_start().starts_with('#') {
            continue;
        }
        clean.push_str(l);
        clean.push('\n');
    }
    #[derive(Debug)]
    enum Tok {
        Header(String),
        Op(String, String),
    }
    let mut toks = Vec::new();
    let s = clean.as_str();
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == b'<' {
            let j = s[i..].find('>').map(|k| i + k).unwrap_or(bytes.len() - 1);
            toks.push(Tok::Header(s[i + 1..j].trim().to_string()));
            i = j + 1;
            continue;
        }
        // opcode
        let eq = match s[i..].find('=') {
            Some(k) => i + k,
            None => break,
        };
        let key = s[i..eq].trim().to_string();
        // value runs to the next " name=" or "<" or end of line
        let rest = &s[eq + 1..];
        let line_end = rest.find('\n').unwrap_or(rest.len());
        let seg = &rest[..line_end];
        let mut vend = seg.len();
        if let Some(h) = seg.find('<') {
            vend = vend.min(h);
        }
        // find next opcode start: whitespace followed by token and '='
        let segb = seg.as_bytes();
        let mut k = 0;
        while k < vend {
            if segb[k].is_ascii_whitespace() {
                let after = &seg[k..vend].trim_start();
                if let Some(e) = after.find('=') {
                    let name = &after[..e];
                    if !name.is_empty() && !name.contains(char::is_whitespace) && key != "sample" {
                        vend = k;
                        break;
                    }
                    if key == "sample"
                        && !name.is_empty()
                        && !name.contains(char::is_whitespace)
                        && !name.contains('.')
                    {
                        vend = k;
                        break;
                    }
                }
            }
            k += 1;
        }
        let val = seg[..vend].trim().to_string();
        toks.push(Tok::Op(key, val));
        i = eq + 1 + vend;
    }
    let mut control: HashMap<String, String> = HashMap::new();
    let mut global: HashMap<String, String> = HashMap::new();
    let mut master: HashMap<String, String> = HashMap::new();
    let mut group: HashMap<String, String> = HashMap::new();
    let mut region: Option<HashMap<String, String>> = None;
    let mut level = "";
    let mut regions: Vec<HashMap<String, String>> = Vec::new();
    for t in toks {
        match t {
            Tok::Header(h) => {
                if let Some(r) = region.take() {
                    regions.push(r);
                }
                match h.as_str() {
                    "control" => level = "control",
                    "global" => {
                        level = "global";
                        global.clear();
                        master.clear();
                        group.clear();
                    }
                    "master" => {
                        level = "master";
                        master.clear();
                        group.clear();
                    }
                    "group" => {
                        level = "group";
                        group.clear();
                    }
                    "region" => {
                        level = "region";
                        let mut m = global.clone();
                        m.extend(master.clone());
                        m.extend(group.clone());
                        region = Some(m);
                    }
                    _ => level = "",
                }
            }
            Tok::Op(k, v) => match level {
                "control" => {
                    control.insert(k, v);
                }
                "global" => {
                    global.insert(k, v);
                }
                "master" => {
                    master.insert(k, v);
                }
                "group" => {
                    group.insert(k, v);
                }
                "region" => {
                    if let Some(r) = region.as_mut() {
                        r.insert(k, v);
                    }
                }
                _ => {}
            },
        }
    }
    if let Some(r) = region.take() {
        regions.push(r);
    }
    let default_path = control.get("default_path").cloned().unwrap_or_default();
    let note_offset: i32 = control
        .get("note_offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let path_of = |r: &HashMap<String, String>| -> Option<PathBuf> {
        let sample = r.get("sample")?;
        if sample.starts_with('*') {
            return None; // generated waveforms (*sine…) are not supported
        }
        Some(base.join(format!("{}{}", default_path, sample).replace('\\', "/")))
    };
    // decode every sample once, on all cores
    let mut unique: Vec<PathBuf> = regions.iter().filter_map(path_of).collect();
    unique.sort();
    unique.dedup();
    let mut cache: HashMap<PathBuf, SampleEntry> = HashMap::new();
    for (p, e) in unique.iter().zip(load_samples(&unique)) {
        if let Some(e) = e {
            cache.insert(p.clone(), e);
        }
    }
    let mut zones = Vec::new();
    for r in regions {
        let Some(p) = path_of(&r) else {
            continue;
        };
        let Some(entry) = cache.get(&p).cloned() else {
            continue;
        };
        let (data, lp, root) = entry;
        let mut z = Zone::new(data);
        let f = |k: &str| r.get(k).and_then(|v| v.trim().parse::<f32>().ok());
        let n = |k: &str| {
            r.get(k)
                .and_then(|v| sfz_note(v))
                .map(|x| (x as i32 + note_offset).clamp(0, 127) as u8)
        };
        if let Some(k) = n("key") {
            z.lokey = k;
            z.hikey = k;
            z.root = k;
        }
        if let Some(k) = n("lokey") {
            z.lokey = k;
        }
        if let Some(k) = n("hikey") {
            z.hikey = k;
        }
        if let Some(k) = n("pitch_keycenter") {
            z.root = k;
        } else if r.get("key").is_none() {
            if let Some(rk) = root {
                z.root = rk;
            }
        }
        if let Some(v) = f("lovel") {
            z.lovel = v as u8;
        }
        if let Some(v) = f("hivel") {
            z.hivel = v as u8;
        }
        z.tune_cents = f("tune").unwrap_or(0.0) + f("transpose").unwrap_or(0.0) * 100.0;
        z.keytrack = f("pitch_keytrack").unwrap_or(100.0);
        z.gain = lg_dsp::db_to_gain(f("volume").unwrap_or(0.0)) * f("amplitude").unwrap_or(100.0)
            / 100.0;
        z.pan = f("pan").unwrap_or(0.0) / 100.0;
        z.start = f("offset").unwrap_or(0.0) as usize;
        if let Some(e) = f("end") {
            z.end = (e as usize + 1).min(z.end);
        }
        if let Some((a, b)) = lp {
            z.loop_start = a as usize;
            z.loop_end = b as usize;
            z.loop_mode = LoopMode::Continuous;
        }
        if let Some(v) = f("loop_start").or(f("loopstart")) {
            z.loop_start = v as usize;
        }
        if let Some(v) = f("loop_end").or(f("loopend")) {
            z.loop_end = v as usize + 1;
        }
        z.loop_mode = match r.get("loop_mode").or(r.get("loopmode")).map(|s| s.as_str()) {
            Some("no_loop") => LoopMode::NoLoop,
            Some("one_shot") => LoopMode::OneShot,
            Some("loop_continuous") => LoopMode::Continuous,
            Some("loop_sustain") => LoopMode::Sustain,
            _ => z.loop_mode,
        };
        z.env = [
            f("ampeg_delay").unwrap_or(0.0),
            f("ampeg_attack").unwrap_or(0.001),
            f("ampeg_hold").unwrap_or(0.0),
            f("ampeg_decay").unwrap_or(0.0),
            f("ampeg_sustain").unwrap_or(100.0) / 100.0,
            f("ampeg_release").unwrap_or(0.05),
        ];
        if let Some(c) = f("cutoff") {
            z.cutoff = Some(c);
        }
        z.resonance = f("resonance").unwrap_or(0.0);
        z.filter = match r.get("fil_type").map(|s| s.as_str()) {
            Some(t) if t.starts_with("hpf") => SvfMode::High,
            Some(t) if t.starts_with("bpf") => SvfMode::Band,
            Some(t) if t.starts_with("brf") => SvfMode::Notch,
            _ => SvfMode::Low,
        };
        z.vel_track = f("amp_veltrack").unwrap_or(100.0) / 100.0;
        z.group = f("group").unwrap_or(0.0) as i32;
        z.off_by = f("off_by").unwrap_or(0.0) as i32;
        z.seq_length = f("seq_length").unwrap_or(1.0) as u32;
        z.seq_position = f("seq_position").unwrap_or(1.0) as u32;
        z.lorand = f("lorand").unwrap_or(0.0);
        z.hirand = f("hirand").unwrap_or(1.0);
        z.trigger = if r.get("trigger").map(|s| s == "release").unwrap_or(false) {
            Trigger::Release
        } else {
            Trigger::Attack
        };
        zones.push(z);
    }
    Ok(zones)
}

// ---------------------------------------------------------------------------------------------
// SoundFont 2

/// Lists a SoundFont's presets: (bank, program, name).
pub fn sf2_presets(path: &Path) -> std::io::Result<Vec<(i32, i32, String)>> {
    let sf = open_sf2(path)?;
    Ok(sf
        .get_presets()
        .iter()
        .map(|p| {
            (
                p.get_bank_number(),
                p.get_patch_number(),
                p.get_name().to_string(),
            )
        })
        .collect())
}

fn open_sf2(path: &Path) -> std::io::Result<rustysynth::SoundFont> {
    let mut f = std::io::BufReader::new(std::fs::File::open(path)?);
    rustysynth::SoundFont::new(&mut f)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e:?}")))
}

impl Sampler {
    /// "Apple-style": converts one SoundFont preset into Sampler zones.
    pub fn from_sf2(path: &Path, bank: i32, program: i32, rate: f32) -> std::io::Result<Sampler> {
        let sf = open_sf2(path)?;
        let preset = sf
            .get_presets()
            .iter()
            .find(|p| p.get_bank_number() == bank && p.get_patch_number() == program)
            .or_else(|| sf.get_presets().first())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "SoundFont has no presets")
            })?;
        let headers = sf.get_sample_headers();
        let pool = Arc::new(SampleData {
            rate: 44100.0,
            ch: vec![Pcm::I16(sf.get_wave_data().to_vec())],
        });
        let mut zones = Vec::new();
        for pr in preset.get_regions() {
            let inst = &sf.get_instruments()[pr.get_instrument_id()];
            for ir in inst.get_regions() {
                let lokey = ir.get_key_range_start().max(pr.get_key_range_start());
                let hikey = ir.get_key_range_end().min(pr.get_key_range_end());
                let lovel = ir
                    .get_velocity_range_start()
                    .max(pr.get_velocity_range_start());
                let hivel = ir.get_velocity_range_end().min(pr.get_velocity_range_end());
                if lokey > hikey || lovel > hivel {
                    continue;
                }
                let sh = &headers[ir.get_sample_id().min(headers.len().saturating_sub(1))];
                let mut z = Zone::new(pool.clone());
                z.rate = sh.get_sample_rate().max(1) as f32;
                z.lokey = lokey.clamp(0, 127) as u8;
                z.hikey = hikey.clamp(0, 127) as u8;
                z.lovel = lovel.clamp(0, 127).max(1) as u8;
                z.hivel = hivel.clamp(0, 127) as u8;
                z.root = ir.get_root_key().clamp(0, 127) as u8;
                z.tune_cents = ((ir.get_coarse_tune() + pr.get_coarse_tune()) * 100
                    + ir.get_fine_tune()
                    + pr.get_fine_tune()) as f32;
                z.keytrack = (ir.get_scale_tuning() + pr.get_scale_tuning()) as f32;
                // SoundFont attenuation is centibels; the usual 0.4 factor (EMU hardware practice)
                z.gain = lg_dsp::db_to_gain(
                    -0.4 * (ir.get_initial_attenuation() + pr.get_initial_attenuation()),
                );
                z.pan = ((ir.get_pan() + pr.get_pan()) / 50.0).clamp(-1.0, 1.0);
                z.start = ir.get_sample_start().max(0) as usize;
                z.end = ir.get_sample_end().max(0) as usize;
                z.loop_start = ir.get_sample_start_loop().max(0) as usize;
                z.loop_end = ir.get_sample_end_loop().max(0) as usize;
                z.loop_mode = match ir.get_sample_modes() {
                    rustysynth::LoopMode::Continuous => LoopMode::Continuous,
                    rustysynth::LoopMode::LoopUntilNoteOff => LoopMode::Sustain,
                    _ => LoopMode::NoLoop,
                };
                let sus_db = (ir.get_sustain_volume_envelope() + pr.get_sustain_volume_envelope())
                    .clamp(0.0, 144.0);
                z.env = [
                    ir.get_delay_volume_envelope() * pr.get_delay_volume_envelope(),
                    ir.get_attack_volume_envelope() * pr.get_attack_volume_envelope(),
                    ir.get_hold_volume_envelope() * pr.get_hold_volume_envelope(),
                    ir.get_decay_volume_envelope() * pr.get_decay_volume_envelope(),
                    lg_dsp::db_to_gain(-sus_db),
                    ir.get_release_volume_envelope() * pr.get_release_volume_envelope(),
                ];
                let fc = ir.get_initial_filter_cutoff_frequency()
                    * pr.get_initial_filter_cutoff_frequency()
                    / 8.176;
                if fc < 18000.0 {
                    z.cutoff = Some(fc);
                    z.resonance = ir.get_initial_filter_q() + pr.get_initial_filter_q();
                }
                z.group = ir.get_exclusive_class();
                z.off_by = ir.get_exclusive_class();
                zones.push(z);
            }
        }
        let mut s = Sampler::new(zones, rate);
        s.name = preset.get_name().to_string();
        Ok(s)
    }
}

/// "Native" SoundFont playback: the .sf2 played by rustysynth's own engine (its envelopes,
/// modulators, LFOs, reverb and chorus), on one MIDI channel.
pub struct SoundFontPlayer {
    synth: rustysynth::Synthesizer,
    bl: Vec<f32>,
    br: Vec<f32>,
    channel: i32,
    params: Params,
}

pub static SF_PARAMS: &[Param] = &[
    Param::new(0, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(1, "Bank", 0.0, 128.0, 0.0, Unit::None),
    Param::new(2, "Program", 0.0, 127.0, 0.0, Unit::None),
];

impl SoundFontPlayer {
    pub fn new(
        path: &Path,
        bank: i32,
        program: i32,
        rate: f32,
    ) -> std::io::Result<SoundFontPlayer> {
        let sf = Arc::new(open_sf2(path)?);
        let mut settings = rustysynth::SynthesizerSettings::new(rate as i32);
        settings.maximum_polyphony = 64;
        let synth = rustysynth::Synthesizer::new(&sf, &settings)
            .map_err(|e| std::io::Error::other(format!("{e:?}")))?;
        let mut p = SoundFontPlayer {
            synth,
            bl: vec![0.0; 8192],
            br: vec![0.0; 8192],
            channel: 0,
            params: Params::new(SF_PARAMS),
        };
        p.params.set(1, bank as f32);
        p.params.set(2, program as f32);
        p.select();
        Ok(p)
    }
    fn select(&mut self) {
        let bank = self.params.get(1) as i32;
        let prog = self.params.get(2) as i32;
        // percussion bank 128 lives on channel 10 in General MIDI SoundFonts
        self.channel = if bank == 128 { 9 } else { 0 };
        self.synth
            .process_midi_message(self.channel, 0xB0, 0x00, bank.min(127));
        self.synth.process_midi_message(self.channel, 0xC0, prog, 0);
        self.synth
            .set_master_volume(0.5 * lg_dsp::db_to_gain(self.params.get(0)));
    }
}

impl Plugin for SoundFontPlayer {
    fn params(&self) -> &[Param] {
        SF_PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            self.select();
        }
    }
    fn reset(&mut self) {
        self.synth.reset();
        self.select();
    }
    fn tail(&self) -> f32 {
        2.0
    }
}

impl Instrument for SoundFontPlayer {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        self.synth
            .note_on(self.channel, pitch as i32, velocity as i32);
    }
    fn note_off(&mut self, pitch: u8) {
        self.synth.note_off(self.channel, pitch as i32);
    }
    fn controller(&mut self, cc: u8, value: u8) {
        self.synth
            .process_midi_message(self.channel, 0xB0, cc as i32, value as i32);
    }
    fn pitch_bend(&mut self, value: i16) {
        let v = (value as i32 + 8192).clamp(0, 16383);
        self.synth
            .process_midi_message(self.channel, 0xE0, v & 0x7F, v >> 7);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(self.bl.len());
            self.synth.render(&mut self.bl[..m], &mut self.br[..m]);
            for i in 0..m {
                left[pos + i] += self.bl[i];
                right[pos + i] += self.br[i];
            }
            pos += m;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// EXS24

fn rd_u32(b: &[u8], o: usize, be: bool) -> u32 {
    if o + 4 > b.len() {
        return 0;
    }
    let a = [b[o], b[o + 1], b[o + 2], b[o + 3]];
    if be {
        u32::from_be_bytes(a)
    } else {
        u32::from_le_bytes(a)
    }
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// One EXS zone as decoded (before samples are resolved).
#[derive(Clone, Debug, Default)]
pub struct ExsZone {
    pub name: String,
    pub root: u8,
    pub fine: i8,
    pub coarse: i8,
    pub pan: i8,
    pub volume: i8,
    pub lokey: u8,
    pub hikey: u8,
    pub lovel: u8,
    pub hivel: u8,
    pub start: u32,
    pub end: u32,
    pub loop_start: u32,
    pub loop_end: u32,
    pub loop_on: bool,
    pub one_shot: bool,
    pub group: i32,
    pub sample: i32,
}

#[derive(Clone, Debug, Default)]
pub struct ExsSample {
    pub name: String,
    pub path: String,
    pub file: String,
    pub rate: u32,
}

#[derive(Clone, Debug, Default)]
pub struct ExsInstrument {
    pub name: String,
    pub zones: Vec<ExsZone>,
    pub samples: Vec<ExsSample>,
}

/// Decodes the chunk structure of an EXS24 instrument (also found inside Logic's Sampler state).
pub fn parse_exs(b: &[u8]) -> Option<ExsInstrument> {
    // find the first chunk magic
    let start = (0..b.len().saturating_sub(4))
        .find(|&i| matches!(&b[i..i + 4], b"TBOS" | b"JBOS" | b"SOBT" | b"SOBJ"))?;
    let o0 = start.checked_sub(16)?;
    let be = matches!(&b[start..start + 4], b"SOBT" | b"SOBJ");
    let mut inst = ExsInstrument::default();
    let mut o = o0;
    while o + 84 <= b.len() {
        let sig = rd_u32(b, o, be);
        let size = rd_u32(b, o + 4, be) as usize;
        let magic = &b[o + 16..o + 20];
        if !matches!(magic, b"TBOS" | b"JBOS" | b"SOBT" | b"SOBJ") {
            break;
        }
        let name = cstr(&b[o + 20..o + 84]);
        let d = o + 84;
        let data = &b[d.min(b.len())..(d + size).min(b.len())];
        let kind = (sig >> 24) & 0x0F;
        match kind {
            0 => inst.name = name,
            1 if data.len() >= 93 => {
                let opts = data[0];
                let z = ExsZone {
                    name,
                    one_shot: opts & 1 != 0,
                    root: data[1].min(127),
                    fine: data[2] as i8,
                    pan: data[3] as i8,
                    volume: data[4] as i8,
                    lokey: data[6].min(127),
                    hikey: data[7].min(127),
                    lovel: data[9].clamp(1, 127),
                    hivel: data[10].clamp(1, 127),
                    start: rd_u32(data, 12, be),
                    end: rd_u32(data, 16, be),
                    loop_start: rd_u32(data, 20, be),
                    loop_end: rd_u32(data, 24, be),
                    loop_on: data[33] & 1 != 0,
                    coarse: data[77] as i8,
                    group: rd_u32(data, 85, be) as i32,
                    sample: rd_u32(data, 89, be) as i32,
                };
                inst.zones.push(z);
            }
            3 => {
                let rate = rd_u32(data, 8, be);
                let path = if data.len() >= 336 {
                    cstr(&data[80..336])
                } else {
                    String::new()
                };
                let file = if data.len() >= 592 {
                    cstr(&data[336..592])
                } else {
                    String::new()
                };
                inst.samples.push(ExsSample {
                    name,
                    path,
                    file,
                    rate,
                });
            }
            _ => {}
        }
        o = d + size;
    }
    if inst.zones.is_empty() {
        None
    } else {
        Some(inst)
    }
}

fn find_file(name: &str, dirs: &[PathBuf], depth: u32) -> Option<PathBuf> {
    for d in dirs {
        let p = d.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    if depth == 0 {
        return None;
    }
    for d in dirs {
        if let Ok(rd) = std::fs::read_dir(d) {
            let subs: Vec<PathBuf> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            if let Some(p) = find_file(name, &subs, depth - 1) {
                return Some(p);
            }
        }
    }
    None
}

impl Sampler {
    /// Loads an .exs file; samples are looked for at their stored path, next to the .exs, and
    /// in `search` folders (recursively, a few levels).
    pub fn from_exs(path: &Path, search: &[PathBuf], rate: f32) -> std::io::Result<Sampler> {
        let b = std::fs::read(path)?;
        let mut dirs: Vec<PathBuf> = search.to_vec();
        if let Some(p) = path.parent() {
            dirs.push(p.to_path_buf());
            if let Some(pp) = p.parent() {
                dirs.push(pp.to_path_buf());
            }
        }
        let mut s = Sampler::from_exs_bytes(&b, &dirs, rate)?;
        if s.name.is_empty() {
            s.name = path
                .file_stem()
                .map(|x| x.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        Ok(s)
    }

    pub fn from_exs_bytes(b: &[u8], search: &[PathBuf], rate: f32) -> std::io::Result<Sampler> {
        let inst = parse_exs(b).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "not an EXS instrument")
        })?;
        let mut loaded: Vec<Option<Arc<SampleData>>> = Vec::new();
        for smp in &inst.samples {
            let fname = if smp.file.is_empty() {
                smp.name.clone()
            } else {
                smp.file.clone()
            };
            let direct = PathBuf::from(&smp.path).join(&fname);
            let p = if direct.is_file() {
                Some(direct)
            } else {
                find_file(&fname, search, 3)
            };
            loaded.push(p.and_then(|p| load_sample(&p).ok()).map(|x| x.0));
        }
        let mut zones = Vec::new();
        for ez in &inst.zones {
            let Some(Some(data)) = loaded.get(ez.sample.max(0) as usize).cloned() else {
                continue;
            };
            let mut z = Zone::new(data);
            z.lokey = ez.lokey.min(ez.hikey);
            z.hikey = ez.hikey.max(ez.lokey);
            z.lovel = ez.lovel.min(ez.hivel);
            z.hivel = ez.hivel.max(ez.lovel);
            z.root = ez.root;
            z.tune_cents = ez.coarse as f32 * 100.0 + ez.fine as f32;
            z.gain = lg_dsp::db_to_gain(ez.volume as f32);
            z.pan = ez.pan as f32 / 50.0;
            let len = z.end;
            if ez.end > ez.start && (ez.end as usize) <= len {
                z.start = ez.start as usize;
                z.end = ez.end as usize;
            }
            if ez.loop_on && ez.loop_end > ez.loop_start && (ez.loop_end as usize) <= len {
                z.loop_mode = LoopMode::Continuous;
                z.loop_start = ez.loop_start as usize;
                z.loop_end = ez.loop_end as usize;
            }
            if ez.one_shot {
                z.loop_mode = LoopMode::OneShot;
            }
            z.env = [0.0, 0.002, 0.0, 0.0, 1.0, 0.3];
            zones.push(z);
        }
        if zones.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "EXS zones found but none of their samples",
            ));
        }
        let mut s = Sampler::new(zones, rate);
        s.name = inst.name;
        Ok(s)
    }
}

/// Writes a small multi-sample SFZ instrument (our own synthesized samples: 3 roots × 2 velocity
/// layers, looped) for tests and demos; returns the .sfz path.
pub fn write_test_instrument(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let rate = 48000u32;
    let mut sfz = String::from(
        "// lg-synth test instrument\n<control> default_path=samples/\n<global> ampeg_release=0.3 ampeg_attack=0.002\n",
    );
    std::fs::create_dir_all(dir.join("samples"))?;
    for (root, lo, hi) in [(48u8, 0u8, 54u8), (60, 55, 66), (72, 67, 127)] {
        for (layer, (lv, hv, bright)) in [(1u8, 80u8, 0.3f32), (81, 127, 1.0)].iter().enumerate() {
            let f0 = lg_dsp::midi_hz(root as f32) as f64;
            // a whole number of cycles over the loop so the loop is seamless
            let period = rate as f64 / f0;
            let cycles = (24000.0 / period).round();
            let loop_len = (cycles * period).round() as usize;
            let attack = 4800usize;
            let n = attack + loop_len;
            let f0 = cycles * rate as f64 / loop_len as f64;
            let mut l = vec![0.0f32; n];
            for (i, x) in l.iter_mut().enumerate() {
                let t = i as f64 / rate as f64;
                let mut s = 0.0;
                for k in 1..12 {
                    s += (std::f64::consts::TAU * f0 * k as f64 * t).sin()
                        * (*bright as f64).powi(k - 1)
                        / k as f64;
                }
                let atk = if i < attack {
                    1.0 + (1.0 - i as f64 / attack as f64) * 0.5
                } else {
                    1.0
                };
                *x = (s * 0.12 * atk) as f32;
            }
            let name = format!("tone_{root}_{layer}.wav");
            wav::write_wav_24(&dir.join("samples").join(&name), rate, &l, &l)?;
            sfz.push_str(&format!(
                "<region> sample={name} lokey={lo} hikey={hi} pitch_keycenter={root} lovel={lv} hivel={hv} loop_mode=loop_continuous loop_start={attack} loop_end={}\n",
                n - 1
            ));
        }
    }
    let p = dir.join("test.sfz");
    std::fs::write(&p, sfz)?;
    Ok(p)
}

/// The loop points (logical frames) moved by the Loop Start, Loop End and Loop Position
/// modulation values (each −1…1 of the zone's length), kept inside start…end with start < end;
/// Loop Position keeps the length.
pub fn moved_loop(ls: usize, le: usize, s0: usize, e0: usize, ms: f32, me: f32, mp: f32) -> (usize, usize) {
    let len = (e0 - s0) as f32;
    let shift = mp * len;
    let lo = (ls as f32 + ms * len + shift).clamp(s0 as f32, e0 as f32 - 2.0);
    let hi = (le as f32 + me * len + shift).clamp(lo + 2.0, e0 as f32);
    // Loop Position at the end: slide back so the length stays
    let (lo, hi) = if mp != 0.0 && hi - lo < (le - ls) as f32 && hi >= e0 as f32 { ((e0 - (le - ls)).max(s0) as f32, e0 as f32) } else { (lo, hi) };
    (lo as usize, hi as usize)
}

#[cfg(test)]
mod loop_target_tests {
    use super::moved_loop;

    #[test]
    fn loop_targets_move_the_loop() {
        // zone 0..1000, loop 400..600
        assert_eq!(moved_loop(400, 600, 0, 1000, 0.1, 0.0, 0.0), (500, 600));
        assert_eq!(moved_loop(400, 600, 0, 1000, 0.0, -0.1, 0.0), (400, 500));
        // Loop Position: the whole loop, length kept
        assert_eq!(moved_loop(400, 600, 0, 1000, 0.0, 0.0, 0.2), (600, 800));
        assert_eq!(moved_loop(400, 600, 0, 1000, 0.0, 0.0, 0.9), (800, 1000));
        // start never passes the end
        let (a, b) = moved_loop(400, 600, 0, 1000, 0.5, 0.0, 0.0);
        assert!(a < b);
    }
}

#[cfg(test)]
mod lfo_target_tests {
    use crate::sampler_synth::{Lfo, LfoSet};

    #[test]
    fn lfo_fade_scaling_and_phase_offset() {
        let s = LfoSet { on: true, wave: 2, hz: 1.0, fade: 1.0, bipolar: false, poly: true, ..Default::default() };
        let sq = LfoSet { wave: 3, ..s };
        let mut a = Lfo::default();
        a.restart(&sq, 1);
        // with the fade doubled (target +0.25 → ×2), half the level after 0.5 s
        let long = LfoSet { fade: sq.fade * 2f32.powf(0.25 * 4.0), ..sq };
        let mut b = Lfo::default();
        b.restart(&long, 1);
        let (x, y) = (a.advance(&sq, 0.0, 0.5), b.advance(&long, 0.0, 0.5));
        assert!(y < x * 0.6, "{x} {y}");
        // a phase offset moves the start of the waveform
        let mut c = Lfo::default();
        c.restart(&LfoSet { fade: 0.0, ..s }, 1);
        c.phase = (c.phase + 0.25).rem_euclid(1.0);
        assert!((c.advance(&LfoSet { fade: 0.0, ..s }, 0.0, 0.0) - 0.25).abs() < 1e-6);
    }
}
