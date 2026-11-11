//! The Mixer's model beyond the tracks' own faders: groups (Logic's Group slot and Group
//! Settings, with linked volume / pan / mute / solo / selection / record / automation mode),
//! VCA faders, each strip's input and output, the Stereo Out's effects, plug-in bypass,
//! filters, the selected strips, and the Mixer's own undo history (Edit ▸ Mixer Undo). What a
//! project saves (lg_core::mixer: auxes, sends, outputs, groups) is read in here, and the whole
//! signal flow handed to the engine.
//!
//! Strip ids: a track's index; an aux AUX + its index; a VCA VCA + its index; the Stereo Out
//! MASTER.

use std::collections::{BTreeSet, HashMap, HashSet};

use lg_core::plugin::Plugin;
use lg_core::TrackKind;

use crate::app::{db_to_gain, App, AuxStrip, SendSlot};
use crate::audio::{self, Out};
use crate::mixer::AUX;

pub const VCA: usize = 2_000_000;
pub const MASTER: usize = usize::MAX;

/// Group link bits, as Logic saves them (format-analysis §25: MSeq bytes 0x46..0x49).
pub const L_VOLUME: (usize, u8) = (0, 0x01);
pub const L_PAN: (usize, u8) = (0, 0x02);
pub const L_MUTE: (usize, u8) = (0, 0x04);
pub const L_SOLO: (usize, u8) = (0, 0x08);
pub const L_SELECT: (usize, u8) = (2, 0x01);
pub const L_COLOR: (usize, u8) = (2, 0x04);
pub const L_RECORD: (usize, u8) = (2, 0x10);
pub const L_HIDE: (usize, u8) = (2, 0x20);
pub const L_AUTOMATION: (usize, u8) = (3, 0x01);
pub const L_INPUT: (usize, u8) = (3, 0x04);

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub name: String,
    /// Logic's Group Settings "Enabled" (a group can be kept but switched off).
    pub on: bool,
    pub mask: [u8; 4],
}

impl Group {
    pub fn new() -> Group {
        Group { name: String::new(), on: true, mask: lg_core::mixer::DEFAULT_GROUP_MASK }
    }

    pub fn links(&self, l: (usize, u8)) -> bool {
        self.mask[l.0] & l.1 != 0
    }

    pub fn set(&mut self, l: (usize, u8), on: bool) {
        if on {
            self.mask[l.0] |= l.1;
        } else {
            self.mask[l.0] &= !l.1;
        }
    }

    /// Send `k` (0-based, Logic links Sends 1-8) linked.
    pub fn links_send(&self, k: usize) -> bool {
        k < 8 && self.mask[1] & (1 << k) != 0
    }

    pub fn label(&self, n: usize) -> String {
        if self.name.is_empty() { format!("Group {}", n + 1) } else { format!("{}: {}", n + 1, self.name) }
    }
}

impl Default for Group {
    fn default() -> Self {
        Group::new()
    }
}

/// A VCA fader: moves the volume of the strips assigned to it, mutes and solos them.
#[derive(Clone, Debug, PartialEq)]
pub struct Vca {
    pub name: String,
    pub volume_db: f32,
    pub mute: bool,
    pub solo: bool,
}

/// An audio track's input (Logic's Input slot): none, an input (mono, from 0) or a stereo pair
/// (its first input), or a bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Input {
    #[default]
    Mono0,
    Mono(usize),
    Stereo(usize),
    Bus(usize),
    None,
}

/// What a strip is, for the filters (Logic's Audio / Inst / Aux / Bus / Input / Output /
/// Master/VCA / MIDI buttons) and Select … Channel Strips.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Audio,
    Inst,
    Aux,
    Bus,
    Input,
    Output,
    MasterVca,
    Midi,
}

/// Everything a mixer edit can change: one Mixer Undo step.
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    pub tracks: Vec<(f32, f32, bool, bool, Vec<Plugin>)>,
    pub auxes: Vec<AuxStrip>,
    pub sends: HashMap<usize, Vec<SendSlot>>,
    pub groups: Vec<Group>,
    pub group_of: HashMap<usize, usize>,
    pub vcas: Vec<Vca>,
    pub vca_of: HashMap<usize, usize>,
    pub outputs: HashMap<usize, Out>,
    pub inputs: HashMap<usize, Input>,
    pub master: Vec<Plugin>,
    pub eqs: HashMap<usize, crate::eq::Params>,
}

pub struct Mix {
    pub groups: Vec<Group>,
    /// Strip id → group index.
    pub group_of: HashMap<usize, usize>,
    /// Options ▸ Enable Groups (the group clutch).
    pub groups_enabled: bool,
    pub group_window: Option<usize>,
    pub vcas: Vec<Vca>,
    pub vca_of: HashMap<usize, usize>,
    pub outputs: HashMap<usize, Out>,
    pub inputs: HashMap<usize, Input>,
    /// The Stereo Out's Audio FX.
    pub master: Vec<Plugin>,
    /// The filter buttons: kinds hidden.
    pub hidden: HashSet<Kind>,
    pub narrow: bool,
    pub selected: BTreeSet<usize>,
    pub undo: Vec<(String, Snap)>,
    pub redo: Vec<(String, Snap)>,
    pub include_in_project_undo: bool,
    /// The effect window open: strip and insert (index in the strip's insert list).
    pub fx_window: Option<(usize, usize)>,
    pub compare: HashMap<(usize, usize), Vec<f32>>,
    pub compare_back: HashMap<(usize, usize), Vec<f32>>,
    pub clipboard: Option<(String, Vec<f32>)>,
    /// A plug-in slot being dragged (strip, insert), and this frame's slots (drop targets).
    pub slot_drag: Option<(usize, usize)>,
    pub slot_rects: Vec<(egui::Rect, usize, usize)>,
    /// Channel strip components hidden (View ▸ Channel Strip Components).
    pub hidden_parts: HashSet<String>,
    /// Plug-in Latency Compensation (Settings ▸ Audio ▸ General: All).
    pub compensate: bool,
    /// A setting name being typed (Save Setting As…).
    pub save_as: Option<String>,
    /// The Mixer's horizontal scroll (strips from the left).
    pub scroll: f32,
    /// Copy Group Settings; Copy Channel Strip Setting (effects, sends, EQ).
    pub group_clipboard: Option<[u8; 4]>,
    pub strip_clipboard: Option<(Vec<Plugin>, Vec<SendSlot>, Option<crate::eq::Params>)>,
    /// The plug-in windows Show/Hide All Plug-in Windows put away (effect, instrument, EQ).
    pub hidden_windows: (Option<(usize, usize)>, Option<usize>, Option<usize>),
    /// Mix ▸ Pre-Fader Metering.
    pub pre_fader_meters: bool,
}

impl Default for Mix {
    fn default() -> Self {
        Mix { groups: Vec::new(), group_of: HashMap::new(), groups_enabled: true, group_window: None, vcas: Vec::new(), vca_of: HashMap::new(), outputs: HashMap::new(), inputs: HashMap::new(), master: Vec::new(), hidden: HashSet::new(), narrow: false, selected: BTreeSet::new(), undo: Vec::new(), redo: Vec::new(), include_in_project_undo: true, fx_window: None, compare: HashMap::new(), compare_back: HashMap::new(), clipboard: None, slot_drag: None, slot_rects: Vec::new(), hidden_parts: HashSet::new(), compensate: true, save_as: None, scroll: 0.0, group_clipboard: None, strip_clipboard: None, hidden_windows: (None, None, None), pre_fader_meters: false }
    }
}

/// A new plug-in for a slot (its settings come from the stand-in's defaults when opened).
pub fn new_plugin(name: &str) -> Plugin {
    Plugin { name: name.to_string(), preset: "#default".into(), params: Vec::new(), channel: 0, slot: 3, bypass: false }
}

/// The insert list a track's engine strip runs: its Audio FX (slot 3 on, not its Channel EQ,
/// which runs as the strip's EQ) as indexes into Track::plugins.
pub fn track_inserts(t: &lg_core::Track) -> Vec<usize> {
    (0..t.plugins.len()).filter(|&k| t.plugins[k].slot >= 3 && t.plugins[k].name != "Channel EQ" && !t.plugins[k].name.is_empty()).collect()
}

/// A strip's Audio FX (the list the Mixer shows and the engine runs), by strip id.
pub fn inserts(app: &App, s: usize) -> Vec<Plugin> {
    if s == MASTER {
        return app.mix.master.clone();
    }
    if (AUX..VCA).contains(&s) {
        return app.auxes.get(s - AUX).map(|a| a.inserts.clone()).unwrap_or_default();
    }
    let Some(t) = app.project.as_ref().and_then(|p| p.tracks.get(s)) else { return Vec::new() };
    track_inserts(t).into_iter().map(|k| t.plugins[k].clone()).collect()
}

/// The insert list of a strip, to change in place (`f` gets the list; tracks keep their
/// instrument and EQ where they are).
pub fn edit_inserts(app: &mut App, s: usize, f: impl FnOnce(&mut Vec<Plugin>)) {
    if s == MASTER {
        f(&mut app.mix.master);
    } else if (AUX..VCA).contains(&s) {
        if let Some(a) = app.auxes.get_mut(s - AUX) {
            f(&mut a.inserts);
        }
    } else if let Some(t) = app.project.as_mut().and_then(|p| p.tracks.get_mut(s)) {
        let idx = track_inserts(t);
        let mut list: Vec<Plugin> = idx.iter().map(|&k| t.plugins[k].clone()).collect();
        f(&mut list);
        let mut rest: Vec<Plugin> = t.plugins.iter().enumerate().filter(|(k, _)| !idx.contains(k)).map(|(_, u)| u.clone()).collect();
        // the Audio FX after the instrument and the EQ, numbered from slot 3 on as Logic does
        let first = rest.iter().map(|u| u.slot + 1).max().unwrap_or(3).max(3);
        for (k, u) in list.iter_mut().enumerate() {
            u.slot = first + k as u32;
        }
        rest.extend(list);
        rest.sort_by_key(|u| u.slot);
        t.plugins = rest;
    }
    app.dirty = true;
}

/// Which kind of strip `s` is.
pub fn kind_of(app: &App, s: usize) -> Kind {
    if s == MASTER || s >= VCA {
        return Kind::MasterVca;
    }
    if s >= AUX {
        return Kind::Aux;
    }
    match app.project.as_ref().and_then(|p| p.tracks.get(s)).map(|t| t.kind) {
        Some(TrackKind::Audio) => Kind::Audio,
        _ => Kind::Inst,
    }
}

pub fn strip_name(app: &App, s: usize) -> String {
    if s == MASTER {
        return "Stereo Out".into();
    }
    if s >= VCA {
        return app.mix.vcas.get(s - VCA).map(|v| v.name.clone()).unwrap_or_default();
    }
    if s >= AUX {
        return app.auxes.get(s - AUX).map(|a| a.name.clone()).unwrap_or_default();
    }
    app.project.as_ref().and_then(|p| p.tracks.get(s)).map(|t| t.name.clone()).unwrap_or_default()
}

/// The strips the Mixer can show (tracks, auxes, VCAs, then the Stereo Out).
pub fn all_strips(app: &App) -> Vec<usize> {
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let mut v: Vec<usize> = (0..n).collect();
    v.extend((0..app.auxes.len()).map(|a| AUX + a));
    v.extend((0..app.mix.vcas.len()).map(|k| VCA + k));
    v.push(MASTER);
    v
}

pub fn snapshot(app: &App) -> Snap {
    let m = &app.mix;
    Snap {
        tracks: app.project.as_ref().map(|p| p.tracks.iter().map(|t| (t.volume_db, t.pan, t.mute, t.solo, t.plugins.clone())).collect()).unwrap_or_default(),
        auxes: app.auxes.clone(),
        sends: app.sends.clone(),
        groups: m.groups.clone(),
        group_of: m.group_of.clone(),
        vcas: m.vcas.clone(),
        vca_of: m.vca_of.clone(),
        outputs: m.outputs.clone(),
        inputs: m.inputs.clone(),
        master: m.master.clone(),
        eqs: app.eqs.clone(),
    }
}

fn restore(app: &mut App, s: Snap) {
    if let Some(p) = app.project.as_mut() {
        for (t, (v, pan, m, so, pl)) in p.tracks.iter_mut().zip(s.tracks) {
            t.volume_db = v;
            t.pan = pan;
            t.mute = m;
            t.solo = so;
            t.plugins = pl;
        }
    }
    app.auxes = s.auxes;
    app.sends = s.sends;
    app.eqs = s.eqs;
    let m = &mut app.mix;
    m.groups = s.groups;
    m.group_of = s.group_of;
    m.vcas = s.vcas;
    m.vca_of = s.vca_of;
    m.outputs = s.outputs;
    m.inputs = s.inputs;
    m.master = s.master;
    app.dirty = true;
    app.load_engine();
}

/// Before a mixer edit: the mixer as it is goes on the Mixer Undo list ("Volume", "Insert…").
pub fn checkpoint(app: &mut App, what: &str) {
    let s = snapshot(app);
    if app.mix.undo.last().is_some_and(|(_, last)| *last == s) {
        return;
    }
    app.mix.undo.push((what.to_string(), s));
    if app.mix.undo.len() > 200 {
        app.mix.undo.remove(0);
    }
    app.mix.redo.clear();
}

pub fn undo(app: &mut App) -> bool {
    let Some((what, s)) = app.mix.undo.pop() else { return false };
    let cur = snapshot(app);
    app.mix.redo.push((what.clone(), cur));
    restore(app, s);
    app.tell(format!("Mixer Undo {what}"));
    true
}

pub fn redo(app: &mut App) -> bool {
    let Some((what, s)) = app.mix.redo.pop() else { return false };
    let cur = snapshot(app);
    app.mix.undo.push((what.clone(), cur));
    restore(app, s);
    app.tell(format!("Mixer Redo {what}"));
    true
}

/// The group strip `s` is in, when groups are on and that group is enabled.
pub fn active_group(app: &App, s: usize) -> Option<usize> {
    let g = *app.mix.group_of.get(&s)?;
    (app.mix.groups_enabled && app.mix.groups.get(g).is_some_and(|x| x.on)).then_some(g)
}

/// The other strips in `s`'s group that follow `link`.
pub fn linked(app: &App, s: usize, link: (usize, u8)) -> Vec<usize> {
    let Some(g) = active_group(app, s) else { return Vec::new() };
    if !app.mix.groups[g].links(link) {
        return Vec::new();
    }
    let mut v: Vec<usize> = app.mix.group_of.iter().filter(|(k, x)| **x == g && **k != s).map(|(k, _)| *k).collect();
    v.sort();
    v
}

/// A strip's fader values: volume (dB), pan, mute, solo.
pub fn values(app: &App, s: usize) -> Option<(f32, f32, bool, bool)> {
    if s >= VCA && s != MASTER {
        return app.mix.vcas.get(s - VCA).map(|v| (v.volume_db, 0.0, v.mute, v.solo));
    }
    if (AUX..VCA).contains(&s) {
        return app.auxes.get(s - AUX).map(|a| (a.volume_db, a.pan, a.mute, a.solo));
    }
    app.project.as_ref()?.tracks.get(s).map(|t| (t.volume_db, t.pan, t.mute, t.solo))
}

pub fn set_values(app: &mut App, s: usize, v: (f32, f32, bool, bool)) {
    if s >= VCA && s != MASTER {
        if let Some(x) = app.mix.vcas.get_mut(s - VCA) {
            (x.volume_db, _, x.mute, x.solo) = (v.0, v.1, v.2, v.3);
        }
    } else if (AUX..VCA).contains(&s) {
        if let Some(x) = app.auxes.get_mut(s - AUX) {
            (x.volume_db, x.pan, x.mute, x.solo) = v;
        }
    } else if let Some(t) = app.project.as_mut().and_then(|p| p.tracks.get_mut(s)) {
        (t.volume_db, t.pan, t.mute, t.solo) = v;
    }
}

/// A strip's fader values changed from `old` to `new`: its group follows (volume and pan by
/// the same amount, mute and solo to the same state), as Logic's linked groups do.
pub fn follow_group(app: &mut App, s: usize, old: (f32, f32, bool, bool), new: (f32, f32, bool, bool)) {
    let mut changes: Vec<(usize, (f32, f32, bool, bool))> = Vec::new();
    let mut touch = |app: &App, link: (usize, u8), f: &dyn Fn(&mut (f32, f32, bool, bool))| {
        for o in linked(app, s, link) {
            let cur = changes.iter().find(|c| c.0 == o).map(|c| c.1).or_else(|| values(app, o));
            if let Some(mut v) = cur {
                f(&mut v);
                changes.retain(|c| c.0 != o);
                changes.push((o, v));
            }
        }
    };
    if new.0 != old.0 {
        let d = new.0 - old.0;
        touch(app, L_VOLUME, &|v| v.0 = (v.0 + d).clamp(-90.0, 6.0));
    }
    if new.1 != old.1 {
        let d = new.1 - old.1;
        touch(app, L_PAN, &|v| v.1 = (v.1 + d).clamp(-1.0, 1.0));
    }
    if new.2 != old.2 {
        touch(app, L_MUTE, &|v| v.2 = new.2);
    }
    if new.3 != old.3 {
        touch(app, L_SOLO, &|v| v.3 = new.3);
    }
    for (o, v) in changes {
        set_values(app, o, v);
    }
}

/// The selected strip's group members selected with it (Group setting: Editing (Selection)).
pub fn select(app: &mut App, s: usize, add: bool) {
    if !add {
        app.mix.selected.clear();
    }
    if add && app.mix.selected.contains(&s) {
        app.mix.selected.remove(&s);
        return;
    }
    app.mix.selected.insert(s);
    for o in linked(app, s, L_SELECT) {
        app.mix.selected.insert(o);
    }
    if s < AUX {
        app.selected_track = Some(s);
    }
}

/// A track's level through its VCA: (dB added, muted by the VCA).
pub fn vca_offset(app: &App, s: usize) -> (f32, bool) {
    match app.mix.vca_of.get(&s).and_then(|v| app.mix.vcas.get(*v)) {
        Some(v) => (if v.volume_db <= -89.0 { -200.0 } else { v.volume_db }, v.mute),
        None => (0.0, false),
    }
}

/// A new VCA for `members` (Options ▸ Create New VCA for Selected Channel Strips).
pub fn new_vca(app: &mut App, members: &[usize]) -> usize {
    checkpoint(app, "Create VCA");
    let n = app.mix.vcas.len();
    app.mix.vcas.push(Vca { name: format!("VCA {}", n + 1), volume_db: 0.0, mute: false, solo: false });
    for &m in members.iter().filter(|m| **m < VCA) {
        app.mix.vca_of.insert(m, n);
    }
    app.sync_mix();
    n
}

/// A new aux strip on the first bus nothing uses (Options ▸ Create New Auxiliary Channel Strip).
pub fn new_aux(app: &mut App, bus: Option<usize>) -> usize {
    let bus = bus.unwrap_or_else(|| (0..256).find(|b| !bus_used(app, *b)).unwrap_or(0));
    let n = app.auxes.len() + 1;
    app.auxes.push(AuxStrip { name: format!("Aux {n}"), bus, volume_db: 0.0, pan: 0.0, mute: false, solo: false, inserts: Vec::new(), output: Out::Stereo });
    app.dirty = true;
    app.auxes.len() - 1
}

/// A bus something reads or writes.
pub fn bus_used(app: &App, b: usize) -> bool {
    app.auxes.iter().any(|a| a.bus == b) || app.sends.values().flatten().any(|s| s.bus == b) || app.mix.outputs.values().any(|o| *o == Out::Bus(b))
}

/// Routes strip `s`'s output (an unused bus gets an aux reading it, as Logic makes one).
pub fn set_output(app: &mut App, s: usize, o: Out) {
    checkpoint(app, "Output");
    if let Out::Bus(b) = o {
        if !app.auxes.iter().any(|a| a.bus == b) {
            new_aux(app, Some(b));
        }
    }
    if (AUX..VCA).contains(&s) {
        if let Some(a) = app.auxes.get_mut(s - AUX) {
            a.output = o;
        }
    } else {
        app.mix.outputs.insert(s, o);
    }
    app.dirty = true;
    app.load_engine();
}

pub fn output_of(app: &App, s: usize) -> Out {
    if (AUX..VCA).contains(&s) {
        return app.auxes.get(s - AUX).map_or(Out::Stereo, |a| a.output);
    }
    app.mix.outputs.get(&s).copied().unwrap_or_default()
}

pub fn out_label(o: Out) -> String {
    match o {
        Out::Stereo => "Stereo Out".into(),
        Out::Bus(b) => format!("Bus {}", b + 1),
        Out::Off => "No Output".into(),
    }
}

pub fn in_label(i: Input) -> String {
    match i {
        Input::Mono0 => "Input 1".into(),
        Input::Mono(k) => format!("Input {}", k + 1),
        Input::Stereo(k) => format!("Input {}-{}", k + 1, k + 2),
        Input::Bus(b) => format!("Bus {}", b + 1),
        Input::None => "No Input".into(),
    }
}

/// Reads what the project saved: its aux strips (with their effects, inputs, outputs), sends,
/// track outputs and groups; clears what belonged to the last project.
pub fn from_project(app: &mut App) {
    let keep = (app.mix.hidden.clone(), app.mix.narrow, app.mix.hidden_parts.clone(), app.mix.compensate, app.mix.clipboard.clone());
    app.mix = Mix::default();
    (app.mix.hidden, app.mix.narrow, app.mix.hidden_parts, app.mix.compensate, app.mix.clipboard) = keep;
    app.auxes.clear();
    app.sends.clear();
    let Some(p) = app.project.as_ref() else { return };
    let m = p.mixer.clone();
    let mut aux_of: HashMap<usize, usize> = HashMap::new();
    for s in m.auxes() {
        let st = &m.strips[s];
        aux_of.insert(s, app.auxes.len());
        let inserts: Vec<Plugin> = st.plugins.iter().filter(|u| u.slot >= 3 && !u.name.is_empty()).cloned().collect();
        app.auxes.push(AuxStrip { name: st.name.clone(), bus: st.input_bus.unwrap_or(st.number as usize), volume_db: st.volume_db, pan: st.pan, mute: st.mute, solo: st.solo, inserts, output: st.output_bus.map_or(Out::Stereo, Out::Bus) });
    }
    let track_of = |s: usize| m.track_strip.iter().position(|x| *x == Some(s));
    for snd in &m.sends {
        let id = match (aux_of.get(&snd.from), track_of(snd.from)) {
            (Some(a), _) => AUX + a,
            (None, Some(t)) => t,
            _ => continue,
        };
        let list = app.sends.entry(id).or_default();
        if !list.iter().any(|x| x.bus == snd.bus) {
            list.push(SendSlot { bus: snd.bus, level_db: snd.level_db, pre: snd.pre });
        }
    }
    for (t, s) in m.track_strip.iter().enumerate() {
        if let Some(b) = s.and_then(|s| m.strips[s].output_bus) {
            app.mix.outputs.insert(t, Out::Bus(b));
        }
    }
    for g in &m.groups {
        app.mix.groups.push(Group { name: g.name.clone(), on: true, mask: g.mask });
    }
    for (t, g) in m.track_group.iter().enumerate() {
        if *g > 0 && (*g as usize) <= app.mix.groups.len() {
            app.mix.group_of.insert(t, *g as usize - 1);
        }
    }
}

/// The signal flow into the engine: the auxes (their sends, outputs, bypassed effects), each
/// track's output and bypassed effects, the Stereo Out's effects, VCAs, compensation.
pub fn to_engine(app: &App, e: &mut audio::Engine) {
    let any_solo = app.auxes.iter().any(|a| a.solo);
    e.auxes = app
        .auxes
        .iter()
        .enumerate()
        .map(|(k, a)| audio::AuxBus {
            bus: a.bus,
            volume: db_to_gain(a.volume_db + vca_offset(app, AUX + k).0),
            pan: a.pan,
            mute: a.mute || vca_offset(app, AUX + k).1 || (any_solo && !a.solo),
            inserts: a.inserts.iter().map(|u| (u.name.clone(), u.params.clone())).collect(),
            bypass: a.inserts.iter().map(|u| u.bypass).collect(),
            output: a.output,
            sends: app.sends.get(&(AUX + k)).map(|v| v.iter().map(|s| audio::Send { bus: s.bus, gain: db_to_gain(s.level_db), pre: s.pre }).collect()).unwrap_or_default(),
        })
        .collect();
    let Some(p) = app.project.as_ref() else { return };
    e.track_out = (0..p.tracks.len()).map(|t| app.mix.outputs.get(&t).copied().unwrap_or_default()).collect();
    e.track_bypass = p.tracks.iter().map(|t| track_inserts(t).into_iter().map(|k| t.plugins[k].bypass).collect()).collect();
    e.master_inserts = app.mix.master.iter().map(|u| (u.name.clone(), u.params.clone())).collect();
    e.master_bypass = app.mix.master.iter().map(|u| u.bypass).collect();
    e.compensate = app.mix.compensate;
    e.pre_fader_meters = app.mix.pre_fader_meters;
    apply_vcas(app, e);
}

/// The tracks' engine volume and mute through their VCAs (after the tracks' own were set).
pub fn apply_vcas(app: &App, e: &mut audio::Engine) {
    if app.mix.vca_of.is_empty() {
        return;
    }
    let Some(p) = app.project.as_ref() else { return };
    let vca_solo = app.mix.vcas.iter().any(|v| v.solo);
    for (i, (pt, t)) in e.tracks.iter_mut().zip(&p.tracks).enumerate() {
        let (db, mute) = vca_offset(app, i);
        pt.volume = db_to_gain(t.volume_db + db);
        pt.mute = t.mute || mute;
        if vca_solo && app.mix.vca_of.get(&i).and_then(|v| app.mix.vcas.get(*v)).is_some_and(|v| v.solo) {
            pt.solo = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_links_follow_logic_bits() {
        let mut g = Group::new();
        assert!(g.links(L_VOLUME) && g.links(L_MUTE) && !g.links(L_PAN) && !g.links(L_SOLO));
        assert!(g.links(L_AUTOMATION));
        g.set(L_PAN, true);
        g.set(L_SOLO, true);
        assert_eq!(g.mask[0], 0x0F);
        g.set(L_VOLUME, false);
        assert_eq!(g.mask[0], 0x0E);
        g.mask[1] = 0x81;
        assert!(g.links_send(0) && g.links_send(7) && !g.links_send(1));
    }

    fn app() -> App {
        let ctx = egui::Context::default();
        App::new(&eframe::CreationContext::_new_kittest(ctx), None)
    }

    fn sample(name: &str) -> Option<std::path::PathBuf> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata-private/sample-files").join(name);
        p.exists().then_some(p)
    }

    #[test]
    fn groups_link_faders_and_undo_restores() {
        let mut a = app();
        a.set_project(crate::demo::song(), true);
        a.mix.groups.push(Group::new());
        a.mix.group_of.insert(0, 0);
        a.mix.group_of.insert(1, 0);
        let old = values(&a, 0).unwrap();
        checkpoint(&mut a, "Volume");
        let new = (old.0 - 6.0, old.1 + 0.5, true, false);
        set_values(&mut a, 0, new);
        follow_group(&mut a, 0, old, new);
        let v1 = values(&a, 1).unwrap();
        assert!((v1.0 - (old.0 - 6.0)).abs() < 1e-4, "volume follows by the same amount");
        assert!(v1.2, "mute follows");
        assert_eq!(v1.1, 0.0, "pan isn't linked by default");
        assert_eq!(values(&a, 2).unwrap().0, old.0, "a track outside the group stays");
        // groups off: nothing follows
        a.mix.groups_enabled = false;
        assert!(linked(&a, 0, L_VOLUME).is_empty());
        // Mixer Undo puts both back
        assert!(undo(&mut a));
        assert_eq!(values(&a, 0).unwrap(), old);
        assert_eq!(values(&a, 1).unwrap().2, false);
        assert!(redo(&mut a));
        assert!(values(&a, 1).unwrap().2);
    }

    #[test]
    fn vca_moves_its_members() {
        let mut a = app();
        a.set_project(crate::demo::song(), true);
        let v = new_vca(&mut a, &[1]);
        a.mix.vcas[v].volume_db = -6.0;
        a.load_engine();
        let e = a.engine.lock().unwrap();
        let g0 = e.tracks[0].volume;
        let g1 = e.tracks[1].volume;
        assert!((crate::app::db_to_gain(-6.0) * g0 - g1).abs() < 1e-4, "{g0} {g1}");
    }

    #[test]
    fn plugins_move_copy_and_bypass() {
        let mut a = app();
        a.set_project(crate::demo::song(), true);
        edit_inserts(&mut a, 0, |v| v.extend([new_plugin("Compressor"), new_plugin("Limiter")]));
        crate::mixer::move_plugin(&mut a, 0, 1, 0, 0, false);
        assert_eq!(inserts(&a, 0).iter().map(|u| u.name.as_str()).collect::<Vec<_>>(), ["Limiter", "Compressor"]);
        let x = AUX + new_aux(&mut a, Some(3));
        crate::mixer::move_plugin(&mut a, 0, 0, x, 0, true);
        assert_eq!(inserts(&a, 0).len(), 2, "Option-drag copies");
        assert_eq!(inserts(&a, AUX).iter().map(|u| u.name.as_str()).collect::<Vec<_>>(), ["Limiter"]);
        edit_inserts(&mut a, 0, |v| v[0].bypass = true);
        a.load_engine();
        assert_eq!(a.engine.lock().unwrap().track_bypass[0], vec![true, false]);
    }

    #[test]
    fn a_project_brings_its_sends_auxes_and_groups() {
        let mut a = app();
        if let Some(p) = sample("E07-send-bus1.logicx") {
            a.open_path(&p);
            assert_eq!(a.auxes.len(), 1);
            assert_eq!(a.auxes[0].bus, 0);
            let sends: Vec<&SendSlot> = a.sends.values().flatten().collect();
            assert_eq!(sends.len(), 1);
            assert_eq!(sends[0].bus, 0);
            assert_eq!(a.engine.lock().unwrap().auxes.len(), 1);
        }
        if let Some(p) = sample("E24-groups.logicx") {
            a.open_path(&p);
            assert_eq!(a.mix.groups.len(), 2);
            assert!(a.mix.groups[0].links(L_PAN) && a.mix.groups[0].links(L_SOLO));
            let mut gs: Vec<usize> = a.mix.group_of.values().copied().collect();
            gs.sort();
            gs.dedup();
            // (E24's take folder reads as a second "Audio 1" track: both in Group 1)
            assert_eq!(gs, vec![0, 1]);
        }
        if let Some(p) = sample("E22-drum-machine-designer.logicx") {
            a.open_path(&p);
            // the kit's reverb and delay returns feed the kit's sum (Bus 3)
            assert!(a.auxes.iter().any(|x| x.bus == 3 && x.output == Out::Bus(2)));
            let n = a.project.as_ref().map_or(0, |p| p.tracks.len());
            assert!(a.sends.values().flatten().filter(|s| s.bus == 3).count() >= 20, "{n} tracks, sends {:?}, strips {:?}", a.sends, a.project.as_ref().map(|p| p.mixer.track_strip.clone()));
        }
    }
}
