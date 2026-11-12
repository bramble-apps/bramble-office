//! Quick Sampler, as Logic's guide describes it: one sample, four modes — Classic (pitched
//! across the keyboard from the root key, with Logic's loop modes: Forward, Reverse, Alternate,
//! Play to End on Release, a crossfade with an equal-power or linear curve), One Shot (start to
//! end marker whatever the note length, with fade in / out), Slice (Transient, Beat Divisions,
//! Equal Divisions or Manual slicing, a slice per key from the Start Key on chromatic, white or
//! black keys, Gate and Play to End) and Recorder (records the side chain input, with Monitor,
//! start on a threshold and an input meter). Flex keeps the sample's speed at every pitch
//! (WSOLA from lg-audio), Follow Tempo syncs it to the project, Speed divides or multiplies it.
//!
//! The synthesis is the Sampler's engine: the Pitch pane (Coarse, Fine, Glide, Bend Range, its
//! own envelope with a depth), the Filter pane (the Sampler's filter designs, Cutoff, Reso,
//! Drive, its envelope's depth, Keyscale), the Amp pane (Volume, Pan, Polyphony, the amplitude
//! envelope with Vel), two LFOs each with Amount, Via and Target, and four Mod Matrix routings.
//! Those parameters keep the Sampler's ids (sampler_synth), so the same controls draw them.
//! Everything here is our own code; the factory values are Studio's.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use crate::sampler::{LoopDir, LoopMode, Pcm, SampleData, Sampler, Zone};
use crate::sampler_synth as ss;
use crate::wav::Audio;
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const MODES: &[&str] = &["Classic", "One Shot", "Slice", "Recorder"];
pub const LOOP_MODES: &[&str] = &["No Loop", "Forward", "Reverse", "Alternate", "Play to End on Release"];
pub const XFADE_CURVES: &[&str] = &["Equal Power", "Linear"];
pub const SLICE_MODES: &[&str] = &["Transient", "Beat Divisions", "Equal Divisions", "Manual"];
pub const DIVISIONS: &[&str] = &["1 Bar", "1/2", "1/4", "1/8", "1/16", "1/32"];
pub const KEY_MAPPINGS: &[&str] = &["Chromatic", "White Keys", "Black Keys"];
pub const POLYPHONY: &[&str] = &["Mono", "Legato", "2", "3", "4", "5", "6", "8", "12", "16", "24", "32", "48", "64"];
pub const FLEX_SPEEDS: &[&str] = &["1/4x", "1/2x", "1x", "2x", "4x"];
pub const RECORD_STARTS: &[&str] = &["Start immediately", "Wait for signal to pass Threshold"];
/// Targets of the LFOs' own routing (indexes into the Sampler's Mod Matrix targets).
pub const LFO_TARGETS: &[usize] = &[
    ss::target::NONE,
    ss::target::PITCH,
    ss::target::SAMPLE_START,
    ss::target::F1_CUT,
    ss::target::F1_RES,
    ss::target::F1_DRIVE,
    ss::target::VOLUME,
    ss::target::PAN,
    ss::target::GLIDE,
    ss::target::BITS,
];

// Quick Sampler's own ids (the first ones are kept from Studio's earlier Quick Sampler, so its
// saved settings still load).
pub const MODE: u32 = 0;
pub const ROOT: u32 = 1;
pub const START: u32 = 2;
pub const END: u32 = 3;
/// (The earlier Loop on / off switch; Loop Mode replaces it.)
pub const LEGACY_LOOP: u32 = 4;
pub const LOOP_START: u32 = 5;
pub const LOOP_END: u32 = 6;
pub const CUTOFF: u32 = 11;
pub const RESO: u32 = 12;
pub const VOLUME: u32 = 13;
pub const SENSITIVITY: u32 = 14;
pub const PITCH_TRACK: u32 = 15;
pub const TUNE: u32 = 16;
pub const REVERSE: u32 = 17;
pub const BEND: u32 = 18;
pub const FILTER_ON: u32 = 19;
pub const LOOP_MODE: u32 = 20;
pub const COARSE: u32 = 1000;
pub const FINE: u32 = 1001;
pub const POLY: u32 = 1002;
pub const PITCH_ENV_DEPTH: u32 = 1003;
pub const FILTER_ENV_DEPTH: u32 = 1004;
pub const KEYSCALE: u32 = 1005;
pub const FADE_IN: u32 = 1006;
pub const FADE_OUT: u32 = 1007;
pub const XFADE: u32 = 1008;
pub const XFADE_CURVE: u32 = 1009;
pub const SLICE_MODE: u32 = 1010;
pub const DIVISION: u32 = 1011;
pub const SLICES: u32 = 1012;
pub const START_KEY: u32 = 1013;
pub const KEY_MAPPING: u32 = 1014;
pub const GATE: u32 = 1015;
pub const PLAY_TO_END: u32 = 1016;
/// LFO k (1, 2): `lfo_route(k)` + [amount, via, via amount (max), target (index in LFO_TARGETS)].
pub fn lfo_route(k: usize) -> u32 {
    1020 + (k as u32 - 1) * 4
}
pub const MONITOR: u32 = 1030;
pub const RECORD_START: u32 = 1031;
pub const THRESHOLD: u32 = 1032;
pub const FLEX: u32 = 1040;
pub const FOLLOW_TEMPO: u32 = 1041;
pub const FLEX_SPEED: u32 = 1042;
/// The sample's tempo (0: not known), from analysis or Derive Tempo from Loop Length.
pub const FILE_TEMPO: u32 = 1043;
/// The Pitch, Filter and Amp panes' envelopes (the Sampler's Envelope 2, 3 and 1).
pub const PITCH_ENV: usize = 2;
pub const FILTER_ENV: usize = 3;
/// The Mod Matrix rows Quick Sampler shows; the hard-wired routings use rows from 14 on.
pub const ROUTINGS: usize = 4;
const ROW_LFO: [usize; 2] = [14, 15];
const ROW_KEYSCALE: usize = 17;
const ROW_FILTER_ENV: usize = 18;
const ROW_PITCH_ENV: usize = 19;

/// The parameters: Quick Sampler's own, then the Sampler synth's that it shows.
pub fn params() -> &'static [Param] {
    static P: OnceLock<Vec<Param>> = OnceLock::new();
    P.get_or_init(|| {
        let mut v = vec![
            Param::menu(MODE, "Mode", MODES, 0.0),
            Param::new(ROOT, "Root Key", 0.0, 127.0, 60.0, Unit::None),
            Param::new(START, "Sample Start", 0.0, 1.0, 0.0, Unit::Ratio),
            Param::new(END, "Sample End", 0.0, 1.0, 1.0, Unit::Ratio),
            Param::new(LEGACY_LOOP, "Loop", 0.0, 1.0, 0.0, Unit::OnOff).hide(),
            Param::new(LOOP_START, "Loop Start", 0.0, 1.0, 0.5, Unit::Ratio),
            Param::new(LOOP_END, "Loop End", 0.0, 1.0, 1.0, Unit::Ratio),
            // Studio's earlier amp envelope ids: they now set Envelope 1
            Param::new(7, "Attack", 0.0, 10.0, 0.0, Unit::Seconds).hide(),
            Param::new(8, "Decay", 0.0, 20.0, 0.0, Unit::Seconds).hide(),
            Param::new(9, "Sustain", 0.0, 1.0, 1.0, Unit::Ratio).hide(),
            Param::new(10, "Release", 0.0, 20.0, 0.1, Unit::Seconds).hide(),
            Param::new(CUTOFF, "Cutoff", 20.0, 20000.0, 20000.0, Unit::Hz),
            Param::new(RESO, "Reso", 0.0, 1.0, 0.0, Unit::Ratio),
            Param::new(VOLUME, "Volume", -40.0, 12.0, 0.0, Unit::Db),
            Param::new(SENSITIVITY, "Sensitivity", 0.0, 1.0, 0.5, Unit::Ratio),
            Param::new(PITCH_TRACK, "Pitch", 0.0, 1.0, 1.0, Unit::OnOff).hide(),
            Param::new(TUNE, "Tune", -100.0, 100.0, 0.0, Unit::Cents),
            Param::new(REVERSE, "Playback Reverse", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(BEND, "Bend Range", 0.0, 36.0, 2.0, Unit::Semitones),
            Param::new(FILTER_ON, "Filter", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::menu(LOOP_MODE, "Loop Mode", LOOP_MODES, 0.0),
            Param::new(COARSE, "Coarse", -36.0, 36.0, 0.0, Unit::Semitones),
            Param::new(FINE, "Fine", -100.0, 100.0, 0.0, Unit::Cents),
            Param::menu(POLY, "Polyphony", POLYPHONY, 11.0),
            Param::new(PITCH_ENV_DEPTH, "Pitch Env Depth", -24.0, 24.0, 0.0, Unit::Semitones),
            Param::new(FILTER_ENV_DEPTH, "Filter Env Depth", -1.0, 1.0, 0.0, Unit::Ratio),
            Param::new(KEYSCALE, "Keyscale", 0.0, 1.0, 0.0, Unit::Ratio),
            Param::new(FADE_IN, "Fade In", 0.0, 5.0, 0.0, Unit::Seconds),
            Param::new(FADE_OUT, "Fade Out", 0.0, 5.0, 0.0, Unit::Seconds),
            Param::new(XFADE, "Xfade", 0.0, 2.0, 0.0, Unit::Seconds),
            Param::menu(XFADE_CURVE, "Crossfade Curve", XFADE_CURVES, 0.0),
            Param::menu(SLICE_MODE, "Slice Mode", SLICE_MODES, 0.0),
            Param::menu(DIVISION, "Division", DIVISIONS, 2.0),
            Param::new(SLICES, "Slices", 1.0, 64.0, 8.0, Unit::None),
            Param::new(START_KEY, "Start Key", 0.0, 127.0, 36.0, Unit::None),
            Param::menu(KEY_MAPPING, "Start Key Mapping", KEY_MAPPINGS, 0.0),
            Param::new(GATE, "Gate", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(PLAY_TO_END, "Play to End", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(MONITOR, "Monitor", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::menu(RECORD_START, "Record Start", RECORD_STARTS, 0.0),
            Param::new(THRESHOLD, "Threshold", -60.0, 0.0, -30.0, Unit::Db),
            Param::new(FLEX, "Flex", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::new(FOLLOW_TEMPO, "Follow Tempo", 0.0, 1.0, 0.0, Unit::OnOff),
            Param::menu(FLEX_SPEED, "Flex Speed", FLEX_SPEEDS, 2.0),
            Param::new(FILE_TEMPO, "Sample Tempo", 0.0, 300.0, 0.0, Unit::None).hide(),
        ];
        static TARGET_NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
        let tnames: &'static [&'static str] = TARGET_NAMES.get_or_init(|| LFO_TARGETS.iter().map(|&t| ss::TARGETS[t]).collect()).as_slice();
        for k in 1..=2 {
            let b = lfo_route(k);
            let n = |s: &str| -> &'static str { Box::leak(format!("LFO {k} {s}").into_boxed_str()) };
            v.extend([
                Param::new(b, n("Amount"), -1.0, 1.0, 0.0, Unit::None),
                Param::menu(b + 1, n("Via"), ss::sources(), 0.0),
                Param::new(b + 2, n("Via Amount"), -1.0, 1.0, 0.0, Unit::None),
                Param::menu(b + 3, n("Target"), tnames, if k == 1 { 1.0 } else { 3.0 }),
            ]);
        }
        // the Sampler synth's parameters Quick Sampler has
        let shown = |id: u32| -> bool {
            matches!(id, ss::F1_TYPE | ss::F1_DRIVE | ss::PAN | ss::GLIDE | ss::MONO_MODE | ss::MONO_BEND)
                || (ss::ENV1..ss::ENV1 + 9).contains(&id)
                || [PITCH_ENV, FILTER_ENV].iter().any(|&k| (ss::env_base(k) + 1..ss::env_base(k) + 10).contains(&id))
                || (1..=2).any(|k| (ss::lfo_base(k)..ss::lfo_base(k) + 11).contains(&id))
                || (0..ROUTINGS).any(|r| (ss::mod_base(r)..ss::mod_base(r) + 8).contains(&id))
        };
        for p in crate::sampler::all_params().iter().filter(|p| shown(p.id)) {
            let mut p = p.clone();
            // Studio's factory values: a short amp release, Filter 1 a 24 dB Lush lowpass, the
            // envelopes of the Pitch and Filter panes AHDSR with no sustain, the routings empty
            // (the Sampler's first one picks velocity layers)
            if p.id == ss::ENV1 + 7 {
                p.default = 0.1;
            } else if p.id == ss::F1_TYPE {
                p.default = 5.0;
            } else if [ss::mod_base(0), ss::mod_base(0) + 1, ss::mod_base(0) + 5, ss::mod_base(0) + 6].contains(&p.id) {
                p.default = 0.0;
            } else if [PITCH_ENV, FILTER_ENV].iter().any(|&k| p.id == ss::env_base(k) + 7 || p.id == ss::env_base(k) + 1) {
                p.default = 0.0;
            }
            v.push(p);
        }
        v
    })
    .as_slice()
}

/// Is this id one the Sampler engine keeps itself (forwarded as it is)?
fn forwarded(id: u32) -> bool {
    (21..1000).contains(&id)
}

/// Slice markers placed or kept by hand: `manual` ignore Sensitivity (orange), `removed` are
/// detected ones taken away.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SliceEdits {
    pub manual: Vec<usize>,
    pub removed: Vec<usize>,
}

/// Recorder mode's state (filled by the side chain on the audio thread).
#[derive(Default)]
pub struct Recorder {
    pub armed: bool,
    pub running: bool,
    pub buf: [Vec<f32>; 2],
    cap: usize,
    /// The input's peak level (0..1), decaying, for the meter.
    pub level: f32,
    mon: [Vec<f32>; 2],
}

pub struct QuickSampler {
    params: crate::poly::Params,
    audio: Option<Audio>,
    data: Option<Arc<SampleData>>,
    inner: Sampler,
    /// Slice start frames now (Slice mode), with whether each was placed by hand.
    pub slices: Vec<usize>,
    pub slice_manual: Vec<bool>,
    /// Slices found by the slice mode alone (no edits).
    pub detected: Vec<usize>,
    pub edits: SliceEdits,
    /// The base zone (Classic / One Shot) or one per slice, as the engine has them without Flex.
    zones: Vec<Zone>,
    /// Flex: the zone made for each key (index into the engine's zones).
    flex_zone: Vec<Option<usize>>,
    rate: f32,
    bpm: f32,
    pub rec: Recorder,
    /// The editor's revision this sample came from.
    pub doc_rev: u64,
}

impl QuickSampler {
    pub fn new(rate: f32) -> QuickSampler {
        let mut q = QuickSampler {
            params: crate::poly::Params::new(params()),
            audio: None,
            data: None,
            inner: Sampler::new(Vec::new(), rate),
            slices: Vec::new(),
            slice_manual: Vec::new(),
            detected: Vec::new(),
            edits: SliceEdits::default(),
            zones: Vec::new(),
            flex_zone: vec![None; 128],
            rate,
            bpm: 120.0,
            rec: Recorder::default(),
            doc_rev: 0,
        };
        for p in params() {
            q.forward(p.id, p.default);
        }
        // no vibrato on the mod wheel (the Sampler's own default)
        q.inner.set(14, 0.0);
        q.wire();
        q
    }
    pub fn from_file(path: &Path, rate: f32) -> std::io::Result<QuickSampler> {
        let b = std::fs::read(path)?;
        let a = crate::wav::decode_audio(&b).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "unsupported audio file"))?;
        let mut q = QuickSampler::new(rate);
        q.set_audio(a);
        Ok(q)
    }
    /// Loads audio as it is (Original): its root key (or the detected pitch) and its loop.
    pub fn set_audio(&mut self, a: Audio) {
        let root = a.root.map(|r| r as f32).or_else(|| detect_root(&a));
        if let Some(r) = root {
            self.params.set(ROOT, r);
        }
        if let Some((s, e)) = a.loop_points {
            let n = a.frames().max(1) as f32;
            self.params.set(LOOP_MODE, 1.0);
            self.params.set(LOOP_START, s as f32 / n);
            self.params.set(LOOP_END, e as f32 / n);
        }
        self.params.set(FILE_TEMPO, detect_tempo(&a).unwrap_or(0.0));
        self.edits = SliceEdits::default();
        self.audio = Some(a);
        self.rebuild_data();
        self.rebuild();
    }
    /// The audio without changing any setting (undo, the editor's document).
    pub fn replace_audio(&mut self, a: Audio) {
        self.audio = Some(a);
        self.rebuild_data();
        self.rebuild();
    }
    fn rebuild_data(&mut self) {
        let Some(a) = &self.audio else { return };
        let ch = a.channels.iter().take(2).map(|c| Pcm::F32(c.clone())).collect();
        self.data = Some(Arc::new(SampleData { rate: a.rate as f32, ch }));
    }
    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    /// A Quick Sampler parameter into the engine.
    fn forward(&mut self, id: u32, v: f32) {
        match id {
            CUTOFF => self.inner.set(6, v),
            RESO => self.inner.set(7, v),
            VOLUME => self.inner.set(0, v),
            BEND => self.inner.set(12, v),
            FILTER_ON => self.inner.set(5, v),
            COARSE => self.inner.set(10, v),
            FINE => self.inner.set(11, v),
            POLY => {
                let k = v.round() as usize;
                let (mode, voices) = match k {
                    0 => (1.0, 1.0),
                    1 => (2.0, 1.0),
                    _ => (0.0, POLYPHONY.get(k).and_then(|s| s.parse::<f32>().ok()).unwrap_or(32.0)),
                };
                self.inner.set(ss::MODE, mode);
                self.inner.set(13, voices);
            }
            id if forwarded(id) => self.inner.set(id, v),
            _ => {}
        }
    }
    /// The hard-wired routings: the Pitch and Filter envelopes, Keyscale and the LFOs' own.
    fn wire(&mut self) {
        fn route(s: &mut Sampler, row: usize, src: usize, target: usize, amount: f32, via: usize, max: f32) {
            let b = ss::mod_base(row);
            let on = target != 0 && (amount != 0.0 || (via != 0 && max != 0.0));
            s.set(b, if on { 1.0 } else { 0.0 });
            s.set(b + 1, src as f32);
            s.set(b + 2, 0.0);
            s.set(b + 3, via as f32);
            s.set(b + 4, 0.0);
            s.set(b + 5, target as f32);
            s.set(b + 6, amount);
            s.set(b + 7, max);
        }
        let pe = self.p(PITCH_ENV_DEPTH) / 24.0;
        let fe = self.p(FILTER_ENV_DEPTH);
        let ks = self.p(KEYSCALE);
        let s = &mut self.inner;
        s.set(ss::env_base(PITCH_ENV), 1.0);
        s.set(ss::env_base(FILTER_ENV), 1.0);
        route(s, ROW_PITCH_ENV, ss::source_index(ss::Source::Env(PITCH_ENV as u8 - 1)), ss::target::PITCH, pe, 0, 0.0);
        route(s, ROW_FILTER_ENV, ss::source_index(ss::Source::Env(FILTER_ENV as u8 - 1)), ss::target::F1_CUT, fe, 0, 0.0);
        route(s, ROW_KEYSCALE, ss::source_index(ss::Source::Key), ss::target::F1_CUT, ks, 0, 0.0);
        for k in 1..=2 {
            let b = lfo_route(k);
            let amount = self.params.get(b);
            let via = self.params.get(b + 1).round().max(0.0) as usize;
            let max = self.params.get(b + 2);
            let target = LFO_TARGETS.get(self.params.get(b + 3).round().max(0.0) as usize).copied().unwrap_or(0);
            route(&mut self.inner, ROW_LFO[k - 1], ss::source_index(ss::Source::Lfo(k as u8 - 1)), target, amount, via, if via != 0 { max } else { 0.0 });
        }
    }
    fn frames(&self) -> usize {
        self.audio.as_ref().map(|a| a.frames()).unwrap_or(0)
    }
    /// The slice starts the slice mode gives (before hand edits).
    fn find_slices(&self, start: usize, end: usize) -> Vec<usize> {
        let Some(data) = &self.data else { return Vec::new() };
        match self.p(SLICE_MODE).round() as i32 {
            0 => detect_slices(data, start, end, self.p(SENSITIVITY)),
            1 => {
                // beat values at the sample's tempo (else the project's)
                let bpm = if self.p(FILE_TEMPO) > 0.0 { self.p(FILE_TEMPO) } else { self.bpm };
                let beats = [4.0, 2.0, 1.0, 0.5, 0.25, 0.125][(self.p(DIVISION).round() as usize).min(5)];
                let step = ((60.0 / bpm.max(20.0)) * beats * data.rate) as usize;
                (start..end).step_by(step.max(16)).collect()
            }
            2 => {
                let n = self.p(SLICES).round().max(1.0) as usize;
                (0..n).map(|k| start + (end - start) * k / n).collect()
            }
            _ => vec![start],
        }
    }
    fn rebuild(&mut self) {
        let Some(data) = self.data.clone() else {
            return;
        };
        let n = self.frames();
        if n < 2 {
            return;
        }
        let p = |i| self.params.get(i);
        let start = (p(START) * n as f32) as usize;
        let end = ((p(END) * n as f32) as usize).clamp(start + 1, n);
        let mode = p(MODE).round() as i32;
        let sr = data.rate;
        let mut base = Zone::new(data.clone());
        base.root = p(ROOT).round().clamp(0.0, 127.0) as u8;
        base.tune_cents = p(TUNE);
        base.keytrack = if p(PITCH_TRACK) >= 0.5 { 100.0 } else { 0.0 };
        base.start = start;
        base.end = end;
        base.synth_env = true;
        base.reverse = p(REVERSE) >= 0.5;
        base.fade_in = (p(FADE_IN) * sr) as usize;
        base.fade_out = (p(FADE_OUT) * sr) as usize;
        let mut zones = Vec::new();
        match mode {
            1 => {
                base.loop_mode = LoopMode::OneShot;
                zones.push(base);
            }
            2 => {
                self.detected = self.find_slices(start, end);
                let tol = (sr * 0.004) as usize;
                let near = |a: usize, b: usize| a.abs_diff(b) <= tol;
                let mut marks: Vec<(usize, bool)> = self
                    .detected
                    .iter()
                    .filter(|&&d| d == start || !self.edits.removed.iter().any(|&r| near(r, d)))
                    .filter(|&&d| !self.edits.manual.iter().any(|&m| near(m, d)))
                    .map(|&d| (d, false))
                    .collect();
                marks.extend(self.edits.manual.iter().filter(|&&m| m >= start && m < end).map(|&m| (m, true)));
                marks.sort_unstable_by_key(|m| m.0);
                marks.dedup_by_key(|m| m.0);
                if !marks.iter().any(|m| near(m.0, start)) {
                    marks.insert(0, (start, false));
                }
                self.slices = marks.iter().map(|m| m.0).collect();
                self.slice_manual = marks.iter().map(|m| m.1).collect();
                let keys = slice_keys(p(START_KEY).round().clamp(0.0, 127.0) as u8, p(KEY_MAPPING).round() as u8, self.slices.len());
                let gate = p(GATE) >= 0.5;
                let to_end = p(PLAY_TO_END) >= 0.5;
                for (k, &s0) in self.slices.iter().enumerate() {
                    let Some(&key) = keys.get(k) else { break };
                    let s1 = if to_end { end } else { self.slices.get(k + 1).copied().unwrap_or(end) };
                    let mut z = base.clone();
                    z.lokey = key;
                    z.hikey = key;
                    z.root = key;
                    z.start = s0;
                    z.end = s1.max(s0 + 1);
                    z.loop_mode = if gate { LoopMode::NoLoop } else { LoopMode::OneShot };
                    z.keytrack = 100.0;
                    zones.push(z);
                }
            }
            _ => {
                let lm = match p(LOOP_MODE).round() as i32 {
                    0 if p(LEGACY_LOOP) >= 0.5 => 1,
                    m => m,
                };
                if lm > 0 {
                    let ls = (p(LOOP_START) * n as f32) as usize;
                    let le = ((p(LOOP_END) * n as f32) as usize).min(end);
                    if le > ls + 16 {
                        base.loop_mode = if lm == 4 { LoopMode::Sustain } else { LoopMode::Continuous };
                        base.loop_dir = match lm {
                            2 => LoopDir::Reverse,
                            3 => LoopDir::Alternate,
                            _ => LoopDir::Forward,
                        };
                        base.loop_start = ls.max(start);
                        base.loop_end = le;
                        base.loop_xfade = ((p(XFADE) * sr) as usize).min((le - ls) / 2);
                        base.loop_eq_power = p(XFADE_CURVE).round() as i32 == 0;
                    }
                }
                zones.push(base);
            }
        }
        self.zones = zones.clone();
        self.flex_zone = vec![None; 128];
        // with Flex the zones are made per key as notes come
        self.inner.set_zones(if self.flex_on() { Vec::new() } else { zones });
    }
    fn flex_on(&self) -> bool {
        self.p(FLEX) >= 0.5 && self.p(MODE).round() as i32 != 3
    }
    /// Flex's playback speed: the Speed choice, times project / sample tempo with Follow Tempo.
    pub fn flex_speed(&self) -> f32 {
        let s = [0.25, 0.5, 1.0, 2.0, 4.0][(self.p(FLEX_SPEED).round() as usize).min(4)];
        let ft = self.p(FILE_TEMPO);
        if self.p(FOLLOW_TEMPO) >= 0.5 && ft > 0.0 { s * self.bpm / ft } else { s }
    }
    /// Flex: the zone that plays `key` at the sample's speed (stretched by the pitch ratio, so
    /// resampling to the key's pitch gives the original length back).
    fn flex_zone_for(&mut self, key: u8) {
        if self.flex_zone[key as usize].is_some() {
            return;
        }
        let Some(z) = self.zones.iter().find(|z| z.lokey <= key && key <= z.hikey).cloned() else { return };
        let ratio = 2f64.powf((key as f64 - z.root as f64) * z.keytrack as f64 / 1200.0) / self.flex_speed() as f64;
        let (s, e) = (z.start.min(z.end), z.end);
        let chans: Vec<&[f32]> = z
            .sample
            .ch
            .iter()
            .filter_map(|c| match c {
                Pcm::F32(v) => Some(&v[s.min(v.len())..e.min(v.len())]),
                Pcm::I16(_) => None,
            })
            .collect();
        if chans.is_empty() || chans[0].is_empty() {
            return;
        }
        let nc = chans.len();
        let len = chans[0].len();
        let mut inter = Vec::with_capacity(len * nc);
        for i in 0..len {
            for c in &chans {
                inter.push(c.get(i).copied().unwrap_or(0.0));
            }
        }
        let out = lg_audio::stretch::stretch(&inter, nc, z.sample.rate as u32, ratio);
        let frames = out.len() / nc;
        let ch: Vec<Pcm> = (0..nc).map(|c| Pcm::F32((0..frames).map(|i| out[i * nc + c]).collect())).collect();
        let data = Arc::new(SampleData { rate: z.sample.rate, ch });
        let at = |x: usize| ((x.saturating_sub(s) as f64) * ratio) as usize;
        let mut fz = z.clone();
        fz.sample = data;
        fz.lokey = key;
        fz.hikey = key;
        fz.start = 0;
        fz.end = frames;
        fz.loop_start = at(z.loop_start).min(frames);
        fz.loop_end = at(z.loop_end).min(frames);
        fz.loop_xfade = ((z.loop_xfade as f64) * ratio) as usize;
        fz.fade_in = ((z.fade_in as f64) * ratio) as usize;
        fz.fade_out = ((z.fade_out as f64) * ratio) as usize;
        self.flex_zone[key as usize] = Some(self.inner.add_zone(fz));
    }
}

impl QuickSampler {
    /// The loaded sample (for the waveform display).
    pub fn audio(&self) -> Option<&Audio> {
        self.audio.as_ref()
    }
    /// Slice markers by hand: `Some(list)` makes exactly these the slices (the earlier API),
    /// None goes back to the found ones.
    pub fn set_slices(&mut self, s: Option<Vec<usize>>) {
        match s {
            Some(v) => {
                let removed = self.detected.iter().copied().filter(|d| !v.contains(d)).collect();
                self.edits = SliceEdits { manual: v, removed };
            }
            None => self.edits = SliceEdits::default(),
        }
        self.rebuild();
    }
    pub fn set_slice_edits(&mut self, e: SliceEdits) {
        self.edits = e;
        self.rebuild();
    }
    /// A copy with the same sample and settings.
    pub fn duplicate(&self) -> QuickSampler {
        let mut q = QuickSampler::new(self.rate);
        for p in params() {
            q.params.set(p.id, self.params.get(p.id));
            q.forward(p.id, self.params.get(p.id));
        }
        q.wire();
        q.audio = self.audio.clone();
        q.edits = self.edits.clone();
        q.rebuild_data();
        q.rebuild();
        q
    }
    /// Stops the sound at once (a choke).
    pub fn choke(&mut self) {
        self.inner.choke();
    }
    /// The zones as the engine plays them without Flex (Create Sampler Track, Open in Sampler).
    pub fn zones(&self) -> &[Zone] {
        &self.zones
    }
    /// Voices sounding now.
    pub fn used_voices(&self) -> usize {
        self.inner.used_voices()
    }
    /// Recorder: arms recording (room for `seconds`); it starts at once or when the input
    /// passes the threshold.
    pub fn record_start(&mut self, seconds: f32) {
        let cap = (seconds * self.rate) as usize;
        self.rec.buf = [Vec::with_capacity(cap), Vec::with_capacity(cap)];
        self.rec.cap = cap;
        self.rec.armed = true;
        self.rec.running = self.p(RECORD_START).round() as i32 == 0;
    }
    /// Stops recording; the recording (if any) becomes the sample.
    pub fn record_stop(&mut self) -> Option<Audio> {
        let was = self.rec.armed;
        self.rec.armed = false;
        self.rec.running = false;
        let [l, r] = std::mem::take(&mut self.rec.buf);
        if !was || l.len() < 64 {
            return None;
        }
        let a = Audio { rate: self.rate as u32, channels: vec![l, r], loop_points: None, root: None };
        self.set_audio(a.clone());
        Some(a)
    }
}

/// Keys for `n` slices from `start`: every key, the white keys or the black keys.
pub fn slice_keys(start: u8, mapping: u8, n: usize) -> Vec<u8> {
    let black = |k: u8| matches!(k % 12, 1 | 3 | 6 | 8 | 10);
    let mut out = Vec::new();
    let mut k = start as u32;
    while out.len() < n && k <= 127 {
        let kk = k as u8;
        let ok = out.is_empty()
            || match mapping {
                1 => !black(kk),
                2 => black(kk),
                _ => true,
            };
        if ok {
            out.push(kk);
        }
        k += 1;
    }
    out
}

/// Pitch of the loudest stretch by autocorrelation (50–2000 Hz), as a MIDI note.
fn detect_root(a: &Audio) -> Option<f32> {
    detect_pitch(a).map(|m| m.round())
}

/// The sample's pitch as a (fractional) MIDI note.
pub fn detect_pitch(a: &Audio) -> Option<f32> {
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
    if bv < 0.5 || bl < 2 {
        return None;
    }
    // parabolic refinement of the peak, for the cents
    let ac = |lag: usize| (0..win - lag).map(|k| x[k] * x[k + lag]).sum::<f32>();
    let (y0, y1, y2) = (ac(bl - 1), ac(bl), ac(bl + 1));
    let den = y0 - 2.0 * y1 + y2;
    let d = if den.abs() > 1e-9 { (0.5 * (y0 - y2) / den).clamp(-0.5, 0.5) } else { 0.0 };
    let hz = rate / (bl as f32 + d);
    Some(69.0 + 12.0 * (hz / 440.0).log2())
}

/// The tempo of rhythmic material (None for short or unrhythmic sounds).
pub fn detect_tempo(a: &Audio) -> Option<f32> {
    let n = a.frames();
    if n < a.rate as usize * 2 || a.channels.is_empty() {
        return None;
    }
    let mono: Vec<f32> = (0..n).map(|i| a.channels.iter().map(|c| c[i]).sum::<f32>() / a.channels.len() as f32).collect();
    let env = lg_audio::transients::onset_strength(&mono, a.rate);
    let (_, hop) = lg_audio::transients::frame_hop(a.rate);
    lg_audio::tempo::estimate_tempo(&env, a.rate as f64 / hop as f64).map(|t| t as f32)
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
    let peak = (start..end).step_by(4).map(|k| at(k).abs()).fold(0.0f32, f32::max);
    while i + frame < end {
        let e = ((i..i + frame).map(|k| at(k) * at(k)).sum::<f32>() / frame as f32).sqrt();
        if e > prev * ratio + 1e-4 && e > peak * 0.05 * (1.2 - sens) && i - last > (rate * 0.05) as usize {
            out.push(i);
            last = i;
        }
        prev = prev * 0.6 + e * 0.4;
        i += frame;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The Action menu's analysis (own methods: how well the loop's ends join)

fn mono_of(a: &Audio) -> Vec<f32> {
    let n = a.frames();
    let c = a.channels.len().max(1) as f32;
    (0..n).map(|i| a.channels.iter().map(|ch| ch[i]).sum::<f32>() / c).collect()
}

/// How well a loop from `ls` to `le` joins: the difference between the samples before the end
/// and before the start, against their level (lower is smoother).
fn loop_cost(m: &[f32], ls: usize, le: usize, w: usize) -> f32 {
    if ls < w || le > m.len() || le <= ls + w {
        return f32::MAX;
    }
    let mut d = 0.0;
    for k in 0..w {
        let x = m[le - w + k] - m[ls - w + k];
        d += x * x;
    }
    let e: f32 = m[le - w..le].iter().map(|x| x * x).sum::<f32>() + 1e-9;
    d / e
}

/// Auto-Loop: a loop in the later, steadier part of the sample (or inside `within`); each
/// `attempt` tries another, shorter one.
pub fn auto_loop(a: &Audio, start: usize, end: usize, within: Option<(usize, usize)>, attempt: u32) -> Option<(usize, usize)> {
    let m = mono_of(a);
    let rate = a.rate as usize;
    let w = (rate / 200).max(32);
    let (lo, hi) = match within {
        Some((s, e)) => (s.max(start), e.min(end)),
        None => (start + (end - start) / 3, end.saturating_sub(rate / 50)),
    };
    if hi <= lo + 4 * w {
        return None;
    }
    // the loop length: whole periods near a target that shrinks with each attempt
    let period = detect_pitch(a).map(|n| (a.rate as f32 / (440.0 * 2f32.powf((n - 69.0) / 12.0))) as usize).unwrap_or(rate / 100).max(8);
    let span = hi - lo;
    let target = (span as f32 * 0.8 / (1.0 + attempt as f32 * 0.6)) as usize;
    let len = ((target / period).max(2) * period).min(span.saturating_sub(w)).max(w + 1);
    let mut best = (f32::MAX, lo, lo + len);
    let step = (period / 4).max(1);
    let mut s = lo.max(w);
    while s + len <= hi {
        let mut de = 0;
        while de < period {
            let e = (s + len + de).min(hi);
            let c = loop_cost(&m, s, e, w);
            if c < best.0 {
                best = (c, s, e);
            }
            de += step;
        }
        s += step * 8;
    }
    (best.0 < f32::MAX).then_some((best.1, best.2))
}

/// Optimize Loop Start: the start (within 20 ms) that joins the end best.
pub fn optimize_loop_start(a: &Audio, ls: usize, le: usize) -> usize {
    let m = mono_of(a);
    let w = (a.rate as usize / 200).max(32);
    let reach = a.rate as usize / 50;
    (ls.saturating_sub(reach)..(ls + reach).min(le.saturating_sub(w * 2)))
        .min_by(|&x, &y| loop_cost(&m, x, le, w).partial_cmp(&loop_cost(&m, y, le, w)).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(ls)
}

/// Optimize Loop Crossfade / End: a crossfade (seconds) for the loop's join without moving it,
/// longer when the ends differ more.
pub fn optimize_crossfade(a: &Audio, ls: usize, le: usize) -> f32 {
    let m = mono_of(a);
    let w = (a.rate as usize / 200).max(32);
    let c = loop_cost(&m, ls, le, w).min(4.0);
    let max = (le.saturating_sub(ls) as f32 / a.rate as f32 / 2.0).min(0.5);
    (0.005 + c * 0.05).min(max)
}

/// Optimize Sample Gain: the Volume (dB) that brings the played part's peak to -1 dBFS.
pub fn optimize_gain(a: &Audio, start: usize, end: usize) -> f32 {
    let peak = a.channels.iter().flat_map(|c| c[start.min(c.len())..end.min(c.len())].iter()).fold(0.0f32, |m, x| m.max(x.abs()));
    if peak <= 1e-6 { 0.0 } else { (-1.0 - 20.0 * peak.log10()).clamp(-40.0, 12.0) }
}

/// The frames from the first sound above -60 dBFS to just past the last (silence cropped).
pub fn sound_bounds(a: &Audio) -> (usize, usize) {
    let th = 0.001;
    let n = a.frames();
    let loud = |i: usize| a.channels.iter().any(|c| c[i].abs() > th);
    let s = (0..n).find(|&i| loud(i)).unwrap_or(0);
    let e = (0..n).rev().find(|&i| loud(i)).map(|i| (i + a.rate as usize / 100).min(n)).unwrap_or(n);
    (s, e.max(s + 1))
}

/// Derive Tempo from Loop Length: the tempo that makes the loop a power-of-two number of beats,
/// in 70–180 BPM.
pub fn tempo_from_length(frames: usize, rate: u32) -> Option<f32> {
    let secs = frames as f32 / rate as f32;
    if secs <= 0.05 {
        return None;
    }
    let mut beats = 1.0f32;
    while beats <= 256.0 {
        let bpm = 60.0 * beats / secs;
        if (70.0..=180.0).contains(&bpm) {
            return Some(bpm);
        }
        beats *= 2.0;
    }
    None
}

impl Plugin for QuickSampler {
    fn params(&self) -> &[Param] {
        params()
    }
    fn get(&self, id: u32) -> f32 {
        match id {
            7 => self.params.get(ss::ENV1 + 2),
            8 => self.params.get(ss::ENV1 + 5),
            9 => self.params.get(ss::ENV1 + 6),
            10 => self.params.get(ss::ENV1 + 7),
            _ => self.params.get(id),
        }
    }
    fn set(&mut self, id: u32, v: f32) {
        // the earlier amp envelope ids set Envelope 1; the earlier Loop switch, Loop Mode
        let id = match id {
            7 => ss::ENV1 + 2,
            8 => ss::ENV1 + 5,
            9 => ss::ENV1 + 6,
            10 => ss::ENV1 + 7,
            _ => id,
        };
        if !self.params.set(id, v) {
            return;
        }
        let v = self.params.get(id);
        if id == LEGACY_LOOP && v >= 0.5 && self.params.get(LOOP_MODE) < 0.5 {
            self.params.set(LOOP_MODE, 1.0);
        }
        self.forward(id, v);
        match id {
            PITCH_ENV_DEPTH | FILTER_ENV_DEPTH | KEYSCALE => self.wire(),
            id if (lfo_route(1)..lfo_route(2) + 4).contains(&id) => self.wire(),
            MODE | ROOT | START | END | LEGACY_LOOP | LOOP_START | LOOP_END | SENSITIVITY | PITCH_TRACK | TUNE | REVERSE | LOOP_MODE | FADE_IN | FADE_OUT | XFADE | XFADE_CURVE | SLICE_MODE | DIVISION | SLICES | START_KEY | KEY_MAPPING | GATE | PLAY_TO_END | FLEX | FOLLOW_TEMPO | FLEX_SPEED | FILE_TEMPO => self.rebuild(),
            _ => {}
        }
    }
    fn reset(&mut self) {
        self.inner.reset();
        self.rebuild();
    }
    fn tail(&self) -> f32 {
        self.params.get(ss::ENV1 + 7) + 0.1
    }
}

impl Instrument for QuickSampler {
    fn note_on(&mut self, p: u8, v: u8) {
        if self.flex_on() && v > 0 {
            self.flex_zone_for(p);
        }
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
    fn pressure(&mut self, v: u8) {
        self.inner.pressure(v)
    }
    fn set_tempo(&mut self, bpm: f64) {
        let b = bpm as f32;
        if (b - self.bpm).abs() > 0.01 {
            self.bpm = b;
            let follows = self.flex_on() && self.p(FOLLOW_TEMPO) >= 0.5;
            let beats = self.p(MODE).round() as i32 == 2 && self.p(SLICE_MODE).round() as i32 == 1 && self.p(FILE_TEMPO) <= 0.0;
            if follows || beats {
                self.rebuild();
            }
        }
        self.inner.set_tempo(bpm);
    }
    fn side_chain(&mut self, l: &[f32], r: &[f32]) {
        let peak = l.iter().chain(r.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
        self.rec.level = peak.max(self.rec.level * 0.9);
        if self.p(MODE).round() as i32 == 3 && self.p(MONITOR) >= 0.5 {
            self.rec.mon[0].clear();
            self.rec.mon[0].extend_from_slice(l);
            self.rec.mon[1].clear();
            self.rec.mon[1].extend_from_slice(r);
        } else {
            self.rec.mon[0].clear();
            self.rec.mon[1].clear();
        }
        if !self.rec.armed {
            return;
        }
        if !self.rec.running {
            if peak > lg_dsp::db_to_gain(self.p(THRESHOLD)) {
                self.rec.running = true;
            } else {
                return;
            }
        }
        let room = self.rec.cap.saturating_sub(self.rec.buf[0].len());
        let n = l.len().min(r.len()).min(room);
        self.rec.buf[0].extend_from_slice(&l[..n]);
        self.rec.buf[1].extend_from_slice(&r[..n]);
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        self.inner.render(l, r);
        let [ml, mr] = &self.rec.mon;
        for (o, x) in l.iter_mut().zip(ml) {
            *o += x;
        }
        for (o, x) in r.iter_mut().zip(mr) {
            *o += x;
        }
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

/// Writes the loop (and root key) into a WAV file's 'smpl' chunk, the audio kept as it is
/// (Write Sample Loop to Audio File).
pub fn write_wav_loop(path: &Path, root: u8, lp: (u32, u32)) -> std::io::Result<()> {
    let b = std::fs::read(path)?;
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "not a WAV file"));
    }
    let mut out = b[..12].to_vec();
    let mut o = 12;
    while o + 8 <= b.len() {
        let sz = u32::from_le_bytes([b[o + 4], b[o + 5], b[o + 6], b[o + 7]]) as usize;
        let end = (o + 8 + sz + (sz & 1)).min(b.len());
        if &b[o..o + 4] != b"smpl" {
            out.extend_from_slice(&b[o..end]);
        }
        o = end;
    }
    // smpl: manufacturer, product, period, root, fraction, SMPTE format / offset, 1 loop, no
    // extra data; the loop: id, forward, start, end (inclusive), fraction, play count
    let mut s = Vec::new();
    for v in [0u32, 0, 0, root as u32, 0, 0, 0, 1, 0, 0, 0, lp.0, lp.1.saturating_sub(1), 0, 0] {
        s.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(b"smpl");
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(&s);
    let total = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&total.to_le_bytes());
    std::fs::write(path, out)
}
