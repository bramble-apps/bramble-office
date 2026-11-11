//! Alchemy's performance and effects pages, on top of the sound engine (alchemy.rs): the
//! Arpeggiator (mode, octaves, rate, note length, swing, latch) with its 16-step sequencer (each
//! step on / off, velocity, length), an MSEG (up to 16 points, loop) driving the master filter,
//! the volume and the pitch, and the effects rack (four slots of Studio's own effects, each with
//! its mix and bypass). Parameter ids from 1000, past Logic's.

use std::sync::OnceLock;

use lg_dsp::{Effect, Param, Unit};

use crate::arp::{ARP_MODES, ARP_RATES, ArpSettings, MsegShape, STEPS};

pub const ARP: u32 = 1000;
pub const STEP_ON: u32 = 1010;
pub const STEP_VEL: u32 = 1030;
pub const STEP_LEN: u32 = 1050;
pub const MSEG: u32 = 1100;
pub const MSEG_TIME: u32 = 1110;
pub const MSEG_LEVEL: u32 = 1130;
pub const FX: u32 = 1200;
pub const FX_SLOTS: usize = 4;
/// An effect slot's own parameters: FX_PARAM + slot × 100 + the effect's parameter id.
pub const FX_PARAM: u32 = 1300;

/// The effects the rack offers (Studio's stand-ins by Logic's names).
pub const FX_TYPES: &[&str] = &["Off", "Distortion", "Bitcrusher", "Compressor", "Channel EQ", "Chorus", "Flanger", "Phaser", "Tremolo", "AutoFilter", "Stereo Delay", "Tape Delay", "ChromaVerb"];

pub fn params() -> Vec<Param> {
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    static RATES: OnceLock<Vec<&'static str>> = OnceLock::new();
    let rates: &'static [&'static str] = RATES.get_or_init(|| ARP_RATES.iter().map(|r| r.0).collect()).as_slice();
    let mut v = vec![
        Param::new(ARP, "Arp On", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::menu(ARP + 1, "Arp Mode", ARP_MODES, 0.0),
        Param::new(ARP + 2, "Arp Octaves", 1.0, 4.0, 1.0, Unit::None),
        Param::menu(ARP + 3, "Arp Rate", rates, 7.0),
        Param::new(ARP + 4, "Arp Note Length", 0.05, 1.0, 0.5, Unit::Ratio),
        Param::new(ARP + 5, "Arp Swing", 0.0, 1.0, 0.0, Unit::Ratio),
        Param::new(ARP + 6, "Arp Latch", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(ARP + 7, "Arp Steps", 1.0, 16.0, 16.0, Unit::None),
        Param::new(ARP + 8, "Arp Step Velocity", 0.0, 1.0, 0.0, Unit::OnOff),
    ];
    for k in 0..STEPS {
        v.push(Param::new(STEP_ON + k as u32, leak(format!("Step {} On", k + 1)), 0.0, 1.0, 1.0, Unit::OnOff));
        v.push(Param::new(STEP_VEL + k as u32, leak(format!("Step {} Velocity", k + 1)), 0.0, 1.0, 1.0, Unit::Ratio));
        v.push(Param::new(STEP_LEN + k as u32, leak(format!("Step {} Length", k + 1)), 0.05, 1.0, 1.0, Unit::Ratio));
    }
    v.extend([
        Param::new(MSEG, "MSEG On", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(MSEG + 1, "MSEG to Cutoff", -1.0, 1.0, 0.0, Unit::None),
        Param::new(MSEG + 2, "MSEG to Volume", -1.0, 1.0, 0.0, Unit::None),
        Param::new(MSEG + 3, "MSEG to Pitch", -24.0, 24.0, 0.0, Unit::Semitones),
        Param::new(MSEG + 4, "MSEG Loop", 0.0, 1.0, 0.0, Unit::OnOff),
        Param::new(MSEG + 5, "MSEG Loop Start", 0.0, 15.0, 1.0, Unit::None),
        Param::new(MSEG + 6, "MSEG Loop End", 0.0, 15.0, 2.0, Unit::None),
        Param::new(MSEG + 7, "MSEG Points", 2.0, 16.0, 4.0, Unit::None),
    ]);
    let shape = MsegShape::default();
    for k in 0..16 {
        let (t, l) = if k < shape.n { shape.points[k] } else { (0.25, 0.0) };
        v.push(Param::new(MSEG_TIME + k as u32, leak(format!("MSEG Point {} Time", k + 1)), 0.0, 10.0, t, Unit::Seconds));
        v.push(Param::new(MSEG_LEVEL + k as u32, leak(format!("MSEG Point {} Level", k + 1)), -1.0, 1.0, l, Unit::None));
    }
    for s in 0..FX_SLOTS {
        let b = FX + s as u32 * 10;
        v.push(Param::menu(b, leak(format!("Effect {} Type", s + 1)), FX_TYPES, 0.0));
        v.push(Param::new(b + 1, leak(format!("Effect {} Mix", s + 1)), 0.0, 1.0, 1.0, Unit::Ratio));
        v.push(Param::new(b + 2, leak(format!("Effect {} Bypass", s + 1)), 0.0, 1.0, 0.0, Unit::OnOff));
    }
    v
}

/// The extras' running state inside Alchemy.
pub struct Extras {
    pub arp_set: ArpSettings,
    pub arp: crate::arp::Arp,
    pub mseg: MsegShape,
    pub mseg_on: bool,
    pub mseg_amt: [f32; 3],
    pub mseg_t: f32,
    pub held: usize,
    pub fx: [Option<Box<dyn Effect>>; FX_SLOTS],
    pub fx_kind: [u8; FX_SLOTS],
    pub fx_mix: [f32; FX_SLOTS],
    pub fx_bypass: [bool; FX_SLOTS],
    pub bpm: f64,
    rate: f32,
    dry: Vec<f32>,
    dry_r: Vec<f32>,
}

impl Extras {
    pub fn new(rate: f32) -> Extras {
        Extras {
            arp_set: ArpSettings::default(),
            arp: crate::arp::Arp::default(),
            mseg: MsegShape::default(),
            mseg_on: false,
            mseg_amt: [0.0; 3],
            mseg_t: 0.0,
            held: 0,
            fx: Default::default(),
            fx_kind: [0; FX_SLOTS],
            fx_mix: [1.0; FX_SLOTS],
            fx_bypass: [false; FX_SLOTS],
            bpm: 120.0,
            rate,
            dry: vec![0.0; 4096],
            dry_r: vec![0.0; 4096],
        }
    }
    /// Reads the settings (`p(id)`); builds effects whose type changed (not on the audio thread).
    pub fn read(&mut self, p: &dyn Fn(u32) -> f32) {
        let s = &mut self.arp_set;
        s.on = p(ARP) >= 0.5;
        s.mode = p(ARP + 1).round() as u8;
        s.octaves = p(ARP + 2).round().clamp(1.0, 4.0) as u8;
        s.beats = ARP_RATES[(p(ARP + 3).round().max(0.0) as usize).min(ARP_RATES.len() - 1)].1;
        s.gate = p(ARP + 4);
        s.swing = p(ARP + 5);
        s.latch = p(ARP + 6) >= 0.5;
        s.steps = p(ARP + 7).round().clamp(1.0, 16.0) as usize;
        s.seq_vel = p(ARP + 8) >= 0.5;
        for k in 0..STEPS {
            s.step_on[k] = p(STEP_ON + k as u32) >= 0.5;
            s.step_vel[k] = p(STEP_VEL + k as u32);
            s.step_len[k] = p(STEP_LEN + k as u32);
        }
        self.mseg_on = p(MSEG) >= 0.5;
        self.mseg_amt = [p(MSEG + 1), p(MSEG + 2), p(MSEG + 3)];
        self.mseg.n = p(MSEG + 7).round().clamp(2.0, 16.0) as usize;
        for k in 0..16 {
            self.mseg.points[k] = (if k == 0 { 0.0 } else { p(MSEG_TIME + k as u32) }, p(MSEG_LEVEL + k as u32));
        }
        let looped = p(MSEG + 4) >= 0.5;
        self.mseg.loop_from = looped.then_some(p(MSEG + 5).round() as usize);
        self.mseg.loop_to = looped.then_some(p(MSEG + 6).round() as usize);
        for s in 0..FX_SLOTS {
            let b = FX + s as u32 * 10;
            let kind = p(b).round().max(0.0) as u8;
            if kind != self.fx_kind[s] {
                self.fx_kind[s] = kind;
                self.fx[s] = FX_TYPES.get(kind as usize).filter(|_| kind > 0).and_then(|n| lg_fx::by_logic_name(n, self.rate));
                if let Some(f) = self.fx[s].as_mut() {
                    f.set_tempo(self.bpm);
                }
            }
            self.fx_mix[s] = p(b + 1);
            self.fx_bypass[s] = p(b + 2) >= 0.5;
        }
    }
    /// An effect slot's parameter (FX_PARAM + slot × 100 + id): get, set.
    pub fn fx_param(&self, id: u32) -> Option<f32> {
        let k = id.checked_sub(FX_PARAM)?;
        let (s, pid) = ((k / 100) as usize, k % 100);
        self.fx.get(s)?.as_ref().map(|f| f.get(pid))
    }
    pub fn set_fx_param(&mut self, id: u32, v: f32) -> bool {
        let Some(k) = id.checked_sub(FX_PARAM) else { return false };
        let (s, pid) = ((k / 100) as usize, k % 100);
        match self.fx.get_mut(s).and_then(|f| f.as_mut()) {
            Some(f) => {
                f.set(pid, v);
                true
            }
            None => false,
        }
    }
    pub fn set_tempo(&mut self, bpm: f64) {
        self.bpm = bpm;
        for f in self.fx.iter_mut().flatten() {
            f.set_tempo(bpm);
        }
    }
    /// The MSEG's value now (0 when off).
    pub fn mseg_level(&self) -> f32 {
        if self.mseg_on { self.mseg.level(self.mseg_t, self.held > 0) } else { 0.0 }
    }
    /// The effects rack over a block.
    pub fn process_fx(&mut self, l: &mut [f32], r: &mut [f32]) {
        let n = l.len().min(r.len()).min(self.dry.len());
        for s in 0..FX_SLOTS {
            if self.fx_bypass[s] {
                continue;
            }
            let Some(f) = self.fx[s].as_mut() else { continue };
            let mix = self.fx_mix[s];
            self.dry[..n].copy_from_slice(&l[..n]);
            self.dry_r[..n].copy_from_slice(&r[..n]);
            f.process(&mut l[..n], &mut r[..n]);
            if mix < 1.0 {
                for i in 0..n {
                    l[i] = self.dry[i] + (l[i] - self.dry[i]) * mix;
                    r[i] = self.dry_r[i] + (r[i] - self.dry_r[i]) * mix;
                }
            }
        }
    }
    pub fn reset(&mut self) {
        self.arp.clear();
        self.held = 0;
        for f in self.fx.iter_mut().flatten() {
            f.reset();
        }
    }
}
