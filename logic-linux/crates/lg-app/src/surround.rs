//! Surround and Spatial Audio with Dolby Atmos: the project's side (Spatial Audio on or off, the
//! surround format, every strip's Surround Panner or 3D Object Panner, the surround master's
//! effects with the Dolby Atmos plug-in and its monitoring / downmix / trim / binaural settings)
//! and the engine's: a multichannel bed bus in the project's format, objects carried with their
//! positions, the surround master chain (native surround effects, others Multi Mono), the
//! Dolby Atmos renderer, per-channel meters, a capture for surround bounces and ADM BWF export,
//! and the fold to the stereo monitoring device.
//!
//! A stereo project is untouched: none of this runs until a strip's output is Surround or a
//! 3D Object Panner, or Spatial Audio is set to Dolby Atmos (`active`).
//!
//! What a project keeps: in its project settings file (projsettings.rs), keys "Surround / …".

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use lg_core::plugin::Plugin;
use lg_dsp::surround::{self as sur, BinMode, Ch, Format, Object, Pan, RenderCfg};

use crate::app::App;
use crate::audio::{Out, Units, RATE};
use crate::mixer::AUX;

/// Track automation parameters (Logic's Main submenu entries for the panners): our numbers.
pub const P_ANGLE: u8 = 200;
pub const P_DIVERSITY: u8 = 201;
pub const P_ELEVATION: u8 = 202;
pub const P_LFE: u8 = 203;
pub const P_SPREAD: u8 = 204;
pub const P_CENTER: u8 = 205;
pub const P_X: u8 = 206;
pub const P_Y: u8 = 207;
pub const P_Z: u8 = 208;
pub const P_SIZE: u8 = 209;
pub const P_OSPREAD: u8 = 210;
pub const P_AMOUNT: u8 = 211;

pub const ATMOS: &str = "Dolby Atmos";

/// The project's surround and Spatial Audio state.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    /// Spatial Audio: Dolby Atmos (else Off).
    pub atmos: bool,
    /// The project surround format (the bed's, with Atmos).
    pub format: Format,
    /// Surround Panner / Balancer per strip id (tracks; auxes as AUX + index).
    pub pans: BTreeMap<usize, Pan>,
    /// 3D Object Panner per strip id.
    pub objects: BTreeMap<usize, Object>,
    /// The surround master's Audio FX (the Dolby Atmos plug-in among them with Atmos).
    pub master: Vec<Plugin>,
    /// The Dolby Atmos plug-in: monitoring format, downmixes, trims, Music / Movie mode.
    pub render: RenderCfg,
    /// Binaural render modes: bed channels (by channel) and objects (by strip id).
    pub bin_bed: BTreeMap<Ch, BinMode>,
    pub bin_obj: BTreeMap<usize, BinMode>,
    /// Apple Renderer buttons (no effect here: they need Apple's hardware).
    pub head_tracking: bool,
    pub personalized: bool,
    /// Save As Default: the starting pan / object position for new panners.
    pub default_pan: Option<Pan>,
    pub default_object: Option<Object>,
}

impl Default for State {
    fn default() -> State {
        State { atmos: false, format: Format::S51, pans: BTreeMap::new(), objects: BTreeMap::new(), master: Vec::new(), render: RenderCfg::default(), bin_bed: BTreeMap::new(), bin_obj: BTreeMap::new(), head_tracking: false, personalized: false, default_pan: None, default_object: None }
    }
}

impl State {
    pub fn pan(&self, s: usize) -> Pan {
        self.pans.get(&s).copied().or(self.default_pan).unwrap_or_default()
    }

    pub fn object(&self, s: usize) -> Object {
        self.objects.get(&s).copied().or(self.default_object).unwrap_or_default()
    }

    pub fn bin_mode_bed(&self, c: Ch) -> BinMode {
        if c == Ch::Lfe {
            return BinMode::Off;
        }
        self.bin_bed.get(&c).copied().unwrap_or_default()
    }

    /// Where the Dolby Atmos plug-in sits in the master chain.
    pub fn atmos_slot(&self) -> Option<usize> {
        self.master.iter().position(|u| u.name == ATMOS)
    }
}

/// The project is a surround (or Spatial Audio) project.
pub fn active(app: &App) -> bool {
    app.sur.atmos || app.mix.outputs.values().any(|o| matches!(o, Out::Surround | Out::Object)) || app.auxes.iter().any(|a| matches!(a.output, Out::Surround | Out::Object))
}

pub fn output_of_is_object(app: &App, s: usize) -> bool {
    crate::mixstate::output_of(app, s) == Out::Object
}

pub fn atmos(app: &App) -> bool {
    app.sur.atmos
}

/// Sets Spatial Audio (Dolby Atmos or Off): Atmos inserts the Dolby Atmos plug-in on the
/// surround master, sets the bed to 7.1.2 (5.1 and 7.1 allowed) and the frame rate to 24 fps,
/// and makes every audio and instrument track a bed track.
pub fn set_atmos(app: &mut App, on: bool) {
    crate::mixstate::checkpoint(app, "Spatial Audio");
    app.sur.atmos = on;
    if on {
        if !Format::ATMOS_BEDS.contains(&app.sur.format) || !app.sur.master.iter().any(|u| u.name == ATMOS) {
            app.sur.format = Format::S712;
        }
        if app.sur.atmos_slot().is_none() {
            app.sur.master.insert(0, crate::mixstate::new_plugin(ATMOS));
        }
        app.proj.0.insert("Synchronization / General / Frame Rate:".into(), "24".into());
        let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
        for t in 0..n {
            if !matches!(app.mix.outputs.get(&t), Some(Out::Object) | Some(Out::Bus(_)) | Some(Out::Off)) {
                app.mix.outputs.insert(t, Out::Surround);
            }
        }
    } else {
        app.sur.master.retain(|u| u.name != ATMOS);
        // object tracks become bed tracks
        for o in app.mix.outputs.values_mut() {
            if *o == Out::Object {
                *o = Out::Surround;
            }
        }
        for a in app.auxes.iter_mut() {
            if a.output == Out::Object {
                a.output = Out::Surround;
            }
        }
    }
    app.dirty = true;
    app.load_engine();
}

/// Sets the project surround format (existing plug-ins follow it: Multi Mono instances are laid
/// out again for the new channels).
pub fn set_format(app: &mut App, f: Format) {
    if app.sur.atmos && !Format::ATMOS_BEDS.contains(&f) {
        return;
    }
    crate::mixstate::checkpoint(app, "Surround Format");
    app.sur.format = f;
    app.dirty = true;
    app.load_engine();
}

/// Routes strip `s` (and the other selected strips when it is one of them) to `o`. Choosing
/// Surround or a 3D Object Panner makes the project a surround / Atmos bed or object strip.
pub fn set_output(app: &mut App, s: usize, o: Out) {
    let mut strips: Vec<usize> = vec![s];
    if app.mix.selected.contains(&s) || app.selected_track == Some(s) && app.mix.selected.len() > 1 {
        strips.extend(app.mix.selected.iter().copied().filter(|x| *x != s && *x < crate::mixstate::VCA));
    }
    // (3D objects only with Spatial Audio)
    let o = if o == Out::Object && !app.sur.atmos { Out::Surround } else { o };
    let first = strips[0];
    crate::mixstate::set_output(app, first, o);
    for x in strips.into_iter().skip(1) {
        if (AUX..crate::mixstate::VCA).contains(&x) {
            if let Some(a) = app.auxes.get_mut(x - AUX) {
                a.output = o;
            }
        } else {
            app.mix.outputs.insert(x, o);
        }
    }
    app.load_engine();
}

// ---- engine --------------------------------------------------------------------------------------

/// How a strip reaches the surround master.
#[derive(Clone, Debug, PartialEq)]
pub enum StripPan {
    /// A mono / stereo strip through its Surround Panner (gains per bed channel).
    Panner { pan: Pan, stereo: bool },
    /// A surround-input strip through its Surround Balancer.
    Balancer { pan: Pan },
    /// An object (mono or stereo: two objects).
    Object { obj: Object, stereo: bool, mode: BinMode },
}

/// A multichannel audio region a surround-input track plays: start tick, frames per channel.
#[derive(Clone)]
pub struct McRegion {
    pub start: u32,
    pub length: u32,
    pub chans: Arc<Vec<Vec<f32>>>,
}

/// What a bounce or an export collects while rendering.
#[derive(Default)]
pub struct Capture {
    /// The surround master's output (after its fader), in `layout`.
    pub out: Vec<Vec<f32>>,
    pub layout: Vec<Ch>,
    /// ADM: the bed as it enters the Dolby Atmos plug-in, the objects' audio and positions
    /// (frame, position) per object (by node; a stereo object as two: node * 2 + side).
    pub adm: bool,
    pub bed: Vec<Vec<f32>>,
    pub objects: BTreeMap<usize, (String, Vec<f32>, Vec<(usize, Object)>)>,
    pub frames: usize,
}

/// The engine's surround part (in `Engine::surround` while a project is surround).
pub struct EngineSurround {
    pub bed: Format,
    pub atmos: bool,
    /// Per node (tracks, then auxes): its panning; None for strips not routed to surround.
    pub strips: Vec<Option<StripPan>>,
    pub names: Vec<String>,
    /// The surround master's Audio FX (name, saved floats), bypass, and the running units.
    pub master: Vec<(String, Vec<f32>)>,
    pub master_bypass: Vec<bool>,
    pub units: Units,
    built_for: (Vec<String>, Format, sur::Monitoring),
    pub render: RenderCfg,
    pub bin_bed: Vec<BinMode>,
    renderer: sur::Renderer,
    bus: Vec<Vec<f32>>,
    gains: Vec<Option<(Vec<f32>, Vec<f32>)>>,
    prev: Vec<Option<(Vec<f32>, Vec<f32>)>>,
    objects: Vec<(usize, u8, Vec<f32>, Object, BinMode)>,
    /// Multichannel regions by node.
    pub regions: Vec<(usize, McRegion)>,
    /// Automation by node: (parameter, points as (tick, 0–127)).
    pub lanes: Vec<(usize, u8, Vec<(u32, f32)>)>,
    /// The device is two channels: the mix folded down (false) or rendered binaurally.
    pub monitor_binaural: bool,
    headphones: sur::Headphones,
    /// Peak per output channel (decaying) and the output layout, for the master's meters.
    pub meters: Vec<f32>,
    pub layout: Vec<Ch>,
    pub capture: Option<Capture>,
    /// Peak per object (for the 3D Object Viewer).
    pub object_peaks: HashMap<usize, f32>,
}

impl EngineSurround {
    pub fn new() -> EngineSurround {
        EngineSurround {
            bed: Format::S51,
            atmos: false,
            strips: Vec::new(),
            names: Vec::new(),
            master: Vec::new(),
            master_bypass: Vec::new(),
            units: Units::default(),
            built_for: (Vec::new(), Format::S51, sur::Monitoring::M714),
            render: RenderCfg::default(),
            bin_bed: Vec::new(),
            renderer: sur::Renderer::new(RATE as f32),
            bus: Vec::new(),
            gains: Vec::new(),
            prev: Vec::new(),
            objects: Vec::new(),
            regions: Vec::new(),
            lanes: Vec::new(),
            monitor_binaural: false,
            headphones: sur::Headphones::new(RATE as f32, 1.0),
            meters: Vec::new(),
            layout: Vec::new(),
            capture: None,
            object_peaks: HashMap::new(),
        }
    }

    /// Builds the master chain's units when the list, the bed or the monitoring format changed:
    /// native surround effects as they are, others Multi Mono for the channels at their slot.
    pub fn build(&mut self, tempo: f64) {
        let names: Vec<String> = self.master.iter().map(|x| x.0.clone()).collect();
        let key = (names.clone(), self.bed, self.render.monitoring);
        if self.built_for == key && self.units.inserts.len() == self.units.insert_index.len() && !self.units.inserts_for.is_empty() == !names.is_empty() {
            return;
        }
        let mut fx: Vec<Box<dyn lg_dsp::Effect>> = Vec::new();
        let mut at = Vec::new();
        let mut layout: Vec<Ch> = self.bed.channels().to_vec();
        for (k, (n, floats)) in self.master.iter().enumerate() {
            if n == ATMOS {
                if self.atmos {
                    layout = self.render.monitoring.layout();
                }
                continue;
            }
            let unit: Option<Box<dyn lg_dsp::Effect>> = if lg_fx::surround::NATIVE.contains(&n.as_str()) {
                crate::units::effect(n, RATE as f32)
            } else {
                let name = n.clone();
                lg_fx::surround::MultiMono::new(Box::new(move || crate::units::effect(&name, RATE as f32))).map(|mut m| {
                    m.configure(&layout);
                    Box::new(m) as Box<dyn lg_dsp::Effect>
                })
            };
            if let Some(mut e) = unit {
                if !floats.is_empty() {
                    e.load_floats(floats);
                }
                e.set_tempo(tempo);
                fx.push(e);
                at.push(k);
            }
        }
        self.units.inserts = fx;
        self.units.insert_index = at;
        self.units.inserts_for = names;
        self.built_for = key;
    }

    /// Start of a block: clears the bus, follows automation, plays surround-input regions.
    pub fn begin(&mut self, frames: usize, tick: f64, tps: f64, playing: bool, tracks: &[crate::audio::PlayTrack]) {
        let n = self.bed.n();
        self.bus.resize_with(n, Vec::new);
        self.bus.truncate(n);
        for c in self.bus.iter_mut() {
            c.clear();
            c.resize(frames, 0.0);
        }
        self.objects.clear();
        if self.gains.len() != self.strips.len() {
            self.gains = vec![None; self.strips.len()];
            self.prev = vec![None; self.strips.len()];
        }
        // automation
        if playing && !self.lanes.is_empty() {
            for (node, param, pts) in &self.lanes {
                let Some(v) = crate::automation::curve_at(pts, tick) else { continue };
                let Some(Some(sp)) = self.strips.get_mut(*node) else { continue };
                apply_param(sp, *param, v);
            }
        }
        // gains for this block (a strip's gains ramp from the last block's)
        for (k, sp) in self.strips.iter().enumerate() {
            let g = match sp {
                Some(StripPan::Panner { pan, stereo }) => Some(if *stereo { sur::stereo_gains(self.bed, pan) } else { let g = sur::mono_gains(self.bed, pan); (g.clone(), g) }),
                Some(StripPan::Balancer { pan }) => {
                    let g = sur::balancer_gains(self.bed, pan);
                    Some((g.clone(), g))
                }
                _ => None,
            };
            self.prev[k] = self.gains[k].take().or_else(|| g.clone());
            self.gains[k] = g;
        }
        // surround-input tracks' multichannel regions, through their balancer and fader
        if playing {
            let any_solo = tracks.iter().any(|t| t.solo);
            for (node, r) in &self.regions {
                let Some(t) = tracks.get(*node) else { continue };
                if t.mute || (any_solo && !t.solo) {
                    continue;
                }
                let g = match self.gains.get(*node) {
                    Some(Some((g, _))) => g.clone(),
                    _ => vec![1.0; n],
                };
                let len = r.chans.first().map_or(0, |c| c.len());
                for f in 0..frames {
                    let at = tick + f as f64 * tps - r.start as f64;
                    if at < 0.0 || at >= r.length as f64 {
                        continue;
                    }
                    let frame = (at / tps) as usize;
                    if frame >= len {
                        break;
                    }
                    for (c, src) in r.chans.iter().enumerate().take(n) {
                        self.bus[c][f] += src[frame] * g[c] * t.volume;
                    }
                }
            }
        }
    }

    /// A strip's output (after its fader) arriving at the surround master.
    pub fn add(&mut self, node: usize, l: &[f32], r: &[f32]) {
        let frames = l.len();
        match self.strips.get(node).cloned().flatten() {
            Some(StripPan::Object { obj, stereo, mode }) => {
                if stereo {
                    let (a, b) = obj.stereo();
                    self.objects.push((node, 0, l.to_vec(), a, mode));
                    self.objects.push((node, 1, r.to_vec(), b, mode));
                } else {
                    let m: Vec<f32> = l.iter().zip(r).map(|(a, b)| 0.5 * (a + b)).collect();
                    self.objects.push((node, 0, m, obj, mode));
                }
                let pk = l.iter().chain(r).fold(0f32, |m, x| m.max(x.abs()));
                let e = self.object_peaks.entry(node).or_insert(0.0);
                *e = e.max(pk);
            }
            Some(StripPan::Panner { .. }) | Some(StripPan::Balancer { .. }) if matches!(self.gains.get(node), Some(Some(_))) => {
                let (gl, gr) = self.gains[node].clone().unwrap_or_default();
                let (pl, pr) = self.prev[node].clone().unwrap_or_else(|| (gl.clone(), gr.clone()));
                let mono = matches!(self.strips[node], Some(StripPan::Panner { stereo: false, .. }) | Some(StripPan::Balancer { .. }));

                for c in 0..self.bus.len() {
                    let (a0, a1) = (pl.get(c).copied().unwrap_or(0.0), gl.get(c).copied().unwrap_or(0.0));
                    let (b0, b1) = (pr.get(c).copied().unwrap_or(0.0), gr.get(c).copied().unwrap_or(0.0));
                    if a0 == 0.0 && a1 == 0.0 && b0 == 0.0 && b1 == 0.0 {
                        continue;
                    }
                    let bus = &mut self.bus[c];
                    for f in 0..frames {
                        let t = f as f32 / frames as f32;
                        let (ga, gb) = (a0 + (a1 - a0) * t, b0 + (b1 - b0) * t);
                        bus[f] += if mono { 0.5 * (l[f] + r[f]) * ga } else { l[f] * ga + r[f] * gb };
                    }
                }
            }
            _ => {
                // (no panning set up yet: front left and right)
                if let (Some(a), Some(b)) = (self.bed.index(Ch::L), self.bed.index(Ch::R)) {
                    for f in 0..frames {
                        self.bus[a][f] += l[f];
                        self.bus[b][f] += r[f];
                    }
                }
            }
        }
    }

    /// End of a block: the surround master chain (the Dolby Atmos plug-in's renderer among
    /// it), the strips still on the stereo output added to L / R, meters and capture; `out`
    /// (interleaved stereo, holding the stereo-routed strips) becomes the device's fold.
    pub fn finish(&mut self, out: &mut [f32], frames: usize, at: lg_dsp::Transport, master: f32) {
        let mut chans = std::mem::take(&mut self.bus);
        let mut layout: Vec<Ch> = self.bed.channels().to_vec();
        let objects = std::mem::take(&mut self.objects);
        let mut rendered = false;
        let atmos_at = self.master.iter().position(|x| x.0 == ATMOS);
        let run_until = |chans: &mut Vec<Vec<f32>>, layout: &mut Vec<Ch>, from: usize, to: usize, units: &mut Units, bypass: &[bool]| {
            for (fx, k) in units.inserts.iter_mut().zip(units.insert_index.iter()) {
                if *k < from || *k >= to || bypass.get(*k).copied().unwrap_or(false) {
                    continue;
                }
                fx.set_transport(at);
                fx.process_surround(chans, layout);
            }
        };
        let split = atmos_at.filter(|_| self.atmos).unwrap_or(usize::MAX);
        run_until(&mut chans, &mut layout, 0, split, &mut self.units, &self.master_bypass);
        if self.atmos {
            if let Some(cap) = self.capture.as_mut().filter(|c| c.adm) {
                cap.bed.resize_with(chans.len(), Vec::new);
                for (d, s) in cap.bed.iter_mut().zip(chans.iter()) {
                    d.extend_from_slice(s);
                }
                for (node, side, sig, pos, _) in &objects {
                    let key = node * 2 + *side as usize;
                    let e = cap.objects.entry(key).or_insert_with(|| (self.names.get(*node).cloned().unwrap_or_default() + if *side == 1 { " R" } else { "" }, vec![0.0; cap.frames], Vec::new()));
                    e.1.resize(cap.frames, 0.0);
                    e.1.extend_from_slice(sig);
                    if e.2.last().is_none_or(|l| l.1 != *pos) {
                        e.2.push((cap.frames, *pos));
                    }
                }
                cap.frames += frames;
            }
            if split != usize::MAX {
                let ins: Vec<sur::ObjectIn> = objects.iter().map(|(_, _, s, p, m)| sur::ObjectIn { signal: s, pos: *p, mode: *m }).collect();
                chans = self.renderer.render(&self.render, self.bed, &chans, &self.bin_bed, &ins, frames);
                layout = self.render.monitoring.layout();
                rendered = true;
            }
        }
        if !rendered && !objects.is_empty() {
            // (no Dolby Atmos plug-in on the master: objects rendered straight onto the bed)
            for (_, _, sig, pos, _) in &objects {
                let g = sur::object_gains(self.bed, pos);
                for (c, gc) in g.iter().enumerate() {
                    if let Some(buf) = chans.get_mut(c) {
                        for f in 0..frames {
                            buf[f] += sig[f] * gc;
                        }
                    }
                }
            }
        }
        if split != usize::MAX {
            run_until(&mut chans, &mut layout, split + 1, usize::MAX, &mut self.units, &self.master_bypass);
        }
        // strips on the stereo output: to front left / right
        let (li, ri) = (layout.iter().position(|c| *c == Ch::L), layout.iter().position(|c| *c == Ch::R));
        for f in 0..frames {
            if let Some(i) = li {
                chans[i][f] += out[2 * f];
            }
            if let Some(i) = ri {
                chans[i][f] += out[2 * f + 1];
            }
        }
        // meters, capture (after the master fader)
        if self.meters.len() != layout.len() || self.layout != layout {
            self.meters = vec![0.0; layout.len()];
        }
        for (k, c) in chans.iter().enumerate() {
            let pk = c.iter().fold(0f32, |m, x| m.max((x * master).abs()));
            self.meters[k] = self.meters[k].max(pk);
        }
        if let Some(cap) = self.capture.as_mut() {
            if cap.layout != layout {
                cap.layout = layout.clone();
                cap.out.resize_with(layout.len(), Vec::new);
            }
            for (d, s) in cap.out.iter_mut().zip(chans.iter()) {
                d.extend(s.iter().map(|x| x * master));
            }
        }
        // the device: two channels
        for x in out.iter_mut() {
            *x = 0.0;
        }
        if layout.len() == 2 {
            for f in 0..frames {
                out[2 * f] = chans[0][f];
                out[2 * f + 1] = chans[1][f];
            }
        } else if self.monitor_binaural {
            if let Some(fmt) = Format::of_channels(&layout) {
                let ordered: Vec<Vec<f32>> = fmt.channels().iter().map(|c| layout.iter().position(|x| x == c).map(|k| chans[k].clone()).unwrap_or_else(|| vec![0.0; frames])).collect();
                let (mut l, mut r) = (vec![0f32; frames], vec![0f32; frames]);
                self.headphones.run(fmt, &ordered, &mut l, &mut r);
                for f in 0..frames {
                    out[2 * f] = l[f];
                    out[2 * f + 1] = r[f];
                }
            }
        } else {
            for (k, c) in layout.iter().enumerate() {
                let (gl, gr) = sur::fold_gains(*c);
                for f in 0..frames {
                    out[2 * f] += chans[k][f] * gl;
                    out[2 * f + 1] += chans[k][f] * gr;
                }
            }
        }
        self.layout = layout;
        let fall = 0.985f32.powf(frames as f32 / 64.0);
        for m in self.meters.iter_mut() {
            *m *= fall;
        }
        for m in self.object_peaks.values_mut() {
            *m *= fall;
        }
        chans.truncate(self.bed.n());
        self.bus = chans;
    }
}

impl Default for EngineSurround {
    fn default() -> Self {
        EngineSurround::new()
    }
}

/// An automation value (0–127) applied to a strip's panning.
fn apply_param(sp: &mut StripPan, param: u8, v: f32) {
    let u = (v / 127.0).clamp(0.0, 1.0);
    match sp {
        StripPan::Panner { pan, .. } | StripPan::Balancer { pan } => match param {
            P_ANGLE => pan.angle = u * 360.0 - 180.0,
            P_DIVERSITY => pan.diversity = u,
            P_ELEVATION => pan.elevation = u * 90.0,
            P_LFE => pan.lfe_db = u * 88.0 - 82.0,
            P_CENTER => pan.center_db = u * 88.0 - 82.0,
            P_SPREAD => pan.spread = u * 360.0 - 180.0,
            P_AMOUNT => pan.amount = u,
            _ => {}
        },
        StripPan::Object { obj, .. } => match param {
            P_X => obj.x = u * 2.0 - 1.0,
            P_Y => obj.y = u * 2.0 - 1.0,
            P_Z => obj.z = u,
            P_SIZE => obj.size = u,
            P_OSPREAD => obj.spread = u,
            _ => {}
        },
    }
}

/// A panner value as automation's 0–127.
pub fn param_value(app: &App, s: usize, param: u8) -> f32 {
    let p = app.sur.pan(s);
    let o = app.sur.object(s);
    127.0
        * match param {
            P_ANGLE => (p.angle + 180.0) / 360.0,
            P_DIVERSITY => p.diversity,
            P_ELEVATION => p.elevation / 90.0,
            P_LFE => ((p.lfe_db.max(-82.0) + 82.0) / 88.0).clamp(0.0, 1.0),
            P_CENTER => ((p.center_db.max(-82.0) + 82.0) / 88.0).clamp(0.0, 1.0),
            P_SPREAD => (p.spread + 180.0) / 360.0,
            P_AMOUNT => p.amount,
            P_X => (o.x + 1.0) / 2.0,
            P_Y => (o.y + 1.0) / 2.0,
            P_Z => o.z,
            P_SIZE => o.size,
            P_OSPREAD => o.spread,
            _ => 0.0,
        }
}

pub fn param_name(p: u8) -> Option<&'static str> {
    Some(match p {
        P_ANGLE => "Surround Angle",
        P_DIVERSITY => "Surround Diversity",
        P_ELEVATION => "Surround Elevation",
        P_LFE => "Surround LFE",
        P_SPREAD => "Surround Spread",
        P_CENTER => "Surround Center Level",
        P_AMOUNT => "Surround Amount",
        P_X => "3D Object Left/Right",
        P_Y => "3D Object Back/Front",
        P_Z => "3D Object Elevation",
        P_SIZE => "3D Object Size",
        P_OSPREAD => "3D Object Spread",
        _ => return None,
    })
}

/// The panner parameters a track's automation menu offers (Main submenu), by its output.
pub fn auto_params(app: &App, t: usize) -> Vec<u8> {
    match crate::mixstate::output_of(app, t) {
        Out::Surround => {
            let mut v = vec![P_ANGLE, P_DIVERSITY, P_LFE];
            if app.sur.format.has_height() {
                v.push(P_ELEVATION);
            }
            if strip_stereo(app, t) {
                v.push(P_SPREAD);
            }
            v.push(P_CENTER);
            v
        }
        Out::Object => {
            let mut v = vec![P_X, P_Y, P_Z, P_SIZE];
            if strip_stereo(app, t) {
                v.push(P_OSPREAD);
            }
            v
        }
        _ => Vec::new(),
    }
}

/// Records a panner move into the track's automation (Touch / Latch / Write while playing).
pub fn record(app: &mut App, s: usize, params: &[u8]) {
    if s >= AUX {
        return;
    }
    for p in params {
        let v = param_value(app, s, *p);
        crate::automation::record(app, s, *p, v);
    }
}

/// A mono or stereo strip (Logic: the channel strip's input format): an audio track with a
/// mono input is mono, everything else stereo.
pub fn strip_stereo(app: &App, s: usize) -> bool {
    if s >= AUX {
        return true;
    }
    let audio = app.project.as_ref().and_then(|p| p.tracks.get(s)).is_some_and(|t| t.kind == lg_core::TrackKind::Audio);
    if !audio {
        return true;
    }
    !matches!(app.mix.inputs.get(&s).copied().unwrap_or_default(), crate::mixstate::Input::Mono0 | crate::mixstate::Input::Mono(_)) || app.rec.inputs.get(&s).is_some_and(|i| i.label().contains('-'))
}

/// Channels of an audio file (header only).
fn file_channels(p: &std::path::Path) -> usize {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<PathBuf, usize>> = Default::default();
    }
    CACHE.with(|c| {
        if let Some(n) = c.borrow().get(p) {
            return *n;
        }
        let n = header_channels(p).unwrap_or(2);
        c.borrow_mut().insert(p.to_path_buf(), n);
        n
    })
}

fn header_channels(p: &std::path::Path) -> Option<usize> {
    use std::io::Read;
    let mut f = std::fs::File::open(p).ok()?;
    let mut b = vec![0u8; 4096];
    let n = f.read(&mut b).ok()?;
    let b = &b[..n];
    if b.len() >= 12 && &b[8..12] == b"WAVE" {
        let i = b.windows(4).position(|w| w == b"fmt ")?;
        return Some(u16::from_le_bytes([*b.get(i + 10)?, *b.get(i + 11)?]) as usize);
    }
    if b.len() >= 12 && &b[0..4] == b"FORM" {
        let i = b.windows(4).position(|w| w == b"COMM")?;
        return Some(u16::from_be_bytes([*b.get(i + 8)?, *b.get(i + 9)?]) as usize);
    }
    None
}

/// An audio track whose files are surround (more than two channels): its channel strip's input
/// format is Surround and its pan control a Surround Balancer.
pub fn surround_input(app: &App, s: usize) -> bool {
    let Some(t) = app.project.as_ref().and_then(|p| p.tracks.get(s)) else { return false };
    t.kind == lg_core::TrackKind::Audio && t.regions.iter().find_map(|r| r.audio.as_ref()).is_some_and(|a| file_channels(&a.file) > 2)
}

/// A surround file's channels at the engine's rate, laid out for `bed` (by count: the format
/// with that many channels in Logic's order, else in order).
fn load_mc(path: &std::path::Path, bed: Format) -> Option<Vec<Vec<f32>>> {
    let af = lg_audio::file::AudioFile::read(path).ok()?;
    let ch = af.channels.max(1);
    let ratio = af.rate as f64 / RATE as f64;
    let n = (af.frames() as f64 / ratio) as usize;
    let src_fmt = Format::ALL.into_iter().find(|f| f.n() == ch && (f.n() != bed.n() || *f == bed));
    let mut out = vec![vec![0f32; n]; bed.n()];
    for c in 0..ch {
        let dest = match src_fmt {
            Some(f) => f.channels().get(c).and_then(|x| bed.index(*x)),
            None => (c < bed.n()).then_some(c),
        };
        let Some(d) = dest else { continue };
        for (k, o) in out[d].iter_mut().enumerate() {
            let s = k as f64 * ratio;
            let i = s as usize;
            let t = (s - i as f64) as f32;
            let a = af.at(i, c);
            let b = if i + 1 < af.frames() { af.at(i + 1, c) } else { a };
            *o = a + (b - a) * t;
        }
    }
    Some(out)
}

thread_local! {
    static MC_CACHE: std::cell::RefCell<HashMap<(PathBuf, Format), Arc<Vec<Vec<f32>>>>> = Default::default();
}

/// The surround setup for the engine (mixstate::to_engine calls this last).
pub fn to_engine(app: &App, e: &mut crate::audio::Engine) {
    if !active(app) {
        e.surround = None;
        return;
    }
    let Some(p) = app.project.as_ref() else { return };
    let n_tracks = p.tracks.len();
    let mut s = e.surround.take().unwrap_or_else(|| Box::new(EngineSurround::new()));
    s.bed = app.sur.format;
    s.atmos = app.sur.atmos;
    s.render = app.sur.render.clone();
    s.render.movie_mode = app.sur.render.movie_mode;
    s.bin_bed = app.sur.format.channels().iter().map(|c| app.sur.bin_mode_bed(*c)).collect();
    s.monitor_binaural = app.logic_settings.get(MONITOR_KEY) == "Binaural";
    let n = n_tracks + app.auxes.len();
    let mut strips = vec![None; n];
    let mut names = vec![String::new(); n];
    for (node, slot) in strips.iter_mut().enumerate() {
        let id = if node < n_tracks { node } else { AUX + node - n_tracks };
        names[node] = crate::mixstate::strip_name(app, id);
        *slot = match crate::mixstate::output_of(app, id) {
            Out::Surround => Some(if node < n_tracks && surround_input(app, id) { StripPan::Balancer { pan: app.sur.pan(id) } } else { StripPan::Panner { pan: app.sur.pan(id), stereo: strip_stereo(app, id) } }),
            Out::Object => Some(StripPan::Object { obj: app.sur.object(id), stereo: strip_stereo(app, id), mode: app.sur.bin_obj.get(&id).copied().unwrap_or_default() }),
            _ => None,
        };
    }
    s.strips = strips;
    s.names = names;
    // surround-input tracks play their multichannel files here, not as stereo
    s.regions.clear();
    for (t, tr) in p.tracks.iter().enumerate() {
        if !matches!(s.strips.get(t), Some(Some(StripPan::Balancer { .. }))) {
            continue;
        }
        if let Some(pt) = e.tracks.get_mut(t) {
            pt.audio.clear();
        }
        for r in tr.regions.iter().filter(|r| !r.muted) {
            let Some(a) = &r.audio else { continue };
            let key = (a.file.clone(), app.sur.format);
            let data = MC_CACHE.with(|c| c.borrow().get(&key).cloned()).or_else(|| {
                let d = Arc::new(load_mc(&a.file, app.sur.format)?);
                MC_CACHE.with(|c| c.borrow_mut().insert(key.clone(), d.clone()));
                Some(d)
            });
            if let Some(chans) = data {
                s.regions.push((t, McRegion { start: r.start, length: r.length, chans }));
            }
        }
    }
    // automation lanes (tracks not in Off mode)
    s.lanes.clear();
    for (t, tr) in p.tracks.iter().enumerate() {
        if crate::automation::mode(app, t) == crate::automation::AutoMode::Off {
            continue;
        }
        for l in tr.automation.iter().filter(|l| l.plugin.is_none() && (P_ANGLE..=P_AMOUNT).contains(&l.param) && !l.points.is_empty()) {
            s.lanes.push((t, l.param, l.points.iter().map(|(k, v)| (*k, *v as f32)).collect()));
        }
    }
    s.master = app.sur.master.iter().map(|u| (u.name.clone(), u.params.clone())).collect();
    s.master_bypass = app.sur.master.iter().map(|u| u.bypass).collect();
    s.build(e.tempo);
    e.surround = Some(s);
}

/// Settings key: how a surround mix is heard on a two-channel device.
pub const MONITOR_KEY: &str = "Audio / I/O Assignments / Bramble Monitor";

// ---- kept with the project -------------------------------------------------------------------------

fn pan_text(p: &Pan) -> String {
    format!("{},{},{},{},{},{},{},{},{},{},{},{},{}", p.angle, p.diversity, p.elevation, p.spread, p.center_db, p.lfe_db, p.separation as u8, p.sep_xf, p.sep_xr, p.sep_y, p.off, p.spherical as u8, p.amount)
}

fn pan_of(s: &str) -> Option<Pan> {
    let v: Vec<f32> = s.split(',').map(|x| x.trim().parse::<f32>().unwrap_or(f32::NEG_INFINITY)).collect();
    if v.len() < 13 {
        return None;
    }
    Some(Pan { angle: v[0], diversity: v[1], elevation: v[2], spread: v[3], center_db: v[4], lfe_db: v[5], separation: v[6] > 0.5, sep_xf: v[7], sep_xr: v[8], sep_y: v[9], off: v[10] as u32, spherical: v[11] > 0.5, amount: v[12] })
}

fn obj_text(o: &Object) -> String {
    format!("{},{},{},{},{}", o.x, o.y, o.z, o.size, o.spread)
}

fn obj_of(s: &str) -> Option<Object> {
    let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (v.len() >= 5).then(|| Object { x: v[0], y: v[1], z: v[2], size: v[3], spread: v[4] })
}

/// The state as (key, value) lines for the project settings file.
pub fn to_values(app: &App) -> Vec<(String, String)> {
    let st = &app.sur;
    let mut v: Vec<(String, String)> = Vec::new();
    let any_out = app.mix.outputs.values().any(|o| matches!(o, Out::Surround | Out::Object)) || app.auxes.iter().any(|a| matches!(a.output, Out::Surround | Out::Object));
    if !st.atmos && !any_out && *st == State::default() {
        return v;
    }
    v.push(("Surround /  / Spatial Audio".into(), if st.atmos { "Dolby Atmos".into() } else { "Off".into() }));
    v.push(("Surround /  / Format".into(), st.format.name().into()));
    for (s, p) in &st.pans {
        v.push((format!("Surround / pan / {s}"), pan_text(p)));
    }
    for (s, o) in &st.objects {
        v.push((format!("Surround / object / {s}"), obj_text(o)));
    }
    for (s, o) in &app.mix.outputs {
        match o {
            Out::Surround => v.push((format!("Surround / out / {s}"), "Surround".into())),
            Out::Object => v.push((format!("Surround / out / {s}"), "Object".into())),
            _ => {}
        }
    }
    for (a, x) in app.auxes.iter().enumerate() {
        match x.output {
            Out::Surround => v.push((format!("Surround / out / {}", AUX + a), "Surround".into())),
            Out::Object => v.push((format!("Surround / out / {}", AUX + a), "Object".into())),
            _ => {}
        }
    }
    for (k, u) in st.master.iter().enumerate() {
        let floats: Vec<String> = u.params.iter().map(|f| f.to_string()).collect();
        v.push((format!("Surround / master / {k:02}"), format!("{}|{}|{}", u.name, u.bypass as u8, floats.join(","))));
    }
    let r = &st.render;
    v.push(("Surround / render / monitoring".into(), r.monitoring.name().into()));
    v.push(("Surround / render / down51".into(), r.down51.name().into()));
    v.push(("Surround / render / down20".into(), r.down20.name().into()));
    v.push(("Surround / render / movie".into(), (r.movie_mode as u8).to_string()));
    for (k, t) in r.trims.iter().enumerate() {
        v.push((format!("Surround / trim / {k}"), format!("{},{},{},{},{}", t.manual as u8, t.surround_db, t.height_db, t.overhead, t.listener)));
    }
    for (c, m) in &st.bin_bed {
        v.push((format!("Surround / binaural / {}", c.label()), m.name().into()));
    }
    for (s, m) in &st.bin_obj {
        v.push((format!("Surround / binaural / {s}"), m.name().into()));
    }
    if let Some(p) = &st.default_pan {
        v.push(("Surround / default / pan".into(), pan_text(p)));
    }
    if let Some(o) = &st.default_object {
        v.push(("Surround / default / object".into(), obj_text(o)));
    }
    v
}

/// The state read back from the project settings.
pub fn from_values(vals: &BTreeMap<String, String>) -> (State, Vec<(usize, Out)>) {
    let mut st = State::default();
    let mut outs = Vec::new();
    let mut master: BTreeMap<String, Plugin> = BTreeMap::new();
    for (k, v) in vals.iter().filter(|(k, _)| k.starts_with("Surround / ")) {
        let parts: Vec<&str> = k.splitn(3, " / ").collect();
        let (sec, key) = (parts.get(1).copied().unwrap_or(""), parts.get(2).copied().unwrap_or(""));
        match (sec, key) {
            ("", "Spatial Audio") => st.atmos = v == "Dolby Atmos",
            ("", "Format") => st.format = Format::from_name(v).unwrap_or_default(),
            ("pan", s) => {
                if let (Ok(s), Some(p)) = (s.parse(), pan_of(v)) {
                    st.pans.insert(s, p);
                }
            }
            ("object", s) => {
                if let (Ok(s), Some(o)) = (s.parse(), obj_of(v)) {
                    st.objects.insert(s, o);
                }
            }
            ("out", s) => {
                if let Ok(s) = s.parse() {
                    outs.push((s, if v == "Object" { Out::Object } else { Out::Surround }));
                }
            }
            ("master", k) => {
                let f: Vec<&str> = v.splitn(3, '|').collect();
                let mut u = crate::mixstate::new_plugin(f.first().copied().unwrap_or(""));
                u.bypass = f.get(1) == Some(&"1");
                u.params = f.get(2).map(|x| x.split(',').filter_map(|y| y.parse().ok()).collect()).unwrap_or_default();
                master.insert(k.to_string(), u);
            }
            ("render", "monitoring") => st.render.monitoring = sur::Monitoring::from_name(v).unwrap_or_default(),
            ("render", "down51") => st.render.down51 = sur::Down51::ALL.into_iter().find(|d| d.name() == v).unwrap_or_default(),
            ("render", "down20") => st.render.down20 = sur::Down20::ALL.into_iter().find(|d| d.name() == v).unwrap_or_default(),
            ("render", "movie") => st.render.movie_mode = v == "1",
            ("trim", k) => {
                let f: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                if let (Ok(k), 5) = (k.parse::<usize>(), f.len()) {
                    if k < 5 {
                        st.render.trims[k] = sur::Trim { manual: f[0] > 0.5, surround_db: f[1], height_db: f[2], overhead: f[3], listener: f[4] };
                    }
                }
            }
            ("binaural", k) => {
                let m = BinMode::from_name(v);
                match (Ch::from_label(k), k.parse::<usize>()) {
                    (_, Ok(s)) => {
                        st.bin_obj.insert(s, m);
                    }
                    (Some(c), _) => {
                        st.bin_bed.insert(c, m);
                    }
                    _ => {}
                }
            }
            ("default", "pan") => st.default_pan = pan_of(v),
            ("default", "object") => st.default_object = obj_of(v),
            _ => {}
        }
    }
    st.master = master.into_values().collect();
    (st, outs)
}

/// A project opened: its surround state (projsettings::after_open).
pub fn after_open(app: &mut App) {
    let (st, outs) = from_values(&app.proj.0);
    app.sur = st;
    for (s, o) in outs {
        if (AUX..crate::mixstate::VCA).contains(&s) {
            if let Some(a) = app.auxes.get_mut(s - AUX) {
                a.output = o;
            }
        } else {
            app.mix.outputs.insert(s, o);
        }
    }
    crate::projfile::outputs_opened(app);
    app.load_engine();
}

/// Before the project settings are written (projsettings::save).
pub fn store(app: &mut App) {
    let vals = to_values(app);
    app.proj.0.retain(|k, _| !k.starts_with("Surround / "));
    for (k, v) in vals {
        app.proj.0.insert(k, v);
    }
}

/// Every strip routed to surround (for the windows' Link and the Atmos plug-in's lists).
pub fn surround_strips(app: &App) -> Vec<(usize, Out)> {
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let mut v: Vec<(usize, Out)> = (0..n).map(|t| (t, crate::mixstate::output_of(app, t))).filter(|x| matches!(x.1, Out::Surround | Out::Object)).collect();
    v.extend((0..app.auxes.len()).map(|a| (AUX + a, app.auxes[a].output)).filter(|x| matches!(x.1, Out::Surround | Out::Object)));
    v
}

/// The objects in Tracks-area order: (strip id, name, stereo, objects it uses).
pub fn object_list(app: &App) -> Vec<(usize, String, bool)> {
    surround_strips(app).into_iter().filter(|x| x.1 == Out::Object).map(|(s, _)| (s, crate::mixstate::strip_name(app, s), strip_stereo(app, s))).collect()
}

/// Objects in use (a stereo object counts two), of Dolby Atmos's 118.
pub fn objects_used(app: &App) -> usize {
    object_list(app).iter().map(|o| if o.2 { 2 } else { 1 }).sum()
}

pub const MAX_OBJECTS: usize = 118;

/// The other selected strips that get the same change (Output slot, 3D Object Panner menu).
pub fn chosen(app: &App, s: usize) -> BTreeSet<usize> {
    let mut v: BTreeSet<usize> = [s].into_iter().collect();
    if app.mix.selected.contains(&s) {
        v.extend(app.mix.selected.iter().copied().filter(|x| *x < crate::mixstate::VCA));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_through_the_settings() {
        let ctx = egui::Context::default();
        let mut app = App::new(&eframe::CreationContext::_new_kittest(ctx), None);
        app.sur.atmos = true;
        app.sur.format = Format::S712;
        let mut p = Pan::default();
        p.angle = 45.0;
        p.lfe_db = -6.0;
        app.sur.pans.insert(0, p);
        app.sur.objects.insert(1, Object { x: -0.5, y: 0.2, z: 0.7, size: 0.1, spread: 0.3 });
        app.sur.master = vec![crate::mixstate::new_plugin(ATMOS), crate::mixstate::new_plugin("Multichannel Gain")];
        app.sur.render.monitoring = sur::Monitoring::M514;
        app.sur.render.trims[2].manual = true;
        app.sur.render.trims[2].height_db = -4.0;
        app.sur.bin_bed.insert(Ch::C, BinMode::Near);
        app.sur.bin_obj.insert(1, BinMode::Far);
        app.mix.outputs.insert(1, Out::Object);
        let vals: BTreeMap<String, String> = to_values(&app).into_iter().collect();
        let (st, outs) = from_values(&vals);
        assert_eq!(st, app.sur);
        assert_eq!(outs, vec![(1, Out::Object)]);
    }

    fn engine_with(fmt: Format) -> EngineSurround {
        let mut s = EngineSurround::new();
        s.bed = fmt;
        s
    }

    #[test]
    fn panned_strips_fill_the_bed_and_the_master_meters_it() {
        let mut s = engine_with(Format::S51);
        let mut pan = Pan::default();
        pan.angle = 180.0;
        s.strips = vec![Some(StripPan::Panner { pan, stereo: false })];
        let n = 64;
        s.begin(n, 0.0, 1.0, false, &[]);
        s.add(0, &vec![0.5; n], &vec![0.5; n]);
        let mut out = vec![0f32; 2 * n];
        s.finish(&mut out, n, Default::default(), 1.0);
        assert_eq!(s.layout, Format::S51.channels().to_vec());
        assert!(s.meters[3] > 0.3 && s.meters[4] > 0.3, "behind: the surrounds {:?}", s.meters);
        assert!(s.meters[0] < 1e-6 && s.meters[1] < 1e-6);
        assert!(out.iter().any(|x| x.abs() > 0.1), "folded to the two-channel device");
    }

    #[test]
    fn atmos_renders_objects_and_bed_to_the_monitoring_format() {
        let mut s = engine_with(Format::S712);
        s.atmos = true;
        s.master = vec![(ATMOS.into(), Vec::new())];
        s.master_bypass = vec![false];
        s.render.monitoring = sur::Monitoring::M714;
        s.strips = vec![Some(StripPan::Object { obj: Object { x: 1.0, y: -1.0, z: 0.0, size: 0.0, spread: 0.0 }, stereo: false, mode: BinMode::Mid })];
        s.build(120.0);
        s.capture = Some(Capture { adm: true, ..Default::default() });
        let n = 64;
        s.begin(n, 0.0, 1.0, false, &[]);
        s.add(0, &vec![0.5; n], &vec![0.5; n]);
        let mut out = vec![0f32; 2 * n];
        s.finish(&mut out, n, Default::default(), 1.0);
        assert_eq!(s.layout.len(), 12);
        let rs = Format::S714.index(Ch::Rs).unwrap();
        assert!(s.meters[rs] > 0.4, "object at back right: Rs {:?}", s.meters);
        let cap = s.capture.take().unwrap();
        assert_eq!(cap.bed.len(), 10, "the 7.1.2 bed captured for ADM");
        assert_eq!(cap.objects.len(), 1);
        assert_eq!(cap.objects.values().next().unwrap().1.len(), n);
        assert_eq!(cap.out.len(), 12);
    }

    #[test]
    fn master_chain_runs_native_and_multi_mono() {
        let mut s = engine_with(Format::S51);
        s.master = vec![("Gain".into(), Vec::new()), ("Down Mixer".into(), Vec::new())];
        s.master_bypass = vec![false, false];
        s.strips = vec![Some(StripPan::Panner { pan: Pan::default(), stereo: true })];
        s.build(120.0);
        assert_eq!(s.units.inserts.len(), 2);
        let n = 64;
        s.begin(n, 0.0, 1.0, false, &[]);
        s.add(0, &vec![0.5; n], &vec![0.5; n]);
        let mut out = vec![0f32; 2 * n];
        s.finish(&mut out, n, Default::default(), 1.0);
        assert_eq!(s.layout, vec![Ch::L, Ch::R], "Down Mixer to stereo");
    }
}
