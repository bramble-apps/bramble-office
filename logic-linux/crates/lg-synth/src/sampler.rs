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
    /// Loops until note-off, then plays on to the end.
    Sustain,
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
        }
    }
    fn matches(&self, key: u8, vel: u8) -> bool {
        key >= self.lokey && key <= self.hikey && vel >= self.lovel && vel <= self.hivel
    }
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

pub(crate) struct SamplerShared {
    rate: f32,
    zones: Vec<Zone>,
    env_scale: [f32; 4],
    filter_on: bool,
    cutoff: f32,
    res: f32,
    vel_cut: f32,
    vel_sens: f32,
    transpose: f32,
    tune: f32,
    vib: f32,
    vib_phase: f32,
    /// Zone chosen for the next note_on (set by the instrument, which handles round robin).
    pending: [usize; 16],
    npending: usize,
}

pub(crate) struct SamplerVoice {
    on: bool,
    zone: usize,
    note: u8,
    pos: f64,
    dir: f64,
    released: bool,
    env: Adsr,
    delay: u32,
    gain: f32,
    pan_l: f32,
    pan_r: f32,
    svf: [Svf; 2],
    filt: bool,
    fade: Fade,
    killing: bool,
    level: f32,
    pub group: i32,
    pub off_by: i32,
    /// Extra zones layered on the same note (up to 3 more voices are used for layers).
    base_inc: f64,
}

impl SamplerVoice {
    fn new() -> SamplerVoice {
        SamplerVoice {
            on: false,
            zone: 0,
            note: 0,
            pos: 0.0,
            dir: 1.0,
            released: false,
            env: Adsr::default(),
            delay: 0,
            gain: 0.0,
            pan_l: 1.0,
            pan_r: 1.0,
            svf: [Svf::default(); 2],
            filt: false,
            fade: Fade::default(),
            killing: false,
            level: 0.0,
            group: 0,
            off_by: 0,
            base_inc: 1.0,
        }
    }
}

impl Voice<SamplerShared> for SamplerVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &SamplerShared, _ctl: &Ctl) {
        if sh.npending == 0 {
            self.on = false;
            return;
        }
        // the instrument queued the zone for this voice
        let zi = sh.pending[0];
        let z = &sh.zones[zi];
        let rate = sh.rate;
        self.zone = zi;
        self.note = note;
        let semis = (note as f32 - z.root as f32) * z.keytrack / 100.0
            + (z.tune_cents + sh.tune) / 100.0
            + sh.transpose;
        self.base_inc = (z.rate / rate) as f64 * dsp::semis(semis) as f64;
        self.pos = z.start as f64;
        self.dir = 1.0;
        self.released = false;
        let [delay, a, hold, d, s, r] = z.env;
        let _ = hold;
        self.env.set(
            a * sh.env_scale[0],
            d.max(0.001) * sh.env_scale[1] * if d <= 0.0 { 1000.0 } else { 1.0 },
            s * sh.env_scale[2],
            r * sh.env_scale[3],
            rate,
        );
        self.env.trigger();
        self.delay = (delay * rate) as u32;
        let v = velocity.max(1) as f32 / 127.0;
        let vt = z.vel_track * sh.vel_sens;
        let vel_gain = 1.0 - vt + vt * v * v;
        self.gain = z.gain * vel_gain;
        let p = z.pan.clamp(-1.0, 1.0);
        self.pan_l = (1.0 - p).sqrt();
        self.pan_r = (1.0 + p).sqrt();
        self.filt = sh.filter_on || z.cutoff.is_some();
        let fc = z.cutoff.unwrap_or(20000.0).min(if sh.filter_on {
            sh.cutoff * (1.0 + 3.0 * sh.vel_cut * (v - 0.5)).max(0.1)
        } else {
            20000.0
        });
        let q = 0.707 + (z.resonance / 24.0).max(sh.res) * 10.0;
        for f in &mut self.svf {
            *f = Svf::new(z.filter, fc.clamp(20.0, rate * 0.45), q, rate);
        }
        self.group = z.group;
        self.off_by = z.off_by;
        self.killing = false;
        self.on = true;
        self.level = self.gain;
    }
    fn release(&mut self, sh: &SamplerShared, _ctl: &Ctl) {
        let z = &sh.zones[self.zone];
        if z.loop_mode == LoopMode::OneShot {
            return;
        }
        self.released = true;
        self.env.gate_off();
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
        let vib = 1.0 + (sh.vib * ctl.mod_wheel * 0.006 * dsp::sin_cycles(sh.vib_phase)) as f64;
        let inc = self.base_inc * ctl.bend_ratio as f64 * vib;
        let looping = match z.loop_mode {
            LoopMode::Continuous => true,
            LoopMode::Sustain => !self.released,
            _ => false,
        } && z.loop_end > z.loop_start + 1;
        let end = z.end.min(data.ch[0].len()) as f64;
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            if self.delay > 0 {
                self.delay -= 1;
                continue;
            }
            if looping && self.pos >= z.loop_end as f64 {
                self.pos -= (z.loop_end - z.loop_start) as f64;
            }
            if !looping && self.pos >= end - 1.0 {
                self.on = false;
                break;
            }
            let ip = self.pos as usize;
            let f = (self.pos - ip as f64) as f32;
            let read = |c: &Pcm| {
                let xm1 = if ip > 0 { c.at(ip - 1) } else { c.at(ip) };
                let x0 = c.at(ip);
                let mut i1 = ip + 1;
                let mut i2 = ip + 2;
                if looping {
                    if i1 >= z.loop_end {
                        i1 -= z.loop_end - z.loop_start;
                    }
                    if i2 >= z.loop_end {
                        i2 -= z.loop_end - z.loop_start;
                    }
                }
                dsp::hermite(xm1, x0, c.at(i1), c.at(i2), f)
            };
            let mut a = read(&data.ch[0]);
            let mut b = if stereo { read(&data.ch[1]) } else { a };
            if self.filt {
                a = self.svf[0].run(a);
                b = if stereo { self.svf[1].run(b) } else { a };
            }
            let mut g = self.env.next() * self.gain;
            if self.killing {
                g *= self.fade.next();
            }
            let yl = a * g * self.pan_l;
            let yr = b * g * self.pan_r;
            peak = peak.max(yl.abs().max(yr.abs()));
            l[i] += yl;
            r[i] += yr;
            self.pos += inc * self.dir;
        }
        self.level = peak;
        if self.env.idle() || (self.killing && self.fade.gain <= 0.0) {
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
    pub name: String,
}

impl Sampler {
    pub fn new(zones: Vec<Zone>, rate: f32) -> Sampler {
        dsp::warm_tables();
        let has_release_zones = zones.iter().any(|z| z.trigger == Trigger::Release);
        let mut s = Sampler {
            params: Params::new(PARAMS),
            poly: Poly::new(64, SamplerVoice::new),
            ch: Channel::new(rate),
            sh: SamplerShared {
                rate,
                zones,
                env_scale: [1.0; 4],
                filter_on: false,
                cutoff: 20000.0,
                res: 0.0,
                vel_cut: 0.0,
                vel_sens: 1.0,
                transpose: 0.0,
                tune: 0.0,
                vib: 0.3,
                vib_phase: 0.0,
                pending: [0; 16],
                npending: 0,
            },
            rate,
            rr: [0; 128],
            rng: dsp::Rng::new(4242),
            held_vel: [0; 128],
            has_release_zones,
            name: String::new(),
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
    pub fn zones(&self) -> &[Zone] {
        &self.sh.zones
    }
    fn update(&mut self) {
        let p = |i| self.params.get(i);
        self.sh.env_scale = [p(1), p(2), p(3), p(4)];
        self.sh.filter_on = p(5) >= 0.5;
        self.sh.cutoff = p(6);
        self.sh.res = p(7);
        self.sh.vel_cut = p(8);
        self.sh.vel_sens = p(9);
        self.sh.transpose = p(10);
        self.sh.tune = p(11);
        self.ch.bend_up = p(12);
        self.ch.bend_down = p(12);
        self.ch.update_bend();
        self.sh.vib = p(14);
        let max = p(13) as usize;
        self.poly.set_max(max);
    }

    fn trigger(&mut self, note: u8, vel: u8, trig: Trigger) {
        let rand = self.rng.unit();
        let counter = self.rr[note as usize];
        let mut chosen = [0usize; 16];
        let mut n = 0;
        for (i, z) in self.sh.zones.iter().enumerate() {
            if z.trigger != trig || !z.matches(note, vel) {
                continue;
            }
            if z.seq_length > 1 && (counter % z.seq_length) + 1 != z.seq_position {
                continue;
            }
            if rand < z.lorand || rand >= z.hirand.max(z.lorand + 1e-6) && z.hirand < 1.0 {
                continue;
            }
            if n < chosen.len() {
                chosen[n] = i;
                n += 1;
            }
        }
        self.rr[note as usize] = counter.wrapping_add(1);
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
            self.sh.pending[0] = zi;
            self.sh.npending = 1;
            self.poly.note_on(note, vel, &self.sh, &self.ch.ctl);
            self.sh.npending = 0;
        }
    }
}

impl Plugin for Sampler {
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
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        self.sh
            .zones
            .iter()
            .map(|z| z.env[5])
            .fold(0.0, f32::max)
            .min(10.0)
            * self.params.get(4)
    }
}

impl Instrument for Sampler {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        self.held_vel[pitch as usize] = velocity;
        self.trigger(pitch, velocity, Trigger::Attack);
    }
    fn note_off(&mut self, pitch: u8) {
        self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
        if self.has_release_zones {
            let v = self.held_vel[pitch as usize].max(1);
            self.trigger(pitch, v, Trigger::Release);
        }
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
        let vol = lg_dsp::db_to_gain(self.params.get(0));
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            self.sh.vib_phase = (self.sh.vib_phase + 5.5 * m as f32 / self.rate).fract();
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

pub(crate) fn load_sample(
    path: &Path,
) -> std::io::Result<(Arc<SampleData>, Option<(u32, u32)>, Option<u8>)> {
    let bytes = std::fs::read(path)?;
    let a = wav::decode_audio(&bytes)
        .or_else(|| decode_caf(&bytes))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unsupported sample format: {}", path.display()),
            )
        })?;
    let ch = a.channels.into_iter().take(2).map(Pcm::F32).collect();
    Ok((
        Arc::new(SampleData {
            rate: a.rate as f32,
            ch,
        }),
        a.loop_points,
        a.root,
    ))
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
    let mut cache: HashMap<PathBuf, (Arc<SampleData>, Option<(u32, u32)>, Option<u8>)> =
        HashMap::new();
    let mut zones = Vec::new();
    for r in regions {
        let Some(sample) = r.get("sample") else {
            continue;
        };
        if sample.starts_with('*') {
            continue; // generated waveforms (*sine…) are not supported
        }
        let rel = format!("{}{}", default_path, sample).replace('\\', "/");
        let p = base.join(&rel);
        let entry = match cache.get(&p) {
            Some(e) => e.clone(),
            None => match load_sample(&p) {
                Ok(e) => {
                    cache.insert(p.clone(), e.clone());
                    e
                }
                Err(_) => continue,
            },
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
