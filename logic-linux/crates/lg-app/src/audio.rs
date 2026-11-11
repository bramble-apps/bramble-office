//! The sound engine: plays the song's notes and audio regions from the playhead, in time with
//! the tempo. Instruments are a General MIDI SoundFont played natively (rustysynth, the
//! SoundFont the settings name, or the system's), or a small built-in synth when there's none.
//! The engine runs in rodio's mixer thread; the interface reads the playhead from it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lg_core::events::PPQ;

/// The output rate the engine renders at (rodio's mixer converts to the device's).
pub const RATE: u32 = 48_000;
/// A connection to the surround master (Out::Surround / Out::Object), in the render's bus slot.
const SURROUND: usize = usize::MAX;
const BLOCK: usize = 64;

/// One track as the engine plays it.
#[derive(Clone, Debug)]
pub struct PlayTrack {
    /// MIDI channel (9 = drums).
    pub channel: u8,
    pub program: u8,
    /// (start, end, pitch, velocity), sorted by start; song ticks.
    pub notes: Vec<(u32, u32, u8, u8)>,
    /// Audio regions: start tick and stereo samples at RATE.
    pub audio: Vec<(u32, Arc<Vec<f32>>)>,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// The channel strip's Channel EQ (None: none on the strip).
    pub eq: Option<crate::eq::Params>,
    /// The instrument plug-in (Logic's name, as the project or the Library gives it) played in
    /// place of the SoundFont when we have a stand-in for it; and the patch name.
    pub instrument: Option<String>,
    /// The instrument's saved settings (Logic's float block), and the Audio FX slots in order:
    /// Logic's plug-in name and its saved settings.
    pub instrument_floats: Vec<f32>,
    pub inserts: Vec<(String, Vec<f32>)>,
    /// The channel's sends: to which bus, at what gain, before the fader or after it.
    pub sends: Vec<Send>,
    /// Automation the channel follows while playing: volume (gain) and pan lines at song ticks.
    pub auto_volume: Vec<(u32, f32)>,
    pub auto_pan: Vec<(u32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Send {
    pub bus: usize,
    pub gain: f32,
    pub pre: bool,
}

/// An aux channel strip: what it takes in (a bus), its effects (and which are bypassed), its
/// fader, where its output goes and its own sends.
#[derive(Clone, Debug, PartialEq)]
pub struct AuxBus {
    pub bus: usize,
    pub volume: f32,
    pub pan: f32,
    pub mute: bool,
    pub inserts: Vec<(String, Vec<f32>)>,
    pub bypass: Vec<bool>,
    pub output: Out,
    pub sends: Vec<Send>,
}

/// Where a channel strip's output goes (Logic's Output slot).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Out {
    #[default]
    Stereo,
    Bus(usize),
    Off,
    /// Output ▸ Surround: the strip's Surround Panner into the surround master (a bed track
    /// with Dolby Atmos).
    Surround,
    /// The 3D Object Panner: an object track straight to the Dolby Atmos plug-in.
    Object,
}

/// Which strip a plug-in is on (the plug-in windows' handle into the engine).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FxStrip {
    Track(usize),
    Aux(usize),
    Master,
}

/// A track's own plug-ins as the engine runs them (built from PlayTrack::instrument).
#[derive(Default)]
pub struct Units {
    /// What `instrument` was built from (rebuilt when the track's choice changes).
    pub built_for: Option<String>,
    pub instrument: Option<Box<dyn lg_dsp::Instrument>>,
    pub inserts: Vec<Box<dyn lg_dsp::Effect>>,
    /// What `inserts` were built from.
    pub inserts_for: Vec<String>,
    /// Each built insert's place in the strip's insert list (names without a stand-in are
    /// skipped, so the two can differ).
    pub insert_index: Vec<usize>,
}

/// Builds a strip's effects from (Logic name, saved floats); returns them and their places.
fn build_inserts(list: &[(String, Vec<f32>)], tempo: f64) -> (Vec<Box<dyn lg_dsp::Effect>>, Vec<usize>) {
    let mut fx = Vec::new();
    let mut at = Vec::new();
    for (k, (n, floats)) in list.iter().enumerate() {
        if let Some(mut e) = crate::units::effect(n, RATE as f32) {
            if !floats.is_empty() {
                e.load_floats(floats);
            }
            e.set_tempo(tempo);
            fx.push(e);
            at.push(k);
        }
    }
    (fx, at)
}

/// Runs a strip's effects (the bypassed ones skipped); the largest gain reduction (dB).
fn run_inserts(u: &mut Units, bypass: &[bool], l: &mut [f32], r: &mut [f32], at: lg_dsp::Transport) -> f32 {
    let mut gr = 0f32;
    for (fx, k) in u.inserts.iter_mut().zip(u.insert_index.iter()) {
        if bypass.get(*k).copied().unwrap_or(false) {
            continue;
        }
        fx.set_transport(at);
        fx.process(l, r);
        gr = gr.max(fx.gain_reduction());
    }
    gr
}

fn strip_latency(u: &Units, bypass: &[bool]) -> usize {
    // (a hosted instrument's reported latency counts too: lg-host)
    u.instrument.as_ref().map_or(0, |i| i.latency()) + u.inserts.iter().zip(u.insert_index.iter()).filter(|(_, k)| !bypass.get(**k).copied().unwrap_or(false)).map(|(fx, _)| fx.latency()).sum::<usize>()
}

pub struct Engine {
    pub playing: bool,
    /// The playhead in ticks from bar 1.
    pub pos: f64,
    pub tempo: f64,
    /// Tempo changes (ticks, BPM); empty: `tempo` throughout.
    pub tempo_map: Vec<(u32, f64)>,
    pub tracks: Vec<PlayTrack>,
    pub cycle: Option<(u32, u32)>,
    pub cycle_on: bool,
    /// The metronome click (K): on every beat, higher on the bar's first (Logic's Klopfgeist).
    pub metronome: bool,
    /// Project Settings ▸ Metronome: what clicks and how it sounds.
    pub click_cfg: ClickCfg,
    /// Beats per bar, for the click's accent.
    pub beats_per_bar: u32,
    click: Option<(f32, f32)>,
    pub master: f32,
    /// Peak levels per track (for the meters), decaying.
    pub meters: Vec<f32>,
    pub master_meter: f32,
    /// The SoundFont, and one synthesizer per track playing it (so each track mixes on its own:
    /// volume, pan, mute, its meter).
    sf: Option<Arc<rustysynth::SoundFont>>,
    synths: Vec<Option<rustysynth::Synthesizer>>,
    /// Each track's running EQ (rebuilt when its settings change).
    eqs: Vec<Option<crate::eq::Eq>>,
    voices: Vec<Voice>,
    /// Each track's instrument and effects (our stand-ins for Logic's plug-ins).
    pub units: Vec<Units>,
    /// The aux strips (fed by the sends' buses), their effects as running, their meters.
    pub auxes: Vec<AuxBus>,
    aux_units: Vec<Units>,
    pub aux_meters: Vec<f32>,
    buses: Vec<(Vec<f32>, Vec<f32>)>,
    /// Routing set by the Mixer: each track's output and which of its effects are bypassed;
    /// the Stereo Out's own effects; gain reduction meters (dB); latency compensation on, and
    /// the plan (order, compensating delays) for the current routing.
    pub track_out: Vec<Out>,
    pub track_bypass: Vec<Vec<bool>>,
    pub master_inserts: Vec<(String, Vec<f32>)>,
    pub master_bypass: Vec<bool>,
    master_units: Units,
    pub gr: Vec<f32>,
    pub aux_gr: Vec<f32>,
    pub master_gr: f32,
    pub compensate: bool,
    /// Meters show the level before the fader (Mix ▸ Pre-Fader Metering).
    pub pre_fader_meters: bool,
    plan_for: Vec<lg_dsp::routing::Node>,
    plan: lg_dsp::routing::Plan,
    edge_delays: Vec<Vec<lg_dsp::routing::StereoDelay>>,
    sounding: Vec<(usize, u8, u8, u32)>,
    /// Project Settings ▸ MIDI ▸ Chase ▸ Notes: notes already under way at a play start or
    /// locate start sounding (syncmidi.rs sets it); and whether the next block must chase.
    pub chase_notes: bool,
    chase_due: bool,
    was_playing: bool,
    pub soundfont: Option<PathBuf>,
    pub error: Option<String>,
    /// Recording: the input, input monitoring, the count-in, the capture (recording.rs).
    pub rec: crate::recording::EngineRec,
    /// Plug-in parameter automation by track (automation.rs).
    pub param_auto: Vec<Vec<crate::automation::ParamLane>>,
    /// The tracks' Arpeggiators for keys played live (arp.rs).
    pub arp: crate::arp::EngineArps,
    /// The tracks' MIDI FX chains and the regions' controllers (midifx.rs).
    pub fx: crate::midifx::EngineFx,
    /// Tempo curves and signature changes (lg-core timeline): the tempo while playing, the
    /// click's bars and beats.
    pub timeline: lg_core::timeline::Map,
    /// Surround and Spatial Audio (surround.rs): None in a stereo project.
    pub surround: Option<Box<crate::surround::EngineSurround>>,
    /// Input formats, panners, solo-safe, frozen tracks, send modes, Dim, held peaks (mixengine.rs).
    pub ext: crate::mixengine::EngineExt,
}

#[derive(Clone, Copy)]
struct Voice {
    track: usize,
    pitch: u8,
    phase: f32,
    freq: f32,
    amp: f32,
    env: f32,
    releasing: bool,
    drum: bool,
}

impl Engine {
    pub fn new() -> Engine {
        Engine { playing: false, pos: 0.0, tempo: 120.0, tempo_map: Vec::new(), tracks: Vec::new(), cycle: None, cycle_on: false, metronome: false, click_cfg: ClickCfg::default(), beats_per_bar: 4, click: None, master: 1.0, meters: Vec::new(), master_meter: 0.0, sf: None, synths: Vec::new(), eqs: Vec::new(), voices: Vec::new(), units: Vec::new(), auxes: Vec::new(), aux_units: Vec::new(), aux_meters: Vec::new(), buses: Vec::new(), track_out: Vec::new(), track_bypass: Vec::new(), master_inserts: Vec::new(), master_bypass: Vec::new(), master_units: Units::default(), gr: Vec::new(), aux_gr: Vec::new(), master_gr: 0.0, compensate: true, pre_fader_meters: false, plan_for: Vec::new(), plan: Default::default(), edge_delays: Vec::new(), sounding: Vec::new(), soundfont: None, error: None, rec: Default::default(), param_auto: Vec::new(), arp: Default::default(), fx: Default::default(), timeline: Default::default(), chase_notes: true, chase_due: false, was_playing: false, surround: None, ext: Default::default() }
    }

    /// Loads a SoundFont for the instruments (None: the system's General MIDI one, if any).
    pub fn load_soundfont(&mut self, path: Option<&Path>) {
        let found = path.map(Path::to_path_buf).or_else(system_soundfont);
        self.sf = None;
        self.synths.clear();
        self.soundfont = None;
        let Some(p) = found else { return };
        match std::fs::File::open(&p).map_err(|e| e.to_string()).and_then(|mut f| rustysynth::SoundFont::new(&mut f).map_err(|e| format!("{e:?}"))) {
            Ok(sf) => {
                self.sf = Some(Arc::new(sf));
                self.soundfont = Some(p);
            }
            Err(e) => self.error = Some(format!("SoundFont {}: {e}", p.display())),
        }
        self.program_changes();
    }

    /// Each track's instrument plug-in: built when the track's choice changes.
    pub fn build_units(&mut self) {
        self.units.resize_with(self.tracks.len(), Units::default);
        self.units.truncate(self.tracks.len());
        for (i, t) in self.tracks.iter().enumerate() {
            let u = &mut self.units[i];
            if u.built_for != t.instrument {
                u.instrument = t.instrument.as_deref().and_then(|n| crate::units::instrument(n, RATE as f32));
                if let Some(ins) = u.instrument.as_mut() {
                    if !t.instrument_floats.is_empty() {
                        ins.load_floats(&t.instrument_floats);
                    }
                    ins.set_tempo(self.tempo);
                }
                u.built_for = t.instrument.clone();
            }
            let names: Vec<String> = t.inserts.iter().map(|x| x.0.clone()).collect();
            if u.inserts_for != names {
                (u.inserts, u.insert_index) = build_inserts(&t.inserts, self.tempo);
                u.inserts_for = names;
            }
        }
    }

    /// A note played live (Musical Typing, a MIDI keyboard) on a track's sound; velocity 0 stops it.
    pub fn live(&mut self, track: usize, pitch: u8, velocity: u8) {
        if track >= self.tracks.len() {
            return;
        }
        // (through the track's MIDI FX when it has some)
        if self.fx.active(track) {
            self.fx.push_live(track, if velocity > 0 { lg_dsp::midifx::Msg::On { ch: 0, pitch, vel: velocity } } else { lg_dsp::midifx::Msg::Off { ch: 0, pitch } });
            return;
        }
        // (through the track's Arpeggiator when it's on)
        if self.arp.active(track) {
            if let Some(a) = self.arp.tracks.get_mut(&track) {
                a.key(pitch, velocity);
            }
            return;
        }
        self.live_note(track, pitch, velocity);
    }

    /// A note to the track's instrument (live keys, the Arpeggiator's notes).
    pub fn live_note(&mut self, track: usize, pitch: u8, velocity: u8) {
        if track >= self.tracks.len() {
            return;
        }
        let ch = self.tracks[track].channel;
        if let Some(ins) = self.units.get_mut(track).and_then(|u| u.instrument.as_mut()) {
            if velocity > 0 { ins.note_on(pitch, velocity) } else { ins.note_off(pitch) }
        } else if let Some(Some(s)) = self.synths.get_mut(track) {
            if velocity > 0 { s.note_on(ch as i32, pitch as i32, velocity as i32) } else { s.note_off(ch as i32, pitch as i32) }
        } else if velocity > 0 {
            self.voices.push(Voice { track, pitch, phase: 0.0, freq: lg_dsp::midi_hz(pitch as f32), amp: velocity as f32 / 127.0 * 0.25, env: 0.0, releasing: false, drum: ch == 9 });
        } else {
            for v in self.voices.iter_mut().filter(|v| v.track == track && v.pitch == pitch) {
                v.releasing = true;
            }
        }
    }

    /// A controller played live (sustain pedal, mod wheel, pitch bend as cc 128).
    pub fn live_control(&mut self, track: usize, cc: u8, value: u16) {
        if self.fx.active(track) {
            let m = if cc == 128 { lg_dsp::midifx::Msg::Bend { ch: 0, val: (value as i32 - 8192) as i16 } } else { lg_dsp::midifx::Msg::Cc { ch: 0, num: cc, val: value.min(127) as u8 } };
            self.fx.push_live(track, m);
            return;
        }
        self.control_out(track, cc, value);
    }

    /// A controller to the track's instrument (after the MIDI FX).
    pub fn control_out(&mut self, track: usize, cc: u8, value: u16) {
        let ch = self.tracks.get(track).map_or(0, |t| t.channel);
        if let Some(ins) = self.units.get_mut(track).and_then(|u| u.instrument.as_mut()) {
            if cc == 128 { ins.pitch_bend(value as i16 - 8192) } else { ins.controller(cc, value as u8) }
        } else if let Some(Some(s)) = self.synths.get_mut(track) {
            if cc == 128 {
                s.process_midi_message(ch as i32, 0xE0, (value & 0x7F) as i32, (value >> 7) as i32);
            } else {
                s.process_midi_message(ch as i32, 0xB0, cc as i32, value as i32);
            }
        }
    }

    /// Another channel message to the track's sound (pressure, poly pressure, program change);
    /// Studio's own instruments don't take these.
    pub fn live_message(&mut self, track: usize, status: u8, d1: u8, d2: u8) {
        let ch = self.tracks.get(track).map_or(0, |t| t.channel);
        if self.units.get(track).is_some_and(|u| u.instrument.is_some()) {
            return;
        }
        if let Some(Some(s)) = self.synths.get_mut(track) {
            s.process_midi_message(ch as i32, status as i32 & 0xF0, d1 as i32, d2 as i32);
        }
    }

    /// The aux strips' effects: built when their lists change.
    fn build_aux_units(&mut self) {
        self.aux_units.resize_with(self.auxes.len(), Units::default);
        self.aux_units.truncate(self.auxes.len());
        for (a, u) in self.auxes.iter().zip(self.aux_units.iter_mut()) {
            let names: Vec<String> = a.inserts.iter().map(|x| x.0.clone()).collect();
            if u.inserts_for != names {
                (u.inserts, u.insert_index) = build_inserts(&a.inserts, self.tempo);
                u.inserts_for = names;
            }
        }
        let names: Vec<String> = self.master_inserts.iter().map(|x| x.0.clone()).collect();
        if self.master_units.inserts_for != names {
            (self.master_units.inserts, self.master_units.insert_index) = build_inserts(&self.master_inserts, self.tempo);
            self.master_units.inserts_for = names;
        }
    }

    /// Every strip's running plug-ins (hosting.rs: the hosted ones' editors, state, latency).
    pub(crate) fn all_units_mut(&mut self) -> Vec<(FxStrip, &mut Units)> {
        let mut v: Vec<(FxStrip, &mut Units)> = self.units.iter_mut().enumerate().map(|(i, u)| (FxStrip::Track(i), u)).collect();
        v.extend(self.aux_units.iter_mut().enumerate().map(|(i, u)| (FxStrip::Aux(i), u)));
        v.push((FxStrip::Master, &mut self.master_units));
        v
    }

    fn units_of(&self, s: FxStrip) -> Option<&Units> {
        match s {
            FxStrip::Track(t) => self.units.get(t),
            FxStrip::Aux(a) => self.aux_units.get(a),
            FxStrip::Master => Some(self.surround.as_ref().map_or(&self.master_units, |s| &s.units)),
        }
    }

    fn units_of_mut(&mut self, s: FxStrip) -> Option<&mut Units> {
        match s {
            FxStrip::Track(t) => self.units.get_mut(t),
            FxStrip::Aux(a) => self.aux_units.get_mut(a),
            FxStrip::Master => Some(match self.surround.as_mut() {
                Some(s) => &mut s.units,
                None => &mut self.master_units,
            }),
        }
    }

    /// The running effect in insert `slot` of a strip: its parameters and their values now.
    pub fn effect_params(&self, s: FxStrip, slot: usize) -> Option<(Vec<lg_dsp::Param>, Vec<f32>)> {
        let u = self.units_of(s)?;
        let k = u.insert_index.iter().position(|x| *x == slot)?;
        let fx = &u.inserts[k];
        let params = fx.params().to_vec();
        let values = params.iter().map(|p| fx.get(p.id)).collect();
        Some((params, values))
    }

    /// The running effect's live readouts and one of its curves (for its window).
    pub fn effect_readouts(&self, s: FxStrip, slot: usize, curve: &str) -> (Vec<(&'static str, f32)>, Vec<f32>) {
        let Some(u) = self.units_of(s) else { return (Vec::new(), Vec::new()) };
        let Some(k) = u.insert_index.iter().position(|x| *x == slot) else { return (Vec::new(), Vec::new()) };
        (u.inserts[k].readouts(), if curve.is_empty() { Vec::new() } else { u.inserts[k].curve(curve) })
    }

    pub fn set_effect_param(&mut self, s: FxStrip, slot: usize, id: u32, v: f32) {
        if let Some(u) = self.units_of_mut(s) {
            if let Some(k) = u.insert_index.iter().position(|x| *x == slot) {
                u.inserts[k].set(id, v);
            }
        }
    }

    /// Re-reads an effect's saved floats (Paste, Compare, a setting loaded) without rebuilding.
    pub fn load_effect(&mut self, s: FxStrip, slot: usize, floats: &[f32]) {
        if let Some(u) = self.units_of_mut(s) {
            if let Some(k) = u.insert_index.iter().position(|x| *x == slot) {
                u.inserts[k].load_floats(floats);
            }
        }
    }

    /// The whole mix's plug-in delay (samples) with compensation on.
    pub fn latency(&self) -> usize {
        self.plan.total
    }

    /// The routing as nodes for the planner: tracks first, then the auxes.
    fn route_nodes(&self) -> Vec<lg_dsp::routing::Node> {
        use lg_dsp::routing::{Dest, Node};
        let dest = |o: Out| match o {
            Out::Stereo | Out::Surround | Out::Object => Some(Dest::Master),
            Out::Bus(b) => Some(Dest::Bus(b)),
            Out::Off => None,
        };
        let mut nodes = Vec::with_capacity(self.tracks.len() + self.auxes.len());
        for (i, t) in self.tracks.iter().enumerate() {
            let mut outs: Vec<Dest> = dest(self.track_out.get(i).copied().unwrap_or_default()).into_iter().collect();
            outs.extend(t.sends.iter().map(|s| Dest::Bus(s.bus)));
            let latency = self.units.get(i).map_or(0, |u| strip_latency(u, self.track_bypass.get(i).map_or(&[], |v| v.as_slice())));
            nodes.push(Node { latency, input: None, outs });
        }
        for (a, x) in self.auxes.iter().enumerate() {
            let mut outs: Vec<Dest> = dest(x.output).into_iter().collect();
            outs.extend(x.sends.iter().map(|s| Dest::Bus(s.bus)));
            let latency = self.aux_units.get(a).map_or(0, |u| strip_latency(u, &x.bypass));
            nodes.push(Node { latency, input: Some(x.bus), outs });
        }
        nodes
    }

    /// Plans the routing again when it changed (new delays start silent).
    fn replan(&mut self) {
        let nodes = self.route_nodes();
        if nodes == self.plan_for && self.plan.delay.len() == nodes.len() {
            return;
        }
        self.plan = lg_dsp::routing::plan(&nodes, self.compensate);
        self.edge_delays = self.plan.delay.iter().map(|v| v.iter().map(|d| lg_dsp::routing::StereoDelay::new(d.unwrap_or(0))).collect()).collect();
        self.plan_for = nodes;
    }

    /// The track's instrument parameters and values (Smart Controls, the plug-in window).
    pub fn instrument_params(&self, t: usize) -> Option<(Vec<lg_dsp::Param>, Vec<f32>)> {
        let ins = self.units.get(t)?.instrument.as_ref()?;
        let params = ins.params().to_vec();
        let values = params.iter().map(|p| ins.get(p.id)).collect();
        Some((params, values))
    }

    pub fn set_instrument_param(&mut self, t: usize, id: u32, v: f32) {
        if let Some(ins) = self.units.get_mut(t).and_then(|u| u.instrument.as_mut()) {
            ins.set(id, v);
        }
    }

    /// Each track's synthesizer, with the track's sound on its channel.
    pub fn program_changes(&mut self) {
        self.build_units();
        self.build_aux_units();
        let Some(sf) = self.sf.clone() else {
            self.synths.clear();
            return;
        };
        self.synths.resize_with(self.tracks.len(), || None);
        for (i, t) in self.tracks.iter().enumerate() {
            if self.synths[i].is_none() {
                let settings = rustysynth::SynthesizerSettings::new(RATE as i32);
                self.synths[i] = rustysynth::Synthesizer::new(&sf, &settings).ok();
            }
            if let Some(s) = &mut self.synths[i] {
                s.process_midi_message(t.channel as i32, 0xC0, t.program as i32, 0);
            }
        }
    }

    pub fn has_soundfont(&self) -> bool {
        self.sf.is_some()
    }

    /// Stops every sounding note (stop, jump, cycle).
    pub fn all_off(&mut self) {
        for s in self.synths.iter_mut().flatten() {
            s.note_off_all(false);
        }
        for u in &mut self.units {
            if let Some(ins) = u.instrument.as_mut() {
                for p in 0..128u8 {
                    ins.note_off(p);
                }
                ins.controller(64, 0);
            }
        }
        self.sounding.clear();
        self.fx.reset_all();
        for v in &mut self.voices {
            v.releasing = true;
        }
    }

    pub fn locate(&mut self, ticks: f64) {
        self.all_off();
        self.pos = ticks.max(0.0);
        self.chase_due = true;
    }

    fn audible(&self, i: usize) -> bool {
        crate::mixengine::audible(self, i)
    }

    /// Renders one block of stereo frames into `out` (interleaved).
    pub(crate) fn render(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        out.fill(0.0);
        if self.meters.len() != self.tracks.len() {
            self.meters = vec![0.0; self.tracks.len()];
        }
        let bpm = if self.timeline.points.is_empty() { self.tempo_map.iter().rev().find(|t| t.0 as f64 <= self.pos).map_or(self.tempo, |t| t.1) } else { self.timeline.bpm_at(self.pos) };
        let tps = bpm / 60.0 * PPQ as f64 / RATE as f64;
        // the Arpeggiators' steps for keys held live
        if !self.arp.tracks.is_empty() {
            let song = self.playing.then(|| (self.pos, self.pos + tps * frames as f64, PPQ));
            for (t, p, v) in self.arp.tick(frames, RATE as f64, bpm, song) {
                self.live_note(t, p, v);
            }
        }
        // recording: the input for this block; a count-in before bar 1 holds the playhead
        let input = self.rec.input(frames);
        let held = self.playing && self.rec.hold(frames, tps, self.beats_per_bar, &mut self.click);
        let was = self.was_playing;
        self.was_playing = self.playing && !held;
        if !self.playing {
            self.chase_due = false;
        }

        // the MIDI FX and the regions' controllers for this block
        if !self.fx.chains.is_empty() || self.fx.controls.iter().any(|c| !c.is_empty()) {
            let audible: Vec<bool> = (0..self.tracks.len()).map(|i| self.audible(i)).collect();
            let span = (self.playing && !held).then(|| (self.pos, self.pos + tps * frames as f64));
            crate::midifx::render(self, span, tps * frames as f64, bpm, &audible);
        }
        if self.playing && !held {
            let from = self.pos;
            let to = from + tps * frames as f64;
            // notes ending in this block
            let mut ended = Vec::new();
            self.sounding.retain(|&(t, ch, pitch, end)| {
                if (end as f64) < to {
                    ended.push((t, ch, pitch));
                    false
                } else {
                    true
                }
            });
            for (t, ch, pitch) in ended {
                if let Some(ins) = self.units.get_mut(t).and_then(|u| u.instrument.as_mut()) {
                    ins.note_off(pitch);
                } else if let Some(Some(s)) = self.synths.get_mut(t) {
                    s.note_off(ch as i32, pitch as i32);
                }
                for v in self.voices.iter_mut().filter(|v| v.track == t && v.pitch == pitch) {
                    v.releasing = true;
                }
            }
            // chase: notes begun before a play start or locate sound from here (Chase ▸ Notes)
            if (self.chase_due || !was) && self.chase_notes {
                for i in 0..self.tracks.len() {
                    if !self.audible(i) {
                        continue;
                    }
                    let ch = self.tracks[i].channel;
                    let under: Vec<(u32, u32, u8, u8)> = self.tracks[i].notes.iter().filter(|n| (n.0 as f64) < from && (n.1 as f64) > from + 1.0).copied().collect();
                    for (_, e, pitch, vel) in under {
                        if let Some(ins) = self.units.get_mut(i).and_then(|u| u.instrument.as_mut()) {
                            ins.note_on(pitch, vel);
                        } else if let Some(Some(sy)) = self.synths.get_mut(i) {
                            sy.note_on(ch as i32, pitch as i32, vel as i32);
                        }
                        self.sounding.push((i, ch, pitch, e));
                    }
                }
            }
            self.chase_due = false;
            // notes starting in this block
            for i in 0..self.tracks.len() {
                if !self.audible(i) || self.fx.active(i) {
                    continue;
                }
                let t = &self.tracks[i];
                let (ch, drum) = (t.channel, t.channel == 9);
                let starts: Vec<(u32, u32, u8, u8)> = t.notes.iter().filter(|n| (n.0 as f64) >= from && (n.0 as f64) < to).copied().collect();
                for (s, e, pitch, vel) in starts {
                    let _ = s;
                    if let Some(ins) = self.units.get_mut(i).and_then(|u| u.instrument.as_mut()) {
                        ins.note_on(pitch, vel);
                    } else if let Some(Some(sy)) = self.synths.get_mut(i) {
                        sy.note_on(ch as i32, pitch as i32, vel as i32);
                    } else {
                        self.voices.push(Voice { track: i, pitch, phase: 0.0, freq: lg_dsp::midi_hz(pitch as f32), amp: vel as f32 / 127.0 * 0.25, env: 0.0, releasing: false, drum });
                    }
                    self.sounding.push((i, ch, pitch, e));
                }
            }
            // the click: a bar, beat or division line crossed in this block (Project Settings ▸
            // Metronome: Simple mode clicks as the Metronome button says everywhere; else Click
            // while playing is the button, Click while recording / Only during count-in rule
            // recording)
            let cc = self.click_cfg;
            let counting = self.rec.click_forced(from);
            let want = if cc.simple {
                self.metronome || counting
            } else if counting {
                cc.while_recording
            } else if self.rec.capturing {
                cc.while_recording && !cc.only_count_in
            } else {
                self.metronome
            };
            if want {
                let div = PPQ as f64 / 4.0;
                let next = (from / div).ceil() * div;
                if next < to {
                    let n = (next / div).round() as u32;
                    // (signature changes: beats and bars from the signature map)
                    let (on_beat, bar) = if self.timeline.points.is_empty() {
                        let on_beat = n % 4 == 0;
                        (on_beat, on_beat && (n / 4) % self.beats_per_bar.max(1) == 0)
                    } else {
                        let (_, start, sig) = self.timeline.bar_of(next as u32);
                        let beat = lg_core::timeline::Map::beat_len(sig);
                        ((next as u32 - start) % beat == 0, start == next as u32)
                    };
                    let f = if bar && cc.bar {
                        Some(1760.0)
                    } else if on_beat && cc.beat {
                        Some(1320.0)
                    } else if !on_beat && cc.division {
                        Some(990.0)
                    } else {
                        None
                    };
                    if let Some(f) = f {
                        self.click = Some((f, 0.0));
                    }
                }
            }
            self.pos = to;
            self.rec.capture(from, tps, input.as_deref());
            if let (true, Some((a, b))) = (self.cycle_on, self.cycle) {
                if b > a && self.pos >= b as f64 {
                    self.all_off();
                    self.pos = a as f64;
                    self.rec.wrapped();
                }
            }
        }
        // each strip in the routing's order (lg_dsp::routing): a track makes its sound (its
        // synthesizer and audio regions) and runs its EQ, an aux takes its bus; then the effects
        // (bypassed ones skipped), the fader and pan (equal power); then each connection (the
        // output, the sends) through its compensating delay into its bus or the Stereo Out
        let n_tracks = self.tracks.len();
        self.eqs.resize_with(n_tracks, || None);
        self.replan();
        let start_tick = if self.playing && !held { self.pos - tps * frames as f64 } else { self.pos };
        let at = lg_dsp::Transport { beats: start_tick / PPQ as f64, bpm, playing: self.playing && !held };
        let n_aux = self.auxes.len();
        self.aux_meters.resize(n_aux, 0.0);
        self.gr.resize(n_tracks, 0.0);
        self.aux_gr.resize(n_aux, 0.0);
        let top_bus = self.plan_for.iter().flat_map(|n| n.input.into_iter().chain(n.outs.iter().filter_map(|d| match d {
            lg_dsp::routing::Dest::Bus(b) => Some(*b),
            lg_dsp::routing::Dest::Master => None,
        }))).max();
        if let Some(b) = top_bus {
            if self.buses.len() <= b {
                self.buses.resize_with(b + 1, || (Vec::new(), Vec::new()));
            }
        }
        for (bl, br) in &mut self.buses {
            bl.clear();
            bl.resize(frames, 0.0);
            br.clear();
            br.resize(frames, 0.0);
        }
        if let Some(s) = self.surround.as_mut() {
            s.begin(frames, start_tick, tps, self.playing && !held, &self.tracks);
        }
        let mut l = vec![0f32; frames];
        let mut r = vec![0f32; frames];
        let mut el = vec![0f32; frames];
        let mut er = vec![0f32; frames];
        let order = self.plan.order.clone();
        for node in order {
            l.fill(0.0);
            r.fill(0.0);
            let (gl, gr, vol, sends, output) = if node < n_tracks {
                let i = node;
                if self.playing {
                    if let (Some(lanes), Some(u)) = (self.param_auto.get(i).filter(|l| !l.is_empty()), self.units.get_mut(i)) {
                        crate::automation::apply_params(lanes, u, start_tick);
                    }
                }
                let mut any = false;
                if let Some(ins) = self.units.get_mut(i).and_then(|u| u.instrument.as_mut()) {
                    ins.render(&mut l, &mut r);
                    any = true;
                } else if let Some(Some(sy)) = self.synths.get_mut(i) {
                    sy.render(&mut l, &mut r);
                    any = true;
                }
                if self.playing {
                    for (at, samples) in &self.tracks[i].audio {
                        for k in 0..frames {
                            let tick = start_tick + k as f64 * tps - *at as f64;
                            if tick < 0.0 {
                                continue;
                            }
                            let frame = (tick / tps) as usize;
                            if 2 * frame + 1 >= samples.len() {
                                break;
                            }
                            l[k] += samples[2 * frame];
                            r[k] += samples[2 * frame + 1];
                            any = true;
                        }
                    }
                }
                if self.rec.monitor(i, input.as_deref(), &mut l, &mut r) {
                    any = true;
                }
                // (a frozen track's freeze file; the strip's input format, phase and gain)
                if self.ext.source(i, self.playing && !held, start_tick, tps, &mut l, &mut r) {
                    any = true;
                }
                if !any || !self.audible(i) {
                    continue;
                }
                match (&self.tracks[i].eq, &mut self.eqs[i]) {
                    (Some(p), Some(e)) => {
                        if e.params != *p {
                            e.set(*p);
                        }
                        e.process(&mut l, &mut r);
                    }
                    (Some(p), slot) => {
                        let mut e = crate::eq::Eq::new(*p);
                        e.process(&mut l, &mut r);
                        *slot = Some(e);
                    }
                    (None, slot) => *slot = None,
                }
                let bypass = self.track_bypass.get(i).cloned().unwrap_or_default();
                let lat: Vec<(usize, usize)> = self.units.get(i).map(|u| u.inserts.iter().zip(u.insert_index.iter()).map(|(fx, k)| (*k, fx.latency())).collect()).unwrap_or_default();
                let bypass = self.ext.bypass(i, bypass, &lat);
                if let Some(u) = self.units.get_mut(i) {
                    let g = run_inserts(u, &bypass, &mut l, &mut r, at);
                    self.gr[i] = self.gr[i].max(g);
                }
                let (mut vol, mut pan) = (self.tracks[i].volume, self.tracks[i].pan.clamp(-1.0, 1.0));
                if self.playing {
                    if let Some(v) = crate::automation::curve_at(&self.tracks[i].auto_volume, start_tick) {
                        vol = v;
                    }
                    if let Some(v) = crate::automation::curve_at(&self.tracks[i].auto_pan, start_tick) {
                        pan = v.clamp(-1.0, 1.0);
                    }
                }
                // (to surround: the Surround Panner pans, the fader only sets the level)
                let surround = matches!(self.track_out.get(i), Some(Out::Surround | Out::Object));
                if surround {
                    pan = 0.0;
                }
                let (gl, gr) = self.ext.pan(i, n_tracks, vol, pan, surround, &mut l, &mut r);
                (gl, gr, vol, self.tracks[i].sends.clone(), self.track_out.get(i).copied().unwrap_or_default())
            } else {
                let ai = node - n_tracks;
                let aux = self.auxes[ai].clone();
                if let Some((bl, br)) = self.buses.get(aux.bus) {
                    l.copy_from_slice(bl);
                    r.copy_from_slice(br);
                }
                if let Some(u) = self.aux_units.get_mut(ai) {
                    let g = run_inserts(u, &aux.bypass, &mut l, &mut r, at);
                    self.aux_gr[ai] = self.aux_gr[ai].max(g);
                }
                if aux.mute {
                    continue;
                }
                let surround = matches!(aux.output, Out::Surround | Out::Object);
                let apan = if surround { 0.0 } else { aux.pan.clamp(-1.0, 1.0) };
                if let Some(x) = self.ext.auxes.get(ai) {
                    x.format.apply(&mut l, &mut r);
                }
                let (gl, gr) = self.ext.pan(node, n_tracks, aux.volume, apan, surround, &mut l, &mut r);
                (gl, gr, aux.volume, aux.sends, aux.output)
            };
            // the connections, in route_nodes' order: the output (unless off), then the sends
            // (a send: Post Fader before the panner, Post Pan after it, Independent Pan with its
            // own; a bypassed one keeps its place in the plan and stays silent)
            let mut conns: Vec<(Option<usize>, f32, f32, bool)> = Vec::new();
            match output {
                Out::Stereo => conns.push((None, gl, gr, false)),
                Out::Bus(b) => conns.push((Some(b), gl, gr, false)),
                Out::Off => {}
                Out::Surround | Out::Object => conns.push((Some(SURROUND), gl, gr, false)),
            }
            let n_out = conns.len();
            for (sk, snd) in sends.iter().enumerate() {
                let c = self.ext.send(node, n_tracks, sk, snd, vol, gl, gr).unwrap_or(crate::mixengine::Conn { gl: 0.0, gr: 0.0, dry: false });
                conns.push((Some(snd.bus), c.gl, c.gr, c.dry));
            }
            let mut peak = 0f32;
            for (k, (dest, cl, cr, dry)) in conns.into_iter().enumerate() {
                if self.plan.delay.get(node).and_then(|v| v.get(k)).copied().flatten().is_none() {
                    continue;
                }
                {
                    let (sl, sr) = self.ext.tap(dry, &l, &r);
                    for f in 0..frames {
                        el[f] = sl[f] * cl;
                        er[f] = sr[f] * cr;
                    }
                }
                if k < n_out {
                    let (ml, mr) = if self.pre_fader_meters { (&l, &r) } else { (&el, &er) };
                    for f in 0..frames {
                        peak = peak.max(ml[f].abs()).max(mr[f].abs());
                    }
                }
                if let Some(d) = self.edge_delays.get_mut(node).and_then(|v| v.get_mut(k)) {
                    d.process(&mut el, &mut er);
                }
                match dest {
                    Some(SURROUND) => {
                        if let Some(s) = self.surround.as_mut() {
                            s.add(node, &el, &er);
                        }
                    }
                    None => {
                        for f in 0..frames {
                            out[2 * f] += el[f];
                            out[2 * f + 1] += er[f];
                        }
                    }
                    Some(b) => {
                        if let Some((bl, br)) = self.buses.get_mut(b) {
                            for f in 0..frames {
                                bl[f] += el[f];
                                br[f] += er[f];
                            }
                        }
                    }
                }
            }
            let m = if node < n_tracks { self.meters.get_mut(node) } else { self.aux_meters.get_mut(node - n_tracks) };
            if let Some(m) = m {
                *m = m.max(peak);
            }
            self.ext.meter(node, n_tracks, peak);
        }
        // the Stereo Out's own effects (Logic's master chain; deactivated in surround)
        if !self.master_units.inserts.is_empty() && self.surround.is_none() {
            for f in 0..frames {
                l[f] = out[2 * f];
                r[f] = out[2 * f + 1];
            }
            let g = run_inserts(&mut self.master_units, &self.master_bypass, &mut l, &mut r, at);
            self.master_gr = self.master_gr.max(g);
            for f in 0..frames {
                out[2 * f] = l[f];
                out[2 * f + 1] = r[f];
            }
        }
        // surround: the surround master (and the Dolby Atmos renderer), folded for the device
        if let Some(s) = self.surround.as_mut() {
            s.finish(out, frames, at, self.master);
        }
        if self.sf.is_none() {
            for v in &mut self.voices {
                for k in 0..frames {
                    v.env = if v.releasing { v.env * 0.9992 } else { (v.env + 0.01).min(1.0) };
                    let x = if v.drum {
                        // a short noisy click per drum note
                        let n = ((v.phase * 12.9898).sin() * 43758.547).fract() * 2.0 - 1.0;
                        v.env *= 0.9985;
                        n
                    } else {
                        // a soft saw
                        let s = 2.0 * v.phase - 1.0;
                        s * 0.6 + (v.phase * std::f32::consts::TAU).sin() * 0.4
                    };
                    v.phase = (v.phase + v.freq / RATE as f32).fract();
                    let y = x * v.amp * v.env;
                    out[2 * k] += y;
                    out[2 * k + 1] += y;
                }
            }
            self.voices.retain(|v| v.env > 0.0005 || !v.releasing);
        }
        // the metronome's click: a short decaying sine
        if let Some((freq, ref mut t)) = self.click {
            // (Tone: a short click → a pitched, longer woodblock; Volume: the level)
            let decay = 90.0 - 60.0 * self.click_cfg.tone;
            let gain = 0.35 * lg_dsp::db_to_gain(self.click_cfg.volume_db);
            for k in 0..frames {
                let env = (-*t * decay).exp();
                let y = (std::f32::consts::TAU * freq * *t).sin() * env * gain;
                out[2 * k] += y;
                out[2 * k + 1] += y;
                *t += 1.0 / RATE as f32;
            }
            if *t > 0.08 {
                self.click = None;
            }
        }
        let mut peak = 0f32;
        let mut over = 0f32;
        let mg = self.master * self.ext.master_gain();
        for x in out.iter_mut() {
            over = over.max((*x * mg).abs());
            *x = (*x * mg).clamp(-1.0, 1.0);
            peak = peak.max(x.abs());
        }
        self.master_meter = self.master_meter.max(peak);
        self.ext.master_peak = self.ext.master_peak.max(over);
        let fall = 0.985f32.powf(frames as f32 / 64.0);
        for m in self.meters.iter_mut().chain(self.aux_meters.iter_mut()).chain(self.gr.iter_mut()).chain(self.aux_gr.iter_mut()) {
            *m *= fall;
        }
        self.master_gr *= fall;
        self.master_meter *= fall;
    }
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}

/// Bounce (File > Bounce > Project or Section…): renders `tracks` offline from `from` to `to`
/// (ticks) plus a two-second tail, with the given SoundFont; stereo samples at RATE.
pub fn bounce(tracks: Vec<PlayTrack>, tempo: f64, tempo_map: Vec<(u32, f64)>, soundfont: Option<&Path>, from: u32, to: u32) -> Vec<f32> {
    let mut e = Engine::new();
    e.tempo = tempo;
    e.tempo_map = tempo_map;
    e.tracks = tracks;
    e.load_soundfont(soundfont);
    e.pos = from as f64;
    e.playing = true;
    let mut out = Vec::new();
    let mut buf = vec![0f32; 2 * BLOCK];
    let tps = tempo / 60.0 * PPQ as f64 / RATE as f64;
    let tail = (2.0 * RATE as f64) as usize / BLOCK;
    while e.pos < to as f64 {
        e.render(&mut buf);
        out.extend_from_slice(&buf);
    }
    e.playing = false;
    e.all_off();
    let _ = tps;
    for _ in 0..tail {
        e.render(&mut buf);
        out.extend_from_slice(&buf);
    }
    out
}

/// Renders a prepared engine offline from `from` to `to` (ticks), then its tail: `tail_secs`
/// more at most, ending early once the sound has faded when `until_silent` (Bounce in Place,
/// freeze files, exporting tracks and regions). Interleaved stereo at RATE.
pub fn render_offline(mut e: Engine, from: u32, to: u32, tail_secs: f32, until_silent: bool) -> Vec<f32> {
    e.pos = from as f64;
    e.playing = true;
    let mut out = Vec::new();
    let mut buf = vec![0f32; 2 * BLOCK];
    while e.pos < to as f64 {
        e.render(&mut buf);
        out.extend_from_slice(&buf);
    }
    e.playing = false;
    e.all_off();
    let blocks = (tail_secs.max(0.0) * RATE as f32) as usize / BLOCK;
    let mut quiet = 0;
    for _ in 0..blocks {
        e.render(&mut buf);
        out.extend_from_slice(&buf);
        quiet = if buf.iter().all(|x| x.abs() < 1e-4) { quiet + 1 } else { 0 };
        // (a quarter second of silence: the tail is over)
        if until_silent && quiet * BLOCK > RATE as usize / 4 {
            break;
        }
    }
    out
}

/// A 16-bit stereo WAV file of `samples` at RATE.
pub fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for x in samples {
        b.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    b
}

/// The system's General MIDI SoundFont (Linux distributions install one with FluidSynth / TiMidity).
pub fn system_soundfont() -> Option<PathBuf> {
    let candidates = ["/usr/share/sounds/sf2/default-GM.sf2", "/usr/share/sounds/sf2/FluidR3_GM.sf2", "/usr/share/soundfonts/default.sf2", "/usr/share/soundfonts/FluidR3_GM.sf2", "/usr/share/sounds/sf2/TimGM6mb.sf2", "/usr/share/sounds/sf3/default-GM.sf3"];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

/// rodio's side: pulls blocks from the engine.
struct Stream {
    engine: Arc<Mutex<Engine>>,
    buf: Vec<f32>,
    at: usize,
}

impl Iterator for Stream {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.at >= self.buf.len() {
            let mut e = self.engine.lock().unwrap_or_else(|e| e.into_inner());
            e.render(&mut self.buf);
            self.at = 0;
        }
        let x = self.buf[self.at];
        self.at += 1;
        Some(x)
    }
}

impl rodio::Source for Stream {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(2).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(RATE).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// Project Settings ▸ Metronome, as the engine plays it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickCfg {
    pub simple: bool,
    pub while_recording: bool,
    pub only_count_in: bool,
    pub bar: bool,
    pub beat: bool,
    pub division: bool,
    /// 0 (short click) … 1 (woodblock).
    pub tone: f32,
    pub volume_db: f32,
}

impl Default for ClickCfg {
    fn default() -> ClickCfg {
        ClickCfg { simple: true, while_recording: true, only_count_in: false, bar: true, beat: true, division: false, tone: 0.5, volume_db: 0.0 }
    }
}

/// The audio output: opened once; the engine plays through it for the app's life.
pub struct Output {
    pub engine: Arc<Mutex<Engine>>,
    _sink: Option<rodio::MixerDeviceSink>,
    pub error: Option<String>,
}

impl Output {
    pub fn open(engine: Arc<Mutex<Engine>>) -> Output {
        // (tests and snapshots: no sound device)
        if std::env::var_os("LG_SNAP").is_some() || std::env::var_os("LG_NOAUDIO").is_some() {
            return Output { engine, _sink: None, error: None };
        }
        Output::open_on(engine, None, None)
    }

    /// No output (Settings ▸ Audio ▸ Devices ▸ Enabled off, or while switching devices).
    pub fn silent(engine: Arc<Mutex<Engine>>) -> Output {
        Output { engine, _sink: None, error: None }
    }

    /// The output on a device by name (None: the system's default) with an I/O buffer size in
    /// frames (None: the device's own): Settings ▸ Audio ▸ Devices.
    pub fn open_on(engine: Arc<Mutex<Engine>>, device: Option<&str>, buffer: Option<u32>) -> Output {
        if std::env::var_os("LG_SNAP").is_some() || std::env::var_os("LG_NOAUDIO").is_some() {
            return Output { engine, _sink: None, error: None };
        }
        use rodio::cpal::traits::HostTrait;
        let host = rodio::cpal::default_host();
        let dev = device.and_then(|n| host.output_devices().ok()?.find(|d| rodio::cpal::traits::DeviceTrait::description(d).is_ok_and(|x| x.name() == n)));
        let opened = if dev.is_none() && buffer.is_none() {
            rodio::DeviceSinkBuilder::open_default_sink()
        } else {
            let builder = match dev {
                Some(d) => rodio::DeviceSinkBuilder::from_device(d),
                None => rodio::DeviceSinkBuilder::from_default_device(),
            };
            builder.and_then(|b| match buffer {
                Some(n) => b.with_buffer_size(rodio::cpal::BufferSize::Fixed(n)).open_stream(),
                None => b.open_stream(),
            }).or_else(|_| rodio::DeviceSinkBuilder::open_default_sink())
        };
        match opened {
            Ok(mut s) => {
                s.log_on_drop(false);
                s.mixer().add(Stream { engine: engine.clone(), buf: vec![0.0; BLOCK * 2], at: BLOCK * 2 });
                Output { engine, _sink: Some(s), error: None }
            }
            Err(e) => Output { engine, _sink: None, error: Some(e.to_string()) },
        }
    }
}

impl Output {
    /// Plays a file once over whatever is playing (previews).
    pub fn preview(&self, path: &Path) {
        let Some(sink) = &self._sink else { return };
        let Some(samples) = load_audio(path) else { return };
        sink.mixer().add(rodio::buffer::SamplesBuffer::new(std::num::NonZero::new(2).unwrap(), std::num::NonZero::new(RATE).unwrap(), samples));
    }

    /// Plays a source over whatever is playing (the Browsers' previews, with their volume and
    /// stop); false without a sound device.
    pub fn add_source<S: rodio::Source + std::marker::Send + 'static>(&self, src: S) -> bool {
        let Some(sink) = &self._sink else { return false };
        sink.mixer().add(src);
        true
    }
}

/// A WAV / AIFF / MP3 / FLAC file as stereo samples at RATE (linear resampling).
pub fn load_audio(path: &Path) -> Option<Vec<f32>> {
    use rodio::Source;
    let f = std::fs::File::open(path).ok()?;
    let d = rodio::Decoder::try_from(f).ok()?;
    let ch = d.channels().get() as usize;
    let rate = d.sample_rate().get() as f64;
    let raw: Vec<f32> = d.collect();
    let frames = raw.len() / ch.max(1);
    let out_frames = (frames as f64 * RATE as f64 / rate) as usize;
    let mut out = Vec::with_capacity(out_frames * 2);
    for k in 0..out_frames {
        let src = k as f64 * rate / RATE as f64;
        let i = src as usize;
        let t = (src - i as f64) as f32;
        let get = |f: usize, c: usize| raw.get(f * ch + c.min(ch - 1)).copied().unwrap_or(0.0);
        for c in 0..2 {
            out.push(get(i, c) * (1.0 - t) + get(i + 1, c) * t);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A track playing one click (an audio region) at frame `at`.
    fn click_track(at: usize, inserts: &[&str]) -> PlayTrack {
        let mut a = vec![0f32; 2 * 4096];
        a[2 * at] = 0.5;
        a[2 * at + 1] = 0.5;
        PlayTrack { channel: 0, program: 0, notes: Vec::new(), audio: vec![(0, Arc::new(a))], volume: 1.0, pan: 0.0, mute: false, solo: false, eq: None, instrument: None, instrument_floats: Vec::new(), inserts: inserts.iter().map(|n| (n.to_string(), Vec::new())).collect(), sends: Vec::new(), auto_volume: Vec::new(), auto_pan: Vec::new() }
    }

    fn run(e: &mut Engine, blocks: usize) -> Vec<f32> {
        e.playing = true;
        e.program_changes();
        let mut out = Vec::new();
        let mut buf = vec![0f32; 2 * BLOCK];
        for _ in 0..blocks {
            e.render(&mut buf);
            out.extend_from_slice(&buf);
        }
        out
    }

    fn peaks(out: &[f32]) -> Vec<usize> {
        (0..out.len() / 2).filter(|&f| out[2 * f].abs() > 0.05).collect()
    }

    #[test]
    fn latency_compensation_lines_tracks_up() {
        // a Limiter's look-ahead on one track: with compensation both clicks land together
        let mut e = Engine::new();
        e.tracks = vec![click_track(200, &["Limiter"]), click_track(200, &[])];
        let out = run(&mut e, 40);
        let lat = e.latency();
        assert!(lat > 0, "the limiter reports its look-ahead");
        let pk = peaks(&out);
        assert!(!pk.is_empty());
        assert!(pk.iter().all(|f| f.abs_diff(200 + lat) <= 2), "one click at {}: {pk:?}", 200 + lat);
        // without, the plain track comes first
        let mut e = Engine::new();
        e.compensate = false;
        e.tracks = vec![click_track(200, &["Limiter"]), click_track(200, &[])];
        let pk = peaks(&run(&mut e, 40));
        assert!(pk.iter().any(|f| f.abs_diff(200) <= 1) && pk.iter().any(|f| *f > 200 + 4), "{pk:?}");
    }

    #[test]
    fn outputs_and_sends_reach_the_stereo_out_through_auxes() {
        // track → Bus 2 (output) → aux → Stereo Out; a pre-fader send to Bus 1 → aux
        let mut e = Engine::new();
        let mut t = click_track(100, &[]);
        t.volume = 0.0;
        t.sends = vec![Send { bus: 0, gain: 1.0, pre: true }];
        e.tracks = vec![t, click_track(300, &[])];
        e.track_out = vec![Out::Off, Out::Bus(1)];
        let aux = |bus| AuxBus { bus, volume: 1.0, pan: 0.0, mute: false, inserts: Vec::new(), bypass: Vec::new(), output: Out::Stereo, sends: Vec::new() };
        e.auxes = vec![aux(0), aux(1)];
        let pk = peaks(&run(&mut e, 40));
        assert!(pk.contains(&100), "the pre-fader send is heard with the fader down: {pk:?}");
        assert!(pk.contains(&300), "the output to Bus 2 comes back through its aux: {pk:?}");
        // a muted aux silences what it carries
        e.auxes[1].mute = true;
        e.locate(0.0);
        let pk = peaks(&run(&mut e, 40));
        assert!(pk.contains(&100) && !pk.contains(&300), "{pk:?}");
    }

    #[test]
    fn feedback_is_cut_and_bypass_skips() {
        // an aux sending into its own bus doesn't run away
        let mut e = Engine::new();
        let mut t = click_track(100, &[]);
        t.sends = vec![Send { bus: 0, gain: 1.0, pre: false }];
        e.tracks = vec![t];
        e.track_out = vec![Out::Off];
        e.auxes = vec![AuxBus { bus: 0, volume: 1.0, pan: 0.0, mute: false, inserts: Vec::new(), bypass: Vec::new(), output: Out::Stereo, sends: vec![Send { bus: 0, gain: 1.0, pre: false }] }];
        let out = run(&mut e, 40);
        assert!(out.iter().all(|x| x.is_finite() && x.abs() <= 1.0));
        assert_eq!(peaks(&out), vec![100]);
        // a bypassed Limiter adds no latency
        let mut e = Engine::new();
        e.tracks = vec![click_track(200, &["Limiter"]), click_track(200, &[])];
        e.track_bypass = vec![vec![true], vec![]];
        run(&mut e, 4);
        assert_eq!(e.latency(), 0);
    }

    #[test]
    fn bounces_the_demo_with_the_soundfont() {
        let Some(sf) = system_soundfont() else { return };
        let p = crate::demo::song();
        let tracks: Vec<PlayTrack> = p.tracks.iter().enumerate().map(|(i, t)| PlayTrack {
            channel: if t.kind == lg_core::TrackKind::Drummer { 9 } else { i as u8 },
            program: crate::app::program_for(&t.name, t.kind),
            notes: t.regions.iter().flat_map(|r| r.notes.iter().map(|n| (n.pos, n.pos + n.len, n.pitch, n.velocity))).collect(),
            audio: Vec::new(),
            volume: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            eq: None,
            instrument: None,
            instrument_floats: Vec::new(),
            inserts: Vec::new(),
            sends: Vec::new(),
            auto_volume: Vec::new(),
            auto_pan: Vec::new(),
        }).collect();
        // two bars at 104 BPM: about 4.6 s, plus the tail
        let out = bounce(tracks, p.tempo, Vec::new(), Some(&sf), 0, 2 * 3840);
        let secs = out.len() as f64 / 2.0 / RATE as f64;
        assert!((secs - (2.0 * 4.0 * 60.0 / 104.0 + 2.0)).abs() < 0.1, "{secs} s");
        let rms = (out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32).sqrt();
        assert!(rms > 0.01, "the bounce has sound: rms {rms}");
        let w = wav_bytes(&out[..2 * RATE as usize]);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(w.len(), 44 + 2 * RATE as usize * 2);
    }

    #[test]
    fn each_track_mixes_on_its_own() {
        let Some(sf) = system_soundfont() else { return };
        let level = |vols: [f32; 2], mute: [bool; 2]| {
            let mut e = Engine::new();
            e.tracks = (0..2).map(|i| PlayTrack { channel: i as u8, program: 0, notes: vec![(0, 3840, 60 + 7 * i as u8, 100)], audio: Vec::new(), volume: vols[i], pan: 0.0, mute: mute[i], solo: false, eq: None, instrument: None, instrument_floats: Vec::new(), inserts: Vec::new(), sends: Vec::new(), auto_volume: Vec::new(), auto_pan: Vec::new() }).collect();
            e.load_soundfont(Some(&sf));
            e.playing = true;
            let mut buf = vec![0f32; 2 * BLOCK];
            let mut meters = [0f32; 2];
            for _ in 0..(RATE as usize / 4) / BLOCK {
                e.render(&mut buf);
                meters[0] = meters[0].max(e.meters[0]);
                meters[1] = meters[1].max(e.meters[1]);
            }
            meters
        };
        let both = level([1.0, 1.0], [false, false]);
        assert!(both[0] > 0.01 && both[1] > 0.01, "both tracks sound: {both:?}");
        let muted = level([1.0, 1.0], [true, false]);
        assert!(muted[0] < 1e-6 && muted[1] > 0.01, "muting one leaves the other: {muted:?}");
        let quieter = level([0.25, 1.0], [false, false]);
        assert!(quieter[0] < both[0] * 0.4, "−12 dB is quieter: {quieter:?} vs {both:?}");
    }

    #[test]
    fn plays_notes_in_time() {
        let mut e = Engine::new();
        e.tempo = 120.0;
        e.tracks = vec![PlayTrack { channel: 0, program: 0, notes: vec![(0, 480, 60, 100), (960, 1440, 64, 100)], audio: Vec::new(), volume: 1.0, pan: 0.0, mute: false, solo: false, eq: None, instrument: None, instrument_floats: Vec::new(), inserts: Vec::new(), sends: Vec::new(), auto_volume: Vec::new(), auto_pan: Vec::new() }];
        e.playing = true;
        let mut buf = vec![0f32; 2 * BLOCK];
        // half a second at 120 BPM = one beat = 960 ticks
        let blocks = (RATE as usize / 2) / BLOCK;
        let mut loud = 0f32;
        for _ in 0..blocks {
            e.render(&mut buf);
            loud = loud.max(buf.iter().fold(0f32, |a, x| a.max(x.abs())));
        }
        assert!((e.pos - 960.0).abs() < 10.0, "a beat played: {}", e.pos);
        assert!(loud > 0.01, "something sounded");
        // muted: silence
        e.locate(0.0);
        e.voices.clear();
        e.tracks[0].mute = true;
        let mut quiet = 0f32;
        for _ in 0..blocks {
            e.render(&mut buf);
            quiet = quiet.max(buf.iter().fold(0f32, |a, x| a.max(x.abs())));
        }
        assert!(quiet < 0.001, "muted track is silent ({quiet})");
    }
}
