//! Quick Sampler stand-in: one sample, four modes — Classic (pitched across the keyboard, with a
//! sustain loop), One Shot (plays to the end regardless of note-off), Slice (transients detected
//! and laid out from C1 upwards, one slice per key) and Recorder (the host hands over recorded
//! audio with `set_audio`; it then plays as Classic). Runs on the Sampler's zone engine.

use std::path::Path;
use std::sync::Arc;

use crate::sampler::{LoopMode, Pcm, SampleData, Sampler, Zone};
use crate::wav::Audio;
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const MODES: &[&str] = &["Classic", "One Shot", "Slice", "Recorder"];

pub static PARAMS: &[Param] = &[
    Param::menu(0, "Mode", MODES, 0.0),
    Param::new(1, "Root Key", 0.0, 127.0, 60.0, Unit::None),
    Param::new(2, "Start", 0.0, 1.0, 0.0, Unit::Ratio),
    Param::new(3, "End", 0.0, 1.0, 1.0, Unit::Ratio),
    Param::new(4, "Loop", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(5, "Loop Start", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(6, "Loop End", 0.0, 1.0, 1.0, Unit::Ratio),
    Param::new(7, "Attack", 0.0, 5.0, 0.002, Unit::Seconds),
    Param::new(8, "Decay", 0.0, 10.0, 0.0, Unit::Seconds),
    Param::new(9, "Sustain", 0.0, 1.0, 1.0, Unit::Ratio),
    Param::new(10, "Release", 0.0, 10.0, 0.1, Unit::Seconds),
    Param::new(11, "Filter Cutoff", 20.0, 20000.0, 20000.0, Unit::Hz),
    Param::new(12, "Filter Resonance", 0.0, 24.0, 0.0, Unit::Db),
    Param::new(13, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(14, "Slice Sensitivity", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(15, "Pitch Tracking", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(16, "Fine Tune", -100.0, 100.0, 0.0, Unit::Cents),
    Param::new(17, "Reverse", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::new(18, "Pitch Bend Range", 0.0, 24.0, 2.0, Unit::Semitones),
];

pub struct QuickSampler {
    params: crate::poly::Params,
    audio: Option<Audio>,
    data: Option<Arc<SampleData>>,
    inner: Sampler,
    /// Slice start frames (Slice mode).
    pub slices: Vec<usize>,
    /// Slices placed by hand (the waveform's slice markers); None: found by Slice Sensitivity.
    pub manual_slices: Option<Vec<usize>>,
    rate: f32,
    /// The editor's revision this sample came from.
    pub doc_rev: u64,
}

impl QuickSampler {
    pub fn new(rate: f32) -> QuickSampler {
        QuickSampler {
            params: crate::poly::Params::new(PARAMS),
            audio: None,
            data: None,
            inner: Sampler::new(Vec::new(), rate),
            slices: Vec::new(),
            manual_slices: None,
            rate,
            doc_rev: 0,
        }
    }
    pub fn from_file(path: &Path, rate: f32) -> std::io::Result<QuickSampler> {
        let b = std::fs::read(path)?;
        let a = crate::wav::decode_audio(&b).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "unsupported audio file")
        })?;
        let mut q = QuickSampler::new(rate);
        q.set_audio(a);
        Ok(q)
    }
    /// Loads audio (also the Recorder mode's hand-over). Detects the root key when the file has none.
    pub fn set_audio(&mut self, a: Audio) {
        let root = a.root.map(|r| r as f32).or_else(|| detect_root(&a));
        if let Some(r) = root {
            self.params.set(1, r);
        }
        if let Some((s, e)) = a.loop_points {
            let n = a.frames().max(1) as f32;
            self.params.set(4, 1.0);
            self.params.set(5, s as f32 / n);
            self.params.set(6, e as f32 / n);
        }
        self.audio = Some(a);
        self.rebuild_data();
        self.rebuild();
    }
    fn rebuild_data(&mut self) {
        let Some(a) = &self.audio else { return };
        let rev = self.params.get(17) >= 0.5;
        let ch = a
            .channels
            .iter()
            .take(2)
            .map(|c| {
                let mut v = c.clone();
                if rev {
                    v.reverse();
                }
                Pcm::F32(v)
            })
            .collect();
        self.data = Some(Arc::new(SampleData {
            rate: a.rate as f32,
            ch,
        }));
    }
    fn rebuild(&mut self) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let p = |i| self.params.get(i);
        let n = data.ch.first().map(|c| match c {
            Pcm::F32(v) => v.len(),
            Pcm::I16(v) => v.len(),
        });
        let n = n.unwrap_or(0);
        if n < 2 {
            return;
        }
        let start = (p(2) * n as f32) as usize;
        let end = ((p(3) * n as f32) as usize).clamp(start + 1, n);
        let mode = p(0).round() as i32;
        let mut base = Zone::new(data.clone());
        base.root = p(1).round().clamp(0.0, 127.0) as u8;
        base.tune_cents = p(16);
        base.keytrack = if p(15) >= 0.5 { 100.0 } else { 0.0 };
        base.start = start;
        base.end = end;
        base.env = [
            0.0,
            p(7),
            0.0,
            p(8),
            if p(8) > 0.0 { p(9) } else { 1.0 },
            p(10),
        ];
        if p(11) < 19999.0 {
            base.cutoff = Some(p(11));
            base.resonance = p(12);
        }
        let mut zones = Vec::new();
        match mode {
            1 => {
                base.loop_mode = LoopMode::OneShot;
                zones.push(base);
            }
            2 => {
                self.slices = match &self.manual_slices {
                    Some(m) => {
                        let mut v: Vec<usize> = m.iter().copied().filter(|&x| x >= start && x < end).collect();
                        v.sort_unstable();
                        v.dedup();
                        if v.first() != Some(&start) {
                            v.insert(0, start);
                        }
                        v
                    }
                    None => detect_slices(&data, start, end, p(14)),
                };
                for (k, &s0) in self.slices.iter().enumerate().take(64) {
                    let s1 = self.slices.get(k + 1).copied().unwrap_or(end);
                    let mut z = base.clone();
                    let key = (36 + k).min(127) as u8;
                    z.lokey = key;
                    z.hikey = key;
                    z.root = key;
                    z.start = s0;
                    z.end = s1.max(s0 + 1);
                    z.loop_mode = LoopMode::OneShot;
                    z.keytrack = 100.0;
                    zones.push(z);
                }
            }
            _ => {
                if p(4) >= 0.5 {
                    let ls = (p(5) * n as f32) as usize;
                    let le = ((p(6) * n as f32) as usize).min(end);
                    if le > ls + 16 {
                        base.loop_mode = LoopMode::Sustain;
                        base.loop_start = ls.max(start);
                        base.loop_end = le;
                    }
                }
                zones.push(base);
            }
        }
        self.inner.set_zones(zones);
        self.inner.set(0, p(13));
        self.inner.set(12, p(18));
    }
}

impl QuickSampler {
    /// The loaded sample (for the waveform display).
    pub fn audio(&self) -> Option<&Audio> {
        self.audio.as_ref()
    }
    /// The slice markers by hand (Slice mode), or back to the detected ones.
    pub fn set_slices(&mut self, s: Option<Vec<usize>>) {
        self.manual_slices = s;
        self.rebuild();
    }
    /// A copy with the same sample and settings.
    pub fn duplicate(&self) -> QuickSampler {
        let mut q = QuickSampler::new(self.rate);
        if let Some(a) = &self.audio {
            q.set_audio(a.clone());
        }
        for p in PARAMS {
            q.params.set(p.id, self.params.get(p.id));
        }
        q.manual_slices = self.manual_slices.clone();
        q.rebuild_data();
        q.rebuild();
        q
    }
    /// Stops the sound at once (a choke).
    pub fn choke(&mut self) {
        self.inner.choke();
    }
}

/// Pitch of the loudest stretch by autocorrelation (50–2000 Hz), as a MIDI note.
fn detect_root(a: &Audio) -> Option<f32> {
    let c = a.channels.first()?;
    let rate = a.rate as f32;
    let win = (rate * 0.1) as usize;
    if c.len() < win * 2 {
        return None;
    }
    let mut best = (0usize, 0.0f32);
    let mut i = 0;
    while i + win < c.len() {
        let e: f32 = c[i..i + win].iter().map(|x| x * x).sum();
        if e > best.1 {
            best = (i, e);
        }
        i += win / 2;
    }
    let x = &c[best.0..best.0 + win];
    let (lo, hi) = ((rate / 2000.0) as usize, (rate / 50.0) as usize);
    let mut bl = 0;
    let mut bv = 0.0f32;
    let e0: f32 = x.iter().map(|v| v * v).sum();
    for lag in lo..hi.min(win - 1) {
        let s: f32 = (0..win - lag).map(|k| x[k] * x[k + lag]).sum::<f32>() / e0.max(1e-9);
        if s > bv {
            bv = s;
            bl = lag;
        }
    }
    if bv < 0.5 || bl == 0 {
        return None;
    }
    let hz = rate / bl as f32;
    Some((69.0 + 12.0 * (hz / 440.0).log2()).round())
}

/// Transient positions (energy rise in 5 ms frames), at least 50 ms apart.
pub fn detect_slices(d: &SampleData, start: usize, end: usize, sens: f32) -> Vec<usize> {
    let rate = d.rate;
    let frame = ((rate * 0.005) as usize).max(16);
    let at = |i: usize| -> f32 {
        let mut s = 0.0;
        for c in &d.ch {
            s += match c {
                Pcm::F32(v) => v.get(i).copied().unwrap_or(0.0),
                Pcm::I16(v) => v.get(i).map(|&x| x as f32 / 32768.0).unwrap_or(0.0),
            };
        }
        s
    };
    let mut out = vec![start];
    let mut prev = 0.0f32;
    let mut last = start;
    let ratio = 4.0 - 3.0 * sens.clamp(0.0, 1.0);
    let mut i = start;
    let peak = (start..end)
        .step_by(4)
        .map(|k| at(k).abs())
        .fold(0.0f32, f32::max);
    while i + frame < end {
        let e = ((i..i + frame).map(|k| at(k) * at(k)).sum::<f32>() / frame as f32).sqrt();
        if e > prev * ratio + 1e-4
            && e > peak * 0.05 * (1.2 - sens)
            && i - last > (rate * 0.05) as usize
        {
            out.push(i);
            last = i;
        }
        prev = prev * 0.6 + e * 0.4;
        i += frame;
    }
    out
}

impl Plugin for QuickSampler {
    fn params(&self) -> &[Param] {
        PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            if id == 17 {
                self.rebuild_data();
            }
            self.rebuild();
        }
    }
    fn reset(&mut self) {
        self.inner.reset();
        self.rebuild();
    }
    fn tail(&self) -> f32 {
        self.params.get(10) + 0.1
    }
}

impl Instrument for QuickSampler {
    fn note_on(&mut self, p: u8, v: u8) {
        self.inner.note_on(p, v)
    }
    fn note_off(&mut self, p: u8) {
        self.inner.note_off(p)
    }
    fn controller(&mut self, cc: u8, v: u8) {
        self.inner.controller(cc, v)
    }
    fn pitch_bend(&mut self, v: i16) {
        self.inner.pitch_bend(v)
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let _ = self.rate;
        self.inner.render(l, r)
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
